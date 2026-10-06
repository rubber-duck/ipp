//! Target-independent request queue, frame dispatch, and response publication.
//!
//! HostServices adapters own presentation and source I/O while this crate keeps the
//! session ordering, correlation, and backpressure policy identical across hosts.

mod host;
mod host_services;
#[cfg(feature = "instrumentation")]
mod profiling;
mod reliable_output;
pub mod services;
mod session;
pub mod statistics;
#[cfg(test)]
mod test_support;

pub use host_services::{
    HostInputFailure, HostPresentationFailure, HostServices, builtin_resource, deliver_resource,
};
pub use reliable_output::{PreparedOutputCopy, ReliableResponse, ResponseLease};
pub use services::connection::HostConnectionMessage;
pub use services::presentation::{PresentationCompletion, PresentationDrawSummary};

/// Trusted Host public-source policy identity; copying it grants no client authority.
pub use services::connection::PublicAssetSourceId;

use std::collections::VecDeque;

use ipp_core::systems::lifecycle_publisher::{LifecyclePublisherOutput, LifecyclePublisherSystem};
use ipp_core::{WorldContext, WorldId};
use ipp_protocol::world::{Request, RequestBody, Response, ResponseBody};
use reliable_output::ReliableResponse as QueuedResponse;

/// Requests a connection may queue, and a session may hold admitted but unanswered, before the
/// Host refuses more ingress. Clients control this count, so it is their backpressure: admission
/// reserves each reply's output before acceptance, and uncorrelated output never counts against
/// it. Reliable output is bounded separately by bytes (`reliable_output::MAX_OUTPUT_BYTES`).
///
/// Transports size their read-ahead from this window so a resuming Host refills at once. 64
/// keeps many requests in flight on one connection while its reserved replies stay a small
/// share of the connection's output budget; a request beyond it fails that connection.
pub const MAX_PENDING: usize = 64;

/// One isolated protocol session and its world.
pub struct WorldSession {
    id: u64,
    ready: bool,
    world: WorldId,
    pending: VecDeque<Request>,
    replies: Vec<(u64, WorldSessionReply)>,
    prepared: bool,
    outbox: reliable_output::outbox::SessionOutbox,
    progress_leases: std::rc::Rc<std::cell::Cell<usize>>,
    connection_progress_leases: std::rc::Rc<std::cell::Cell<usize>>,
    progress: Option<reliable_output::progress::WorldProgress>,
    receipts: session::attachment_receipts::SharedReceipts,
    lifecycle_watch: Option<session::lifecycle_watch::SessionLifecycleWatch>,
    gui_observations: Option<session::gui_observations::SessionObservations>,
    reply_budget: reliable_output::SharedReplyBudget,
    reply_reservations: std::collections::BTreeMap<u64, reliable_output::SharedReplyReservation>,
    private_world: bool,
    /// Buffered page bytes of assembled batches still waiting in `pending`.
    batch_leases:
        std::collections::BTreeMap<u64, services::connection::command_batches::BatchBytesLease>,
    last_failure: Option<(ipp_protocol::world::RuntimeFailureScope, bool, String)>,
    request_origins: std::collections::BTreeMap<u64, (u64, Option<u64>)>,
    pending_errors: std::collections::BTreeMap<u64, String>,
    client_sources: std::collections::BTreeMap<
        ipp_core::services::asset_management::AssetSource,
        services::connection::asset_sources::ClientSourceRecord,
    >,
    source_transfers:
        std::collections::BTreeMap<u64, services::connection::asset_sources::SourceTransfer>,
}

enum WorldSessionReply {
    CameraNavigate,
    LifecycleSubscription,
    LifecycleDiagnostics(ipp_protocol::world::lifecycle_diagnostics::LifecycleDiagnosticQuery),
    Batch {
        operations: usize,
    },
    AnimationController,
    Inspect(ipp_protocol::world::InspectionQuery),
    GeometryPick(ipp_protocol::world::view_queries::GeometryPickQuery),
    CameraProject(ipp_protocol::world::view_queries::CameraProjectQuery),
    CompletedView(ResponseBody),
    Rejected(String),
}

/// Temporary access to one session, its Host-owned world and borrowed services.
pub struct WorldSessionContext<'a, P: HostServices> {
    session: &'a mut WorldSession,
    world: WorldContext<'a>,
    services: &'a mut P,
}

/// Process/worker owner of shared services, worlds and world-scoped sessions.
/// World sessions contain routing and protocol queues, never services or worlds.
pub struct Host<P: HostServices> {
    #[cfg(feature = "instrumentation")]
    profiling: profiling::HostProfiling,
    scheduler: services::task_scheduler::TaskSchedulerService,
    connections: services::connection::HostConnectionService,
    sessions: std::collections::BTreeMap<u64, WorldSession>,
    runtime: ipp_core::HostRuntime,
    services: P,
    frame_scratch: HostFrameScratch,
    presentation_time: f64,
    presentation: services::presentation::PresentationCoordinator,
}

#[derive(Default)]
struct HostFrameScratch {
    sessions: Vec<u64>,
    prepared: Vec<u64>,
}
