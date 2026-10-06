//! A text input that holds a number.
//!
//! A `numeric` [`GuiTextInput`] commits its number to `value`, which the
//! component store holds like any other value; it shows that number formatted
//! with `precision` decimals ([`format_number`]). While the input holds focus,
//! the text being edited is the native record's text, GUI System state that
//! starts as the formatted number and is never written to a field. The record
//! keeps the formatted number it started from, its basis, so an edit is
//! pending while the two differ.
//!
//! # Commit, rejection and Escape
//!
//! Enter, and blur by any route that moves focus away while the control stays
//! eligible, commits a pending edit: the text is parsed ([`parse_number`]),
//! clamped to `min..=max` and written to `value`, and the edit shows the
//! formatted result. Typed numbers are not snapped to the step. Text that does
//! not parse leaves `value` unchanged and publishes a momentary
//! [`Rejected`](crate::systems::gui::local::GuiLocalEffectKind::Rejected) effect with that text; on
//! Enter the text stays for the user to correct, and on blur the edit ends.
//! Enter also publishes [`Submitted`](crate::systems::gui::local::GuiLocalEffectKind::Submitted)
//! with the formatted number once the edit commits. Escape discards a pending
//! edit and shows the formatted number again; the input router sends it
//! instead of closing an overlay or blurring while an edit is pending. Focus
//! lost because the control itself became ineligible, or its input session
//! ended, discards the edit.
//!
//! A write of `value` or `precision` by anyone else while the input is edited
//! replaces the edit with the new formatted number, as a client's write of a
//! text input's `text` replaces its native text; so does another input
//! session taking focus from it.
//!
//! An edit that ends without being committed or refused publishes a
//! momentary [`Discarded`](crate::systems::gui::local::GuiLocalEffectKind::Discarded) effect with
//! the discarded text, so a client clears what it showed for the edit, such as
//! a rejection's error: Escape's at its mutation boundary, and every other one
//! (ineligibility, a session ending, a replacing write or focus taken by
//! another session) at the next start or end of the World's evaluation, while
//! the control is still the one it edited. Neither a plain text input nor an
//! edit without changes publishes one.
//!
//! # Steps
//!
//! Up and Down step the number by `step`, or `fine_step` with Shift, and the
//! decrement and increment parts by `step`; every step first commits a pending
//! edit (a rejected edit is reported and discarded) and then moves from the
//! result, clamped to the range, and the edit shows the stepped number. A zero
//! step steps nothing. A step part whose direction has reached its bound, read
//! from the committed number, is disabled and its press does nothing; the other
//! part stays active.
//!
//! A press on a step part steps once at its mutation boundary and arms a held
//! repeat in GUI System state: while a pointer presses that part, the number
//! steps again after [`GUI_NUMBER_REPEAT_DELAY`] and then every
//! [`GUI_NUMBER_REPEAT_INTERVAL`] of the Host frame deltas the World receives,
//! never a client timer, so a paused Host holds it. The repeat stops when no
//! pointer presses the part (release or cancellation), at the bound, and when
//! the control stops being an eligible numeric input with step parts. Each
//! frame's steps are one write, so clients observe one value change per frame.

use super::identity::{GuiControl, ancestry, eligibility, entity_control};
use crate::services::gui_input::GuiInputError;
use crate::systems::gui::GuiSystem;
use crate::systems::gui::local::{
    GuiEntityTarget, GuiInteractionPart, GuiLocalActionError, GuiLocalCommand, GuiLocalEffect,
    GuiLocalEffectKind, GuiLocalEffectSource, GuiTextInput,
};
use crate::systems::gui::system_state::GuiLocalState;
use crate::systems::{SystemCommandContext, SystemOperationContext, SystemUpdateContext};
use crate::{Command, ComponentValue, EntityId, EntityRef, ErrorReason, FieldValue, FieldWrite};
use std::sync::Arc;

/// Host seconds a step part is held before it repeats.
pub(crate) const GUI_NUMBER_REPEAT_DELAY: f64 = 0.4;

/// Host seconds between the repeats of a held step part.
pub(crate) const GUI_NUMBER_REPEAT_INTERVAL: f64 = 0.08;

/// Seconds a repeat may fall short and still count as due: frame deltas do
/// not sum to the delays exactly.
const SLACK: f64 = 1e-6;

/// A numeric text input's decrement or increment part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiNumberStep {
    /// The part at the field's start, which steps the number down.
    Decrement,
    /// The part at the field's end, which steps the number up.
    Increment,
}

impl GuiNumberStep {
    /// The part's index: 0 for the decrement part, 1 for the increment part.
    pub fn index(self) -> usize {
        match self {
            Self::Decrement => 0,
            Self::Increment => 1,
        }
    }

    /// The steps one press makes: -1 down, 1 up.
    pub fn steps(self) -> f32 {
        match self {
            Self::Decrement => -1.0,
            Self::Increment => 1.0,
        }
    }
}

/// Parse a plain decimal number: optional ASCII whitespace around an optional
/// sign (`+` or `-`), digits with at most one decimal point and at least one
/// digit. Exponents, thousands separators, other locales' decimal marks,
/// infinities and expressions are not numbers here.
pub fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim_ascii();
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let mut points = 0;
    let mut counted = 0;
    for byte in digits.bytes() {
        match byte {
            b'0'..=b'9' => counted += 1,
            b'.' => points += 1,
            _ => return None,
        }
    }
    if counted == 0 || points > 1 {
        return None;
    }
    text.parse::<f64>().ok().filter(|value| !value.is_nan())
}

/// `value` with `precision` decimals, without a sign on a zero.
pub fn format_number(value: f32, precision: u32) -> Arc<str> {
    let text = format!("{value:.*}", precision as usize);
    match text.strip_prefix('-') {
        Some(unsigned) if unsigned.bytes().all(|byte| matches!(byte, b'0' | b'.')) => {
            unsigned.into()
        }
        _ => text.into(),
    }
}

impl GuiTextInput {
    /// The committed number as the input shows it.
    pub(in crate::world::systems) fn formatted(&self) -> Arc<str> {
        format_number(self.value, self.precision)
    }

    /// The number `text` commits: parsed and clamped to the range, or None
    /// when it does not parse.
    pub(in crate::world::systems) fn committed(&self, text: &str) -> Option<f32> {
        let parsed = parse_number(text)?;
        let clamped = parsed.clamp(f64::from(self.min), f64::from(self.max)) as f32;
        // A typed negative zero is zero.
        Some(clamped + 0.0)
    }

    /// The increment of one step: `fine_step` when `fine` and set, else `step`.
    fn stride(&self, fine: bool) -> f32 {
        if fine && self.fine_step > 0.0 {
            self.fine_step
        } else {
            self.step
        }
    }

    /// `from` moved by `steps` strides, clamped to the range.
    pub(in crate::world::systems) fn stepped(&self, from: f32, steps: f32, fine: bool) -> f32 {
        (from + steps * self.stride(fine)).clamp(self.min, self.max) + 0.0
    }

    /// Whether the decrement and increment parts step: each until the
    /// committed number reaches its bound, and neither with a zero step.
    pub(in crate::world::systems) fn step_enabled(&self) -> [bool; 2] {
        let steps = self.step > 0.0;
        [
            steps && self.value > self.min,
            steps && self.value < self.max,
        ]
    }

    /// Whether the input shows its decrement and increment parts.
    pub(in crate::world::systems) fn shows_step_parts(&self) -> bool {
        self.numeric && self.step_parts
    }

    /// What `operation` does with the input and its pending `edit`, if any.
    fn number_outcome(
        &self,
        operation: GuiNumberOperation,
        edit: Option<&Arc<str>>,
    ) -> GuiNumberOutcome {
        let unchanged = GuiNumberOutcome {
            value: self.value,
            rejected: None,
            resets: false,
            repeats: false,
        };
        // The pending edit's number, or the committed one with the rejected text.
        let commit = || match edit {
            None => (self.value, None, false),
            Some(text) => match self.committed(text) {
                Some(value) => (value, None, true),
                None => (self.value, Some(text.clone()), false),
            },
        };
        match operation {
            GuiNumberOperation::Discard => GuiNumberOutcome {
                resets: edit.is_some(),
                ..unchanged
            },
            GuiNumberOperation::Commit {
                ..
            } => {
                let (value, rejected, resets) = commit();
                GuiNumberOutcome {
                    value,
                    rejected,
                    resets,
                    repeats: false,
                }
            }
            GuiNumberOperation::Step {
                steps,
                fine,
                part,
            } => {
                let disabled = match part {
                    Some(part) => !self.step_enabled()[part.index()],
                    None => self.stride(fine) == 0.0,
                };
                if disabled {
                    return unchanged;
                }
                let (base, rejected, _) = commit();
                let value = self.stepped(base, steps, fine);
                GuiNumberOutcome {
                    value,
                    rejected,
                    resets: true,
                    repeats: part.is_some() && value != base,
                }
            }
        }
    }
}

/// The local `[x, y, width, height]` of a numeric input's decrement and
/// increment parts in a control of `size`: squares of the control's height at
/// its start and end, never overlapping.
pub(crate) fn number_step_rects(size: [f32; 2]) -> [[f32; 4]; 2] {
    let side = size[1].min(size[0] * 0.5).max(0.0);
    [
        [0.0, 0.0, side, size[1]],
        [size[0] - side, 0.0, side, size[1]],
    ]
}

/// A routed operation on a numeric text input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::world::systems::gui::local) enum GuiNumberOperation {
    /// Commit a pending edit: Enter (`submit`) or blur.
    Commit {
        /// Whether Enter submitted it, which publishes `Submitted`.
        submit: bool,
    },
    /// Discard a pending edit: Escape.
    Discard,
    /// Commit a pending edit, then step: Up or Down, or a press on a part.
    Step {
        /// Signed strides.
        steps: f32,
        /// Whether Shift selects the fine step.
        fine: bool,
        /// The pressed part, which arms the held repeat.
        part: Option<GuiNumberStep>,
    },
}

/// What one number operation does.
struct GuiNumberOutcome {
    /// The committed number afterwards.
    value: f32,
    /// The edit text that did not parse.
    rejected: Option<Arc<str>>,
    /// Whether the edit ends and shows the formatted number afterwards.
    resets: bool,
    /// Whether the pressed part repeats while held.
    repeats: bool,
}

/// A held step part's repeat: GUI System state on the Host clock.
#[derive(Clone, Copy, Debug)]
pub(in crate::world::systems::gui) struct GuiNumberRepeat {
    target: GuiEntityTarget,
    part: GuiNumberStep,
    /// Seconds until the next step.
    left: f64,
    /// Armed at this frame's mutation boundary: its delta predates the press.
    fresh: bool,
}

fn value_write(entity: EntityId, value: f32) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_TEXT_INPUT,
        field: FieldWrite {
            offset: std::mem::offset_of!(GuiTextInput, value) as u32,
            value: FieldValue::F32(value),
        },
    }
}

impl GuiLocalState {
    /// The edit text of `target`'s native record while it differs from the
    /// formatted number it started from.
    pub(in crate::world::systems::gui) fn number_edit(
        &self,
        target: GuiEntityTarget,
    ) -> Option<Arc<str>> {
        let state = self.native_text(target)?;
        let native = self.native_text.as_ref()?;
        let basis = native.basis.as_ref()?;
        (*state.text != **basis).then(|| state.text.clone())
    }

    /// The pending numeric edit that focus moving as `focused` on `target`
    /// ends: the focused control's, when focus leaves it.
    fn ending_number_edit(
        &self,
        target: GuiEntityTarget,
        focused: bool,
    ) -> Option<(GuiEntityTarget, Arc<str>)> {
        let (current, owner) = self.focus.as_ref()?;
        if !crate::systems::gui::system_state::live(owner) || (*current == target) == focused {
            return None;
        }
        Some((*current, self.number_edit(*current)?))
    }

    /// Keep the pending numeric edit focus is leaving, for the GUI System to
    /// commit once it can write: focus the runtime moves into an overlay.
    pub(in crate::world::systems::gui::local) fn keep_number_edit(
        &mut self,
        target: GuiEntityTarget,
    ) {
        if let Some(focused) = self.focus.as_ref().map(|(focused, _)| *focused)
            && focused != target
            && let Some(text) = self.number_edit(focused)
        {
            self.number_commits.push((focused, text));
        }
    }

    /// Whether a live pointer presses `part` of `target`.
    fn step_held(&self, target: GuiEntityTarget, part: GuiNumberStep) -> bool {
        self.pressed_part(target, GuiInteractionPart::Step(part))
    }
}

impl crate::WorldContext<'_> {
    /// Whether `target`'s native record, held by input `session`, has a
    /// pending numeric edit, which Escape discards before anything else.
    pub(crate) fn gui_number_edit(
        &self,
        target: GuiEntityTarget,
        session: crate::services::gui_input::GuiInputSessionId,
    ) -> bool {
        self.gui_native_text(target, session).is_some()
            && self
                .system::<GuiSystem>(GuiSystem::ID)
                .is_some_and(|gui| gui.local.number_edit(target).is_some())
    }
}

impl GuiSystem {
    /// Apply a routed number operation on a numeric text input: commit,
    /// discard or step its edit and number, publish a rejection or Enter's
    /// submission, and arm a pressed part's repeat.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::world::systems::gui::local) fn number_operation(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        command: &GuiLocalCommand,
        control: GuiControl,
        operation: GuiNumberOperation,
        tick: u64,
        ancestry: Arc<[EntityId]>,
    ) -> Result<(), GuiInputError> {
        let unsupported = GuiInputError::Local(GuiLocalActionError::UnsupportedAction);
        let target = control.target;
        let input = context
            .world
            .world
            .components
            .gui_text_input(target.entity.index() as usize)
            .filter(|input| input.numeric)
            .cloned()
            .ok_or(unsupported)?;
        if matches!(
            operation,
            GuiNumberOperation::Step {
                part: Some(_),
                ..
            }
        ) && !input.step_parts
        {
            return Err(unsupported);
        }

        // Only the session holding the edit commits or discards it.
        let owner = command.input.session_lifetime();
        let held = self.local.holds_native_text(target, &owner);
        let edit = held.then(|| self.local.number_edit(target)).flatten();
        let outcome = input.number_outcome(operation, edit.as_ref());
        let formatted = format_number(outcome.value, input.precision);
        let kind = match (outcome.rejected.clone(), operation) {
            (Some(text), _) => Some(GuiLocalEffectKind::Rejected(text)),
            (None, GuiNumberOperation::Discard) => edit.clone().map(GuiLocalEffectKind::Discarded),
            (
                None,
                GuiNumberOperation::Commit {
                    submit: true,
                },
            ) => Some(GuiLocalEffectKind::Submitted(formatted.clone())),
            _ => None,
        };
        let resets = outcome.resets && held;
        let generation = self
            .local
            .native_generation
            .checked_add(u64::from(resets))
            .ok_or(GuiInputError::Capacity)?;
        let presentation_revision = self
            .local
            .presentation_revision
            .checked_add(u64::from(resets))
            .ok_or(GuiInputError::Capacity)?;
        let text_revision = self
            .local
            .text_revision
            .checked_add(u64::from(resets))
            .ok_or(GuiInputError::Capacity)?;
        let changed = outcome.value != input.value;
        let prepared = match kind {
            Some(kind) => command.input.prepare_effect(GuiLocalEffect {
                id: self.local.observations.candidate_id(target.world, &kind)?,
                target,
                source: command.input.effect_source(),
                tick,
                ancestry,
                kind,
            })?,
            None => command.input.prepare_write(changed)?,
        };
        if changed {
            // The edit this session holds is reset below, not by the write's
            // own refresh, which replaces any other record of the control.
            self.local.own_text_write = resets;
            let result = context
                .world
                .apply_authored_commands(Some(self), &[value_write(target.entity, outcome.value)]);
            self.local.own_text_write = false;
            if let Err(reason) = result {
                let error = crate::systems::gui::local::actions::write_error(reason);
                command.input.reject(error);
                return Err(error);
            }
        }
        let local = &mut self.local;
        let part = match operation {
            GuiNumberOperation::Step {
                part,
                ..
            } => part,
            _ => None,
        };
        prepared.commit_with_publication(
            || {
                if resets && let Some(native) = local.native_text.as_mut() {
                    native.state = super::native_text::GuiNativeText::state(
                        target,
                        formatted.clone(),
                        generation,
                        native.state.masked,
                    );
                    native.basis = Some(formatted);
                    local.native_generation = generation;
                    local.presentation_revision = presentation_revision;
                    local.text_revision = text_revision;
                }
                if let Some(part) = part {
                    local.number_repeat = outcome.repeats.then_some(GuiNumberRepeat {
                        target,
                        part,
                        left: GUI_NUMBER_REPEAT_DELAY,
                        fresh: true,
                    });
                }
            },
            |effect| local.observations.publish(effect),
        )
    }

    /// Advance a held step part's repeat by the frame's Host delta, stepping
    /// the number once for each interval that ran out, and stop it when no
    /// pointer holds the part, at the bound, or when the control is no longer
    /// an eligible numeric input with step parts.
    pub(in crate::world::systems::gui) fn advance_number_repeat(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
    ) {
        let Some(mut repeat) = self.local.number_repeat.take() else {
            return;
        };
        let world = &*context.world.world;
        let entity = repeat.target.entity;
        let Some(input) = entity_control(world, &world.state, entity)
            .filter(|control| control.target == repeat.target)
            .filter(|_| eligibility(world, entity).eligible())
            .and_then(|_| world.components.gui_text_input(entity.index() as usize))
            .filter(|input| input.shows_step_parts())
        else {
            return;
        };
        if !self.local.step_held(repeat.target, repeat.part)
            || !input.step_enabled()[repeat.part.index()]
        {
            return;
        }
        if repeat.fresh {
            repeat.fresh = false;
            self.local.number_repeat = Some(repeat);
            return;
        }
        repeat.left -= context.dt();
        let mut steps = 0.0;
        while repeat.left <= SLACK {
            steps += 1.0;
            repeat.left += GUI_NUMBER_REPEAT_INTERVAL;
        }
        let value = input.stepped(input.value, steps * repeat.part.steps(), false);
        if value != input.value {
            if let Err(reason) = context
                .world
                .apply_authored_commands(Some(self), &[value_write(entity, value)])
            {
                crate::diagnostic!(
                    Warn,
                    "[IPP core] gui.number repeat write failed reason={reason:?}"
                );
                return;
            }
            self.motion.dirty_entity(entity);
        }
        self.local.number_repeat = Some(repeat);
    }

    /// Commit the numeric edits focus left when the runtime moved it, as blur
    /// commits them, publishing a rejection for text that does not parse.
    pub(in crate::world::systems::gui) fn commit_kept_number_edits(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
    ) {
        for (target, text) in std::mem::take(&mut self.local.number_commits) {
            let world = &*context.world.world;
            let Some(input) = entity_control(world, &world.state, target.entity)
                .filter(|control| control.target == target)
                .and_then(|_| {
                    world
                        .components
                        .gui_text_input(target.entity.index() as usize)
                })
                .filter(|input| input.numeric)
            else {
                continue;
            };
            match input.committed(&text) {
                Some(value) if value != input.value => {
                    if let Err(reason) = context
                        .world
                        .apply_authored_commands(Some(self), &[value_write(target.entity, value)])
                    {
                        crate::diagnostic!(
                            Warn,
                            "[IPP core] gui.number commit failed reason={reason:?}"
                        );
                    }
                }
                Some(_) => {}
                None => {
                    let tick = world.tick.checked_add(1).expect("World tick exhausted");
                    let ancestry = ancestry(&world.state, target.entity);
                    self.local.publish_number_effect(
                        target,
                        GuiLocalEffectKind::Rejected(text),
                        tick,
                        ancestry,
                    );
                }
            }
        }
    }

    /// Commit the pending numeric edit a client's focus action ends, in the
    /// staged batch, before the focus change itself: `focused` on `target` is
    /// the action's resulting focus.
    pub(in crate::world::systems::gui::local) fn commit_ending_number_edit(
        &mut self,
        context: &mut SystemOperationContext<'_>,
        target: GuiEntityTarget,
        focused: bool,
    ) -> Result<(), ErrorReason> {
        let Some((edited, text)) = self.local.ending_number_edit(target, focused) else {
            return Ok(());
        };
        let Ok(ComponentValue::GuiTextInput(input)) = context.staged.component(
            &context.world_data.components,
            edited.entity,
            ComponentValue::GUI_TEXT_INPUT,
        ) else {
            return Ok(());
        };
        if !input.numeric {
            return Ok(());
        }
        match input.committed(&text) {
            Some(value) if value != input.value => context
                .staged
                .write_component_fields(
                    &context.world_data.components,
                    edited.entity,
                    ComponentValue::GUI_TEXT_INPUT,
                    &[FieldWrite {
                        offset: std::mem::offset_of!(GuiTextInput, value) as u32,
                        value: FieldValue::F32(value),
                    }],
                )
                .map_err(|reason| match reason {
                    ErrorReason::Capacity => ErrorReason::Capacity,
                    _ => ErrorReason::InvalidValue,
                }),
            Some(_) => Ok(()),
            None => {
                let tick = context
                    .world_data
                    .tick
                    .checked_add(1)
                    .ok_or(ErrorReason::Capacity)?;
                let ancestry = ancestry(&context.staged.entities_state, edited.entity);
                self.local.publish_number_effect(
                    edited,
                    GuiLocalEffectKind::Rejected(text),
                    tick,
                    ancestry,
                );
                Ok(())
            }
        }
    }
}

impl GuiLocalState {
    /// Note that the native record about to be dropped or replaced ends its
    /// pending numeric edit without a commit, for the System to report as
    /// discarded at its next publication of discards.
    pub(in crate::world::systems::gui) fn discard_native_edit(&mut self) {
        if let Some(pending) = self
            .native_text
            .as_ref()
            .and_then(super::native_text::GuiNativeText::pending_number_edit)
        {
            self.number_discards.push(pending);
        }
    }

    /// Publish each numeric edit noted as discarded since the last time,
    /// with the tick `tick` of the frame publishing it, on its control while
    /// that is still the control the edit belonged to.
    pub(in crate::world::systems::gui) fn publish_number_discards(
        &mut self,
        world: &crate::world::WorldSimulationState,
        tick: u64,
    ) {
        for (target, text) in std::mem::take(&mut self.number_discards) {
            if entity_control(world, &world.state, target.entity)
                .is_some_and(|control| control.target == target)
            {
                let ancestry = ancestry(&world.state, target.entity);
                self.publish_number_effect(
                    target,
                    GuiLocalEffectKind::Discarded(text),
                    tick,
                    ancestry,
                );
            }
        }
    }

    /// Publish a numeric input's rejection or discard outside a routed
    /// command.
    fn publish_number_effect(
        &mut self,
        target: GuiEntityTarget,
        kind: GuiLocalEffectKind,
        tick: u64,
        ancestry: Arc<[EntityId]>,
    ) {
        let effect = GuiLocalEffect {
            id: self
                .observations
                .candidate_id(target.world, &kind)
                .expect("GUI effect ordinal exhausted"),
            target,
            source: GuiLocalEffectSource::Semantic,
            tick,
            ancestry,
            kind,
        };
        self.observations.publish(&effect);
    }
}

#[cfg(test)]
#[path = "number_tests.rs"]
mod tests;
