//! Host-first connections, discovery and attachment lifetime.

use crate::ReliableResponse;
use crate::attachment_receipts::{ReplyReservation, SharedReplyReservation};
use crate::{Host, HostServices, WorldSession};
use ipp_core::services::reliable_output::OutputClass;
use ipp_core::{WorldDescriptor, WorldId};
use ipp_protocol::host::{self, HostRequest, HostRequestBody, HostResponse, HostResponseBody};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::{cell::RefCell, rc::Rc};

pub(crate) mod scope;

#[cfg(feature = "instrumentation")]
mod asset_export_testing;
mod asset_exports;
pub(crate) mod asset_sources;
#[cfg(feature = "instrumentation")]
mod bulk_read_testing;
mod bulk_reads;
pub use asset_exports::PublicAssetSourceId;
mod datasets;
mod messages;
mod persistence;

pub use messages::HostConnectionMessage;

/// Process-owned connection and attachment routing, independent of World instances.
#[derive(Default)]
pub(crate) struct HostConnectionService {
    pub(crate) states: BTreeMap<u64, HostConnectionState>,
    pub(crate) now: std::time::Duration,
    last_persistence_connection: u64,
    persistence: persistence::HostPersistenceService,
    bulk: super::bulk_read::BulkReadService,
    exports: asset_exports::AssetExportService,
    #[cfg(feature = "instrumentation")]
    asset_export_policy: Option<PublicAssetSourceId>,
}

pub(crate) struct HostConnectionState {
    pub(crate) id: u64,
    ready: bool,
    /// The Host withholds this connection's input until admission recovers.
    pub(crate) throttled: bool,
    sessions: BTreeSet<u64>,
    /// Sessions this connection held that detach or World destruction ended. The
    /// client may still send on one until it reads that end; such messages are stale.
    ended_sessions: BTreeSet<u64>,
    last_output_session: u64,
    last_progress_session: u64,
    progress_turn: bool,
    temporary_worlds: BTreeSet<WorldId>,
    pending: VecDeque<HostConnectionIngress>,
    outbox: crate::outbox::SessionOutbox,
    reply_reservations: BTreeMap<u64, SharedReplyReservation>,
    pub(crate) failure: Option<String>,
    transfer: Option<persistence::HostWorldTransfer>,
    bulk_reads: BTreeMap<u64, super::bulk_read::BulkReadLease>,
    #[cfg(feature = "instrumentation")]
    bulk_test_inputs: BTreeMap<u64, ipp_core::services::io::IoStreamInput>,
    reply_budget: crate::attachment_receipts::SharedReplyBudget,
    progress_leases: Rc<std::cell::Cell<usize>>,
    presentation_pending: usize,
    datasets: datasets::DatasetConnection,
    pub(crate) batches: crate::command_batches::HostBatchBuilders,
}

enum HostConnectionIngress {
    Control {
        request: HostRequest,
        received_at: std::time::Duration,
    },
    DecodedWorld {
        request: ipp_protocol::Request,
        reservation: Option<SharedReplyReservation>,
        /// Buffered page bytes of an assembled batch, released when it leaves the session queue.
        lease: Option<crate::command_batches::BatchBytesLease>,
        /// First failure of an assembled batch, answered on its final page.
        rejection: Option<String>,
    },
}

impl HostConnectionState {
    fn pending_requests(&self, sessions: &BTreeMap<u64, WorldSession>) -> usize {
        self.pending.len()
            + self.presentation_pending
            + self
                .sessions
                .iter()
                .filter_map(|id| sessions.get(id))
                .map(|session| session.pending.len())
                .sum::<usize>()
    }

    /// Whether a World message naming `session` is live work of this connection.
    ///
    /// A session the connection held and has ended answers `Ok(false)`: the message
    /// was sent before the client read that end, and the end already settled the
    /// session's work, so it is fenced without a reply and the connection stays
    /// usable. A session the connection never held fails the connection.
    pub(crate) fn holds_session(&self, session: u64) -> Result<bool, String> {
        if self.sessions.contains(&session) {
            Ok(true)
        } else if self.ended_sessions.contains(&session) {
            Ok(false)
        } else {
            Err("SessionMismatch".into())
        }
    }

    /// End a session of this connection; its later messages are stale.
    fn end_session(&mut self, session: u64) {
        if self.sessions.remove(&session) {
            self.ended_sessions.insert(session);
        }
        self.batches.release_session(session);
    }

    /// Client-controlled request count: queued requests, or admitted correlated requests whose
    /// replies are not yet physically complete, whichever is larger. Uncorrelated output is
    /// bounded by bytes instead and never counts here.
    pub(crate) fn admitted_requests(&self, sessions: &BTreeMap<u64, WorldSession>) -> usize {
        self.pending_requests(sessions)
            .max(self.reply_budget.0.reply_usage().entries)
    }

    /// Correlated replies admitted and not yet physically delivered.
    #[cfg(test)]
    pub(crate) fn reply_entries(&self) -> usize {
        self.reply_budget.0.reply_usage().entries
    }

    /// Admit one correlated request's reply before accepting the request itself.
    pub(crate) fn reserve_reply(&self, bytes: usize) -> Result<SharedReplyReservation, String> {
        ReplyReservation::new(self.reply_budget.clone(), OutputClass::Reply, bytes)
            .map(|reservation| Rc::new(RefCell::new(reservation)))
            .map_err(|error| {
                let usage = self.reply_budget.0.usage();
                format!(
                    "connection congestion: reply capacity exhausted: {error} entries={} bytes={}",
                    usage.entries, usage.bytes
                )
            })
    }

    fn reply(&mut self, request_id: u64, body: HostResponseBody) -> Result<(), String> {
        let response = HostResponse {
            connection: self.id,
            request_id,
            body,
        };
        let mut response = response;
        let size = host::encoded_host_response_size(&response)
            .or_else(|error| {
                response.body =
                    HostResponseBody::Error(format!("Host response unavailable: {error}"));
                host::encoded_host_response_size(&response)
            })
            .map_err(|error| error.to_string())?;
        let reservation = if request_id == 0 {
            Rc::new(RefCell::new(
                ReplyReservation::new(self.reply_budget.clone(), OutputClass::Ordinary, size)
                    .map_err(|error| {
                        format!("connection congestion: Host notice capacity exhausted: {error}")
                    })?,
            ))
        } else {
            self.reply_reservations
                .remove(&request_id)
                .ok_or("Host reply has no reserved output")?
        };
        reservation
            .borrow_mut()
            .reserve_bytes(size)
            .map_err(|error| error.to_string())?;
        let bytes = host::encode_host_response(&response).map_err(|error| error.to_string())?;
        reservation.borrow_mut().encoded(bytes.capacity());
        self.outbox.push_back(ReliableResponse {
            bytes,
            reservation,
        });
        Ok(())
    }
}

#[cfg(test)]
mod connection_tests;

mod service;
