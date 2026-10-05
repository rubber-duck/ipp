//! Local operations at the mutation boundary.
//!
//! A `GuiAction` command and a routed operation are validated against the
//! control's identity and eligibility, compute the fields they write from
//! their current values, and write those fields through the ordinary write
//! path so the changed-fields record, animation write adoption and field
//! observation see them like any other write. A routed operation additionally
//! settles its delivery ticket; a command's result is its batch outcome.

use super::command::GuiLocalOperation;
use super::control::{
    GuiControl, ancestry, component_incarnation, eligibility, entity_control, focus_parts,
    focusable,
};
use super::group::{GuiGroupTree, selection_write};
use super::system_state::live;
use super::*;
use crate::services::gui_input::GuiInputError;
use crate::systems::gui::GuiSystem;
use crate::systems::{SystemCommandContext, SystemOperationContext, SystemRuntimeAccess};
use crate::{Command, ComponentValue, EntityId, EntityRef, ErrorReason, FieldValue, FieldWrite};
use std::sync::Arc;

/// Outcome of a validated routed operation: its momentary kind, if any, the
/// field writes that carry its result, and the scroll delta it consumed.
struct GuiActionOutcome {
    kind: Option<GuiLocalEffectKind>,
    writes: Vec<Command>,
    consumed: [f32; 2],
}

/// The field writes of a value action on one control component, computed from
/// its current value; unchanged fields are not written.
struct GuiValueWrites {
    fields: Vec<FieldWrite>,
    consumed: [f32; 2],
}

impl GuiValueWrites {
    fn new(fields: Vec<FieldWrite>) -> Self {
        Self {
            fields,
            consumed: [0.0; 2],
        }
    }

    fn commands(self, entity: EntityId, component: u16) -> GuiActionOutcome {
        GuiActionOutcome {
            kind: None,
            writes: self
                .fields
                .into_iter()
                .map(|field| Command::SetField {
                    entity: EntityRef::Handle(entity),
                    component,
                    field,
                })
                .collect(),
            consumed: self.consumed,
        }
    }
}

fn field(offset: usize, value: FieldValue) -> FieldWrite {
    FieldWrite {
        offset: offset as u32,
        value,
    }
}

fn set_field(entity: EntityId, component: u16, offset: usize, value: FieldValue) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component,
        field: field(offset, value),
    }
}

pub(super) fn write_error(reason: ErrorReason) -> GuiInputError {
    match reason {
        ErrorReason::Capacity => GuiInputError::Capacity,
        ErrorReason::InvalidEntity | ErrorReason::MissingComponent => {
            GuiInputError::Local(GuiLocalActionError::StaleTarget)
        }
        _ => GuiInputError::Local(GuiLocalActionError::InvalidValue),
    }
}

impl GuiSystem {
    pub(in crate::world::systems::gui) fn local_command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        command: &GuiLocalCommand,
    ) -> Result<(), ErrorReason> {
        let previous_revision = self.local.presentation_revision;
        let previous_focus = self.local.motion_focus();
        let previous_active = self.local.active_entities();
        let result = self.execute(context, command);
        if previous_revision != self.local.presentation_revision {
            self.motion.dirty_entity(command.input.target().entity);
            if previous_focus != self.local.motion_focus()
                && let Some(entity) = previous_focus
            {
                self.motion.dirty_entity(entity);
            }

            // An item that stops being active loses its hover paint.
            for entity in previous_active {
                self.motion.dirty_entity(entity);
            }
        }
        if let Err(error) = result {
            command.input.reject(error);
        }
        result.map_err(|error| match error {
            GuiInputError::Capacity => ErrorReason::Capacity,
            GuiInputError::Local(GuiLocalActionError::StaleTarget) => ErrorReason::InvalidEntity,
            _ => ErrorReason::InvalidValue,
        })
    }

    fn execute(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        command: &GuiLocalCommand,
    ) -> Result<(), GuiInputError> {
        let tick = {
            let view = context.host_ingress().ok_or(GuiInputError::Unavailable)?;
            command.input.validate(&view)?;
            view.world(command.input.target().world)
                .ok_or(GuiInputError::Unavailable)?
                .next_tick()
        };
        let control = self
            .local
            .validate(&context.world, command.input.target())
            .map_err(GuiInputError::Local)?;
        let entity = control.target.entity;
        let ancestry = ancestry(&context.world.world.state, entity);
        match &command.operation {
            GuiLocalOperation::Text {
                fence,
                edit,
            } => return self.edit_text(context, command, control, *fence, edit, tick, ancestry),
            GuiLocalOperation::TextCaret {
                offset,
                extend,
            } => {
                let native = self
                    .local
                    .native_text(control.target)
                    .ok_or(GuiInputError::Unavailable)?;
                let caret = super::text_edit::snap_to_boundary(&native.text, *offset);
                let anchor = if *extend {
                    native.selection[0]
                } else {
                    caret
                };
                let fence = native.fence;
                return self.edit_text(
                    context,
                    command,
                    control,
                    fence,
                    &GuiTextEdit::Selection([anchor, caret]),
                    tick,
                    ancestry,
                );
            }
            GuiLocalOperation::Interaction {
                lease,
                update,
                part,
            } => {
                let group = GuiGroupTree::committed(context.world.world).active_group(control);
                return self.local.interact(
                    command, control, lease, *update, *part, group, tick, ancestry,
                );
            }
            GuiLocalOperation::ActiveItem => {
                let group = GuiGroupTree::committed(context.world.world)
                    .active_group(control)
                    .ok_or(GuiInputError::Local(GuiLocalActionError::UnsupportedAction))?;
                return self.local.make_active(command, control, group);
            }
            GuiLocalOperation::Number(operation) => {
                return self
                    .number_operation(context, command, control, *operation, tick, ancestry);
            }
            // Enter on a numeric input commits its edit and submits the number.
            GuiLocalOperation::Action(GuiLocalAction::Submit)
                if context
                    .world
                    .world
                    .components
                    .gui_text_input(entity.index() as usize)
                    .is_some_and(|input| input.numeric) =>
            {
                return self.number_operation(
                    context,
                    command,
                    control,
                    super::number::GuiNumberOperation::Commit {
                        submit: true,
                    },
                    tick,
                    ancestry,
                );
            }
            _ => {}
        }
        if matches!(
            command.operation,
            GuiLocalOperation::Action(GuiLocalAction::Submit)
        ) && self
            .local
            .native_text(control.target)
            .is_some_and(|native| native.composition.is_some())
        {
            return Err(GuiInputError::Local(GuiLocalActionError::InvalidValue));
        }
        let owner = command.input.session_lifetime();
        let delegated = match &command.operation {
            GuiLocalOperation::Scroll {
                chain,
                ordinal,
            } => Some(GuiLocalOperation::Action(GuiLocalAction::ScrollBy(
                chain.enter(*ordinal, command.input.key())?,
            ))),
            GuiLocalOperation::Focus {
                part,
                ..
            } => Some(GuiLocalOperation::Action(GuiLocalAction::Focus(*part))),
            _ => None,
        };
        let focus_visible = match command.operation {
            GuiLocalOperation::Focus {
                visible,
                ..
            } => visible || control.kind == GuiControlKind::TextInput,
            _ => true,
        };
        let GuiActionOutcome {
            mut kind,
            writes,
            consumed,
        } = self
            .local
            .prepare_action(
                &context.world,
                control,
                delegated.as_ref().unwrap_or(&command.operation),
                &owner,
            )
            .map_err(GuiInputError::Local)?;
        if let Some(GuiLocalEffectKind::FocusChanged {
            focused: true,
            changed,
            ..
        }) = &mut kind
        {
            *changed |= self.local.focus_visible != focus_visible;
        }
        let focus_change = match &kind {
            Some(GuiLocalEffectKind::FocusChanged {
                focused,
                changed: true,
                part,
            }) => Some((*focused, *part)),
            _ => None,
        };

        // Routed focus keeps focus that a command set without an owner, so it
        // outlives this session; any other routed focus belongs to it.
        let commanded = self
            .local
            .focus
            .as_ref()
            .is_some_and(|(focused, owner)| *focused == control.target && owner.is_none());
        let focus_owner = (!commanded).then(|| owner.clone());

        // Focus that moves to this control reveals it in its scroll views.
        let reveal = focus_change.is_some_and(|(focused, _)| focused)
            && self
                .local
                .focus
                .as_ref()
                .is_none_or(|(focused, _)| *focused != control.target);

        // A focus change replaces the native record and drops any provisional
        // run it displayed. Focusing a text input that is already focused
        // installs this session's record when it does not hold one, as when
        // the input context adopts focus a command set.
        let installs_native = matches!(
            kind,
            Some(GuiLocalEffectKind::FocusChanged {
                focused: true,
                ..
            })
        ) && control.kind == GuiControlKind::TextInput
            && !self.local.holds_native_text(control.target, &owner);
        let native_changed = focus_change.is_some() || installs_native;
        let text_changed = native_changed && self.local.composing();
        let presentation_revision = self
            .local
            .presentation_revision
            .checked_add(u64::from(native_changed))
            .ok_or(GuiInputError::Capacity)?;
        let text_revision = self
            .local
            .text_revision
            .checked_add(u64::from(text_changed))
            .ok_or(GuiInputError::Capacity)?;
        let native_generation = self
            .local
            .native_generation
            .checked_add(u64::from(native_changed))
            .ok_or(GuiInputError::Capacity)?;
        let native_reset = native_changed.then(|| {
            focus_change
                .is_none_or(|(focused, _)| focused)
                .then(|| {
                    context
                        .world
                        .world
                        .components
                        .gui_text_input(entity.index() as usize)
                        .map(|input| {
                            super::text::GuiNativeText::new(
                                control.target,
                                owner.clone(),
                                input,
                                native_generation,
                            )
                        })
                })
                .flatten()
        });
        // A numeric edit another input session's focus takes over ends
        // without a commit; this session's own blur committed its edit first.
        let taken = native_changed
            .then(|| {
                self.local
                    .native_text
                    .as_ref()
                    .filter(|native| native.owner.id() != owner.id())
                    .and_then(super::text::GuiNativeText::pending_number_edit)
            })
            .flatten();
        if taken.is_some() {
            self.local
                .number_discards
                .try_reserve(1)
                .map_err(|_| GuiInputError::Capacity)?;
        }
        let effect = kind
            .map(|kind| {
                Ok::<_, GuiInputError>(GuiLocalEffect {
                    id: self
                        .local
                        .observations
                        .candidate_id(control.target.world, &kind)?,
                    target: control.target,
                    source: command.input.effect_source(),
                    tick,
                    ancestry,
                    kind,
                })
            })
            .transpose()?;
        let prepared = match effect {
            Some(effect) => command.input.prepare_effect(effect)?,
            None => command.input.prepare_write(!writes.is_empty())?,
        };
        if !writes.is_empty()
            && let Err(reason) = context.world.apply_authored_commands(Some(self), &writes)
        {
            let error = write_error(reason);
            command.input.reject(error);
            return Err(error);
        }
        let local = &mut self.local;
        prepared.commit_with_publication(
            || {
                if let Some((focused, part)) = focus_change {
                    local.focus = focused.then_some((control.target, focus_owner));
                    local.focus_part = if focused {
                        part
                    } else {
                        0
                    };
                    local.focus_visible = focused && focus_visible;
                }
                if native_changed {
                    local.presentation_revision = presentation_revision;
                    local.text_revision = text_revision;
                }
                if reveal {
                    local.reveal = Some(entity);
                }
                if let Some(native) = native_reset {
                    local.number_discards.extend(taken);
                    local.native_text = native;
                    local.native_generation = native_generation;
                }
                if let GuiLocalOperation::Scroll {
                    chain,
                    ..
                } = &command.operation
                {
                    chain.commit(consumed);
                    command.physical_committed.set(true);
                }
            },
            |effect| local.observations.publish(effect),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn edit_text(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        command: &GuiLocalCommand,
        control: GuiControl,
        fence: GuiTextFence,
        edit: &GuiTextEdit,
        tick: u64,
        ancestry: Arc<[EntityId]>,
    ) -> Result<(), GuiInputError> {
        let input = context
            .world
            .world
            .components
            .gui_text_input(control.target.entity.index() as usize)
            .cloned()
            .ok_or(GuiInputError::Local(GuiLocalActionError::UnsupportedAction))?;

        // A numeric input edits its native text, which no field holds.
        let text = if input.numeric {
            self.local
                .native_text(control.target)
                .map(|native| native.text.clone())
                .ok_or(GuiInputError::Unavailable)?
        } else {
            input.text.clone()
        };
        let super::text::GuiPreparedTextEdit {
            mut state,
            mut effect,
            changed_text,
            mut changed_display,
        } = self
            .local
            .prepare_text_edit(command, control, text, fence, edit, tick, ancestry)?;

        // Enter commits a numeric edit: its number, then the edit shows it
        // formatted and submits that; text that does not parse is rejected
        // and stays.
        let mut value = None;
        let mut basis = None;
        if input.numeric
            && matches!(edit, GuiTextEdit::Submit)
            && let Some(effect) = effect.as_mut()
        {
            match input.committed(&state.text) {
                Some(committed) => {
                    let formatted = super::number::format_number(committed, input.precision);
                    changed_display |= *state.text != *formatted;
                    state = super::text::GuiNativeText::state(
                        control.target,
                        formatted.clone(),
                        state.fence.generation,
                        input.masked,
                    );
                    effect.kind = GuiLocalEffectKind::Submitted(formatted.clone());
                    value = (committed != input.value).then_some(committed);
                    basis = Some(formatted);
                }
                None => effect.kind = GuiLocalEffectKind::Rejected(state.text.clone()),
            }
        }
        let presentation_revision = self
            .local
            .presentation_revision
            .checked_add(1)
            .ok_or(GuiInputError::Capacity)?;
        let text_revision = self
            .local
            .text_revision
            .checked_add(u64::from(changed_display))
            .ok_or(GuiInputError::Capacity)?;
        let generation = state.fence.generation;
        let written = match value {
            Some(value) => Some(set_field(
                control.target.entity,
                ComponentValue::GUI_TEXT_INPUT,
                std::mem::offset_of!(GuiTextInput, value),
                FieldValue::F32(value),
            )),
            None if changed_text && !input.numeric => Some(set_field(
                control.target.entity,
                ComponentValue::GUI_TEXT_INPUT,
                std::mem::offset_of!(GuiTextInput, text),
                FieldValue::String(state.text.clone()),
            )),
            None => None,
        };
        let prepared = command.input.prepare_native(state, effect)?;
        if let Some(written) = written {
            self.local.own_text_write = true;
            let result = context
                .world
                .apply_authored_commands(Some(self), &[written]);
            self.local.own_text_write = false;
            if let Err(reason) = result {
                let error = write_error(reason);
                command.input.reject(error);
                return Err(error);
            }
        }
        let local = &mut self.local;
        prepared.commit(
            |state| {
                local.native_generation = generation;
                let native = local.native_text.as_mut().expect("validated native owner");
                native.state = state.clone();
                if basis.is_some() {
                    native.basis = basis;
                }

                // The field write's own commit may already have advanced these
                // dirty counters; they only move forward.
                local.presentation_revision =
                    local.presentation_revision.max(presentation_revision);
                local.text_revision = local.text_revision.max(text_revision);
            },
            |effect| local.observations.publish(effect),
        )
    }
}

impl GuiLocalState {
    pub(in crate::world::systems::gui) fn motion_focus(&self) -> Option<EntityId> {
        self.focus.as_ref().map(|(target, _)| target.entity)
    }

    /// The part `target` holds focus on, or 0 when it holds none.
    fn held_part(&self, target: GuiEntityTarget) -> u32 {
        match &self.focus {
            Some((focused, owner)) if *focused == target && live(owner) => self.focus_part,
            _ => 0,
        }
    }

    /// Check the exact control lifetime and its eligibility fields.
    fn validate(
        &self,
        world: &SystemRuntimeAccess<'_>,
        target: GuiEntityTarget,
    ) -> Result<GuiControl, GuiLocalActionError> {
        if target.world
            != (crate::WorldRef {
                id: world.world.id,
                incarnation: world.world.identity,
            })
        {
            return Err(GuiLocalActionError::StaleTarget);
        }
        let control = entity_control(world.world, &world.world.state, target.entity)
            .filter(|control| control.target == target)
            .ok_or(GuiLocalActionError::StaleTarget)?;
        if !eligibility(world.world, target.entity).eligible() {
            return Err(GuiLocalActionError::Unavailable);
        }
        Ok(control)
    }

    /// Compute a routed operation's outcome from the current fields,
    /// validating the resulting values before anything is written.
    fn prepare_action(
        &self,
        world: &SystemRuntimeAccess<'_>,
        control: GuiControl,
        operation: &GuiLocalOperation,
        owner: &crate::services::gui_input::GuiInputSession,
    ) -> Result<GuiActionOutcome, GuiLocalActionError> {
        let entity = control.target.entity;
        let component = control.target.component;
        let value = world
            .world
            .components
            .get(component, entity.index() as usize)
            .ok_or(GuiLocalActionError::StaleTarget)?;
        let action = match operation {
            GuiLocalOperation::SliderStep {
                steps,
                fine,
                part,
            } => {
                let ComponentValue::GuiSlider(slider) = &value else {
                    return Err(GuiLocalActionError::UnsupportedAction);
                };
                let step = if *fine && slider.fine_step > 0.0 {
                    slider.fine_step
                } else {
                    slider.step
                };
                let current = slider.thumb_value(*part);
                let scalar = super::slider::nudge(slider.min, slider.max, step, current, *steps)
                    .unwrap_or(current);
                return thumb_writes(slider, *part, scalar)
                    .map(|writes| writes.commands(entity, component));
            }
            GuiLocalOperation::SliderThumb {
                part,
                value: requested,
            } => {
                let ComponentValue::GuiSlider(slider) = &value else {
                    return Err(GuiLocalActionError::UnsupportedAction);
                };
                return thumb_writes(slider, *part, *requested)
                    .map(|writes| writes.commands(entity, component));
            }
            GuiLocalOperation::ColorChannels {
                channels,
            } => {
                let ComponentValue::GuiColor(color) = &value else {
                    return Err(GuiLocalActionError::UnsupportedAction);
                };
                let mut target = color.channels();
                for (channel, requested) in target.iter_mut().zip(channels) {
                    if let Some(requested) = requested {
                        *channel = requested.clamp(0.0, 1.0);
                    }
                }
                return color_writes(color, target)
                    .map(|writes| writes.commands(entity, component));
            }
            GuiLocalOperation::ColorStep {
                channel,
                steps,
                fine,
            } => {
                let ComponentValue::GuiColor(color) = &value else {
                    return Err(GuiLocalActionError::UnsupportedAction);
                };
                let step = if *fine {
                    super::color::COLOR_FINE_STEP
                } else {
                    super::color::COLOR_STEP
                };
                let mut target = color.channels();
                target[*channel] = super::color::nudge(target[*channel], *steps, step);
                return color_writes(color, target)
                    .map(|writes| writes.commands(entity, component));
            }
            GuiLocalOperation::ScrollAxis {
                axis,
                offset,
            } => {
                let scroll = super::scroll::GuiScrollFields::read(&value)?;
                let mut requested = scroll.offset;
                requested[*axis] = *offset;
                return scroll
                    .scroll_to(requested)
                    .map(|writes| writes.commands(entity, component));
            }
            GuiLocalOperation::Context {
                point,
            } => {
                return Ok(GuiActionOutcome {
                    kind: Some(GuiLocalEffectKind::ContextRequested {
                        point: *point,
                    }),
                    writes: Vec::new(),
                    consumed: [0.0; 2],
                });
            }
            GuiLocalOperation::Select => {
                let tree = GuiGroupTree::committed(world.world);
                tree.selecting_group(control)
                    .ok_or(GuiLocalActionError::UnsupportedAction)?;
                return Ok(GuiValueWrites::new(
                    selection_write(tree, control).into_iter().collect(),
                )
                .commands(entity, component));
            }
            GuiLocalOperation::Action(action) => action,
            GuiLocalOperation::Text {
                ..
            }
            | GuiLocalOperation::TextCaret {
                ..
            }
            | GuiLocalOperation::Interaction {
                ..
            }
            | GuiLocalOperation::Scroll {
                ..
            }
            | GuiLocalOperation::Focus {
                ..
            }
            | GuiLocalOperation::ActiveItem
            | GuiLocalOperation::Number(_) => {
                unreachable!("delegated or separately prepared operation")
            }
        };
        let momentary = |kind| GuiActionOutcome {
            kind: Some(kind),
            writes: Vec::new(),
            consumed: [0.0; 2],
        };
        match (action, &value) {
            // Pressing an item of a group that selects selects it.
            (GuiLocalAction::Press, ComponentValue::GuiButton(_)) => Ok(GuiActionOutcome {
                writes: selection_write(GuiGroupTree::committed(world.world), control)
                    .map(|field| Command::SetField {
                        entity: EntityRef::Handle(entity),
                        component,
                        field,
                    })
                    .into_iter()
                    .collect(),
                ..momentary(GuiLocalEffectKind::Pressed)
            }),
            (GuiLocalAction::Submit, ComponentValue::GuiTextInput(input)) => {
                Ok(momentary(GuiLocalEffectKind::Submitted(input.text.clone())))
            }
            (GuiLocalAction::Focus(part), _)
                if !focusable(world.world, entity)
                    || *part >= focus_parts(world.world, control) =>
            {
                Err(GuiLocalActionError::UnsupportedAction)
            }
            (GuiLocalAction::Focus(_) | GuiLocalAction::Blur, _) => {
                let current = self.focus.as_ref().filter(|(_, owner)| live(owner));
                let target_focused = current.is_some_and(|(target, _)| *target == control.target);
                // Focus set by a command has no owner and is never foreign;
                // focusing it again keeps it unowned, so that is no change.
                let owner_id = current.and_then(|(_, current)| current.as_ref().map(|c| c.id()));
                let foreign = owner_id.is_some_and(|id| id != owner.id());
                let held = if target_focused {
                    self.focus_part
                } else {
                    0
                };
                let (focused, part) = match action {
                    GuiLocalAction::Focus(part) => (true, *part),
                    _ => (false, held),
                };
                if !focused && target_focused && foreign {
                    return Err(GuiLocalActionError::Unavailable);
                }
                Ok(momentary(GuiLocalEffectKind::FocusChanged {
                    focused,
                    changed: if focused {
                        !target_focused || foreign || part != held
                    } else {
                        target_focused
                    },
                    part,
                }))
            }
            _ => value_writes(action, &value).map(|writes| writes.commands(entity, component)),
        }
    }
}

impl GuiSystem {
    /// Apply a [`Command::GuiAction`] at its mutation boundary.
    ///
    /// Validation runs in the documented order, and nothing changes before it
    /// completes. Value actions write the staged control component; press,
    /// submit and focus changes publish a momentary effect. A press also
    /// selects an item of a group that selects.
    pub(in crate::world::systems::gui) fn apply_action(
        &mut self,
        context: &mut SystemOperationContext<'_>,
        target: &crate::GuiActionTarget,
        action: &GuiLocalAction,
    ) -> Result<(), ErrorReason> {
        let stale = ErrorReason::StaleTarget;
        let entity = context.resolve_entity(&target.entity).map_err(|_| stale)?;
        let component = target.component;
        GuiControlKind::of_component(component).ok_or(stale)?;
        if component_incarnation(&context.staged.entities_state, entity, component)
            != Some(target.incarnation)
        {
            return Err(stale);
        }
        let control = GuiEntityTarget {
            world: crate::WorldRef {
                id: context.world_data.id,
                incarnation: context.world_data.identity,
            },
            entity,
            component,
            incarnation: target.incarnation,
        };
        if !eligibility(context.world_data, entity).eligible() {
            return Err(ErrorReason::Unavailable);
        }
        let value = context
            .staged
            .component(&context.world_data.components, entity, component)
            .map_err(|_| stale)?;

        // The staged value decides the parts, so a batch may make a range and
        // focus its upper thumb.
        let parts = match &value {
            ComponentValue::GuiSlider(slider) => slider.thumbs(),
            ComponentValue::GuiColor(color) => color.parts(),
            _ => 1,
        };
        let kind = match (action, &value) {
            (GuiLocalAction::Press, ComponentValue::GuiButton(_)) => GuiLocalEffectKind::Pressed,
            (GuiLocalAction::Submit, ComponentValue::GuiTextInput(input)) if input.numeric => {
                GuiLocalEffectKind::Submitted(input.formatted())
            }
            (GuiLocalAction::Submit, ComponentValue::GuiTextInput(input)) => {
                GuiLocalEffectKind::Submitted(input.text.clone())
            }
            (GuiLocalAction::Focus(part), _)
                if !focusable(context.world_data, entity) || *part >= parts =>
            {
                return Err(ErrorReason::UnsupportedAction);
            }
            (GuiLocalAction::Focus(part), _) => GuiLocalEffectKind::FocusChanged {
                focused: true,
                changed: true,
                part: *part,
            },
            (GuiLocalAction::Blur, _) => GuiLocalEffectKind::FocusChanged {
                focused: false,
                changed: true,
                part: self.local.held_part(control),
            },
            _ => {
                let writes = value_writes(action, &value).map_err(action_error)?;
                if !writes.fields.is_empty() {
                    context
                        .staged
                        .write_component_fields(
                            &context.world_data.components,
                            entity,
                            component,
                            &writes.fields,
                        )
                        .map_err(|reason| match reason {
                            ErrorReason::Capacity => ErrorReason::Capacity,
                            _ => ErrorReason::InvalidValue,
                        })?;
                }
                return Ok(());
            }
        };

        // Pressing an item of a group that selects selects it.
        if kind == GuiLocalEffectKind::Pressed {
            let tree = GuiGroupTree {
                world: context.world_data,
                state: &context.staged.entities_state,
            };
            let button = GuiControl {
                target: control,
                kind: GuiControlKind::Button,
            };
            if let Some(field) = selection_write(tree, button) {
                context
                    .staged
                    .write_component_fields(
                        &context.world_data.components,
                        entity,
                        component,
                        &[field],
                    )
                    .map_err(|reason| match reason {
                        ErrorReason::Capacity => ErrorReason::Capacity,
                        _ => ErrorReason::InvalidValue,
                    })?;
            }
        }

        // Focus leaving a numeric input commits its pending edit first.
        if let GuiLocalEffectKind::FocusChanged {
            focused,
            ..
        } = kind
        {
            self.commit_ending_number_edit(context, control, focused)?;
        }

        // The effect is published at this boundary, in ingress order with
        // observation cuts, with the tick of the frame applying the batch.
        let effect = GuiLocalEffect {
            id: self
                .local
                .observations
                .candidate_id(control.world, &kind)
                .map_err(|_| ErrorReason::Capacity)?,
            target: control,
            source: GuiLocalEffectSource::Semantic,
            tick: context
                .world_data
                .tick
                .checked_add(1)
                .ok_or(ErrorReason::Capacity)?,
            ancestry: ancestry(&context.staged.entities_state, entity),
            kind,
        };
        if let GuiLocalEffectKind::FocusChanged {
            focused,
            part,
            ..
        } = effect.kind
        {
            let previous = self.local.motion_focus();
            if !self.local.set_action_focus(control, focused, part)? {
                return Ok(());
            }
            self.motion.dirty_entity(entity);
            if let Some(previous) = previous.filter(|previous| *previous != entity) {
                self.motion.dirty_entity(previous);
            }
        }
        self.local.observations.publish(&effect);
        Ok(())
    }
}

fn action_error(error: GuiLocalActionError) -> ErrorReason {
    match error {
        GuiLocalActionError::StaleTarget => ErrorReason::StaleTarget,
        GuiLocalActionError::Unavailable => ErrorReason::Unavailable,
        GuiLocalActionError::UnsupportedAction => ErrorReason::UnsupportedAction,
        GuiLocalActionError::InvalidValue => ErrorReason::InvalidValue,
    }
}

impl GuiLocalState {
    /// Move logical focus for a `GuiAction`: focus sets it on `target` without
    /// an owning input session, blur clears it only when `target` holds it.
    /// Returns whether focus changed.
    fn set_action_focus(
        &mut self,
        target: GuiEntityTarget,
        focused: bool,
        part: u32,
    ) -> Result<bool, ErrorReason> {
        let target_focused = self
            .focus
            .as_ref()
            .is_some_and(|(current, owner)| *current == target && live(owner));
        if focused == target_focused && (!focused || part == self.focus_part) {
            return Ok(false);
        }
        // Every counter is checked before any state changes.
        let composing = self.composing();
        let native_generation = self.native_generation.checked_add(1);
        let presentation_revision = self.presentation_revision.checked_add(1);
        let text_revision = self.text_revision.checked_add(u64::from(composing));
        let (Some(native_generation), Some(presentation_revision), Some(text_revision)) =
            (native_generation, presentation_revision, text_revision)
        else {
            return Err(ErrorReason::Capacity);
        };
        self.focus = focused.then_some((target, None));
        self.focus_part = if focused {
            part
        } else {
            0
        };
        self.focus_visible = focused;
        if focused {
            self.reveal = Some(target.entity);
        }
        self.native_text = None;
        self.native_generation = native_generation;
        self.presentation_revision = presentation_revision;
        self.text_revision = text_revision;
        Ok(true)
    }
}

/// The write moving thumb `part` of `slider` towards `requested`: clamped to
/// the range and, on a range, stopped at the other thumb. A thumb the slider
/// does not have is unsupported; an unchanged value writes nothing.
fn thumb_writes(
    slider: &GuiSlider,
    part: u32,
    requested: f32,
) -> Result<GuiValueWrites, GuiLocalActionError> {
    if part >= slider.thumbs() {
        return Err(GuiLocalActionError::UnsupportedAction);
    }
    if !requested.is_finite() {
        return Err(GuiLocalActionError::InvalidValue);
    }
    let scalar = slider.thumb_clamp(part, requested);
    let offset = slider.thumb_field(part);
    let mut candidate = *slider;
    if part == 1 {
        candidate.upper = scalar;
    } else {
        candidate.value = scalar;
    }
    crate::components::schema::ComponentLifecycle::validate(&candidate)
        .map_err(|_| GuiLocalActionError::InvalidValue)?;
    Ok(GuiValueWrites::new(if scalar == slider.thumb_value(part) {
        Vec::new()
    } else {
        vec![field(offset, FieldValue::F32(scalar))]
    }))
}

/// The writes moving `color` to the channels `target` in field order; unchanged
/// channels are not written.
fn color_writes(color: &GuiColor, target: [f32; 4]) -> Result<GuiValueWrites, GuiLocalActionError> {
    let candidate = GuiColor {
        hue: target[0],
        saturation: target[1],
        value: target[2],
        alpha: target[3],
        ..*color
    };
    crate::components::schema::ComponentLifecycle::validate(&candidate)
        .map_err(|_| GuiLocalActionError::InvalidValue)?;
    Ok(GuiValueWrites::new(
        color
            .channels()
            .into_iter()
            .zip(target)
            .enumerate()
            .filter(|(_, (current, next))| current != next)
            .map(|(channel, (_, next))| {
                field(GuiColor::channel_field(channel), FieldValue::F32(next))
            })
            .collect(),
    ))
}

/// Compute a value action's field writes from the control's current value,
/// checking the action against the control role before its value.
fn value_writes(
    action: &GuiLocalAction,
    value: &ComponentValue,
) -> Result<GuiValueWrites, GuiLocalActionError> {
    match (action, value) {
        (GuiLocalAction::Toggle, ComponentValue::GuiCheckbox(checkbox)) => {
            Ok(GuiValueWrites::new(vec![field(
                std::mem::offset_of!(GuiCheckbox, checked),
                FieldValue::Bool(!checkbox.checked),
            )]))
        }
        (GuiLocalAction::SetScalar(scalar), ComponentValue::GuiSlider(slider)) => {
            let scalar = *scalar;
            if !scalar.is_finite() || scalar < slider.min || scalar > slider.max {
                return Err(GuiLocalActionError::InvalidValue);
            }
            let candidate = GuiSlider {
                value: scalar,
                ..*slider
            };
            crate::components::schema::ComponentLifecycle::validate(&candidate)
                .map_err(|_| GuiLocalActionError::InvalidValue)?;
            Ok(GuiValueWrites::new(if scalar == slider.value {
                Vec::new()
            } else {
                vec![field(
                    std::mem::offset_of!(GuiSlider, value),
                    FieldValue::F32(scalar),
                )]
            }))
        }
        // A semantic colour outside the channels' range is refused, as a
        // scalar outside the slider's range is.
        (GuiLocalAction::SetColor(channels), ComponentValue::GuiColor(color)) => {
            color_writes(color, *channels)
        }
        // A numeric input's semantic value is its number, which the range
        // bounds as a slider's does.
        (GuiLocalAction::SetScalar(scalar), ComponentValue::GuiTextInput(input))
            if input.numeric =>
        {
            let scalar = *scalar;
            if !scalar.is_finite() || scalar < input.min || scalar > input.max {
                return Err(GuiLocalActionError::InvalidValue);
            }
            Ok(GuiValueWrites::new(if scalar == input.value {
                Vec::new()
            } else {
                vec![field(
                    std::mem::offset_of!(GuiTextInput, value),
                    FieldValue::F32(scalar),
                )]
            }))
        }
        (GuiLocalAction::SetText(text), ComponentValue::GuiTextInput(input)) if !input.numeric => {
            super::component::single_line_text(text)
                .map_err(|_| GuiLocalActionError::InvalidValue)?;
            Ok(GuiValueWrites::new(if **text == *input.text {
                Vec::new()
            } else {
                vec![field(
                    std::mem::offset_of!(GuiTextInput, text),
                    FieldValue::String(text.clone()),
                )]
            }))
        }
        (
            GuiLocalAction::ScrollTo(_)
            | GuiLocalAction::ScrollBy(_)
            | GuiLocalAction::ScrollToIndex {
                ..
            },
            ComponentValue::GuiScrollView(_) | ComponentValue::GuiVirtualList(_),
        ) => {
            let scroll = super::scroll::GuiScrollFields::read(value)?;
            match action {
                GuiLocalAction::ScrollTo(offset) => scroll.scroll_to(*offset),
                GuiLocalAction::ScrollBy(delta) => scroll.scroll_to(std::array::from_fn(|axis| {
                    scroll.offset[axis] + delta[axis]
                })),
                GuiLocalAction::ScrollToIndex {
                    index,
                    offset,
                } => scroll.scroll_to_index(*index, *offset),
                _ => unreachable!("scroll action"),
            }
        }
        _ => Err(GuiLocalActionError::UnsupportedAction),
    }
}

impl super::scroll::GuiScrollFields {
    /// Clamp a requested offset to the capacity of the last layout.
    fn scroll_to(&self, requested: [f32; 2]) -> Result<GuiValueWrites, GuiLocalActionError> {
        if !requested.into_iter().all(f32::is_finite) {
            return Err(GuiLocalActionError::InvalidValue);
        }
        let offset = self.clamp(requested);
        let offsets = if self.anchor.is_none() {
            [
                std::mem::offset_of!(GuiScrollView, offset_x),
                std::mem::offset_of!(GuiScrollView, offset_y),
            ]
        } else {
            [
                std::mem::offset_of!(GuiVirtualList, offset_x),
                std::mem::offset_of!(GuiVirtualList, offset_y),
            ]
        };
        Ok(GuiValueWrites {
            fields: (0..2)
                .filter(|&axis| offset[axis] != self.offset[axis])
                .map(|axis| field(offsets[axis], FieldValue::F32(offset[axis])))
                .collect(),
            consumed: std::array::from_fn(|axis| offset[axis] - self.offset[axis]),
        })
    }

    /// Anchor an existing virtual item; layout derives the offset from it.
    fn scroll_to_index(
        &self,
        index: u32,
        offset: f32,
    ) -> Result<GuiValueWrites, GuiLocalActionError> {
        let Some((anchor_index, anchor_offset, item_count)) = self.anchor else {
            return Err(GuiLocalActionError::UnsupportedAction);
        };
        if index >= item_count || !offset.is_finite() || offset < 0.0 {
            return Err(GuiLocalActionError::InvalidValue);
        }
        let mut fields = Vec::new();
        if index != anchor_index {
            fields.push(field(
                std::mem::offset_of!(GuiVirtualList, anchor_index),
                FieldValue::U32(index),
            ));
        }
        if offset != anchor_offset {
            fields.push(field(
                std::mem::offset_of!(GuiVirtualList, anchor_offset),
                FieldValue::F32(offset),
            ));
        }
        Ok(GuiValueWrites::new(fields))
    }
}
