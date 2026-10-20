//! Client-managed input. Core owns routing; a live exclusive observer owns behavior.

use super::{GuiEntityTarget, GuiLocalActionError};
use crate::ErrorReason;
use crate::components::schema::ComponentLifecycle;
use ipp_schema_derive::SchemaComponent;
use std::sync::Arc;

/// A focusable Canvas region whose input is delivered to a client subscription.
/// It paints no implicit content. Ctrl+Shift+Escape always releases focus.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiInputTarget {
    /// Deliver Tab to the client instead of traversing to the next control.
    pub capture_tab: bool,
    /// Deliver Escape to the client instead of releasing focus.
    pub capture_escape: bool,
}

impl ComponentLifecycle for GuiInputTarget {
    fn required_components() -> &'static [u16] {
        &super::component::CONTROL_REQUIREMENTS
    }
}

/// Maximum UTF-8 bytes in one committed-text input record.
pub const GUI_CLIENT_TEXT_MAX_BYTES: usize = 65_536;

/// Shift, Control, Alt and Meta occupy bits 0 through 3 respectively.
pub const GUI_CLIENT_MODIFIERS: u8 = 15;

/// Typed, bounded input for client-managed content; positions are target-local.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiClientInput {
    /// A logical key name (DOM key vocabulary), separate from committed text.
    Key {
        /// A Unicode key or named key such as ArrowLeft or F12.
        key: Arc<str>,
        /// Physical key code, independent of the active keyboard layout.
        code: Arc<str>,
        /// Shift, Control, Alt, Meta bitmask.
        modifiers: u8,
        /// Whether the platform reports an auto-repeat.
        repeat: bool,
        /// Press when true, release when false.
        pressed: bool,
    },
    /// Committed Unicode text, including a completed platform composition or paste.
    Text {
        /// Committed Unicode content.
        text: Arc<str>,
        /// Clipboard paste, distinct from typing for bracketed-paste encoding.
        paste: bool,
    },
    /// Pointer phases: down 0, move 1, up 2, cancel 3; buttons: primary 0,
    /// secondary 1, auxiliary 2, no button 3.
    Pointer {
        /// Pointer identity within its physical context.
        pointer: u64,
        /// Phase as described above.
        phase: u8,
        /// Button as described above.
        button: u8,
        /// Point in the control's local logical coordinates.
        point: [f32; 2],
    },
    /// Wheel movement in Canvas logical units at a target-local point.
    Wheel {
        /// Point in the control's local logical coordinates.
        point: [f32; 2],
        /// Movement in logical units.
        delta: [f32; 2],
    },
}

impl GuiClientInput {
    /// Owned payload bytes charged to ingress and reliable delivery.
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Key {
                key,
                code,
                ..
            } => key.len() + code.len(),
            Self::Text {
                text,
                ..
            } => text.len(),
            _ => 0,
        }
    }

    /// Validate before accepting owned input into a bounded queue.
    pub fn validate(&self) -> Result<(), ErrorReason> {
        let valid = match self {
            Self::Key {
                key,
                code,
                modifiers,
                ..
            } => {
                !key.is_empty()
                    && key.len() <= 64
                    && !code.is_empty()
                    && code.len() <= 64
                    && modifiers & !GUI_CLIENT_MODIFIERS == 0
            }
            Self::Text {
                text,
                ..
            } => text.len() <= GUI_CLIENT_TEXT_MAX_BYTES,
            Self::Pointer {
                phase,
                button,
                point,
                ..
            } => *phase <= 3 && *button <= 3 && point.iter().all(|v| v.is_finite()),
            Self::Wheel {
                point,
                delta,
            } => point.iter().chain(delta).all(|v| v.is_finite()),
        };
        if valid {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

impl crate::systems::gui::system_state::GuiLocalState {
    pub(in crate::world::systems::gui) fn validate_client_input(
        &self,
        target: GuiEntityTarget,
        binding: super::super::observations::GuiObservationSubscriptionId,
        input: &GuiClientInput,
    ) -> Result<(), GuiLocalActionError> {
        if target.component != crate::ComponentValue::GUI_INPUT_TARGET
            || self.observations.input_binding(target) != Some(binding)
        {
            return Err(GuiLocalActionError::Unavailable);
        }
        input
            .validate()
            .map_err(|_| GuiLocalActionError::InvalidValue)
    }
}
