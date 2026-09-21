//! Bounded binary bootstrap and owned codecs for the headless host contract.

mod codec;

pub mod asset_source;
pub mod host;

#[cfg(feature = "schema-export")]
mod fixture;

#[cfg(feature = "gui")]
mod observations;
mod wire;
pub use codec::{
    ProtocolError, decode_request, decode_request_with_buffer, encode_response,
    encode_response_into,
};
#[cfg(feature = "schema-export")]
pub use fixture::{check as check_layout_fixture, export as export_layout_fixture};
#[cfg(feature = "gui")]
pub use observations::{
    GUI_OBSERVATION_TEXT_BYTES, GUI_OBSERVATIONS_PER_MESSAGE, gui_observation_bodies,
};

/// Runtime trait path used by target fixture derives.
#[cfg(feature = "schema-export")]
pub use ipp_core::components::schema;
use ipp_core::components::schema::{ContractHash, ContractSink};
use ipp_core::{Batch, BatchOutcome, EntitySnapshot};

/// Fixed schema-independent bootstrap marker.
pub const MAGIC: [u8; 4] = *b"IPPB";

/// Bootstrap and wire revision.
pub const VERSION: u32 = 2;

/// Maximum complete application message, before decoding or allocation.
pub const MAX_MESSAGE_BYTES: usize = 1_048_576;

/// A request already fenced to the host's current session.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Fresh connection identity.
    pub session: u64,
    /// Nonzero caller correlation identity, or zero for a system command.
    pub request_id: u64,
    /// Owned operation.
    pub body: RequestBody,
}

/// Implemented request surface. Other tags reject explicitly.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestBody {
    /// Correlated incremental edit of a Surface's ordered items.
    #[cfg(feature = "surfaces")]
    SurfaceCommand(ipp_core::systems::surface::SurfaceCommand),
    /// Correlated ordered GUI edits. A logical batch identity retains the
    /// Host's World gate across byte-bounded buffers until explicit finish.
    #[cfg(feature = "gui")]
    GuiCommands {
        /// Host-issued logical batch identity for a non-final buffer.
        batch_id: Option<u64>,
        /// Edits in this bounded buffer; execution stops at the first failure.
        commands: Vec<ipp_core::systems::gui::GuiCommand>,
    },
    /// Bounded inspection query of an authoritative GUI tree.
    #[cfg(feature = "gui")]
    GuiInspect(ipp_core::systems::gui::GuiInspectQuery),
    /// Correlated routing of one ordered pointer/keyboard/text input; boxed
    /// because text and blocker lists carry owned payloads.
    #[cfg(feature = "gui")]
    GuiInput(Box<ipp_core::GuiInputCommand>),
    /// Correlated bounded semantic snapshot query for one panel.
    #[cfg(feature = "gui")]
    GuiSemanticSnapshot(ipp_core::GuiSemanticSnapshotQuery),
    /// Correlated semantic action dispatched through the validated
    /// control policy; boxed because set-text carries an owned payload.
    #[cfg(feature = "gui")]
    GuiSemanticAction(Box<ipp_core::GuiSemanticActionRequest>),
    /// Ordered World/session lifecycle subscription control.
    LifecycleSubscription(ipp_core::systems::lifecycle_publisher::LifecyclePublisherCommand),
    /// Control a World-owned animation controller without a reply.
    AnimationPlaybackCommand {
        /// World-local controller.
        controller: ipp_core::systems::animation::AnimationControllerId,
        /// Ordered control.
        control: ipp_core::systems::animation::AnimationPlaybackControl,
    },
    /// Correlated controller creation, editing, deletion or playback.
    AnimationController(ipp_core::systems::animation::AnimationControllerCommand),
    /// Allocate a unique logical batch identity for this World session.
    BeginBatch,
    /// Explicitly terminate a logical batch, independently of buffer size.
    EndBatch(u64),
    /// Apply a bounded buffer without completing the logical batch.
    BatchChunk(Batch),
    /// Queue this indivisible command sequence through the core.
    Batch(Batch),
    /// Select a valid scene camera at the ordered mutation boundary.
    CameraActivateCommand {
        /// Permanent entity identity in this session.
        entity: ipp_core::EntityId,
    },
    /// Navigate the active camera through ordinary component base updates.
    CameraNavigateCommand(ipp_core::CameraMotion),
    /// Query final evaluated interaction geometry without advancing host time.
    GeometryPickQuery(ipp_core::GeometryPickQuery),
    /// Project a viewport ray onto a world-space plane without scene mutation.
    CameraProjectQuery(ipp_core::CameraProjectQuery),
    /// Patch session render settings at the ordered mutation boundary.
    RenderStateUpdateCommand(ipp_core::RenderStatePatch),
    /// Read committed world state at the next host frame.
    Inspect(InspectionQuery),
}

/// Bounded read of one inspection collection. Zero target selects a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InspectionQuery {
    /// Summary=0, entities=1, resources=2, controllers=3, render diagnostics=4.
    pub collection: u8,
    /// Exclusive identity cursor, zero for the first page.
    pub after: u64,
    /// Exact identity, or zero to use the cursor.
    pub target: u64,
    /// Maximum records, in 1..=256.
    pub limit: u16,
}

impl Default for InspectionQuery {
    fn default() -> Self {
        Self {
            collection: 0,
            after: 0,
            target: 0,
            limit: 256,
        }
    }
}

/// A request result or unsolicited event at an observed core tick.
#[derive(Clone, Debug)]
pub struct Response {
    /// Connection identity.
    pub session: u64,
    /// Original nonzero request identity, or zero for an unsolicited event.
    pub request_id: u64,
    /// Observed core frame.
    pub tick: u64,
    /// Owned result.
    pub body: ResponseBody,
}

/// Failure scopes that leave the Host and unrelated Worlds available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RuntimeFailureScope {
    /// One draw or rendering pass failed.
    Draw = 0,
    /// A resource representation could not be used.
    Resource = 1,
    /// The graphics context is unavailable.
    Context = 2,
    /// World evaluation could not complete.
    World = 3,
}

/// Host results preserve semantic errors separately from codec errors.
#[derive(Clone, Debug)]
pub enum ResponseBody {
    /// Successful correlated Surface edit.
    #[cfg(feature = "surfaces")]
    SurfaceCommand,
    /// Correlated GUI edit group, including a successful prefix on failure.
    #[cfg(feature = "gui")]
    GuiCommands {
        /// Commands that completed successfully before the first failure.
        applied: u32,
        /// Runtime rejection of the next command; absent on full success.
        error: Option<ipp_core::ErrorReason>,
    },
    /// Bounded inspection response for a GUI root or subtree.
    #[cfg(feature = "gui")]
    GuiInspect(ipp_core::systems::gui::GuiInspectResponse),
    /// Authoritative routing disposition for one correlated GUI input.
    #[cfg(feature = "gui")]
    GuiInput {
        /// Source tick whose current layout and camera routed the input.
        tick: u64,
        /// Why no GUI target accepted the input; absent when GUI handled it.
        unhandled: Option<ipp_core::GuiUnhandledReason>,
    },
    /// Correlated bounded semantic snapshot of one panel.
    #[cfg(feature = "gui")]
    GuiSemanticSnapshot(ipp_core::GuiSemanticTree),
    /// Unsolicited committed GUI observations, chunked like Resources.
    /// Committed control effects with their conflicts and cancellations;
    /// broadcast committed state to every session on the World.
    #[cfg(feature = "gui")]
    GuiObservations {
        /// Committed button presses and control values with ticks and paths.
        effects: Vec<ipp_core::GuiInputEffect>,
        /// Arbitration and admission conflicts, reported separately.
        conflicts: Vec<ipp_core::GuiInputConflict>,
        /// Routed inputs cancelled before application, never mixed with effects.
        cancellations: Vec<ipp_core::GuiInputCancellation>,
        /// Supplier-private authoritative native text bridge update.
        text_focus_updates: Vec<ipp_core::GuiTextFocusUpdate>,
    },
    /// Unsolicited unhandled GUI inputs for scene fallback, chunked like
    /// Resources. Supplier session only; raw input stays private.
    #[cfg(feature = "gui")]
    GuiUnhandledInputs {
        /// Well-formed inputs that reached no GUI target, with reasons.
        inputs: Vec<ipp_core::GuiUnhandledInput>,
    },
    /// Recoverable execution diagnostic, independent of command outcomes.
    RuntimeFailure {
        /// Boundary affected by the failure.
        scope: RuntimeFailureScope,
        /// Whether this World requires explicit destruction instead of correction.
        faulted: bool,
        /// Bounded UTF-8 detail.
        message: String,
    },
    /// Successful correlated lifecycle subscription change.
    LifecycleSubscription,
    /// Applied lifecycle observations or terminal queue overflow.
    LifecycleEvents(ipp_core::systems::lifecycle_publisher::LifecyclePublisherOutput),
    /// Successful controller mutation; creation returns its fresh identity.
    AnimationController(Option<ipp_core::systems::animation::AnimationControllerId>),
    /// Ordered controller transitions, published at the completed frame.
    PlaybackEvents(Vec<ipp_core::systems::animation::AnimationPlaybackEvent>),
    /// Fresh Host-issued logical batch identity.
    BatchStarted(u64),
    /// Logical batch terminated; the World may resume evaluation.
    BatchFinished(u64),
    /// Unsolicited terminal failure of a logical batch. Applied effects remain.
    BatchAborted {
        /// Identity that can no longer accept buffers.
        batch_id: u64,
        /// Bounded failure detail.
        message: String,
    },
    /// Applied command-buffer outcome from the actual World.
    Batch(BatchOutcome),
    /// Sparse committed camera-system state change.
    CameraStateChangedEvent(ipp_core::CameraStateChange),
    /// Terminal correlated geometry result, including evaluated camera identity.
    GeometryPickResultEvent(ipp_core::GeometryPickOutcome),
    /// Terminal correlated camera/plane projection result.
    CameraProjectResultEvent(ipp_core::CameraProjectOutcome),
    /// Sparse committed render-system settings change.
    RenderStateUpdatedEvent(ipp_core::RenderStateChange),
    /// Unsolicited notification after a completed host-owned frame.
    Frame {
        /// Finite nonnegative total simulation time in seconds.
        time: f64,
    },
    /// AssetProvider lifecycle observations published after rendering, before the frame event.
    Resources {
        /// At most 128 ordered lifecycle observations; one resource may report several transitions.
        resources: Vec<ipp_core::AssetResourceSnapshot>,
    },
    /// Read-only authored/effective state.
    Inspect {
        /// Exclusive continuation cursor; zero marks completion.
        next: u64,
        /// World-owned controllers and their shared playback state.
        controllers: Vec<ipp_core::systems::animation::AnimationControllerSnapshot>,
        /// Total supplied simulation time.
        time: f64,
        /// Deterministically ordered world entities.
        entities: Vec<EntitySnapshot>,
        /// Current source demand and asynchronous readiness.
        resources: Vec<ipp_core::AssetResourceSnapshot>,
        /// Per-entity render compatibility failures.
        render_diagnostics: Vec<ipp_core::RenderDiagnostic>,
    },
    /// Scoped strict-lifecycle losses after a completed mutation boundary.
    Lifecycle {
        /// Invalidations in deterministic operation order.
        diagnostics: Vec<ipp_core::StateOverlayLifecycleDiagnostic>,
    },
    /// Explicit host rejection.
    Error {
        /// Host error category.
        code: u16,
        /// Human-readable diagnostic.
        message: String,
    },
}

fn write_contract(sink: &mut impl ContractSink) {
    ipp_core::components::registry::write_contract(sink);
    wire::write_contract(sink);
}

/// Identity of this target's actual compiled registry, defaults and wire contract.
pub fn schema_hash() -> u64 {
    let mut hash = ContractHash::default();
    write_contract(&mut hash);
    hash.0
}

/// Fixed 16-byte bootstrap, always checked before schema-dependent decoding.
pub fn bootstrap() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes[..4].copy_from_slice(&MAGIC);
    bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());
    bytes[8..].copy_from_slice(&schema_hash().to_le_bytes());
    bytes
}

/// Validate the exact bootstrap and append the host's fresh session identity.
pub fn accept_bootstrap(bytes: &[u8], session: u64) -> Result<Vec<u8>, ProtocolError> {
    if bytes.len() != 16 {
        return Err(ProtocolError::Malformed("bootstrap length"));
    }
    if bytes[..4] != MAGIC {
        return Err(ProtocolError::Malformed("bootstrap magic"));
    }
    if bytes[4..8] != VERSION.to_le_bytes() {
        return Err(ProtocolError::VersionMismatch);
    }
    if bytes[8..16] != schema_hash().to_le_bytes() {
        return Err(ProtocolError::SchemaMismatch);
    }
    if session == 0 {
        return Err(ProtocolError::SessionMismatch);
    }
    let mut reply = bytes.to_vec();
    reply.extend_from_slice(&session.to_le_bytes());
    Ok(reply)
}

/// Optional binary descriptors, executed in the same target/feature build as the host.
#[cfg(feature = "schema-export")]
pub fn export_contract() -> Vec<u8> {
    let mut bytes = bootstrap().to_vec();
    write_contract(&mut bytes);
    bytes
}
