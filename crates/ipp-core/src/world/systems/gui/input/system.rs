//! Ordered GUI input routing against one immutable per-tick snapshot.
//!
//! [`GuiInputSystem`] owns one input context per world: the keyboard focus,
//! per-pointer capture/hover/press cursors, per-node predicted control
//! revisions, input-owned scroll offsets and the ordered envelope queue.
//! Exactly one session owns the context at a time (see [`GuiInputOwner`]).
//! [`System::command`] only admits well-formed inputs into the ordered
//! ingress queue during mutation. Routing runs in [`System::update`], after
//! the current layout and final camera/geometry evaluation, against the
//! retained layout views (one immutable per-tick snapshot shared by every
//! input of the tick); cursors update there so chained inputs of the same
//! tick observe each other. Routed control, button, focus and scroll intents
//! queue as ordered envelopes with their source tick and apply at the
//! following mutation boundary ([`System::accept_ingress`], before animation
//! evaluation) with liveness and ownership revalidation. Effects,
//! cancellations, conflicts and unhandled inputs are reported separately
//! into [`WorldUpdateReport`](crate::WorldUpdateReport) at
//! [`System::finish_update`], each carrying source/effect ticks. Committed
//! control effects additionally pin their runtime logical ancestor path,
//! root-first including the target, for listener dispatch.
//!
//! Panels are [`GuiRoot`](super::super::GuiRoot) entities placed as transformed
//! Surfaces in the World. Pointer events carry a viewport point (normalized
//! `0..=1`, top-left origin, +Y down, shared with the geometry pick
//! contract), the explicitly marked scene blocker entities and an optional
//! panel distance. With an active camera and host viewport, routing builds
//! the current-tick camera ray, intersects transformed Surfaces, keeps
//! front faces only and resolves every panel and marked blocker distance
//! from scene geometry; the nearest eligible hit wins with deterministic
//! entity ties. Without a camera or viewport (native and headless use),
//! pointer events carry a GUI-logical point instead: without a distance the
//! overlay panel counts as nearest and the topmost hit across panels is the
//! greatest [`EntityId`](crate::EntityId). Keys carry no point: keyboard
//! traversal crosses panels, and Tab without focus enters one, in the
//! keyboard panel order of [`GuiKey`], which the active camera's view
//! distance decides with front-facing panels first. Each source input
//! dispatches at most once. Scroll offsets stay owned by this system so scrolled content
//! never reflows layout; scroll routing consumes deltas innermost-first
//! against evaluated ScrollView extents with edge clamping and outward
//! propagation, chaining same-tick scrolls against routed offsets that
//! include the deltas already queued, while hit testing observes the
//! retained snapshot translated by committed ancestor offsets and clipped by
//! viewports that move with their own outer offsets. Button and
//! checkbox presses stay provisional until an eligible tap completes on
//! release. A primary press inside eligible ScrollView ancestry also arms a
//! pointer drag: once its travel passes the drag slop, the nearest
//! ScrollView with capacity on the dominant axis wins the gesture, the
//! pending tap cancels and pointer travel scrolls through the same
//! innermost-first consumption. Outside ScrollViews, drags leaving the
//! press-time rectangle cancel the tap. ScrollView scroll bars sit above
//! their content for hits: a thumb press captures the pointer and maps its
//! travel along the track to the offset, a track press pages by one
//! viewport toward the pressed side, and both scroll through the same
//! consumption; bars take hover and press state per part.

use super::super::system::{check_control_value, commit_control_value, resolve_control_effective};
use super::super::tree::GuiNodeDataRow;
use super::super::tree::nodes::{GuiControlValue, GuiNodeData, GuiNodeHandle, GuiNodeId};
use super::super::{
    GuiEvaluatedContent, GuiEvaluatedNode, GuiEvaluatedView, GuiHit, GuiLayoutSystem, GuiRoot,
    GuiScrollBarCursor, GuiSkinCursors, GuiSystem,
};
use super::keyboard_panels::{GuiKeyboardView, keyboard_panel_order};
use super::target_policy::{
    GuiTargetStatus, GuiTargetValidity, current_target, evaluated_status, evaluated_validity,
    producer_root, producer_status, producer_validity,
};

use super::super::layout::scroll_bars::{
    GuiScrollBar, scroll_bar_axis, scroll_bar_enabled, scroll_bars_in_view, scroll_capacity,
};
use crate::systems::geometry::{GeometryBounds, GeometryRay};
use crate::systems::lifecycle_publisher::{
    ComponentLifecycleKind, EntityLifecycleKind, LifecycleObservation,
};
use crate::systems::surface::{GuiPrimitivePart, GuiScrollPlacement, SurfaceClipRect};
use crate::systems::{
    System, SystemCommandContext, SystemDependency, SystemDependencyBinding, SystemFactory,
    SystemId, SystemInitContext, SystemInitError, SystemLifecycleContext, SystemRuntimeAccess,
    SystemUpdateContext,
};
use crate::world::WorldSimulationState;
use crate::world::access::commit_components;
use crate::{ComponentValue, EntityId, ErrorReason};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// Stable identity of the per-world GUI input routing pass.
const MAX_PENDING_ENVELOPES: usize = 1024;

/// Pointer travel, as a fraction of the panel's shorter logical extent,
/// that a press inside ScrollView content must exceed before dragging wins
/// over the tap.
const SCROLL_DRAG_SLOP: f32 = 0.01;

/// Which physical button a pointer event carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiPointerButton {
    /// Primary button or touch contact.
    Primary,
    /// Secondary button.
    Secondary,
    /// Auxiliary button.
    Auxiliary,
}

/// Non-text keys routable to the focused control.
///
/// Keyboard traversal follows tree order within the innermost focus scope
/// (a node whose `focus_scope` style property is set) containing the focused
/// control, or across every panel in keyboard panel order when no scope
/// contains it, and wraps at either end. With an active camera, keyboard
/// panel order puts front-facing panels first by World-space view distance,
/// then back-facing panels by the same distance, with entity ties; without
/// one it is entity order. Without focus, [`Self::Tab`] and [`Self::BackTab`]
/// enter the keyboard panel at its first or last control: the first panel
/// in keyboard panel order, which is the nearest front-facing panel with a
/// camera. Only without a camera does entry prefer the panel that most
/// recently held focus in this input context while it still has a focusable
/// control. Entry acquires an unowned input context. Disabled, hidden,
/// transparent and unavailable controls are skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiKey {
    /// Move keyboard focus to the next control in traversal order, or enter
    /// the keyboard panel at its first control.
    Tab,
    /// Activate the focused button or checkbox, or submit focused text
    /// outside composition.
    Enter,
    /// Activate the focused button or checkbox, or type a space into text.
    Space,
    /// Release keyboard focus, leaving every focus scope; the next
    /// [`Self::Tab`] or [`Self::BackTab`] enters the keyboard panel again.
    Escape,
    /// Delete the grapheme before the caret, or the selection, in focused text.
    Backspace,
    /// Delete the grapheme after the caret, or the selection, in focused text.
    Delete,
    /// Nudge a focused slider, or nothing on other controls.
    Left,
    /// Nudge a focused slider, or nothing on other controls.
    Right,
    /// Nudge a focused slider, or nothing on other controls.
    Up,
    /// Nudge a focused slider, or nothing on other controls.
    Down,
    /// Jump a focused slider to its minimum.
    Home,
    /// Jump a focused slider to its maximum.
    End,
    /// Move keyboard focus to the previous control in traversal order, or
    /// enter the keyboard panel at its last control (Shift+Tab).
    BackTab,
}

/// One ordered input routed against the retained per-tick snapshot.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiInputCommand {
    /// Press a pointer at a viewport point, capturing its target.
    PointerDown {
        /// Platform pointer identity; one capture per pointer.
        pointer: u32,
        /// Restrict routing to one panel, or hit-test every panel.
        panel: Option<EntityId>,
        /// Viewport point shared with hit testing: normalized `0..=1` with
        /// an active camera and host viewport, otherwise a GUI-logical point.
        position: [f32; 2],
        /// Pressed button.
        button: GuiPointerButton,
        /// Explicitly marked scene blocker entities. Distances resolve from
        /// current-tick scene geometry on the projected path; caller values
        /// apply only to logical routing without a camera.
        blockers: Vec<super::super::GuiBlockerHit>,
        /// World-space panel distance, or None for overlay-nearest. Resolved
        /// in core from the camera ray on the projected path; a caller value
        /// applies only to logical routing without a camera.
        panel_distance: Option<f32>,
    },
    /// Release a pointer, completing or cancelling its press.
    PointerUp {
        /// Platform pointer identity.
        pointer: u32,
        /// Restrict routing to one panel, or hit-test every panel.
        panel: Option<EntityId>,
        /// Viewport release point: normalized `0..=1` with an active camera
        /// and host viewport, otherwise a GUI-logical point.
        position: [f32; 2],
        /// Released button.
        button: GuiPointerButton,
        /// Explicitly marked scene blocker entities; see [`GuiInputCommand::PointerDown`].
        blockers: Vec<super::super::GuiBlockerHit>,
        /// World-space panel distance, or None for overlay-nearest; see
        /// [`GuiInputCommand::PointerDown`].
        panel_distance: Option<f32>,
    },
    /// Move a pointer, updating hover or a captured drag.
    PointerMove {
        /// Platform pointer identity.
        pointer: u32,
        /// Restrict routing to one panel, or hit-test every panel.
        panel: Option<EntityId>,
        /// Viewport point: normalized `0..=1` with an active camera and host
        /// viewport, otherwise a GUI-logical point.
        position: [f32; 2],
        /// Explicitly marked scene blocker entities; see [`GuiInputCommand::PointerDown`].
        blockers: Vec<super::super::GuiBlockerHit>,
        /// World-space panel distance, or None for overlay-nearest; see
        /// [`GuiInputCommand::PointerDown`].
        panel_distance: Option<f32>,
    },
    /// Lose a pointer to platform blur or capture loss, cancelling its press.
    PointerCancel {
        /// Platform pointer identity.
        pointer: u32,
    },
    /// Scroll at a viewport point without reflowing layout.
    Scroll {
        /// Restrict routing to one panel, or hit-test every panel.
        panel: Option<EntityId>,
        /// Viewport scroll point: normalized `0..=1` with an active camera
        /// and host viewport, otherwise a GUI-logical point.
        position: [f32; 2],
        /// Scroll delta in logical units.
        delta: [f32; 2],
        /// Explicitly marked scene blocker entities; see [`GuiInputCommand::PointerDown`].
        blockers: Vec<super::super::GuiBlockerHit>,
        /// World-space panel distance, or None for overlay-nearest; see
        /// [`GuiInputCommand::PointerDown`].
        panel_distance: Option<f32>,
    },
    /// Press or release a non-text key on the focused control.
    Key {
        /// Key identity.
        key: GuiKey,
        /// True for press, false for release (releases are silent).
        pressed: bool,
    },
    /// Insert text at the caret, replacing the selection, in the focused text input.
    Text {
        /// Text to insert at the committed caret.
        text: String,
        /// Focus and revision the sender observed; see [`GuiTextFence`].
        fence: Option<GuiTextFence>,
    },
    /// Move keyboard focus to one fenced control node.
    Focus {
        /// Fenced node handle; session-fenced like authored handles.
        handle: GuiNodeHandle,
    },
    /// Release keyboard focus.
    Blur,
    /// Set caret/selection on the focused single-line text input.
    /// UTF-8 byte offsets; both must be grapheme boundaries in the current
    /// predicted/committed text. Collapsed (`start == end`) moves the caret.
    SetTextSelection {
        /// Anchor byte offset.
        start: u32,
        /// Caret byte offset.
        end: u32,
        /// Focus and exact revision the offsets were chosen against; see
        /// [`GuiTextFence`].
        fence: Option<GuiTextFence>,
    },
    /// Provisional IME composition for the focused text input.
    /// Distinct from committed text: no reflow until explicit commit.
    /// Caret offsets index `text` (same revision) and must be boundaries in it.
    UpdateComposition {
        /// Provisional IME text.
        text: String,
        /// Provisional selection anchor within `text`.
        caret_start: u32,
        /// Provisional caret within `text`.
        caret_end: u32,
        /// Focus and revision the sender observed; see [`GuiTextFence`].
        fence: Option<GuiTextFence>,
    },
    /// Commit the active provisional at the committed caret/selection.
    CommitComposition {
        /// Focus and revision the sender observed; see [`GuiTextFence`].
        fence: Option<GuiTextFence>,
    },
    /// Cancel the active provisional without committing.
    CancelComposition {
        /// Focus and revision the sender observed; see [`GuiTextFence`].
        fence: Option<GuiTextFence>,
    },
}

/// Focus and text revision a native text buffer observed when it produced
/// one text, selection or composition command.
///
/// The fields mirror the published [`GuiTextFocusState`]. Routing rejects a
/// fenced command as a conflict, without any write, when the input context
/// generation, focus generation or target differs from the current focus,
/// or when the revision is newer than the current text. Selection offsets
/// must name the current revision exactly; insertions and composition may
/// name an older revision of the same focus, because the sender's own
/// in-flight edits advance the revision before it observes them. An external
/// replacement of the focused text moves the focus generation, so an
/// insertion stamped before it conflicts instead of rebasing onto the newer
/// text; a rejected prediction republishes the rolled-back text, and
/// insertions stamped against the prediction name a newer revision and
/// conflict.
///
/// Adapters holding a temporary native buffer or delivering delayed platform
/// results (clipboard reads, IME updates) stamp every edit. An unfenced
/// command applies to the current focus in ingress order, like keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiTextFence {
    /// Input context generation the sender observed.
    pub context_generation: u64,
    /// Focus generation the sender observed.
    pub focus_generation: u64,
    /// Focused text input the sender observed.
    pub target: GuiInputTarget,
    /// Text revision the sender observed.
    pub revision: u32,
}

/// Identity of a routed node, fenced against removal and replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GuiInputTarget {
    /// Panel entity owning the target.
    pub entity: EntityId,
    /// Root-local node identity.
    pub node: GuiNodeId,
    /// Root component incarnation the routing snapshot was built against.
    pub root_incarnation: u64,
}

/// Keyboard focus with the session that owns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiInputFocus {
    /// Focused node.
    pub target: GuiInputTarget,
    /// Session that set the focus.
    pub session: u64,
}

/// Authoritative transient state for the focused native text bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct GuiTextFocusState {
    pub session: u64,
    pub context_generation: u64,
    pub focus_generation: u64,
    pub target: GuiInputTarget,
    pub revision: u32,
    pub text: String,
    /// UTF-8 byte anchor; greater than `selection_end` for a backward selection.
    pub selection_start: u32,
    /// UTF-8 byte caret (the moving end of the selection).
    pub selection_end: u32,
    pub composition: Option<GuiTextCompositionState>,
}

/// Provisional IME text and its UTF-8 byte selection.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct GuiTextCompositionState {
    pub text: String,
    pub caret_start: u32,
    pub caret_end: u32,
}

/// Changed native text bridge state published after routing.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GuiTextFocusUpdate {
    Focused(GuiTextFocusState),
    Cleared {
        session: u64,
        context_generation: u64,
        focus_generation: u64,
    },
}

/// One committed GUI input effect with source/effect ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiInputEffect {
    /// Session that supplied the source input.
    pub session: u64,
    /// Frame whose snapshot the input routed against.
    pub source_tick: u64,
    /// Frame whose mutation boundary applied the effect.
    pub effect_tick: u64,
    /// Committed outcome.
    pub kind: GuiInputEffectKind,
}

/// Committed GUI input outcomes, reported separately from cancellations,
/// conflicts and unhandled inputs.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiInputEffectKind {
    /// A button press completed (pointer tap or key activation).
    ButtonPressed {
        /// Panel entity.
        entity: EntityId,
        /// Root component incarnation fencing replacement.
        root_incarnation: u64,
        /// Pressed node.
        node: GuiNodeId,
        /// Runtime logical ancestor path, root-first including the target,
        /// pinned from the live tree at application for listener dispatch.
        path: Vec<GuiNodeId>,
    },
    /// A revision-gated control value committed.
    ControlCommitted {
        /// Panel entity.
        entity: EntityId,
        /// Root component incarnation fencing replacement.
        root_incarnation: u64,
        /// Control node.
        node: GuiNodeId,
        /// Committed value.
        value: GuiControlValue,
        /// Revision that produced it.
        revision: u32,
        /// Runtime logical ancestor path, root-first including the target,
        /// pinned from the live tree at application for listener dispatch.
        path: Vec<GuiNodeId>,
        /// What produced the commit.
        source: GuiCommitSource,
    },
    /// Enter submitted a focused single-line text input outside composition.
    Submitted {
        /// Panel entity.
        entity: EntityId,
        /// Root component incarnation fencing replacement.
        root_incarnation: u64,
        /// Submitted text input node.
        node: GuiNodeId,
        /// Committed text revision that was submitted.
        revision: u32,
        /// Committed text at that revision.
        text: String,
        /// Runtime logical ancestor path, root-first including the target,
        /// pinned from the live tree at application for listener dispatch.
        path: Vec<GuiNodeId>,
    },
    /// Keyboard focus moved, or cleared when None.
    FocusChanged {
        /// New focus, or None after blur.
        focus: Option<GuiInputFocus>,
    },
    /// Pointer hover moved, or cleared when the target is None.
    HoverChanged {
        /// Platform pointer identity.
        pointer: u32,
        /// Hovered node, or None when nothing is hovered.
        target: Option<GuiInputTarget>,
        /// Final-logical point that selected the hover.
        position: [f32; 2],
    },
    /// An input-owned scroll offset accumulated without reflow.
    ScrollChanged {
        /// Panel entity.
        entity: EntityId,
        /// Consuming ScrollView.
        node: GuiNodeId,
        /// Accumulated offset in logical units.
        offset: [f32; 2],
    },
    /// The item range a VirtualList wants declared changed: its visible
    /// items widened by the overscan, published when the offset, viewport,
    /// item properties or measurements move it, on attach and after restore.
    /// Runtime-originated, so its session is zero.
    VirtualRangeChanged {
        /// Panel entity.
        entity: EntityId,
        /// VirtualList node.
        node: GuiNodeId,
        /// First wanted item index.
        first: u32,
        /// One past the last wanted item index; equal to `first` when empty.
        last: u32,
        /// Publication revision per list, starting at 1 and monotonic while
        /// the list's root incarnation lives.
        revision: u32,
    },
}

/// What produced one committed control value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiCommitSource {
    /// Routed pointer, key, text or composition input.
    User,
    /// A semantic action through the validated control path.
    Semantic,
    /// An explicit revision-aware external replacement
    /// ([`GuiCommand::SetControlValue`](crate::GuiCommand)).
    External,
}

/// One routed input cancelled between routing and application.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiInputCancellation {
    /// Session that supplied the source input.
    pub session: u64,
    /// Frame whose snapshot the input routed against.
    pub source_tick: u64,
    /// Frame whose mutation boundary observed the loss.
    pub effect_tick: u64,
    /// Intended target, when routing reached one.
    pub target: Option<GuiInputTarget>,
    /// Why the routed intent never applied.
    pub reason: GuiInputCancelReason,
}

/// Why a routed intent never applied. Never mixed with effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiInputCancelReason {
    /// The target node, root or entity went away before application.
    TargetRemoved,
    /// The target became hidden, unavailable or disabled before application.
    TargetHidden,
    /// The owning session was replaced before application.
    SessionReplaced,
    /// The pointer gesture was cancelled or released off-target.
    GestureCancelled,
}

/// One routed input that lost arbitration or admission at application.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiInputConflict {
    /// Session that supplied the source input.
    pub session: u64,
    /// Frame whose snapshot the input routed against.
    pub source_tick: u64,
    /// Frame whose mutation boundary observed the conflict.
    pub effect_tick: u64,
    /// Intended target, when routing reached one.
    pub target: Option<GuiInputTarget>,
    /// Why the routed intent could not apply cleanly.
    pub reason: GuiInputConflictReason,
}

/// Why a routed intent could not apply cleanly. Reported separately from
/// effects and cancellations.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiInputConflictReason {
    /// The committed control revision moved between routing and application.
    RevisionMismatch {
        /// Revision the envelope expected.
        expected: u32,
        /// Revision actually committed.
        found: u32,
    },
    /// The staged commit was refused at the mutation boundary.
    AdmissionFailed(ErrorReason),
    /// Another pointer already presses this control.
    TouchArbitration {
        /// Pointer holding the press.
        owner_pointer: u32,
    },
    /// A fenced text command named another input context, focus generation
    /// or target than the current focus.
    FocusMismatch,
}

/// One well-formed input that reached no GUI target, observable by scene
/// controls. Each source input appears here at most once and never
/// alongside an effect.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiUnhandledInput {
    /// Session that supplied the source input.
    pub session: u64,
    /// Correlated Host request that supplied the input, or zero for direct
    /// fire-and-forget ingress. This identity is internal routing metadata;
    /// supplier-private observation codecs intentionally omit it.
    pub source_request_id: u64,
    /// Frame whose snapshot the input routed against.
    pub tick: u64,
    /// Source input that reached no target.
    pub input: GuiInputCommand,
    /// Why routing reached no target.
    pub reason: GuiUnhandledReason,
}

/// Why routing reached no target.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiUnhandledReason {
    /// No live panel contained the point.
    NoPanelHit,
    /// An explicit scene blocker won the distance compare.
    Blocked {
        /// Winning blocker entity.
        entity: EntityId,
    },
    /// The snapshot target died before routing could fence it.
    StaleTarget,
    /// A key or text input arrived with no keyboard focus.
    NoFocus,
    /// A pointer release arrived with no capture.
    NoCapture,
    /// The routed node cannot consume this input.
    NotFocusable,
    /// Another session owns the single active input context.
    NotOwner,
    /// No ScrollView consumed the scroll movement: the point has no eligible
    /// ScrollView ancestry, or every ScrollView in it already sits at its
    /// edge in the scroll's direction. Scene controls may take the scroll.
    ScrollUnconsumed,
}

/// Control behaviour of one routed node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlKind {
    /// Momentary press completing on release over the same node.
    Button,
    /// Toggles on press.
    Checkbox,
    /// Continuous value set from the pointer fraction or keys.
    Slider,
    /// Receives focus and text edits.
    TextInput,
}

impl ControlKind {
    fn of<S>(data: &GuiNodeData<S>) -> Option<Self> {
        match data {
            GuiNodeData::Button {
                ..
            } => Some(Self::Button),
            GuiNodeData::Checkbox => Some(Self::Checkbox),
            GuiNodeData::Slider => Some(Self::Slider),
            GuiNodeData::TextInput {
                ..
            } => Some(Self::TextInput),
            _ => None,
        }
    }
}

/// Owner of the single active input context: every focus, capture,
/// composition and pending action belongs to exactly one session. The epoch
/// fences delayed work across replacement and reconnect: acquiring,
/// replacing or releasing the context bumps it, and envelopes apply only
/// while both session and epoch still match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuiInputOwner {
    /// Session holding the context.
    session: u64,
    /// Context generation at acquisition.
    epoch: u64,
}

/// One well-formed input admitted during mutation, awaiting routing after
/// the current layout and final camera/geometry evaluation.
struct QueuedInput {
    /// Session that supplied the input.
    session: u64,
    /// Correlated Host request, or zero for direct ingress.
    request_id: u64,
    /// Input to route in admission order.
    command: GuiInputCommand,
}

/// Correlated raw GUI input passed through the generic System command boundary.
#[derive(Clone)]
struct CorrelatedGuiInputCommand {
    request_id: u64,
    command: GuiInputCommand,
}

/// One routed intent awaiting the next mutation boundary.
struct PendingEnvelope {
    /// Session that supplied the source input.
    session: u64,
    /// Owner epoch at routing; fences replacement and reconnect races.
    epoch: u64,
    /// Frame whose snapshot the input routed against.
    source_tick: u64,
    /// Intended target, or None for blur.
    target: Option<GuiInputTarget>,
    /// Pointer whose gesture produced this envelope; a pointer cancel
    /// cancels its pending envelopes.
    pointer: Option<u32>,
    /// Routed intent.
    kind: EnvelopeKind,
}

/// Routed intents applied in order with liveness revalidation.
enum EnvelopeKind {
    /// Revision-gated control commit.
    SetValue {
        /// Revision the routing snapshot predicted.
        expected_revision: u32,
        /// Value to commit.
        value: GuiControlValue,
        /// Routed user input or a semantic action.
        source: GuiCommitSource,
    },
    /// Completed button press.
    PressButton,
    /// Text input submission of the revision the routing snapshot predicted.
    Submit {
        /// Revision the submitted text must still hold at application.
        expected_revision: u32,
    },
    /// Focus move, or blur when None.
    Focus {
        /// New focus, or None after blur.
        focus: Option<GuiInputTarget>,
        /// Whether keyboard or programmatic focus earns the focus ring.
        /// Pointer-press focus takes semantic focus without the ring.
        keyboard: bool,
    },
    /// Input-owned scroll accumulation for one ScrollView, already
    /// clamped against its evaluated extents at routing and re-clamped at
    /// application.
    Scroll {
        /// Consumed delta in logical units.
        delta: [f32; 2],
    },
}

/// Predicted control state chaining same-tick envelopes against one snapshot.
#[derive(Clone, Debug, PartialEq)]
struct PredictedControl {
    /// Value the next envelope builds on.
    value: GuiControlValue,
    /// Revision the next envelope must expect.
    revision: u32,
}

/// Transient caret/selection for one single-line text input.
///
/// Byte offsets into the predicted/committed text at `revision`; both land on
/// grapheme boundaries. `anchor` is None while collapsed. The cursor never
/// enters snapshots, persistence or retained views: caret moves never reflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextCursor {
    /// Caret byte offset.
    caret: u32,
    /// Selection anchor byte offset, or None while collapsed.
    anchor: Option<u32>,
    /// Predicted/committed revision the offsets apply to.
    revision: u32,
    /// Session that set the cursor.
    session: u64,
}

impl TextCursor {
    /// Normalized selection range, or None while collapsed.
    fn selection(self) -> Option<(u32, u32)> {
        match self.anchor {
            Some(anchor) if anchor != self.caret => {
                Some(super::text_edit::normalize_range(anchor, self.caret))
            }
            _ => None,
        }
    }
}

/// Provisional composition payload for one routed IME update.
/// Groups the borrowed text with its in-provisional caret range so routing
/// stays under the argument limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CompositionUpdate<'a> {
    /// Provisional IME text.
    text: &'a str,
    /// Provisional selection anchor within `text`.
    caret_start: u32,
    /// Provisional caret within `text`.
    caret_end: u32,
    /// Focus and revision the sender observed.
    fence: Option<&'a GuiTextFence>,
}

/// Shape validation for provisional IME updates, shared by admission (the
/// command boundary) and routing. Needs no world state.
fn validate_composition_shape(
    text: &str,
    caret_start: u32,
    caret_end: u32,
) -> Result<(), ErrorReason> {
    if text.len() > super::super::tree::nodes::MAX_TEXT_BYTES {
        return Err(ErrorReason::InvalidValue);
    }
    if caret_start > text.len() as u32
        || caret_end > text.len() as u32
        || !super::text_edit::is_boundary(text, caret_start)
        || !super::text_edit::is_boundary(text, caret_end)
    {
        return Err(ErrorReason::InvalidValue);
    }
    Ok(())
}

/// Best projected candidate: world distance, panel, node hit, root
/// incarnation, retained rectangle and mapped logical point.
type ProjectedCandidate = (f64, EntityId, super::super::GuiHit, u64, [f32; 4], [f32; 2]);

/// Topmost scroll-aware hit on one panel: an evaluated node with its
/// scrolled rectangle, or a ScrollView scroll bar part painted above it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollViewHit {
    /// Hit node; the ScrollView itself for a scroll bar hit.
    node: GuiNodeId,
    /// Scrolled rectangle of the hit node.
    rect: [f32; 4],
    /// Scroll bar part and scrolled bar geometry under the point.
    bar: Option<(GuiPrimitivePart, GuiScrollBar)>,
}

/// Scroll-aware hit resolved across panels: panel entity, root
/// incarnation, hit and the logical point.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollResolved {
    entity: EntityId,
    root_incarnation: u64,
    hit: ScrollViewHit,
    point: [f32; 2],
}

/// Best scroll candidate: world/entity distance first, then the resolved hit.
type ScrollCandidate = (f64, EntityId, ScrollViewHit, u64, [f32; 2]);

/// Current-tick camera ray for viewport-space pointer routing. The origin
/// and unit direction live in World space; `near` and `far` bound the ray
/// parameter in the same units, so panel and blocker distances compare
/// directly.
#[derive(Clone, Copy, Debug)]
struct ProjectedRay {
    /// World-space ray origin.
    origin: [f64; 3],
    /// World-space unit direction.
    direction: [f64; 3],
    /// Nearest accepted ray parameter in World units.
    near: f64,
    /// Farthest accepted ray parameter in World units.
    far: f64,
}

/// One pointer's capture/press cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PointerCapture {
    /// Captured node.
    target: GuiInputTarget,
    /// Pressed button; releases of other buttons never complete the press.
    button: GuiPointerButton,
    /// Session that pressed.
    session: u64,
    /// Frame whose snapshot established the capture.
    source_tick: u64,
}

/// One pointer's retained hover observation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HoverCursor {
    target: GuiInputTarget,
    session: u64,
    source_tick: u64,
    position: [f32; 2],
}

/// One primary press inside eligible ScrollView ancestry. The drag stays
/// pending until its travel passes the slop; the nearest ScrollView with
/// capacity on the dominant axis then wins, and later travel scrolls that
/// ScrollView and its outer chain.
#[derive(Clone, Debug, PartialEq)]
struct ScrollDrag {
    /// Panel entity the press routed to.
    entity: EntityId,
    /// Root incarnation fencing the chain.
    root_incarnation: u64,
    /// Innermost-first eligible ScrollView ancestry of the pressed point.
    chain: Vec<GuiNodeId>,
    /// Final-logical press point.
    origin: [f32; 2],
    /// Final-logical point the latest scroll consumed up to.
    last: [f32; 2],
    /// Chain index of the winning ScrollView once the drag passed the slop.
    active: Option<usize>,
    /// Session that pressed.
    session: u64,
}

impl ScrollDrag {
    /// Whether this drag scrolls one fenced ScrollView target.
    fn scrolls(&self, target: &GuiInputTarget) -> bool {
        self.entity == target.entity
            && self.root_incarnation == target.root_incarnation
            && self.chain.contains(&target.node)
    }
}

/// One pointer holding a ScrollView scroll bar: dragging its thumb, or
/// holding a track press that already paged.
#[derive(Clone, Debug, PartialEq)]
struct ScrollBarPress {
    /// Pressed ScrollView.
    target: GuiInputTarget,
    /// Pressed track or thumb part.
    part: GuiPrimitivePart,
    /// Innermost-first eligible ScrollView chain from the pressed view.
    chain: Vec<GuiNodeId>,
    /// Pointer distance past the thumb start along the bar axis.
    grab: f32,
    /// Session that pressed.
    session: u64,
}

/// One pointer hovering a ScrollView scroll bar part.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollBarHover {
    /// Hovered ScrollView.
    target: GuiInputTarget,
    /// Hovered track or thumb part.
    part: GuiPrimitivePart,
    /// Session that hovered.
    session: u64,
}

/// Outcome of one pointer move for a pointer's scroll drag.
enum ScrollDragStep {
    /// The drag owns the gesture; the move is consumed.
    Scrolling,
    /// Travel stays within the slop or no ScrollView can take it yet.
    Pending,
    /// The panel or its incarnation went away; the drag ends.
    Lost,
}

/// Input-owned scroll state with enough provenance to publish invalidation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ScrollCursor {
    pub(super) offset: [f32; 2],
    pub(super) session: u64,
    pub(super) source_tick: u64,
}

/// Ordered GUI input routing pass. See the module documentation for the
/// routing/application contract.
pub struct GuiInputSystem {
    /// Authored edit owner whose accepted external replacements this system
    /// publishes and fences the focused text against.
    gui: Option<SystemDependencyBinding<GuiSystem>>,
    /// Sequence of the next external replacement to observe.
    external_commit_next: u64,
    /// Retained layout views borrowed during routing.
    layout: Option<SystemDependencyBinding<GuiLayoutSystem>>,
    /// Active camera and host viewport borrowed during routing. Projection
    /// reads current-tick camera state; routing never mutates it.
    camera: Option<SystemDependencyBinding<crate::systems::camera::CameraSystem>>,
    /// Ordered ingress admitted during mutation, routed after evaluation.
    ingress: Vec<QueuedInput>,
    /// Session owning the single active input context, if any.
    owner: Option<GuiInputOwner>,
    /// Current context generation; bumped on every ownership change.
    owner_epoch: u64,
    /// Keyboard focus cursor, updated during routing for same-tick chaining.
    focus: Option<GuiInputFocus>,
    /// Whether the focus cursor earns the focus ring: keyboard and
    /// programmatic focus show it, pointer-press focus does not. Semantic
    /// focus (activation, nudging, caret) follows every source.
    focus_visible: bool,
    /// Panel and root incarnation that most recently held keyboard focus in
    /// this context: the entry panel for traversal from no focus when no
    /// camera orders the panels. A hint revalidated against the current
    /// traversal order on use.
    keyboard_panel: Option<(EntityId, u64)>,
    /// Frame the focus cursor was set against.
    focus_tick: u64,
    /// Monotonic fence for focus/blur, focused-node lifetime changes and
    /// external replacement of the focused text.
    focus_generation: u64,
    /// Per-pointer capture/press cursors.
    captures: BTreeMap<u32, PointerCapture>,
    /// Per-pointer hover cursors with the session that set them.
    hovers: BTreeMap<u32, HoverCursor>,
    /// Controls pressed by pointer, for touch arbitration.
    press_owners: BTreeMap<GuiInputTarget, u32>,
    /// Pending and active scroll drags per pointer.
    scroll_drags: BTreeMap<u32, ScrollDrag>,
    /// Scroll bar thumb drags and held track presses per pointer.
    scroll_bar_presses: BTreeMap<u32, ScrollBarPress>,
    /// Scroll bar parts hovered per pointer.
    scroll_bar_hovers: BTreeMap<u32, ScrollBarHover>,
    /// Ordered routed intents awaiting the next mutation boundary.
    envelopes: Vec<PendingEnvelope>,
    /// Predicted control state chaining same-tick envelopes.
    predicted: BTreeMap<GuiInputTarget, PredictedControl>,
    /// Input-owned scroll offsets in logical units, never reflowed. A
    /// VirtualList's offset follows its persisted anchor.
    pub(super) scroll_offsets: BTreeMap<GuiInputTarget, ScrollCursor>,
    /// VirtualList anchors awaiting persistence and published ranges.
    pub(super) virtual_scroll: super::virtual_scroll::GuiVirtualScroll,
    /// Routed scroll offsets chaining same-tick scroll envelopes: each
    /// ScrollView's committed offset plus the deltas already queued for it.
    /// Dropped when those envelopes apply or cancel.
    scroll_predicted: BTreeMap<GuiInputTarget, [f32; 2]>,
    /// Revision of input-owned scroll offsets. Bumped only when an applied
    /// scroll actually moves an offset, so paint can refresh on scroll
    /// without reflowing layout.
    pub(super) scroll_revision: u64,
    /// Transient caret/selection per text input, never reflowed.
    text_carets: BTreeMap<GuiInputTarget, TextCursor>,
    /// Active provisional IME composition, never committed until explicit commit.
    composition: Option<super::composition::ActiveComposition>,
    /// Transient caret/selection/composition generation. Bumped on every
    /// cursor or provisional change so derived paint refreshes without
    /// touching committed state or retained views.
    caret_revision: u64,
    /// Last transient generation detached into a frame report.
    published_caret_revision: u64,
    /// Session that owned the last published focused text state.
    published_text_session: Option<u64>,
    /// Last frame observed, fencing session-replacement records.
    last_tick: u64,
    /// Routed effects awaiting the frame report.
    pub(super) pending_effects: Vec<GuiInputEffect>,
    /// Routed cancellations awaiting the frame report.
    pending_cancellations: Vec<GuiInputCancellation>,
    /// Routed conflicts awaiting the frame report.
    pending_conflicts: Vec<GuiInputConflict>,
    /// Routed unhandled inputs awaiting the frame report.
    pending_unhandled: Vec<GuiUnhandledInput>,
    /// Request identity of the input currently routing synchronously.
    routing_request_id: u64,
}

impl GuiInputSystem {
    /// Stable system identity.
    pub const ID: SystemId = SystemId("ipp.gui-input");
}

crate::system_parameter!(GuiInputSystem);

/// Factory for [`GuiInputSystem`].
#[derive(Default)]
pub struct GuiInputSystemFactory;

impl SystemFactory for GuiInputSystemFactory {
    fn id(&self) -> SystemId {
        GuiInputSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(GuiSystem::ID),
            SystemDependency::Required(GuiLayoutSystem::ID),
            SystemDependency::Required(crate::systems::camera::CameraSystem::ID),
            SystemDependency::Required(crate::systems::geometry::GeometrySystem::ID),
        ]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        let layout = context.dependency::<GuiLayoutSystem>(GuiLayoutSystem::ID)?;
        // Enforce concrete implementations for the remaining declared
        // predecessors; routing runs after their evaluation and observes the
        // current layout snapshot, while camera and geometry presence keeps
        // the host projection/blocker contract behind every routed pointer
        // total.
        let gui = context.dependency::<GuiSystem>(GuiSystem::ID)?;
        let camera = context.dependency::<crate::systems::camera::CameraSystem>(
            crate::systems::camera::CameraSystem::ID,
        )?;
        context.dependency::<crate::systems::geometry::GeometrySystem>(
            crate::systems::geometry::GeometrySystem::ID,
        )?;
        Ok(Box::new(GuiInputSystem {
            gui: Some(gui),
            external_commit_next: 0,
            layout: Some(layout),
            camera: Some(camera),
            ingress: Vec::new(),
            owner: None,
            owner_epoch: 0,
            focus: None,
            focus_visible: false,
            keyboard_panel: None,
            focus_tick: 0,
            focus_generation: 0,
            captures: BTreeMap::new(),
            hovers: BTreeMap::new(),
            press_owners: BTreeMap::new(),
            scroll_drags: BTreeMap::new(),
            scroll_bar_presses: BTreeMap::new(),
            scroll_bar_hovers: BTreeMap::new(),
            envelopes: Vec::new(),
            predicted: BTreeMap::new(),
            scroll_offsets: BTreeMap::new(),
            virtual_scroll: Default::default(),
            scroll_predicted: BTreeMap::new(),
            scroll_revision: 0,
            text_carets: BTreeMap::new(),
            composition: None,
            caret_revision: 0,
            published_caret_revision: 0,
            published_text_session: None,
            last_tick: 0,
            pending_effects: Vec::new(),
            pending_cancellations: Vec::new(),
            pending_conflicts: Vec::new(),
            pending_unhandled: Vec::new(),
            routing_request_id: 0,
        }))
    }
}

// ---------------------------------------------------------------------------
// Routing-time observation helpers.
// ---------------------------------------------------------------------------

/// One hit node rechecked against authoritative producer state: live root,
/// matching incarnation and node, and positive opacity. Snapshot
/// geometry stays immutable; only producer liveness is rechecked.
struct RecheckedHit<'a> {
    /// Fenced routed target.
    target: GuiInputTarget,
    /// Final-logical rectangle from the routing snapshot.
    rect: [f32; 4],
    /// GUI-logical point the hit was decided at: the input position on the
    /// logical path, the ray-mapped panel point on the projected path.
    /// Every downstream consumer (hover, caret, slider fractions) observes
    /// this point, never the raw viewport input.
    point: [f32; 2],
    /// Authoritative control behaviour, or None for non-control nodes.
    kind: Option<ControlKind>,
    /// Producer root the recheck observed.
    root: Cow<'a, GuiRoot>,
}

fn recheck_hit<'a>(
    sim: &'a WorldSimulationState,
    layout: &GuiLayoutSystem,
    target: GuiInputTarget,
    rect: [f32; 4],
    point: [f32; 2],
) -> Option<RecheckedHit<'a>> {
    let GuiTargetStatus::Eligible(root) = evaluated_status(sim, layout, &target) else {
        return None;
    };
    let live = root.nodes().node(target.node)?;
    let kind = ControlKind::of(&live.data);
    Some(RecheckedHit {
        target,
        rect,
        point,
        kind,
        root,
    })
}

impl GuiInputSystem {
    /// Borrow the retained layout pass for routing.
    pub(super) fn layout<'a>(
        &self,
        access: &'a SystemRuntimeAccess<'a>,
    ) -> Result<&'a GuiLayoutSystem, ErrorReason> {
        let binding = self.layout.ok_or(ErrorReason::InvalidValue)?;
        access.dependency(binding).ok_or(ErrorReason::InvalidValue)
    }

    /// Borrow the retained views and producer state together for one
    /// routed command. Both borrows are shared: the snapshot cannot change
    /// during the mutation drain.
    fn route_with_layout(
        &mut self,
        access: &SystemRuntimeAccess<'_>,
        route: impl FnOnce(&mut Self, &GuiLayoutSystem, &WorldSimulationState),
    ) -> Result<(), ErrorReason> {
        let layout = self.layout(access)?;
        let sim: &WorldSimulationState = &*access.world;
        route(self, layout, sim);
        Ok(())
    }

    /// Record one input that reached no target.
    fn unhandled(
        &mut self,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        reason: GuiUnhandledReason,
    ) {
        self.pending_unhandled.push(GuiUnhandledInput {
            session,
            source_request_id: self.routing_request_id,
            tick,
            input: input.clone(),
            reason,
        });
    }

    /// Record one arbitration or admission conflict at routing time.
    fn conflict(
        &mut self,
        session: u64,
        tick: u64,
        target: Option<GuiInputTarget>,
        reason: GuiInputConflictReason,
    ) {
        self.pending_conflicts.push(GuiInputConflict {
            session,
            source_tick: tick,
            effect_tick: tick,
            target,
            reason,
        });
    }

    /// Queue one ordered envelope, bounding the deferred queue. Overflow
    /// is a conflict, never silent loss: the intent is reported with the
    /// session it already carries.
    fn push_envelope(&mut self, tick: u64, envelope: PendingEnvelope) {
        if self.envelopes.len() >= MAX_PENDING_ENVELOPES {
            self.pending_conflicts.push(GuiInputConflict {
                session: envelope.session,
                source_tick: envelope.source_tick,
                effect_tick: tick,
                target: envelope.target,
                reason: GuiInputConflictReason::AdmissionFailed(ErrorReason::Capacity),
            });
            return;
        }
        self.envelopes.push(envelope);
    }

    /// Hit-test one point across the selected panels and fence the result
    /// against producer state and scene blockers. A current-tick camera ray
    /// selects the nearest front-facing panel with geometry-resolved
    /// blockers; without one, the point routes as GUI-logical with the
    /// overlay panel nearest and the greatest entity on top.
    #[allow(clippy::too_many_arguments)]
    fn route_point<'s>(
        &self,
        layout: &GuiLayoutSystem,
        sim: &'s WorldSimulationState,
        panel: Option<EntityId>,
        position: [f32; 2],
        blockers: &[super::super::GuiBlockerHit],
        panel_distance: Option<f32>,
        projection: Option<ProjectedRay>,
    ) -> Result<RecheckedHit<'s>, GuiUnhandledReason> {
        if let Some(ray) = projection {
            return self.route_point_projected(layout, sim, panel, blockers, ray);
        }

        self.route_point_logical(layout, sim, panel, position, blockers, panel_distance)
    }

    /// Scroll-aware hit test for one panel: the retained snapshot test
    /// while scrolled subtrees translate by ancestor offsets. Without any
    /// offset on the panel this is exactly the retained test, so unscrolled
    /// routing behaves bit-identically; projection, blockers, ties and the
    /// control-only fence around the call sites never change.
    fn hit_test_scrolled(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        entity: EntityId,
        point: [f32; 2],
    ) -> Option<GuiHit> {
        let view = layout.view(entity)?;
        let moved = self
            .scroll_offsets
            .iter()
            .any(|(target, cursor)| target.entity == entity && cursor.offset != [0.0, 0.0]);
        if !moved {
            return view.hit_test(point);
        }
        let (_, root) = producer_root(sim, entity)?;
        self.scroll_hit_in_view(view, &root, entity, point, false)
            .map(|hit| GuiHit {
                node: hit.node,
                position: point,
            })
    }

    /// Final-logical scroll shift for one node, or zero. Fail-open without
    /// producer or view state, so slider fractions and caret pens keep their
    /// retained mapping outside scrolled subtrees.
    fn scroll_shift_for(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        entity: EntityId,
        node: GuiNodeId,
    ) -> [f32; 2] {
        let (Some(view), Some((_, root))) = (layout.view(entity), producer_root(sim, entity))
        else {
            return [0.0, 0.0];
        };
        self.ancestor_shift(view, &root, entity, node)
    }

    /// Hit-test one GUI-logical point across the selected panels and fence
    /// the topmost hit (greatest entity) against producer state and caller
    /// scene blockers. Native and headless routing without a camera.
    fn route_point_logical<'s>(
        &self,
        layout: &GuiLayoutSystem,
        sim: &'s WorldSimulationState,
        panel: Option<EntityId>,
        position: [f32; 2],
        blockers: &[super::super::GuiBlockerHit],
        panel_distance: Option<f32>,
    ) -> Result<RecheckedHit<'s>, GuiUnhandledReason> {
        let mut best: Option<(EntityId, super::super::GuiHit, u64, [f32; 4])> = None;
        let panels: Vec<EntityId> = match panel {
            Some(entity) => vec![entity],
            None => layout.evaluated_entities(),
        };
        for entity in panels {
            let Some(view) = layout.view(entity) else {
                continue;
            };
            let Some(hit) = self.hit_test_scrolled(layout, sim, entity, position) else {
                continue;
            };
            let replace = best.is_none_or(|(current, _, _, _)| entity > current);
            if replace {
                // The retained rectangle travels with the hit so later
                // pointer fractions observe the routing snapshot, never a
                // reflowed layout.
                let rect = view
                    .nodes
                    .iter()
                    .find(|node| node.node == hit.node)
                    .map(|node| node.rect)
                    .unwrap_or([0.0, 0.0, 0.0, 0.0]);
                best = Some((entity, hit, view.root_incarnation, rect));
            }
        }
        let Some((entity, hit, root_incarnation, rect)) = best else {
            return Err(GuiUnhandledReason::NoPanelHit);
        };
        // Without a panel distance the overlay panel counts as nearest and
        // keeps the hit; otherwise explicit blockers may win the compare. A
        // missing or non-finite distance is an observable miss.
        // A live non-control node is observable but not focusable; a dead
        // one is stale.
        let fence = |hit: Option<RecheckedHit<'s>>| match hit {
            Some(routed) if routed.kind.is_some() => Ok(routed),
            Some(_) => Err(GuiUnhandledReason::NotFocusable),
            None => Err(GuiUnhandledReason::StaleTarget),
        };
        if let Some(distance) = panel_distance {
            match super::super::resolve_panel_hit(Some(distance), Some(hit), blockers) {
                super::super::GuiPanelResolution::Panel(decided) => {
                    // The decided hit keeps the topmost panel's rectangle;
                    // arbitration never fabricates a different node.
                    return fence(recheck_hit(
                        sim,
                        layout,
                        GuiInputTarget {
                            entity,
                            root_incarnation,
                            node: decided.node,
                        },
                        rect,
                        position,
                    ));
                }
                super::super::GuiPanelResolution::Blocked {
                    entity,
                } => {
                    return Err(GuiUnhandledReason::Blocked {
                        entity,
                    });
                }
                super::super::GuiPanelResolution::Miss => {
                    return Err(GuiUnhandledReason::NoPanelHit);
                }
            }
        }
        fence(recheck_hit(
            sim,
            layout,
            GuiInputTarget {
                entity,
                root_incarnation,
                node: hit.node,
            },
            rect,
            position,
        ))
    }

    /// Current-tick camera ray for one normalized viewport point, or None
    /// for logical routing without a camera. The ray shares the geometry
    /// pick convention: normalized top-left viewport coordinates, +Y down,
    /// with the host viewport supplying the projection aspect. Reads the
    /// evaluated camera and viewport; routing never mutates them.
    fn project_ray(
        &self,
        access: &SystemRuntimeAccess<'_>,
        sim: &WorldSimulationState,
        position: [f32; 2],
    ) -> Option<ProjectedRay> {
        let binding = self.camera?;
        let camera = access.dependency(binding)?;
        let read = camera.read(sim);
        let entity = read.active_camera()?;
        let (width, height) = read.render_viewport()?;
        if width == 0 || height == 0 {
            return None;
        }
        if !position.iter().all(|value| value.is_finite()) {
            return None;
        }
        let index = entity.index() as usize;
        let lens = *sim.components.camera(index)?;
        if !lens.near.is_finite()
            || !lens.far.is_finite()
            || lens.near <= 0.0
            || lens.far <= lens.near
        {
            return None;
        }
        let affine = crate::systems::hierarchy::evaluated_affine(sim, entity).ok()?;
        let aspect = f64::from(width) / f64::from(height);
        let x = f64::from(position[0]) * 2.0 - 1.0;
        let y = 1.0 - f64::from(position[1]) * 2.0;
        let (origin, direction) = if lens.projection == 0 {
            if !lens.fov_y.is_finite() || lens.fov_y <= 0.0 || lens.fov_y >= std::f32::consts::PI {
                return None;
            }
            let extent = (f64::from(lens.fov_y) * 0.5).tan();
            (
                affine.point([0.0; 3]),
                affine.vector([x * extent * aspect, y * extent, -1.0]),
            )
        } else {
            if !lens.ortho_height.is_finite() || lens.ortho_height <= 0.0 {
                return None;
            }
            let extent = f64::from(lens.ortho_height) * 0.5;
            (
                affine.point([x * extent * aspect, y * extent, 0.0]),
                affine.vector([0.0, 0.0, -1.0]),
            )
        };
        let length =
            direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2];
        let length = length.sqrt();
        if !length.is_finite() || length <= 0.0 {
            return None;
        }
        Some(ProjectedRay {
            origin,
            direction: [
                direction[0] / length,
                direction[1] / length,
                direction[2] / length,
            ],
            // The direction above is unit length, so distances along the ray
            // are already World units: the lens range applies unscaled.
            near: f64::from(lens.near),
            far: f64::from(lens.far),
        })
    }

    /// Map one camera ray onto a panel's GUI-logical plane point: the world
    /// distance alongside the logical point. The Surface content rectangle
    /// maps to the centred entity-local XY plane with +Z facing forward;
    /// the ray pulls back front-face only inside the camera range, then the
    /// content point scales through per-panel units. Ordinary hit testing
    /// requires the finite Surface rectangle, while a retained capture may
    /// continue across its owning panel's infinite plane so controls can
    /// clamp or cancel against an out-of-bounds logical point.
    fn project_panel_point(
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        entity: EntityId,
        ray: &ProjectedRay,
        require_surface_bounds: bool,
    ) -> Option<(f64, [f32; 2])> {
        let view = layout.view(entity)?;
        if !view.available {
            return None;
        }
        let index = entity.index() as usize;
        let surface = sim.components.surface(index)?;
        let affine = crate::systems::hierarchy::evaluated_affine(sim, entity).ok()?;
        let geometry_ray = GeometryRay {
            origin: ray.origin,
            direction: ray.direction,
        };
        let local = affine.inverse_ray(&geometry_ray);
        if local.direction[2] >= 0.0 {
            return None;
        }
        let distance = -local.origin[2] / local.direction[2];
        if !(distance >= ray.near && distance <= ray.far) {
            return None;
        }
        let world_hit = [
            ray.origin[0] + ray.direction[0] * distance,
            ray.origin[1] + ray.direction[1] * distance,
            ray.origin[2] + ray.direction[2] * distance,
        ];
        let local_hit = affine.inverse_point(world_hit);
        let content = [
            local_hit[0] + f64::from(surface.width) * 0.5,
            f64::from(surface.height) * 0.5 - local_hit[1],
        ];
        if !content[0].is_finite() || !content[1].is_finite() {
            return None;
        }
        if require_surface_bounds
            && (content[0] < 0.0
                || content[1] < 0.0
                || content[0] > f64::from(surface.width)
                || content[1] > f64::from(surface.height))
        {
            return None;
        }
        if !(view.units_per_metre.is_finite() && view.units_per_metre > 0.0) {
            return None;
        }
        let units = f64::from(view.units_per_metre);
        let logical = [(content[0] * units) as f32, (content[1] * units) as f32];
        if !logical.iter().all(|value| value.is_finite()) {
            return None;
        }
        Some((distance, logical))
    }

    /// Remap one raw input point onto an owned panel through the current
    /// camera ray. Capture continuations (drags, captured hovers) reuse the
    /// press-time target while the camera may have moved since the press
    /// routed. A projected ray uses the panel's unbounded plane: raw viewport
    /// coordinates are a different domain and must never stand in for a
    /// failed projection.
    fn remap_capture_point(
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        target: &GuiInputTarget,
        position: [f32; 2],
        projection: &Option<ProjectedRay>,
    ) -> Option<[f32; 2]> {
        match projection {
            Some(ray) => Self::project_panel_point(layout, sim, target.entity, ray, false)
                .map(|(_, point)| point),
            None => Some(position),
        }
    }

    /// Hit-test one viewport point across the selected panels through the
    /// current-tick camera ray. Transformed Surfaces intersect in World
    /// space; front faces only, nearest eligible hit wins with deterministic
    /// entity ties, and marked blocker distances resolve from current-tick
    /// scene geometry. Caller distances are stale by construction and never
    /// reused here; marked entities without evaluated picking geometry
    /// cannot block.
    fn route_point_projected<'s>(
        &self,
        layout: &GuiLayoutSystem,
        sim: &'s WorldSimulationState,
        panel: Option<EntityId>,
        blockers: &[super::super::GuiBlockerHit],
        ray: ProjectedRay,
    ) -> Result<RecheckedHit<'s>, GuiUnhandledReason> {
        let geometry_ray = GeometryRay {
            origin: ray.origin,
            direction: ray.direction,
        };
        let mut best: Option<ProjectedCandidate> = None;
        let panels: Vec<EntityId> = match panel {
            Some(entity) => vec![entity],
            None => layout.evaluated_entities(),
        };
        for entity in panels {
            let Some((distance, logical)) =
                Self::project_panel_point(layout, sim, entity, &ray, true)
            else {
                continue;
            };
            let Some(view) = layout.view(entity) else {
                continue;
            };
            let Some(hit) = self.hit_test_scrolled(layout, sim, entity, logical) else {
                continue;
            };
            // The retained rectangle travels with the hit so later pointer
            // fractions observe the routing snapshot, never a reflowed layout.
            let rect = view
                .nodes
                .iter()
                .find(|node| node.node == hit.node)
                .map(|node| node.rect)
                .unwrap_or([0.0, 0.0, 0.0, 0.0]);
            let better = match best {
                None => true,
                Some((current_distance, current, _, _, _, _)) => {
                    distance < current_distance
                        || (distance == current_distance && entity < current)
                }
            };
            if better {
                best = Some((distance, entity, hit, view.root_incarnation, rect, logical));
            }
        }
        let Some((distance, entity, hit, root_incarnation, rect, logical)) = best else {
            return Err(GuiUnhandledReason::NoPanelHit);
        };
        let mut resolved = Vec::with_capacity(blockers.len());
        for blocker in blockers {
            let index = blocker.entity.index() as usize;
            let Some(component) = sim.components.picking_geometry(index) else {
                continue;
            };
            let Some(evaluation) = component.runtime.evaluation.as_ref() else {
                continue;
            };
            let Ok(shape) = evaluation.evaluated() else {
                continue;
            };
            let mut intervals = Vec::new();
            if let Some(found) = shape.ray_intersection_with_scratch(
                &geometry_ray,
                ray.near,
                ray.far,
                &mut intervals,
            ) {
                let resolved_distance = found.distance as f32;
                if resolved_distance.is_finite() {
                    resolved.push(super::super::GuiBlockerHit {
                        distance: resolved_distance,
                        entity: blocker.entity,
                    });
                }
            }
        }
        // A live non-control node is observable but not focusable; a dead
        // one is stale.
        let fence = |hit: Option<RecheckedHit<'s>>| match hit {
            Some(routed) if routed.kind.is_some() => Ok(routed),
            Some(_) => Err(GuiUnhandledReason::NotFocusable),
            None => Err(GuiUnhandledReason::StaleTarget),
        };
        match super::super::resolve_panel_hit(Some(distance as f32), Some(hit), &resolved) {
            super::super::GuiPanelResolution::Panel(decided) => fence(recheck_hit(
                sim,
                layout,
                GuiInputTarget {
                    entity,
                    root_incarnation,
                    node: decided.node,
                },
                rect,
                logical,
            )),
            super::super::GuiPanelResolution::Blocked {
                entity,
            } => Err(GuiUnhandledReason::Blocked {
                entity,
            }),
            super::super::GuiPanelResolution::Miss => Err(GuiUnhandledReason::NoPanelHit),
        }
    }

    /// Revalidate a captured target without re-deciding it: captures stay
    /// on their panel under this system's own scope.
    fn revalidate_capture<'a>(
        &self,
        sim: &'a WorldSimulationState,
        target: &GuiInputTarget,
    ) -> Option<(ControlKind, Cow<'a, GuiRoot>)> {
        let GuiTargetStatus::Eligible(root) = producer_status(sim, target) else {
            return None;
        };
        let live = root.nodes().node(target.node)?;
        let kind = ControlKind::of(&live.data)?;
        Some((kind, root))
    }

    /// Bump the context generation. Every acquisition, replacement and
    /// release moves the epoch so delayed envelopes fence correctly.
    fn next_epoch(&mut self) -> u64 {
        self.owner_epoch = self.owner_epoch.saturating_add(1).max(1);
        self.owner_epoch
    }

    /// Require the caller's ownership for a context-establishing input,
    /// acquiring the unowned context. Reports `NotOwner` when another
    /// session holds it. Pointer presses, scrolls and programmatic focus
    /// acquire; hover-only moves and follow-on edits never do.
    fn check_owner(&mut self, session: u64, tick: u64, input: &GuiInputCommand) -> bool {
        match self.owner {
            Some(owner) if owner.session == session => true,
            Some(_) => {
                self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
                false
            }
            None => {
                let epoch = self.next_epoch();
                self.owner = Some(GuiInputOwner {
                    session,
                    epoch,
                });
                true
            }
        }
    }

    /// Explicitly replace the context owner after a validated acquisition
    /// gesture (programmatic focus with a session-fenced handle). The
    /// previous owner's in-flight envelopes cancel as session-replaced and
    /// its cursors release; delayed inputs from that session fence on the
    /// new epoch. A no-op when the caller already owns the context.
    fn replace_owner(&mut self, session: u64, tick: u64) {
        if let Some(owner) = self.owner
            && owner.session != session
        {
            self.last_tick = tick;
            self.release_session(owner.session);
        }
        if self.owner.is_none() {
            let epoch = self.next_epoch();
            self.owner = Some(GuiInputOwner {
                session,
                epoch,
            });
        }
    }

    /// Update one pointer's hover cursor, reporting genuine changes.
    fn set_hover(
        &mut self,
        session: u64,
        tick: u64,
        pointer: u32,
        hover: Option<GuiInputTarget>,
        position: [f32; 2],
    ) {
        let changed = match (self.hovers.get(&pointer).map(|cursor| cursor.target), hover) {
            (Some(current), Some(next)) => current != next,
            (None, None) => false,
            _ => true,
        };
        if !changed {
            if let Some(cursor) = self.hovers.get_mut(&pointer) {
                cursor.source_tick = tick;
                cursor.position = position;
            }
            return;
        }
        match hover {
            Some(target) => {
                self.hovers.insert(
                    pointer,
                    HoverCursor {
                        target,
                        session,
                        source_tick: tick,
                        position,
                    },
                );
            }
            None => {
                self.hovers.remove(&pointer);
            }
        }
        self.pending_effects.push(GuiInputEffect {
            session,
            source_tick: tick,
            effect_tick: tick,
            kind: GuiInputEffectKind::HoverChanged {
                pointer,
                target: hover,
                position,
            },
        });
    }

    /// Move keyboard focus, queueing the revalidated commit envelope.
    /// Focus moves fence provisional composition: a move away from the
    /// composed node (or to no focus) drops it, and blur collapses the old
    /// selection to its caret.
    fn set_focus(
        &mut self,
        session: u64,
        tick: u64,
        focus: Option<GuiInputTarget>,
        keyboard: bool,
    ) {
        self.update_focus_cursor(session, tick, focus, keyboard);
        self.push_envelope(
            tick,
            PendingEnvelope {
                session,
                epoch: self.owner_epoch,
                source_tick: tick,
                target: focus,
                pointer: None,
                kind: EnvelopeKind::Focus {
                    focus,
                    keyboard,
                },
            },
        );
    }

    /// Commit the transient focus cursor without enqueuing another envelope.
    fn update_focus_cursor(
        &mut self,
        session: u64,
        tick: u64,
        focus: Option<GuiInputTarget>,
        keyboard: bool,
    ) {
        let previous = self.focus.map(|focus| focus.target);
        self.focus = focus.map(|target| GuiInputFocus {
            target,
            session,
        });
        self.focus_visible = focus.is_some() && keyboard;
        self.focus_tick = tick;
        if let Some(target) = focus {
            self.keyboard_panel = Some((target.entity, target.root_incarnation));
        }
        if previous != focus {
            self.focus_generation = self.focus_generation.saturating_add(1).max(1);
            self.touch_caret();
        }
        if let Some(composed) = self.composition.clone()
            && Some(composed.target) != focus
        {
            self.clear_composition();
        }
        if focus.is_none()
            && let Some(old) = previous
            && let Some(cursor) = self.text_carets.get_mut(&old)
        {
            cursor.anchor = None;
        }
    }

    /// Current predicted or committed control state for envelope chaining.
    fn base_control(&self, root: &GuiRoot, target: &GuiInputTarget) -> (GuiControlValue, u32) {
        if let Some(predicted) = self.predicted.get(target) {
            return (predicted.value.clone(), predicted.revision);
        }
        match root.control_state(target.node) {
            Some(state) => (state.value.clone(), state.revision),
            None => (GuiControlValue::None, 0),
        }
    }

    /// Queue a revision-gated value envelope and advance the prediction.
    #[allow(clippy::too_many_arguments)]
    fn push_value_envelope(
        &mut self,
        session: u64,
        tick: u64,
        target: GuiInputTarget,
        pointer: Option<u32>,
        expected_revision: u32,
        value: GuiControlValue,
    ) {
        self.predicted.insert(
            target,
            PredictedControl {
                value: value.clone(),
                revision: expected_revision.saturating_add(1),
            },
        );
        self.push_envelope(
            tick,
            PendingEnvelope {
                session,
                epoch: self.owner_epoch,
                source_tick: tick,
                target: Some(target),
                pointer,
                kind: EnvelopeKind::SetValue {
                    expected_revision,
                    value,
                    source: GuiCommitSource::User,
                },
            },
        );
    }

    /// Current predicted/committed text plus revision for one text input.
    fn base_text(&self, root: &GuiRoot, target: &GuiInputTarget) -> Option<(String, u32)> {
        let (value, revision) = self.base_control(root, target);
        match value {
            GuiControlValue::Text(text) => Some((text, revision)),
            _ => None,
        }
    }

    /// Predicted text when a value envelope is pending, else the committed
    /// text from the authoritative root. None for non-text nodes.
    fn predicted_text_or_committed(
        &self,
        ctx: &crate::WorldContext<'_>,
        entity: EntityId,
        node: GuiNodeId,
    ) -> Option<(String, u32)> {
        let layout = ctx.system::<GuiLayoutSystem>(GuiLayoutSystem::ID)?;
        let target = current_target(ctx.world, layout, entity, node)?;
        if let Some(predicted) = self.predicted.get(&target) {
            if let GuiControlValue::Text(text) = &predicted.value {
                return Some((text.clone(), predicted.revision));
            }
            return None;
        }
        let root = ctx.gui_root(entity)?;
        let state = root.control_state(node)?;
        match &state.value {
            GuiControlValue::Text(text) => Some((text.clone(), state.revision)),
            _ => None,
        }
    }

    /// Fetch the transient cursor fenced to the current base revision.
    /// A stale revision (external reset) resets the caret to the end and
    /// clears the selection; offsets always snap to grapheme boundaries.
    fn cursor_for(
        &mut self,
        root: &GuiRoot,
        target: &GuiInputTarget,
        session: u64,
    ) -> Option<(String, u32, TextCursor)> {
        let (text, revision) = self.base_text(root, target)?;
        let key = *target;
        let stored = self.text_carets.get(&key).copied();
        // Cursors are session-fenced: a foreign session never inherits
        // another owner's caret, it restarts at the end of the text.
        let cursor = match stored {
            Some(cursor) if cursor.revision == revision && cursor.session == session => {
                let caret =
                    super::text_edit::snap_to_boundary(&text, cursor.caret.min(text.len() as u32));
                let anchor = cursor.anchor.map(|anchor| {
                    super::text_edit::snap_to_boundary(&text, anchor.min(text.len() as u32))
                });
                let snapped = TextCursor {
                    caret,
                    anchor: anchor.filter(|anchor| *anchor != caret),
                    revision,
                    session,
                };
                self.store_cursor(key, snapped);
                snapped
            }
            _ => {
                let caret = text.len() as u32;
                let fresh = TextCursor {
                    caret,
                    anchor: None,
                    revision,
                    session,
                };
                self.store_cursor(key, fresh);
                fresh
            }
        };
        Some((text, revision, cursor))
    }

    /// Place a collapsed caret, fenced to the base revision.
    fn place_caret(
        &mut self,
        root: &GuiRoot,
        target: &GuiInputTarget,
        session: u64,
        caret: u32,
    ) -> Option<(String, u32)> {
        let (text, revision) = self.base_text(root, target)?;
        let caret = super::text_edit::snap_to_boundary(&text, caret.min(text.len() as u32));
        self.store_cursor(
            *target,
            TextCursor {
                caret,
                anchor: None,
                revision,
                session,
            },
        );
        Some((text, revision))
    }

    /// Set caret plus anchor, fenced to the base revision.
    fn set_selection_cursor(
        &mut self,
        root: &GuiRoot,
        target: &GuiInputTarget,
        session: u64,
        start: u32,
        end: u32,
    ) -> Option<(String, u32)> {
        let (text, revision) = self.base_text(root, target)?;
        let len = text.len() as u32;
        let start = super::text_edit::snap_to_boundary(&text, start.min(len));
        let end = super::text_edit::snap_to_boundary(&text, end.min(len));
        let anchor = if start == end {
            None
        } else {
            Some(start)
        };
        self.store_cursor(
            *target,
            TextCursor {
                caret: end,
                anchor,
                revision,
                session,
            },
        );
        Some((text, revision))
    }

    /// Store the predicted cursor after a text commit envelope advances the
    /// revision. Paint bumps only when the cursor actually moved.
    fn store_committed_cursor(
        &mut self,
        target: &GuiInputTarget,
        caret: u32,
        revision: u32,
        session: u64,
    ) {
        self.store_cursor(
            *target,
            TextCursor {
                caret,
                anchor: None,
                revision: revision.saturating_add(1),
                session,
            },
        );
    }

    /// Clear keyboard focus, bumping paint only when one was set.
    fn clear_focus(&mut self) {
        if self.focus.is_some() {
            self.focus = None;
            self.focus_visible = false;
            self.focus_generation = self.focus_generation.saturating_add(1).max(1);
            self.touch_caret();
        }
    }

    /// Drop provisional composition for one node, if present.
    fn cancel_composition_for_target(&mut self, target: &GuiInputTarget) {
        if self
            .composition
            .as_ref()
            .is_some_and(|composed| composed.target == *target)
        {
            self.composition = None;
            self.touch_caret();
        }
    }

    /// Map a logical x to a caret offset using the retained measurement.
    /// Falls back to the end of `base_text` while the font is pending.
    fn pen_offset(
        layout: &GuiLayoutSystem,
        entity: EntityId,
        node: GuiNodeId,
        base_text: &str,
        x_logical: f32,
    ) -> u32 {
        let view = layout.view(entity);
        let evaluated = view
            .as_ref()
            .and_then(|view| view.nodes.iter().find(|evaluated| evaluated.node == node));
        let Some(evaluated) = evaluated else {
            return base_text.len() as u32;
        };
        let GuiEvaluatedContent::TextInput {
            layout,
            font_size,
            ..
        } = &evaluated.content
        else {
            return base_text.len() as u32;
        };
        // Pointer mapping shares the retained accumulated visual scale, so
        // presses land on the same geometry that paint and hit testing use.
        let scale = font_size * evaluated.acc_scale[0];
        if !scale.is_finite() || scale == 0.0 {
            return base_text.len() as u32;
        }
        let x_ems = (x_logical - evaluated.content_origin[0]) / scale;
        let offset = super::text_edit::caret_offset_at_x(layout, x_ems);
        super::text_edit::snap_to_boundary(base_text, offset.min(base_text.len() as u32))
    }

    /// Active camera pose ordering keyboard panels, or None without an
    /// active camera. Keys carry no viewport point, so unlike pointer
    /// projection this needs no host viewport.
    fn keyboard_view(
        &self,
        access: &SystemRuntimeAccess<'_>,
        sim: &WorldSimulationState,
    ) -> Option<GuiKeyboardView> {
        let camera = access.dependency(self.camera?)?;
        let read = camera.read(sim);
        // The selection may outlive its entity; only a live camera orders.
        let lens = read.active_camera_component()?;
        GuiKeyboardView::of_camera(sim, read.active_camera()?, lens)
    }

    /// All focusable controls in deterministic tab order: panels in keyboard
    /// panel order (view order with a camera, otherwise entity order), each
    /// in tree order.
    fn focusables(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        view: Option<&GuiKeyboardView>,
    ) -> Vec<GuiInputTarget> {
        let mut panels = layout.evaluated_entities();
        if let Some(view) = view {
            panels = keyboard_panel_order(sim, view, panels);
        }

        let mut order = Vec::new();
        for entity in panels {
            let Some(view) = layout.view(entity) else {
                continue;
            };
            if !view.available {
                continue;
            }
            let (incarnation, root) = match producer_root(sim, entity) {
                Some(root) => root,
                None => continue,
            };
            if incarnation != view.root_incarnation {
                continue;
            }
            for node in &view.nodes {
                if !node.available || !node.enabled || !node.visible {
                    continue;
                }
                let Some(live) = root.nodes().node(node.node) else {
                    continue;
                };
                if ControlKind::of(&live.data).is_none() {
                    continue;
                }
                let opacity = root
                    .style_row(node.node)
                    .map(|style| style.opacity)
                    .unwrap_or(1.0);
                if opacity > 0.0 {
                    order.push(GuiInputTarget {
                        entity,
                        node: node.node,
                        root_incarnation: incarnation,
                    });
                }
            }
        }
        order
    }

    /// Keyboard traversal order around one focused target: the focusable
    /// controls inside its innermost focus scope, in tree order, or every
    /// panel's order when no scope contains it.
    fn traversal_order(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        view: Option<&GuiKeyboardView>,
        root: &GuiRoot,
        focused: &GuiInputTarget,
    ) -> Vec<GuiInputTarget> {
        let order = self.focusables(layout, sim, view);
        let Some(scope) = Self::focus_scope_of(root, focused.node) else {
            return order;
        };

        order
            .into_iter()
            .filter(|target| {
                target.entity == focused.entity
                    && target.root_incarnation == focused.root_incarnation
                    && Self::within_subtree(root, target.node, scope)
            })
            .collect()
    }

    /// Innermost focus-scope ancestor of one node, excluding the node.
    fn focus_scope_of(root: &GuiRoot, node: GuiNodeId) -> Option<GuiNodeId> {
        let nodes = root.nodes();
        let mut current = nodes.node(node)?.parent;
        // Parent links form a tree; the bound only guards malformed input.
        for _ in 0..nodes.len() {
            let id = current?;
            if root.style_row(id).is_some_and(|style| style.focus_scope) {
                return Some(id);
            }
            current = nodes.node(id)?.parent;
        }
        None
    }

    /// Whether `node` lies in the subtree rooted at `ancestor`.
    fn within_subtree(root: &GuiRoot, node: GuiNodeId, ancestor: GuiNodeId) -> bool {
        let nodes = root.nodes();
        let mut current = Some(node);
        for _ in 0..=nodes.len() {
            match current {
                Some(id) if id == ancestor => return true,
                Some(id) => current = nodes.node(id).and_then(|live| live.parent),
                None => return false,
            }
        }
        false
    }

    /// Enter keyboard traversal without focus: Tab takes the keyboard panel's
    /// first focusable control and BackTab its last. The keyboard panel is
    /// the first panel in keyboard panel order; only without a camera does
    /// the most recently focused panel take precedence. Entry acquires an
    /// unowned context like other context-establishing input; nothing
    /// focusable leaves the input unhandled without acquiring.
    #[allow(clippy::too_many_arguments)]
    fn enter_traversal(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        view: Option<&GuiKeyboardView>,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        backward: bool,
    ) {
        let order = self.focusables(layout, sim, view);
        let in_panel = |target: &GuiInputTarget, (entity, incarnation): (EntityId, u64)| {
            target.entity == entity && target.root_incarnation == incarnation
        };
        let recent = self
            .keyboard_panel
            .filter(|_| view.is_none())
            .filter(|panel| order.iter().any(|target| in_panel(target, *panel)));
        let panel = recent.or_else(|| {
            order
                .first()
                .map(|target| (target.entity, target.root_incarnation))
        });
        let Some(panel) = panel else {
            self.unhandled(session, tick, input, GuiUnhandledReason::NoFocus);
            return;
        };
        if !self.check_owner(session, tick, input) {
            return;
        }

        let mut candidates = order.iter().filter(|target| in_panel(target, panel));
        let entry = if backward {
            candidates.next_back()
        } else {
            candidates.next()
        };
        self.set_focus(session, tick, entry.copied(), true);
    }

    /// Whether one full target remains live and evaluated eligible.
    fn target_eligible(
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        target: &GuiInputTarget,
    ) -> bool {
        matches!(
            evaluated_status(sim, layout, target),
            GuiTargetStatus::Eligible(_)
        )
    }

    /// Innermost-first eligible ScrollView ancestry of one hit node,
    /// including the node itself when it is a ScrollView viewport. Control
    /// hits walk from their parent; container hits resolve through the
    /// scroll-aware hit test below.
    fn scroll_chain(root: &GuiRoot, node: GuiNodeId) -> Vec<GuiNodeId> {
        let mut chain = Vec::new();
        let mut current = root.nodes().node(node).map(|_| node);
        let cap = root.nodes().len().saturating_add(1).max(2);
        while let Some(id) = current {
            if chain.len() >= cap {
                break;
            }
            let Some(live) = root.nodes().node(id) else {
                break;
            };
            if matches!(live.data, GuiNodeData::Container(kind) if kind.is_scrollable()) {
                chain.push(id);
            }
            current = live.parent;
        }
        chain
    }

    /// Scroll capacity of one ScrollView in local logical units: the clamped
    /// maximum offset from evaluated content extents over the retained
    /// viewport. Unknown or degenerate viewports hold still.
    fn scroll_max(view: &GuiEvaluatedView, node: GuiNodeId) -> [f32; 2] {
        view.nodes
            .iter()
            .find(|record| record.node == node)
            .map_or([0.0, 0.0], scroll_capacity)
    }

    /// Total final-logical shift for one node from its ancestor ScrollView
    /// offsets. Each offset moves scrolled content through that ScrollView's
    /// accumulated scale beneath its viewport, which itself moves only with
    /// outer offsets. Paint, hit and caret consumers share this translation.
    fn ancestor_shift(
        &self,
        view: &GuiEvaluatedView,
        root: &GuiRoot,
        entity: EntityId,
        node: GuiNodeId,
    ) -> [f32; 2] {
        let mut shift = [0.0, 0.0];
        let mut current = root.nodes().node(node).and_then(|live| live.parent);
        let cap = root.nodes().len().saturating_add(1).max(2);
        let mut depth = 0;
        while let Some(id) = current {
            if depth >= cap {
                break;
            }
            depth += 1;
            let Some(live) = root.nodes().node(id) else {
                break;
            };
            if matches!(live.data, GuiNodeData::Container(kind) if kind.is_scrollable())
                && let Some(record) = view.nodes.iter().find(|record| record.node == id)
                && let Some(cursor) = self.scroll_offsets.get(&GuiInputTarget {
                    entity,
                    root_incarnation: view.root_incarnation,
                    node: id,
                })
            {
                shift[0] -= cursor.offset[0] * record.acc_scale[0];
                shift[1] -= cursor.offset[1] * record.acc_scale[1];
            }
            current = live.parent;
        }
        shift
    }

    /// Final-logical rectangle of one evaluated node with its ancestor
    /// scroll shift applied. Unscrolled subtrees keep retained geometry.
    fn scrolled_rect(
        &self,
        view: &GuiEvaluatedView,
        root: &GuiRoot,
        entity: EntityId,
        record: &GuiEvaluatedNode,
    ) -> [f32; 4] {
        let shift = self.ancestor_shift(view, root, entity, record.node);
        [
            record.rect[0] + shift[0],
            record.rect[1] + shift[1],
            record.rect[2],
            record.rect[3],
        ]
    }

    /// Topmost eligible node containing a final-logical point with ancestor
    /// scroll shifts applied, traversing in reverse painter order within the
    /// shared clips. Accepts containers as well as controls so scrolls
    /// resolve their ScrollView ancestry; pointer routing keeps its own
    /// control-only fence on top. With `bars`, an enabled scroll bar painted
    /// above the hit node wins instead. Never fabricates coordinates.
    fn scroll_hit_in_view(
        &self,
        view: &GuiEvaluatedView,
        root: &GuiRoot,
        entity: EntityId,
        point: [f32; 2],
        bars: bool,
    ) -> Option<ScrollViewHit> {
        if !view.available {
            return None;
        }
        let placements = self.scrolled_placements(view, entity);
        let bar = if bars {
            self.scroll_bar_hit(view, root, entity, &placements, point)
        } else {
            None
        };
        for (index, (record, placement)) in view.nodes.iter().zip(&placements).enumerate().rev() {
            // A bar paints right after its ScrollView's subtree, so it covers
            // every record up to that subtree's last one.
            if let Some((end, hit)) = bar
                && end >= index
            {
                return Some(hit);
            }
            if !record.available || !record.enabled || !record.visible {
                continue;
            }
            let rect = [
                record.rect[0] + placement.shift[0],
                record.rect[1] + placement.shift[1],
                record.rect[2],
                record.rect[3],
            ];
            if !rect_contains_point(rect, point) {
                continue;
            }
            if let Some(clip) = placement.clip
                && !clip_contains_point(clip, point)
            {
                continue;
            }
            return Some(ScrollViewHit {
                node: record.node,
                rect,
                bar: None,
            });
        }
        bar.map(|(_, hit)| hit)
    }

    /// Topmost enabled scroll bar under a final-logical point, with the
    /// index of the last record of its ScrollView's subtree, after which it
    /// paints. Outer bars paint after inner ones ending at the same record.
    fn scroll_bar_hit(
        &self,
        view: &GuiEvaluatedView,
        root: &GuiRoot,
        entity: EntityId,
        placements: &[GuiScrollPlacement],
        point: [f32; 2],
    ) -> Option<(usize, ScrollViewHit)> {
        let mut best: Option<(usize, u32, ScrollViewHit)> = None;
        for (index, (record, placement)) in view.nodes.iter().zip(placements).enumerate() {
            if record.viewport.is_none()
                || !record.available
                || !record.visible
                || record.paint_suppressed
                || !(0..2).any(|axis| scroll_bar_enabled(record, axis))
            {
                continue;
            }
            if let Some(clip) = placement.clip
                && !clip_contains_point(clip, point)
            {
                continue;
            }
            let offset_of =
                |node: GuiNodeId| self.committed_scroll_offset(entity, view.root_incarnation, node);
            let Some(bar) = scroll_bars_in_view(view, root, index, offset_of)
                .into_iter()
                .flatten()
                .filter(|bar| scroll_bar_enabled(record, bar.axis))
                .map(|bar| bar.shifted(placement.shift))
                .find(|bar| rect_contains_point(bar.track, point))
            else {
                continue;
            };
            let end = view.nodes[index + 1..]
                .iter()
                .position(|next| next.depth <= record.depth)
                .map_or(view.nodes.len() - 1, |after| index + after);
            let above = best.is_none_or(|(best_end, best_depth, _)| {
                end > best_end || (end == best_end && record.depth < best_depth)
            });
            if above {
                let parts = super::super::layout::scroll_bars::scroll_bar_parts(bar.axis);
                let part = if rect_contains_point(bar.thumb, point) {
                    parts.1
                } else {
                    parts.0
                };
                let rect = [
                    record.rect[0] + placement.shift[0],
                    record.rect[1] + placement.shift[1],
                    record.rect[2],
                    record.rect[3],
                ];
                best = Some((
                    end,
                    record.depth,
                    ScrollViewHit {
                        node: record.node,
                        rect,
                        bar: Some((part, bar)),
                    },
                ));
            }
        }
        best.map(|(end, _, hit)| (end, hit))
    }

    /// Committed offset of one ScrollView, or zero.
    fn committed_scroll_offset(
        &self,
        entity: EntityId,
        root_incarnation: u64,
        node: GuiNodeId,
    ) -> [f32; 2] {
        self.scroll_offsets
            .get(&GuiInputTarget {
                entity,
                root_incarnation,
                node,
            })
            .map_or([0.0, 0.0], |cursor| cursor.offset)
    }

    /// Scrolled bar of one ScrollView on one axis at its committed offset,
    /// when that bar takes input.
    fn scroll_bar_of(
        &self,
        view: &GuiEvaluatedView,
        root: &GuiRoot,
        target: &GuiInputTarget,
        axis: usize,
    ) -> Option<GuiScrollBar> {
        let index = view
            .nodes
            .iter()
            .position(|record| record.node == target.node)?;
        let record = &view.nodes[index];
        if !scroll_bar_enabled(record, axis) {
            return None;
        }
        let shift = self.scrolled_placements(view, target.entity)[index].shift;
        let offset_of = |node: GuiNodeId| {
            self.committed_scroll_offset(target.entity, target.root_incarnation, node)
        };
        scroll_bars_in_view(view, root, index, offset_of)[axis].map(|bar| bar.shifted(shift))
    }

    /// Scrolled placement of every evaluated record, index-aligned with
    /// `view.nodes`: the final-logical shift from ancestor ScrollView
    /// offsets and the clip with every contributing viewport moved by its
    /// own ancestor shift. A ScrollView's viewport stays fixed relative to
    /// its own content only, so nested content clips by the moved inner
    /// viewport after an outer scroll. Records are in pre-order, so each
    /// parent is the nearest preceding shallower record.
    fn scrolled_placements(
        &self,
        view: &GuiEvaluatedView,
        entity: EntityId,
    ) -> Vec<GuiScrollPlacement> {
        // (depth, shift inherited by children, clip inherited by children)
        let mut ancestors: Vec<(u32, [f32; 2], Option<SurfaceClipRect>)> = Vec::new();
        let mut placements = Vec::with_capacity(view.nodes.len());
        for record in &view.nodes {
            while ancestors
                .last()
                .is_some_and(|(depth, ..)| *depth >= record.depth)
            {
                ancestors.pop();
            }
            let (shift, clip) = ancestors
                .last()
                .map_or(([0.0, 0.0], record.clip), |(_, shift, clip)| {
                    (*shift, *clip)
                });
            placements.push(GuiScrollPlacement {
                shift,
                clip,
            });

            let (children_shift, children_clip) = match record.viewport {
                Some(viewport) => {
                    let moved = [
                        viewport[0] + shift[0],
                        viewport[1] + shift[1],
                        viewport[2] + shift[0],
                        viewport[3] + shift[1],
                    ];
                    let offset = self
                        .scroll_offsets
                        .get(&GuiInputTarget {
                            entity,
                            root_incarnation: view.root_incarnation,
                            node: record.node,
                        })
                        .map_or([0.0, 0.0], |cursor| cursor.offset);
                    (
                        [
                            shift[0] - offset[0] * record.acc_scale[0],
                            shift[1] - offset[1] * record.acc_scale[1],
                        ],
                        Some(match clip {
                            Some(outer) => {
                                super::super::layout::evaluation::nested_clip(outer, moved)
                            }
                            None => moved,
                        }),
                    )
                }
                None => (shift, clip),
            };
            ancestors.push((record.depth, children_shift, children_clip));
        }
        placements
    }

    /// Final-logical clip of one evaluated node with every contributing
    /// ScrollView viewport moved by its own ancestor scroll shift.
    fn scrolled_clip(
        &self,
        view: &GuiEvaluatedView,
        entity: EntityId,
        node: GuiNodeId,
    ) -> Option<SurfaceClipRect> {
        let index = view.nodes.iter().position(|record| record.node == node)?;
        self.scrolled_placements(view, entity)
            .get(index)
            .and_then(|placement| placement.clip)
    }

    /// Current revision of input-owned scroll offsets. Render preparation
    /// watches this alongside paint revisions so pure scrolls refresh
    /// scrolled paint without reflowing layout.
    pub(crate) fn scroll_revision(&self) -> u64 {
        self.scroll_revision
    }

    /// Final-logical scroll placements of one panel's moved paint, keyed by
    /// node; identities are never reused within a root incarnation. Only
    /// nodes whose shift is nonzero or whose clip moved appear. Viewport
    /// nodes carry only outer shifts: their own offset never applies to
    /// themselves, while their descendants shift beneath the viewport clip,
    /// which itself moves with the viewport's outer shift.
    pub(crate) fn scroll_shifts_logical(
        &self,
        layout: &GuiLayoutSystem,
        entity: EntityId,
    ) -> BTreeMap<GuiNodeId, GuiScrollPlacement> {
        let Some(view) = layout.view(entity) else {
            return BTreeMap::new();
        };
        view.nodes
            .iter()
            .zip(self.scrolled_placements(view, entity))
            .filter(|(record, placement)| {
                placement.shift != [0.0, 0.0] || placement.clip != record.clip
            })
            .map(|(record, placement)| (record.node, placement))
            .collect()
    }
}

impl GuiInputSystem {
    /// Current transient caret generation for derived paint.
    pub(crate) fn caret_revision(&self) -> u64 {
        self.caret_revision
    }

    /// Bump the caret generation after a transient cursor change.
    fn touch_caret(&mut self) {
        self.caret_revision = self.caret_revision.saturating_add(1).max(1);
    }

    /// Store one transient cursor, bumping paint only on change.
    fn store_cursor(&mut self, key: GuiInputTarget, cursor: TextCursor) {
        if self.text_carets.get(&key) != Some(&cursor) {
            self.text_carets.insert(key, cursor);
            self.touch_caret();
        }
    }

    /// Clear the provisional, bumping paint only when one was active.
    fn clear_composition(&mut self) {
        if self.composition.is_some() {
            self.composition = None;
            self.touch_caret();
        }
    }

    /// Panel entities with live interaction priority: keyboard focus, pointer
    /// hover, press or capture. Surface cache presentation reads this to force
    /// direct rendering; removed or disabled roots leave with their cursors.
    pub(crate) fn interaction_roots(&self) -> BTreeSet<EntityId> {
        let mut roots = BTreeSet::new();
        roots.extend(self.focus.map(|focus| focus.target.entity));
        roots.extend(self.hovers.values().map(|cursor| cursor.target.entity));
        roots.extend(self.press_owners.keys().map(|target| target.entity));
        roots.extend(self.captures.values().map(|capture| capture.target.entity));
        roots.extend(self.scroll_drags.values().map(|drag| drag.entity));
        roots.extend(
            self.scroll_bar_presses
                .values()
                .map(|press| press.target.entity),
        );
        roots.extend(
            self.scroll_bar_hovers
                .values()
                .map(|hover| hover.target.entity),
        );
        roots
    }

    /// Snapshot hover, press and focus cursors for skin paint. Read-only:
    /// prepared paint observes live interaction here instead of
    /// reconstructing it from routed effects. Never mutates cursors,
    /// predictions or envelopes.
    pub(crate) fn skin_cursors(&self) -> GuiSkinCursors {
        let mut scroll_bars: BTreeMap<GuiInputTarget, GuiScrollBarCursor> = BTreeMap::new();
        for (target, cursor) in &self.scroll_offsets {
            if cursor.offset != [0.0, 0.0] {
                scroll_bars.entry(*target).or_default().offset = cursor.offset;
            }
        }
        for hover in self.scroll_bar_hovers.values() {
            scroll_bars.entry(hover.target).or_default().hovered = Some(hover.part);
        }
        for press in self.scroll_bar_presses.values() {
            scroll_bars.entry(press.target).or_default().pressed = Some(press.part);
        }
        GuiSkinCursors {
            hovered: self.hovers.values().map(|cursor| cursor.target).collect(),
            pressed: self.press_owners.keys().copied().collect(),
            focus: self.focus,
            focus_visible: self.focus_visible,
            scroll_bars,
        }
    }

    /// Committed scroll offsets of one root incarnation's ScrollViews.
    pub(crate) fn scroll_offsets_for(
        &self,
        entity: EntityId,
        root_incarnation: u64,
    ) -> BTreeMap<GuiNodeId, [f32; 2]> {
        self.scroll_offsets
            .iter()
            .filter(|(target, _)| {
                target.entity == entity && target.root_incarnation == root_incarnation
            })
            .map(|(target, cursor)| (target.node, cursor.offset))
            .collect()
    }
}

/// Range and committed value of a live slider node.
fn slider_row(root: &GuiRoot, node: GuiNodeId) -> Option<GuiNodeDataRow> {
    match root.nodes().node(node)?.data {
        GuiNodeData::Slider => root.data_row(node).cloned(),
        _ => None,
    }
}

/// Map a logical x coordinate to a slider value along a snapshot rectangle.
/// Endpoints remain exact; within the rail an accepted off-step current value
/// participates beside the step lattice so grabbing its thumb cannot jump.
pub(crate) fn slider_value_at(
    values: &GuiNodeDataRow,
    rect: [f32; 4],
    x: f32,
    current: &GuiControlValue,
) -> Option<GuiControlValue> {
    let (Some(min), Some(max), Some(step)) = (&values.min, &values.max, &values.step) else {
        return None;
    };
    let fraction = super::super::slider_rail(rect)?.fraction_at(x)?;
    let mut value = min + fraction * (max - min);
    if fraction <= 0.0 {
        value = *min;
    } else if fraction >= 1.0 {
        value = *max;
    } else if *step > 0.0 {
        let snapped = ((value - min) / step).round() * step + min;
        value = match current {
            GuiControlValue::Scalar(current)
                if current.is_finite()
                    && *current >= *min
                    && *current <= *max
                    && (value - current).abs() <= (value - snapped).abs() =>
            {
                *current
            }
            _ => snapped,
        };
    }
    value = value.clamp(*min, *max);
    if value.is_finite() {
        Some(GuiControlValue::Scalar(value))
    } else {
        None
    }
}

/// Whether a final-logical `[x, y, width, height]` rectangle contains a
/// point. Mirrors the retained layout rule so arbitration agrees with hit
/// testing: edges belong on the min side only, zero-area hits nothing.
fn rect_contains_point(rect: [f32; 4], point: [f32; 2]) -> bool {
    rect[2] > 0.0
        && rect[3] > 0.0
        && point[0] >= rect[0]
        && point[0] < rect[0] + rect[2]
        && point[1] >= rect[1]
        && point[1] < rect[1] + rect[3]
}

/// Whether an accumulated `[min_x, min_y, max_x, max_y]` clip contains a
/// point, with the same min-inclusive edge rule as rectangles.
fn clip_contains_point(clip: crate::systems::surface::SurfaceClipRect, point: [f32; 2]) -> bool {
    point[0] >= clip[0] && point[0] < clip[2] && point[1] >= clip[1] && point[1] < clip[3]
}

/// Nudge a slider value by key, honouring bounds and the step lane.
fn slider_nudge(
    values: &GuiNodeDataRow,
    committed: &GuiControlValue,
    steps: f32,
) -> Option<GuiControlValue> {
    let (Some(min), Some(max), Some(step)) = (&values.min, &values.max, &values.step) else {
        return None;
    };
    let GuiControlValue::Scalar(current) = committed else {
        return None;
    };
    let stride = if *step > 0.0 {
        *step
    } else {
        (max - min) / 100.0
    };
    if !stride.is_finite() || stride == 0.0 {
        return None;
    }
    let mut value = (current + steps * stride).clamp(*min, *max);
    if *step > 0.0 {
        value = ((value - min) / step).round() * step + min;
        value = value.clamp(*min, *max);
    }
    if value.is_finite() {
        Some(GuiControlValue::Scalar(value))
    } else {
        None
    }
}

/// Retained text measurement for geometry queries, fenced to `revision`.
/// Returns the layout, logical units per em and content origin, or None
/// while the font is pending or the revision moved.
fn retained_text_metrics(
    layout: &GuiLayoutSystem,
    entity: EntityId,
    node: GuiNodeId,
    revision: u32,
) -> Option<(crate::systems::surface::TextLayout, f32, [f32; 2])> {
    let view = layout.view(entity)?;
    let evaluated = view.nodes.iter().find(|evaluated| evaluated.node == node)?;
    let GuiEvaluatedContent::TextInput {
        layout,
        font_size,
        revision: measured_revision,
        ..
    } = &evaluated.content
    else {
        return None;
    };
    if *measured_revision != revision {
        return None;
    }
    if !font_size.is_finite() || *font_size <= 0.0 {
        return None;
    }
    Some((layout.clone(), *font_size, evaluated.content_origin))
}

/// Logical zero-width caret pen for a retained revision, or None.
fn text_caret_rect(
    layout: &GuiLayoutSystem,
    entity: EntityId,
    node: GuiNodeId,
    caret: u32,
    revision: u32,
) -> Option<[f32; 4]> {
    let (measured, scale, origin) = retained_text_metrics(layout, entity, node, revision)?;
    let pen = measured.caret_position(caret)?;
    Some([
        origin[0] + pen.position[0] * scale,
        origin[1] + pen.position[1] * scale,
        0.0,
        pen.height * scale,
    ])
}

/// Logical per-line selection rects for a retained revision.
fn text_selection_rects(
    layout: &GuiLayoutSystem,
    entity: EntityId,
    node: GuiNodeId,
    start: u32,
    end: u32,
    revision: u32,
) -> Vec<[f32; 4]> {
    let Some((measured, scale, origin)) = retained_text_metrics(layout, entity, node, revision)
    else {
        return Vec::new();
    };
    measured
        .selection_rects(start, end)
        .into_iter()
        .map(|rect| {
            [
                origin[0] + rect[0] * scale,
                origin[1] + rect[1] * scale,
                rect[2] * scale,
                rect[3] * scale,
            ]
        })
        .collect()
}

/// Derived caret/selection/composition paint for one text input.
///
/// Transient only: committed text, revisions and retained views never
/// change here. The caret paints one bar at the committed caret; a
/// selection fills its line rects; an active provisional paints its caret
/// over the committed caret so composition never duplicates text.
///
/// Overlays paint under their own [`GuiPrimitivePart`]s, so they never share
/// an identity with each other, with the node's `Label` or with the
/// provisional `Composition` run: retained renderer caches key paint by
/// identity. At most one caret bar and one selection highlight paint: a
/// clean provisional owns both, replacing the committed caret and
/// selection until it commits or cancels, and a single-line input yields
/// at most one rect per selection.
///
/// [`GuiPrimitivePart`]: crate::systems::surface::GuiPrimitivePart
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextPaintKind {
    /// Collapsed caret bar at the committed caret.
    Caret,
    /// Filled selection rects over committed text.
    Selection,
    /// Caret bar while a provisional is active on the node.
    Composition,
}

impl TextPaintKind {
    /// Paint part; both caret kinds share `Caret` because only one caret
    /// bar ever paints.
    const fn part(self) -> crate::systems::surface::GuiPrimitivePart {
        use crate::systems::surface::GuiPrimitivePart;

        match self {
            Self::Caret | Self::Composition => GuiPrimitivePart::Caret,
            Self::Selection => GuiPrimitivePart::Selection,
        }
    }

    /// Fill colour of the overlay's rects.
    const fn color(self) -> [f32; 4] {
        match self {
            Self::Selection => TEXT_SELECTION_COLOR,
            Self::Caret | Self::Composition => TEXT_CARET_COLOR,
        }
    }
}

/// One derived text-paint overlay in final-logical units with ancestor
/// scroll shifts applied, clipped like the scrolled node it annotates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextPaintOverlay {
    /// Focused text input owning the overlay.
    pub target: GuiInputTarget,
    /// Which transient state the overlay shows.
    pub kind: TextPaintKind,
    /// Paint rects in final-logical units (caret bar widened).
    pub rects: Vec<[f32; 4]>,
    /// Scrolled node clip in final-logical units, if any.
    pub clip: Option<crate::systems::surface::SurfaceClipRect>,
    /// Retained node opacity.
    pub opacity: f32,
}

/// Opaque caret bar color in derived paint.
const TEXT_CARET_COLOR: [f32; 4] = [0.05, 0.05, 0.05, 1.0];

/// Translucent selection fill in derived paint.
const TEXT_SELECTION_COLOR: [f32; 4] = [0.25, 0.5, 1.0, 0.35];

impl GuiInputSystem {
    /// Derived text paint for the focused text input on one panel, if any.
    /// Read-only: retained metrics at the committed revision supply
    /// geometry; a routed-but-unapplied edit keeps the committed caret for
    /// one frame rather than guessing provisional geometry.
    pub(crate) fn text_paint_overlays(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        entity: EntityId,
    ) -> Vec<TextPaintOverlay> {
        let Some(focus) = self.focus else {
            return Vec::new();
        };
        if focus.target.entity != entity {
            return Vec::new();
        }
        let target = focus.target;
        let Some((kind, root)) = self.revalidate_capture(sim, &target) else {
            return Vec::new();
        };
        if kind != ControlKind::TextInput {
            return Vec::new();
        }
        let Some(view) = layout.view(entity) else {
            return Vec::new();
        };
        let Some(evaluated) = view.nodes.iter().find(|node| node.node == target.node) else {
            return Vec::new();
        };
        if !evaluated.available || !evaluated.enabled || !evaluated.visible {
            return Vec::new();
        }
        let (predicted_text, predicted_revision) = match self.base_text(&root, &target) {
            Some(base) => base,
            None => return Vec::new(),
        };
        let committed_revision = root
            .control_state(target.node)
            .map(|state| state.revision)
            .unwrap_or(predicted_revision);
        // Geometry follows the committed measurement: while a routed edit
        // still awaits its mutation boundary the committed caret holds.
        let metrics_revision = if predicted_revision == committed_revision {
            predicted_revision
        } else {
            committed_revision
        };
        let metrics_text = if metrics_revision == predicted_revision {
            predicted_text.clone()
        } else {
            match root.control_state(target.node) {
                Some(state) => match &state.value {
                    GuiControlValue::Text(text) => text.clone(),
                    _ => return Vec::new(),
                },
                None => return Vec::new(),
            }
        };
        let (caret, anchor) = match self.text_carets.get(&target) {
            Some(cursor)
                if cursor.session == focus.session && cursor.revision == predicted_revision =>
            {
                let len = predicted_text.len() as u32;
                let caret =
                    super::text_edit::snap_to_boundary(&predicted_text, cursor.caret.min(len));
                let anchor = cursor.anchor.map(|anchor| {
                    super::text_edit::snap_to_boundary(&predicted_text, anchor.min(len))
                });
                (caret, anchor.filter(|anchor| *anchor != caret))
            }
            _ => (metrics_text.len() as u32, None),
        };
        if metrics_revision != predicted_revision {
            // While live provisional glyphs paint at the committed caret,
            // the provisional owns the end caret for that frame.
            let live_provisional = self.composition.as_ref().is_some_and(|composed| {
                composed.is_for(&target, focus.session)
                    && composed.revision == predicted_revision
                    && !composed.provisional.is_empty()
            });
            if live_provisional {
                return Vec::new();
            }
            // Stale-cursor frame: collapse to the committed end.
            let end = metrics_text.len() as u32;
            return self.caret_overlay(
                layout,
                view,
                &root,
                entity,
                target,
                evaluated.opacity,
                self.scrolled_clip(view, entity, target.node),
                end,
                metrics_revision,
            );
        }
        let clip = self.scrolled_clip(view, entity, target.node);
        let mut overlays = Vec::new();
        // A clean provisional owns the selection highlight as it owns the
        // caret: it replaces the committed selection when it commits.
        let anchor = anchor.filter(|_| !self.clean_provisional(&target, metrics_revision));
        if let Some(anchor) = anchor {
            let (start, end) = super::text_edit::normalize_range(anchor, caret);
            let rects: Vec<[f32; 4]> =
                text_selection_rects(layout, entity, target.node, start, end, metrics_revision)
                    .into_iter()
                    .map(|rect| {
                        let shift = self.ancestor_shift(view, &root, entity, target.node);
                        [rect[0] + shift[0], rect[1] + shift[1], rect[2], rect[3]]
                    })
                    .collect();
            if !rects.is_empty() {
                overlays.push(TextPaintOverlay {
                    target,
                    kind: TextPaintKind::Selection,
                    rects,
                    clip,
                    opacity: evaluated.opacity,
                });
            }
        }
        overlays.extend(self.caret_overlay(
            layout,
            view,
            &root,
            entity,
            target,
            evaluated.opacity,
            clip,
            caret,
            metrics_revision,
        ));
        overlays
    }

    /// One caret bar overlay at `caret`, widened from the zero-width pen.
    /// An active provisional on the target paints as composition kind,
    /// except while clean provisional glyphs paint below: then the
    /// provisional path owns the end caret and this bar stays out so only
    /// one caret ever paints.
    #[allow(clippy::too_many_arguments)]
    fn caret_overlay(
        &self,
        layout: &GuiLayoutSystem,
        view: &GuiEvaluatedView,
        root: &GuiRoot,
        entity: EntityId,
        target: GuiInputTarget,
        opacity: f32,
        clip: Option<crate::systems::surface::SurfaceClipRect>,
        caret: u32,
        revision: u32,
    ) -> Vec<TextPaintOverlay> {
        let Some(mut rect) = text_caret_rect(layout, entity, target.node, caret, revision) else {
            return Vec::new();
        };
        // Skip degenerate carets; NaN heights never paint.
        if rect[3].is_nan() || rect[3] <= 0.0 {
            return Vec::new();
        }
        let shift = self.ancestor_shift(view, root, entity, target.node);
        rect[0] += shift[0];
        rect[1] += shift[1];
        rect[2] = rect[3] * 0.08;
        let composing = self
            .composition
            .as_ref()
            .is_some_and(|composed| composed.target == target);
        if composing && self.clean_provisional(&target, revision) {
            return Vec::new();
        }
        Vec::from([TextPaintOverlay {
            target,
            kind: if composing {
                TextPaintKind::Composition
            } else {
                TextPaintKind::Caret
            },
            rects: Vec::from([rect]),
            clip,
            opacity,
        }])
    }

    /// Whether a clean provisional paints for `target` at `revision`.
    /// Clean means the provisional belongs to the target, started against
    /// the painted revision and holds visible text; stale provisionals
    /// and pending fonts keep the committed caret bar instead.
    fn clean_provisional(&self, target: &GuiInputTarget, revision: u32) -> bool {
        self.composition.as_ref().is_some_and(|composed| {
            composed.target == *target
                && composed.revision == revision
                && !composed.provisional.is_empty()
        })
    }

    /// Derived provisional composition glyphs for one panel, with the
    /// end-caret and selection overlays over them.
    ///
    /// Transient only: shapes the provisional with the retained font at
    /// the committed caret without touching committed text, revisions or
    /// retained views. Returns the glyph primitive plus overlay rects in
    /// final-logical units, or None when no clean provisional is active
    /// or the font is pending.
    fn composition_glyph_paint(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        entity: EntityId,
        world: crate::WorldId,
        assets: &crate::services::asset_management::AssetManagementService,
    ) -> Option<(
        crate::systems::surface::SurfaceRenderPrimitive,
        Vec<TextPaintOverlay>,
    )> {
        use crate::services::asset_management::font::FontAsset;
        use crate::systems::surface::{
            GuiPrimitiveId, SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
            SurfaceRenderPrimitive, TextFont, TextLinePolicy, TextMaxWidth, TextMeasureRequest,
            TextOutcome, gui_logical_to_surface_content, measure_text,
        };

        let focus = self.focus?;
        if focus.target.entity != entity {
            return None;
        }
        let target = focus.target;
        let (kind, root) = self.revalidate_capture(sim, &target)?;
        if kind != ControlKind::TextInput {
            return None;
        }
        let view = layout.view(entity)?;
        let evaluated = view.nodes.iter().find(|node| node.node == target.node)?;
        if !evaluated.available || !evaluated.enabled || !evaluated.visible {
            return None;
        }
        let crate::GuiEvaluatedContent::TextInput {
            font,
            font_size,
            ..
        } = &evaluated.content
        else {
            return None;
        };
        let (predicted_text, predicted_revision) = self.base_text(&root, &target)?;
        let committed_revision = root
            .control_state(target.node)
            .map(|state| state.revision)
            .unwrap_or(predicted_revision);
        let composed = self.composition.as_ref()?;
        if !composed.is_for(&target, focus.session) || composed.provisional.is_empty() {
            return None;
        }
        // A routed-but-unapplied edit leaves a prediction behind; the live
        // provisional still paints at the committed caret until the commit
        // reflows. Stale provisionals keep the committed caret bar instead.
        if composed.revision != predicted_revision {
            return None;
        }
        let units = view.units_per_metre;
        if !units.is_finite() || units <= 0.0 {
            return None;
        }
        let caret = if predicted_revision == committed_revision {
            match self.text_carets.get(&target) {
                Some(cursor)
                    if cursor.session == focus.session && cursor.revision == predicted_revision =>
                {
                    super::text_edit::snap_to_boundary(
                        &predicted_text,
                        cursor.caret.min(predicted_text.len() as u32),
                    )
                }
                _ => predicted_text.len() as u32,
            }
        } else {
            {
                let state = root.control_state(target.node)?;
                match &state.value {
                    GuiControlValue::Text(text) => text.len() as u32,
                    _ => return None,
                }
            }
        };
        let (measured, _, origin) =
            retained_text_metrics(layout, entity, target.node, committed_revision)?;
        let pen = measured.caret_position(caret)?;
        let key = assets.find_source(
            world,
            font.source.kind,
            &font.source.uri,
            font.source.variant,
        )?;
        let font_asset = assets
            .get(key)?
            .data()?
            .decoded()
            .downcast_ref::<FontAsset>()?;
        let request = TextMeasureRequest::new(
            &composed.provisional,
            TextFont::Ready {
                key,
                font: font_asset,
            },
            *font_size,
            TextLinePolicy::SingleLine,
            TextMaxWidth::Unbounded,
        )
        .ok()?;
        let TextOutcome::Measured(provisional) = measure_text(&request) else {
            return None;
        };
        if provisional.glyphs.is_empty() {
            return None;
        }
        let provisional_caret = provisional.caret_position(composed.caret_end)?;
        let scale = *font_size;
        let metres_per_em = *font_size / units;
        let shift = self.ancestor_shift(view, &root, entity, target.node);
        let clip = self.scrolled_clip(view, entity, target.node);
        let origin = [origin[0] + shift[0], origin[1] + shift[1]];
        let pen_ems = pen.position;
        let pen = [
            origin[0] + pen_ems[0] * scale,
            origin[1] + pen_ems[1] * scale,
        ];
        let glyphs = provisional
            .glyphs
            .iter()
            .map(|glyph| SurfaceGlyph {
                glyph_id: glyph.glyph_id,
                position: [
                    (pen_ems[0] + glyph.position[0]) * metres_per_em,
                    (pen_ems[1] + glyph.position[1]) * metres_per_em,
                ],
                color: None,
            })
            .collect::<Vec<_>>();
        let position = gui_logical_to_surface_content(origin, units)?;
        let glyphs = SurfaceRenderPrimitive::Glyphs {
            style: SurfacePrimitiveStyle {
                identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                    root_incarnation: view.root_incarnation,
                    node: target.node,
                    part: crate::systems::surface::GuiPrimitivePart::Composition,
                }),
                position,
                scale: evaluated.acc_scale,
                color: evaluated.color,
                opacity: evaluated.opacity,
                clip: clip.and_then(|clip| {
                    let min = gui_logical_to_surface_content([clip[0], clip[1]], units)?;
                    let max = gui_logical_to_surface_content([clip[2], clip[3]], units)?;
                    Some([min[0], min[1], max[0], max[1]])
                }),
            },
            font: font.clone(),
            font_size: metres_per_em,
            glyphs,
        };
        let caret_height = provisional_caret.height * scale;
        if caret_height.is_nan() || caret_height <= 0.0 {
            return None;
        }
        let caret_rect = [
            pen[0] + provisional_caret.position[0] * scale,
            pen[1] + provisional_caret.position[1] * scale,
            caret_height * 0.08,
            caret_height,
        ];
        let mut overlays = vec![TextPaintOverlay {
            target,
            kind: TextPaintKind::Composition,
            rects: vec![caret_rect],
            clip,
            opacity: evaluated.opacity,
        }];
        if composed.caret_start != composed.caret_end {
            let rects = provisional
                .selection_rects(composed.caret_start, composed.caret_end)
                .into_iter()
                .map(|rect| {
                    [
                        pen[0] + rect[0] * scale,
                        pen[1] + rect[1] * scale,
                        rect[2] * scale,
                        rect[3] * scale,
                    ]
                })
                .collect::<Vec<_>>();
            if !rects.is_empty() {
                overlays.push(TextPaintOverlay {
                    target,
                    kind: TextPaintKind::Selection,
                    rects,
                    clip,
                    opacity: evaluated.opacity,
                });
            }
        }
        Some((glyphs, overlays))
    }

    /// Derived caret/selection/composition primitives for one panel in
    /// Surface content metres, sharing the retained node, clip mapping and
    /// scroll translation with skinned paint under per-kind overlay parts.
    /// Committed state is never touched: overlays observe cursors and
    /// retained metrics, and the provisional shapes against the retained
    /// font at paint time.
    pub(crate) fn text_caret_primitives(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        entity: EntityId,
        world: crate::WorldId,
        assets: &crate::services::asset_management::AssetManagementService,
    ) -> Vec<crate::systems::surface::SurfaceRenderPrimitive> {
        use crate::systems::surface::{
            GuiPrimitiveId, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
            SurfaceRenderPrimitive, gui_logical_to_surface_content,
        };
        let Some(view) = layout.view(entity) else {
            return Vec::new();
        };
        let units = view.units_per_metre;
        if !units.is_finite() || units <= 0.0 {
            return Vec::new();
        }
        let mut primitives = Vec::new();
        let mut overlays = self.text_paint_overlays(layout, sim, entity);
        // Provisional text paints above committed paint: glyphs first,
        // then its end caret and selection through the shared mapping.
        if let Some((glyphs, mut provisional)) =
            self.composition_glyph_paint(layout, sim, entity, world, assets)
        {
            primitives.push(glyphs);
            overlays.append(&mut provisional);
        }
        for overlay in overlays {
            let clip = overlay.clip.and_then(|clip| {
                let min = gui_logical_to_surface_content([clip[0], clip[1]], units)?;
                let max = gui_logical_to_surface_content([clip[2], clip[3]], units)?;
                Some([min[0], min[1], max[0], max[1]])
            });
            let color = overlay.kind.color();
            for rect in &overlay.rects {
                if rect[2] <= 0.0 || rect[3] <= 0.0 {
                    continue;
                }
                let Some(position) = gui_logical_to_surface_content([rect[0], rect[1]], units)
                else {
                    continue;
                };
                primitives.push(SurfaceRenderPrimitive::Box {
                    style: SurfacePrimitiveStyle {
                        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                            root_incarnation: view.root_incarnation,
                            node: overlay.target.node,
                            part: overlay.kind.part(),
                        }),
                        position,
                        scale: [1.0, 1.0],
                        color,
                        opacity: overlay.opacity,
                        clip,
                    },
                    size: [rect[2] / units, rect[3] / units],
                    corner_radius: [0.0, 0.0],
                    border_width: 0.0,
                    border_color: [0.0, 0.0, 0.0, 0.0],
                    fill: crate::systems::surface::GuiShapeFill::Solid(color),
                    glow: None,
                });
            }
        }
        primitives
    }
}

// ---------------------------------------------------------------------------
// Command dispatch: route against the immutable snapshot, update cursors,
// queue ordered envelopes.
// ---------------------------------------------------------------------------

impl GuiInputSystem {
    /// Route one pointer-down: capture, focus, press tracking and the
    /// press-time value intent.
    #[allow(clippy::too_many_arguments)]
    fn route_down(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        pointer: u32,
        panel: Option<EntityId>,
        position: [f32; 2],
        button: GuiPointerButton,
        blockers: &[super::super::GuiBlockerHit],
        panel_distance: Option<f32>,
        projection: Option<ProjectedRay>,
    ) {
        // A captured pointer stays on its target under this system's scope,
        // but only its owning session may reuse it: identical pointer IDs
        // from another session never disturb the capture.
        if let Some(capture) = self.captures.get(&pointer).copied() {
            if capture.session != session {
                self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
                return;
            }
            if self.revalidate_capture(sim, &capture.target).is_some() {
                if let Some(position) =
                    Self::remap_capture_point(layout, sim, &capture.target, position, &projection)
                {
                    self.set_hover(session, tick, pointer, Some(capture.target), position);
                }
                return;
            }
            self.drop_capture(pointer);
        }
        // Presses establish the input context when unowned.
        if !self.check_owner(session, tick, input) {
            return;
        }
        self.scroll_drags.remove(&pointer);
        self.scroll_bar_presses.remove(&pointer);
        self.scroll_bar_hovers.remove(&pointer);
        // A primary press on a scroll bar drags its thumb or pages its
        // track; inside eligible ScrollView ancestry it arms a drag that may
        // later win the gesture from the tap.
        let mut drag = None;
        if button == GuiPointerButton::Primary {
            let resolved = match projection {
                Some(ray) => self.scroll_hit_projected(layout, sim, panel, blockers, ray),
                None => {
                    self.scroll_hit_logical(layout, sim, panel, position, blockers, panel_distance)
                }
            };
            if let Ok(resolved) = resolved {
                if let Some((part, bar)) = resolved.hit.bar {
                    self.press_scroll_bar(layout, sim, session, tick, pointer, resolved, part, bar);
                    return;
                }
                drag = self.scroll_drag_from(layout, sim, session, resolved);
            }
        }
        let routed = match self.route_point(
            layout,
            sim,
            panel,
            position,
            blockers,
            panel_distance,
            projection,
        ) {
            Ok(routed) => routed,
            Err(GuiUnhandledReason::NotFocusable) if drag.is_some() => {
                // Non-control ScrollView content: the drag owns the press.
                self.set_hover(session, tick, pointer, None, position);
                self.scroll_drags.extend(drag.map(|drag| (pointer, drag)));
                return;
            }
            Err(reason) => {
                self.unhandled(session, tick, input, reason);
                self.set_hover(session, tick, pointer, None, position);
                return;
            }
        };
        // Downstream consumers observe the routed logical point: on the
        // projected path the raw input is a viewport point, not a surface
        // point.
        let position = routed.point;
        // Disabled controls never activate: no capture, focus or intent.
        // Inside a ScrollView the press may still drag its content.
        if !Self::target_eligible(layout, sim, &routed.target) {
            self.set_hover(session, tick, pointer, None, position);
            match drag {
                Some(drag) => {
                    self.scroll_drags.insert(pointer, drag);
                }
                None => {
                    self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
                }
            }
            return;
        }
        // Touch arbitration: one press per control across pointers.
        if let Some(owner) = self.press_owners.get(&routed.target).copied()
            && owner != pointer
        {
            self.conflict(
                session,
                tick,
                Some(routed.target),
                GuiInputConflictReason::TouchArbitration {
                    owner_pointer: owner,
                },
            );
            return;
        }
        self.captures.insert(
            pointer,
            PointerCapture {
                target: routed.target,
                button,
                session,
                source_tick: tick,
            },
        );
        self.press_owners.insert(routed.target, pointer);
        self.set_hover(session, tick, pointer, Some(routed.target), position);
        // Taps arbitrate against dragging their ScrollView content; slider
        // and text drags keep their own capture.
        if matches!(
            routed.kind,
            Some(ControlKind::Button) | Some(ControlKind::Checkbox)
        ) && let Some(drag) = drag.filter(|drag| drag.entity == routed.target.entity)
        {
            self.scroll_drags.insert(pointer, drag);
        }
        // Pressing a control takes keyboard focus so the same tick can chain
        // focus-then-key inputs. Semantic focus follows the press; the ring
        // follows the focus-visible policy (keyboard-only outside text
        // inputs, recorded in GuiInteractionState::ring_visible and gui.md).
        self.set_focus(session, tick, Some(routed.target), false);
        // Non-control hits never reach here: routing reports them as
        // not focusable instead. Text presses place the caret by the retained
        // pen and cancel provisional composition; they queue no value.
        // Button and checkbox presses stay provisional through capture and
        // focus cursors: the value commits only when an eligible tap
        // completes on release, so holds, cancels and scroll-starts change
        // nothing.
        match routed.kind {
            Some(ControlKind::TextInput) => {
                self.cancel_composition_for_target(&routed.target);
                if let Some((base, _)) = self.base_text(&routed.root, &routed.target) {
                    // The caret maps in content coordinates: shift the
                    // viewport point back by scrolled ancestors.
                    let shift = self.scroll_shift_for(
                        layout,
                        sim,
                        routed.target.entity,
                        routed.target.node,
                    );
                    let caret = Self::pen_offset(
                        layout,
                        routed.target.entity,
                        routed.target.node,
                        &base,
                        position[0] - shift[0],
                    );
                    self.place_caret(&routed.root, &routed.target, session, caret);
                }
            }
            Some(ControlKind::Button) | Some(ControlKind::Checkbox) | None => {}
            Some(ControlKind::Slider) => {
                let content = slider_row(&routed.root, routed.target.node);
                let Some(content) = content else {
                    return;
                };
                // Fractions map against the scrolled rectangle, matching the
                // translated hit that selected this target.
                let shift =
                    self.scroll_shift_for(layout, sim, routed.target.entity, routed.target.node);
                let rect = [
                    routed.rect[0] + shift[0],
                    routed.rect[1] + shift[1],
                    routed.rect[2],
                    routed.rect[3],
                ];
                let (base, revision) = self.base_control(&routed.root, &routed.target);
                if let Some(value) = slider_value_at(&content, rect, position[0], &base) {
                    self.push_value_envelope(
                        session,
                        tick,
                        routed.target,
                        Some(pointer),
                        revision,
                        value,
                    );
                }
            }
        }
    }

    /// Route one pointer-up: complete taps on the pressed control and commit
    /// drag ends; a release off-target completes no tap. A release of a different
    /// button never completes the press.
    #[allow(clippy::too_many_arguments)]
    fn route_up(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        pointer: u32,
        button: GuiPointerButton,
        panel: Option<EntityId>,
        position: [f32; 2],
        blockers: &[super::super::GuiBlockerHit],
        panel_distance: Option<f32>,
        projection: Option<ProjectedRay>,
    ) {
        if button == GuiPointerButton::Primary
            && let Some(press) = self.scroll_bar_presses.get(&pointer)
        {
            if press.session != session {
                self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
                return;
            }
            // Releasing a bar completes nothing; the next move re-hovers.
            self.scroll_bar_presses.remove(&pointer);
            self.scroll_bar_hovers.remove(&pointer);
            return;
        }
        if button == GuiPointerButton::Primary
            && let Some(drag) = self.scroll_drags.get(&pointer)
        {
            if drag.session != session {
                self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
                return;
            }
            let active = drag.active.is_some();
            self.scroll_drags.remove(&pointer);
            // A drag that won the gesture already cancelled its tap, and a
            // drag over plain content has no tap to complete.
            if active || !self.captures.contains_key(&pointer) {
                self.set_hover(session, tick, pointer, None, position);
                return;
            }
        }
        let Some(capture) = self.captures.get(&pointer).copied() else {
            self.unhandled(session, tick, input, GuiUnhandledReason::NoCapture);
            return;
        };
        // Only the pressing session may release its capture; a foreign
        // release with an identical pointer ID leaves it untouched.
        if capture.session != session {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
            return;
        }
        if capture.button != button {
            self.unhandled(session, tick, input, GuiUnhandledReason::NoCapture);
            return;
        }
        let live = self.revalidate_capture(sim, &capture.target);
        self.drop_capture(pointer);
        self.set_hover(session, tick, pointer, None, position);
        let Some((kind, _)) = live else {
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return;
        };
        // A fresh hit decides same-control completion; capture is released
        // either way. The release point is the routed logical point so
        // slider fractions map against the retained view.
        let same = match self.route_point(
            layout,
            sim,
            panel,
            position,
            blockers,
            panel_distance,
            projection,
        ) {
            Ok(hit)
                if hit.target.entity == capture.target.entity
                    && hit.target.node == capture.target.node =>
            {
                Some(hit.point)
            }
            _ => None,
        };
        // Taps queue their click intents only here, so a release off the
        // pressed control completes nothing. A drag released off-target
        // still commits the values it already routed.
        let Some(position) = same else {
            if kind == ControlKind::Slider {
                self.set_hover(session, tick, pointer, None, position);
            }
            return;
        };
        // A target disabled mid-press completes nothing rather than
        // committing a stale tap.
        if !Self::target_eligible(layout, sim, &capture.target) {
            return;
        }
        self.set_hover(session, tick, pointer, Some(capture.target), position);
        match kind {
            ControlKind::Button => {
                self.push_envelope(
                    tick,
                    PendingEnvelope {
                        session,
                        epoch: self.owner_epoch,
                        source_tick: tick,
                        target: Some(capture.target),
                        pointer: Some(pointer),
                        kind: EnvelopeKind::PressButton,
                    },
                );
            }
            ControlKind::Checkbox => {
                let (_, root) = match producer_root(sim, capture.target.entity) {
                    Some(root) => root,
                    None => return,
                };
                let (base, revision) = self.base_control(&root, &capture.target);
                let GuiControlValue::Bool(current) = base else {
                    self.conflict(
                        session,
                        tick,
                        Some(capture.target),
                        GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
                    );
                    return;
                };
                // The tap completed in-bounds: exactly one commit. The
                // release already verified the target, so no later miss
                // cancels this envelope; session replacement still fences it.
                self.push_value_envelope(
                    session,
                    tick,
                    capture.target,
                    Some(pointer),
                    revision,
                    GuiControlValue::Bool(!current),
                );
            }
            ControlKind::Slider => {
                let (_, root) = match producer_root(sim, capture.target.entity) {
                    Some(root) => root,
                    None => return,
                };
                let content = slider_row(&root, capture.target.node);
                let Some(content) = content else {
                    return;
                };
                // The release point maps against the retained view: the
                // same immutable snapshot the press routed against,
                // translated by scrolled ancestors like the selecting hit.
                let shift =
                    self.scroll_shift_for(layout, sim, capture.target.entity, capture.target.node);
                let rect = layout
                    .view(capture.target.entity)
                    .and_then(|view| {
                        view.nodes
                            .iter()
                            .find(|node| node.node == capture.target.node)
                    })
                    .map(|node| node.rect)
                    .unwrap_or([0.0, 0.0, 0.0, 0.0]);
                let rect = [rect[0] + shift[0], rect[1] + shift[1], rect[2], rect[3]];
                let (base, revision) = self.base_control(&root, &capture.target);
                if let Some(value) = slider_value_at(&content, rect, position[0], &base) {
                    self.push_value_envelope(
                        session,
                        tick,
                        capture.target,
                        Some(pointer),
                        revision,
                        value,
                    );
                }
            }
            ControlKind::TextInput => {}
        }
    }

    /// Route one pointer-move: drag captured sliders, otherwise hover.
    #[allow(clippy::too_many_arguments)]
    fn route_move(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        pointer: u32,
        panel: Option<EntityId>,
        position: [f32; 2],
        blockers: &[super::super::GuiBlockerHit],
        panel_distance: Option<f32>,
        projection: Option<ProjectedRay>,
    ) {
        if let Some(press) = self.scroll_bar_presses.get(&pointer).cloned() {
            if press.session != session {
                self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
                return;
            }
            self.drag_scroll_bar(layout, sim, tick, &press, position, &projection);
            return;
        }
        if let Some(mut drag) = self.scroll_drags.remove(&pointer) {
            if drag.session != session {
                self.scroll_drags.insert(pointer, drag);
                self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
                return;
            }
            match self.continue_scroll_drag(
                layout,
                sim,
                tick,
                pointer,
                &mut drag,
                position,
                &projection,
            ) {
                ScrollDragStep::Scrolling => {
                    self.scroll_drags.insert(pointer, drag);
                    return;
                }
                ScrollDragStep::Pending => {
                    self.scroll_drags.insert(pointer, drag);
                    // A pressed tap keeps its own capture handling below;
                    // plain content waits for more travel.
                    if !self.captures.contains_key(&pointer) {
                        return;
                    }
                }
                ScrollDragStep::Lost => {}
            }
        }
        if let Some(capture) = self.captures.get(&pointer).copied() {
            // Drags follow only their pressing session; hover-only moves
            // below never acquire the context.
            if capture.session != session {
                self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
                return;
            }
            let Some((kind, root)) = self.revalidate_capture(sim, &capture.target) else {
                self.drop_capture(pointer);
                self.set_hover(session, tick, pointer, None, position);
                return;
            };
            // Drag continuations remap onto the owned panel through the
            // current ray: the camera may have moved since the press routed.
            let Some(position) =
                Self::remap_capture_point(layout, sim, &capture.target, position, &projection)
            else {
                return;
            };
            if kind == ControlKind::Button || kind == ControlKind::Checkbox {
                // Tap-versus-drag arbitration outside ScrollView content (a
                // scroll drag above wins first inside it): a button/checkbox
                // drag that leaves the press-time rectangle dies here with a
                // cancellation so a later release, cancel or scroll-start
                // commits nothing; slider and text drags below keep their
                // capture.
                let inside = layout
                    .view(capture.target.entity)
                    .and_then(|view| {
                        view.nodes
                            .iter()
                            .find(|record| record.node == capture.target.node)
                            .map(|record| {
                                self.scrolled_rect(view, &root, capture.target.entity, record)
                            })
                    })
                    .is_some_and(|rect| rect_contains_point(rect, position));
                if !inside {
                    self.pending_cancellations.push(GuiInputCancellation {
                        session,
                        source_tick: tick,
                        effect_tick: tick,
                        target: Some(capture.target),
                        reason: GuiInputCancelReason::GestureCancelled,
                    });
                    self.drop_capture(pointer);
                    self.set_hover(session, tick, pointer, None, position);
                    return;
                }
                self.set_hover(session, tick, pointer, Some(capture.target), position);
                return;
            }
            if kind == ControlKind::TextInput {
                // Drag extends the selection from the press-time anchor.
                // Provisional composition cancels on explicit selection drags.
                self.cancel_composition_for_target(&capture.target);
                if let Some((base, revision)) = self.base_text(&root, &capture.target) {
                    // Selection drags map in content coordinates like the
                    // press-time caret.
                    let shift = self.scroll_shift_for(
                        layout,
                        sim,
                        capture.target.entity,
                        capture.target.node,
                    );
                    let caret = Self::pen_offset(
                        layout,
                        capture.target.entity,
                        capture.target.node,
                        &base,
                        position[0] - shift[0],
                    );
                    let key = capture.target;
                    let anchor = self
                        .text_carets
                        .get(&key)
                        .and_then(|cursor| {
                            if cursor.revision == revision {
                                cursor.anchor.or(Some(cursor.caret))
                            } else {
                                None
                            }
                        })
                        .or(Some(super::text_edit::snap_to_boundary(
                            &base,
                            caret.min(base.len() as u32),
                        )));
                    let anchor = match anchor {
                        Some(anchor) if anchor != caret => Some(anchor),
                        _ => None,
                    };
                    self.store_cursor(
                        key,
                        TextCursor {
                            caret,
                            anchor,
                            revision,
                            session,
                        },
                    );
                }
                self.set_hover(session, tick, pointer, Some(capture.target), position);
                return;
            }
            if kind != ControlKind::Slider {
                self.set_hover(session, tick, pointer, Some(capture.target), position);
                return;
            }
            let content = slider_row(&root, capture.target.node);
            let Some(content) = content else {
                return;
            };
            // Drag fractions map against the scrolled rectangle, matching
            // the translated hit that selected this target.
            let shift =
                self.scroll_shift_for(layout, sim, capture.target.entity, capture.target.node);
            let rect = layout
                .view(capture.target.entity)
                .and_then(|view| {
                    view.nodes
                        .iter()
                        .find(|node| node.node == capture.target.node)
                })
                .map(|node| node.rect)
                .unwrap_or([0.0, 0.0, 0.0, 0.0]);
            let rect = [rect[0] + shift[0], rect[1] + shift[1], rect[2], rect[3]];
            let (base, revision) = self.base_control(&root, &capture.target);
            if let Some(value) = slider_value_at(&content, rect, position[0], &base) {
                // Skip envelopes that would commit no change over the
                // prediction, keeping drags quiet.
                if base != value {
                    self.push_value_envelope(
                        session,
                        tick,
                        capture.target,
                        Some(pointer),
                        revision,
                        value,
                    );
                }
            }
            self.set_hover(session, tick, pointer, Some(capture.target), position);
            return;
        }
        // Hover-only moves observe without acquiring: they track for the
        // caller while unowned but never disturb another owner's hover.
        if self.owner.is_some_and(|owner| owner.session != session) {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
            return;
        }
        // Scroll bars paint above their content, so they take the hover.
        match self.scroll_bar_under(
            layout,
            sim,
            panel,
            position,
            blockers,
            panel_distance,
            projection,
        ) {
            Some((target, part)) => {
                self.set_hover(session, tick, pointer, None, position);
                self.scroll_bar_hovers.insert(
                    pointer,
                    ScrollBarHover {
                        target,
                        part,
                        session,
                    },
                );
                return;
            }
            None => {
                self.scroll_bar_hovers.remove(&pointer);
            }
        }
        match self.route_point(
            layout,
            sim,
            panel,
            position,
            blockers,
            panel_distance,
            projection,
        ) {
            Ok(routed) => self.set_hover(session, tick, pointer, Some(routed.target), routed.point),
            Err(_) => self.set_hover(session, tick, pointer, None, position),
        }
    }

    /// Resolve one scroll point to its topmost scroll-aware hit: panel,
    /// root incarnation, node, scrolled rectangle and the logical
    /// point. Mirrors the logical panel loop, but hit-tests with ancestor
    /// scroll shifts applied and accepts containers so wheel input over a
    /// ScrollView viewport or a non-control child still finds its ancestry.
    fn scroll_hit_logical(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        panel: Option<EntityId>,
        position: [f32; 2],
        blockers: &[super::super::GuiBlockerHit],
        panel_distance: Option<f32>,
    ) -> Result<ScrollResolved, GuiUnhandledReason> {
        let mut best: Option<(EntityId, ScrollViewHit, u64)> = None;
        let panels: Vec<EntityId> = match panel {
            Some(entity) => vec![entity],
            None => layout.evaluated_entities(),
        };
        for entity in panels {
            let Some(view) = layout.view(entity) else {
                continue;
            };
            let Some((_, root)) = producer_root(sim, entity) else {
                continue;
            };
            let Some(hit) = self.scroll_hit_in_view(view, &root, entity, position, true) else {
                continue;
            };
            let replace = best.is_none_or(|(current, _, _)| entity > current);
            if replace {
                best = Some((entity, hit, view.root_incarnation));
            }
        }
        let Some((entity, hit, root_incarnation)) = best else {
            return Err(GuiUnhandledReason::NoPanelHit);
        };
        let node = hit.node;
        let decided = match panel_distance {
            Some(distance) => {
                let hit = GuiHit {
                    node,
                    position,
                };
                match super::super::resolve_panel_hit(Some(distance), Some(hit), blockers) {
                    super::super::GuiPanelResolution::Panel(decided) => decided,
                    super::super::GuiPanelResolution::Blocked {
                        entity,
                    } => {
                        return Err(GuiUnhandledReason::Blocked {
                            entity,
                        });
                    }
                    super::super::GuiPanelResolution::Miss => {
                        return Err(GuiUnhandledReason::NoPanelHit);
                    }
                }
            }
            None => GuiHit {
                node,
                position,
            },
        };
        match recheck_hit(
            sim,
            layout,
            GuiInputTarget {
                entity,
                root_incarnation,
                node: decided.node,
            },
            hit.rect,
            position,
        ) {
            Some(_) => Ok(ScrollResolved {
                entity,
                root_incarnation,
                hit: ScrollViewHit {
                    node: decided.node,
                    ..hit
                },
                point: position,
            }),
            None => Err(GuiUnhandledReason::StaleTarget),
        }
    }

    /// Resolve one scroll point through the current-tick camera ray, keeping
    /// the nearest front-facing panel with geometry-resolved blockers.
    /// Mirrors the projected panel loop with scroll-aware hit testing.
    fn scroll_hit_projected(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        panel: Option<EntityId>,
        blockers: &[super::super::GuiBlockerHit],
        ray: ProjectedRay,
    ) -> Result<ScrollResolved, GuiUnhandledReason> {
        let geometry_ray = GeometryRay {
            origin: ray.origin,
            direction: ray.direction,
        };
        let mut best: Option<ScrollCandidate> = None;
        let panels: Vec<EntityId> = match panel {
            Some(entity) => vec![entity],
            None => layout.evaluated_entities(),
        };
        for entity in panels {
            let Some((distance, logical)) =
                Self::project_panel_point(layout, sim, entity, &ray, true)
            else {
                continue;
            };
            let Some(view) = layout.view(entity) else {
                continue;
            };
            let Some((_, root)) = producer_root(sim, entity) else {
                continue;
            };
            let Some(hit) = self.scroll_hit_in_view(view, &root, entity, logical, true) else {
                continue;
            };
            let better = match best {
                None => true,
                Some((current_distance, current, _, _, _)) => {
                    distance < current_distance
                        || (distance == current_distance && entity < current)
                }
            };
            if better {
                best = Some((distance, entity, hit, view.root_incarnation, logical));
            }
        }
        let Some((distance, entity, hit, root_incarnation, logical)) = best else {
            return Err(GuiUnhandledReason::NoPanelHit);
        };
        let node = hit.node;
        let mut resolved = Vec::with_capacity(blockers.len());
        for blocker in blockers {
            let index = blocker.entity.index() as usize;
            let Some(component) = sim.components.picking_geometry(index) else {
                continue;
            };
            let Some(evaluation) = component.runtime.evaluation.as_ref() else {
                continue;
            };
            let Ok(shape) = evaluation.evaluated() else {
                continue;
            };
            let mut intervals = Vec::new();
            if let Some(found) = shape.ray_intersection_with_scratch(
                &geometry_ray,
                ray.near,
                ray.far,
                &mut intervals,
            ) {
                let resolved_distance = found.distance as f32;
                if resolved_distance.is_finite() {
                    resolved.push(super::super::GuiBlockerHit {
                        distance: resolved_distance,
                        entity: blocker.entity,
                    });
                }
            }
        }
        let panel_hit = GuiHit {
            node,
            position: logical,
        };
        match super::super::resolve_panel_hit(Some(distance as f32), Some(panel_hit), &resolved) {
            super::super::GuiPanelResolution::Panel(decided) => {
                match recheck_hit(
                    sim,
                    layout,
                    GuiInputTarget {
                        entity,
                        root_incarnation,
                        node: decided.node,
                    },
                    hit.rect,
                    logical,
                ) {
                    Some(_) => Ok(ScrollResolved {
                        entity,
                        root_incarnation,
                        hit: ScrollViewHit {
                            node: decided.node,
                            ..hit
                        },
                        point: logical,
                    }),
                    None => Err(GuiUnhandledReason::StaleTarget),
                }
            }
            super::super::GuiPanelResolution::Blocked {
                entity,
            } => Err(GuiUnhandledReason::Blocked {
                entity,
            }),
            super::super::GuiPanelResolution::Miss => Err(GuiUnhandledReason::NoPanelHit),
        }
    }

    /// Route one scroll: resolve the eligible ScrollView ancestry under the
    /// scroll point, consume the delta innermost-first against evaluated
    /// extents, pass the remainder outward and queue one envelope per
    /// ScrollView that consumed movement. A scroll that moves content
    /// disarms held button/checkbox taps on the same panel and session, so
    /// scroll-starts never toggle. A scroll no ScrollView consumes, over
    /// non-scrollable content or at every edge, is reported unhandled once
    /// so scene controls can take it; a remainder left after partial
    /// consumption drops at the outermost edge instead, because the input
    /// was handled.
    #[allow(clippy::too_many_arguments)]
    fn route_scroll(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        panel: Option<EntityId>,
        position: [f32; 2],
        delta: [f32; 2],
        blockers: &[super::super::GuiBlockerHit],
        panel_distance: Option<f32>,
        projection: Option<ProjectedRay>,
    ) {
        if !delta.iter().all(|lane| lane.is_finite()) {
            return;
        }
        // Scrolls manipulate scrolled state, so they establish the input
        // context when unowned.
        if !self.check_owner(session, tick, input) {
            return;
        }
        let resolved = match projection {
            Some(ray) => self.scroll_hit_projected(layout, sim, panel, blockers, ray),
            None => self.scroll_hit_logical(layout, sim, panel, position, blockers, panel_distance),
        };
        let ScrollResolved {
            entity,
            root_incarnation,
            hit,
            ..
        } = match resolved {
            Ok(resolved) => resolved,
            Err(reason) => {
                self.unhandled(session, tick, input, reason);
                return;
            }
        };
        let hit = hit.node;
        let target = GuiInputTarget {
            entity,
            node: hit,
            root_incarnation,
        };
        if !Self::target_eligible(layout, sim, &target) {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        }
        let (_, root) = match producer_root(sim, entity) {
            Some(root) => root,
            None => {
                self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
                return;
            }
        };
        let chain = Self::scroll_chain(&root, hit);
        let Some(view) = layout.view(entity) else {
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return;
        };
        if !self.consume_scroll(view, entity, &chain, delta, session, tick) {
            self.unhandled(session, tick, input, GuiUnhandledReason::ScrollUnconsumed);
            return;
        }
        // Leftover remainder drops at the outer edge: movement clamps.
        self.cancel_held_taps_for_scroll(sim, session, tick, entity);
    }

    /// Routed offset of one ScrollView: its committed offset plus every
    /// delta already queued for it this tick, so same-tick scrolls chain
    /// against the movement their predecessors consumed.
    fn routed_scroll_offset(&self, target: &GuiInputTarget) -> [f32; 2] {
        self.scroll_predicted
            .get(target)
            .copied()
            .unwrap_or_else(|| {
                self.scroll_offsets
                    .get(target)
                    .map(|cursor| cursor.offset)
                    .unwrap_or([0.0, 0.0])
            })
    }

    /// Consume one delta innermost-first along a ScrollView chain against
    /// evaluated extents and routed offsets, passing each remainder outward
    /// and queueing one envelope per ScrollView that consumed movement.
    /// Returns whether any ScrollView consumed movement; a leftover
    /// remainder at the outermost edge drops.
    fn consume_scroll(
        &mut self,
        view: &GuiEvaluatedView,
        entity: EntityId,
        chain: &[GuiNodeId],
        delta: [f32; 2],
        session: u64,
        tick: u64,
    ) -> bool {
        let mut remainder = delta;
        let mut consumed_any = false;
        for &scroll in chain {
            if remainder == [0.0, 0.0] {
                break;
            }
            if !view.nodes.iter().any(|record| record.node == scroll) {
                continue;
            }

            let target = GuiInputTarget {
                entity,
                root_incarnation: view.root_incarnation,
                node: scroll,
            };
            let max = Self::scroll_max(view, scroll);
            let current = self.routed_scroll_offset(&target);
            let next = [
                (current[0] + remainder[0]).clamp(0.0, max[0]),
                (current[1] + remainder[1]).clamp(0.0, max[1]),
            ];
            let consumed = [next[0] - current[0], next[1] - current[1]];
            if consumed != [0.0, 0.0] {
                consumed_any = true;
                self.scroll_predicted.insert(target, next);
                self.push_envelope(
                    tick,
                    PendingEnvelope {
                        session,
                        epoch: self.owner_epoch,
                        source_tick: tick,
                        target: Some(target),
                        pointer: None,
                        kind: EnvelopeKind::Scroll {
                            delta: consumed,
                        },
                    },
                );
            }
            remainder = [remainder[0] - consumed[0], remainder[1] - consumed[1]];
        }
        consumed_any
    }

    /// Innermost-first eligible ScrollView ancestry of one hit node that
    /// the retained view evaluated.
    fn eligible_scroll_chain(
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        entity: EntityId,
        root_incarnation: u64,
        node: GuiNodeId,
    ) -> Vec<GuiNodeId> {
        let (Some(view), Some((_, root))) = (layout.view(entity), producer_root(sim, entity))
        else {
            return Vec::new();
        };
        Self::scroll_chain(&root, node)
            .into_iter()
            .filter(|&node| {
                Self::target_eligible(
                    layout,
                    sim,
                    &GuiInputTarget {
                        entity,
                        root_incarnation,
                        node,
                    },
                ) && view.nodes.iter().any(|record| record.node == node)
            })
            .collect()
    }

    /// Arm a scroll drag for a primary press from its scroll-aware hit,
    /// keeping the innermost-first eligible ScrollView ancestry. None
    /// outside ScrollView content.
    fn scroll_drag_from(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        session: u64,
        resolved: ScrollResolved,
    ) -> Option<ScrollDrag> {
        let chain = Self::eligible_scroll_chain(
            layout,
            sim,
            resolved.entity,
            resolved.root_incarnation,
            resolved.hit.node,
        );
        if chain.is_empty() {
            return None;
        }

        Some(ScrollDrag {
            entity: resolved.entity,
            root_incarnation: resolved.root_incarnation,
            chain,
            origin: resolved.point,
            last: resolved.point,
            active: None,
            session,
        })
    }

    /// Press a scroll bar part: capture the pointer on it, and page the
    /// track by one viewport extent toward the pressed side. Thumb presses
    /// scroll only once the pointer moves. Bars never take focus.
    #[allow(clippy::too_many_arguments)]
    fn press_scroll_bar(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        pointer: u32,
        resolved: ScrollResolved,
        part: GuiPrimitivePart,
        bar: GuiScrollBar,
    ) {
        let target = GuiInputTarget {
            entity: resolved.entity,
            root_incarnation: resolved.root_incarnation,
            node: resolved.hit.node,
        };
        let chain = Self::eligible_scroll_chain(
            layout,
            sim,
            target.entity,
            target.root_incarnation,
            target.node,
        );
        let Some(view) = layout.view(target.entity) else {
            return;
        };
        if chain.first() != Some(&target.node) {
            return;
        }
        self.set_hover(session, tick, pointer, None, resolved.point);

        let axis = bar.axis;
        let along = resolved.point[axis];
        let thumb = matches!(
            part,
            GuiPrimitivePart::ScrollThumbX | GuiPrimitivePart::ScrollThumbY
        );
        if !thumb {
            let mut delta = [0.0, 0.0];
            delta[axis] = if along < bar.thumb_start() {
                -bar.page
            } else {
                bar.page
            };
            if self.consume_scroll(view, target.entity, &chain, delta, session, tick) {
                self.cancel_held_taps_for_scroll(sim, session, tick, target.entity);
            }
        }
        self.scroll_bar_hovers.insert(
            pointer,
            ScrollBarHover {
                target,
                part,
                session,
            },
        );
        self.scroll_bar_presses.insert(
            pointer,
            ScrollBarPress {
                target,
                part,
                chain,
                grab: along - bar.thumb_start(),
                session,
            },
        );
    }

    /// Drag a pressed scroll bar thumb: the thumb follows the pointer along
    /// the track, clamped to its ends, and the ScrollView scrolls to the
    /// matching offset through the ordinary consumption.
    fn drag_scroll_bar(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        tick: u64,
        press: &ScrollBarPress,
        position: [f32; 2],
        projection: &Option<ProjectedRay>,
    ) {
        let Some(axis) = scroll_bar_axis(press.part) else {
            return;
        };
        if !matches!(
            press.part,
            GuiPrimitivePart::ScrollThumbX | GuiPrimitivePart::ScrollThumbY
        ) {
            return;
        }
        let Some(point) =
            Self::remap_capture_point(layout, sim, &press.target, position, projection)
        else {
            return;
        };
        let (Some(view), Some((_, root))) = (
            layout.view(press.target.entity),
            producer_root(sim, press.target.entity),
        ) else {
            return;
        };
        let Some(bar) = self.scroll_bar_of(view, &root, &press.target, axis) else {
            return;
        };
        let desired = bar.offset_for_thumb(point[axis] - press.grab);
        let mut delta = [0.0, 0.0];
        delta[axis] = desired - self.routed_scroll_offset(&press.target)[axis];
        if delta[axis].is_finite()
            && self.consume_scroll(
                view,
                press.target.entity,
                &press.chain,
                delta,
                press.session,
                tick,
            )
        {
            self.cancel_held_taps_for_scroll(sim, press.session, tick, press.target.entity);
        }
    }

    /// Scroll bar part under a hover point, resolved like a scroll.
    #[allow(clippy::too_many_arguments)]
    fn scroll_bar_under(
        &self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        panel: Option<EntityId>,
        position: [f32; 2],
        blockers: &[super::super::GuiBlockerHit],
        panel_distance: Option<f32>,
        projection: Option<ProjectedRay>,
    ) -> Option<(GuiInputTarget, GuiPrimitivePart)> {
        let resolved = match projection {
            Some(ray) => self.scroll_hit_projected(layout, sim, panel, blockers, ray),
            None => self.scroll_hit_logical(layout, sim, panel, position, blockers, panel_distance),
        }
        .ok()?;
        let (part, _) = resolved.hit.bar?;
        Some((
            GuiInputTarget {
                entity: resolved.entity,
                root_incarnation: resolved.root_incarnation,
                node: resolved.hit.node,
            },
            part,
        ))
    }

    /// Advance one pointer's scroll drag. A pending drag wins once its
    /// travel passes the slop and a ScrollView in its chain has capacity on
    /// the dominant axis: the pointer's pending tap cancels and the whole
    /// travel since the press scrolls. An active drag scrolls by each
    /// move's travel, innermost-first from the winning ScrollView outward.
    #[allow(clippy::too_many_arguments)]
    fn continue_scroll_drag(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        tick: u64,
        pointer: u32,
        drag: &mut ScrollDrag,
        position: [f32; 2],
        projection: &Option<ProjectedRay>,
    ) -> ScrollDragStep {
        let Some(view) = layout
            .view(drag.entity)
            .filter(|view| view.root_incarnation == drag.root_incarnation)
        else {
            return ScrollDragStep::Lost;
        };
        let point = match projection {
            Some(ray) => match Self::project_panel_point(layout, sim, drag.entity, ray, false) {
                Some((_, point)) => point,
                None => return ScrollDragStep::Pending,
            },
            None => position,
        };
        if !point.iter().all(|lane| lane.is_finite()) {
            return ScrollDragStep::Pending;
        }

        let winner = match drag.active {
            Some(winner) => winner,
            None => {
                let travel = [point[0] - drag.origin[0], point[1] - drag.origin[1]];
                let slop =
                    SCROLL_DRAG_SLOP * view.root_bounds[2].abs().min(view.root_bounds[3].abs());
                if travel[0].hypot(travel[1]) <= slop {
                    return ScrollDragStep::Pending;
                }
                let axis = usize::from(travel[1].abs() >= travel[0].abs());
                let Some(winner) = drag
                    .chain
                    .iter()
                    .position(|&node| Self::scroll_max(view, node)[axis] > 0.0)
                else {
                    return ScrollDragStep::Pending;
                };
                // The drag wins: the pointer's pending tap dies with one
                // cancellation, so its release commits nothing.
                if let Some(capture) = self.captures.get(&pointer).copied() {
                    self.pending_cancellations.push(GuiInputCancellation {
                        session: drag.session,
                        source_tick: tick,
                        effect_tick: tick,
                        target: Some(capture.target),
                        reason: GuiInputCancelReason::GestureCancelled,
                    });
                }
                self.set_hover(drag.session, tick, pointer, None, point);
                self.drop_capture(pointer);
                drag.active = Some(winner);
                winner
            }
        };

        // Content follows the pointer: travel down scrolls toward the start.
        // Offsets are ScrollView-local, so travel divides by its scale.
        let scale = view
            .nodes
            .iter()
            .find(|record| record.node == drag.chain[winner])
            .map_or([1.0, 1.0], |record| record.acc_scale);
        let travel = [drag.last[0] - point[0], drag.last[1] - point[1]];
        let delta = [
            travel[0] / scale[0].abs().max(f32::MIN_POSITIVE),
            travel[1] / scale[1].abs().max(f32::MIN_POSITIVE),
        ];
        drag.last = point;
        let chain = drag.chain[winner..].to_vec();
        if self.consume_scroll(view, drag.entity, &chain, delta, drag.session, tick) {
            self.cancel_held_taps_for_scroll(sim, drag.session, tick, drag.entity);
        }
        ScrollDragStep::Scrolling
    }

    /// Disarm held button/checkbox taps on one panel after a scroll moved
    /// content there. Slider and text captures own their drag gestures and
    /// survive; the cancelled taps report once and release into no capture.
    fn cancel_held_taps_for_scroll(
        &mut self,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        entity: EntityId,
    ) {
        let held: Vec<(u32, GuiInputTarget)> = self
            .captures
            .iter()
            .filter(|(_, capture)| capture.session == session && capture.target.entity == entity)
            .map(|(pointer, capture)| (*pointer, capture.target))
            .collect();
        for (pointer, target) in held {
            let (_, root) = match producer_root(sim, target.entity) {
                Some(root) => root,
                None => continue,
            };
            let taps = root
                .nodes()
                .node(target.node)
                .and_then(|live| ControlKind::of(&live.data))
                .is_some_and(|kind| kind == ControlKind::Button || kind == ControlKind::Checkbox);
            if !taps {
                continue;
            }
            self.pending_cancellations.push(GuiInputCancellation {
                session,
                source_tick: tick,
                effect_tick: tick,
                target: Some(target),
                reason: GuiInputCancelReason::GestureCancelled,
            });
            self.drop_capture(pointer);
        }
    }

    /// Route one key press on the focused control.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn route_key(
        &mut self,
        layout: &GuiLayoutSystem,
        sim: &WorldSimulationState,
        view: Option<&GuiKeyboardView>,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        key: GuiKey,
    ) {
        let Some(focus) = self.focus else {
            match key {
                GuiKey::Tab | GuiKey::BackTab => {
                    let backward = key == GuiKey::BackTab;
                    self.enter_traversal(layout, sim, view, session, tick, input, backward);
                }
                _ => self.unhandled(session, tick, input, GuiUnhandledReason::NoFocus),
            }
            return;
        };
        // Keys follow only the focus owner's session; follow-on edits never
        // acquire the context on their own.
        if focus.session != session {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
            return;
        }
        let Some((kind, root)) = self.revalidate_capture(sim, &focus.target) else {
            self.clear_focus();
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return;
        };
        // Disabled focus targets ignore keys; the application backstop
        // below cancels any queued commit that raced the disable.
        if !Self::target_eligible(layout, sim, &focus.target) {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        }
        match key {
            GuiKey::Tab | GuiKey::BackTab => {
                let order = self.traversal_order(layout, sim, view, &root, &focus.target);
                let len = order.len();
                if len == 0 {
                    self.unhandled(session, tick, input, GuiUnhandledReason::NoFocus);
                    return;
                }

                let backward = key == GuiKey::BackTab;
                let next = match order.iter().position(|target| *target == focus.target) {
                    Some(index) if backward => order[(index + len - 1) % len],
                    Some(index) => order[(index + 1) % len],
                    None if backward => order[len - 1],
                    None => order[0],
                };
                self.set_focus(session, tick, Some(next), true);
            }
            GuiKey::Escape => {
                if let Some(old) = self.focus.map(|focus| focus.target) {
                    self.cancel_composition_for_target(&old);
                    if let Some(cursor) = self.text_carets.get_mut(&old) {
                        cursor.anchor = None;
                    }
                }
                self.clear_focus();
                self.push_envelope(
                    tick,
                    PendingEnvelope {
                        session,
                        epoch: self.owner_epoch,
                        source_tick: tick,
                        target: None,
                        pointer: None,
                        kind: EnvelopeKind::Focus {
                            focus: None,
                            keyboard: false,
                        },
                    },
                );
            }
            GuiKey::Enter | GuiKey::Space => match kind {
                ControlKind::Button => {
                    self.push_envelope(
                        tick,
                        PendingEnvelope {
                            session,
                            epoch: self.owner_epoch,
                            source_tick: tick,
                            target: Some(focus.target),
                            pointer: None,
                            kind: EnvelopeKind::PressButton,
                        },
                    );
                }
                ControlKind::Checkbox => {
                    let (base, revision) = self.base_control(&root, &focus.target);
                    let GuiControlValue::Bool(current) = base else {
                        self.conflict(
                            session,
                            tick,
                            Some(focus.target),
                            GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
                        );
                        return;
                    };
                    self.push_value_envelope(
                        session,
                        tick,
                        focus.target,
                        None,
                        revision,
                        GuiControlValue::Bool(!current),
                    );
                }
                ControlKind::TextInput if key == GuiKey::Space => {
                    self.append_text(session, tick, input, &root, &focus.target, " ".into());
                }
                ControlKind::TextInput => self.route_submit(session, tick, &root, focus.target),
                _ => self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable),
            },
            GuiKey::Backspace => {
                if kind != ControlKind::TextInput {
                    self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
                    return;
                }
                self.cancel_composition_for_target(&focus.target);
                let Some((text, revision, cursor)) = self.cursor_for(&root, &focus.target, session)
                else {
                    self.conflict(
                        session,
                        tick,
                        Some(focus.target),
                        GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
                    );
                    return;
                };
                let Some(outcome) = super::text_edit::backspace(&text, cursor.caret, cursor.anchor)
                else {
                    self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
                    return;
                };
                if outcome.text.len() > super::super::tree::nodes::MAX_TEXT_BYTES {
                    self.conflict(
                        session,
                        tick,
                        Some(focus.target),
                        GuiInputConflictReason::AdmissionFailed(ErrorReason::Capacity),
                    );
                    return;
                }
                self.push_value_envelope(
                    session,
                    tick,
                    focus.target,
                    None,
                    revision,
                    GuiControlValue::Text(outcome.text.clone()),
                );
                self.store_committed_cursor(&focus.target, outcome.caret, revision, session);
            }
            GuiKey::Left | GuiKey::Right | GuiKey::Up | GuiKey::Down => {
                if kind == ControlKind::TextInput {
                    if matches!(key, GuiKey::Up | GuiKey::Down) {
                        self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
                        return;
                    }
                    self.cancel_composition_for_target(&focus.target);
                    let Some((text, revision, cursor)) =
                        self.cursor_for(&root, &focus.target, session)
                    else {
                        self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
                        return;
                    };
                    // A non-collapsed selection collapses first; otherwise
                    // step one grapheme in the key direction.
                    let caret = match cursor.selection() {
                        Some((start, _)) if key == GuiKey::Left => start,
                        Some((_, end)) if key == GuiKey::Right => end,
                        _ if key == GuiKey::Left => {
                            super::text_edit::prev_boundary(&text, cursor.caret)
                                .unwrap_or(cursor.caret)
                        }
                        _ => super::text_edit::next_boundary(&text, cursor.caret)
                            .unwrap_or(cursor.caret),
                    };
                    self.store_cursor(
                        focus.target,
                        TextCursor {
                            caret,
                            anchor: None,
                            revision,
                            session,
                        },
                    );
                    return;
                }
                if kind != ControlKind::Slider {
                    self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
                    return;
                }
                let content = slider_row(&root, focus.target.node);
                let Some(content) = content else {
                    self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
                    return;
                };
                let (base, revision) = self.base_control(&root, &focus.target);
                let steps = match key {
                    GuiKey::Left | GuiKey::Down => -1.0,
                    _ => 1.0,
                };
                match slider_nudge(&content, &base, steps) {
                    Some(value) => {
                        self.push_value_envelope(session, tick, focus.target, None, revision, value)
                    }
                    None => self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable),
                }
            }
            GuiKey::Home | GuiKey::End => {
                if kind == ControlKind::TextInput {
                    self.cancel_composition_for_target(&focus.target);
                    let Some((text, revision, _)) = self.cursor_for(&root, &focus.target, session)
                    else {
                        self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
                        return;
                    };
                    let caret = if key == GuiKey::Home {
                        0
                    } else {
                        text.len() as u32
                    };
                    self.store_cursor(
                        focus.target,
                        TextCursor {
                            caret,
                            anchor: None,
                            revision,
                            session,
                        },
                    );
                    return;
                }
                if kind != ControlKind::Slider {
                    self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
                    return;
                }
                let content = slider_row(&root, focus.target.node);
                let Some(GuiNodeDataRow {
                    min: Some(min),
                    max: Some(max),
                    ..
                }) = content
                else {
                    self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
                    return;
                };
                let (_, revision) = self.base_control(&root, &focus.target);
                let value = if key == GuiKey::Home {
                    min
                } else {
                    max
                };
                self.push_value_envelope(
                    session,
                    tick,
                    focus.target,
                    None,
                    revision,
                    GuiControlValue::Scalar(value),
                );
            }
            GuiKey::Delete => {
                if kind != ControlKind::TextInput {
                    self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
                    return;
                }
                self.cancel_composition_for_target(&focus.target);
                let Some((text, revision, cursor)) = self.cursor_for(&root, &focus.target, session)
                else {
                    self.conflict(
                        session,
                        tick,
                        Some(focus.target),
                        GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
                    );
                    return;
                };
                let Some(outcome) =
                    super::text_edit::delete_forward(&text, cursor.caret, cursor.anchor)
                else {
                    self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
                    return;
                };
                if outcome.text.len() > super::super::tree::nodes::MAX_TEXT_BYTES {
                    self.conflict(
                        session,
                        tick,
                        Some(focus.target),
                        GuiInputConflictReason::AdmissionFailed(ErrorReason::Capacity),
                    );
                    return;
                }
                self.push_value_envelope(
                    session,
                    tick,
                    focus.target,
                    None,
                    revision,
                    GuiControlValue::Text(outcome.text.clone()),
                );
                self.store_committed_cursor(&focus.target, outcome.caret, revision, session);
            }
        }
    }

    /// Queue a submission of the focused text input's predicted text. Enter
    /// during an active composition belongs to the IME: it is consumed
    /// without submitting, and the composition stays for the platform to
    /// commit or cancel.
    fn route_submit(&mut self, session: u64, tick: u64, root: &GuiRoot, target: GuiInputTarget) {
        let composing = self
            .composition
            .as_ref()
            .is_some_and(|composed| composed.target == target);
        if composing {
            return;
        }

        let expected_revision = self.base_revision(root, &target);
        self.push_envelope(
            tick,
            PendingEnvelope {
                session,
                epoch: self.owner_epoch,
                source_tick: tick,
                target: Some(target),
                pointer: None,
                kind: EnvelopeKind::Submit {
                    expected_revision,
                },
            },
        );
    }

    /// Insert payload text at the caret, replacing the selection, with
    /// revision chaining. Direct edits cancel provisional composition first.
    fn append_text(
        &mut self,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        root: &GuiRoot,
        target: &GuiInputTarget,
        payload: String,
    ) {
        let _ = input;
        self.cancel_composition_for_target(target);
        let Some((text, revision, cursor)) = self.cursor_for(root, target, session) else {
            self.conflict(
                session,
                tick,
                Some(*target),
                GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
            );
            return;
        };
        let (start, end) = match cursor.selection() {
            Some((start, end)) => (start, end),
            None => (cursor.caret, cursor.caret),
        };
        let outcome = super::text_edit::insert_at(&text, start, end, &payload);
        if outcome.text.len() > super::super::tree::nodes::MAX_TEXT_BYTES {
            self.conflict(
                session,
                tick,
                Some(*target),
                GuiInputConflictReason::AdmissionFailed(ErrorReason::Capacity),
            );
            return;
        }
        self.push_value_envelope(
            session,
            tick,
            *target,
            None,
            revision,
            GuiControlValue::Text(outcome.text.clone()),
        );
        self.store_committed_cursor(target, outcome.caret, revision, session);
    }

    /// Current keyboard focus for one text, selection or composition
    /// command, or None after reporting why it cannot route. A fence naming
    /// another input context, focus generation or target (including one
    /// observed before the focus cleared) conflicts before any other check;
    /// otherwise text follows only the focus owner's session.
    fn text_command_focus(
        &mut self,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        fence: Option<&GuiTextFence>,
    ) -> Option<GuiInputFocus> {
        if let Some(fence) = fence {
            let current = self.focus.is_some_and(|focus| {
                fence.context_generation == self.owner_epoch
                    && fence.focus_generation == self.focus_generation
                    && fence.target == focus.target
            });
            if !current {
                self.conflict(
                    session,
                    tick,
                    Some(fence.target),
                    GuiInputConflictReason::FocusMismatch,
                );
                return None;
            }
        }

        let Some(focus) = self.focus else {
            self.unhandled(session, tick, input, GuiUnhandledReason::NoFocus);
            return None;
        };
        if focus.session != session {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
            return None;
        }
        Some(focus)
    }

    /// Check one fenced text command's revision against the current
    /// predicted or committed revision. Selections name their offsets'
    /// revision exactly; insertions and composition may name an older
    /// revision of the same focus generation, never a newer one. Reports
    /// the conflict and returns false when stale.
    fn text_revision_current(
        &mut self,
        session: u64,
        tick: u64,
        target: GuiInputTarget,
        fence: Option<&GuiTextFence>,
        revision: u32,
        exact: bool,
    ) -> bool {
        let Some(fence) = fence else {
            return true;
        };
        let stale = if exact {
            fence.revision != revision
        } else {
            fence.revision > revision
        };
        if stale {
            self.conflict(
                session,
                tick,
                Some(target),
                GuiInputConflictReason::RevisionMismatch {
                    expected: fence.revision,
                    found: revision,
                },
            );
        }
        !stale
    }

    /// Current predicted or committed revision for one control, without
    /// copying its value.
    fn base_revision(&self, root: &GuiRoot, target: &GuiInputTarget) -> u32 {
        match self.predicted.get(target) {
            Some(predicted) => predicted.revision,
            None => root
                .control_state(target.node)
                .map_or(0, |state| state.revision),
        }
    }

    /// Route one text append on the focused text input.
    fn route_text(
        &mut self,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        payload: &str,
        fence: Option<&GuiTextFence>,
    ) -> Result<(), ErrorReason> {
        if payload.is_empty() {
            return Err(ErrorReason::InvalidValue);
        }
        let Some(focus) = self.text_command_focus(session, tick, input, fence) else {
            return Ok(());
        };
        let Some((kind, root)) = self.revalidate_capture(sim, &focus.target) else {
            self.clear_focus();
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return Ok(());
        };
        if kind != ControlKind::TextInput {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return Ok(());
        }
        let revision = self.base_revision(&root, &focus.target);
        if !self.text_revision_current(session, tick, focus.target, fence, revision, false) {
            return Ok(());
        }
        self.append_text(
            session,
            tick,
            input,
            &root,
            &focus.target,
            payload.to_owned(),
        );
        Ok(())
    }

    /// Route an explicit caret/selection move on the focused text input.
    /// Offsets are fenced to the current predicted/committed text: past-end
    /// or split-grapheme ranges conflict without moving the cursor.
    #[allow(clippy::too_many_arguments)]
    fn route_selection(
        &mut self,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        start: u32,
        end: u32,
        fence: Option<&GuiTextFence>,
    ) {
        let Some(focus) = self.text_command_focus(session, tick, input, fence) else {
            return;
        };
        let Some((kind, root)) = self.revalidate_capture(sim, &focus.target) else {
            self.clear_focus();
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return;
        };
        if kind != ControlKind::TextInput {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        }
        let Some((text, revision)) = self.base_text(&root, &focus.target) else {
            self.conflict(
                session,
                tick,
                Some(focus.target),
                GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
            );
            return;
        };
        // Delayed native ranges are fenced to the revision their offsets
        // were chosen against: an external reset or equal-length replace
        // that moved the revision conflicts instead of rebasing the range
        // onto newer text. A fenced range names that revision itself;
        // unfenced ranges fall back to the stored cursor's revision. Fresh
        // gestures re-resolve through cursor_for.
        if fence.is_some() {
            if !self.text_revision_current(session, tick, focus.target, fence, revision, true) {
                return;
            }
        } else if let Some(stored) = self.text_carets.get(&focus.target)
            && stored.revision != revision
        {
            self.conflict(
                session,
                tick,
                Some(focus.target),
                GuiInputConflictReason::RevisionMismatch {
                    expected: stored.revision,
                    found: revision,
                },
            );
            return;
        }
        let len = text.len() as u32;
        if start > len
            || end > len
            || !super::text_edit::is_boundary(&text, start)
            || !super::text_edit::is_boundary(&text, end)
        {
            self.conflict(
                session,
                tick,
                Some(focus.target),
                GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
            );
            return;
        }
        // Explicit selection moves cancel provisional composition.
        self.cancel_composition_for_target(&focus.target);
        self.set_selection_cursor(&root, &focus.target, session, start, end);
    }

    /// Route a provisional IME update for the focused text input.
    /// The provisional never touches committed state or retained views.
    fn route_composition_update(
        &mut self,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        update: CompositionUpdate<'_>,
    ) -> Result<(), ErrorReason> {
        let CompositionUpdate {
            text,
            caret_start,
            caret_end,
            fence,
        } = update;
        validate_composition_shape(text, caret_start, caret_end)?;
        let Some(focus) = self.text_command_focus(session, tick, input, fence) else {
            return Ok(());
        };
        let Some((kind, root)) = self.revalidate_capture(sim, &focus.target) else {
            self.clear_focus();
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return Ok(());
        };
        if kind != ControlKind::TextInput {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return Ok(());
        }
        let Some((_, revision)) = self.base_text(&root, &focus.target) else {
            self.conflict(
                session,
                tick,
                Some(focus.target),
                GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
            );
            return Ok(());
        };
        if !self.text_revision_current(session, tick, focus.target, fence, revision, false) {
            return Ok(());
        }

        // Ensure a cursor exists so later commits have an insertion point.
        let _ = self.cursor_for(&root, &focus.target, session);
        let Some(composed) = super::composition::ActiveComposition::new(
            focus.target,
            session,
            revision,
            text.to_owned(),
            caret_start,
            caret_end,
        ) else {
            return Err(ErrorReason::InvalidValue);
        };
        self.composition = Some(composed);
        self.touch_caret();
        Ok(())
    }

    /// Route an explicit commit of the active provisional at the committed
    /// caret/selection through the ordinary revision-gated envelope path.
    fn route_composition_commit(
        &mut self,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        fence: Option<&GuiTextFence>,
    ) {
        let Some(focus) = self.text_command_focus(session, tick, input, fence) else {
            return;
        };
        let Some((kind, root)) = self.revalidate_capture(sim, &focus.target) else {
            self.clear_focus();
            self.clear_composition();
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return;
        };
        if kind != ControlKind::TextInput {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        }
        let Some(composed) = self.composition.clone() else {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        };
        if !composed.is_for(&focus.target, session) {
            self.clear_composition();
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        }
        let Some((text, revision, cursor)) = self.cursor_for(&root, &focus.target, session) else {
            self.composition = None;
            self.conflict(
                session,
                tick,
                Some(focus.target),
                GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
            );
            return;
        };
        if !self.text_revision_current(session, tick, focus.target, fence, revision, false) {
            return;
        }
        if composed.revision != revision {
            let found = revision;
            self.clear_composition();
            self.conflict(
                session,
                tick,
                Some(focus.target),
                GuiInputConflictReason::RevisionMismatch {
                    expected: composed.revision,
                    found,
                },
            );
            return;
        }
        if composed.provisional.is_empty() {
            self.composition = None;
            return;
        }
        let (start, end) = match cursor.selection() {
            Some((start, end)) => (start, end),
            None => (cursor.caret, cursor.caret),
        };
        let outcome = super::text_edit::insert_at(&text, start, end, &composed.provisional);
        if outcome.text.len() > super::super::tree::nodes::MAX_TEXT_BYTES {
            self.conflict(
                session,
                tick,
                Some(focus.target),
                GuiInputConflictReason::AdmissionFailed(ErrorReason::Capacity),
            );
            return;
        }
        self.clear_composition();
        self.push_value_envelope(
            session,
            tick,
            focus.target,
            None,
            revision,
            GuiControlValue::Text(outcome.text.clone()),
        );
        self.store_committed_cursor(&focus.target, outcome.caret, revision, session);
    }

    /// Route an explicit cancel of the active provisional without committing.
    fn route_composition_cancel(
        &mut self,
        sim: &WorldSimulationState,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        fence: Option<&GuiTextFence>,
    ) {
        let Some(focus) = self.text_command_focus(session, tick, input, fence) else {
            return;
        };
        let Some((kind, _)) = self.revalidate_capture(sim, &focus.target) else {
            self.clear_focus();
            self.clear_composition();
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return;
        };
        if kind != ControlKind::TextInput {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        }
        let Some(composed) = self.composition.clone() else {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        };
        if !composed.is_for(&focus.target, session) {
            self.clear_composition();
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return;
        }
        self.clear_composition();
    }

    /// Route a programmatic focus request against a fenced handle.
    /// Disabled controls refuse focus without acquiring the context.
    fn route_focus(
        &mut self,
        sim: &WorldSimulationState,
        layout: &GuiLayoutSystem,
        session: u64,
        tick: u64,
        input: &GuiInputCommand,
        handle: &GuiNodeHandle,
    ) -> Result<(), ErrorReason> {
        if handle.session != session {
            return Err(ErrorReason::InvalidValue);
        }
        let Some((incarnation, root)) = producer_root(sim, handle.entity) else {
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return Ok(());
        };
        if incarnation != handle.root_incarnation {
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return Ok(());
        }
        let Some(node) = root.nodes().node(handle.node_id) else {
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return Ok(());
        };
        if ControlKind::of(&node.data).is_none() {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return Ok(());
        }
        let opacity = root
            .style_row(handle.node_id)
            .map(|style| style.opacity)
            .unwrap_or(1.0);
        if opacity <= 0.0 {
            self.unhandled(session, tick, input, GuiUnhandledReason::StaleTarget);
            return Ok(());
        }
        if !Self::target_eligible(
            layout,
            sim,
            &GuiInputTarget {
                entity: handle.entity,
                node: handle.node_id,
                root_incarnation: incarnation,
            },
        ) {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotFocusable);
            return Ok(());
        }
        // A validated programmatic focus explicitly acquires the unowned
        // context or replaces its owner; the previous owner's in-flight
        // intents cancel as session-replaced.
        self.replace_owner(session, tick);
        self.set_focus(
            session,
            tick,
            Some(GuiInputTarget {
                entity: handle.entity,
                node: handle.node_id,
                root_incarnation: incarnation,
            }),
            true,
        );
        Ok(())
    }

    /// Drop one pointer's capture, press ownership and hover.
    fn drop_capture(&mut self, pointer: u32) {
        if let Some(capture) = self.captures.remove(&pointer)
            && self.press_owners.get(&capture.target) == Some(&pointer)
        {
            self.press_owners.remove(&capture.target);
        }
        self.hovers.remove(&pointer);
    }
}

// ---------------------------------------------------------------------------
// Envelope application at the next mutation boundary, before animation.
// ---------------------------------------------------------------------------

/// Stage a routed producer root and commit it through the ordinary commit
/// path (validation, invalidation rounds, lifecycle dispatch) without
/// re-entering operation hooks: the envelope already passed the shared
/// revision gate, and the [`GuiSystem`](super::GuiSystem) ownership guards
/// observe authored operations only.
///
/// The staged value is a refreshed evaluation, not the raw producer
/// admission: still-active sparse overlay contributions observed before the
/// commit are re-applied to the new producer base (mirroring
/// `StateOverlayMutationAccess::resolve_layers`, which this path cannot
/// re-enter), and the layer's hidden producer originals are refreshed so
/// later producer reads restore exact authored values. When producer and
/// overlay disagree the overlay still wins.
pub(super) fn stage_producer_root(
    access: &mut SystemRuntimeAccess<'_>,
    entity: EntityId,
    root: GuiRoot,
) -> Result<(), ErrorReason> {
    let sim = &mut *access.world;
    let incarnation = sim
        .state
        .entities
        .get(&entity)
        .and_then(|record| record.input(ComponentValue::GUI_ROOT))
        .map(|input| input.incarnation)
        .ok_or(ErrorReason::MissingComponent)?;
    let key = (entity, ComponentValue::GUI_ROOT);
    let layer = sim
        .state
        .entities
        .get(&entity)
        .and_then(|record| record.layers.get(&ComponentValue::GUI_ROOT))
        .ok_or(ErrorReason::MissingComponent)?;
    // Without overlays, hidden originals or staged values, the producer and
    // effective roots are both live storage: no contribution survives to
    // re-apply and the new producer is the new effective value.
    let unlayered = layer.inputs.overlay_handles.is_empty()
        && layer.inputs.hidden_fields.is_empty()
        && layer.inputs.base_value().is_none()
        && !layer.inputs.is_staged()
        && !sim.state.prepared.contains_key(&key)
        && !sim.state.dirty.contains(&key)
        && sim.state.evaluated_target != Some(key);
    let next_producer = ComponentValue::GuiRoot(root);
    let (effective, hidden) = if unlayered {
        (next_producer.clone(), Vec::new())
    } else {
        let previous = super::super::system::producer_root(&sim.state, &sim.components, entity)
            .ok_or(ErrorReason::MissingComponent)?
            .into_owned();
        let previous_effective = sim
            .state
            .input_value(&sim.components, entity, ComponentValue::GUI_ROOT)
            .ok_or(ErrorReason::MissingComponent)?;
        resolve_control_effective(
            &ComponentValue::GuiRoot(previous),
            &previous_effective,
            next_producer.clone(),
        )?
    };
    if let Some(layer) = sim
        .state
        .entities
        .get_mut(&entity)
        .and_then(|record| record.layers.get_mut(&ComponentValue::GUI_ROOT))
    {
        layer.inputs.hidden_fields = hidden;
        // Without contributions the staged producer is the effective input.
        layer.inputs.staged = if unlayered {
            crate::world::systems::state_overlay::ComponentStagedInput::Base
        } else {
            crate::world::systems::state_overlay::ComponentStagedInput::Layered(Box::new(
                effective.clone(),
            ))
        };
        layer.inputs.base_value = Some(Box::new(next_producer));
    }
    sim.state.changed.insert(key, Some(incarnation));
    sim.state.prepared.insert(key, effective);
    commit_components(
        sim,
        &mut access.instances,
        None,
        &mut *access.asset_acquisition,
        false,
    )
}

impl GuiInputSystem {
    /// Every fully fenced target retained by routing or deferred application.
    fn retained_targets(&self) -> BTreeSet<GuiInputTarget> {
        let mut targets = BTreeSet::new();
        targets.extend(self.focus.map(|focus| focus.target));
        targets.extend(self.captures.values().map(|capture| capture.target));
        targets.extend(self.hovers.values().map(|cursor| cursor.target));
        targets.extend(self.press_owners.keys().copied());
        targets.extend(self.scroll_drags.values().flat_map(|drag| {
            drag.chain.iter().map(|&node| GuiInputTarget {
                entity: drag.entity,
                root_incarnation: drag.root_incarnation,
                node,
            })
        }));
        targets.extend(self.scroll_bar_presses.values().map(|press| press.target));
        targets.extend(self.scroll_bar_hovers.values().map(|hover| hover.target));
        targets.extend(self.envelopes.iter().filter_map(|envelope| envelope.target));
        targets.extend(self.predicted.keys().copied());
        targets.extend(self.scroll_offsets.keys().copied());
        targets.extend(self.text_carets.keys().copied());
        targets.extend(self.composition.as_ref().map(|value| value.target));
        targets
    }

    /// Admit one semantic operation as one owner-, identity-, eligibility-
    /// and revision-fenced envelope. Every fallible check precedes owner or
    /// cursor mutation, so rejection cannot leave a partial focus/action.
    fn admit_semantic_action(
        &mut self,
        context: &SystemCommandContext<'_>,
        session: u64,
        command: &crate::GuiSemanticActionCommand,
    ) -> Result<(), ErrorReason> {
        if self.envelopes.len() >= MAX_PENDING_ENVELOPES {
            return Err(ErrorReason::Capacity);
        }
        if self.owner.is_some_and(|owner| owner.session != session) {
            return Err(ErrorReason::InvalidValue);
        }
        let layout = self
            .layout
            .and_then(|binding| context.world.dependency(binding))
            .ok_or(ErrorReason::InvalidValue)?;
        let GuiTargetStatus::Eligible(root) =
            evaluated_status(context.world.world, layout, &command.target)
        else {
            return Err(ErrorReason::InvalidValue);
        };
        let live = root
            .nodes()
            .node(command.target.node)
            .ok_or(ErrorReason::InvalidValue)?;
        let kind = ControlKind::of(&live.data).ok_or(ErrorReason::InvalidValue)?;
        let found_revision = root
            .control_state(command.target.node)
            .map(|state| state.revision)
            .unwrap_or(0);
        if found_revision != command.expected_revision {
            return Err(ErrorReason::InvalidValue);
        }

        let envelope_kind = match &command.action {
            crate::GuiSemanticAction::Press if kind == ControlKind::Button => {
                EnvelopeKind::PressButton
            }
            crate::GuiSemanticAction::Toggle if kind == ControlKind::Checkbox => {
                let Some(state) = root.control_state(command.target.node) else {
                    return Err(ErrorReason::InvalidValue);
                };
                let GuiControlValue::Bool(value) = state.value else {
                    return Err(ErrorReason::InvalidValue);
                };
                EnvelopeKind::SetValue {
                    expected_revision: command.expected_revision,
                    value: GuiControlValue::Bool(!value),
                    source: GuiCommitSource::Semantic,
                }
            }
            crate::GuiSemanticAction::SetScalar(value) if kind == ControlKind::Slider => {
                let value = GuiControlValue::Scalar(*value);
                let handle = GuiNodeHandle::new(
                    session,
                    command.target.entity,
                    command.target.root_incarnation,
                    command.target.node,
                );
                check_control_value(
                    &root,
                    command.target.root_incarnation,
                    session,
                    &handle,
                    command.expected_revision,
                    &value,
                )?;
                EnvelopeKind::SetValue {
                    expected_revision: command.expected_revision,
                    value,
                    source: GuiCommitSource::Semantic,
                }
            }
            crate::GuiSemanticAction::SetText(value) if kind == ControlKind::TextInput => {
                let value = GuiControlValue::Text(value.clone());
                let handle = GuiNodeHandle::new(
                    session,
                    command.target.entity,
                    command.target.root_incarnation,
                    command.target.node,
                );
                check_control_value(
                    &root,
                    command.target.root_incarnation,
                    session,
                    &handle,
                    command.expected_revision,
                    &value,
                )?;
                EnvelopeKind::SetValue {
                    expected_revision: command.expected_revision,
                    value,
                    source: GuiCommitSource::Semantic,
                }
            }
            crate::GuiSemanticAction::Focus => EnvelopeKind::Focus {
                focus: Some(command.target),
                keyboard: true,
            },
            _ => return Err(ErrorReason::InvalidValue),
        };

        if self.owner.is_none() {
            let epoch = self.next_epoch();
            self.owner = Some(GuiInputOwner {
                session,
                epoch,
            });
        }
        let tick = context.world.world.tick.saturating_add(1);
        if let EnvelopeKind::SetValue {
            expected_revision,
            ref value,
            ..
        } = envelope_kind
        {
            self.predicted.insert(
                command.target,
                PredictedControl {
                    value: value.clone(),
                    revision: expected_revision.saturating_add(1),
                },
            );
        }
        self.envelopes.push(PendingEnvelope {
            session,
            epoch: self.owner_epoch,
            source_tick: tick,
            target: Some(command.target),
            pointer: None,
            kind: envelope_kind,
        });
        Ok(())
    }

    /// Invalidate every retained cursor and deferred action for one exact
    /// identity. Publications preserve the session/source provenance that
    /// established each cursor; replacement identities remain untouched.
    fn invalidate_target(
        &mut self,
        target: GuiInputTarget,
        tick: u64,
        reason: GuiInputCancelReason,
    ) {
        let mut cancelled = BTreeSet::new();
        let mut kept = Vec::with_capacity(self.envelopes.len());
        for envelope in std::mem::take(&mut self.envelopes) {
            if envelope.target == Some(target) {
                cancelled.insert((envelope.session, target));
                self.pending_cancellations.push(GuiInputCancellation {
                    session: envelope.session,
                    source_tick: envelope.source_tick,
                    effect_tick: tick,
                    target: Some(target),
                    reason,
                });
            } else {
                kept.push(envelope);
            }
        }
        self.envelopes = kept;
        self.predicted.remove(&target);
        self.scroll_predicted.remove(&target);

        if let Some(focus) = self.focus.filter(|focus| focus.target == target) {
            self.focus = None;
            self.focus_visible = false;
            self.focus_generation = self.focus_generation.saturating_add(1).max(1);
            self.pending_effects.push(GuiInputEffect {
                session: focus.session,
                source_tick: self.focus_tick,
                effect_tick: tick,
                kind: GuiInputEffectKind::FocusChanged {
                    focus: None,
                },
            });
            self.touch_caret();
        }

        let hovered: Vec<_> = self
            .hovers
            .iter()
            .filter(|(_, cursor)| cursor.target == target)
            .map(|(&pointer, &cursor)| (pointer, cursor))
            .collect();
        for (pointer, cursor) in hovered {
            self.hovers.remove(&pointer);
            self.pending_effects.push(GuiInputEffect {
                session: cursor.session,
                source_tick: cursor.source_tick,
                effect_tick: tick,
                kind: GuiInputEffectKind::HoverChanged {
                    pointer,
                    target: None,
                    position: cursor.position,
                },
            });
        }

        let captured: Vec<_> = self
            .captures
            .iter()
            .filter(|(_, capture)| capture.target == target)
            .map(|(&pointer, &capture)| (pointer, capture))
            .collect();
        for (pointer, capture) in captured {
            self.drop_capture(pointer);
            if !cancelled.contains(&(capture.session, target)) {
                self.pending_cancellations.push(GuiInputCancellation {
                    session: capture.session,
                    source_tick: capture.source_tick,
                    effect_tick: tick,
                    target: Some(target),
                    reason,
                });
            }
        }
        self.press_owners.remove(&target);
        self.scroll_drags.retain(|_, drag| !drag.scrolls(&target));
        self.scroll_bar_presses
            .retain(|_, press| press.target != target && !press.chain.contains(&target.node));
        self.scroll_bar_hovers
            .retain(|_, hover| hover.target != target);

        let dropped_caret = self.text_carets.remove(&target).is_some();
        let dropped_composition = self
            .composition
            .as_ref()
            .is_some_and(|value| value.target == target);
        if dropped_composition {
            self.composition = None;
        }
        if dropped_caret || dropped_composition {
            self.touch_caret();
        }

        if let Some(scroll) = self.scroll_offsets.remove(&target) {
            self.scroll_revision = self.scroll_revision.saturating_add(1).max(1);
            self.pending_effects.push(GuiInputEffect {
                session: scroll.session,
                source_tick: scroll.source_tick,
                effect_tick: tick,
                kind: GuiInputEffectKind::ScrollChanged {
                    entity: target.entity,
                    node: target.node,
                    offset: [0.0, 0.0],
                },
            });
        }
    }

    /// Fail closed against the current evaluated view even on an idle input
    /// tick. Cost is proportional to active transient/deferred targets.
    fn revalidate_retained_targets(
        &mut self,
        sim: &WorldSimulationState,
        layout: &GuiLayoutSystem,
        tick: u64,
    ) {
        let targets = self.retained_targets();
        let mut roots: BTreeMap<EntityId, Option<(u64, Cow<'_, GuiRoot>)>> = BTreeMap::new();
        let mut invalid = Vec::new();
        for target in targets {
            let root = roots
                .entry(target.entity)
                .or_insert_with(|| producer_root(sim, target.entity));
            let validity = match root.as_ref() {
                Some((incarnation, root)) => {
                    evaluated_validity(root, *incarnation, layout, &target)
                }
                None => GuiTargetValidity::Removed,
            };
            match validity {
                GuiTargetValidity::Eligible => {}
                GuiTargetValidity::Removed => {
                    invalid.push((target, GuiInputCancelReason::TargetRemoved));
                }
                GuiTargetValidity::Ineligible => {
                    invalid.push((target, GuiInputCancelReason::TargetHidden));
                }
            }
        }
        for (target, reason) in invalid {
            self.invalidate_target(target, tick, reason);
        }
    }

    /// Apply every queued envelope in order with liveness revalidation.
    fn apply_envelopes(&mut self, access: &mut SystemRuntimeAccess<'_>, tick: u64) {
        let envelopes = std::mem::take(&mut self.envelopes);
        // Every queued scroll resolves below, so routed offsets resync from
        // the committed ones on the next routing pass.
        self.scroll_predicted.clear();
        if envelopes.is_empty() {
            return;
        }
        // Predicted entries survive only while an envelope still needs them;
        // every resolution below drops the entry with its last envelope.
        let mut remaining: BTreeMap<GuiInputTarget, usize> = BTreeMap::new();
        for envelope in &envelopes {
            if let EnvelopeKind::SetValue {
                ..
            } = envelope.kind
                && let Some(target) = envelope.target
            {
                *remaining.entry(target).or_default() += 1;
            }
        }
        for envelope in envelopes {
            self.apply_envelope(access, tick, envelope, &mut remaining);
        }
        self.persist_virtual_anchors(access);
    }

    /// Clear cursors still pointing at one fully fenced target.
    fn purge_cursors_for_target(&mut self, target: &GuiInputTarget) {
        if self.focus.is_some_and(|focus| focus.target == *target) {
            self.focus = None;
            self.focus_visible = false;
            self.focus_generation = self.focus_generation.saturating_add(1).max(1);
            self.touch_caret();
        }
        if self.text_carets.remove(target).is_some() {
            self.touch_caret();
        }
        self.cancel_composition_for_target(target);
        let pointers: Vec<u32> = self
            .captures
            .iter()
            .filter(|(_, capture)| capture.target == *target)
            .map(|(pointer, _)| *pointer)
            .collect();
        for pointer in pointers {
            self.drop_capture(pointer);
        }
        self.press_owners.remove(target);
        self.hovers.retain(|_, cursor| cursor.target != *target);
    }

    /// Drop one node's prediction with its last envelope.
    fn release_prediction(
        &mut self,
        target: &GuiInputTarget,
        remaining: &mut BTreeMap<GuiInputTarget, usize>,
    ) {
        if let Some(count) = remaining.get_mut(target) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                remaining.remove(target);
                self.predicted.remove(target);
            }
        } else {
            self.predicted.remove(target);
        }
    }

    /// Drop one node's prediction after its envelope conflicted. A focused
    /// text input republishes its authoritative text, so the native buffer
    /// resynchronizes from the rolled-back revision instead of the
    /// prediction it last observed.
    fn reject_prediction(
        &mut self,
        target: &GuiInputTarget,
        remaining: &mut BTreeMap<GuiInputTarget, usize>,
    ) {
        self.release_prediction(target, remaining);
        if self.focus.is_some_and(|focus| focus.target == *target) {
            self.touch_caret();
        }
    }

    /// Cancel one envelope whose target died or hid before application.
    fn cancel_envelope(
        &mut self,
        tick: u64,
        envelope: &PendingEnvelope,
        reason: GuiInputCancelReason,
        remaining: &mut BTreeMap<GuiInputTarget, usize>,
    ) {
        if let Some(target) = envelope.target {
            self.purge_cursors_for_target(&target);
            self.release_prediction(&target, remaining);
        }
        self.pending_cancellations.push(GuiInputCancellation {
            session: envelope.session,
            source_tick: envelope.source_tick,
            effect_tick: tick,
            target: envelope.target,
            reason,
        });
    }

    /// Apply one envelope with liveness and ownership revalidation.
    /// Envelopes whose session or epoch no longer holds the single active
    /// input context (replacement, disconnect, delayed input) cancel as
    /// session-replaced instead of mutating another owner's state.
    fn apply_envelope(
        &mut self,
        access: &mut SystemRuntimeAccess<'_>,
        tick: u64,
        envelope: PendingEnvelope,
        remaining: &mut BTreeMap<GuiInputTarget, usize>,
    ) {
        let owned = self.owner.is_some_and(|owner| {
            owner.session == envelope.session && owner.epoch == envelope.epoch
        });
        if !owned {
            // Never purge cursors here: the target may already belong to the
            // new owner. Drop the prediction and report the replacement.
            if let Some(target) = envelope.target {
                self.release_prediction(&target, remaining);
            }
            self.pending_cancellations.push(GuiInputCancellation {
                session: envelope.session,
                source_tick: envelope.source_tick,
                effect_tick: tick,
                target: envelope.target,
                reason: GuiInputCancelReason::SessionReplaced,
            });
            return;
        }
        match &envelope.kind {
            EnvelopeKind::Focus {
                focus,
                keyboard,
            } => {
                let focus = *focus;
                let keyboard = *keyboard;
                if let Some(target) = focus {
                    let status = self
                        .layout(&*access)
                        .map_or(GuiTargetStatus::Ineligible, |layout| {
                            evaluated_status(access.world, layout, &target)
                        });
                    match status {
                        GuiTargetStatus::Eligible(root)
                            if root
                                .nodes()
                                .node(target.node)
                                .and_then(|node| ControlKind::of(&node.data))
                                .is_some() => {}
                        GuiTargetStatus::Eligible(_) => {
                            self.pending_conflicts.push(GuiInputConflict {
                                session: envelope.session,
                                source_tick: envelope.source_tick,
                                effect_tick: tick,
                                target: Some(target),
                                reason: GuiInputConflictReason::AdmissionFailed(
                                    ErrorReason::InvalidValue,
                                ),
                            });
                            return;
                        }
                        GuiTargetStatus::Removed => {
                            self.cancel_envelope(
                                tick,
                                &envelope,
                                GuiInputCancelReason::TargetRemoved,
                                remaining,
                            );
                            return;
                        }
                        GuiTargetStatus::Ineligible => {
                            self.cancel_envelope(
                                tick,
                                &envelope,
                                GuiInputCancelReason::TargetHidden,
                                remaining,
                            );
                            return;
                        }
                    }
                }
                self.update_focus_cursor(envelope.session, envelope.source_tick, focus, keyboard);
                self.pending_effects.push(GuiInputEffect {
                    session: envelope.session,
                    source_tick: envelope.source_tick,
                    effect_tick: tick,
                    kind: GuiInputEffectKind::FocusChanged {
                        focus: focus.map(|target| GuiInputFocus {
                            target,
                            session: envelope.session,
                        }),
                    },
                });
            }
            EnvelopeKind::Scroll {
                delta,
            } => {
                let delta = *delta;
                let Some(target) = envelope.target else {
                    return;
                };
                let status = self
                    .layout(&*access)
                    .map_or(GuiTargetStatus::Ineligible, |layout| {
                        evaluated_status(access.world, layout, &target)
                    });
                match status {
                    GuiTargetStatus::Eligible(_) => {}
                    GuiTargetStatus::Removed => {
                        self.cancel_envelope(
                            tick,
                            &envelope,
                            GuiInputCancelReason::TargetRemoved,
                            remaining,
                        );
                        return;
                    }
                    GuiTargetStatus::Ineligible => {
                        self.cancel_envelope(
                            tick,
                            &envelope,
                            GuiInputCancelReason::TargetHidden,
                            remaining,
                        );
                        return;
                    }
                }
                // Targets clamp authoritatively against current extents, so
                // same-tick chains converge on the bound instead of
                // overshooting it; routing only queues ScrollView targets.
                let current = self
                    .scroll_offsets
                    .get(&target)
                    .map(|cursor| cursor.offset)
                    .unwrap_or([0.0, 0.0]);
                let max = self
                    .layout(&*access)
                    .ok()
                    .and_then(|layout| layout.view(target.entity))
                    .map(|view| Self::scroll_max(view, target.node))
                    .unwrap_or([f32::INFINITY, f32::INFINITY]);
                let offset = [
                    (current[0] + delta[0]).clamp(0.0, max[0]),
                    (current[1] + delta[1]).clamp(0.0, max[1]),
                ];
                let moved = offset != current;
                self.scroll_offsets.insert(
                    target,
                    ScrollCursor {
                        offset,
                        session: envelope.session,
                        source_tick: envelope.source_tick,
                    },
                );
                // Clamp-to-edge no-ops leave the stored offset untouched and
                // never bump; dropped outer-edge remainder never reaches an
                // envelope, so this is the only routed bump site; a
                // VirtualList's anchor sync bumps after layout, and its
                // moved offset persists as an anchor below.
                if moved {
                    self.scroll_revision = self.scroll_revision.saturating_add(1);
                    self.virtual_scroll.offset_moved(target);
                }
                self.pending_effects.push(GuiInputEffect {
                    session: envelope.session,
                    source_tick: envelope.source_tick,
                    effect_tick: tick,
                    kind: GuiInputEffectKind::ScrollChanged {
                        entity: target.entity,
                        node: target.node,
                        offset,
                    },
                });
            }
            EnvelopeKind::PressButton => {
                let Some(target) = envelope.target else {
                    return;
                };
                let status = self
                    .layout(&*access)
                    .map_or(GuiTargetStatus::Ineligible, |layout| {
                        evaluated_status(access.world, layout, &target)
                    });
                let root = match status {
                    GuiTargetStatus::Eligible(root) => root,
                    GuiTargetStatus::Removed => {
                        self.cancel_envelope(
                            tick,
                            &envelope,
                            GuiInputCancelReason::TargetRemoved,
                            remaining,
                        );
                        return;
                    }
                    GuiTargetStatus::Ineligible => {
                        self.cancel_envelope(
                            tick,
                            &envelope,
                            GuiInputCancelReason::TargetHidden,
                            remaining,
                        );
                        return;
                    }
                };
                let still_button = root
                    .nodes()
                    .node(target.node)
                    .is_some_and(|node| ControlKind::of(&node.data) == Some(ControlKind::Button));
                if !still_button {
                    self.pending_conflicts.push(GuiInputConflict {
                        session: envelope.session,
                        source_tick: envelope.source_tick,
                        effect_tick: tick,
                        target: Some(target),
                        reason: GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
                    });
                    return;
                }
                self.pending_effects.push(GuiInputEffect {
                    session: envelope.session,
                    source_tick: envelope.source_tick,
                    effect_tick: tick,
                    kind: GuiInputEffectKind::ButtonPressed {
                        entity: target.entity,
                        root_incarnation: target.root_incarnation,
                        node: target.node,
                        path: ancestor_path(&root, target.node),
                    },
                });
            }
            EnvelopeKind::Submit {
                expected_revision,
            } => {
                let expected_revision = *expected_revision;
                let Some(target) = envelope.target else {
                    return;
                };
                let status = self
                    .layout(&*access)
                    .map_or(GuiTargetStatus::Ineligible, |layout| {
                        evaluated_status(access.world, layout, &target)
                    });
                let root = match status {
                    GuiTargetStatus::Eligible(root) => root,
                    GuiTargetStatus::Removed => {
                        self.cancel_envelope(
                            tick,
                            &envelope,
                            GuiInputCancelReason::TargetRemoved,
                            remaining,
                        );
                        return;
                    }
                    GuiTargetStatus::Ineligible => {
                        self.cancel_envelope(
                            tick,
                            &envelope,
                            GuiInputCancelReason::TargetHidden,
                            remaining,
                        );
                        return;
                    }
                };

                // The submitted text is the committed text at the revision
                // routing predicted: a text input that changed kind or text
                // since then conflicts instead of submitting other text.
                let committed = root
                    .nodes()
                    .node(target.node)
                    .filter(|node| ControlKind::of(&node.data) == Some(ControlKind::TextInput))
                    .and_then(|_| root.control_state(target.node));
                let Some(state) = committed else {
                    self.pending_conflicts.push(GuiInputConflict {
                        session: envelope.session,
                        source_tick: envelope.source_tick,
                        effect_tick: tick,
                        target: Some(target),
                        reason: GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
                    });
                    return;
                };
                let GuiControlValue::Text(text) = &state.value else {
                    self.pending_conflicts.push(GuiInputConflict {
                        session: envelope.session,
                        source_tick: envelope.source_tick,
                        effect_tick: tick,
                        target: Some(target),
                        reason: GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
                    });
                    return;
                };
                if state.revision != expected_revision {
                    self.pending_conflicts.push(GuiInputConflict {
                        session: envelope.session,
                        source_tick: envelope.source_tick,
                        effect_tick: tick,
                        target: Some(target),
                        reason: GuiInputConflictReason::RevisionMismatch {
                            expected: expected_revision,
                            found: state.revision,
                        },
                    });
                    return;
                }
                self.pending_effects.push(GuiInputEffect {
                    session: envelope.session,
                    source_tick: envelope.source_tick,
                    effect_tick: tick,
                    kind: GuiInputEffectKind::Submitted {
                        entity: target.entity,
                        root_incarnation: target.root_incarnation,
                        node: target.node,
                        revision: state.revision,
                        text: text.clone(),
                        path: ancestor_path(&root, target.node),
                    },
                });
            }
            EnvelopeKind::SetValue {
                expected_revision,
                value,
                source,
            } => {
                let expected_revision = *expected_revision;
                let source = *source;
                let Some(target) = envelope.target else {
                    return;
                };
                let status = self
                    .layout(&*access)
                    .map_or(GuiTargetStatus::Ineligible, |layout| {
                        evaluated_status(access.world, layout, &target)
                    });
                let root = match status {
                    GuiTargetStatus::Eligible(root) => root,
                    GuiTargetStatus::Removed => {
                        self.cancel_envelope(
                            tick,
                            &envelope,
                            GuiInputCancelReason::TargetRemoved,
                            remaining,
                        );
                        return;
                    }
                    GuiTargetStatus::Ineligible => {
                        self.cancel_envelope(
                            tick,
                            &envelope,
                            GuiInputCancelReason::TargetHidden,
                            remaining,
                        );
                        return;
                    }
                };
                let committed_revision =
                    root.control_state(target.node).map(|state| state.revision);
                let Some(found) = committed_revision else {
                    self.reject_prediction(&target, remaining);
                    self.pending_conflicts.push(GuiInputConflict {
                        session: envelope.session,
                        source_tick: envelope.source_tick,
                        effect_tick: tick,
                        target: Some(target),
                        reason: GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
                    });
                    return;
                };
                if found != expected_revision {
                    self.reject_prediction(&target, remaining);
                    self.cancel_composition_for_target(&target);
                    self.pending_conflicts.push(GuiInputConflict {
                        session: envelope.session,
                        source_tick: envelope.source_tick,
                        effect_tick: tick,
                        target: Some(target),
                        reason: GuiInputConflictReason::RevisionMismatch {
                            expected: expected_revision,
                            found,
                        },
                    });
                    return;
                }
                let handle = GuiNodeHandle::new(
                    envelope.session,
                    target.entity,
                    target.root_incarnation,
                    target.node,
                );
                // Staging writes the complete producer root, so commit on a
                // copy; the World still holds the pre-commit producer that
                // overlay refresh derives from.
                let mut root = root.into_owned();
                let commit = commit_control_value(
                    &mut root,
                    target.root_incarnation,
                    envelope.session,
                    &handle,
                    expected_revision,
                    value,
                );
                if let Err(reason) = commit {
                    self.reject_prediction(&target, remaining);
                    self.pending_conflicts.push(GuiInputConflict {
                        session: envelope.session,
                        source_tick: envelope.source_tick,
                        effect_tick: tick,
                        target: Some(target),
                        reason: GuiInputConflictReason::AdmissionFailed(reason),
                    });
                    return;
                }
                // Pin the committed ancestry before the staged root moves
                // into storage: value commits never reparent nodes.
                let path = ancestor_path(&root, target.node);
                match stage_producer_root(access, target.entity, root) {
                    Ok(()) => {
                        self.release_prediction(&target, remaining);
                        // Read back the committed revision from storage.
                        let revision = producer_root(access.world, target.entity)
                            .and_then(|(_, root)| {
                                root.control_state(target.node).map(|state| state.revision)
                            })
                            .unwrap_or(expected_revision.saturating_add(1));
                        self.pending_effects.push(GuiInputEffect {
                            session: envelope.session,
                            source_tick: envelope.source_tick,
                            effect_tick: tick,
                            kind: GuiInputEffectKind::ControlCommitted {
                                entity: target.entity,
                                root_incarnation: target.root_incarnation,
                                node: target.node,
                                value: value.clone(),
                                revision,
                                path,
                                source,
                            },
                        });

                        // A semantic replacement of the focused text is not
                        // part of the native buffer's own edit chain: fence
                        // its stamped edits and resynchronize it.
                        if source != GuiCommitSource::User
                            && self.focus.is_some_and(|focus| focus.target == target)
                        {
                            self.focus_generation = self.focus_generation.saturating_add(1).max(1);
                            self.touch_caret();
                        }
                    }
                    Err(reason) => {
                        self.reject_prediction(&target, remaining);
                        self.pending_conflicts.push(GuiInputConflict {
                            session: envelope.session,
                            source_tick: envelope.source_tick,
                            effect_tick: tick,
                            target: Some(target),
                            reason: GuiInputConflictReason::AdmissionFailed(reason),
                        });
                    }
                }
            }
        }
    }
}

/// Runtime logical ancestor path for one live target, root-first including
/// the target. Pinned from the authoritative tree at application so listener
/// dispatch observes the committed ancestry even if later edits move the
/// tree. Validated trees are acyclic; the length cap fails safe otherwise.
pub(in crate::world::systems::gui) fn ancestor_path(
    root: &GuiRoot,
    node: GuiNodeId,
) -> Vec<GuiNodeId> {
    let mut path = vec![node];
    let mut current = node;
    let cap = root.nodes().len().saturating_add(1).max(2);
    while path.len() < cap {
        let Some(parent) = root.nodes().node(current).and_then(|live| live.parent) else {
            break;
        };
        path.push(parent);
        current = parent;
    }
    path.reverse();
    path
}

// ---------------------------------------------------------------------------
// System implementation.
// ---------------------------------------------------------------------------

impl System for GuiInputSystem {
    /// Route admitted ingress after the current layout and final
    /// camera/geometry evaluation. Dependencies order this update after
    /// those passes, so every input of the tick observes the geometry the
    /// same evaluation just produced, including resized, moved and newly
    /// populated panels.
    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let tick = context.world.world.tick.saturating_add(1);
        self.last_tick = tick;
        self.observe_external_commits(context);
        if let Some(binding) = self.layout
            && let Some(layout) = context.dependency(binding)
        {
            self.revalidate_retained_targets(context.world.world, layout, tick);
            self.sync_virtual_lists(context.world.world, layout, tick);
        } else {
            let targets: Vec<_> = self.retained_targets().into_iter().collect();
            for target in targets {
                self.invalidate_target(target, tick, GuiInputCancelReason::TargetHidden);
            }
        }
        let queued = std::mem::take(&mut self.ingress);
        for item in queued {
            self.route_ingress(&context.world, tick, item);
        }
    }

    /// Admit one well-formed input into the ordered ingress queue during
    /// mutation. Malformed inputs fail here, at the command boundary;
    /// routing, ownership checks and reports all happen in [`System::update`].
    fn command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        if let Some(command) = command.downcast_ref::<crate::GuiSemanticActionCommand>() {
            return self.admit_semantic_action(context, session, command);
        }
        let (request_id, command) =
            if let Some(correlated) = command.downcast_ref::<CorrelatedGuiInputCommand>() {
                (correlated.request_id, &correlated.command)
            } else {
                (
                    0,
                    command
                        .downcast_ref::<GuiInputCommand>()
                        .ok_or(ErrorReason::InvalidValue)?,
                )
            };
        match command {
            GuiInputCommand::Key {
                pressed: false,
                ..
            } => return Ok(()),
            GuiInputCommand::Scroll {
                delta,
                ..
            } if !delta.iter().all(|lane| lane.is_finite()) => {
                return Err(ErrorReason::InvalidValue);
            }
            GuiInputCommand::Text {
                text,
                ..
            } if text.is_empty() => return Err(ErrorReason::InvalidValue),
            GuiInputCommand::UpdateComposition {
                text,
                caret_start,
                caret_end,
                ..
            } => validate_composition_shape(text, *caret_start, *caret_end)?,
            GuiInputCommand::Focus {
                handle,
            } if handle.session != session => return Err(ErrorReason::InvalidValue),
            _ => {}
        }
        self.ingress.push(QueuedInput {
            session,
            request_id,
            command: command.clone(),
        });
        Ok(())
    }

    /// Synchronously fence producer identity and authored eligibility before
    /// removed/replaced storage can be reused. Evaluated availability is
    /// reconciled in every scheduled update after layout runs.
    fn lifecycle(
        &mut self,
        context: &SystemLifecycleContext<'_>,
        observation: &LifecycleObservation,
    ) {
        let tick = context.world.next_tick();
        let invalid: Vec<_> = match observation {
            LifecycleObservation::Entity {
                entity,
                kind: EntityLifecycleKind::Deleted,
            } => self
                .retained_targets()
                .into_iter()
                .filter(|target| target.entity == *entity)
                .map(|target| (target, GuiInputCancelReason::TargetRemoved))
                .collect(),
            LifecycleObservation::Component {
                entity,
                component,
                kind,
                incarnation,
                ..
            } if *component == ComponentValue::GUI_ROOT => {
                let targets: Vec<_> = self
                    .retained_targets()
                    .into_iter()
                    .filter(|target| target.entity == *entity)
                    .collect();
                match kind {
                    _ if targets.is_empty() => Vec::new(),
                    ComponentLifecycleKind::Removed | ComponentLifecycleKind::Replaced => targets
                        .into_iter()
                        .map(|target| (target, GuiInputCancelReason::TargetRemoved))
                        .collect(),
                    ComponentLifecycleKind::Inserted | ComponentLifecycleKind::Updated => {
                        // Borrow the committed root: an update batch may carry one
                        // observation per ordered property write.
                        let root = context
                            .world
                            .authored
                            .allocator
                            .contains(*entity)
                            .then(|| {
                                context
                                    .world
                                    .world
                                    .components
                                    .gui_root(entity.index() as usize)
                            })
                            .flatten();
                        targets
                            .into_iter()
                            .filter_map(|target| {
                                let validity = match root {
                                    Some(root) => {
                                        producer_validity(root, incarnation.unwrap_or(0), &target)
                                    }
                                    _ => GuiTargetValidity::Removed,
                                };
                                match validity {
                                    GuiTargetValidity::Eligible => None,
                                    GuiTargetValidity::Removed => {
                                        Some((target, GuiInputCancelReason::TargetRemoved))
                                    }
                                    GuiTargetValidity::Ineligible => {
                                        Some((target, GuiInputCancelReason::TargetHidden))
                                    }
                                }
                            })
                            .collect()
                    }
                }
            }
            _ => Vec::new(),
        };
        for (target, reason) in invalid {
            self.invalidate_target(target, tick, reason);
        }
    }

    /// Admit owned subsystem input after every fallible frame check
    /// succeeds: queued envelopes apply here, at the next mutation boundary
    /// before animation evaluation.
    fn accept_ingress(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let tick = context.world.world.tick.saturating_add(1);
        self.last_tick = tick;
        // Replacements a Host applied between frames precede this frame's
        // envelopes; those applied during this frame's mutation are observed
        // in `update`, before routing.
        self.observe_external_commits(context);
        self.apply_envelopes(&mut context.world, tick);
    }

    /// Detach completed input observations into the frame report.
    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        report: &mut crate::WorldUpdateReport,
    ) {
        self.last_tick = report.tick;
        if self.published_caret_revision != self.caret_revision {
            let update = self.text_focus_state(context.world.world).map_or_else(
                || GuiTextFocusUpdate::Cleared {
                    session: self.published_text_session.unwrap_or(0),
                    context_generation: self.owner_epoch,
                    focus_generation: self.focus_generation,
                },
                GuiTextFocusUpdate::Focused,
            );
            self.published_text_session = match &update {
                GuiTextFocusUpdate::Focused(state) => Some(state.session),
                GuiTextFocusUpdate::Cleared {
                    ..
                } => None,
            };
            self.published_caret_revision = self.caret_revision;
            report.gui_text_focus_updates.push(update);
        }
        report
            .gui_input_effects
            .extend(std::mem::take(&mut self.pending_effects));
        report
            .gui_input_cancellations
            .extend(std::mem::take(&mut self.pending_cancellations));
        report
            .gui_input_conflicts
            .extend(std::mem::take(&mut self.pending_conflicts));
        report
            .gui_unhandled_inputs
            .extend(std::mem::take(&mut self.pending_unhandled));
    }

    /// Release one session's cursors, admitted ingress and queued envelopes
    /// at the fence. Acquiring, replacing or releasing the context owner
    /// moves the epoch; disconnecting the owner frees the context for the
    /// next session. Focus and in-flight intents owned by the session cancel
    /// with session-replacement records; input-owned scroll offsets survive.
    /// Transient caret/selection and provisional composition owned by the
    /// session clear with one authoritative text-focus update; other
    /// sessions' cursors survive.
    fn release_session(&mut self, session: u64) {
        let tick = self.last_tick;
        // Admitted but unrouted ingress from a departing session never
        // routes: delayed input fences here, before ownership moves on.
        self.ingress.retain(|item| item.session != session);
        if self.owner.is_some_and(|owner| owner.session == session) {
            self.owner = None;
            self.keyboard_panel = None;
            self.next_epoch();
        }
        let drained: Vec<_> = std::mem::take(&mut self.envelopes);
        let mut kept = Vec::with_capacity(drained.len());
        for envelope in drained {
            if envelope.session == session {
                if let Some(target) = envelope.target {
                    self.purge_cursors_for_target(&target);
                }
                self.pending_cancellations.push(GuiInputCancellation {
                    session,
                    source_tick: envelope.source_tick,
                    effect_tick: tick,
                    target: envelope.target,
                    reason: GuiInputCancelReason::SessionReplaced,
                });
                continue;
            }
            kept.push(envelope);
        }
        self.envelopes = kept;
        let dropped_focus = self.focus.is_some_and(|focus| focus.session == session);
        if dropped_focus {
            let source_tick = self.focus_tick;
            self.focus = None;
            self.focus_visible = false;
            self.focus_generation = self.focus_generation.saturating_add(1).max(1);
            self.pending_cancellations.push(GuiInputCancellation {
                session,
                source_tick,
                effect_tick: tick,
                target: None,
                reason: GuiInputCancelReason::SessionReplaced,
            });
        }
        let pointers: Vec<u32> = self
            .captures
            .iter()
            .filter(|(_, capture)| capture.session == session)
            .map(|(pointer, _)| *pointer)
            .collect();
        for pointer in pointers {
            self.drop_capture(pointer);
        }
        self.hovers.retain(|_, cursor| cursor.session != session);
        self.scroll_drags.retain(|_, drag| drag.session != session);
        self.scroll_bar_presses
            .retain(|_, press| press.session != session);
        self.scroll_bar_hovers
            .retain(|_, hover| hover.session != session);
        // Ownership derives from captures; rebuild it after the purge.
        self.press_owners.clear();
        for (pointer, capture) in &self.captures {
            self.press_owners.insert(capture.target, *pointer);
        }
        // Predictions resync from committed values on next routing.
        self.predicted.clear();
        self.scroll_predicted.clear();
        let carets_before = self.text_carets.len();
        self.text_carets
            .retain(|_, cursor| cursor.session != session);
        let dropped_carets = self.text_carets.len() != carets_before;
        let dropped_composition = self
            .composition
            .as_ref()
            .is_some_and(|composed| composed.session == session);
        if dropped_composition {
            self.composition = None;
        }
        // Focus clears below; one paint bump covers every cursor loss here.
        if dropped_focus || dropped_carets || dropped_composition {
            self.touch_caret();
        }
    }

    fn has_deferred_input(&self) -> bool {
        !self.envelopes.is_empty() || !self.ingress.is_empty()
    }
}

impl GuiInputSystem {
    /// Observe the external replacements accepted since the last
    /// observation, in commit order: each publishes the committed control
    /// effect a user edit would, marked external. Replacing the focused
    /// text moves the
    /// focus generation and republishes the text focus, so edits stamped
    /// before the replacement conflict and the native buffer resynchronizes
    /// without further input. A provisional composition stays until its
    /// commit conflicts against the moved revision or it is cancelled.
    fn observe_external_commits(&mut self, context: &SystemUpdateContext<'_, '_>) {
        let Some(gui) = self.gui.and_then(|binding| context.dependency(binding)) else {
            return;
        };
        let (commits, next) = gui.external_commits_since(self.external_commit_next);
        self.external_commit_next = next;
        for commit in commits {
            self.pending_effects.push(GuiInputEffect {
                session: commit.session,
                source_tick: commit.tick,
                effect_tick: commit.tick,
                kind: GuiInputEffectKind::ControlCommitted {
                    entity: commit.target.entity,
                    root_incarnation: commit.target.root_incarnation,
                    node: commit.target.node,
                    value: commit.value.clone(),
                    revision: commit.revision,
                    path: commit.path.clone(),
                    source: GuiCommitSource::External,
                },
            });
            if self
                .focus
                .is_some_and(|focus| focus.target == commit.target)
            {
                self.focus_generation = self.focus_generation.saturating_add(1).max(1);
                self.touch_caret();
            }
        }
    }

    fn text_focus_state(&self, sim: &WorldSimulationState) -> Option<GuiTextFocusState> {
        let focus = self.focus?;
        let (text, revision) = if let Some(predicted) = self.predicted.get(&focus.target) {
            match &predicted.value {
                GuiControlValue::Text(text) => (text.clone(), predicted.revision),
                _ => return None,
            }
        } else {
            let (_, root) = producer_root(sim, focus.target.entity)?;
            self.base_text(&root, &focus.target)?
        };
        let cursor = self.text_carets.get(&focus.target);
        let (selection_start, selection_end) =
            match cursor.filter(|cursor| cursor.revision == revision) {
                Some(cursor) => (cursor.anchor.unwrap_or(cursor.caret), cursor.caret),
                None => (text.len() as u32, text.len() as u32),
            };
        let composition = self
            .composition
            .as_ref()
            .filter(|value| value.target == focus.target)
            .map(|value| GuiTextCompositionState {
                text: value.provisional.clone(),
                caret_start: value.caret_start,
                caret_end: value.caret_end,
            });
        Some(GuiTextFocusState {
            session: focus.session,
            context_generation: self.owner_epoch,
            focus_generation: self.focus_generation,
            target: focus.target,
            revision,
            text,
            selection_start,
            selection_end,
            composition,
        })
    }

    /// Route one admitted input against the current evaluation's retained
    /// snapshot, in admission order. Routing failures here (only a missing
    /// layout binding, which factory validation already excludes) report a
    /// conflict rather than dropping later ingress.
    fn route_ingress(&mut self, access: &SystemRuntimeAccess<'_>, tick: u64, item: QueuedInput) {
        let QueuedInput {
            session,
            request_id,
            command,
        } = &item;
        debug_assert_eq!(self.routing_request_id, 0);
        self.routing_request_id = *request_id;
        let failed = |input: &mut Self| {
            input.conflict(
                *session,
                tick,
                None,
                GuiInputConflictReason::AdmissionFailed(ErrorReason::InvalidValue),
            );
        };
        match command.clone() {
            GuiInputCommand::PointerDown {
                pointer,
                panel,
                position,
                button,
                blockers,
                panel_distance,
            } => {
                if self
                    .route_with_layout(access, |input, layout, sim| {
                        let projection = input.project_ray(access, sim, position);
                        input.route_down(
                            layout,
                            sim,
                            *session,
                            tick,
                            command,
                            pointer,
                            panel,
                            position,
                            button,
                            &blockers,
                            panel_distance,
                            projection,
                        );
                    })
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::PointerUp {
                pointer,
                panel,
                position,
                button,
                blockers,
                panel_distance,
            } => {
                if self
                    .route_with_layout(access, |input, layout, sim| {
                        let projection = input.project_ray(access, sim, position);
                        input.route_up(
                            layout,
                            sim,
                            *session,
                            tick,
                            command,
                            pointer,
                            button,
                            panel,
                            position,
                            &blockers,
                            panel_distance,
                            projection,
                        );
                    })
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::PointerMove {
                pointer,
                panel,
                position,
                blockers,
                panel_distance,
            } => {
                if self
                    .route_with_layout(access, |input, layout, sim| {
                        let projection = input.project_ray(access, sim, position);
                        input.route_move(
                            layout,
                            sim,
                            *session,
                            tick,
                            command,
                            pointer,
                            panel,
                            position,
                            &blockers,
                            panel_distance,
                            projection,
                        );
                    })
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::PointerCancel {
                pointer,
            } => {
                self.route_cancel(*session, tick, command, pointer);
            }
            GuiInputCommand::Scroll {
                panel,
                position,
                delta,
                blockers,
                panel_distance,
            } => {
                if self
                    .route_with_layout(access, |input, layout, sim| {
                        let projection = input.project_ray(access, sim, position);
                        input.route_scroll(
                            layout,
                            sim,
                            *session,
                            tick,
                            command,
                            panel,
                            position,
                            delta,
                            &blockers,
                            panel_distance,
                            projection,
                        );
                    })
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::Key {
                key,
                pressed,
            } => {
                debug_assert!(pressed, "key releases are filtered during admission");
                if self
                    .route_with_layout(access, |input, layout, sim| {
                        // Only traversal reads the camera pose.
                        let view = matches!(key, GuiKey::Tab | GuiKey::BackTab)
                            .then(|| input.keyboard_view(access, sim))
                            .flatten();
                        input.route_key(layout, sim, view.as_ref(), *session, tick, command, key);
                    })
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::Text {
                text,
                fence,
            } => {
                if self
                    .route_text(access.world, *session, tick, command, &text, fence.as_ref())
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::SetTextSelection {
                start,
                end,
                fence,
            } => {
                if self
                    .route_with_layout(access, |input, _, sim| {
                        input.route_selection(
                            sim,
                            *session,
                            tick,
                            command,
                            start,
                            end,
                            fence.as_ref(),
                        );
                    })
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::UpdateComposition {
                text,
                caret_start,
                caret_end,
                fence,
            } => {
                if self
                    .route_composition_update(
                        access.world,
                        *session,
                        tick,
                        command,
                        CompositionUpdate {
                            text: &text,
                            caret_start,
                            caret_end,
                            fence: fence.as_ref(),
                        },
                    )
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::CommitComposition {
                fence,
            } => {
                self.route_composition_commit(
                    access.world,
                    *session,
                    tick,
                    command,
                    fence.as_ref(),
                );
            }
            GuiInputCommand::CancelComposition {
                fence,
            } => {
                self.route_composition_cancel(
                    access.world,
                    *session,
                    tick,
                    command,
                    fence.as_ref(),
                );
            }
            GuiInputCommand::Focus {
                handle,
            } => {
                if self
                    .route_with_layout(access, |input, layout, sim| {
                        // Admission already fenced the handle session; only
                        // the layout borrow can fail here.
                        let _ = input.route_focus(sim, layout, *session, tick, command, &handle);
                    })
                    .is_err()
                {
                    failed(self);
                }
            }
            GuiInputCommand::Blur => {
                self.route_blur(*session, tick, command);
            }
        }
        self.routing_request_id = 0;
    }

    /// Route one pointer cancellation: only the pressing session's
    /// envelopes cancel and only its capture drops. A foreign cancellation
    /// with an identical pointer ID reports `NotOwner` without touching
    /// the owner's gesture.
    fn route_cancel(&mut self, session: u64, tick: u64, input: &GuiInputCommand, pointer: u32) {
        if self
            .scroll_drags
            .get(&pointer)
            .is_some_and(|drag| drag.session == session)
        {
            self.scroll_drags.remove(&pointer);
        }
        if self
            .scroll_bar_presses
            .get(&pointer)
            .is_some_and(|press| press.session == session)
        {
            self.scroll_bar_presses.remove(&pointer);
            self.scroll_bar_hovers.remove(&pointer);
        }
        let Some(capture) = self.captures.get(&pointer).copied() else {
            return;
        };
        if capture.session != session {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
            return;
        }
        let mut kept = Vec::with_capacity(self.envelopes.len());
        for envelope in self.envelopes.drain(..) {
            if envelope.session == session && envelope.pointer == Some(pointer) {
                if let Some(target) = envelope.target {
                    self.predicted.remove(&target);
                }
                self.pending_cancellations.push(GuiInputCancellation {
                    session,
                    source_tick: envelope.source_tick,
                    effect_tick: tick,
                    target: envelope.target,
                    reason: GuiInputCancelReason::GestureCancelled,
                });
                continue;
            }
            kept.push(envelope);
        }
        self.envelopes = kept;
        self.drop_capture(pointer);
    }

    /// Route one blur: only the focus owner's session may clear the focus.
    /// Ownership itself survives the blur; release and replacement move it.
    fn route_blur(&mut self, session: u64, tick: u64, input: &GuiInputCommand) {
        let Some(focus) = self.focus else {
            return;
        };
        if focus.session != session {
            self.unhandled(session, tick, input, GuiUnhandledReason::NotOwner);
            return;
        }
        self.cancel_composition_for_target(&focus.target);
        if let Some(cursor) = self.text_carets.get_mut(&focus.target) {
            cursor.anchor = None;
        }
        self.clear_focus();
        self.push_envelope(
            tick,
            PendingEnvelope {
                session,
                epoch: self.owner_epoch,
                source_tick: tick,
                target: None,
                pointer: None,
                kind: EnvelopeKind::Focus {
                    focus: None,
                    keyboard: false,
                },
            },
        );
    }
}

// ---------------------------------------------------------------------------
// WorldContext admission and observation.
// ---------------------------------------------------------------------------

impl crate::WorldContext<'_> {
    /// Queue ordered GUI input in ordinary World mutation order.
    pub fn enqueue_gui_input_command(
        &mut self,
        session: u64,
        command: GuiInputCommand,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command(GuiInputSystem::ID, session, command)
    }

    /// Queue correlated GUI input in ordinary World mutation order.
    pub fn enqueue_gui_input_command_with_reply(
        &mut self,
        session: u64,
        request_id: u64,
        command: GuiInputCommand,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command_with_reply(
            GuiInputSystem::ID,
            session,
            request_id,
            CorrelatedGuiInputCommand {
                request_id,
                command,
            },
        )
    }

    /// Queue one correlated semantic action as one atomic input-system
    /// command. The wire API remains semantic rather than exposing raw input.
    pub fn enqueue_gui_semantic_action_with_reply(
        &mut self,
        session: u64,
        request_id: u64,
        command: crate::GuiSemanticActionCommand,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command_with_reply(GuiInputSystem::ID, session, request_id, command)
    }

    /// Current keyboard focus cursor, if set.
    pub fn gui_input_focus(&self) -> Option<GuiInputFocus> {
        self.system::<GuiInputSystem>(GuiInputSystem::ID)
            .and_then(|input| input.focus)
    }

    /// Input-owned scroll offset for one node, or zero when never scrolled.
    /// Keyed by the consuming ScrollView; hits on non-scrollable content
    /// accumulate on the hit node itself.
    pub fn gui_input_scroll(&self, entity: EntityId, node: GuiNodeId) -> [f32; 2] {
        let (Some(input), Some(layout)) = (
            self.system::<GuiInputSystem>(GuiInputSystem::ID),
            self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID),
        ) else {
            return [0.0, 0.0];
        };
        current_target(self.world, layout, entity, node)
            .and_then(|target| input.scroll_offsets.get(&target))
            .map(|cursor| cursor.offset)
            .unwrap_or([0.0, 0.0])
    }

    /// Current revision of input-owned scroll offsets, or zero without an
    /// input context. Bumped only when an applied scroll actually moves an
    /// offset, so scrolled paint can refresh without reflowing layout.
    pub fn gui_scroll_revision(&self) -> u64 {
        self.system::<GuiInputSystem>(GuiInputSystem::ID)
            .map(|input| input.scroll_revision())
            .unwrap_or(0)
    }

    /// Total final-logical shift for one node from its ancestor ScrollView
    /// offsets, or zero outside scrolled subtrees. Retained paint, hit and
    /// caret consumers apply this translation to the retained rectangle and
    /// clip it by viewports moved with their own outer shifts. Render
    /// preparation places retained paint the same way when the scroll
    /// revision moves.
    pub fn gui_scroll_shift(&self, entity: EntityId, node: GuiNodeId) -> [f32; 2] {
        let (Some(input), Some(layout), Some(root)) = (
            self.system::<GuiInputSystem>(GuiInputSystem::ID),
            self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID),
            self.gui_root(entity),
        ) else {
            return [0.0, 0.0];
        };
        let Some(view) = layout.view(entity) else {
            return [0.0, 0.0];
        };
        input.ancestor_shift(view, root, entity, node)
    }

    /// Retained final-logical rectangle of one evaluated node with its
    /// ancestor scroll shift applied, or None when never evaluated.
    pub fn gui_scrolled_rect(&self, entity: EntityId, node: GuiNodeId) -> Option<[f32; 4]> {
        let layout = self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID)?;
        let view = layout.view(entity)?;
        let record = view.nodes.iter().find(|record| record.node == node)?;
        let shift = self.gui_scroll_shift(entity, node);
        Some([
            record.rect[0] + shift[0],
            record.rect[1] + shift[1],
            record.rect[2],
            record.rect[3],
        ])
    }

    /// Hovered node for one pointer, if set.
    ///
    /// Read-only embedder snapshot of the routing hover cursor, mirroring
    /// [`Self::gui_input_focus`], so a host can observe hover without
    /// reconstructing it from `HoverChanged` effects. Never mutates cursors,
    /// predictions or envelopes.
    pub fn gui_input_hover(&self, pointer: u32) -> Option<GuiInputTarget> {
        self.system::<GuiInputSystem>(GuiInputSystem::ID)
            .and_then(|input| input.hovers.get(&pointer).map(|cursor| cursor.target))
    }

    /// Pressed (captured) node for one pointer, if set.
    ///
    /// Read-only embedder snapshot of the routing capture cursor, mirroring
    /// [`Self::gui_input_focus`], so a host can observe the press without
    /// reconstructing it from routed effects. Never mutates cursors,
    /// predictions or envelopes.
    pub fn gui_input_pressed(&self, pointer: u32) -> Option<GuiInputTarget> {
        self.system::<GuiInputSystem>(GuiInputSystem::ID)
            .and_then(|input| input.captures.get(&pointer).map(|capture| capture.target))
    }

    /// Whether routed input still awaits its mutation boundary.
    #[cfg(test)]
    pub(crate) fn gui_input_has_deferred(&self) -> bool {
        self.system::<GuiInputSystem>(GuiInputSystem::ID)
            .is_some_and(|input| input.has_deferred_input())
    }

    /// Transient caret/selection for one text input as `(start, end, revision)`.
    /// Byte offsets, `start <= end`, collapsed while no selection. Returns the
    /// end of the predicted/committed text when never edited, or None for
    /// non-text nodes. External resets fence the revision: a stale cursor
    /// reports the end of the current text.
    pub fn gui_text_selection(&self, entity: EntityId, node: GuiNodeId) -> Option<(u32, u32, u32)> {
        let input = self.system::<GuiInputSystem>(GuiInputSystem::ID)?;
        let layout = self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID)?;
        let target = current_target(self.world, layout, entity, node)?;
        let (base_text, base_revision) = input.predicted_text_or_committed(self, entity, node)?;
        let Some(cursor) = input.text_carets.get(&target) else {
            let end = base_text.len() as u32;
            return Some((end, end, base_revision));
        };
        if cursor.revision != base_revision {
            let end = base_text.len() as u32;
            return Some((end, end, base_revision));
        }
        let caret = super::text_edit::snap_to_boundary(
            &base_text,
            cursor.caret.min(base_text.len() as u32),
        );
        let anchor = cursor.anchor.map(|anchor| {
            super::text_edit::snap_to_boundary(&base_text, anchor.min(base_text.len() as u32))
        });
        match anchor {
            Some(anchor) if anchor != caret => {
                let (start, end) = super::text_edit::normalize_range(anchor, caret);
                Some((start, end, base_revision))
            }
            _ => Some((caret, caret, base_revision)),
        }
    }

    /// Active provisional IME composition for one text input as
    /// `(provisional, caret_start, caret_end, revision)`, or None.
    /// The provisional never enters retained views until explicit commit.
    pub fn gui_text_composition(
        &self,
        entity: EntityId,
        node: GuiNodeId,
    ) -> Option<(String, u32, u32, u32)> {
        let input = self.system::<GuiInputSystem>(GuiInputSystem::ID)?;
        let layout = self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID)?;
        let target = current_target(self.world, layout, entity, node)?;
        let composed = input.composition.as_ref()?;
        if composed.target != target {
            return None;
        }
        Some((
            composed.provisional.clone(),
            composed.caret_start,
            composed.caret_end,
            composed.revision,
        ))
    }

    /// Logical caret pen `[x, y, 0, height]` for one text input, or None.
    /// Reuses the retained measurement (independent headless metrics scaled by
    /// the logical `font_size` from `content_origin`); predicted carets
    /// past the retained revision have no rect until the commit reflows.
    pub fn gui_text_caret_rect(&self, entity: EntityId, node: GuiNodeId) -> Option<[f32; 4]> {
        let (caret, _, revision) = self.gui_text_caret_parts(entity, node)?;
        let layout = self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID)?;
        text_caret_rect(layout, entity, node, caret, revision)
    }

    /// Logical per-line selection rects for one text input (empty collapsed).
    /// Same retained-metrics contract as [`Self::gui_text_caret_rect`].
    pub fn gui_text_selection_rects(&self, entity: EntityId, node: GuiNodeId) -> Vec<[f32; 4]> {
        let Some((start, end, revision)) = self.gui_text_selection(entity, node) else {
            return Vec::new();
        };
        if start == end {
            return Vec::new();
        }
        let Some(layout) = self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID) else {
            return Vec::new();
        };
        text_selection_rects(layout, entity, node, start, end, revision)
    }

    /// Caret offset plus anchor for geometry queries.
    fn gui_text_caret_parts(
        &self,
        entity: EntityId,
        node: GuiNodeId,
    ) -> Option<(u32, Option<u32>, u32)> {
        let input = self.system::<GuiInputSystem>(GuiInputSystem::ID)?;
        let layout = self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID)?;
        let target = current_target(self.world, layout, entity, node)?;
        let (base_text, revision) = input.predicted_text_or_committed(self, entity, node)?;
        let Some(cursor) = input
            .text_carets
            .get(&target)
            .filter(|cursor| cursor.revision == revision)
        else {
            return Some((base_text.len() as u32, None, revision));
        };
        let caret = super::text_edit::snap_to_boundary(
            &base_text,
            cursor.caret.min(base_text.len() as u32),
        );
        let anchor = cursor
            .anchor
            .map(|anchor| {
                super::text_edit::snap_to_boundary(&base_text, anchor.min(base_text.len() as u32))
            })
            .filter(|anchor| *anchor != caret);
        Some((caret, anchor, revision))
    }
}

#[cfg(test)]
#[path = "commit_source_tests.rs"]
mod commit_source_tests;
#[cfg(test)]
#[path = "drag_scrolling_tests.rs"]
mod drag_scrolling_tests;
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[cfg(test)]
#[path = "routing_tests.rs"]
mod routing_tests;
#[cfg(test)]
#[path = "scroll_bar_tests.rs"]
mod scroll_bar_tests;
#[cfg(test)]
#[path = "scrolling_tests.rs"]
mod scrolling_tests;
#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "text_fence_tests.rs"]
mod text_fence_tests;
#[cfg(test)]
#[path = "text_submit_tests.rs"]
mod text_submit_tests;
#[cfg(test)]
#[path = "text_tests.rs"]
mod text_tests;
#[cfg(test)]
#[path = "traversal_tests.rs"]
mod traversal_tests;

#[cfg(test)]
#[path = "virtual_scroll_tests.rs"]
mod virtual_scroll_tests;
