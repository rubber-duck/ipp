use super::local::composites::overlay::GuiOverlayState;
use super::local::controls::identity::{
    component_incarnation, eligibility, entity_control, focus_parts, focusable,
};
use super::local::controls::native_text::GuiNativeText;
use super::local::controls::number::GuiNumberRepeat;
use super::local::{
    CONTROL_COMPONENTS, GuiEntityTarget, GuiFocusRecord, GuiPointerFeedback, refresh_eligibility,
};
use super::observations::GuiEffectPublisher;
use crate::systems::SystemCommitContext;
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, EntityId};
use std::collections::BTreeSet;
use std::sync::Arc;

/// GUI System state: focus, pointer feedback, active items, overlay
/// invokers, hint delays, held step repeats, native text and effect
/// publication. Control values, scroll state, eligibility and overlays' open
/// state are component fields; nothing here mirrors them.
#[derive(Default)]
pub(in crate::world::systems::gui) struct GuiLocalState {
    pub(in crate::world::systems::gui) observations: GuiEffectPublisher,
    pub(in crate::world::systems::gui) pointers: Vec<GuiPointerFeedback>,
    /// Logical focus and the physical input session owning it; focus set by a
    /// `GuiAction` command has no owner, and keeps none while routed input
    /// focuses the same control again.
    pub(in crate::world::systems::gui) focus: Option<(
        GuiEntityTarget,
        Option<crate::services::gui_input::GuiInputSession>,
    )>,
    /// The focus part of `focus`'s control that focus names, such as a
    /// range's thumb; 0 for a control with one part and without focus.
    pub(in crate::world::systems::gui) focus_part: u32,
    pub(in crate::world::systems::gui) focus_visible: bool,
    /// Each group's active item: the group entity and its item, an eligible
    /// control that does not take focus.
    pub(in crate::world::systems::gui) active: Vec<(EntityId, GuiEntityTarget)>,
    /// The control focus moved to at this frame's mutation boundary. Layout
    /// scrolls it into view in the same frame, and the System forgets it
    /// when the frame finishes.
    pub(in crate::world::systems::gui) reveal: Option<EntityId>,
    /// Open light and modal overlays with their invokers, and hint timing.
    pub(in crate::world::systems::gui) overlays: GuiOverlayState,
    pub(in crate::world::systems::gui) native_text: Option<GuiNativeText>,
    pub(in crate::world::systems::gui) native_generation: u64,
    /// The held step part of a numeric text input and its repeat timing.
    pub(in crate::world::systems::gui) number_repeat: Option<GuiNumberRepeat>,
    /// Numeric edits focus left when the runtime moved it, to commit at the
    /// next write the System makes.
    pub(in crate::world::systems::gui) number_commits: Vec<(GuiEntityTarget, Arc<str>)>,
    /// Numeric edits that ended without a commit, to publish as discarded.
    pub(in crate::world::systems::gui) number_discards: Vec<(GuiEntityTarget, Arc<str>)>,
    /// Set while the GUI System writes the `text` of its own native record.
    pub(in crate::world::systems::gui) own_text_write: bool,
    /// Entities whose policy changed through evaluated writes since the last refresh.
    eligibility_dirty: BTreeSet<EntityId>,
    /// Entities whose evaluated eligibility fields changed since skin motion
    /// last took them.
    pub(in crate::world::systems::gui) eligibility_changed: BTreeSet<EntityId>,
    initialized: bool,
    /// Dirty counter of GUI System state that paint reads: focus, pointer
    /// feedback and native text.
    pub(in crate::world::systems::gui) presentation_revision: u64,
    /// Dirty counter of the displayed native text.
    pub(in crate::world::systems::gui) text_revision: u64,
}

/// Whether a focus owner is live; focus without an owner has no session to end.
pub(in crate::world::systems::gui) fn live(
    owner: &Option<crate::services::gui_input::GuiInputSession>,
) -> bool {
    owner
        .as_ref()
        .is_none_or(crate::services::gui_input::GuiInputSession::is_live)
}

impl GuiLocalState {
    pub(in crate::world::systems::gui) fn focus_visible(&self, target: GuiEntityTarget) -> bool {
        self.focus_ring(target).is_some()
    }

    /// The focused part of `target` while it shows the focus ring, which
    /// paints on that part.
    pub(in crate::world::systems::gui) fn focus_ring(
        &self,
        target: GuiEntityTarget,
    ) -> Option<u32> {
        (self.focus_visible
            && self
                .focus
                .as_ref()
                .is_some_and(|(focused, owner)| *focused == target && live(owner)))
        .then_some(self.focus_part)
    }

    /// Logical focus of a live owner, as the `GuiFocus` System query reports it.
    pub(in crate::world::systems::gui) fn focus_record(&self) -> Option<GuiFocusRecord> {
        let (target, owner) = self.focus.as_ref()?;
        live(owner).then_some(GuiFocusRecord {
            target: *target,
            visible: self.focus_visible,
            part: self.focus_part,
        })
    }

    /// Logical focus of a live owner with its part, whether a command or the
    /// runtime set it, so that no input session owns it, and whether it shows
    /// the ring.
    pub(in crate::world::systems::gui) fn focus_owner(
        &self,
    ) -> Option<(GuiEntityTarget, u32, bool, bool)> {
        let (target, owner) = self.focus.as_ref()?;
        live(owner).then_some((
            *target,
            self.focus_part,
            owner.is_none(),
            self.focus_visible,
        ))
    }

    /// Whether native text currently displays a provisional run over its text.
    pub(in crate::world::systems::gui) fn composing(&self) -> bool {
        self.native_text
            .as_ref()
            .is_some_and(|native| native.state.composition.is_some())
    }

    /// Entity whose native text record is displayed, if any.
    pub(in crate::world::systems::gui) fn native_text_entity(&self) -> Option<EntityId> {
        self.native_text
            .as_ref()
            .map(|native| native.state.fence.target.entity)
    }

    pub(in crate::world::systems::gui) fn presentation_changed(&mut self, text: bool) {
        self.presentation_revision = self
            .presentation_revision
            .checked_add(1)
            .expect("GUI presentation revision exhausted");
        if text {
            self.text_revision = self
                .text_revision
                .checked_add(1)
                .expect("GUI text layout revision exhausted");
        }
    }

    /// Evaluate every eligibility field once, before the first evaluation.
    pub(in crate::world::systems::gui) fn initialize(&mut self, world: &mut WorldSimulationState) {
        if self.initialized {
            return;
        }
        let roots: BTreeSet<_> = world
            .state
            .entities
            .keys()
            .copied()
            .filter(|&entity| {
                world
                    .state
                    .links
                    .effective(entity)
                    .is_none_or(|link| link.parent.is_none())
            })
            .collect();
        refresh_eligibility(
            &mut world.components,
            &world.state,
            &roots,
            &mut self.eligibility_changed,
        );
        self.eligibility_dirty.clear();
        self.initialized = true;
    }

    /// Record an evaluated policy write for the next eligibility refresh.
    pub(in crate::world::systems::gui) fn policy_changed(&mut self, entity: EntityId) {
        self.eligibility_dirty.insert(entity);
    }

    /// Refresh the subtrees whose policy changed through evaluated writes.
    pub(in crate::world::systems::gui) fn refresh_pending_eligibility(
        &mut self,
        world: &mut WorldSimulationState,
    ) {
        if self.eligibility_dirty.is_empty() {
            return;
        }
        let roots = std::mem::take(&mut self.eligibility_dirty);
        refresh_eligibility(
            &mut world.components,
            &world.state,
            &roots,
            &mut self.eligibility_changed,
        );
    }

    pub(in crate::world::systems::gui) fn before_commit(
        &mut self,
        context: &SystemCommitContext<'_>,
    ) {
        self.invalidate_interactions(&context.staged.entities_state);
        if let Some((target, _)) = &self.focus
            && CONTROL_COMPONENTS.iter().any(|&component| {
                context
                    .staged
                    .changed
                    .get(&(target.entity, component))
                    .is_some()
            })
            && entity_control(
                context.world_data,
                &context.staged.entities_state,
                target.entity,
            )
            .is_none_or(|control| control.target != *target)
        {
            self.focus = None;
            self.focus_part = 0;
            self.native_text = None;
        }
    }

    pub(in crate::world::systems::gui) fn after_commit(
        &mut self,
        context: &mut SystemCommitContext<'_>,
    ) {
        let mut roots: BTreeSet<_> = context.changed_entity_links().collect();
        for (entity, component) in context.changed_components() {
            if component == ComponentValue::GUI_BEHAVIOR
                || (CONTROL_COMPONENTS.contains(&component)
                    && !context.retains_component(entity, component))
            {
                roots.insert(entity);
            }

            // A new, removed or changed overlay may have opened or closed.
            if component == ComponentValue::GUI_OVERLAY {
                self.overlays.changed.insert(entity);
            }
        }
        roots.append(&mut self.eligibility_dirty);
        if !roots.is_empty() {
            refresh_eligibility(
                &mut context.world_data.components,
                &context.staged.entities_state,
                &roots,
                &mut self.eligibility_changed,
            );
        }
        self.refresh_native_text(context);
        if context.is_evaluated() {
            self.revalidate_focus(context.world_data, &context.staged.entities_state);
        }
        self.revalidate_interactions(context.world_data, &context.staged.entities_state);
        self.revalidate_active(context.world_data, &context.staged.entities_state);
    }

    /// A write to the focused input that the native record did not make
    /// replaces the native record and advances its generation: a change of
    /// its `text`, or of a numeric input's formatted number, or of whether it
    /// holds a number.
    fn refresh_native_text(&mut self, context: &SystemCommitContext<'_>) {
        if self.own_text_write {
            return;
        }
        let Some(native) = &self.native_text else {
            return;
        };
        let target = native.state.fence.target;
        if context
            .staged
            .changed
            .get(&(target.entity, ComponentValue::GUI_TEXT_INPUT))
            .is_none()
            || component_incarnation(
                &context.staged.entities_state,
                target.entity,
                ComponentValue::GUI_TEXT_INPUT,
            ) != Some(target.incarnation)
        {
            return;
        }
        let Some(input) = context
            .world_data
            .components
            .gui_text_input(target.entity.index() as usize)
        else {
            return;
        };
        let unchanged = match &native.basis {
            Some(basis) => input.numeric && *input.formatted() == **basis,
            None => !input.numeric && Arc::ptr_eq(&input.text, &native.state.text),
        };
        if unchanged {
            if native.state.masked != input.masked {
                self.native_text
                    .as_mut()
                    .expect("native text record")
                    .state
                    .masked = input.masked;
                self.presentation_changed(true);
            }
            return;
        }
        self.discard_native_edit();
        self.native_generation = self
            .native_generation
            .checked_add(1)
            .expect("GUI native text generation exhausted");
        let native = self.native_text.as_mut().expect("native text record");
        let owner = native.owner.clone();
        *native = GuiNativeText::new(target, owner, input, self.native_generation);
        self.presentation_changed(true);
    }

    pub(in crate::world::systems::gui) fn revalidate_focus(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
    ) {
        if let Some((target, owner)) = &self.focus {
            let control = entity_control(world, state, target.entity)
                .filter(|control| control.target == *target);
            let valid = live(owner)
                && control.is_some()
                && eligibility(world, target.entity).eligible()
                && focusable(world, target.entity);
            if !valid {
                let composing = self.composing();
                self.discard_native_edit();
                self.focus = None;
                self.focus_part = 0;
                self.native_text = None;
                self.presentation_changed(composing);
            } else if control.is_some_and(|control| self.focus_part >= focus_parts(world, control))
            {
                // A control that lost the focused part, as a range made a
                // single slider, keeps focus on its first part.
                self.focus_part = 0;
                self.presentation_changed(false);
            }
        }

        // Focus a command set outlives the session holding its native record;
        // the record and any provisional run end with that session.
        if self
            .native_text
            .as_ref()
            .is_some_and(|native| !native.owner.is_live())
        {
            let composing = self.composing();
            self.discard_native_edit();
            self.native_text = None;
            self.presentation_changed(composing);
        }
    }
}
