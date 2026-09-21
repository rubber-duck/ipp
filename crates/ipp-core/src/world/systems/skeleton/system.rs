//! SkeletonSystem: factory configuration and exclusively owned per-world state.

use super::SkeletonSystemState;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct SkeletonSystem {
    pub(in crate::world) state: SkeletonSystemState,
}

impl SkeletonSystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.skeleton");
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct SkeletonSystemFactory;

impl SystemFactory for SkeletonSystemFactory {
    fn id(&self) -> SystemId {
        SkeletonSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(SystemId("ipp.asset-dependencies")),
            SystemDependency::After(SystemId("ipp.constraints")),
        ]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        context.dependency::<crate::systems::asset_dependencies::AssetDependencySystem>(
            crate::systems::asset_dependencies::AssetDependencySystem::ID,
        )?;
        Ok(Box::new(SkeletonSystem::default()))
    }
}

impl System for SkeletonSystem {
    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.state.components.after_commit(
            context,
            crate::ComponentValue::SKELETON,
            crate::components::registry::ComponentStorage::skeleton_ptr,
        );
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.state
            .components
            .before_commit(context, crate::ComponentValue::SKELETON);
    }

    fn validate_commit(
        &self,
        context: &crate::systems::SystemCommitContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        super::validate_changes(context.world_data, context.assets, context.staged)?;
        let _ = context;
        Ok(())
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state.skeleton_diagnostics.clear();
    }

    fn prepare_evaluation(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.prepare(&mut context.world);
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.evaluate(&mut context.world);
    }
}
