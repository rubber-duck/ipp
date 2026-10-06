//! Per-control value routing: text carets, slider and dial drags, range thumbs,
//! numeric steps and colour channels, computed from the completed control geometry
//! and applied through ordinary routed commands.

use super::routing::{GuiInputRouter, GuiRoutingContext, Target, enqueue, local_point};
use super::{GuiInputError, GuiPhysicalKey, GuiRoutingDelivery, GuiRoutingDisposition};
use crate::systems::gui::local::controls::slider::GuiDialDrag;
use crate::systems::gui::local::{GuiLocalAction, GuiLocalCommand};
use crate::{HostRuntime, ViewQueryTarget};

impl GuiInputRouter {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn caret(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        extend: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let (layout, origin) = target
            .control
            .text
            .as_ref()
            .ok_or(GuiInputError::Unavailable)?;
        let point = local_point(host, query, target, point)?;
        let offset = crate::systems::gui::local::controls::text_edit::caret_offset_at_x(
            layout,
            (point[0] - origin[0]) / layout.font_size,
        );
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::text_caret(input, offset, extend)?,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn slide(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        offset: f32,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let slider = target.control.slider.ok_or(GuiInputError::Unavailable)?;
        let mut local = local_point(host, query, target, point)?;
        local[slider.axis] -= offset;
        let fraction = slider.fraction_at(local);
        let crate::systems::gui::presentation::GuiRoutingValue::Scalar(lower) =
            target.control.record.value
        else {
            return Err(GuiInputError::Unavailable);
        };
        let current = match (slider.range, target.focus_part) {
            (Some(range), Some(part)) => range.values[part.min(1) as usize],
            _ => lower,
        };
        let value = crate::systems::gui::local::controls::slider::value_at(
            slider.min,
            slider.max,
            slider.step,
            fraction,
            current,
        )
        .ok_or(GuiInputError::Unavailable)?;
        self.move_thumb(host, context, target, value, delivery)
    }

    /// Move the slider thumb `target` names towards `value`; the World clamps
    /// it to the range and stops it at a range's other thumb.
    pub(super) fn move_thumb(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        value: f32,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::slider_thumb(input, target.focus_part(), value)?,
        )
    }

    /// `target` naming the thumb of a range slider that a pointer at `point`
    /// takes ([`range_thumb`]), the step part of a numeric text input it is
    /// over, or the colour control's surface under it; other controls name no
    /// part. A hover over a range's track names none and a press there takes
    /// the nearer thumb; a colour control's swatch names none.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pointed_part(
        &self,
        host: &HostRuntime,
        context: &GuiRoutingContext,
        query: ViewQueryTarget,
        mut target: Target,
        point: [f32; 2],
        press: bool,
    ) -> Result<Target, GuiInputError> {
        if let Some(number) = target
            .control
            .number
            .filter(|number| number.steps.is_some())
        {
            let local = local_point(host, query, &target, point)?;
            target.step = number.step_at(local);
            return Ok(target);
        }
        if let Some(layout) = target.control.color {
            let local = local_point(host, query, &target, point)?;
            target.focus_part = layout.part_at(local);
            return Ok(target);
        }
        let Some(slider) = target
            .control
            .slider
            .filter(|slider| slider.range.is_some())
        else {
            return Ok(target);
        };
        let local = local_point(host, query, &target, point)?;
        let focused = context
            .focus
            .as_ref()
            .filter(|focus| {
                focus.control.record.target == target.control.record.target
                    && focus.path == target.path
            })
            .map(Target::focus_part);
        target.focus_part = range_thumb(&slider, local, focused, press);
        Ok(target)
    }

    /// Turn a dial by its drag to `point` and return the drag that continues
    /// from there; the value snaps to the step as a rail drag's does.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn turn(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        drag: GuiDialDrag,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<GuiDialDrag, GuiInputError> {
        let slider = target.control.slider.ok_or(GuiInputError::Unavailable)?;
        let travel = slider.dial_travel.ok_or(GuiInputError::Unavailable)?;
        let local = local_point(host, query, target, point)?;
        let (fraction, drag) = drag
            .turned(local[1], travel)
            .ok_or(GuiInputError::Unavailable)?;
        let crate::systems::gui::presentation::GuiRoutingValue::Scalar(current) =
            target.control.record.value
        else {
            return Err(GuiInputError::Unavailable);
        };
        let value = crate::systems::gui::local::controls::slider::value_at(
            slider.min,
            slider.max,
            slider.step,
            fraction,
            current,
        )
        .ok_or(GuiInputError::Unavailable)?;
        self.action(
            host,
            context,
            target,
            GuiLocalAction::SetScalar(value),
            delivery,
        )?;
        Ok(drag)
    }

    /// Blur the context's keyboard target, committing a numeric text input's
    /// pending edit first, as blur does.
    pub(super) fn blur(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        if target.control.number.is_some() {
            let input = self.input(host, context, target, delivery)?;
            enqueue(
                host,
                context.queue_owner,
                target.control.record.target,
                GuiLocalCommand::number_commit(input)?,
            )?;
        }
        context.number_edit = false;
        self.action(host, context, target, GuiLocalAction::Blur, delivery)
    }

    /// Whether the focused numeric text input `focus` holds a pending edit
    /// for Escape to discard: the GUI System's record once its World applied
    /// this context's work, else whether this context routed an edit since.
    pub(super) fn number_edit_pending(
        &self,
        host: &mut HostRuntime,
        context: &GuiRoutingContext,
        focus: &Target,
    ) -> bool {
        if focus.control.number.is_none() {
            return false;
        }
        let target = focus.control.record.target;
        if context.queued_worlds.borrow().contains_key(&target.world) {
            return context.number_edit;
        }
        host.world_mut(target.world.id())
            .is_some_and(|world| world.gui_number_edit(target, context.session.id()))
    }

    /// Step a value control by `steps` of its step, or of its fine step when
    /// `fine`: arrow keys on the focused control and the wheel over it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn step_value(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        steps: f32,
        fine: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::slider_thumb_step(input, target.focus_part(), steps, fine)?,
        )
    }

    /// Set the channels of colour control part `part` at the pointer: the
    /// field's saturation and value, or a rail's hue or alpha.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pick(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        part: u32,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let layout = target.control.color.ok_or(GuiInputError::Unavailable)?;
        let local = local_point(host, query, target, point)?;
        self.color_channels(
            host,
            context,
            target,
            layout.channels_at(part, local),
            delivery,
        )
    }

    fn color_channels(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        channels: [Option<f32>; 4],
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::color_channels(input, channels)?,
        )
    }

    /// Step colour channel `channel` by `steps` of the colour step, or of the
    /// fine step when `fine`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn color_step(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        channel: usize,
        steps: f32,
        fine: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::color_step(input, channel, steps, fine)?,
        )
    }

    /// A key on the focused part of a colour control: on the field, Left
    /// and Right step the saturation and Down and Up the value, and Home and
    /// End take the saturation to its bounds; on a rail, Right and Up raise
    /// its channel and Left and Down lower it, and Home and End take it to
    /// its bounds. Shift steps finely. Other keys are left to the caller.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn color_key(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        key: GuiPhysicalKey,
        fine: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<Option<GuiRoutingDisposition>, GuiInputError> {
        use crate::systems::gui::local::controls::color::{SATURATION, VALUE};

        let part = target.focus_part();
        let channel = |vertical: bool| match (part, vertical) {
            (0, false) => SATURATION,
            (0, true) => VALUE,
            _ => color_rail_channel(part),
        };
        let routed = Some(GuiRoutingDisposition::Routed {
            target: target.control.record.target,
        });
        let (channel, steps) = match key {
            GuiPhysicalKey::Left => (channel(false), -1.0),
            GuiPhysicalKey::Right => (channel(false), 1.0),
            GuiPhysicalKey::Down => (channel(true), -1.0),
            GuiPhysicalKey::Up => (channel(true), 1.0),
            GuiPhysicalKey::Home | GuiPhysicalKey::End => {
                let mut channels = [None; 4];
                channels[channel(false)] = Some(f32::from(u8::from(key == GuiPhysicalKey::End)));
                self.color_channels(host, context, target, channels, delivery)?;
                return Ok(routed);
            }
            _ => return Ok(None),
        };
        self.color_step(host, context, target, channel, steps, fine, delivery)?;
        Ok(routed)
    }
}

/// The channel a colour control's rail `part` holds: hue for 1, alpha for 2.
pub(super) fn color_rail_channel(part: u32) -> usize {
    use crate::systems::gui::local::controls::color::{ALPHA, HUE};

    if part == 2 {
        ALPHA
    } else {
        HUE
    }
}

/// The thumb of a range a pointer at control-local `local` takes: the one
/// under it, or for a press on the track the nearer one. Where both thumbs
/// are under it, or a track press is as near to both, it takes the one that
/// can move when both sit at one end of the range, else the last active one,
/// the thumb `focused` while the slider holds focus, else the one on the
/// pointer's side: the upper towards the maximum. A hover over the track
/// takes none.
fn range_thumb(
    slider: &crate::systems::gui::presentation::GuiSliderGeometry,
    local: [f32; 2],
    focused: Option<u32>,
    press: bool,
) -> Option<u32> {
    let range = slider.range?;
    let axis = slider.axis;
    let over = range.thumb_rects.map(|rect| {
        (0..2).all(|axis| local[axis] >= rect[axis] && local[axis] < rect[axis] + rect[axis + 2])
    });
    match over {
        [true, false] => return Some(0),
        [false, true] => return Some(1),
        [false, false] if !press => return None,
        _ => {}
    }
    let centre = |rect: [f32; 4]| rect[axis] + rect[axis + 2] * 0.5;
    let distance = range
        .thumb_rects
        .map(|rect| (local[axis] - centre(rect)).abs());
    if over == [false, false] && distance[0] != distance[1] {
        return Some(u32::from(distance[1] < distance[0]));
    }
    let [lower, upper] = range.values;
    if lower == upper && lower >= slider.max {
        return Some(0);
    }
    if lower == upper && upper <= slider.min {
        return Some(1);
    }
    if let Some(part) = focused {
        return Some(part.min(1));
    }
    let towards_max = slider.thumb_centers[1] - slider.thumb_centers[0];
    Some(u32::from(
        (local[axis] - centre(range.thumb_rects[0])) * towards_max > 0.0,
    ))
}

/// The value steps one wheel event makes: one, along its larger component,
/// up increasing and down decreasing. A horizontal component counts as a
/// vertical one turned to the right, since browsers deliver Shift with the
/// wheel, the fine step, as horizontal movement. None for no movement.
pub(super) fn wheel_steps(delta: [f32; 2]) -> Option<f32> {
    let along = if delta[1].abs() >= delta[0].abs() {
        delta[1]
    } else {
        delta[0]
    };
    (along.is_finite() && along != 0.0).then(|| -along.signum())
}
