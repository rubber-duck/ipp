//! GeometrySystem: factory configuration and exclusively owned per-world state.

use super::GeometrySystemState;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct GeometrySystem {
    pub(in crate::world) state: GeometrySystemState,
    refresh: bool,
    pub(super) programs_ready: bool,
}

impl GeometrySystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.geometry");

    fn invalidate_programs(&mut self, world: &mut crate::world::WorldSimulationState) {
        if !self.programs_ready {
            return;
        }
        macro_rules! invalidate {
            ($query:ident) => {
                for &(_, binding) in self.state.$query.entries() {
                    if let Some(state) =
                        &mut binding.get_mut(&mut world.components).runtime.evaluation
                    {
                        state.program = None;
                    }
                }
            };
        }
        invalidate!(bounds);
        invalidate!(picking);
        self.programs_ready = false;
    }
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct GeometrySystemFactory;

impl SystemFactory for GeometrySystemFactory {
    fn id(&self) -> SystemId {
        GeometrySystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(SystemId("ipp.asset-dependencies")),
            SystemDependency::Required(SystemId("ipp.final-propagation")),
            SystemDependency::After(SystemId("ipp.skeleton")),
            SystemDependency::After(SystemId("ipp.skinning")),
            SystemDependency::After(SystemId("ipp.particles")),
        ]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        context.dependency::<crate::systems::asset_dependencies::AssetDependencySystem>(
            crate::systems::asset_dependencies::AssetDependencySystem::ID,
        )?;
        Ok(Box::new(GeometrySystem::default()))
    }
}

impl System for GeometrySystem {
    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        self.refresh = true;
        let _ = context;
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        if context.changed_components().next().is_none() {
            return;
        }
        self.state.spatial_bounds.invalidate();
        self.state.spatial_picking.invalidate();
        self.invalidate_programs(context.world_data);
        self.state
            .bounds
            .before_commit(context, crate::ComponentValue::BOUNDING_GEOMETRY);
        self.state
            .picking
            .before_commit(context, crate::ComponentValue::PICKING_GEOMETRY);
        self.refresh |= context.changed_components().next().is_some();
        #[cfg(feature = "skeletal-animation")]
        self.before_geometry_commit(context);
    }

    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.state.bounds.after_commit(
            context,
            crate::ComponentValue::BOUNDING_GEOMETRY,
            crate::components::registry::ComponentStorage::bounding_geometry_ptr,
        );
        self.state.picking.after_commit(
            context,
            crate::ComponentValue::PICKING_GEOMETRY,
            crate::components::registry::ComponentStorage::picking_geometry_ptr,
        );
    }

    fn before_asset_release(
        &mut self,
        context: &mut crate::systems::SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        if event.kind != crate::services::asset_management::AssetLifecycleKind::GraphicsInvalidated
        {
            self.state.spatial_bounds.invalidate();
            self.state.spatial_picking.invalidate();
            self.invalidate_programs(context.world.world);
            #[cfg(feature = "skeletal-animation")]
            self.before_geometry_asset_release(context, event);
        }
    }

    fn asset_lifecycle(
        &mut self,
        context: &mut crate::systems::SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        if event.kind != crate::services::asset_management::AssetLifecycleKind::GraphicsInvalidated
        {
            self.invalidate_programs(context.world.world);
            self.refresh = true;
        }
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state = Default::default();
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.evaluate(&mut context.world);
        self.refresh = false;
    }

    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        _: &mut crate::WorldUpdateReport,
    ) {
        if self.refresh {
            self.update(context);
        }
    }
}
