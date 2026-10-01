use super::*;
use systems::{
    System, SystemCommitContext, SystemDependency, SystemFactory, SystemId, SystemInitContext,
    SystemInitError, SystemUpdateContext,
};

/// Compiled affine propagation over the core's effective ordered links.
#[derive(Default)]
pub struct HierarchySystem {
    pub(crate) graph: HierarchyGraph,
    refresh: bool,
}

impl HierarchySystem {
    /// Stable identity of initial spatial propagation.
    pub const ID: SystemId = SystemId("ipp.hierarchy");
}

crate::system_parameter!(HierarchySystem);

/// Reusable initial propagation factory with no per-World state.
pub struct HierarchySystemFactory;

impl SystemFactory for HierarchySystemFactory {
    fn id(&self) -> SystemId {
        HierarchySystem::ID
    }

    fn capabilities(&self) -> crate::systems::SystemCapabilities {
        crate::systems::SystemCapabilities::new([crate::ComponentValue::TRANSFORM], [])
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::After(SystemId("ipp.animation")),
            SystemDependency::After(SystemId("ipp.constraints")),
            SystemDependency::After(SystemId("ipp.skeleton")),
        ]
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(HierarchySystem::default()))
    }
}

impl System for HierarchySystem {
    fn after_operation(
        &mut self,
        context: &mut systems::SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        for entity in context.staged.operation_created.clone() {
            context.staged.links.prepare_transform(entity);
        }
        if !context.is_restoring() {
            self.graph.reconcile(context.staged);
        }
        Ok(())
    }

    fn load_persistent_state(
        &mut self,
        context: &mut systems::SystemLoadContext<'_, '_>,
        state: Option<&systems::SystemPersistentState>,
    ) -> Result<(), String> {
        if state.is_some() {
            return Err("Hierarchy state is derived".into());
        }
        let world = &context.world.world;
        self.graph = HierarchyGraph::build(world, &world.state);
        if !self.graph.invalid.is_empty() {
            return Err("Invalid restored hierarchy".into());
        }
        Ok(())
    }

    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        self.refresh = true;
        for &(entity, _) in context.changed {
            if let Some(runtime) = context.world_data.state.links.transform_mut(entity) {
                runtime.clear();
            }
        }
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        if context.changed_components().next().is_some()
            || context.changed_entity_links().next().is_some()
        {
            self.graph.compiled.invalidate();
            self.refresh = true;
        }
        if context.is_evaluated() && !context.staged.links.operation_changed.is_empty() {
            self.graph.reconcile(context.staged);
        }
        for entity in context.staged.links.changed.clone() {
            if let Some(runtime) = context.staged.links.transform_mut(entity) {
                runtime.clear();
            }
        }
        for entity in context
            .staged
            .changed
            .keys()
            .map(|(entity, _)| *entity)
            .collect::<Vec<_>>()
        {
            if let Some(runtime) = context.staged.links.transform_mut(entity) {
                runtime.clear();
            }
        }
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.graph.prepare_order(&context.world.world.state);
        self.graph.prepare_access(context.world.world);
        self.graph
            .compiled
            .reset_aims(&mut context.world.world.components);
        self.graph.propagate(context.world.world, false);
        self.refresh = false;
    }

    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        _: &mut crate::WorldUpdateReport,
    ) {
        if self.refresh {
            <Self as System>::update(self, context);
        }
    }
}

/// Final affine propagation after terminal aiming.
#[derive(Default)]
pub struct FinalPropagationSystem {
    bindings: systems::SystemBindings<Self>,
    refresh: bool,
}

impl FinalPropagationSystem {
    /// Stable identity of final spatial propagation.
    pub const ID: SystemId = SystemId("ipp.final-propagation");
}

/// Reusable final propagation factory with no per-World state.
pub struct FinalPropagationSystemFactory;

impl SystemFactory for FinalPropagationSystemFactory {
    fn id(&self) -> SystemId {
        FinalPropagationSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        <FinalPropagationSystem as systems::SystemBoundUpdate>::dependencies()
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(FinalPropagationSystem {
            bindings: systems::SystemBindings::resolve(context)?,
            refresh: false,
        }))
    }
}

impl System for FinalPropagationSystem {
    fn before_numeric_update(&mut self, _: &mut crate::systems::SystemNumericContext<'_>) {
        self.refresh = true;
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.refresh |= context.changed_components().next().is_some()
            || context.changed_entity_links().next().is_some();
    }

    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        _: &mut crate::WorldUpdateReport,
    ) {
        if self.refresh {
            <Self as systems::SystemBoundUpdate>::update_bound(self, self.bindings.get(), context);
        }
    }

    crate::system_update!(bindings);
}

#[systems::system_update(SystemDependency::Required(systems::look_at::LookAtSystem::ID))]
impl FinalPropagationSystem {
    fn update(&mut self, ecs: systems::SystemEcsAccess<'_>, hierarchy: &HierarchySystem, _dt: f64) {
        hierarchy.graph.propagate(ecs.world, true);
        self.refresh = false;
    }
}
