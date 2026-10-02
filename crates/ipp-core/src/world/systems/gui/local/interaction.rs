use super::control::{GuiControl, component_incarnation, eligibility, entity_control};
use super::*;
use crate::services::gui_input::{GuiInputError, GuiPointerLease, GuiPointerLeaseId};
use crate::world::{WorldEntityState, WorldSimulationState};

/// Transient feedback only: no implicit value, activation or logical-focus action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiInteractionUpdate {
    /// Set pointer hover without changing press/capture ownership.
    Hover(bool),
    /// Mark a pressed control without activating it or moving focus.
    Press,
    /// Mark logical capture after Press; native capture remains platform-owned.
    Capture,
    /// Clear press and capture, retaining hover. Repeated release is an applied no-op.
    Release,
    /// Clear all feedback and permanently revoke this activation generation.
    Cancel,
}

/// Which part of a control a pointer hovers or presses. Scroll bar parts, the
/// focus parts of a control with several and a numeric text input's step
/// parts resolve their own skin state; every other part follows the control
/// body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GuiInteractionPart {
    /// The control body.
    #[default]
    Control,
    /// A scroll bar track on axis 0 (horizontal) or 1 (vertical).
    ScrollTrack(usize),
    /// A scroll bar thumb on axis 0 (horizontal) or 1 (vertical).
    ScrollThumb(usize),
    /// One focus part of a control with several, such as a range's thumb:
    /// the one under the pointer, or the one it presses or drags.
    FocusPart(u32),
    /// A numeric text input's decrement or increment part, which is no
    /// focus part: the one under the pointer or the one it holds.
    Step(super::GuiNumberStep),
}

/// Most focus parts one control has: a range's two thumbs, and room for a
/// colour control's surfaces.
pub(in crate::world::systems) const GUI_MAX_FOCUS_PARTS: usize = 4;

/// Live pointer feedback of one control's scroll bar parts, per axis, of its
/// focus parts when it has several, and of a numeric input's step parts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::world::systems) struct GuiPartInteraction {
    /// Pointers over or pressing each axis's scroll track.
    pub track: [GuiInteractionFlags; 2],
    /// Pointers over or pressing each axis's scroll thumb.
    pub thumb: [GuiInteractionFlags; 2],
    /// Pointers over, pressing or dragging each focus part.
    pub parts: [GuiInteractionFlags; GUI_MAX_FOCUS_PARTS],
    /// Pointers over or holding the decrement and increment parts.
    pub steps: [GuiInteractionFlags; 2],
    /// Pointers over or pressing the control body, naming none of its parts.
    pub body: GuiInteractionFlags,
}

/// Read-only aggregate local feedback, not authored or durable component state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiInteractionFlags {
    /// At least one live pointer hovers this control.
    pub hovered: bool,
    /// At least one live pointer presses this control.
    pub pressed: bool,
    /// At least one live logical pointer capture belongs to this control.
    pub captured: bool,
}

impl GuiInteractionFlags {
    pub(in crate::world::systems) fn any(self) -> bool {
        self.hovered || self.pressed || self.captured
    }
}

/// Exact prepared feedback result; control values are unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiInteractionEffect {
    /// Service-issued exact activation, never a reusable wire number.
    pub lease: GuiPointerLeaseId,
    /// Pointer number scoped by the lease's service session/context.
    pub pointer: u64,
    /// This pointer's resulting local flags, not the control-wide aggregate.
    pub state: GuiInteractionFlags,
    /// Whether this pointer's feedback flags changed; duplicates are applied no-ops.
    pub changed: bool,
}

pub(super) struct GuiPointerFeedback {
    target: GuiEntityTarget,
    lease: GuiPointerLease,
    state: GuiInteractionFlags,
    part: GuiInteractionPart,
    /// The group whose active item the target can be: the target's hover
    /// shows only while it is that item or the group has none.
    group: Option<crate::EntityId>,
}

impl GuiLocalState {
    /// Whether Canvas styles up to the top level leave the control visible
    /// and hittable.
    pub(super) fn interaction_policy(
        world: &WorldSimulationState,
        state: &WorldEntityState,
        entity: crate::EntityId,
    ) -> bool {
        let mut current = Some(entity);
        let mut steps = 0;
        while let Some(ancestor) = current {
            if let Some(style) = world.components.canvas_style(ancestor.index() as usize)
                && (style.opacity == 0.0
                    || style.scale_x == 0.0
                    || style.scale_y == 0.0
                    || (style.clipped
                        && (style.clip_max_x <= style.clip_min_x
                            || style.clip_max_y <= style.clip_min_y)))
            {
                return false;
            }
            steps += 1;
            if steps > state.entities.len() {
                return false;
            }
            current = state.links.effective(ancestor).and_then(|link| link.parent);
        }
        true
    }

    /// Aggregate live pointer feedback of one control, as paint reads it: a
    /// group's active item shows as hovered, and pointer hover on another
    /// item of that group does not, so the group shows one highlight.
    pub(in crate::world::systems) fn interaction_flags(
        &self,
        target: GuiEntityTarget,
    ) -> GuiInteractionFlags {
        let mut result = GuiInteractionFlags::default();
        for record in &self.pointers {
            if record.target == target && record.lease.is_live() {
                let shown = record.group.is_none_or(|group| {
                    self.active_item(group)
                        .is_none_or(|active| active == target)
                });
                result.hovered |= record.state.hovered && shown;
                result.pressed |= record.state.pressed;
                result.captured |= record.state.captured;
            }
        }
        result.hovered |= self.is_active(target);
        result
    }

    /// Controls a live pointer hovers, in the order the pointers came.
    pub(super) fn hovered_controls(&self) -> impl Iterator<Item = GuiEntityTarget> + '_ {
        self.pointers
            .iter()
            .filter(|record| record.lease.is_live() && record.state.hovered)
            .map(|record| record.target)
    }

    /// Live pointer records in target entity and pointer order, as the
    /// `GuiPointers` System query reports them.
    pub(in crate::world::systems::gui) fn pointer_records(&self) -> Vec<GuiPointerRecord> {
        let mut records: Vec<_> = self
            .pointers
            .iter()
            .filter(|record| record.lease.is_live() && record.state.any())
            .map(|record| GuiPointerRecord {
                target: record.target,
                pointer: record.lease.pointer(),
                state: record.state,
            })
            .collect();
        records.sort_by_key(|record| (record.target.entity, record.pointer, record.target));
        records
    }

    /// Scroll bar and focus part feedback of an eligible control whose
    /// aggregate `interaction` is not empty; ineligible controls show no
    /// feedback.
    pub(in crate::world::systems::gui) fn part_interaction(
        &self,
        target: GuiEntityTarget,
        interaction: GuiInteractionFlags,
    ) -> GuiPartInteraction {
        let mut result = GuiPartInteraction::default();
        if !interaction.any() {
            return result;
        }
        for record in &self.pointers {
            if record.target != target || !record.lease.is_live() {
                continue;
            }
            let flags = match record.part {
                GuiInteractionPart::Control => &mut result.body,
                GuiInteractionPart::Step(step) => &mut result.steps[step.index()],
                GuiInteractionPart::ScrollTrack(axis) => &mut result.track[axis.min(1)],
                GuiInteractionPart::ScrollThumb(axis) => &mut result.thumb[axis.min(1)],
                GuiInteractionPart::FocusPart(part) => match result.parts.get_mut(part as usize) {
                    Some(flags) => flags,
                    None => continue,
                },
            };
            flags.hovered |= record.state.hovered;
            flags.pressed |= record.state.pressed;
            flags.captured |= record.state.captured;
        }
        result
    }

    /// Whether a live pointer presses `part` of `target`.
    pub(super) fn pressed_part(&self, target: GuiEntityTarget, part: GuiInteractionPart) -> bool {
        self.pointers.iter().any(|record| {
            record.target == target
                && record.lease.is_live()
                && record.state.pressed
                && record.part == part
        })
    }

    pub(super) fn invalidate_interactions(&self, state: &WorldEntityState) {
        for record in &self.pointers {
            if component_incarnation(state, record.target.entity, record.target.component)
                != Some(record.target.incarnation)
            {
                record.lease.revoke();
            }
        }
    }

    pub(in crate::world::systems::gui) fn revalidate_interactions(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
    ) {
        let tree = super::group::GuiGroupTree {
            world,
            state,
        };
        for record in &mut self.pointers {
            let entity = record.target.entity;
            let control = entity_control(world, state, entity)
                .filter(|control| control.target == record.target);
            if !(control.is_some()
                && eligibility(world, entity).eligible()
                && Self::interaction_policy(world, state, entity))
            {
                record.lease.revoke();
            }

            // A move between groups changes which active item hides its hover.
            record.group = control.and_then(|control| tree.active_group(control));
        }
        let previous = self.pointers.len();
        self.pointers.retain(|record| record.lease.is_live());
        if self.pointers.len() != previous {
            self.presentation_changed(false);
        }
    }

    pub(super) fn clear_interactions(&mut self) {
        for record in &self.pointers {
            record.lease.revoke();
        }
        self.pointers.clear();
    }

    /// Apply pointer feedback. Hover over an item that does not take focus,
    /// of `group`, also makes it the group's active item.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn interact(
        &mut self,
        command: &GuiLocalCommand,
        control: GuiControl,
        lease: &GuiPointerLease,
        update: GuiInteractionUpdate,
        part: GuiInteractionPart,
        group: Option<crate::EntityId>,
        tick: u64,
        ancestry: std::sync::Arc<[crate::EntityId]>,
    ) -> Result<(), GuiInputError> {
        lease.validate(&command.input)?;
        let index = self
            .pointers
            .iter()
            .position(|record| record.lease.id() == lease.id());
        let (before, before_part) = index.map_or(Default::default(), |index| {
            (self.pointers[index].state, self.pointers[index].part)
        });
        let mut state = before;
        match update {
            GuiInteractionUpdate::Hover(hovered) => state.hovered = hovered,
            GuiInteractionUpdate::Press => state.pressed = true,
            GuiInteractionUpdate::Capture if state.pressed => state.captured = true,
            GuiInteractionUpdate::Capture => {
                return Err(GuiInputError::Local(GuiLocalActionError::UnsupportedAction));
            }
            GuiInteractionUpdate::Release => {
                state.pressed = false;
                state.captured = false;
            }
            GuiInteractionUpdate::Cancel => state = GuiInteractionFlags::default(),
        }
        let changed = state != before;
        let activates = group.filter(|&group| {
            update == GuiInteractionUpdate::Hover(true)
                && self.active_item(group) != Some(control.target)
        });
        let repainted = changed || (state.any() && part != before_part) || activates.is_some();
        let revision = self
            .presentation_revision
            .checked_add(u64::from(repainted))
            .ok_or(GuiInputError::Capacity)?;
        if index.is_none() && state.any() {
            self.pointers
                .try_reserve(1)
                .map_err(|_| GuiInputError::Capacity)?;
        }
        if activates.is_some() {
            self.reserve_active()?;
        }
        let kind = GuiLocalEffectKind::InteractionChanged(GuiInteractionEffect {
            lease: lease.id(),
            pointer: lease.pointer(),
            state,
            changed,
        });
        let effect = GuiLocalEffect {
            id: self
                .observations
                .candidate_id(control.target.world, &kind)?,
            target: control.target,
            source: command.input.effect_source(),
            tick,
            ancestry,
            kind,
        };
        let prepared = command.input.prepare_effect(effect)?;
        lease.validate(&command.input)?;
        let pointers = &mut self.pointers;
        let presentation_revision = &mut self.presentation_revision;
        let observations = &mut self.observations;
        let active = &mut self.active;
        prepared.commit_with_publication(
            || {
                if update == GuiInteractionUpdate::Cancel {
                    lease.revoke();
                } else if state.any() {
                    lease.activate();
                }
                match index {
                    Some(index) => {
                        pointers[index].state = state;
                        pointers[index].part = part;
                        pointers[index].group = group;
                    }
                    None if state.any() => pointers.push(GuiPointerFeedback {
                        target: control.target,
                        lease: lease.clone(),
                        state,
                        part,
                        group,
                    }),
                    None => {}
                }
                if let Some(group) = activates {
                    active.retain(|(entry, _)| *entry != group);
                    active.push((group, control.target));
                }
                *presentation_revision = revision;
            },
            |effect| observations.publish(effect),
        )?;
        Ok(())
    }
}

impl Drop for GuiLocalState {
    fn drop(&mut self) {
        self.clear_interactions();
    }
}
