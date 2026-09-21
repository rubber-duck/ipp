//! SkinningSystem: factory configuration and exclusively owned per-world state.

use super::SkinningSystemState;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemTeardownContext, SystemUpdateContext,
};

/// Fresh runtime state owned by one World.
pub struct SkinningSystem {
    pub(in crate::world) state: SkinningSystemState,
    refresh: bool,
}

impl SkinningSystem {
    /// Stable factory and instance identity.
    pub const ID: SystemId = SystemId("ipp.skinning");
}

/// Reusable factory; it retains no mutable world state.
#[derive(Default)]
pub struct SkinningSystemFactory;

impl SystemFactory for SkinningSystemFactory {
    fn id(&self) -> SystemId {
        SkinningSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(SystemId("ipp.skeleton")),
            SystemDependency::Required(SystemId("ipp.final-propagation")),
        ]
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(SkinningSystem {
            state: Default::default(),
            refresh: false,
        }))
    }
}

impl System for SkinningSystem {
    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.state.components.after_commit(
            context,
            crate::ComponentValue::SKIN,
            crate::components::registry::ComponentStorage::skin_ptr,
        );
    }

    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        if !self.refresh {
            for &(_, binding) in self.state.components.entries() {
                binding
                    .get_mut(&mut context.world_data.components)
                    .runtime
                    .valid = false;
            }
        }
        self.refresh = true;
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.state
            .components
            .before_commit(context, crate::ComponentValue::SKIN);
        let already_invalid = self.refresh;
        self.refresh |= context.changed_components().next().is_some();
        // Only evaluate() makes palettes valid, and update() clears refresh after
        // that pass. Inserted/replaced Skin values start invalid; preserving a
        // palette transfers the already-invalid runtime. Keep the synchronous
        // barrier, but do not rescan every entity for each subsequent field write.
        if self.refresh && (!crate::skinning_invalidation_reuse_enabled() || !already_invalid) {
            for &(_, binding) in self.state.components.entries() {
                binding
                    .get_mut(&mut context.world_data.components)
                    .runtime
                    .valid = false;
            }
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
