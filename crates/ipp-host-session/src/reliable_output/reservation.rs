//! Reply reservations and the reliable responses that carry them to transport completion.

use super::{COPY_METADATA_BYTES, RESPONSE_METADATA_BYTES, SharedReplyBudget};
use ipp_core::ErrorReason;
use ipp_core::services::reliable_output::{
    OutputCharge, OutputClass, ReliableOutputAccount, ReliableOutputLease,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[cfg(test)]
#[path = "reservation_tests.rs"]
mod tests;

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

/// Wire storage. Routine frame progress fits inline; variable output owns its allocation.
#[derive(Debug)]
pub enum ResponseBytes {
    /// Variable-length reliable output.
    Owned(Vec<u8>),
    /// Fixed-size World progress.
    Frame([u8; 33]),
}

impl ResponseBytes {
    /// Capacity retained until the transport finishes the message.
    pub fn capacity(&self) -> usize {
        match self {
            Self::Owned(bytes) => bytes.capacity(),
            Self::Frame(bytes) => bytes.len(),
        }
    }
}

impl From<Vec<u8>> for ResponseBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Owned(bytes)
    }
}

impl std::ops::Deref for ResponseBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Frame(bytes) => bytes,
        }
    }
}

pub(crate) enum ResponseReservation {
    Shared(SharedReplyReservation),
    Frame(ReplyReservation),
}

impl From<SharedReplyReservation> for ResponseReservation {
    fn from(value: SharedReplyReservation) -> Self {
        Self::Shared(value)
    }
}

pub(crate) enum ReservationRead<'a> {
    Shared(std::cell::Ref<'a, ReplyReservation>),
    Frame(&'a ReplyReservation),
}

impl std::ops::Deref for ReservationRead<'_> {
    type Target = ReplyReservation;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Shared(value) => value,
            Self::Frame(value) => value,
        }
    }
}

pub(crate) enum ReservationWrite<'a> {
    Shared(std::cell::RefMut<'a, ReplyReservation>),
    Frame(&'a mut ReplyReservation),
}

impl std::ops::Deref for ReservationWrite<'_> {
    type Target = ReplyReservation;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Shared(value) => value,
            Self::Frame(value) => value,
        }
    }
}

impl std::ops::DerefMut for ReservationWrite<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Shared(value) => value,
            Self::Frame(value) => value,
        }
    }
}

impl ResponseReservation {
    pub(crate) fn borrow(&self) -> ReservationRead<'_> {
        match self {
            Self::Shared(value) => ReservationRead::Shared(value.borrow()),
            Self::Frame(value) => ReservationRead::Frame(value),
        }
    }

    fn borrow_mut(&mut self) -> ReservationWrite<'_> {
        match self {
            Self::Shared(value) => ReservationWrite::Shared(value.borrow_mut()),
            Self::Frame(value) => ReservationWrite::Frame(value),
        }
    }
}

/// Owned wire bytes and their still-charged transport allocation. Not Clone or Send.
pub struct ReliableResponse {
    pub(crate) bytes: ResponseBytes,
    pub(crate) reservation: ResponseReservation,
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
    pub fn from_parts(bytes: ResponseBytes, lease: ResponseLease) -> Self {
        assert!(bytes.capacity() <= lease.capacity());
        Self {
            bytes,
            reservation: lease.0,
        }
    }

    /// Separate wire bytes for a bounded socket handoff. Keep the lease on the Host
    /// thread until the socket confirms flush completion or destroys the payload.
    pub fn into_parts(self) -> (ResponseBytes, ResponseLease) {
        (self.bytes, ResponseLease(self.reservation))
    }
}

/// Source bytes and independent copy credit, retained until transport completion.
/// Not Clone or Send; the owning adapter keeps this on the Host thread.
pub struct PreparedOutputCopy {
    bytes: ResponseBytes,
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
        self.bytes = Vec::new().into();
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
pub struct ResponseLease(ResponseReservation);

impl ResponseLease {
    /// Retained encoded capacity, excluding bounded adapter metadata.
    pub fn capacity(&self) -> usize {
        self.0.borrow().bytes
    }
}
