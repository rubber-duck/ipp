use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
};
use crate::world::systems::SystemCommitContext;
use crate::{ComponentValue, EntityId, ErrorReason};
use std::collections::BTreeSet;

/// Owner of focus, pointer interaction, native text state, local actions and
/// skin motion preparation of ordinary GUI entities. Control values are
/// component fields; this System writes them for actions and input and keeps
/// the eligibility fields current.
#[derive(Default)]
pub struct GuiSystem {
    pub(super) local: super::local::GuiLocalState,
    pub(super) motion: super::motion::GuiMotionState,
}

impl GuiSystem {
    /// Stable system identity.
    pub const ID: SystemId = SystemId("ipp.gui");

    pub(in crate::world::systems) fn local_presentation_revision(&self) -> u64 {
        self.local.presentation_revision
    }

    pub(in crate::world::systems) fn part_interaction(
        &self,
        target: super::local::GuiEntityTarget,
        interaction: super::local::GuiInteractionFlags,
    ) -> super::local::GuiPartInteraction {
        self.local.part_interaction(target, interaction)
    }

    pub(in crate::world::systems) fn interaction_flags(
        &self,
        target: super::local::GuiEntityTarget,
    ) -> super::local::GuiInteractionFlags {
        self.local.interaction_flags(target)
    }

    pub(in crate::world::systems) fn focused(&self, target: super::local::GuiEntityTarget) -> bool {
        self.local
            .focus_record()
            .is_some_and(|focus| focus.target == target)
    }

    pub(in crate::world::systems) fn focus_visible(
        &self,
        target: super::local::GuiEntityTarget,
    ) -> bool {
        self.local.focus_visible(target)
    }

    pub(in crate::world::systems) fn native_text_state(
        &self,
        target: super::local::GuiEntityTarget,
    ) -> Option<&super::local::GuiNativeTextState> {
        self.local.native_text(target)
    }

    pub(in crate::world::systems::gui) fn local_text_revision(&self) -> u64 {
        self.local.text_revision
    }

    pub(in crate::world::systems::gui) fn native_text_entity(&self) -> Option<EntityId> {
        self.local.native_text_entity()
    }

    pub(in crate::world::systems) fn motion_changes(&self) -> &[super::motion::GuiMotionOwner] {
        self.motion.changes()
    }

    pub(in crate::world::systems) fn pending_motion_entities(&self) -> BTreeSet<EntityId> {
        self.motion.pending_entities()
    }

    pub(in crate::world::systems) fn motion_request_source(
        &self,
        world: &crate::world::WorldSimulationState,
        owner: super::motion::GuiMotionOwner,
    ) -> Option<crate::services::asset_management::AssetSource> {
        super::motion::GuiMotionState::request_source(&self.local, world, owner)
    }

    pub(in crate::world) fn motion_work(&self) -> super::motion::GuiMotionPreparationWork {
        self.motion.statistics
    }
}

impl crate::WorldContext<'_> {
    /// `GuiFocus` System query: the World's logical focus, paged by target
    /// entity like the other inspection collections.
    pub fn gui_focus_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<super::local::GuiFocusRecord> {
        self.system::<GuiSystem>(GuiSystem::ID)
            .and_then(|gui| gui.local.focus_record())
            .filter(|record| {
                let entity = record.target.entity.to_bits();
                if target != 0 {
                    entity == target
                } else {
                    entity > after
                }
            })
            .into_iter()
            .take(limit)
            .collect()
    }

    /// `GuiPointers` System query: live pointer feedback records ordered by
    /// target entity, then pointer, paged by target entity. The page holds at
    /// least `limit` records when that many exist, completed with every
    /// remaining record of its last entity, so an entity cursor skips none.
    pub fn gui_pointer_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<super::local::GuiPointerRecord> {
        let mut page: Vec<super::local::GuiPointerRecord> = Vec::new();
        let records = self
            .system::<GuiSystem>(GuiSystem::ID)
            .map(|gui| gui.local.pointer_records())
            .unwrap_or_default();
        for record in records {
            let entity = record.target.entity.to_bits();
            let selected = if target != 0 {
                entity == target
            } else {
                entity > after
            };
            if !selected {
                continue;
            }
            if page.len() >= limit
                && page
                    .last()
                    .is_none_or(|last| last.target.entity != record.target.entity)
            {
                break;
            }
            page.push(record);
        }
        page
    }
}

/// Factory for GuiSystem.
#[derive(Default)]
pub struct GuiSystemFactory;

impl SystemFactory for GuiSystemFactory {
    fn id(&self) -> SystemId {
        GuiSystem::ID
    }

    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        let mut capabilities = crate::systems::SystemCapabilities::new(
            [
                ComponentValue::GUI_BEHAVIOR,
                ComponentValue::GUI_VIRTUAL_ITEM,
            ],
            [crate::systems::WorldOperation::Gui],
        );

        // Every control requires CanvasBounds, which the Canvas System that
        // paints it admits and writes.
        capabilities.components.extend(
            [
                ComponentValue::GUI_BUTTON,
                ComponentValue::GUI_CHECKBOX,
                ComponentValue::GUI_SLIDER,
                ComponentValue::GUI_TEXT_INPUT,
                ComponentValue::GUI_SCROLL_VIEW,
                ComponentValue::GUI_VIRTUAL_LIST,
            ]
            .map(|component| {
                crate::systems::SystemCapability::requiring(
                    component,
                    [crate::systems::canvas::CanvasSystem::ID],
                )
            }),
        );
        capabilities.components.extend(
            [
                ComponentValue::GUI_THEME,
                ComponentValue::GUI_SKIN,
                ComponentValue::GUI_FONT,
            ]
            .map(|component| {
                crate::systems::SystemCapability::requiring(
                    component,
                    [crate::systems::asset_dependencies::AssetDependencySystem::ID],
                )
            }),
        );
        capabilities
            .components
            .push(crate::systems::SystemCapability::requiring(
                ComponentValue::GUI_THEME_MOTION,
                [
                    crate::systems::animation::AnimationSystem::ID,
                    crate::systems::asset_dependencies::AssetDependencySystem::ID,
                ],
            ));
        capabilities
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[SystemDependency::After(
            crate::systems::surface::SurfaceSystem::ID,
        )]
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(GuiSystem::default()))
    }
}

impl System for GuiSystem {
    fn prepare_evaluation(&mut self, context: &mut crate::systems::SystemUpdateContext<'_, '_>) {
        self.local.initialize(context.world.world);
        self.local.refresh_pending_eligibility(context.world.world);
        let focus = self.local.motion_focus();
        self.local
            .revalidate_focus(context.world.world, &context.world.world.state);
        if focus != self.local.motion_focus()
            && let Some(entity) = focus
        {
            self.motion.dirty_entity(entity);
        }
        let revision = self.local.presentation_revision;
        self.local
            .revalidate_interactions(context.world.world, &context.world.world.state);
        if revision != self.local.presentation_revision {
            self.motion.dirty_watched();
        }
        self.motion.prepare(&self.local, &mut context.world);
    }

    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        for &(entity, component) in context.changed_components() {
            if component == ComponentValue::GUI_SKIN
                && context
                    .world_data
                    .components
                    .gui_skin(entity.index() as usize)
                    .is_some_and(|skin| skin.runtime.notifying_sample)
            {
                continue;
            }
            if component == ComponentValue::GUI_BEHAVIOR {
                self.local.policy_changed(entity);
            }
            self.motion.component_changed(entity, component);
        }
    }

    fn update(&mut self, context: &mut crate::systems::SystemUpdateContext<'_, '_>) {
        self.local.observations.prune();
        self.local.initialize(context.world.world);
        self.local
            .revalidate_focus(context.world.world, &context.world.world.state);
        self.local
            .revalidate_interactions(context.world.world, &context.world.world.state);
    }

    fn finish_update(
        &mut self,
        context: &mut crate::systems::SystemUpdateContext<'_, '_>,
        _report: &mut crate::WorldUpdateReport,
    ) {
        // Animated policy is sampled after this System's update.
        self.local.refresh_pending_eligibility(context.world.world);
    }

    fn apply_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Option<Result<(), ErrorReason>> {
        let crate::Command::GuiAction {
            target,
            action,
        } = context.command
        else {
            return None;
        };
        Some(self.apply_action(context, target, action))
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.motion.before_commit(context);
        self.local.before_commit(context);
    }

    fn command_world_references(
        &self,
        command: &dyn std::any::Any,
        visit: &mut dyn FnMut(crate::WorldRef),
    ) {
        if let Some(command) = command.downcast_ref::<super::local::GuiLocalCommand>() {
            command.world_references(visit);
        }
    }

    fn command_ready(&self, command: &dyn std::any::Any) -> bool {
        command
            .downcast_ref::<super::local::GuiLocalCommand>()
            .is_none_or(super::local::GuiLocalCommand::ready)
    }

    fn command(
        &mut self,
        context: &mut crate::systems::SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        if let Some(command) = command.downcast_ref::<super::observations::GuiObservationCommand>()
        {
            self.local.observations.command(
                crate::WorldRef {
                    id: context.world.world.id,
                    incarnation: context.world.world.identity,
                },
                command,
            );
            return Ok(());
        }
        if let Some(command) = command.downcast_ref::<super::local::GuiLocalCommand>() {
            return self.local_command(context, command);
        }
        Err(ErrorReason::InvalidValue)
    }

    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.motion.after_commit(context);
        let revision = self.local.presentation_revision;
        self.local.after_commit(context);
        if revision != self.local.presentation_revision {
            self.motion.dirty_watched();
        }
    }
}

#[cfg(test)]
#[path = "motion_lifetime_tests.rs"]
mod motion_lifetime_tests;
