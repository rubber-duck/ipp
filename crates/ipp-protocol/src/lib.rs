//! Bounded binary bootstrap and owned codecs for the headless host contract.

mod codec;

pub mod asset_source;
pub mod attachment_receipts;
pub mod host;
#[cfg(feature = "diagnostics")]
pub mod lifecycle_diagnostics;
pub mod lifecycle_watch;
pub mod presentation;
/// Owned, untrusted transport reference tokens and exact Host resolution.
pub mod references;
pub mod views;

#[cfg(feature = "schema-export")]
mod fixture;

#[cfg(feature = "gui")]
pub mod gui;
#[cfg(feature = "gui")]
pub mod gui_input;
mod wire;
pub use codec::{
    ProtocolError, RejectedBatchPage, RequestDecodeError, decode_request,
    decode_request_with_buffer, decode_world_request, encode_response, encode_response_into,
    encoded_response_size, is_batch_page,
};
#[cfg(feature = "schema-export")]
pub use fixture::{check as check_layout_fixture, export as export_layout_fixture};

use ipp_core::EntitySnapshot;
/// Runtime trait path used by target fixture derives.
#[cfg(feature = "schema-export")]
pub use ipp_core::components::schema;
use ipp_core::components::schema::{ContractHash, ContractSink};

/// Fixed schema-independent bootstrap marker.
pub const MAGIC: [u8; 4] = *b"IPPB";

/// Bootstrap and wire revision.
pub const VERSION: u32 = 2;

/// Maximum complete application message, before decoding or allocation.
pub const MAX_MESSAGE_BYTES: usize = 1_048_576;

/// Most lifecycle publications in one response; a session's drained observations span as
/// many responses as they need.
pub const MAX_LIFECYCLE_PUBLICATIONS: usize = 128;

/// Maximum commands in one batch page; larger batches span several pages.
///
/// 1024 commands carry about 78 thirteen-command GUI rows, so ordinary edits
/// travel as one page. A full page's command slots (1024 × at most
/// [`ipp_core::MAX_COMMAND_INLINE_BYTES`] = 128 KiB) fit one recycled World
/// command buffer, which the Host decodes pages into.
pub const COMMAND_PAGE_COMMANDS: usize = 1024;

const _: () = assert!(COMMAND_PAGE_COMMANDS <= ipp_core::RECYCLED_COMMAND_BUFFER_COMMANDS);

/// Maximum encoded bytes of one batch page message.
///
/// Measured commands encode to 41 bytes on average for GUI rows and 89 for
/// Blender scenes, but URL-bearing mesh instances take about 150 bytes, so
/// 1024 of them would exceed 128 KiB. 256 KiB lets pages of commands up to
/// about 250 bytes reach [`COMMAND_PAGE_COMMANDS`], so the command limit rather
/// than the byte limit decides page size, while staying a quarter of
/// [`MAX_MESSAGE_BYTES`].
pub const COMMAND_PAGE_BYTES: usize = 256 * 1024;

/// Most entity aliases and symbol reports one batch outcome carries for one
/// logical batch.
///
/// A batch is answered once, on its final page, with every alias it defined and
/// every symbol and handle its symbolic references resolved to, so this bounds
/// the whole batch; the Host rejects a batch that defines more aliases or can
/// report more symbols before it applies. Symbol text counts against the reply's
/// message size at the same admission. Each alias report encodes to 12 bytes,
/// so a full alias list takes 384 KiB, within [`MAX_MESSAGE_BYTES`]; it is
/// about three times the entities of the maintained 40,064-command Blender
/// stress import, which the Host answers in one reply. Commands that define
/// no alias add only their symbol reports and applied effects to the reply.
pub const BATCH_OUTCOME_ALIASES: usize = 32_768;

const _: () = assert!(COMMAND_PAGE_COMMANDS <= BATCH_OUTCOME_ALIASES);

/// Most applied effects one batch outcome carries: as many of the smallest, an
/// adoption report of [`attachment_receipts::ADOPTED_EFFECT_BYTES`], as one
/// message holds.
///
/// The Host reserves each effect's encoded size in the batch's reply before the
/// effect can apply (adoption reports with the batch, attachment effects per
/// operation), so the message size rather than this count decides how many
/// effects one outcome reports.
pub const BATCH_OUTCOME_EFFECTS: usize =
    MAX_MESSAGE_BYTES / attachment_receipts::ADOPTED_EFFECT_BYTES;

/// Largest ordinary text or byte field: names, symbolic identities, sources, reasons,
/// dynamic property values and transfer chunks.
///
/// Protocol framing limit. 64 KiB keeps any single field a small share of
/// [`MAX_MESSAGE_BYTES`], so one message still carries several; encoding a longer field
/// fails and decoding one reports a limit error.
pub const MAX_FIELD_BYTES: usize = 65_536;

/// Classes one entity's metadata carries on the wire.
///
/// Classes are short authoring labels; 256 is far beyond any maintained scene while keeping
/// metadata a bounded share of a command page. Longer lists fail to encode or decode.
pub const MAX_METADATA_CLASSES: usize = 256;

/// Field values one `InsertComponent` command writes.
///
/// Matches the widest compiled component, whose field count stays well under 256; a
/// longer list fails to encode or decode.
pub const MAX_INSERT_FIELDS: usize = 256;

/// Records of one kind in one inspection or entity-tree page, and the largest page a
/// query may request.
///
/// Inspection pages; a query continues from the returned cursor. 256 records of the
/// largest kind stay within [`MAX_MESSAGE_BYTES`]; a larger requested page is malformed.
pub const INSPECTION_PAGE_RECORDS: usize = 256;

/// Components one inspected entity reports in a page.
///
/// Bounded by the compiled component registry, well under 256; a longer list fails to
/// encode or decode.
pub const MAX_INSPECTED_COMPONENTS: usize = 256;

/// Fields one inspected component reports: at most one per 16-bit schema offset.
pub const MAX_INSPECTED_FIELDS: usize = 65_536;

/// Deepest descendant level an entity-tree query may request below its root.
///
/// A query page never descends further; deeper descendants are reached by querying from
/// a deeper root. Larger requests are malformed.
pub const MAX_ENTITY_TREE_DEPTH: u16 = 64;

/// Resources one resource event reports; the Host splits larger sets across events.
pub const MAX_RESOURCE_EVENT_RECORDS: usize = 128;

/// Playback events one response reports; the Host splits larger sets across responses.
pub const MAX_PLAYBACK_EVENTS: usize = 1024;

/// UTF-8 bytes of a batch-abort or runtime-failure message; longer diagnostics are
/// truncated by their producer and refused by the codec.
pub const MAX_FAILURE_MESSAGE_BYTES: usize = 2048;

/// Property offsets or joint indices one animation target names.
///
/// 4096 covers every field of the widest component and every joint of a skeleton many
/// times over; a longer list fails to encode or decode.
pub const MAX_ANIMATION_TARGET_INDICES: usize = 4096;

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
    #[cfg(feature = "gui")]
    GuiObservation(gui::GuiObservationRequest),
    /// Ordered World/session lifecycle subscription control.
    LifecycleSubscription(ipp_core::systems::lifecycle_publisher::LifecyclePublisherCommand),
    /// Ordered exact-target memberships with a sole owned typed acknowledgement.
    LifecycleWatch(lifecycle_watch::LifecycleWatchRequest),
    /// Diagnostic-only read fenced to one already acknowledged endpoint.
    #[cfg(feature = "diagnostics")]
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
    GeometryPickQuery(views::GeometryPickQuery),
    /// Project a viewport ray onto a world-space plane without scene mutation.
    CameraProjectQuery(views::CameraProjectQuery),
    /// View-fenced Camera producer navigation with an ordered correlated outcome.
    CameraNavigate(views::CameraNavigateRequest),
    /// Patch session render settings at the ordered mutation boundary.
    RenderStateUpdateCommand(ipp_core::RenderStatePatch),
    /// Sparse Canvas System state update at the ordered mutation boundary;
    /// uncorrelated, and a rejected update is reported only as a diagnostic.
    #[cfg(feature = "surfaces")]
    CanvasStateUpdateCommand(ipp_core::CanvasStateUpdate),
    /// Read committed world state at the next host frame.
    Inspect(InspectionQuery),
}

/// Bounded read of one inspection collection. Zero target selects a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InspectionQuery {
    /// Summary=0, entities=1, resources=2, controllers=3, render diagnostics=4,
    /// tree=5; in GUI builds also the GUI System queries GuiFocus=6 and
    /// GuiPointers=7, whose identity cursor and target are entity identities;
    /// in Surface builds also the Canvas System query Canvas=8, one record
    /// without cursor or target.
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
    #[cfg(feature = "diagnostics")]
    LifecycleDiagnostics(lifecycle_diagnostics::LifecycleDiagnosticSample),
    /// Ordered registration marker or immutable applied observation; outer tick is zero.
    #[cfg(feature = "gui")]
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
    /// Applied lifecycle observations, at most [`MAX_LIFECYCLE_PUBLICATIONS`] per response.
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
    GeometryPickResultEvent(views::GeometryPickOutcome),
    /// Terminal correlated camera/plane projection result.
    CameraProjectResultEvent(views::CameraProjectOutcome),
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
        #[cfg(feature = "gui")]
        gui_focus: Vec<ipp_core::systems::gui::local::GuiFocusRecord>,
        /// GUI System query: live pointer feedback, by target entity then pointer.
        #[cfg(feature = "gui")]
        gui_pointers: Vec<ipp_core::systems::gui::local::GuiPointerRecord>,
        /// Canvas System query: the World canvas's state and last evaluated extent.
        #[cfg(feature = "surfaces")]
        canvas: Option<ipp_core::CanvasStateRecord>,
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

fn write_contract(sink: &mut impl ContractSink) {
    ipp_core::components::registry::write_contract(sink);
    wire::write_contract(sink);
}

/// Identity of this target's actual compiled registry, defaults and wire contract.
///
/// The contract is fixed for a compiled build, so it is hashed once per process.
/// Zero marks the unset cache; concurrent first reads compute the same value.
pub fn schema_hash() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    static HASH: AtomicU64 = AtomicU64::new(0);

    let cached = HASH.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }

    let mut hash = ContractHash::default();
    write_contract(&mut hash);
    HASH.store(hash.0, Ordering::Relaxed);
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
