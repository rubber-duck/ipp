//! AnimationSystem: factory configuration and exclusively owned per-world state.

use super::AnimationSystemState;
use crate::systems::{
    System, SystemAssetContext, SystemCommandContext, SystemCommitContext, SystemDependency,
    SystemFactory, SystemId, SystemInitContext, SystemInitError, SystemTeardownContext,
    SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct AnimationSystem {
    pub(in crate::world) state: AnimationSystemState,
    motion: super::gui_motion::GuiMotionAnimations,
    gui: Option<crate::systems::SystemDependencyBinding<crate::systems::gui::GuiSystem>>,
}

impl AnimationSystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.animation");

    pub(in crate::world) fn motion_work(
        &self,
    ) -> crate::systems::gui::motion::GuiMotionSamplingWork {
        self.motion.statistics
    }

    /// Inspect ordinary controllers in deterministic identity order.
    pub(in crate::world) fn ordinary_controllers(&self) -> Vec<super::AnimationControllerSnapshot> {
        self.state
            .controllers
            .values()
            .map(|controller| controller.snapshot.clone())
            .collect()
    }

    /// Read a bounded ordinary-controller page in identity order.
    pub(in crate::world) fn ordinary_controller_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<super::AnimationControllerSnapshot> {
        if target != 0 {
            return self
                .ordinary_controller(super::AnimationControllerId::from_bits(target))
                .into_iter()
                .take(limit)
                .collect();
        }
        self.state
            .controllers
            .range((
                std::ops::Bound::Excluded(super::AnimationControllerId::from_bits(after)),
                std::ops::Bound::Unbounded,
            ))
            .take(limit)
            .map(|(_, controller)| controller.snapshot.clone())
            .collect()
    }

    /// Inspect one ordinary controller's descriptions and shared clock.
    pub(in crate::world) fn ordinary_controller(
        &self,
        id: super::AnimationControllerId,
    ) -> Option<super::AnimationControllerSnapshot> {
        Some(self.state.controllers.get(&id)?.snapshot.clone())
    }
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct AnimationSystemFactory;

impl SystemFactory for AnimationSystemFactory {
    fn id(&self) -> SystemId {
        AnimationSystem::ID
    }

    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        let mut capabilities = crate::systems::SystemCapabilities::new(
            [],
            [crate::systems::WorldOperation::Animation],
        );
        capabilities
            .operations
            .push(crate::systems::SystemCapability::requiring(
                crate::systems::WorldOperation::JointAnimation,
                [crate::systems::skeleton::SkeletonSystem::ID],
            ));
        capabilities
    }

    // Constraints write their targets first; animation contributions apply on top.
    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::After(crate::systems::constraints::ConstraintSystem::ID),
            SystemDependency::After(crate::systems::gui::GuiSystem::ID),
        ]
    }

    fn capacity_hints(&self) -> crate::WorldSystemCapacityHints {
        crate::WorldSystemCapacityHints::new([("controllers", 128)])
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(AnimationSystem {
            gui: context
                .dependency::<crate::systems::gui::GuiSystem>(crate::systems::gui::GuiSystem::ID)
                .ok(),
            ..Default::default()
        }))
    }
}

impl System for AnimationSystem {
    fn command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), crate::ErrorReason> {
        let command = command
            .downcast_ref::<super::world_api::AnimationCommand>()
            .ok_or(crate::ErrorReason::InvalidValue)?;
        let mut access = super::AnimationAccess {
            system: self,
            context: &mut context.world,
        };
        match command.clone() {
            super::world_api::AnimationCommand::Controller {
                request_id,
                command,
            } => access.apply_animation_controller_command(request_id, command),
            super::world_api::AnimationCommand::Playback {
                id,
                control,
            } => access.control_playback(id, control),
        }
    }

    fn after_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        // A placement written over a structural driver's lasts until the
        // driver's next frame, like a write to a field an absolute driver writes.
        if let crate::Command::PlaceEntity {
            entity,
            ..
        } = context.command
            && let Ok(entity) = context.staged.resolve(entity, context.aliases)
            && context.staged.links.operation_changed.contains(&entity)
        {
            self.state.resample_structural_target(entity);
        }
        Ok(())
    }

    fn before_absolute_writes(&mut self, fields: &[(crate::EntityId, u16, u32)]) {
        self.forget_overwritten(fields);
    }

    fn before_asset_release(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.motion
            .reconcile_sources(&context.world, self.gui, event);
        self.motion.release(&mut context.world, event);
        use crate::services::asset_management::AssetLifecycleKind;
        if event.kind == AssetLifecycleKind::StatusChanged {
            self.suspend_asset(
                context.world.world,
                context.world.asset_acquisition,
                event.key,
            );
            return;
        }
        if event.kind != AssetLifecycleKind::Removed {
            return;
        }
        super::AnimationAccess {
            system: self,
            context: &mut context.world,
        }
        .invalidate_structural_asset(event.key);
        let restorations = self.invalidate_asset(
            context.world.world,
            context.world.asset_acquisition,
            event.key,
        );
        for (entity, value) in restorations {
            context.restore_evaluated_component(entity, value);
        }
    }

    fn validate_commit(&self, context: &SystemCommitContext<'_>) -> Result<(), crate::ErrorReason> {
        super::AnimationReadAccess {
            animation: &self.state,
            world: context.world_data,
            state: context.staged,
            asset_acquisition: context.assets,
        }
        .validate_animation_changes(context.staged)?;
        let _ = context;
        Ok(())
    }

    fn asset_lifecycle(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.motion
            .reconcile_sources(&context.world, self.gui, event);
        self.motion.asset_lifecycle(&mut context.world, event);
    }

    fn finish_update(
        &mut self,
        _context: &mut SystemUpdateContext<'_, '_>,
        report: &mut crate::WorldUpdateReport,
    ) {
        report
            .playback_events
            .append(&mut self.state.playback_events);
        report
            .animation_controller_outcomes
            .append(&mut self.state.controller_outcomes);
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.motion.before_commit(context);
        self.invalidate_changes(context);
    }

    fn after_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.motion
            .flush_demand(context.world_data.id, context.assets);
    }

    fn save_persistent_state(
        &self,
        context: &mut crate::systems::SystemSaveContext<'_>,
    ) -> Result<Option<crate::systems::SystemPersistentState>, String> {
        let state = self.save_persistent_state(context.ids, context.bytes, context.max_bytes)?;
        state.validate_entities(context.entities)?;
        state.encode(context.max_bytes).map(Some)
    }

    fn load_persistent_state(
        &mut self,
        context: &mut crate::systems::SystemLoadContext<'_, '_>,
        state: Option<&crate::systems::SystemPersistentState>,
    ) -> Result<(), String> {
        let persistent = match state {
            Some(bytes) => super::AnimationPersistentState::decode(bytes, context.max_bytes)?
                .remap_entities(context.ids)?,
            None => super::AnimationPersistentState::default(),
        };
        super::AnimationAccess {
            system: self,
            context: &mut context.world,
        }
        .restore_animation_controllers(persistent)
        .map_err(|reason| format!("Invalid persistent animation: {reason:?}"))
    }

    fn reserve_capacity(
        &mut self,
        hints: &crate::WorldSystemCapacityHints,
    ) -> Result<(), crate::ErrorReason> {
        let _ = hints;
        Ok(())
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state.controllers.clear();
        self.state.rebuild_target_index();
        self.state.affected_controllers.clear();
        self.state.controller_outcomes.clear();
        self.state.playback_events.clear();
        self.state.animation_sources.clear();
        self.motion = Default::default();
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let dt = context.dt();
        let changes = self
            .gui
            .and_then(|binding| context.dependency(binding))
            .map_or_else(Vec::new, |gui| gui.motion_changes().to_vec());
        self.motion.update(&mut context.world, dt, &changes);

        super::AnimationAccess {
            system: self,
            context: &mut context.world,
        }
        .evaluate_animation(dt);
    }
}
