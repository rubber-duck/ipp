//! World-scoped request and response envelopes, fenced to one World session,
//! with their owned codecs and message vocabularies.

pub mod attachment_receipts;
pub mod gui;
pub mod lifecycle_diagnostics;
pub mod lifecycle_watch;
pub mod view_queries;

mod animation;
mod lifecycle;
mod requests;
mod responses;

#[cfg(test)]
mod codec_tests;

#[cfg(test)]
mod manifest_tests;

#[cfg(test)]
mod reference_tests;

pub use requests::{
    RejectedBatchPage, RequestDecodeError, decode_request, decode_request_with_buffer,
    decode_world_request, is_batch_page,
};
pub use responses::{encode_response, encode_response_into, encoded_response_size};

use ipp_core::EntitySnapshot;

/// One page of a logical batch under a client-assigned identity.
///
/// The client keeps `batch_id` unique among its connection's open batches. Pages
/// append in arrival order; the page with `last` set completes the batch, which
/// the Host then applies as one ordered batch. Only the final page carries a
/// correlated request identity and receives a reply.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchPage {
    /// Client-assigned identity, unique among the connection's open batches.
    pub batch_id: u32,
    /// Whether this page completes the batch.
    pub last: bool,
    /// Commands of this page, in order.
    pub operations: Vec<ipp_core::Command>,
}

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
    /// Observe or explicitly release a receipt in this exact session.
    AttachmentReceipt {
        /// Session-owned receipt handle.
        receipt: u64,
        /// Drop this handle instead of observing its retirement.
        release: bool,
    },
    /// Ordered independent applied-effect observation registration.
    GuiObservation(gui::GuiObservationRequest),
    /// Ordered World/session lifecycle subscription control.
    LifecycleSubscription(ipp_core::systems::lifecycle_publisher::LifecyclePublisherCommand),
    /// Ordered exact-target memberships with a sole owned typed acknowledgement.
    LifecycleWatch(lifecycle_watch::LifecycleWatchRequest),
    /// Diagnostic-only read fenced to one already acknowledged endpoint.
    LifecycleDiagnostics(lifecycle_diagnostics::LifecycleDiagnosticQuery),
    /// Control a World-owned animation controller without a reply.
    AnimationPlaybackCommand {
        /// World-local controller.
        controller: ipp_core::systems::animation::AnimationControllerId,
        /// Ordered control.
        control: ipp_core::systems::animation::AnimationPlaybackControl,
    },
    /// Correlated controller creation, editing, deletion or playback.
    AnimationController(ipp_core::systems::animation::AnimationControllerCommand),
    /// One page of a client-identified logical batch. The Host applies the
    /// whole batch once, when its final page arrives, and answers only that page.
    SubmitBatch(BatchPage),
    /// Query final evaluated interaction geometry without advancing host time.
    GeometryPickQuery(view_queries::GeometryPickQuery),
    /// Project a viewport ray onto a world-space plane without scene mutation.
    CameraProjectQuery(view_queries::CameraProjectQuery),
    /// View-fenced Camera producer navigation with an ordered correlated outcome.
    CameraNavigate(view_queries::CameraNavigateRequest),
    /// Patch session render settings at the ordered mutation boundary.
    RenderStateUpdateCommand(ipp_core::RenderStatePatch),
    /// Sparse Canvas System state update at the ordered mutation boundary;
    /// uncorrelated, and a rejected update is reported only as a diagnostic.
    CanvasStateUpdateCommand(ipp_core::CanvasStateUpdate),
    /// Sparse GUI preferences update at the ordered mutation boundary;
    /// uncorrelated, and a rejected update is reported only as a diagnostic.
    GuiPreferencesUpdateCommand(ipp_core::systems::gui::GuiPreferencesUpdate),
    /// Read committed world state at the next host frame.
    Inspect(InspectionQuery),
}

/// Bounded read of one inspection collection. Zero target selects a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InspectionQuery {
    /// Summary=0, entities=1, resources=2, controllers=3, render diagnostics=4,
    /// tree=5; in GUI builds also the GUI System queries GuiFocus=6,
    /// GuiPointers=7 and GuiActiveItems=9, whose identity cursor and target
    /// are entity identities, the group's for GuiActiveItems;
    /// in Surface builds also the Canvas System query Canvas=8, and in GUI
    /// builds the GUI System query GuiPreferences=10, each one record without
    /// cursor or target.
    pub collection: u8,
    /// Exclusive identity cursor, zero for the first page.
    pub after: u64,
    /// Exact identity, or zero to use the cursor.
    pub target: u64,
    /// Maximum records, in 1..=256.
    pub limit: u16,
    /// Maximum descendant depth for tree reads, in 0..=64.
    pub max_depth: u16,
}

impl Default for InspectionQuery {
    fn default() -> Self {
        Self {
            collection: 0,
            after: 0,
            target: 0,
            limit: 256,
            max_depth: 0,
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
    /// Observed core frame; GUI terminals carry their effect tick or zero for no effect.
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
    /// Camera navigation committed at the reported World mutation tick.
    CameraNavigated,
    /// Constant-size diagnostic sample, not a completed frame or applied effect.
    LifecycleDiagnostics(lifecycle_diagnostics::LifecycleDiagnosticSample),
    /// Ordered registration marker or immutable applied observation; outer tick is zero.
    GuiObservation(ipp_core::systems::gui::observations::GuiObservationRecord),
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
    /// Applied lifecycle observations, at most [`crate::MAX_LIFECYCLE_PUBLICATIONS`] per response.
    LifecycleEvents(ipp_core::systems::lifecycle_publisher::LifecyclePublisherOutput),
    /// Successful controller mutation; creation returns its fresh identity.
    AnimationController(Option<ipp_core::systems::animation::AnimationControllerId>),
    /// Ordered controller transitions, published at the completed frame.
    PlaybackEvents(Vec<ipp_core::systems::animation::AnimationPlaybackEvent>),
    /// Unsolicited failure of an incomplete batch whose next page did not arrive
    /// within the Host's progress deadline. Nothing of the batch was applied; its
    /// final page, if it still arrives, is rejected with the same failure.
    BatchAborted {
        /// Client-assigned identity of the failed batch.
        batch_id: u64,
        /// Bounded failure detail.
        message: String,
    },
    /// Applied command-buffer outcome from the actual World.
    Batch(attachment_receipts::ReceiptBatchOutcome),
    /// Receipt release acknowledgement, or its current retirement observation.
    AttachmentReceipt {
        /// Correlated session-owned handle.
        receipt: u64,
        /// None acknowledges release; false is pending, true is terminal retirement.
        retired: Option<bool>,
    },
    /// Terminal correlated geometry result, including evaluated camera identity.
    GeometryPickResultEvent(view_queries::GeometryPickOutcome),
    /// Terminal correlated camera/plane projection result.
    CameraProjectResultEvent(view_queries::CameraProjectOutcome),
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
    /// Read-only entity state.
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
        /// GUI System query: logical focus.
        gui_focus: Vec<ipp_core::systems::gui::local::GuiFocusRecord>,
        /// GUI System query: live pointer feedback, by target entity then pointer.
        gui_pointers: Vec<ipp_core::systems::gui::local::GuiPointerRecord>,
        /// GUI System query: groups' active items, by group entity.
        gui_active_items: Vec<ipp_core::systems::gui::local::GuiActiveItemRecord>,
        /// Canvas System query: the World canvas's state and last evaluated extent.
        canvas: Option<ipp_core::CanvasStateRecord>,
        /// GUI System query: the World's GUI presentation preferences.
        gui_preferences: Option<ipp_core::systems::gui::GuiPreferences>,
    },
    /// Bounded hierarchy in depth-first sibling order.
    EntityTree {
        /// Exclusive continuation identity, zero when complete.
        next: u64,
        /// Total supplied simulation time.
        time: f64,
        /// Ordered nodes in the requested root or forest.
        nodes: Vec<EntityTreeNode>,
    },
    /// Explicit host rejection.
    Error {
        /// Host error category.
        code: u16,
        /// Human-readable diagnostic.
        message: String,
    },
}

/// One hierarchy node and its depth relative to the requested root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityTreeNode {
    /// Entity identity.
    pub id: ipp_core::EntityId,
    /// Parent, if any.
    pub parent: Option<ipp_core::EntityId>,
    /// Full sibling order label.
    pub order: u128,
    /// Depth relative to the requested root or forest.
    pub depth: u16,
}
