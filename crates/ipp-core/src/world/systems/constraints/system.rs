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
        let mut capabilities = crate::systems::SystemCapabilities::new(
            [
                crate::ComponentValue::SCALAR,
                crate::ComponentValue::LINEAR_DRIVER,
            ],
            [crate::systems::WorldOperation::Constraints],
        );
        capabilities
            .components
            .push(crate::systems::SystemCapability::requiring(
                crate::ComponentValue::EXPRESSION_DRIVER,
                [crate::systems::asset_dependencies::AssetDependencySystem::ID],
            ));
        capabilities
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
        self.reconcile_expressions(context.world_data, context.staged, true);
        self.reconcile(context.world_data, context.staged)
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.reconcile_expressions(context.world_data, context.staged, false);
        let _ = self.reconcile(context.world_data, context.staged);
        if self.state.numeric_dirty {
            self.state.numeric.clear();
            self.state.order.clear();
        }
    }

    fn prepare_evaluation(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        if self.state.numeric_dirty {
            self.prepare_drivers(context.world.world, context.world.asset_acquisition);
        }
    }

    fn before_asset_release(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        if event.kind == crate::services::asset_management::AssetLifecycleKind::GraphicsInvalidated
        {
            return;
        }
        let mut affected = false;
        for binding in self
            .state
            .expressions
            .values_mut()
            .filter(|binding| binding.key == Some(event.key))
        {
            affected = true;
            if event.kind == crate::services::asset_management::AssetLifecycleKind::Removed {
                binding.removed_asset = true;
            }
            binding.runtime = None;
            binding.status.availability = super::ExpressionDriverAvailability::Unavailable;
            binding.status.state = super::ExpressionDriverState::Retained(
                super::ExpressionDriverReason::AssetUnavailable,
            );
            binding.status.recovered = false;
        }
        if affected {
            self.state.order.clear();
            self.state.numeric_dirty = true;
        }
    }

    fn asset_lifecycle(
        &mut self,
        context: &mut crate::systems::SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        use crate::services::asset_management::{AssetLoadStatus, expression::EXPRESSION_TYPE};
        if event.source.kind != EXPRESSION_TYPE {
            return;
        }

        for (&entity, binding) in &mut self.state.expressions {
            if binding.removed_asset {
                continue;
            }
            let Some(driver) = context
                .world
                .world
                .components
                .expression_driver(entity.index() as usize)
            else {
                continue;
            };
            let key = crate::world::systems::asset_dependencies::source_key_from_fields(
                context.world.asset_acquisition,
                context.world.world.id,
                binding.key,
                EXPRESSION_TYPE,
                &driver.expression_source,
                driver.expression_variant,
            );
            if key != Some(event.key) {
                continue;
            }

            binding.key = key;
            binding.status.availability = if event.representation.decoded {
                super::ExpressionDriverAvailability::Ready
            } else if matches!(
                event.status,
                AssetLoadStatus::Unloaded | AssetLoadStatus::Failed(_)
            ) {
                super::ExpressionDriverAvailability::Unavailable
            } else {
                super::ExpressionDriverAvailability::Pending
            };
            if !event.representation.decoded {
                binding.runtime = None;
                binding.status.state = super::ExpressionDriverState::Retained(
                    if matches!(event.status, AssetLoadStatus::Failed(_)) {
                        super::ExpressionDriverReason::AssetFailed
                    } else {
                        super::ExpressionDriverReason::AssetUnavailable
                    },
                );
                binding.status.recovered = false;
                self.state.order.clear();
            }
            self.state.numeric_dirty = true;
        }
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state.numeric.clear();
        self.state.order.clear();
        self.state.expressions.clear();
        self.state.bindings.clear();
        self.state.invalid.clear();
        self.state.declarations.clear();
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.evaluate(&mut context.world);
    }
}
