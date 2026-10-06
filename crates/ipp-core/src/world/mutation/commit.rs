//! Commit of staged component changes through System validation, invalidation
//! and observation hooks, and evaluated writes that commit immediately.

use crate::world::context::SystemInstanceAccess;
use crate::world::systems;
use crate::world::{WorldContext, WorldMutationState, WorldSimulationState};
use crate::{ComponentValue, EntityId, ErrorReason};

impl WorldContext<'_> {
    pub(crate) fn flush_lifecycle_cleanup(&mut self) {
        let cleanup = std::mem::take(&mut self.world.lifecycle_cleanup);
        if cleanup.is_empty() {
            return;
        }
        for (entity, value) in cleanup {
            let component = value.type_id();
            let Some(incarnation) = self
                .world
                .state
                .entities
                .get(&entity)
                .and_then(|record| record.input(component))
                .map(|input| input.incarnation)
            else {
                continue;
            };
            self.world
                .state
                .changed
                .insert((entity, component), Some(incarnation));
            self.world.state.prepared.insert((entity, component), value);
        }
        if let Err(_error) = commit_components(
            self.world,
            &mut self.instances,
            None,
            self.asset_acquisition,
            self.data,
            true,
        ) {
            crate::diagnostic!(
                Debug,
                "world={} lifecycle.cleanup.validation error={}",
                self.world.id.0,
                _error
            );
        }
    }

    pub(in crate::world) fn commit_pending_changes(&mut self) -> Result<(), ErrorReason> {
        commit_components(
            self.world,
            &mut self.instances,
            None,
            self.asset_acquisition,
            self.data,
            false,
        )
    }
}

pub(in crate::world) fn commit_components(
    world: &mut WorldSimulationState,
    instances: &mut SystemInstanceAccess<'_>,
    mut current: Option<&mut dyn systems::System>,
    assets: &mut crate::services::asset_management::AssetManagementService,
    data: &mut crate::services::data::DataService,
    evaluated: bool,
) -> Result<(), ErrorReason> {
    #[cfg(feature = "instrumentation")]
    let _context = crate::profiling::ContextScope::world(world.profile_context);
    #[cfg(feature = "instrumentation")]
    let _allocation_scope = crate::profiling::AllocationScope::new(200, "world.commit");

    let mut staged = WorldMutationState {
        entities_state: std::mem::take(&mut world.state),
    };
    let mut cleanup = Vec::new();
    // Staged copies of the batch move into the prepared values before any
    // commit observer reads them.
    staged.prepare_deferred_components();
    let reservation = staged
        .prepared
        .iter()
        .try_for_each(|(&(entity, component), value)| {
            if value.type_id() == component {
                world
                    .components
                    .try_reserve_component(component, entity.index() as usize + 1)?;
            }
            Ok::<_, ErrorReason>(())
        });
    if let Err(error) = reservation {
        world.state = staged.entities_state;
        return Err(error);
    }
    let mut validation = Ok(());
    #[cfg(feature = "instrumentation")]
    let measurement = crate::profiling::Stage::fixed(crate::profiling::FixedStage::CommitValidate);

    instances.visit(current.as_deref_mut(), |system| {
        let result = system.validate_commit(&systems::SystemCommitContext {
            world_data: world,
            staged: &mut staged,
            assets,
            data,
            evaluated,
            cleanup: &mut cleanup,
        });
        validation = validation.and(result);
    });
    #[cfg(feature = "instrumentation")]
    drop(measurement);
    #[cfg(feature = "instrumentation")]
    let measurement = crate::profiling::Stage::fixed(crate::profiling::FixedStage::CommitBefore);

    let mut round = 0;
    let mut accept_component_cleanup = true;
    loop {
        instances.visit(current.as_deref_mut(), |system| {
            system.before_commit(&mut systems::SystemCommitContext {
                world_data: world,
                staged: &mut staged,
                assets,
                data,
                evaluated,
                cleanup: &mut cleanup,
            });
        });
        let mut changed = false;
        if accept_component_cleanup {
            for (entity, value) in std::mem::take(&mut cleanup) {
                let component = value.type_id();
                let Some(incarnation) = staged
                    .entities
                    .get(&entity)
                    .and_then(|record| record.input(component))
                    .map(|input| input.incarnation)
                else {
                    continue;
                };
                if staged.prepared.get(&(entity, component)) != Some(&value) {
                    staged
                        .changed
                        .insert_if_absent((entity, component), Some(incarnation));
                    staged.prepared.insert((entity, component), value);
                    changed = true;
                }
            }
        } else {
            cleanup.clear();
        }
        if !changed {
            break;
        }
        round += 1;
        if round == 64 && accept_component_cleanup {
            world.fault = Some(ErrorReason::NonConvergentCommit);
            validation = Err(ErrorReason::NonConvergentCommit);
            accept_component_cleanup = false;
        }
    }
    #[cfg(feature = "instrumentation")]
    drop(measurement);
    #[cfg(feature = "instrumentation")]
    let measurement = crate::profiling::Stage::fixed(crate::profiling::FixedStage::CommitStorage);

    for entity in std::mem::take(&mut staged.retired_entities) {
        staged.links.release_retired(entity);
        staged.allocator.release(entity);
    }
    for (&(entity, component), &previous) in staged.changed.iter() {
        let next = staged
            .entities
            .get(&entity)
            .and_then(|record| record.input(component))
            .map(|input| input.incarnation);
        if previous != next {
            world
                .components
                .clear(component, entity.index() as usize)
                .expect("registered component");
        }
    }
    for &(entity, component) in staged.entities_state.changed.keys() {
        if let Some(value) = staged.entities_state.prepared.remove(&(entity, component)) {
            world.components.set(entity.index() as usize, value);
        } else if staged.evaluated_target == Some((entity, component)) {
            // Validation and every before_commit observer ran against old storage.
            // No identity or resource change is permitted through this path.
            let result = world.components.write_numeric_properties(
                component,
                entity.index() as usize,
                &staged.evaluated_properties,
            );
            debug_assert!(result.is_ok(), "validated numeric property patch");
            validation = validation.and(result);
        }
        // The staged copy is dropped once its value is installed.
        if let Some(state) = staged
            .entities_state
            .entities
            .get_mut(&entity)
            .and_then(|record| record.components.get_mut(&component))
        {
            state.staged = None;
        }
    }
    staged.evaluated_target = None;
    staged.evaluated_properties.clear();
    #[cfg(feature = "instrumentation")]
    drop(measurement);
    #[cfg(feature = "instrumentation")]
    let _measurement = crate::profiling::Stage::fixed(crate::profiling::FixedStage::CommitAfter);

    instances.visit(current.as_deref_mut(), |system| {
        system.after_commit(&mut systems::SystemCommitContext {
            world_data: world,
            staged: &mut staged,
            assets,
            data,
            evaluated,
            cleanup: &mut cleanup,
        });
    });
    debug_assert!(
        cleanup.is_empty(),
        "post-commit handlers cannot request structural restoration"
    );
    for observation in std::mem::take(&mut staged.lifecycle_effects) {
        instances.visit(current.as_deref_mut(), |system| {
            system.lifecycle(
                &systems::SystemLifecycleContext {
                    world: systems::SystemWorldView {
                        world,
                        authored: &staged.entities_state,
                    },
                },
                &observation,
            );
        });
    }
    staged.changed.clear();
    staged.links.changed.clear();
    staged.observed_components.clear();
    staged.observed_writes.clear();
    world.state = staged.entities_state;
    validation
}

impl systems::SystemRuntimeAccess<'_> {
    pub(in crate::world) fn apply_evaluated_value(
        &mut self,
        current: &mut dyn systems::System,
        entity: EntityId,
        value: ComponentValue,
    ) -> Result<(), ErrorReason> {
        value.validate_lifecycle()?;
        let component = value.type_id();
        let incarnation = self
            .world
            .state
            .entities
            .get(&entity)
            .and_then(|record| record.input(component))
            .ok_or(ErrorReason::MissingComponent)?
            .incarnation;
        self.world
            .state
            .changed
            .insert((entity, component), Some(incarnation));
        self.world.state.prepared.insert((entity, component), value);
        commit_components(
            self.world,
            &mut self.instances,
            Some(current),
            self.asset_acquisition,
            self.data,
            true,
        )
    }
}

impl systems::SystemRuntimeAccess<'_> {
    /// Mutate existing numeric properties, preserving commit observers and identity.
    /// Resource values, descriptor changes and replacement use the lifecycle API.
    pub fn apply_evaluated_properties(
        &mut self,
        current: &mut dyn systems::System,
        entity: EntityId,
        component: u16,
        properties: impl IntoIterator<Item = (u32, crate::components::schema::FieldValue)>,
    ) -> Result<(), ErrorReason> {
        let incarnation = self
            .world
            .state
            .entities
            .get(&entity)
            .and_then(|record| record.input(component))
            .ok_or(ErrorReason::MissingComponent)?
            .incarnation;
        debug_assert!(self.world.state.evaluated_target.is_none());
        self.world.state.evaluated_properties.clear();
        for (offset, value) in properties {
            if let Some((_, previous)) = self
                .world
                .state
                .evaluated_properties
                .iter_mut()
                .find(|(key, _)| *key == offset)
            {
                *previous = value;
            } else {
                self.world.state.evaluated_properties.push((offset, value));
            }
        }
        let result = self.world.components.validate_numeric_properties(
            component,
            entity.index() as usize,
            &self.world.state.evaluated_properties,
        );
        if let Err(error) = result {
            self.world.state.evaluated_properties.clear();
            return Err(error);
        }
        self.world.state.evaluated_target = Some((entity, component));
        self.world
            .state
            .changed
            .insert((entity, component), Some(incarnation));
        commit_components(
            self.world,
            &mut self.instances,
            Some(current),
            self.asset_acquisition,
            self.data,
            true,
        )
    }
}

impl systems::SystemRuntimeAccess<'_> {
    /// Tell the other Systems that the caller is about to overwrite these
    /// fields with absolute values.
    pub(in crate::world) fn before_absolute_writes(&mut self, fields: &[(EntityId, u16, u32)]) {
        if fields.is_empty() {
            return;
        }
        self.instances.visit(None, |system| {
            system.before_absolute_writes(fields);
        });
    }

    pub(in crate::world) fn before_numeric_update(&mut self, changed: &[(EntityId, u16)]) {
        if changed.is_empty() {
            return;
        }
        self.instances.visit(None, |system| {
            system.before_numeric_update(&mut systems::SystemNumericContext {
                world_data: self.world,
                changed,
            });
        });
    }
}
