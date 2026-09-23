//! Borrowed world operations and read-only views of system-owned state.

use super::*;
use systems::scheduler::SystemInstance;

pub(in crate::world) struct SystemInstanceAccess<'a> {
    pub before: &'a mut [SystemInstance],
    pub current: Option<systems::SystemId>,
    pub after: &'a mut [SystemInstance],
}

/// Exclusive access to a Host-owned World and temporarily borrowed services.
/// System instances and state remain in their owning World's schedule.
pub struct WorldContext<'a> {
    pub(in crate::world) world: &'a mut WorldSimulationState,
    pub(in crate::world) instances: SystemInstanceAccess<'a>,
    pub(crate) asset_acquisition:
        &'a mut crate::services::asset_management::service::AssetManagementService,
    pub(crate) data_sources: &'a mut crate::services::data_source::DataSourceManagementService,
    pub(in crate::world) owns_update: bool,
}

impl Drop for WorldContext<'_> {
    fn drop(&mut self) {
        if self.owns_update {
            self.world.updating = false;
        }
    }
}

impl WorldContext<'_> {
    /// Identity within this Host.
    pub fn id(&self) -> crate::WorldId {
        self.world.id
    }

    /// Stored system order, with no per-frame sorting.
    pub fn system_ids(&self) -> impl Iterator<Item = systems::SystemId> + '_ {
        self.instances
            .before
            .iter()
            .map(|instance| instance.id)
            .chain(self.instances.current)
            .chain(self.instances.after.iter().map(|instance| instance.id))
    }

    pub(in crate::world) fn read(&self) -> WorldReadContext<'_> {
        WorldReadContext {
            world: self.world,
            state: &self.world.state,
            before: self.instances.before,
            after: self.instances.after,
        }
    }
}

pub(in crate::world) struct WorldReadContext<'a> {
    pub world: &'a WorldSimulationState,
    pub state: &'a WorldEntityState,
    pub before: &'a [systems::scheduler::SystemInstance],
    pub after: &'a [systems::scheduler::SystemInstance],
}

impl WorldContext<'_> {
    pub(crate) fn dispatch_asset_lifecycle(
        &mut self,
        event: &crate::services::asset_management::AssetLifecycleEvent,
        before_release: bool,
    ) {
        let tick = self.world.tick;
        for index in 0..self.instances.before.len() {
            let (before, rest) = self.instances.before.split_at_mut(index);
            let (current, after) = rest.split_first_mut().expect("schedule index");
            let mut context = systems::SystemAssetContext {
                world: systems::SystemRuntimeAccess {
                    world: self.world,
                    instances: SystemInstanceAccess {
                        before,
                        current: Some(current.id),
                        after,
                    },
                    asset_acquisition: self.asset_acquisition,
                    data_sources: self.data_sources,
                },
                tick,
            };
            if before_release {
                current.system.before_asset_release(&mut context, event);
            } else {
                current.system.asset_lifecycle(&mut context, event);
            }
        }
    }
}

impl WorldContext<'_> {
    /// Queue typed subsystem input alongside ordinary entity/component batches.
    pub fn enqueue_system_command<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
        command: T,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command_with_reply(system, session, 0, command)
    }

    /// Queue a correlated subsystem command without depending on its event queue.
    pub fn enqueue_system_command_with_reply<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        command: T,
    ) -> Result<(), ErrorReason> {
        if self.world.queue.len() >= self.world.limits.max_queued_batches
            || !self.system_ids().any(|id| id == system)
        {
            return Err(ErrorReason::Capacity);
        }
        self.world.queue.push_back(Ingress::System {
            system,
            session,
            request_id,
            command: Box::new(command),
        });
        Ok(())
    }

    /// Queue one ordered subsystem command group. Execution stops at the first
    /// failure and publishes one outcome with the successful applied prefix.
    pub fn enqueue_system_command_batch_with_reply<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
        request_id: u64,
        commands: Vec<T>,
    ) -> Result<(), ErrorReason> {
        if commands.is_empty()
            || self.world.queue.len() >= self.world.limits.max_queued_batches
            || !self.system_ids().any(|id| id == system)
        {
            return Err(ErrorReason::Capacity);
        }
        self.world.queue.push_back(Ingress::SystemBatch {
            system,
            session,
            request_id,
            commands: commands
                .into_iter()
                .map(|command| Box::new(command) as Box<dyn std::any::Any>)
                .collect(),
        });
        Ok(())
    }

    /// Move owned subsystem events into the transport's temporary delivery phase.
    pub fn drain_system_events<T: 'static>(
        &mut self,
        system: systems::SystemId,
        session: u64,
    ) -> Vec<T> {
        self.instances
            .before
            .iter_mut()
            .chain(self.instances.after.iter_mut())
            .find(|instance| instance.id == system)
            .map(|instance| {
                instance
                    .system
                    .drain_events(session)
                    .into_iter()
                    .filter_map(|value| value.downcast::<T>().ok().map(|value| *value))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Fence pending commands and release every subsystem's state for one attachment.
    pub fn release_system_session(&mut self, session: u64) {
        self.world.queue.retain(|input| {
            !matches!(
                input,
                Ingress::System {session: origin, ..}
                    | Ingress::SystemBatch {session: origin, ..}
                    if *origin == session
            )
        });
        for instance in self
            .instances
            .before
            .iter_mut()
            .chain(self.instances.after.iter_mut())
        {
            instance.system.release_session(session);
        }
    }
}

impl WorldContext<'_> {
    /// Borrow one concrete implementation and a generic ECS/service context.
    pub fn with_system<T: systems::System, R>(
        &mut self,
        id: systems::SystemId,
        operation: impl FnOnce(&mut T, &mut systems::SystemRuntimeAccess<'_>) -> R,
    ) -> Option<R> {
        let index = self
            .instances
            .before
            .iter()
            .position(|instance| instance.id == id)?;
        let (before, rest) = self.instances.before.split_at_mut(index);
        let (current, after) = rest.split_first_mut()?;
        let any: &mut dyn std::any::Any = current.system.as_mut();
        let system = any.downcast_mut::<T>()?;
        Some(operation(
            system,
            &mut systems::SystemRuntimeAccess {
                world: self.world,
                instances: SystemInstanceAccess {
                    before,
                    current: Some(id),
                    after,
                },
                asset_acquisition: self.asset_acquisition,
                data_sources: self.data_sources,
            },
        ))
    }

    /// Read an explicitly selected implementation without collecting or cloning dependencies.
    pub fn system<T: systems::System>(&self, id: systems::SystemId) -> Option<&T> {
        let instance = self
            .instances
            .before
            .iter()
            .chain(self.instances.after.iter())
            .find(|instance| instance.id == id)?;
        let any: &dyn std::any::Any = instance.system.as_ref();
        any.downcast_ref()
    }
}

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
            false,
        )
    }
}

pub(in crate::world) fn commit_components(
    world: &mut WorldSimulationState,
    instances: &mut SystemInstanceAccess<'_>,
    mut current: Option<&mut dyn systems::System>,
    assets: &mut crate::services::asset_management::AssetManagementService,
    evaluated: bool,
) -> Result<(), ErrorReason> {
    #[cfg(feature = "profiling")]
    let _allocation_scope = crate::profiling::AllocationScope::new(200, "world.commit");

    let mut staged = WorldMutationState {
        entities_state: std::mem::take(&mut world.state),
    };
    let mut cleanup = Vec::new();
    // Copy-prepared inputs staged by the batch get their single effective copy
    // before any commit observer reads prepared values.
    let mut validation = staged.prepare_deferred_components();
    #[cfg(feature = "profiling")]
    let measurement = crate::profiling::Stage::fixed(crate::profiling::FixedStage::CommitValidate);

    instances.visit(current.as_deref_mut(), |system| {
        let result = system.validate_commit(&systems::SystemCommitContext {
            world_data: world,
            staged: &mut staged,
            assets,
            evaluated,
            cleanup: &mut cleanup,
        });
        validation = validation.and(result);
    });
    #[cfg(feature = "profiling")]
    drop(measurement);
    #[cfg(feature = "profiling")]
    let measurement = crate::profiling::Stage::fixed(crate::profiling::FixedStage::CommitBefore);

    for round in 0..64 {
        instances.visit(current.as_deref_mut(), |system| {
            system.before_commit(&mut systems::SystemCommitContext {
                world_data: world,
                staged: &mut staged,
                assets,
                evaluated,
                cleanup: &mut cleanup,
            });
        });
        let mut changed = false;
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
        if !changed {
            break;
        }
        if round == 63 {
            world.fault = Some(ErrorReason::NonConvergentCommit);
            validation = Err(ErrorReason::NonConvergentCommit);
            // Final invalidation observes the last prepared values. No new
            // restoration is accepted once convergence has failed.
            instances.visit(current.as_deref_mut(), |system| {
                system.before_commit(&mut systems::SystemCommitContext {
                    world_data: world,
                    staged: &mut staged,
                    assets,
                    evaluated,
                    cleanup: &mut cleanup,
                });
            });
            cleanup.clear();
        }
    }
    #[cfg(feature = "profiling")]
    drop(measurement);
    #[cfg(feature = "profiling")]
    let measurement = crate::profiling::Stage::fixed(crate::profiling::FixedStage::CommitStorage);

    for entity in std::mem::take(&mut staged.retired_entities) {
        staged.allocator.release(entity);
    }
    world.components.reserve(staged.allocator.slots());
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
        if let Some(layer) = staged
            .entities_state
            .entities
            .get_mut(&entity)
            .and_then(|record| record.layers.get_mut(&component))
        {
            layer.inputs.finish_commit();
        }
    }
    #[cfg(feature = "surfaces")]
    for mutation in std::mem::take(&mut staged.deferred_mutations) {
        let previous_incarnation = staged
            .changed
            .get(&(mutation.entity, mutation.component))
            .copied()
            .flatten();
        let result = (mutation.apply)(&mut world.components);
        if result.is_ok() {
            staged.lifecycle_effects.push(
                systems::lifecycle_publisher::LifecycleObservation::Component {
                    entity: mutation.entity,
                    component: mutation.component,
                    kind: systems::lifecycle_publisher::ComponentLifecycleKind::Updated,
                    previous_incarnation,
                    incarnation: previous_incarnation,
                },
            );
        }
        validation = validation.and(result);
    }
    staged.evaluated_target = None;
    staged.evaluated_properties.clear();
    #[cfg(feature = "profiling")]
    drop(measurement);
    #[cfg(feature = "profiling")]
    let _measurement = crate::profiling::Stage::fixed(crate::profiling::FixedStage::CommitAfter);

    instances.visit(current.as_deref_mut(), |system| {
        system.after_commit(&mut systems::SystemCommitContext {
            world_data: world,
            staged: &mut staged,
            assets,
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
    staged.observed_components.clear();
    staged.observed_writes.clear();
    staged.prepared_bytes = 0;
    world.state = staged.entities_state;
    validation
}

impl SystemInstanceAccess<'_> {
    fn visit(
        &mut self,
        current: Option<&mut dyn systems::System>,
        mut callback: impl FnMut(&mut dyn systems::System),
    ) {
        for instance in &mut *self.before {
            callback(instance.system.as_mut());
        }
        if let Some(system) = current {
            callback(system);
        }
        for instance in &mut *self.after {
            callback(instance.system.as_mut());
        }
    }
}

impl systems::SystemRuntimeAccess<'_> {
    pub(in crate::world) fn apply_evaluated_value(
        &mut self,
        current: &mut dyn systems::System,
        entity: EntityId,
        value: ComponentValue,
    ) -> Result<(), ErrorReason> {
        value.validate_lifecycle()?;
        let value = value.prepare_effective(self.world.limits.max_staging_bytes)?;
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
            true,
        )
    }
}

impl systems::SystemRuntimeAccess<'_> {
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
