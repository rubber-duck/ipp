//! One physical-connection account, retained through actual transport completion.

use ipp_core::ErrorReason;
use ipp_core::services::reliable_output::{
    OutputCharge, OutputClass, OutputLimits, ReliableOutputAccount, ReliableOutputLease,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// Bytes charged for every retained record in addition to its payload and encoding.
///
/// It covers the Host bookkeeping a record keeps until physical completion: its linked outbox
/// node, the shared reservation cell, correlation map entries for replies, the transport's
/// completion record (WASM delivery node or native in-flight lease) and allocator headers.
/// `record_overhead_covers_retained_bookkeeping` measures those structures on the build target
/// and requires at least twice their size: about 400 bytes on 64-bit targets, less on wasm32.
/// The margin covers allocator size classes and partially filled map nodes. Because every record
/// pays it, the byte budget alone bounds how many records a connection can retain: at most
/// `MAX_OUTPUT_BYTES / RESPONSE_METADATA_BYTES` (8192) even for empty payloads.
pub(crate) const RESPONSE_METADATA_BYTES: usize = 1024;

/// Bookkeeping for a transport's second copy of a record, such as the worker-side buffer.
const COPY_METADATA_BYTES: usize = 1024;

/// Reliable output one physical connection may retain until physical completion: payloads,
/// peak encodings, transport copies and per-record bookkeeping of replies, events, notices
/// and frame progress together. Eight maximum-size messages hold the reply reserve, a native
/// socket's two in-flight maximum-size buffers and a burst from many Worlds sharing the
/// connection, while bounding memory for a reader that stops draining. There is no separate
/// record-count limit.
pub(crate) const MAX_OUTPUT_BYTES: usize = 8 * ipp_protocol::MAX_MESSAGE_BYTES;

/// Part of [`MAX_OUTPUT_BYTES`] that only replies to admitted requests may use: one
/// maximum-size reply with its bookkeeping plus a simultaneous transport copy. Uncorrelated
/// output fails at the remaining share, so it can never leave an admitted request unanswerable.
pub(crate) const REPLY_RESERVE_BYTES: usize =
    2 * ipp_protocol::MAX_MESSAGE_BYTES + RESPONSE_METADATA_BYTES + COPY_METADATA_BYTES;

/// Bytes ordinary output may occupy before the reply reserve.
pub(crate) const ORDINARY_OUTPUT_BYTES: usize = MAX_OUTPUT_BYTES - REPLY_RESERVE_BYTES;

#[cfg(test)]
#[path = "reliable_output_tests.rs"]
mod tests;

#[derive(Clone)]
pub(crate) struct SharedReplyBudget(pub(crate) ReliableOutputAccount);

impl Default for SharedReplyBudget {
    fn default() -> Self {
        Self(ReliableOutputAccount::new(OutputLimits {
            bytes: MAX_OUTPUT_BYTES,
            reply_reserve: REPLY_RESERVE_BYTES,
        }))
    }
}

pub(crate) type SharedReplyReservation = Rc<RefCell<ReplyReservation>>;

pub(crate) struct ReplyReservation {
    lease: ReliableOutputLease,
    account: ReliableOutputAccount,
    pub(crate) bytes: usize,
    retained: usize,
    progress: Option<ProgressLease>,
}

struct ProgressLease {
    session: Rc<Cell<usize>>,
    connection: Rc<Cell<usize>>,
}

impl Drop for ProgressLease {
    fn drop(&mut self) {
        self.session.set(self.session.get() - 1);
        self.connection.set(self.connection.get() - 1);
    }
}

impl ReplyReservation {
    /// Reserve one record; admitted requests reserve [`OutputClass::Reply`] before acceptance.
    pub(crate) fn new(
        budget: SharedReplyBudget,
        class: OutputClass,
        bytes: usize,
    ) -> Result<Self, ErrorReason> {
        if bytes > ipp_protocol::MAX_MESSAGE_BYTES {
            return Err(ErrorReason::Capacity);
        }
        let lease = budget
            .0
            .reserve_as(
                class,
                OutputCharge {
                    entries: 1,
                    bytes: bytes
                        .checked_add(RESPONSE_METADATA_BYTES)
                        .ok_or(ErrorReason::Capacity)?,
                },
            )
            .map_err(|_| ErrorReason::Capacity)?;
        Ok(Self {
            lease,
            bytes,
            retained: 0,
            account: budget.0,
            progress: None,
        })
    }

    pub(crate) fn mark_progress(
        &mut self,
        session: &Rc<Cell<usize>>,
        connection: &Rc<Cell<usize>>,
    ) {
        session.set(session.get() + 1);
        connection.set(connection.get() + 1);
        self.progress = Some(ProgressLease {
            session: session.clone(),
            connection: connection.clone(),
        });
    }

    pub(crate) fn is_progress(&self) -> bool {
        self.progress.is_some()
    }

    pub(crate) fn grow(&mut self, additional: usize) -> Result<(), ErrorReason> {
        if !matches!(
            self.account.status(),
            ipp_core::services::reliable_output::OutputStatus::Open
        ) {
            return Err(ErrorReason::Capacity);
        }
        let bytes = self
            .bytes
            .checked_add(additional)
            .ok_or(ErrorReason::Capacity)?;
        if bytes > ipp_protocol::MAX_MESSAGE_BYTES {
            return Err(ErrorReason::Capacity);
        }
        self.lease
            .resize(OutputCharge {
                entries: 1,
                bytes: bytes + self.retained + RESPONSE_METADATA_BYTES,
            })
            .map_err(|_| ErrorReason::Capacity)?;
        self.bytes = bytes;
        Ok(())
    }

    pub(crate) fn reserve_bytes(&mut self, bytes: usize) -> Result<(), ErrorReason> {
        self.grow(bytes.saturating_sub(self.bytes))
    }

    pub(crate) fn reserve_retained(&mut self, bytes: usize) -> Result<(), ErrorReason> {
        let charge = self
            .bytes
            .checked_add(bytes)
            .and_then(|bytes| bytes.checked_add(RESPONSE_METADATA_BYTES))
            .ok_or(ErrorReason::Capacity)?;
        self.lease
            .resize(OutputCharge {
                entries: 1,
                bytes: charge,
            })
            .map_err(|_| ErrorReason::Capacity)?;
        self.retained = bytes;
        Ok(())
    }

    pub(crate) fn shrink(&mut self, bytes: usize) {
        assert!(bytes <= self.bytes);
        self.lease
            .resize(OutputCharge {
                entries: 1,
                bytes: bytes + self.retained + RESPONSE_METADATA_BYTES,
            })
            .expect("shrinking retained output credit");
        self.bytes = bytes;
    }

    pub(crate) fn encoded(&mut self, capacity: usize) {
        self.retained = 0;
        self.shrink(capacity);
    }

    pub(crate) fn into_lease(self) -> ReliableOutputLease {
        self.lease
    }

    pub(crate) fn from_observation(
        mut lease: ReliableOutputLease,
        account: ReliableOutputAccount,
        capacity: usize,
    ) -> Self {
        let charge = OutputCharge {
            entries: 1,
            bytes: capacity + RESPONSE_METADATA_BYTES,
        };
        assert!(charge.bytes <= lease.charge().bytes);
        lease
            .resize(charge)
            .expect("shrinking released observation payload");
        Self {
            lease,
            account,
            bytes: capacity,
            retained: 0,
            progress: None,
        }
    }
}

/// Owned wire bytes and their still-charged transport allocation. Not Clone or Send.
pub struct ReliableResponse {
    pub(crate) bytes: Vec<u8>,
    pub(crate) reservation: SharedReplyReservation,
}

impl ReliableResponse {
    /// Reserve a second transport-owned payload before copying these bytes.
    pub fn prepare_copy(self) -> Result<PreparedOutputCopy, (Self, ErrorReason)> {
        let copy = {
            let reservation = self.reservation.borrow();
            reservation.account.reserve_as(
                reservation.lease.class(),
                OutputCharge {
                    entries: 0,
                    bytes: self.bytes.len() + COPY_METADATA_BYTES,
                },
            )
        };
        match copy {
            Ok(copy) => {
                let (bytes, source) = self.into_parts();
                Ok(PreparedOutputCopy {
                    bytes,
                    source,
                    _copy: copy,
                })
            }
            Err(_) => Err((self, ErrorReason::Capacity)),
        }
    }

    /// Restore an unsent bounded handoff without relinquishing its original credit.
    pub fn from_parts(bytes: Vec<u8>, lease: ResponseLease) -> Self {
        assert!(bytes.capacity() <= lease.capacity());
        Self {
            bytes,
            reservation: lease.0,
        }
    }

    /// Separate wire bytes for a bounded socket handoff. Keep the lease on the Host
    /// thread until the socket confirms flush completion or destroys the payload.
    pub fn into_parts(self) -> (Vec<u8>, ResponseLease) {
        (self.bytes, ResponseLease(self.reservation))
    }
}

/// Source bytes and independent copy credit, retained until transport completion.
/// Not Clone or Send; the owning adapter keeps this on the Host thread.
pub struct PreparedOutputCopy {
    bytes: Vec<u8>,
    source: ResponseLease,
    _copy: ReliableOutputLease,
}

impl PreparedOutputCopy {
    /// Read-only source; credit for the destination is already reserved.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Destroy the source allocation without completing the external delivery.
    pub fn release_source(&mut self) {
        self.bytes = Vec::new();
        self.source.0.borrow_mut().encoded(0);
    }
}

impl std::ops::Deref for ReliableResponse {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

/// Host-thread completion credit; dropping it acknowledges final payload release.
pub struct ResponseLease(SharedReplyReservation);

impl ResponseLease {
    /// Retained encoded capacity, excluding bounded adapter metadata.
    pub fn capacity(&self) -> usize {
        self.0.borrow().bytes
    }
}
