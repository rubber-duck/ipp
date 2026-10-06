//! Host-first connections, discovery and attachment lifetime.

use crate::ReliableResponse;
use crate::reliable_output::{ReplyReservation, SharedReplyReservation};
use crate::{Host, HostServices, WorldSession};
use ipp_core::services::reliable_output::OutputClass;
use ipp_core::{WorldDescriptor, WorldId};
use ipp_protocol::host::{self, HostRequest, HostRequestBody, HostResponse, HostResponseBody};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::{cell::RefCell, rc::Rc};

#[cfg(feature = "instrumentation")]
mod asset_export_testing;
mod asset_exports;
pub(crate) mod asset_sources;
#[cfg(feature = "instrumentation")]
mod bulk_read_testing;
mod bulk_reads;
pub(crate) mod command_batches;
mod control_requests;
mod datasets;
mod messages;
mod persistence;
pub(crate) mod scope;
mod service;

#[cfg(test)]
mod connection_tests;

pub use asset_exports::PublicAssetSourceId;
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
    pub(in crate::services) pending: VecDeque<HostConnectionIngress>,
    outbox: crate::reliable_output::outbox::SessionOutbox,
    reply_reservations: BTreeMap<u64, SharedReplyReservation>,
    pub(crate) failure: Option<String>,
    transfer: Option<persistence::HostWorldTransfer>,
    bulk_reads: BTreeMap<u64, super::bulk_read::BulkReadLease>,
    #[cfg(feature = "instrumentation")]
    bulk_test_inputs: BTreeMap<u64, ipp_core::services::io::IoStreamInput>,
    reply_budget: crate::reliable_output::SharedReplyBudget,
    progress_leases: Rc<std::cell::Cell<usize>>,
    pub(in crate::services) presentation_pending: usize,
    datasets: datasets::DatasetConnection,
    pub(crate) batches: command_batches::HostBatchBuilders,
}

pub(in crate::services) enum HostConnectionIngress {
    Control {
        request: HostRequest,
        received_at: std::time::Duration,
    },
    DecodedWorld {
        request: ipp_protocol::world::Request,
        reservation: Option<SharedReplyReservation>,
        /// Buffered page bytes of an assembled batch, released when it leaves the session queue.
        lease: Option<command_batches::BatchBytesLease>,
        /// First failure of an assembled batch, answered on its final page.
        rejection: Option<String>,
    },
}
