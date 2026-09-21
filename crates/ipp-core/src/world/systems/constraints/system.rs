//! ConstraintSystem: factory configuration and exclusively owned per-world state.

use super::ConstraintSystemState;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct ConstraintSystem {
    pub(in crate::world) state: ConstraintSystemState,
}

impl ConstraintSystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.constraints");
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct ConstraintSystemFactory;

impl SystemFactory for ConstraintSystemFactory {
    fn id(&self) -> SystemId {
        ConstraintSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::After(SystemId("ipp.animation")),
            SystemDependency::After(SystemId("ipp.state-overlay")),
        ]
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(ConstraintSystem::default()))
    }
}

impl System for ConstraintSystem {
    fn restore_component_input(
        &self,
        entity: crate::EntityId,
        incarnation: u64,
        value: &mut crate::ComponentValue,
    ) {
        if let crate::ComponentValue::Scalar(value) = value
            && self.state.restores_active
            && let Some(original) = self
                .state
                .restores
                .get(&entity)
                .filter(|original| original.incarnation == incarnation)
        {
            *value = original.base;
        }
    }

    fn after_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        self.reconcile(&context.world_data.components, context.staged)
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        if context.changed_components().any(|(_, component)| {
            matches!(
                component,
                crate::ComponentValue::SCALAR | crate::ComponentValue::LINEAR_DRIVER
            )
        }) {
            self.state.numeric.clear();
            self.state.numeric_dirty = true;
        }
        let _ = self.reconcile(&context.world_data.components, context.staged);
        self.state.restores.retain(|entity, original| {
            if !self.state.bindings.contains_key(entity) {
                return false;
            }
            if !context.is_evaluated()
                && context
                    .staged
                    .changed
                    .contains_key(&(*entity, crate::ComponentValue::SCALAR))
            {
                return false;
            }
            context
                .staged
                .entities
                .get(entity)
                .and_then(|record| record.input(crate::ComponentValue::SCALAR))
                .is_some_and(|input| input.incarnation == original.incarnation)
        });
    }

    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        if self.state.numeric_dirty {
            self.prepare_numeric(&context.world_data.components);
        }
    }

    fn validate_commit(
        &self,
        context: &crate::systems::SystemCommitContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        #[cfg(debug_assertions)]
        self.validate(&context.world_data.components, context.staged)?;
        let _ = context;
        Ok(())
    }

    fn prepare_mutation(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.restore_inputs(context.world.world);
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state.restores.clear();
        self.state.bindings.clear();
        self.state.declarations.clear();
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.evaluate(context.world.world);
    }
}
