use super::{LookAt, math};

use crate::systems::{
    self,
    hierarchy::{HierarchyGraph, HierarchySystem, evaluated_affine, parent_affine},
};
use crate::{ComponentValue, ErrorReason};
use std::collections::BTreeSet;
use systems::{
    System, SystemFactory, SystemId, SystemInitContext, SystemInitError, SystemUpdateContext,
};

/// Owns dependency diagnosis; each LookAt owns its evaluated local rotation.
#[derive(Default)]
pub struct LookAtSystem {
    dependencies: super::system_state::LookAtDependencies,
    references: systems::entity_references::ProducerReferenceIndex,
    bindings: systems::SystemBindings<Self>,
    refresh: bool,
}

impl LookAtSystem {
    /// Stable terminal pose-changing pass identity.
    pub const ID: SystemId = SystemId("ipp.look-at");
}

/// Fresh terminal constraint evaluator per World.
pub struct LookAtSystemFactory;

impl SystemFactory for LookAtSystemFactory {
    fn id(&self) -> SystemId {
        LookAtSystem::ID
    }

    fn dependencies(&self) -> &[systems::SystemDependency] {
        <LookAtSystem as systems::SystemBoundUpdate>::dependencies()
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(LookAtSystem {
            bindings: systems::SystemBindings::resolve(context)?,
            ..Default::default()
        }))
    }
}

impl System for LookAtSystem {
    fn after_operation(
        &mut self,
        context: &mut systems::SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        if context.is_restoring() {
            return Ok(());
        }
        self.references.reconcile(
            context,
            ComponentValue::LOOK_AT,
            std::mem::offset_of!(LookAt, target) as u32,
        );
        let hierarchy = context
            .dependency(self.bindings.get().hierarchy)
            .expect("hierarchy dependency");
        let changed: BTreeSet<_> = context
            .staged
            .operation_components
            .iter()
            .filter(|(_, c)| *c == ComponentValue::LOOK_AT)
            .map(|(entity, _)| *entity)
            .chain(context.staged.operation_deleted.iter().copied())
            .chain(hierarchy.graph.affected.iter().copied())
            .collect();
        if changed.is_empty() {
            return Ok(());
        }
        let rejected = self.dependencies.reconcile(
            context.world_data,
            context.staged,
            &hierarchy.graph,
            changed,
        );
        self.refresh = true;
        if rejected {
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
            return Err("LookAt state is derived".into());
        }
        let hierarchy = context
            .world
            .dependency(self.bindings.get().hierarchy)
            .ok_or("Missing restored hierarchy dependency")?;
        self.dependencies.rebuild(
            context.world.world,
            &context.world.world.state,
            &hierarchy.graph,
        );
        self.references.rebuild(
            context.world.world,
            &context.world.world.state,
            ComponentValue::LOOK_AT,
            std::mem::offset_of!(LookAt, target) as u32,
        );
        if !self.dependencies.invalid.is_empty() {
            return Err("Invalid restored LookAt dependency".into());
        }
        Ok(())
    }

    fn validate_commit(
        &self,
        context: &systems::SystemCommitContext<'_>,
    ) -> Result<(), ErrorReason> {
        if context.is_evaluated()
            && context
                .changed_components()
                .any(|(_, c)| [ComponentValue::HIERARCHY, ComponentValue::LOOK_AT].contains(&c))
        {
            let graph = HierarchyGraph::build(context.world_data, context.staged);
            let mut dependencies = super::system_state::LookAtDependencies::default();
            dependencies.rebuild(context.world_data, context.staged, &graph);
            if !dependencies.invalid.is_empty() {
                return Err(ErrorReason::UnsupportedDependency);
            }
        }
        Ok(())
    }

    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        self.refresh = true;
        for &(entity, _) in context.changed {
            if let Some(value) = context
                .world_data
                .components
                .look_at_mut(entity.index() as usize)
            {
                value.runtime.rotation = None;
            }
        }
    }

    fn before_commit(&mut self, context: &mut systems::SystemCommitContext<'_>) {
        if context.world_data.restoring {
            return;
        }
        if context.is_evaluated()
            && context
                .changed_components()
                .any(|(_, c)| [ComponentValue::HIERARCHY, ComponentValue::LOOK_AT].contains(&c))
        {
            let graph = HierarchyGraph::build(context.world_data, context.staged);
            self.dependencies
                .rebuild(context.world_data, context.staged, &graph);
            self.refresh = true;
        }
        if crate::allocation_optimizations_enabled() {
            for &(entity, _) in context.staged.changed.keys() {
                if let Some(value) = context
                    .world_data
                    .components
                    .look_at_mut(entity.index() as usize)
                {
                    value.runtime.rotation = None;
                }
            }
        } else {
            for (entity, _) in context.changed_components().collect::<Vec<_>>() {
                if let Some(value) = context
                    .world_data
                    .components
                    .look_at_mut(entity.index() as usize)
                {
                    value.runtime.rotation = None;
                }
            }
        }
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

#[systems::system_update]
impl LookAtSystem {
    fn update(&mut self, ecs: systems::SystemEcsAccess<'_>, hierarchy: &HierarchySystem, _dt: f64) {
        for &entity in self.dependencies.targets.keys() {
            let index = entity.index() as usize;
            let Some(value) = ecs.world.components.look_at(index) else {
                continue;
            };
            let mut invalid = self.dependencies.invalid.contains(&entity);
            let rotation = (|| {
                if !value.enabled || value.target.to_bits() == 0 || invalid {
                    return None;
                }
                let target = match evaluated_affine(ecs.world, value.target) {
                    Ok(target) => target.point([0.0; 3]),
                    Err(_) => {
                        invalid = true;
                        return None;
                    }
                };
                let parent = match hierarchy.graph.parents.get(&entity) {
                    Some(&parent) => match parent_affine(ecs.world, entity, parent) {
                        Ok(parent) => parent,
                        Err(_) => {
                            invalid = true;
                            return None;
                        }
                    },
                    None => Default::default(),
                };
                let transform = ecs
                    .world
                    .components
                    .transform(index)
                    .copied()
                    .unwrap_or_default();
                let origin = [transform.x, transform.y, transform.z].map(f64::from);
                let local_target = parent.inverse_point(target);
                let direction = std::array::from_fn(|i| local_target[i] - origin[i]);
                // Pull World up through the complete parent inverse, retaining shear.
                math::aim(direction, parent.inverse_vector([0.0, 1.0, 0.0]))
            })();
            let runtime = &mut ecs.world.components.look_at_mut(index).unwrap().runtime;
            runtime.rotation = rotation;
            runtime.invalid = invalid;
            if let Some(runtime) = ecs.world.components.transform_runtime_mut(index) {
                runtime.affine.take();
            }
        }
        self.refresh = false;
    }
}
