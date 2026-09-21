use super::*;
use systems::{
    System, SystemCommitContext, SystemDependency, SystemFactory, SystemId, SystemInitContext,
    SystemInitError, SystemUpdateContext,
};

/// Owns object graph bookkeeping and the pre-constraint propagation pass.
#[derive(Default)]
pub struct HierarchySystem {
    pub(crate) graph: HierarchyGraph,
    refresh: bool,
    references: systems::entity_references::ProducerReferenceIndex,
    transforms: crate::world::component_query::ComponentQuery<ObjectTransformRuntime>,
}

impl HierarchySystem {
    /// Stable identity of initial object propagation.
    pub const ID: SystemId = SystemId("ipp.hierarchy");
}

crate::system_parameter!(HierarchySystem);

/// Reusable hierarchy factory without per-World state.
pub struct HierarchySystemFactory;

impl SystemFactory for HierarchySystemFactory {
    fn id(&self) -> SystemId {
        HierarchySystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
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
        if context.is_restoring() {
            return Ok(());
        }
        self.references.reconcile(
            context,
            ComponentValue::HIERARCHY,
            std::mem::offset_of!(super::Hierarchy, parent) as u32,
        );
        self.graph.reconcile(
            context.world_data,
            context.staged,
            context.staged.operation_components.iter().copied(),
            context.staged.operation_created.iter().copied(),
            context.staged.operation_deleted.iter().copied(),
        );
        if self
            .graph
            .affected
            .iter()
            .any(|id| self.graph.invalid.contains(id))
        {
            Err(ErrorReason::UnsupportedDependency)
        } else {
            Ok(())
        }
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
        self.references.rebuild(
            world,
            &world.state,
            ComponentValue::HIERARCHY,
            std::mem::offset_of!(super::Hierarchy, parent) as u32,
        );
        if !self.graph.invalid.is_empty() {
            return Err("Invalid restored hierarchy".into());
        }
        Ok(())
    }

    fn validate_commit(&self, context: &SystemCommitContext<'_>) -> Result<(), ErrorReason> {
        if context.is_evaluated()
            && context
                .changed_components()
                .any(|(_, c)| c == ComponentValue::HIERARCHY)
            && !HierarchyGraph::build(context.world_data, context.staged)
                .invalid
                .is_empty()
        {
            return Err(ErrorReason::UnsupportedDependency);
        }
        Ok(())
    }

    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        self.refresh = true;
        for &(entity, _) in context.changed {
            if let Some(runtime) = context
                .world_data
                .components
                .transform_runtime_mut(entity.index() as usize)
            {
                runtime.affine.take();
            }
            if let Some(value) = context
                .world_data
                .components
                .hierarchy_mut(entity.index() as usize)
            {
                value.runtime.world = None;
            }
        }
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.transforms
            .before_commit(context, ComponentValue::TRANSFORM);
        if context.changed_components().any(|(entity, component)| {
            component == ComponentValue::HIERARCHY || !context.retains_component(entity, component)
        }) {
            self.graph.compiled.invalidate();
        }
        if context.world_data.restoring {
            return;
        }
        if context.is_evaluated()
            && context
                .changed_components()
                .any(|(_, c)| c == ComponentValue::HIERARCHY)
        {
            self.graph.reconcile(
                context.world_data,
                context.staged,
                context.changed_components(),
                [],
                [],
            );
        }
        if context.changed_components().next().is_some() {
            self.refresh = true;
            // Placement is recomputed in the scheduled propagation pass. The
            // lifecycle barrier invalidates departing storage synchronously;
            // it must not repeatedly traverse surviving subtrees during a batch.
            for &(entity, _) in context.staged.changed.keys() {
                if let Some(runtime) = context
                    .world_data
                    .components
                    .transform_runtime_mut(entity.index() as usize)
                {
                    runtime.affine.take();
                }
                if let Some(value) = context
                    .world_data
                    .components
                    .hierarchy_mut(entity.index() as usize)
                {
                    value.runtime.world = None;
                }
            }
        }
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.transforms.prepare(
            context.world.world,
            crate::components::registry::ComponentStorage::transform_runtime_ptr,
        );
        for &(_, runtime) in self.transforms.entries() {
            runtime
                .get_mut(&mut context.world.world.components)
                .affine
                .take();
        }
        self.graph.prepare_order();
        self.graph.prepare_access(&context.world.world.components);
        if crate::compiled_hierarchy_enabled() {
            self.graph
                .compiled
                .reset_aims(&mut context.world.world.components);
        } else {
            for &entity in &self.graph.order {
                if let Some(value) = context
                    .world
                    .world
                    .components
                    .look_at_mut(entity.index() as usize)
                {
                    value.runtime.rotation = None;
                    value.runtime.invalid = false;
                }
            }
        }
        self.graph.propagate(context.world.world, false);
        self.refresh = false;
    }

    fn after_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.transforms.after_commit(
            context,
            ComponentValue::TRANSFORM,
            crate::components::registry::ComponentStorage::transform_runtime_ptr,
        );
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

/// Final descendant propagation after terminal constraints.
#[derive(Default)]
pub struct FinalPropagationSystem {
    bindings: systems::SystemBindings<Self>,
    refresh: bool,
}

impl FinalPropagationSystem {
    /// Stable identity distinct from initial propagation and terminal LookAt.
    pub const ID: SystemId = SystemId("ipp.final-propagation");
}

/// Reusable final propagation factory.
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
        self.refresh |= context.changed_components().next().is_some();
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
