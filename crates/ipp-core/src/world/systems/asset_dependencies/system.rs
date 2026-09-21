//! AssetDependencySystem: factory configuration and exclusively owned per-world state.

use super::AssetDependencySystemState;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct AssetDependencySystem {
    pub(in crate::world) state: AssetDependencySystemState,
    pub(super) bindings: crate::systems::SystemBindings<Self>,
}

impl AssetDependencySystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.asset-dependencies");
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct AssetDependencySystemFactory;

impl SystemFactory for AssetDependencySystemFactory {
    fn id(&self) -> SystemId {
        AssetDependencySystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        <AssetDependencySystem as crate::systems::SystemBoundUpdate>::dependencies()
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(AssetDependencySystem {
            state: Default::default(),
            bindings: crate::systems::SystemBindings::resolve(context)?,
        }))
    }
}

impl System for AssetDependencySystem {
    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        if context
            .changed_components()
            .any(|(_, component)| match component {
                crate::ComponentValue::BOUNDING_GEOMETRY
                | crate::ComponentValue::PICKING_GEOMETRY
                | crate::ComponentValue::MESH_INSTANCE => true,
                #[cfg(feature = "mesh-poses")]
                crate::ComponentValue::MESH_POSE => true,
                _ => false,
            })
        {
            self.state.evaluation_meshes_initialized = false;
        }
    }

    fn prepare_frame(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
    ) -> Result<(), crate::ErrorReason> {
        let changes = self.access(&mut context.world).begin_resource_reconcile()?;
        self.state.prepared_changes.extend(changes);
        Ok(())
    }

    fn accept_ingress(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.access(&mut context.world).accept_asset_uploads(0);
    }

    fn after_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        self.update_authored_demand(context)
    }

    fn load_persistent_state(
        &mut self,
        context: &mut crate::systems::SystemLoadContext<'_, '_>,
        state: Option<&crate::systems::SystemPersistentState>,
    ) -> Result<(), String> {
        if state.is_some() {
            return Err("Asset dependency state is derived from authored components".into());
        }
        self.restore_authored_demand(&mut context.world)
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state = Default::default();
    }

    crate::system_update!(bindings);

    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        report: &mut crate::WorldUpdateReport,
    ) {
        report
            .resource_changes
            .append(&mut self.state.prepared_changes);
        report
            .assets
            .extend(self.access(&mut context.world).take_asset_outcomes());
    }
}

#[crate::systems::system_update(SystemDependency::After(SystemId("ipp.constraints")))]
impl AssetDependencySystem {
    fn update(
        &mut self,
        ecs: crate::systems::SystemEcsAccess<'_>,
        assets: &mut crate::services::asset_management::AssetManagementService,
        animation: &crate::systems::animation::AnimationSystem,
        overlay: &crate::systems::state_overlay::StateOverlaySystem,
        _dt: f64,
    ) {
        let _ = overlay;
        if !crate::allocation_optimizations_enabled()
            || self.state.animation_demand_revision != Some(animation.state.demand_revision)
        {
            self.state.animation_demand_revision = Some(animation.state.demand_revision);
            let animation_sources: std::collections::BTreeSet<_> =
                animation.state.animation_sources.iter().cloned().collect();
            self.state.changed_sources.extend(
                self.state
                    .animation_sources
                    .symmetric_difference(&animation_sources)
                    .cloned(),
            );
            self.state.animation_sources = animation_sources;
        }
        let changes = std::mem::take(&mut self.state.changed_sources)
            .into_iter()
            .map(|source| {
                let retained = self.state.source_users.contains_key(&source)
                    || self.state.animation_sources.contains(&source);
                (source, retained)
            })
            .collect();
        assets.update_user_deltas(ecs.world.id, changes);
        if crate::evaluation_scratch_reuse_enabled() {
            if !self.state.evaluation_meshes_initialized {
                let changed = crate::systems::geometry::update_evaluation_mesh_demand(
                    ecs.world,
                    &ecs.world.state,
                    &mut self.state.evaluation_meshes,
                );
                if changed || !self.state.evaluation_meshes_initialized {
                    assets.set_evaluation_meshes(
                        ecs.world.id,
                        self.state.evaluation_meshes.keys().cloned().collect(),
                    );
                    self.state.evaluation_meshes_initialized = true;
                }
            }
        } else {
            // A later comparison switch must reconcile against this actual set.
            self.state.evaluation_meshes_initialized = false;
            assets.set_evaluation_meshes(
                ecs.world.id,
                crate::systems::geometry::evaluation_mesh_demand(ecs.world, &ecs.world.state),
            );
        }
        self.state
            .prepared_changes
            .extend(assets.finish_reconcile(ecs.world.id));
    }
}
