use super::*;
use crate::services::gui_input::{GuiInputCommand, GuiInputError};

/// Delivery-reserved routed GUI work. Raw requests carry no execution authority.
pub struct GuiLocalCommand {
    pub(super) input: GuiInputCommand,
    pub(super) operation: GuiLocalOperation,
    pub(super) physical_committed: std::cell::Cell<bool>,
}

pub(super) enum GuiLocalOperation {
    TextCaret {
        offset: u32,
        extend: bool,
    },
    Text {
        fence: GuiTextFence,
        edit: GuiTextEdit,
    },
    Interaction {
        lease: crate::services::gui_input::GuiPointerLease,
        update: GuiInteractionUpdate,
        part: GuiInteractionPart,
    },
    Action(GuiLocalAction),
    Scroll {
        chain: GuiScrollChain,
        ordinal: usize,
    },
    SliderStep(f32),
    Focus {
        visible: bool,
    },
    ScrollAxis {
        axis: usize,
        offset: f32,
    },
}

impl GuiLocalCommand {
    /// Pointer selection uses the completed label metrics, then validates local focus at commit.
    pub fn text_caret(
        input: GuiInputCommand,
        offset: u32,
        extend: bool,
    ) -> Result<Self, GuiInputError> {
        Self::new(
            input,
            GuiLocalOperation::TextCaret {
                offset,
                extend,
            },
        )
    }

    /// Strict native-buffer edit, still requiring the exact routed context/path.
    pub fn text(
        input: GuiInputCommand,
        fence: GuiTextFence,
        edit: GuiTextEdit,
    ) -> Result<Self, GuiInputError> {
        if input.target() != fence.target {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(
            input,
            GuiLocalOperation::Text {
                fence,
                edit,
            },
        )
    }

    /// Physical focus modality is feedback, not an authored property or value.
    pub fn focus(input: GuiInputCommand, visible: bool) -> Result<Self, GuiInputError> {
        Self::new(
            input,
            GuiLocalOperation::Focus {
                visible,
            },
        )
    }

    /// Apply a physical key increment to the current `value` field.
    pub fn slider_step(input: GuiInputCommand, steps: f32) -> Result<Self, GuiInputError> {
        if !steps.is_finite() {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(input, GuiLocalOperation::SliderStep(steps))
    }

    /// Move one dragged bar without overwriting the other axis's current offset.
    pub fn scroll_axis(
        input: GuiInputCommand,
        axis: usize,
        offset: f32,
    ) -> Result<Self, GuiInputError> {
        if axis > 1 || !offset.is_finite() {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(
            input,
            GuiLocalOperation::ScrollAxis {
                axis,
                offset,
            },
        )
    }

    /// Consume a wheel/drag remainder at the ordered local mutation boundary.
    pub fn scroll(input: GuiInputCommand, chain: GuiScrollChain) -> Result<Self, GuiInputError> {
        Self::new(
            input,
            GuiLocalOperation::Scroll {
                ordinal: chain.register()?,
                chain,
            },
        )
    }

    pub(in crate::world::systems::gui) fn ready(&self) -> bool {
        self.input.is_cancelled()
            || match &self.operation {
                GuiLocalOperation::Scroll {
                    chain,
                    ordinal,
                } => chain.ready(*ordinal),
                _ => true,
            }
    }

    /// Delivery-reserved physical feedback with exact pointer activation identity.
    pub fn interaction(
        input: GuiInputCommand,
        lease: crate::services::gui_input::GuiPointerLease,
        update: GuiInteractionUpdate,
    ) -> Result<Self, GuiInputError> {
        Self::part_interaction(input, lease, update, GuiInteractionPart::Control)
    }

    /// Physical feedback that names the hovered or pressed part of the control.
    pub fn part_interaction(
        input: GuiInputCommand,
        lease: crate::services::gui_input::GuiPointerLease,
        update: GuiInteractionUpdate,
        part: GuiInteractionPart,
    ) -> Result<Self, GuiInputError> {
        if let Err(error) = lease.validate(&input) {
            input.reject(error);
            return Err(error);
        }
        Self::new(
            input,
            GuiLocalOperation::Interaction {
                lease,
                update,
                part,
            },
        )
    }

    /// Construct physical work routed from a completed publication.
    pub fn routed(input: GuiInputCommand, action: GuiLocalAction) -> Result<Self, GuiInputError> {
        Self::new(input, GuiLocalOperation::Action(action))
    }

    fn new(input: GuiInputCommand, operation: GuiLocalOperation) -> Result<Self, GuiInputError> {
        let text = match &operation {
            GuiLocalOperation::Action(GuiLocalAction::SetText(text)) => Some(text),
            _ => None,
        };
        if text.is_some_and(|text| super::component::bounded_text(text).is_err()) {
            let error = GuiInputError::Local(GuiLocalActionError::InvalidValue);
            input.reject(error);
            return Err(error);
        }
        let additional = match &operation {
            GuiLocalOperation::Text {
                edit,
                ..
            } => edit.retained_bytes(),
            _ => 0,
        };
        let bytes = std::mem::size_of::<Self>()
            .checked_add(text.map_or(0, |text| text.len()))
            .and_then(|bytes| bytes.checked_add(additional));
        let Some(bytes) = bytes else {
            input.reject(GuiInputError::Capacity);
            return Err(GuiInputError::Capacity);
        };
        input.reserve_payload(bytes)?;
        Ok(Self {
            input,
            operation,
            physical_committed: std::cell::Cell::new(false),
        })
    }

    pub(in crate::world::systems::gui) fn world_references(
        &self,
        visit: &mut dyn FnMut(crate::WorldRef),
    ) {
        self.input.world_references(visit);
    }
}

impl Drop for GuiLocalCommand {
    fn drop(&mut self) {
        if let GuiLocalOperation::Scroll {
            chain,
            ..
        } = &self.operation
            && !self.physical_committed.get()
        {
            chain.abandon();
        }
    }
}
