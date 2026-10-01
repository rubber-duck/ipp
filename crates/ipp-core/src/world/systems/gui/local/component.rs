use crate::components::schema::ComponentLifecycle;
use crate::{ComponentValue, ErrorReason};
use ipp_schema_derive::SchemaComponent;
use std::sync::Arc;

/// Inherited local interaction policy, independent of native input ownership.
///
/// Every control requires it. The authored policy fields are inherited by
/// descendants; the eligibility fields hold this entity's evaluated result,
/// which the GUI System writes after each commit. An entity without this
/// component contributes the defaults to its descendants.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct GuiBehavior {
    /// Whether this entity and its descendants accept control actions.
    pub enabled: bool,
    /// Whether this entity and its descendants participate in paint and input.
    pub visible: bool,
    /// Whether keyboard traversal is bounded by this subtree.
    pub focus_scope: bool,
    /// Explicit semantic name; an empty value uses the control's label.
    pub semantic_label: Arc<str>,
    /// Evaluated: this entity and all its ancestors are enabled.
    pub effective_enabled: bool,
    /// Evaluated: this entity and all its ancestors are visible.
    pub effective_visible: bool,
    /// Evaluated: the control and its local scope are ready for actions and input.
    pub available: bool,
}

impl Default for GuiBehavior {
    fn default() -> Self {
        Self {
            enabled: true,
            visible: true,
            focus_scope: false,
            semantic_label: Arc::default(),
            effective_enabled: true,
            effective_visible: true,
            available: true,
        }
    }
}

impl ComponentLifecycle for GuiBehavior {
    fn validate(&self) -> Result<(), ErrorReason> {
        bounded_text(&self.semantic_label)
    }
}

/// A momentary control. Pressing it does not create a committed value.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiButton {
    /// Authored visible and default semantic label.
    pub label: Arc<str>,
}

impl ComponentLifecycle for GuiButton {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        bounded_text(&self.label)
    }
}

/// A boolean control whose committed value is an ordinary field.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiCheckbox {
    /// Authored visible and default semantic label.
    pub label: Arc<str>,
    /// Committed value, written by clients and by the GUI System.
    pub checked: bool,
}

impl ComponentLifecycle for GuiCheckbox {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        bounded_text(&self.label)
    }
}

/// A numeric control whose committed value is an ordinary field.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiSlider {
    /// Inclusive lower bound for accepted commits.
    pub min: f32,
    /// Inclusive upper bound for accepted commits.
    pub max: f32,
    /// Step size; zero means continuous input.
    pub step: f32,
    /// Committed value, written by clients and by the GUI System.
    pub value: f32,
}

impl Default for GuiSlider {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            step: 0.0,
            value: 0.0,
        }
    }
}

impl ComponentLifecycle for GuiSlider {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if [self.min, self.max, self.step, self.value]
            .into_iter()
            .all(f32::is_finite)
            && self.min <= self.max
            && self.step >= 0.0
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// A single-line text control whose committed text is an ordinary field.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiTextInput {
    /// Text painted when the committed value is empty.
    pub placeholder: Arc<str>,
    /// Committed single-line text, written by clients and by the GUI System.
    pub text: Arc<str>,
}

impl ComponentLifecycle for GuiTextInput {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        bounded_text(&self.placeholder)?;
        single_line_text(&self.text)
    }
}

/// Maximum UTF-8 bytes in one authored, committed, provisional or restored
/// GUI text value.
pub const MAX_GUI_TEXT_BYTES: usize = 65_536;

/// Components every control requires; missing ones are inserted with defaults.
pub(in crate::world::systems::gui) const CONTROL_REQUIREMENTS: [u16; 2] =
    [ComponentValue::GUI_BEHAVIOR, ComponentValue::CANVAS_BOUNDS];

pub(super) const CONTROL_COMPONENTS: [u16; 6] = [
    ComponentValue::GUI_BUTTON,
    ComponentValue::GUI_CHECKBOX,
    ComponentValue::GUI_SLIDER,
    ComponentValue::GUI_TEXT_INPUT,
    ComponentValue::GUI_SCROLL_VIEW,
    ComponentValue::GUI_VIRTUAL_LIST,
];

pub(super) fn bounded_text(text: &str) -> Result<(), ErrorReason> {
    if text.len() <= MAX_GUI_TEXT_BYTES {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

pub(super) fn single_line_text(text: &str) -> Result<(), ErrorReason> {
    bounded_text(text)?;
    if text.contains(['\r', '\n']) {
        Err(ErrorReason::InvalidValue)
    } else {
        Ok(())
    }
}
