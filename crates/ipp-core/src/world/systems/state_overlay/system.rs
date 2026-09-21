//! StateOverlaySystem: factory configuration and exclusively owned per-world state.

use super::StateOverlaySystemState;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct StateOverlaySystem {
    pub(in crate::world) state: StateOverlaySystemState,
    batch: super::StateOverlayBatch,
}

impl StateOverlaySystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.state-overlay");
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct StateOverlaySystemFactory;

impl SystemFactory for StateOverlaySystemFactory {
    fn id(&self) -> SystemId {
        StateOverlaySystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[]
    }

    fn capacity_hints(&self) -> crate::WorldSystemCapacityHints {
        crate::WorldSystemCapacityHints::new([("handles", 256)])
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(StateOverlaySystem::default()))
    }
}

impl System for StateOverlaySystem {
    fn include_entity_in_snapshot(&self, entity: crate::EntityId) -> bool {
        !self.state.owns_entity(entity)
    }

    fn begin_batch(&mut self) {
        self.batch = Default::default();
    }

    fn finish_batch(&mut self, outcome: &mut crate::BatchOutcome) {
        outcome.state_overlays.append(&mut self.batch.created);
    }

    fn before_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        let mut access = super::StateOverlayMutationAccess {
            staged: context.staged,
            state_overlays: &mut self.state,
        };
        access.stage_overlay_inputs(
            &context.world_data.components,
            context.command,
            &self.batch,
        )?;
        if let crate::Command::Delete {
            entity,
        } = context.command
        {
            let entity = access.resolve(*entity, context.aliases)?;
            access.invalidate_overlay_bindings(entity);
        }
        Ok(())
    }

    fn apply_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Option<Result<(), crate::ErrorReason>> {
        use crate::Command;
        if !matches!(
            context.command,
            Command::CreateStateOverlayOwner { .. }
                | Command::ReleaseStateOverlayOwner { .. }
                | Command::AttachEntityOverlayBinding { .. }
                | Command::ReleaseEntityOverlayBinding { .. }
                | Command::AttachComponentStateOverlay { .. }
                | Command::UpdateComponentStateOverlay { .. }
                | Command::UpdateDynamicComponentStateOverlay { .. }
                | Command::ReleaseComponentStateOverlay { .. }
        ) {
            return None;
        }
        let mut access = super::StateOverlayMutationAccess {
            staged: context.staged,
            state_overlays: &mut self.state,
        };
        Some(access.apply_state_overlay(
            context.command,
            context.aliases,
            &mut self.batch,
            context.world_data.limits,
        ))
    }

    fn after_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        let mut access = super::StateOverlayMutationAccess {
            staged: context.staged,
            state_overlays: &mut self.state,
        };
        let result = access.resolve_layers(&context.world_data.components);
        if result.is_err() && context.world_data.forced_cleanup {
            let affected: Vec<_> = access.staged.dirty.iter().copied().collect();
            for (entity, component) in affected {
                if let Some(layer) = access
                    .staged
                    .entities
                    .get_mut(&entity)
                    .and_then(|record| record.layers.get_mut(&component))
                {
                    layer.inputs.deactivate();
                }
            }
        }
        if self.state.diagnostics.len() > crate::MAX_STATE_OVERLAY_DIAGNOSTICS {
            return result.and(Err(crate::ErrorReason::Capacity));
        }
        result
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        let mut access = super::StateOverlayMutationAccess {
            staged: context.staged,
            state_overlays: &mut self.state,
        };
        let inactive: Vec<_> = access
            .changed
            .keys()
            .copied()
            .filter(|&(entity, component)| {
                access
                    .entities
                    .get(&entity)
                    .and_then(|record| record.input(component))
                    .is_none()
            })
            .collect();
        for (entity, component) in inactive {
            access.invalidate_component_state_overlays(entity, component);
        }
    }

    fn finish_update(
        &mut self,
        _context: &mut crate::systems::SystemUpdateContext<'_, '_>,
        report: &mut crate::WorldUpdateReport,
    ) {
        report.diagnostics.extend(
            self.state.diagnostics.drain(
                ..self
                    .state
                    .diagnostics
                    .len()
                    .min(crate::MAX_STATE_OVERLAY_DIAGNOSTICS),
            ),
        );
    }

    fn reserve_capacity(
        &mut self,
        hints: &crate::WorldSystemCapacityHints,
    ) -> Result<(), crate::ErrorReason> {
        self.state.registry.reserve(hints.get("handles"))
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state = Default::default();
    }

    fn update(&mut self, _context: &mut SystemUpdateContext<'_, '_>) {}
}
