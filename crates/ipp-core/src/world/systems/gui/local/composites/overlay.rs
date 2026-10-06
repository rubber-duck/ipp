//! Overlay modes in the GUI System: focus entering and leaving light and
//! modal overlays, light overlays closing when focus leaves them, the
//! closing an input router requests and the timing of hints.
//!
//! An overlay is open while its evaluated `GuiBehavior.effective_visible`
//! holds; the System checks each overlay whose eligibility fields the
//! refresh wrote, and every overlay whose `GuiOverlay` changed. It records
//! each open light and modal overlay with its invoker: the control it moved
//! focus from when the overlay opened, if it moved focus.
//! Opening moves focus to the overlay's first eligible focusable control in
//! tree order, in the focus-visible modality of the focus it replaces, or
//! with the ring when nothing was focused (a text input shows it either
//! way); an overlay without one leaves focus where it is. Closing an
//! overlay while focus is inside it returns focus to its invoker when that
//! is still an eligible focusable control, keeping the modality; otherwise
//! the ordinary revalidation clears focus. Each move publishes the focus
//! changes of the control losing and the control taking focus.
//!
//! Focus moving anywhere outside an open light overlay and its parent's
//! subtree closes the overlay in the same frame, including focus ending:
//! focus moving to another panel ends this World's focus. Moves inside a
//! nested overlay are inside every overlay it opened from.
//!
//! Focus the runtime moves out of a numeric text input commits its pending
//! edit, as blur does ([numbers](crate::systems::gui::local::controls::number)).
//!
//! The System writes the `visible` field it closes or opens through the
//! ordinary authored write path, so clients observe it like their own
//! writes. It does all of this before the frame's focus revalidation; a
//! canvas without overlays, hovered hints or focus changes does no work.

use crate::systems::SystemUpdateContext;
use crate::systems::gui::GuiSystem;
use crate::systems::gui::layout::GuiOverlay;
use crate::systems::gui::local::controls::identity::{
    ancestry, component_incarnation, eligibility, entity_control, focus_parts, focusable,
};
use crate::systems::gui::local::{
    GuiBehavior, GuiEntityTarget, GuiLocalEffect, GuiLocalEffectKind, GuiLocalEffectSource,
};
use crate::systems::gui::system_state::{GuiLocalState, live};
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{Command, ComponentValue, EntityId, EntityRef, FieldValue, FieldWrite};
use std::collections::BTreeSet;

/// One open light or modal overlay the System saw open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuiOpenOverlay {
    entity: EntityId,
    /// Its `GuiOverlay` lifetime.
    incarnation: u64,
    mode: u32,
    /// The control focus moved from when the overlay took focus, and its
    /// focused part.
    invoker: Option<(GuiEntityTarget, u32)>,
}

/// GUI System state of the overlays: the open light and modal overlays with
/// their invokers, the overlays to check and the hint timing.
#[derive(Default)]
pub(in crate::world::systems::gui) struct GuiOverlayState {
    open: Vec<GuiOpenOverlay>,
    /// Overlays that may have opened or closed since the last check.
    pub(in crate::world::systems::gui) changed: BTreeSet<EntityId>,
    /// Logical focus at the last check of the light overlays focus closes.
    checked_focus: Option<GuiEntityTarget>,
    hint: super::hint::GuiHintState,
}

/// A `GUI_OVERLAY` component lifetime and its `mode`.
fn overlay(world: &WorldSimulationState, entity: EntityId) -> Option<(u64, u32)> {
    let incarnation = component_incarnation(&world.state, entity, ComponentValue::GUI_OVERLAY)?;
    let mode = world.components.gui_overlay(entity.index() as usize)?.mode;
    Some((incarnation, mode))
}

/// Whether the overlay `entity` is open: it exists and it and all its
/// ancestors are visible.
pub(in crate::world::systems::gui::local) fn is_open(
    world: &WorldSimulationState,
    entity: EntityId,
) -> bool {
    world.state.entities.contains_key(&entity)
        && world
            .components
            .gui_behavior(entity.index() as usize)
            .is_some_and(|behavior| behavior.effective_visible)
}

/// Whether `target`'s root-first ancestry places it inside `overlay` or, for
/// a light overlay, its parent's subtree.
fn holds(state: &WorldEntityState, overlay: EntityId, parent: bool, target: EntityId) -> bool {
    let ancestry = ancestry(state, target);
    ancestry.contains(&overlay)
        || (parent
            && state
                .links
                .effective(overlay)
                .and_then(|link| link.parent)
                .is_some_and(|parent| ancestry.contains(&parent)))
}

impl GuiLocalState {
    /// Focus `target` for the runtime, without an owning input session, as a
    /// command focuses: the input router adopts it as its keyboard target.
    /// A text input shows the ring in either modality, as a pointer press
    /// on one does. The control losing focus and `target` publish focus
    /// changes, so clients following focus feedback see both.
    fn runtime_focus(
        &mut self,
        world: &WorldSimulationState,
        target: GuiEntityTarget,
        part: u32,
        visible: bool,
    ) {
        let composing = self.composing();
        let previous = self
            .focus
            .as_ref()
            .filter(|(focused, owner)| live(owner) && *focused != target)
            .map(|(focused, _)| *focused);
        if let Some(previous) = previous {
            self.publish_focus(world, previous, false, self.focus_part);
        }
        self.publish_focus(world, target, true, part);
        self.keep_number_edit(target);
        self.focus = Some((target, None));
        self.focus_part = part;
        self.focus_visible = visible || target.component == ComponentValue::GUI_TEXT_INPUT;
        self.reveal = Some(target.entity);
        self.native_text = None;
        self.native_generation = self
            .native_generation
            .checked_add(1)
            .expect("GUI native text generation exhausted");
        self.presentation_changed(composing);
    }

    /// Publish that `target` took or lost focus, with the tick of the frame
    /// moving it, as a client's focus action publishes its change.
    fn publish_focus(
        &mut self,
        world: &WorldSimulationState,
        target: GuiEntityTarget,
        focused: bool,
        part: u32,
    ) {
        let kind = GuiLocalEffectKind::FocusChanged {
            focused,
            changed: true,
            part,
        };
        let effect = GuiLocalEffect {
            id: self
                .observations
                .candidate_id(target.world, &kind)
                .expect("GUI effect ordinal exhausted"),
            target,
            source: GuiLocalEffectSource::Semantic,
            tick: world.tick.checked_add(1).expect("World tick exhausted"),
            ancestry: ancestry(&world.state, target.entity),
            kind,
        };
        self.observations.publish(&effect);
    }

    /// The first eligible control in `overlay`'s subtree, in tree order, that
    /// takes focus and is a traversal stop.
    fn first_focusable(world: &WorldSimulationState, overlay: EntityId) -> Option<GuiEntityTarget> {
        let mut pending = vec![overlay];
        let mut visited = BTreeSet::new();
        while let Some(entity) = pending.pop() {
            if !visited.insert(entity) {
                continue;
            }
            if let Some(control) = entity_control(world, &world.state, entity)
                && super::group::item_kind(control.kind)
                && eligibility(world, entity).eligible()
                && focusable(world, entity)
            {
                return Some(control.target);
            }
            let children: Vec<_> = world.state.links.children(Some(entity)).collect();
            pending.extend(children.into_iter().rev());
        }
        None
    }

    /// Whether `target` is still a control that may take focus.
    fn focusable_target(world: &WorldSimulationState, target: GuiEntityTarget) -> bool {
        entity_control(world, &world.state, target.entity)
            .is_some_and(|control| control.target == target)
            && eligibility(world, target.entity).eligible()
            && focusable(world, target.entity)
    }

    /// Add the overlays among the entities whose eligibility fields changed
    /// since the frame's report began to the overlays to check.
    fn collect_overlay_changes(&mut self, world: &WorldSimulationState) {
        self.overlays
            .changed
            .extend(self.eligibility_changed.iter().filter(|entity| {
                world
                    .components
                    .gui_overlay(entity.index() as usize)
                    .is_some()
            }));
    }

    /// Apply the opening and closing of the overlays checked since the last
    /// time: focus returns from each closed overlay holding it to its
    /// invoker, then enters each opened overlay, outermost first.
    fn overlay_transitions(&mut self, world: &WorldSimulationState) {
        self.collect_overlay_changes(world);
        let changed = std::mem::take(&mut self.overlays.changed);
        if changed.is_empty() {
            return;
        }
        let world_ref = crate::WorldRef {
            id: world.id,
            incarnation: world.identity,
        };
        let mut opened = Vec::new();
        for entity in changed {
            let current = overlay(world, entity).filter(|&(_, mode)| {
                mode == GuiOverlay::MODE_LIGHT || mode == GuiOverlay::MODE_MODAL
            });
            let open = current.is_some() && is_open(world, entity);
            let recorded = self
                .overlays
                .open
                .iter()
                .position(|record| record.entity == entity);
            if let Some(index) = recorded
                && (!open
                    || current.map(|(incarnation, _)| incarnation)
                        != Some(self.overlays.open[index].incarnation))
            {
                let closed = self.overlays.open.remove(index);
                self.focus_returns(world, closed);
            }
            if let Some((incarnation, mode)) = current
                && open
                && !self
                    .overlays
                    .open
                    .iter()
                    .any(|record| record.entity == entity)
            {
                opened.push(GuiOpenOverlay {
                    entity,
                    incarnation,
                    mode,
                    invoker: None,
                });
            }
        }

        // An overlay opened inside another one takes focus after it.
        opened.sort_by_key(|record| ancestry(&world.state, record.entity).len());
        for mut record in opened {
            if let Some(first) = Self::first_focusable(world, record.entity) {
                let current = self.focus.as_ref().filter(|(_, owner)| live(owner));
                let visible = current.is_none() || self.focus_visible;
                record.invoker = current
                    .map(|(target, _)| (*target, self.focus_part))
                    .filter(|(target, _)| target.world == world_ref);
                if current.map(|(target, _)| *target) != Some(first) {
                    self.runtime_focus(world, first, 0, visible);
                }
            }
            self.overlays.open.push(record);
        }
    }

    /// Return focus from `closed` to its invoker, when focus is inside it and
    /// the invoker may still take focus.
    fn focus_returns(&mut self, world: &WorldSimulationState, closed: GuiOpenOverlay) {
        let Some((invoker, part)) = closed.invoker else {
            return;
        };
        let Some((focused, owner)) = self.focus.as_ref() else {
            return;
        };
        if !live(owner)
            || !world.state.entities.contains_key(&focused.entity)
            || !holds(&world.state, closed.entity, false, focused.entity)
            || !Self::focusable_target(world, invoker)
        {
            return;
        }
        // The invoker's part is kept unless the control lost it meanwhile.
        let part = entity_control(world, &world.state, invoker.entity)
            .map_or(0, |control| part.min(focus_parts(world, control) - 1));
        self.runtime_focus(world, invoker, part, self.focus_visible);
    }

    /// The open light overlays focus closes: when focus changed since the
    /// last check, each one holding neither focus in itself nor in its
    /// parent's subtree.
    fn focus_closes(&mut self, world: &WorldSimulationState) -> Vec<EntityId> {
        let focus = self
            .focus
            .as_ref()
            .filter(|(_, owner)| live(owner))
            .map(|(target, _)| *target);
        if focus == self.overlays.checked_focus {
            return Vec::new();
        }
        self.overlays.checked_focus = focus;
        self.overlays
            .open
            .iter()
            .filter(|record| record.mode == GuiOverlay::MODE_LIGHT)
            .filter(|record| {
                focus.is_none_or(|focus| {
                    !world.state.entities.contains_key(&focus.entity)
                        || !holds(&world.state, record.entity, true, focus.entity)
                })
            })
            .map(|record| record.entity)
            .collect()
    }

    /// The hint the pointer or visible focus asks for, the delay before it
    /// opens and whether a pointer presses its parent. Hover wins over focus.
    fn wanted_hint(&self, world: &WorldSimulationState) -> super::hint::GuiHintRequest {
        let focus = self
            .focus
            .as_ref()
            .filter(|(_, owner)| self.focus_visible && live(owner))
            .map(|(target, _)| *target);
        for parent in self.hovered_controls().chain(focus) {
            let Some(hint) = world
                .state
                .links
                .children(Some(parent.entity))
                .find(|child| {
                    overlay(world, *child).is_some_and(|(_, mode)| mode == GuiOverlay::MODE_HINT)
                })
            else {
                continue;
            };
            return super::hint::GuiHintRequest {
                wanted: Some(super::hint::GuiHint {
                    overlay: hint,
                    parent,
                }),
                delay: if focus == Some(parent) {
                    super::hint::GUI_HINT_FOCUS_DELAY
                } else {
                    super::hint::GUI_HINT_HOVER_DELAY
                },
                pressed: self.interaction_flags(parent).pressed,
            };
        }
        super::hint::GuiHintRequest::default()
    }
}

impl GuiSystem {
    /// The frame's overlay work, before focus revalidation: focus entering
    /// and returning, light overlays closing as focus leaves and hints
    /// opening and closing on the Host clock.
    pub(in crate::world::systems::gui) fn evaluate_overlays(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
    ) {
        let focus = self.local.motion_focus();
        self.local.collect_overlay_changes(context.world.world);
        let hint_dirty = !self.local.overlays.changed.is_empty();
        self.local.overlay_transitions(context.world.world);

        // Focus left in an overlay that closed without returning it ends
        // here, so leaving counts in this frame.
        self.local
            .revalidate_focus(context.world.world, &context.world.world.state);
        let mut writes: Vec<(EntityId, bool)> = self
            .local
            .focus_closes(context.world.world)
            .into_iter()
            .map(|entity| (entity, false))
            .collect();

        // Hints follow hover and visible focus, and wait while a delay or a
        // grace runs.
        let revision = self.local.presentation_revision;
        if self.local.overlays.hint.pending(revision) || hint_dirty {
            let request = self.local.wanted_hint(context.world.world);
            self.local.overlays.hint.step(
                context.world.world,
                request,
                context.dt(),
                revision,
                &mut writes,
            );
        }
        if !writes.is_empty() {
            self.write_open(context, &writes);

            // Closing drops the records; focus is outside what focus closed.
            self.local.overlay_transitions(context.world.world);
        }
        if focus != self.local.motion_focus() {
            for entity in focus.into_iter().chain(self.local.motion_focus()) {
                self.motion.dirty_entity(entity);
            }
        }

        // Focus the runtime moved out of a numeric input commits its edit.
        self.commit_kept_number_edits(context);
    }

    /// Write the `visible` field of each overlay to its value in `writes`,
    /// skipping overlays that are gone or already hold it.
    fn write_open(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        writes: &[(EntityId, bool)],
    ) {
        let world = &*context.world.world;
        let commands: Vec<Command> = writes
            .iter()
            .filter(|&&(entity, open)| {
                world.state.entities.contains_key(&entity)
                    && world
                        .components
                        .gui_behavior(entity.index() as usize)
                        .is_some_and(|behavior| behavior.visible != open)
            })
            .map(|&(entity, open)| visible_write(entity, open))
            .collect();
        if commands.is_empty() {
            return;
        }
        if let Err(reason) = context.world.apply_authored_commands(Some(self), &commands) {
            crate::diagnostic!(
                Warn,
                "[IPP core] gui.overlay visibility write failed reason={reason:?}"
            );
        }
    }

    /// Close an open overlay at the input router's request: a press outside
    /// a light overlay, or Escape. An overlay that is gone, closed or manual
    /// by now stays as it is.
    pub(in crate::world::systems::gui) fn close_overlay(
        &mut self,
        context: &mut crate::systems::SystemCommandContext<'_>,
        command: &GuiOverlayCommand,
    ) -> Result<(), crate::ErrorReason> {
        let world = &*context.world.world;
        let target = command.overlay;
        if target.world.id != world.id
            || target.world.incarnation != world.identity
            || overlay(world, target.entity).is_none_or(|(incarnation, mode)| {
                incarnation != target.incarnation || mode == GuiOverlay::MODE_MANUAL
            })
            || !world
                .components
                .gui_behavior(target.entity.index() as usize)
                .is_some_and(|behavior| behavior.visible)
        {
            return Ok(());
        }
        context
            .world
            .apply_authored_commands(Some(self), &[visible_write(target.entity, false)])
    }
}

#[cfg(test)]
impl GuiSystem {
    /// Whether the next frame has overlay or hint work: an overlay to
    /// check, a delay or grace running, or hover or focus that changed.
    pub(crate) fn overlay_work_pending(&self) -> bool {
        !self.local.overlays.changed.is_empty()
            || self
                .local
                .overlays
                .hint
                .pending(self.local.presentation_revision)
    }
}

/// The write of an overlay's `GuiBehavior.visible`.
fn visible_write(entity: EntityId, open: bool) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_BEHAVIOR,
        field: FieldWrite {
            offset: std::mem::offset_of!(GuiBehavior, visible) as u32,
            value: FieldValue::Bool(open),
        },
    }
}

/// The input router's request to close one open overlay that is not manual,
/// queued for its World's next mutation boundary. It names the exact
/// `GuiOverlay` lifetime and changes nothing once that has ended, the
/// overlay is closed or its mode is manual.
pub struct GuiOverlayCommand {
    pub(in crate::world::systems::gui) overlay: GuiEntityTarget,
}

impl GuiOverlayCommand {
    /// Close the overlay whose exact `GuiOverlay` lifetime is `overlay`.
    pub fn close(overlay: GuiEntityTarget) -> Self {
        Self {
            overlay,
        }
    }
}
