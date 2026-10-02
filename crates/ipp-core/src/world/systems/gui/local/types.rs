use crate::systems::canvas::CanvasTarget;
use crate::{ComponentValue, EntityId, WorldPublicationId, WorldRef};
use std::sync::Arc;

/// A control's ordinary entity and exact component lifetime in one World.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuiEntityTarget {
    /// Exact World lifetime, not a selected root presentation.
    pub world: WorldRef,
    /// Generational entity identity.
    pub entity: EntityId,
    /// The concrete control component, distinguishing control kinds.
    pub component: u16,
    /// Component lifetime; replacement never reuses this identity.
    pub incarnation: u64,
}

impl GuiEntityTarget {
    /// The same local identity carried by Canvas paint and hit records.
    pub fn canvas_target(self) -> CanvasTarget {
        CanvasTarget {
            entity: self.entity,
            component: self.component,
            incarnation: self.incarnation,
        }
    }

    /// Qualify a completed Canvas hit by its publication's World lifetime.
    pub fn from_canvas(world: WorldRef, target: CanvasTarget) -> Self {
        Self {
            world,
            entity: target.entity,
            component: target.component,
            incarnation: target.incarnation,
        }
    }
}

/// Concrete local control role, independent of its appearance or layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiControlKind {
    /// Momentary activation without a value.
    Button,
    /// Boolean `checked` field.
    Checkbox,
    /// Numeric `value` field constrained by the authored range.
    Slider,
    /// Single-line Unicode `text` field.
    TextInput,
    /// Ordinary scrolling viewport.
    ScrollView,
    /// Scrolling viewport with sparsely realized indexed items.
    VirtualList,
    /// One HSVA colour in `hue`, `saturation`, `value` and `alpha` fields.
    Color,
}

impl GuiControlKind {
    /// The role of a control component, if `component` is one.
    pub fn of_component(component: u16) -> Option<Self> {
        Some(match component {
            ComponentValue::GUI_BUTTON => Self::Button,
            ComponentValue::GUI_CHECKBOX => Self::Checkbox,
            ComponentValue::GUI_SLIDER => Self::Slider,
            ComponentValue::GUI_TEXT_INPUT => Self::TextInput,
            ComponentValue::GUI_SCROLL_VIEW => Self::ScrollView,
            ComponentValue::GUI_VIRTUAL_LIST => Self::VirtualList,
            ComponentValue::GUI_COLOR => Self::Color,
            _ => return None,
        })
    }
}

/// A control operation: the action of a [`Command::GuiAction`](crate::Command::GuiAction)
/// or of already-routed physical input, validated against the control's
/// identity and eligibility only.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiLocalAction {
    /// Momentary button activation; focus stays unchanged.
    Press,
    /// Invert a checkbox's `checked` field.
    Toggle,
    /// Set a slider's `value` field within its current configured range.
    SetScalar(f32),
    /// Replace a text input's single-line `text` field.
    SetText(Arc<str>),
    /// Set a colour control's hue, saturation, value and alpha, each in
    /// `0..=1`.
    SetColor([f32; 4]),
    /// Submit the current text without changing it or focus.
    Submit,
    /// Move to a logical offset, clamped to the capacity of the last layout.
    ScrollTo([f32; 2]),
    /// Consume a logical delta within the capacity of the last layout.
    ScrollBy([f32; 2]),
    /// Anchor a virtual item at a logical offset within it; layout derives the offset.
    ScrollToIndex {
        /// Logical item index within the current count.
        index: u32,
        /// Nonnegative logical offset into the item.
        offset: f32,
    },
    /// Move logical focus to a focus part of the control without requiring
    /// native presentation: part 0 for a control with one, a range's upper
    /// thumb 1, a colour control's hue rail 1 or alpha rail 2. A part the
    /// control does not have is unsupported.
    Focus(u32),
    /// Release this session's logical focus on the exact target.
    Blur,
}

/// Provenance of an applied local effect, not an ingress permission token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiLocalEffectSource {
    /// A [`Command::GuiAction`](crate::Command::GuiAction), or the GUI
    /// System's own change such as an overlay moving focus in or back; valid
    /// without a presented root or native input context.
    Semantic,
    /// Host-routed input validated against this completed source publication.
    Routed {
        /// Publication used by the Host router; path/context fences stay Host-owned.
        publication: WorldPublicationId,
    },
}

/// The concrete reason a local operation did not apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiLocalActionError {
    /// World, entity or component lifetime no longer matches.
    StaleTarget,
    /// Hidden, disabled, unavailable or ambiguous local control.
    Unavailable,
    /// This action does not match the control role.
    UnsupportedAction,
    /// The new value violates its text or numeric constraints.
    InvalidValue,
}

/// One applied operation's momentary outcome. Control values are component
/// fields and reach clients through field observation, not through effects.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiLocalEffectKind {
    /// Momentary button press; no value or focus transition.
    Pressed,
    /// Submitted text, retained at the application boundary: a numeric
    /// input's committed number as it shows it.
    Submitted(Arc<str>),
    /// Text a numeric input refused to commit because it does not parse; its
    /// number is unchanged. Clients show an error beside the field.
    Rejected(Arc<str>),
    /// A numeric input's pending edit ended without being committed or
    /// refused, such as by Escape, carrying the discarded text; its number is
    /// unchanged and it shows it formatted. Clients clear what they showed
    /// for the edit.
    Discarded(Arc<str>),
    /// Momentary context request from a secondary press, the Menu key or
    /// Shift+F10; the client decides what opens. No value or focus change is
    /// implied: routed input focuses the target before requesting.
    ContextRequested {
        /// Canvas logical point of the request in the target's canvas: the
        /// press point, or the bottom-left corner of the control's visible
        /// box for a key.
        point: [f32; 2],
    },
    /// Resulting logical focus on the target; ownership and focus part changes
    /// count as changes too.
    FocusChanged {
        /// Whether the effect target is logically focused after this operation.
        focused: bool,
        /// Whether this operation changes focus, its focus part or its
        /// input-session ownership.
        changed: bool,
        /// The focus part focus names after this operation, or the part a
        /// blurred target held; 0 for a control with one part.
        part: u32,
    },
    /// Transient ordinary pointer feedback; no value or focus action is implicit.
    InteractionChanged(super::GuiInteractionEffect),
}

/// Applied local effect with the core ancestry captured at its mutation boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiLocalEffect {
    /// Actual applied-record identity; outcomes that are never observed have none.
    pub id: Option<super::super::observations::GuiEffectId>,
    /// Exact affected control lifetime.
    pub target: GuiEntityTarget,
    /// Source category, separate from session and acknowledgement delivery.
    pub source: GuiLocalEffectSource,
    /// World tick applying the effect.
    pub tick: u64,
    /// Root-first core ancestry at commit, unaffected by later reparenting.
    pub ancestry: Arc<[EntityId]>,
    /// Actual local effect.
    pub kind: GuiLocalEffectKind,
}
