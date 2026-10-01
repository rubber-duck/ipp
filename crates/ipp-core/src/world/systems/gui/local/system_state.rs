use super::component::CONTROL_COMPONENTS;
use super::control::{component_incarnation, eligibility, entity_control};
use super::*;
use crate::systems::SystemCommitContext;
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, EntityId};
use std::collections::BTreeSet;
use std::sync::Arc;

/// GUI System state: focus, pointer feedback, native text and effect
/// publication. Control values, scroll state and eligibility are component
/// fields; nothing here mirrors them.
#[derive(Default)]
pub(in crate::world::systems::gui) struct GuiLocalState {
    pub(in crate::world::systems::gui) observations: super::super::observations::GuiEffectPublisher,
    pub(super) pointers: Vec<super::interaction::GuiPointerFeedback>,
    /// Logical focus and the physical input session owning it; focus set by a
    /// `GuiAction` command has no owner.
    pub(super) focus: Option<(
        GuiEntityTarget,
        Option<crate::services::gui_input::GuiInputSession>,
    )>,
    pub(super) focus_visible: bool,
    pub(super) native_text: Option<super::text::GuiNativeText>,
    pub(super) native_generation: u64,
    /// Set while the GUI System writes the `text` of its own native record.
    pub(in crate::world::systems::gui) own_text_write: bool,
    /// Entities whose policy changed through evaluated writes since the last refresh.
    eligibility_dirty: BTreeSet<EntityId>,
    initialized: bool,
    /// Dirty counter of GUI System state that paint reads: focus, pointer
    /// feedback and native text.
    pub(in crate::world::systems::gui) presentation_revision: u64,
    /// Dirty counter of the displayed native text.
    pub(in crate::world::systems::gui) text_revision: u64,
}

/// Whether a focus owner is live; focus without an owner has no session to end.
pub(super) fn live(owner: &Option<crate::services::gui_input::GuiInputSession>) -> bool {
    owner
        .as_ref()
        .is_none_or(crate::services::gui_input::GuiInputSession::is_live)
}

impl GuiLocalState {
    pub(in crate::world::systems::gui) fn focus_visible(&self, target: GuiEntityTarget) -> bool {
        self.focus_visible
            && self
                .focus
                .as_ref()
                .is_some_and(|(focused, owner)| *focused == target && live(owner))
    }

    /// Logical focus of a live owner, as the `GuiFocus` System query reports it.
    pub(in crate::world::systems::gui) fn focus_record(&self) -> Option<GuiFocusRecord> {
        let (target, owner) = self.focus.as_ref()?;
        live(owner).then_some(GuiFocusRecord {
            target: *target,
            visible: self.focus_visible,
        })
    }

    /// Whether native text currently displays a provisional run over its text.
    pub(super) fn composing(&self) -> bool {
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

    pub(super) fn presentation_changed(&mut self, text: bool) {
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
        super::eligibility::refresh_eligibility(&mut world.components, &world.state, &roots);
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
        super::eligibility::refresh_eligibility(&mut world.components, &world.state, &roots);
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
        }
        roots.append(&mut self.eligibility_dirty);
        if !roots.is_empty() {
            super::eligibility::refresh_eligibility(
                &mut context.world_data.components,
                &context.staged.entities_state,
                &roots,
            );
        }
        self.refresh_native_text(context);
        if context.is_evaluated() {
            self.revalidate_focus(context.world_data, &context.staged.entities_state);
        }
        self.revalidate_interactions(context.world_data, &context.staged.entities_state);
    }

    /// A write to the focused input's `text` that the native record did not
    /// make replaces the native record and advances its generation.
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
        if Arc::ptr_eq(&input.text, &native.state.text) {
            return;
        }
        let text = input.text.clone();
        self.native_generation = self
            .native_generation
            .checked_add(1)
            .expect("GUI native text generation exhausted");
        let native = self.native_text.as_mut().expect("native text record");
        native.state = super::text::GuiNativeText::state(target, text, self.native_generation);
        self.presentation_changed(true);
    }

    pub(in crate::world::systems::gui) fn revalidate_focus(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
    ) {
        if let Some((target, owner)) = &self.focus {
            let valid = live(owner)
                && entity_control(world, state, target.entity)
                    .is_some_and(|control| control.target == *target)
                && eligibility(world, target.entity).eligible();
            if !valid {
                let composing = self.composing();
                self.focus = None;
                self.native_text = None;
                self.presentation_changed(composing);
            }
        }
    }
}
