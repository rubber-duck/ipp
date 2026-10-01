//! ConstraintSystem: factory configuration and exclusively owned per-world state.

use super::ConstraintSystemState;
use crate::systems::{
    System, SystemFactory, SystemId, SystemInitContext, SystemInitError, SystemTeardownContext,
    SystemUpdateContext,
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

    // The Scalar values that drivers read and write are admitted with them;
    // no other System owns the Scalar component.
    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        crate::systems::SystemCapabilities::new(
            [
                crate::ComponentValue::SCALAR,
                crate::ComponentValue::LINEAR_DRIVER,
            ],
            [crate::systems::WorldOperation::Constraints],
        )
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(ConstraintSystem::default()))
    }
}

impl System for ConstraintSystem {
    fn after_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        self.reconcile(context.world_data, context.staged)
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        if context.changed_components().any(|(_, component)| {
            matches!(
                component,
                crate::ComponentValue::SCALAR | crate::ComponentValue::LINEAR_DRIVER
            )
        }) {
            self.state.numeric.clear();
            self.state.targets.clear();
            self.state.numeric_dirty = true;
        }
        let _ = self.reconcile(context.world_data, context.staged);
    }

    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        if self.state.numeric_dirty {
            self.prepare_numeric(&context.world_data.components);
        }
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state.numeric.clear();
        self.state.targets.clear();
        self.state.bindings.clear();
        self.state.invalid.clear();
        self.state.declarations.clear();
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        // Targets take absolute values; contributions others keep to them
        // apply again on top.
        context.world.before_absolute_writes(&self.state.targets);
        self.evaluate(context.world.world);
    }
}
