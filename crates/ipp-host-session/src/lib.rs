//! Target-independent request queue, frame dispatch, and response publication.
//!
//! HostServices adapters own presentation and source I/O while this crate keeps the
//! session ordering, correlation, and backpressure policy identical across hosts.

use std::collections::VecDeque;

use ipp_core::systems::lifecycle_publisher::{LifecyclePublisherOutput, LifecyclePublisherSystem};
use ipp_core::{WorldContext, WorldId};
use ipp_protocol::{Request, RequestBody, Response, ResponseBody};

#[cfg(feature = "instrumentation")]
mod profiling;

mod attachment_receipts;
mod inspection;
mod outbox;
mod presentation_failure;
mod progress;
pub mod services;
pub use presentation_failure::{HostInputFailure, HostPresentationFailure};
pub use services::connection::HostConnectionMessage;
pub use services::presentation::{PresentationCompletion, PresentationDrawSummary};

pub use services::asset_provider::deliver_resource;

pub use services::asset_provider::builtin_resource;

/// Requests a connection may queue, and a session may hold admitted but unanswered, before the
/// Host refuses more ingress. Clients control this count, so it is their backpressure: admission
/// reserves each reply's output before acceptance, and uncorrelated output never counts against
/// it. Reliable output is bounded separately by bytes (`reliable_output::MAX_OUTPUT_BYTES`).
///
/// Transports size their read-ahead from this window so a resuming Host refills at once. 64
/// keeps many requests in flight on one connection while its reserved replies stay a small
/// share of the connection's output budget; a request beyond it fails that connection.
pub const MAX_PENDING: usize = 64;

/// HostServices work executed at Host-owned service and frame boundaries.
pub trait HostServices {
    /// Short diagnostic identity such as `server` or `wasm`.
    const NAME: &'static str;

    /// Start optional renderer measurements at an accepted capture boundary.
    #[cfg(feature = "instrumentation")]
    fn render_profile_start(
        &mut self,
        _capture: u64,
        _host: u64,
        _options: ipp_protocol::profiling::ProfileRenderOptions,
    ) -> ipp_protocol::profiling::ProfileGpuCapability {
        ipp_protocol::profiling::ProfileGpuCapability::Unsupported
    }

    /// Freeze terminal and pending GPU records without waiting for the device.
    #[cfg(feature = "instrumentation")]
    fn render_profile_stop(&mut self, _capture: u64) -> ipp_protocol::profiling::ProfileGpuCapture {
        use ipp_protocol::profiling::*;
        ProfileGpuCapture {
            sampling: ProfileGpuSampling::Off,
            capability: ProfileGpuCapability::Unsupported,
            availability: ProfileGpuAvailability::Unsupported,
            dropped_records: 0,
            records: Vec::new(),
            gl_calls: None,
        }
    }

    /// Cancel pending owned GPU queries at a quiescent control boundary.
    #[cfg(feature = "instrumentation")]
    fn render_profile_cancel(&mut self, _capture: u64) {}

    /// Initialize shared facilities before publishing any world.
    fn initialize(host: &mut ipp_core::HostRuntime) -> Result<Self, String>
    where
        Self: Sized;

    /// Current rendered surface dimensions; headless hosts accept query dimensions.
    fn render_viewport(&self) -> Option<(u32, u32)> {
        None
    }

    /// Observe the completed Host evaluation boundary without advancing or mutating Worlds.
    fn record_frame(
        &mut self,
        _host: &mut ipp_core::HostRuntime,
        _frame: &ipp_core::HostFrameReport,
    ) {
    }

    /// Admit composed input, continuing healthy roots and returning scoped failures.
    fn route_input(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        _frame: &ipp_core::HostFrameReport,
    ) -> Vec<HostInputFailure> {
        let surface = self.presentation_surface().ok();
        if let Some(input) = self.gui_input() {
            input.route(host, surface);
        }

        Vec::new()
    }

    /// Optional physical GUI adapter. Headless semantic commands do not use this owner.
    fn gui_input(&mut self) -> Option<&mut services::gui_input::GuiHostInputService> {
        None
    }

    /// Physical selection lifetime hook, independent of authoring sessions.
    fn presentation_selection(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        selection: Option<ipp_protocol::presentation::PresentationView>,
    ) {
        if let Some(input) = self.gui_input() {
            input.selection(host, selection);
        }
    }

    /// Prepare renderer demand without borrowing live World component values.
    fn prepare_presentation(
        &mut self,
        _host: &mut ipp_core::HostRuntime,
        _selected: Option<(ipp_core::OutputRef, ipp_core::WorldPublicationId)>,
    ) -> Result<(), HostPresentationFailure> {
        Ok(())
    }

    /// Present an explicitly selected completed root output, independently of sessions.
    fn present(
        &mut self,
        _host: &ipp_core::HostRuntime,
        _output: ipp_core::OutputRef,
        _publication: ipp_core::WorldPublicationId,
        _viewport: ipp_core::WorldViewport,
        _presentation_time: f64,
        _completion: PresentationCompletion<'_>,
    ) -> Result<PresentationDrawSummary, HostPresentationFailure> {
        Err(HostPresentationFailure {
            scope: ipp_protocol::RuntimeFailureScope::Context,
            message: "presentation is unsupported".into(),
        })
    }

    /// Actual surface/context identity and device bounds, independent of World sessions.
    fn presentation_surface(
        &self,
    ) -> Result<
        ipp_protocol::presentation::PresentationSurface,
        ipp_protocol::presentation::PresentationError,
    > {
        Err(ipp_protocol::presentation::PresentationError::Unsupported)
    }

    /// Configure the exact acknowledged extent; adapters must not silently clamp it.
    fn configure_presentation(
        &mut self,
        _viewport: ipp_core::WorldViewport,
    ) -> Result<(), ipp_protocol::presentation::PresentationError> {
        Err(ipp_protocol::presentation::PresentationError::Unsupported)
    }

    /// Service provider cancellations and requests at the host boundary.
    fn service_resources(&mut self, host: &mut ipp_core::HostRuntime) -> Result<(), String>;

    /// Progress shared loaders exactly once before world evaluation.
    fn progress_assets(&mut self, host: &mut ipp_core::HostRuntime) {
        self.progress_resources(host);
    }

    /// Progress shared loaders between frames without resetting frame-local state.
    fn progress_resources(&mut self, host: &mut ipp_core::HostRuntime) {
        host.progress_assets();
    }

    /// Dequeue one services-owned provider request, when the host exposes one.
    fn take_resource_request(&mut self) -> Option<Vec<u8>> {
        None
    }
}

/// One isolated protocol session and its world.
pub struct WorldSession {
    id: u64,
    ready: bool,
    world: WorldId,
    pending: VecDeque<Request>,
    replies: Vec<(u64, WorldSessionReply)>,
    prepared: bool,
    outbox: outbox::SessionOutbox,
    progress_leases: std::rc::Rc<std::cell::Cell<usize>>,
    connection_progress_leases: std::rc::Rc<std::cell::Cell<usize>>,
    progress: Option<progress::WorldProgress>,
    receipts: attachment_receipts::SharedReceipts,
    lifecycle_watch: Option<lifecycle_watch::SessionLifecycleWatch>,
    gui_observations: Option<gui_observations::SessionObservations>,
    reply_budget: attachment_receipts::SharedReplyBudget,
    reply_reservations:
        std::collections::BTreeMap<u64, attachment_receipts::SharedReplyReservation>,
    private_world: bool,
    /// Buffered page bytes of assembled batches still waiting in `pending`.
    batch_leases: std::collections::BTreeMap<u64, command_batches::BatchBytesLease>,
    last_failure: Option<(ipp_protocol::RuntimeFailureScope, bool, String)>,
    request_origins: std::collections::BTreeMap<u64, (u64, Option<u64>)>,
    pending_errors: std::collections::BTreeMap<u64, String>,
    client_sources: std::collections::BTreeMap<
        ipp_core::services::asset_management::AssetSource,
        services::connection::asset_sources::ClientSourceRecord,
    >,
    source_transfers:
        std::collections::BTreeMap<u64, services::connection::asset_sources::SourceTransfer>,
}

mod reliable_output;
use reliable_output::ReliableResponse as QueuedResponse;
pub use reliable_output::{PreparedOutputCopy, ReliableResponse, ResponseLease};

enum WorldSessionReply {
    CameraNavigate,
    LifecycleSubscription,
    LifecycleDiagnostics(ipp_protocol::lifecycle_diagnostics::LifecycleDiagnosticQuery),
    Batch {
        operations: usize,
    },
    AnimationController,
    Inspect(ipp_protocol::InspectionQuery),
    GeometryPick(ipp_protocol::views::GeometryPickQuery),
    CameraProject(ipp_protocol::views::CameraProjectQuery),
    CompletedView(ResponseBody),
    Rejected(String),
}

/// Temporary access to one session, its Host-owned world and borrowed services.
pub struct WorldSessionContext<'a, P: HostServices> {
    session: &'a mut WorldSession,
    world: WorldContext<'a>,
    services: &'a mut P,
}

mod command_batches;
mod frame;
mod host;
mod lifecycle_watch;
mod publication;
mod session;
mod view_queries;
pub(crate) use session::next_ingress_id;

/// Process/worker owner of shared services, worlds and world-scoped sessions.
/// World sessions contain routing and protocol queues, never services or worlds.
pub struct Host<P: HostServices> {
    #[cfg(feature = "instrumentation")]
    profiling: profiling::HostProfiling,
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

#[cfg(test)]
mod camera_tests;

#[cfg(test)]
mod host_session_tests;

#[cfg(test)]
mod render_state_tests;

#[cfg(test)]
mod command_batches_tests;
mod gui_observations;
