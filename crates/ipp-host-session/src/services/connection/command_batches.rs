//! Client-identified batch pages, assembled per connection and applied whole.
//!
//! A client submits a logical batch as one or more pages under a batch identity
//! it keeps unique among its connection's open batches. Pages arrive in the
//! connection's ingress order and append to that connection's builder for the
//! identity. Nothing reaches the World before the final page: the final page
//! carries the assembled commands into the session's ordinary request queue,
//! where the batch applies as one ordered batch at the next mutation boundary.
//!
//! A batch is one correlated request for admission. Its first page reserves the
//! reply of its final page and counts once against [`MAX_PENDING`]; earlier pages
//! carry no request identity and produce no output.
//!
//! Pages arrive decoded: the transport decodes each page when it arrives, on its
//! own thread where the host has one, so decoding overlaps the client encoding
//! and sending later pages and only application remains at the final page. A
//! builder retains decoded commands, and the connection is charged what they keep
//! alive: command slots at the builder's capacity plus every owned payload, until
//! the assembled batch leaves the session queue. A page that cannot be decoded,
//! an exhausted budget or a session mismatch drops the buffered commands and
//! keeps a payload-free failed marker, so later pages of the same identity are
//! discarded instead of starting a new batch; the final page is then rejected
//! with the first failure and nothing of the batch applies.
//!
//! The progress deadline starts when a page is accepted into the builder, at the
//! Host's clock when that page arrives, and restarts with every later page. It
//! measures only time in which the Host reads the connection: while the Host
//! throttles a connection, for example behind earlier input of another branch
//! on the same connection, its next pages wait in the transport, so every open
//! batch of that connection keeps restarting its deadline until the Host resumes
//! reading. An open batch that receives no page for [`BATCH_DEADLINE`] of read
//! time drops its buffered
//! commands and notifies its session with [`ResponseBody::BatchAborted`]. It
//! keeps its failed marker and reply reservation until its final page arrives or
//! the connection closes, because pages already in flight must never assemble
//! into a new, partial batch.

use std::{cell::Cell, collections::BTreeMap, rc::Rc, time::Duration};

use crate::reliable_output::SharedReplyReservation;
use crate::*;

#[cfg(test)]
#[path = "command_batches_tests.rs"]
mod tests;

/// Longest Host-clock interval between two accepted pages of one open batch,
/// counted while the Host reads the batch's connection.
pub(crate) const BATCH_DEADLINE: Duration = Duration::from_secs(2);

/// Decoded bytes one connection may retain in open or assembled multi-page batches.
///
/// Decoded commands retain about seven times their encoding: a command slot of
/// at most 128 bytes plus owned payloads, measured at about 240 bytes per plain
/// creation, 300 per GUI row command and 570 per Blender scene command. The
/// previous 8 MiB of encoded pages therefore held some 60 MB of decoded commands.
/// 32 MiB keeps the maintained Blender stress import (40,064 commands, 23 MB)
/// within one batch while bounding eight native connections to 256 MiB. It holds
/// roughly 58,000 Blender, 112,000 GUI or 140,000 plain commands, that is 57 to
/// 136 full pages; a batch that needs more fails loudly.
pub(crate) const MAX_BUFFERED_BATCH_BYTES: usize = 32 * 1024 * 1024;

/// Buffered page bytes charged to one connection until an assembled batch leaves
/// its session queue or is dropped.
pub(crate) struct BatchBytesLease {
    charged: Rc<Cell<usize>>,
    bytes: usize,
}

impl BatchBytesLease {
    /// Charge `bytes` for this batch in place of its previous charge.
    fn charge(&mut self, bytes: usize) -> Result<(), String> {
        let others = self.charged.get() - self.bytes;
        if bytes > MAX_BUFFERED_BATCH_BYTES - others {
            return Err(format!(
                "Batch pages exceed the connection's buffered batch budget of {MAX_BUFFERED_BATCH_BYTES} decoded bytes"
            ));
        }
        self.charged.set(others + bytes);
        self.bytes = bytes;
        Ok(())
    }

    fn clear(&mut self) {
        self.charged.set(self.charged.get() - self.bytes);
        self.bytes = 0;
    }
}

impl Drop for BatchBytesLease {
    fn drop(&mut self) {
        self.clear();
    }
}

/// One open batch of a connection, or the failed marker that replaces it.
struct HostBatchBuilder {
    session: u64,
    progress: Duration,
    /// Pages received, including rejected ones, to name a failing page.
    pages: u32,
    /// Decoded pages in arrival order, each at its exact capacity.
    pages_received: Vec<Vec<ipp_core::Command>>,
    /// Bytes charged for `pages_received`: slots, owned payloads and page headers.
    retained: usize,
    /// Aliases the kept pages define, bounded by what one outcome reports.
    aliases: usize,
    lease: BatchBytesLease,
    reservation: SharedReplyReservation,
    failure: Option<String>,
    notified: bool,
}

impl HostBatchBuilder {
    fn fail(&mut self, failure: String) {
        self.pages_received = Vec::new();
        self.retained = 0;
        self.lease.clear();
        self.failure.get_or_insert(failure);
    }

    /// Keep a decoded page, charging its retained size before keeping it.
    ///
    /// Pages stay separate until the final page, so the charge is exact rather
    /// than a growing vector's spare capacity. A page decoded into a larger
    /// recycled buffer moves into exact storage and returns the buffer for reuse.
    fn keep(
        &mut self,
        mut page: Vec<ipp_core::Command>,
        heap_bytes: usize,
        aliases: usize,
    ) -> Result<Option<Vec<ipp_core::Command>>, String> {
        let aliases = self.aliases.saturating_add(aliases);
        if aliases > ipp_protocol::BATCH_OUTCOME_ALIASES {
            return Err(format!(
                "Batch defines more than {} aliases, which one batch outcome cannot report",
                ipp_protocol::BATCH_OUTCOME_ALIASES
            ));
        }
        let slots = std::mem::size_of::<ipp_core::Command>();
        let exact = page.capacity() > page.len().max(1) * 2;
        let capacity = if exact {
            page.len()
        } else {
            page.capacity()
        };
        let retained = self.retained.saturating_add(
            capacity
                .saturating_mul(slots)
                .saturating_add(heap_bytes)
                .saturating_add(std::mem::size_of::<Vec<ipp_core::Command>>()),
        );
        self.lease.charge(retained)?;
        self.retained = retained;
        self.aliases = aliases;
        if !exact {
            self.pages_received.push(page);
            return Ok(None);
        }
        let mut kept = Vec::with_capacity(capacity);
        kept.append(&mut page);
        self.pages_received.push(kept);
        Ok(Some(page))
    }

    /// Join the kept pages into one ordered command list, returning emptied page
    /// buffers. The joined list never retains more than the pages it replaces.
    fn assemble(&mut self) -> (Vec<ipp_core::Command>, Vec<Vec<ipp_core::Command>>) {
        let mut pages = std::mem::take(&mut self.pages_received).into_iter();
        let Some(mut operations) = pages.next() else {
            return (Vec::new(), Vec::new());
        };
        let total = operations.len() + pages.as_slice().iter().map(Vec::len).sum::<usize>();
        operations.reserve_exact(total - operations.len());
        let emptied = pages
            .map(|mut page| {
                operations.append(&mut page);
                page
            })
            .collect();
        (operations, emptied)
    }
}

/// One batch page as it arrives: decoded commands, or the page's decoding error.
pub(crate) struct BatchPageArrival {
    pub(crate) session: u64,
    pub(crate) request_id: u64,
    pub(crate) batch_id: u32,
    pub(crate) last: bool,
    pub(crate) operations: Result<Vec<ipp_core::Command>, ipp_protocol::ProtocolError>,
    /// Owned payload bytes of the decoded commands, beyond their slots.
    pub(crate) heap_bytes: usize,
    /// Commands of the page that define an alias its batch outcome reports.
    pub(crate) aliases: usize,
}

impl BatchPageArrival {
    /// The page's request, carrying `operations`, and its decoded content.
    fn split(
        self,
    ) -> (
        Request,
        Result<Vec<ipp_core::Command>, ipp_protocol::ProtocolError>,
    ) {
        let request = Request {
            session: self.session,
            request_id: self.request_id,
            body: RequestBody::SubmitBatch(ipp_protocol::world::BatchPage {
                batch_id: self.batch_id,
                last: self.last,
                operations: Vec::new(),
            }),
        };
        (request, self.operations)
    }
}

/// Place assembled commands into a batch page request.
fn with_operations(mut request: Request, operations: Vec<ipp_core::Command>) -> Request {
    let RequestBody::SubmitBatch(page) = &mut request.body else {
        unreachable!("batch page request");
    };
    page.operations = operations;
    request
}

fn undecodable(batch_id: u32, page: u32, error: &ipp_protocol::ProtocolError) -> String {
    format!("Batch {batch_id} page {page} could not be decoded: {error}")
}

/// A connection's open batches, keyed by client-assigned identity.
#[derive(Default)]
pub(crate) struct HostBatchBuilders {
    open: BTreeMap<u32, HostBatchBuilder>,
    charged: Rc<Cell<usize>>,
}

impl HostBatchBuilders {
    /// Drop every open batch of a departing session; its pages can no longer be answered.
    pub(crate) fn release_session(&mut self, session: u64) {
        self.open.retain(|_, builder| builder.session != session);
    }

    #[cfg(test)]
    pub(crate) fn buffered_bytes(&self) -> usize {
        self.charged.get()
    }

    #[cfg(test)]
    pub(crate) fn open_len(&self) -> usize {
        self.open.len()
    }
}

/// A batch completed by its final page, ready for the session queue.
pub(crate) struct AssembledBatch {
    pub(crate) request: Request,
    pub(crate) reservation: Option<SharedReplyReservation>,
    /// Buffered page bytes still charged to the connection.
    pub(crate) lease: Option<BatchBytesLease>,
    /// First failure of the batch, answered on its final page instead of applying it.
    pub(crate) rejection: Option<String>,
}

impl<P: HostServices> Host<P> {
    /// Assemble one decoded batch page at its arrival in connection ingress order.
    ///
    /// Returns the assembled batch once its final page arrives. Only a page that
    /// opens a batch is subject to request admission; later pages belong to an
    /// already admitted request and are bounded by the buffered batch budget.
    pub(crate) fn receive_batch_page(
        &mut self,
        connection: u64,
        page: BatchPageArrival,
    ) -> Result<Option<AssembledBatch>, String> {
        let now = self.connections.now;
        let world = self
            .sessions
            .get(&page.session)
            .map(|session| session.world);
        let admitted = self
            .connections
            .states
            .get(&connection)
            .ok_or("Connection is closed")?
            .admitted_requests(&self.sessions);
        let state = self
            .connections
            .states
            .get_mut(&connection)
            .ok_or("Connection is closed")?;
        let batch_id = page.batch_id;
        let mut recycled = None;
        let Some(builder) = state.batches.open.get_mut(&batch_id) else {
            if admitted >= MAX_PENDING {
                // The connection fails through the ordinary admission path, which
                // rejects this batch's final page with the connection failure.
                return Err(format!(
                    "connection congestion: Host ingress capacity exhausted: {admitted} of {MAX_PENDING} pending requests admitted, {} held by open or failed batches awaiting their final page",
                    state.batches.open.len()
                ));
            }
            let reservation = state.reserve_reply(256)?;
            let (session, last, heap_bytes, aliases) =
                (page.session, page.last, page.heap_bytes, page.aliases);
            let (request, operations) = page.split();
            if last {
                return Ok(Some(match operations {
                    Ok(operations) => AssembledBatch {
                        request: with_operations(request, operations),
                        reservation: Some(reservation),
                        lease: None,
                        rejection: None,
                    },
                    Err(error) => AssembledBatch {
                        request,
                        reservation: Some(reservation),
                        lease: None,
                        rejection: Some(undecodable(batch_id, 1, &error)),
                    },
                }));
            }
            let mut builder = HostBatchBuilder {
                session,
                progress: now,
                pages: 1,
                pages_received: Vec::new(),
                retained: 0,
                aliases: 0,
                lease: BatchBytesLease {
                    charged: state.batches.charged.clone(),
                    bytes: 0,
                },
                reservation,
                failure: None,
                notified: false,
            };
            match operations {
                Ok(operations) => match builder.keep(operations, heap_bytes, aliases) {
                    Ok(buffer) => recycled = buffer,
                    Err(failure) => builder.fail(failure),
                },
                Err(error) => builder.fail(undecodable(batch_id, 1, &error)),
            }
            state.batches.open.insert(batch_id, builder);
            self.recycle_page_buffer(world, recycled);
            return Ok(None);
        };

        builder.progress = now;
        builder.pages = builder.pages.saturating_add(1);
        let (session, last, heap_bytes, aliases) =
            (page.session, page.last, page.heap_bytes, page.aliases);
        let (request, operations) = page.split();
        if builder.failure.is_none() {
            if builder.session != session {
                builder.fail(format!(
                    "Batch {batch_id} pages target different World sessions"
                ));
            } else {
                match operations {
                    Ok(operations) => match builder.keep(operations, heap_bytes, aliases) {
                        Ok(buffer) => recycled = buffer,
                        Err(failure) => builder.fail(failure),
                    },
                    Err(error) => builder.fail(undecodable(batch_id, builder.pages, &error)),
                }
            }
        }
        self.recycle_page_buffer(world, recycled);
        if !last {
            return Ok(None);
        }

        let mut builder = self
            .connections
            .states
            .get_mut(&connection)
            .expect("receiving connection")
            .batches
            .open
            .remove(&batch_id)
            .expect("open batch builder");
        let (operations, emptied) = builder.assemble();
        for buffer in emptied {
            self.recycle_page_buffer(world, Some(buffer));
        }
        let reservation = Some(builder.reservation);
        Ok(Some(match builder.failure {
            Some(failure) => AssembledBatch {
                request,
                reservation,
                lease: None,
                rejection: Some(failure),
            },
            None => AssembledBatch {
                request: with_operations(request, operations),
                reservation,
                lease: Some(builder.lease),
                rejection: None,
            },
        }))
    }

    /// Return an emptied page buffer to its World's bounded decode pool.
    fn recycle_page_buffer(
        &mut self,
        world: Option<ipp_core::WorldId>,
        buffer: Option<Vec<ipp_core::Command>>,
    ) {
        if let Some(buffer) = buffer
            && let Some(mut world) = world.and_then(|world| self.runtime.world_mut(world))
        {
            world.recycle_command_buffer(buffer);
        }
    }

    /// Fail open batches whose next page did not arrive within the progress deadline.
    ///
    /// A throttled connection's pages wait in its transport, not in its client,
    /// so its open batches restart their deadline instead of consuming it.
    pub(crate) fn expire_command_batches(&mut self) {
        let now = self.connections.now;
        let mut notices = Vec::new();
        for state in self.connections.states.values_mut() {
            let withheld = state.throttled;
            for (&batch_id, builder) in &mut state.batches.open {
                if withheld {
                    builder.progress = now;
                    continue;
                }
                if builder.notified || now.saturating_sub(builder.progress) < BATCH_DEADLINE {
                    continue;
                }
                builder.fail(format!(
                    "Batch {batch_id} received no page for {} ms",
                    BATCH_DEADLINE.as_millis()
                ));
                builder.notified = true;
                notices.push((
                    state.id,
                    builder.session,
                    batch_id,
                    builder.failure.clone().expect("failed marker"),
                ));
            }
        }
        for (connection, session, batch_id, message) in notices {
            let Some(mut context) = self.session_mut(session) else {
                continue;
            };
            let delivered = context.queue_response(
                0,
                ResponseBody::BatchAborted {
                    batch_id: batch_id.into(),
                    message,
                },
            );
            drop(context);
            if let Err(error) = delivered
                && let Some(state) = self.connections.states.get_mut(&connection)
            {
                state.failure.get_or_insert(error);
            }
        }
    }
}
