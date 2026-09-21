//! Target-independent request queue, frame dispatch, and response publication.
//!
//! HostServices adapters own presentation and source I/O while this crate keeps the
//! session ordering, correlation, and backpressure policy identical across hosts.

use std::collections::VecDeque;

use ipp_core::systems::lifecycle_publisher::{LifecyclePublisherOutput, LifecyclePublisherSystem};
use ipp_core::{WorldContext, WorldId};
use ipp_protocol::{Request, RequestBody, Response, ResponseBody};

mod inspection;
mod presentation_failure;
pub mod services;
pub use presentation_failure::HostPresentationFailure;

pub use services::asset_provider::deliver_resource;

#[cfg(feature = "builtin-assets")]
pub use services::asset_provider::builtin_resource;

const MAX_PENDING: usize = 64;
const EVENT_RESERVE: usize = 21;
const MAX_OUTBOX: usize = MAX_PENDING + EVENT_RESERVE;

/// HostServices work executed at Host-owned service and frame boundaries.
pub trait HostServices {
    /// Short diagnostic identity such as `server` or `wasm`.
    const NAME: &'static str;

    /// Initialize shared facilities before publishing any world.
    fn initialize(host: &mut ipp_core::HostRuntime) -> Result<Self, String>
    where
        Self: Sized;

    /// Current rendered surface dimensions; headless hosts accept query dimensions.
    fn render_viewport(&self) -> Option<(u32, u32)> {
        None
    }

    /// Select a presentation target when a logical World session attaches.
    fn attach_world(&mut self, _world: WorldId) -> Result<(), String> {
        Ok(())
    }

    /// Release a presentation attachment without destroying the World.
    fn detach_world(&mut self, _world: WorldId) {}

    /// Present after core evaluation and before resource/event publication.
    fn present(&mut self, _world: &mut WorldContext<'_>) -> Result<(), HostPresentationFailure> {
        Ok(())
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
    outbox: VecDeque<Vec<u8>>,
    private_world: bool,
    command_batch: Option<command_batches::HostCommandBatch>,
    last_failure: Option<(ipp_protocol::RuntimeFailureScope, bool, String)>,
    request_origins: std::collections::BTreeMap<u64, (u64, Option<u64>)>,
    pending_errors: std::collections::BTreeMap<u64, String>,
    client_sources: std::collections::BTreeMap<
        ipp_core::services::asset_management::AssetSource,
        services::connection::asset_sources::ClientSourceRecord,
    >,
    source_transfers:
        std::collections::BTreeMap<u64, services::connection::asset_sources::SourceTransfer>,
    owners: std::collections::BTreeSet<u64>,
}

enum WorldSessionReply {
    #[cfg(feature = "surfaces")]
    SurfaceCommand,
    #[cfg(feature = "gui")]
    GuiCommands,
    #[cfg(feature = "gui")]
    GuiInspect(ipp_core::systems::gui::GuiInspectQuery),
    #[cfg(feature = "gui")]
    GuiInput,
    #[cfg(feature = "gui")]
    GuiSemanticSnapshot(ipp_core::GuiSemanticSnapshotQuery),
    LifecycleSubscription,
    Batch {
        #[cfg(feature = "diagnostics")]
        operations: usize,
    },
    AnimationController,
    Inspect(ipp_protocol::InspectionQuery),
    GeometryPick(Result<(), ipp_core::ErrorReason>),
    CameraProject(Result<(), ipp_core::ErrorReason>),
    Rejected(String),
}

/// Temporary access to one session, its Host-owned world and borrowed services.
pub struct WorldSessionContext<'a, P: HostServices> {
    session: &'a mut WorldSession,
    world: WorldContext<'a>,
    services: &'a mut P,
    response_buffers: &'a mut Vec<Vec<u8>>,
}

mod command_batches;
mod frame;
mod host;
mod publication;
mod session;
pub(crate) use session::{is_command_batch_continuation, next_ingress_id};

/// Process/worker owner of shared services, worlds and world-scoped sessions.
/// World sessions contain routing and protocol queues, never services or worlds.
pub struct Host<P: HostServices> {
    connections: services::connection::HostConnectionService,
    sessions: std::collections::BTreeMap<u64, WorldSession>,
    runtime: ipp_core::HostRuntime,
    services: P,
    frame_scratch: HostFrameScratch,
    response_buffers: Vec<Vec<u8>>,
}

#[derive(Default)]
struct HostFrameScratch {
    sessions: Vec<u64>,
    prepared: Vec<u64>,
    worlds: Vec<WorldId>,
    evaluating: Vec<WorldId>,
}

#[cfg(test)]
mod camera_tests;

#[cfg(test)]
mod host_session_tests;

#[cfg(test)]
mod render_state_tests;

#[cfg(test)]
mod command_batches_tests;
#[cfg(all(test, feature = "gui"))]
mod gui_command_batches_tests;

#[cfg(all(test, feature = "gui"))]
mod gui_input_tests;

#[cfg(feature = "gui")]
mod gui_semantics;

#[cfg(all(test, feature = "gui"))]
mod gui_semantics_tests;
