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
    SliderStep {
        steps: f32,
        fine: bool,
        part: u32,
    },
    SliderThumb {
        part: u32,
        value: f32,
    },
    ColorChannels {
        channels: [Option<f32>; 4],
    },
    ColorStep {
        channel: usize,
        steps: f32,
        fine: bool,
    },
    Focus {
        visible: bool,
        part: u32,
    },
    Context {
        point: [f32; 2],
    },
    ScrollAxis {
        axis: usize,
        offset: f32,
    },
    Select,
    ActiveItem,
    Number(super::number::GuiNumberOperation),
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

    /// Focus the control's focus `part`, 0 for a control with one. Physical
    /// focus modality is feedback, not an authored property or value.
    pub fn focus(input: GuiInputCommand, part: u32, visible: bool) -> Result<Self, GuiInputError> {
        Self::new(
            input,
            GuiLocalOperation::Focus {
                visible,
                part,
            },
        )
    }

    /// A momentary context request on the control at a canvas logical point;
    /// any control role accepts it.
    pub fn context(input: GuiInputCommand, point: [f32; 2]) -> Result<Self, GuiInputError> {
        if !point.iter().all(|value| value.is_finite()) {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(
            input,
            GuiLocalOperation::Context {
                point,
            },
        )
    }

    /// Move the current `value` field by `steps` of its step, or of its fine
    /// step when `fine`, from an arrow key or the wheel.
    pub fn slider_step(
        input: GuiInputCommand,
        steps: f32,
        fine: bool,
    ) -> Result<Self, GuiInputError> {
        Self::slider_thumb_step(input, 0, steps, fine)
    }

    /// Move thumb `part` of a slider, a range's upper thumb for 1, by `steps`
    /// of its step, or of its fine step when `fine`, from its current value
    /// and up to the other thumb of a range.
    pub fn slider_thumb_step(
        input: GuiInputCommand,
        part: u32,
        steps: f32,
        fine: bool,
    ) -> Result<Self, GuiInputError> {
        if !steps.is_finite() {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(
            input,
            GuiLocalOperation::SliderStep {
                steps,
                fine,
                part,
            },
        )
    }

    /// Move thumb `part` of a slider to `value`, clamped to the range and,
    /// on a range, stopped at the other thumb: a drag, a track press, Home
    /// or End.
    pub fn slider_thumb(
        input: GuiInputCommand,
        part: u32,
        value: f32,
    ) -> Result<Self, GuiInputError> {
        if !value.is_finite() {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(
            input,
            GuiLocalOperation::SliderThumb {
                part,
                value,
            },
        )
    }

    /// Step a numeric text input by `steps` of its step, or of its fine step
    /// when `fine`, after committing its pending edit: Up and Down.
    pub fn number_step(
        input: GuiInputCommand,
        steps: f32,
        fine: bool,
    ) -> Result<Self, GuiInputError> {
        if !steps.is_finite() {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(
            input,
            GuiLocalOperation::Number(super::number::GuiNumberOperation::Step {
                steps,
                fine,
                part: None,
            }),
        )
    }

    /// A press on a numeric text input's step `part`: one step after
    /// committing its pending edit, repeating while a pointer holds the part.
    pub fn number_part(input: GuiInputCommand, part: GuiNumberStep) -> Result<Self, GuiInputError> {
        Self::new(
            input,
            GuiLocalOperation::Number(super::number::GuiNumberOperation::Step {
                steps: part.steps(),
                fine: false,
                part: Some(part),
            }),
        )
    }

    /// Commit a numeric text input's pending edit before focus leaves it.
    pub fn number_commit(input: GuiInputCommand) -> Result<Self, GuiInputError> {
        Self::new(
            input,
            GuiLocalOperation::Number(super::number::GuiNumberOperation::Commit {
                submit: false,
            }),
        )
    }

    /// Discard a numeric text input's pending edit: Escape.
    pub fn number_discard(input: GuiInputCommand) -> Result<Self, GuiInputError> {
        Self::new(
            input,
            GuiLocalOperation::Number(super::number::GuiNumberOperation::Discard),
        )
    }

    /// Set the colour channels `channels` names, in field order (hue,
    /// saturation, value, alpha), each clamped to `0..=1`: a drag on the
    /// field or a rail, Home or End. Channels without a value keep theirs.
    pub fn color_channels(
        input: GuiInputCommand,
        channels: [Option<f32>; 4],
    ) -> Result<Self, GuiInputError> {
        if !channels.iter().flatten().all(|value| value.is_finite()) {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(
            input,
            GuiLocalOperation::ColorChannels {
                channels,
            },
        )
    }

    /// Move colour channel `channel` in field order by `steps` of the colour
    /// step, or of the fine step when `fine`, from its current value and
    /// within `0..=1`: an arrow key or the wheel.
    pub fn color_step(
        input: GuiInputCommand,
        channel: usize,
        steps: f32,
        fine: bool,
    ) -> Result<Self, GuiInputError> {
        if channel > 3 || !steps.is_finite() {
            input.reject(GuiInputError::Unavailable);
            return Err(GuiInputError::Unavailable);
        }
        Self::new(
            input,
            GuiLocalOperation::ColorStep {
                channel,
                steps,
                fine,
            },
        )
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

    /// Select a Button item of a group that selects, as arrow movement does
    /// in a group whose selection follows it.
    pub fn select(input: GuiInputCommand) -> Result<Self, GuiInputError> {
        Self::new(input, GuiLocalOperation::Select)
    }

    /// Make an item that does not take focus its group's active item.
    pub fn active_item(input: GuiInputCommand) -> Result<Self, GuiInputError> {
        Self::new(input, GuiLocalOperation::ActiveItem)
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
