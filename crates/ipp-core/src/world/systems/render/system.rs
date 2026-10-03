//! RenderSystem: factory configuration and exclusively owned per-world state.

use super::{RenderReadAccess, RenderSystemState};
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
#[derive(Default)]
pub struct RenderSystem {
    pub(in crate::world) state: RenderSystemState,
    bindings: crate::systems::SystemBindings<Self>,
    prepared_dirty: bool,
}

impl RenderSystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.render");
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct RenderSystemFactory;

impl SystemFactory for RenderSystemFactory {
    fn id(&self) -> SystemId {
        RenderSystem::ID
    }

    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        crate::systems::SystemCapabilities::new(
            [
                crate::ComponentValue::UNLIT_MATERIAL,
                crate::ComponentValue::MESH_INSTANCE,
                crate::ComponentValue::UNLIT_TEXTURE,
                crate::ComponentValue::PBR_MATERIAL,
                crate::ComponentValue::LIGHT,
                crate::ComponentValue::BASE_COLOR_TEXTURE,
                crate::ComponentValue::CUSTOM_MATERIAL,
                crate::ComponentValue::MESH_POSE,
            ],
            [crate::systems::WorldOperation::Rendering],
        )
    }

    fn dependencies(&self) -> &[SystemDependency] {
        <RenderSystem as crate::systems::SystemBoundUpdate>::dependencies()
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(RenderSystem {
            state: Default::default(),
            bindings: crate::systems::SystemBindings::resolve(context)?,
            prepared_dirty: false,
        }))
    }
}

impl System for RenderSystem {
    fn publish_output(
        &self,
        world: &crate::WorldContext<'_>,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), crate::ErrorReason> {
        self.publish(world, output)
    }

    fn before_numeric_update(&mut self, _context: &mut crate::systems::SystemNumericContext<'_>) {
        self.prepared_dirty = true;
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        if context.changed_components().next().is_some()
            || context.changed_entity_links().next().is_some()
        {
            self.prepared_dirty = true;
            self.state.entries.clear();
            self.state.debug_entries.clear();
            self.state.light_entries.clear();
            self.state.entries_ready = false;
        }
    }

    fn before_asset_release(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.state.entries.clear();
        self.state.entries_ready = false;
        let released = event.key.to_u64();
        let previous = self.state.items.len();
        self.state.items.retain(|item| {
            if item.mesh.asset == released {
                return false;
            }
            if item.pose.is_some_and(|(mesh, _)| mesh.asset == released) {
                return false;
            }
            if item
                .texture
                .is_some_and(|texture| texture.asset == released)
            {
                return false;
            }
            true
        });
        // Debug rows own their copied primitive data and use renderer-private assets.
        // Unrelated prepared output remains usable after this Host resource release.
        self.prepared_dirty |= self.state.items.len() != previous;
    }

    fn asset_lifecycle(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        _event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.state.entries.clear();
        self.state.entries_ready = false;
        self.prepared_dirty = true;
    }

    fn validate_commit(
        &self,
        context: &crate::systems::SystemCommitContext<'_>,
    ) -> Result<(), crate::ErrorReason> {
        RenderReadAccess::new(context.world_data, context.assets, &self.state)
            .validate_mesh_pose_changes(context.staged)
    }

    fn command(
        &mut self,
        _context: &mut crate::systems::SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), crate::ErrorReason> {
        let patch = command
            .downcast_ref::<crate::RenderStatePatch>()
            .ok_or(crate::ErrorReason::InvalidValue)?;
        match self.update_render_state(*patch) {
            Ok(changes) => {
                if let Some(changes) = changes {
                    self.state.state_changes.push(changes);
                }
                Ok(())
            }
            Err(reason) => {
                crate::diagnostic!(Warn, "[IPP core] render_state.reject reason={reason}");
                Err(reason)
            }
        }
    }

    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        report: &mut crate::WorldUpdateReport,
    ) {
        // The evaluated pass already observed final state; only changes
        // committed after it (deferred removals, later numeric writes) or
        // pending resources prepare again.
        let needs_update = self.prepared_dirty || !self.state.entries_ready;
        if needs_update {
            <Self as crate::systems::SystemBoundUpdate>::update_bound(
                self,
                self.bindings.get(),
                context,
            );
        }
        report
            .render_state_changes
            .extend(
                self.state
                    .state_changes
                    .drain(..)
                    .map(|changes| crate::RenderStateChange {
                        tick: report.tick,
                        changes,
                    }),
            );
    }

    fn teardown(&mut self, _context: &mut SystemTeardownContext<'_>) {
        self.state = Default::default();
    }

    crate::system_update!(bindings);
}

#[crate::systems::system_update(
    SystemDependency::Required(crate::systems::geometry::GeometrySystem::ID),
    SystemDependency::After(crate::systems::camera::CameraSystem::ID),
    SystemDependency::After(SystemId("ipp.particles")),
    SystemDependency::After(crate::systems::data_bindings::DataBindingSystem::ID),
    SystemDependency::After(crate::systems::plot::PlotSystem::ID)
)]
impl RenderSystem {
    fn update(
        &mut self,
        ecs: crate::systems::SystemEcsAccess<'_>,
        assets: &mut crate::services::asset_management::AssetManagementService,
        skeleton: Option<&crate::systems::skeleton::SkeletonSystem>,
        skinning: Option<&crate::systems::skinning::SkinningSystem>,
        _dt: f64,
    ) {
        if !self.state.entries_ready {
            let mut entries = std::mem::take(&mut self.state.entries);
            let mut debug_entries = std::mem::take(&mut self.state.debug_entries);
            let mut light_entries = std::mem::take(&mut self.state.light_entries);
            let mut diagnostics = std::mem::take(&mut self.state.compatibility_diagnostics);
            diagnostics.clear();
            let read = RenderReadAccess::new(ecs.world, assets, &self.state);
            let pending_resources = read.compile_entries(&mut entries);
            read.compile_auxiliary_entries(&mut debug_entries, &mut light_entries);
            diagnostics = read.prepare_render_diagnostics(diagnostics);
            self.state.entries = entries;
            self.state.debug_entries = debug_entries;
            self.state.light_entries = light_entries;
            self.state.compatibility_diagnostics = diagnostics;
            self.state.entries_ready = !pending_resources;
            self.state.items.clear();
        }
        let mut items = std::mem::take(&mut self.state.items);
        let debug_items = std::mem::take(&mut self.state.debug_items);
        let mut diagnostics = std::mem::take(&mut self.state.diagnostics);
        diagnostics.clear();
        if let Some(skeleton) = skeleton {
            diagnostics.extend(skeleton.state.skeleton_diagnostics.iter().cloned());
        }
        if let Some(skinning) = skinning {
            diagnostics.extend(skinning.state.diagnostics.iter().cloned());
        }
        diagnostics.extend(self.state.compatibility_diagnostics.iter().cloned());
        let mut written = 0;
        for entry in &self.state.entries {
            entry.append(ecs.world, &mut items, &mut written);
        }
        items.truncate(written);
        let debug_items = {
            let world = RenderReadAccess::new(ecs.world, assets, &self.state);
            world.prepare_debug_render_items(debug_items)
        };
        self.prepared_dirty = false;
        self.state.items = items;
        self.state.debug_items = debug_items;
        self.state.diagnostics = diagnostics;
    }
}
