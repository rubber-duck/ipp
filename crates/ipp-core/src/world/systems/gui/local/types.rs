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
    /// Move logical focus without requiring native presentation.
    Focus,
    /// Release this session's logical focus on the exact target.
    Blur,
}

/// Provenance of an applied local effect, not an ingress permission token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiLocalEffectSource {
    /// A [`Command::GuiAction`](crate::Command::GuiAction), valid without a
    /// presented root or native input context.
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
    /// Submitted text, retained at the application boundary.
    Submitted(Arc<str>),
    /// Resulting logical focus on the target; ownership changes count as changes too.
    FocusChanged {
        /// Whether the effect target is logically focused after this operation.
        focused: bool,
        /// Whether this operation changes focus or its input-session ownership.
        changed: bool,
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
