//! Scoped dependency and service access. Bindings never own runtime instances.

use super::scheduler::SystemInstance;
use super::{System, SystemId, SystemInitError};
use crate::world::{WorldEntityState, WorldSimulationState};
use std::{any::Any, marker::PhantomData};

/// A typed slot resolved by a factory, usable only by that dependent in its World.
/// It neither borrows nor retains either system between callbacks.
#[derive(Debug)]
pub struct SystemDependencyBinding<T: System> {
    world: usize,
    dependent: usize,
    slot: usize,
    marker: PhantomData<fn() -> T>,
}

impl<T: System> Copy for SystemDependencyBinding<T> {}

impl<T: System> Clone for SystemDependencyBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Owned effective values observed at one System callback boundary.
/// Authored producer reconstruction belongs to outer `WorldContext::inspect` and persistence.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemEffectiveEntitySnapshot {
    /// World-local generational identity.
    pub id: crate::EntityId,
    /// Current normalized entity metadata.
    pub metadata: crate::EntityMetadata,
    /// Current evaluated components in registry order, without authored base values.
    pub components: Vec<crate::ComponentValue>,
}

/// Read-only ECS observation during construction and synchronous lifecycle callbacks.
pub struct SystemWorldView<'a> {
    pub(in crate::world) world: &'a WorldSimulationState,
    pub(in crate::world) authored: &'a WorldEntityState,
}

impl SystemWorldView<'_> {
    /// Frame receiving the next ordered mutation.
    pub fn next_tick(&self) -> u64 {
        self.world.tick.saturating_add(1)
    }

    /// Identity within the owning Host.
    pub fn id(&self) -> crate::WorldId {
        self.world.id
    }

    /// Observe current effective components without claiming authored producer values.
    pub fn inspect_effective(
        &self,
        entity: crate::EntityId,
    ) -> Option<SystemEffectiveEntitySnapshot> {
        if !self.authored.allocator.contains(entity) {
            return None;
        }
        let record = self.authored.entities.get(&entity)?;
        let mut components = Vec::new();
        self.world
            .components
            .inspect(entity.index() as usize, &mut components);
        components.sort_by_key(crate::ComponentValue::type_id);
        Some(SystemEffectiveEntitySnapshot {
            id: entity,
            metadata: record.metadata.clone(),
            components,
        })
    }

    /// Read still-occupied effective storage, including a target staged for deletion.
    /// The allocator generation remains live until every invalidation handler returns.
    pub fn effective_component(
        &self,
        entity: crate::EntityId,
        component: u16,
    ) -> Option<crate::ComponentValue> {
        if !self.authored.allocator.contains(entity) {
            return None;
        }
        self.world
            .components
            .get(component, entity.index() as usize)
    }

    /// Stable entity order, with owned observations that do not retain storage.
    pub fn entities_effective(&self) -> Vec<SystemEffectiveEntitySnapshot> {
        self.authored
            .entities
            .keys()
            .filter_map(|&id| self.inspect_effective(id))
            .collect()
    }
}

/// Factory access to already initialized, declared predecessors and Host services.
pub struct SystemInitContext<'a> {
    /// Temporarily borrowed Host generic I/O service.
    pub data_sources: &'a mut crate::services::data_source::DataSourceManagementService,
    /// Resolved World-owned reservations for the system being constructed.
    pub capacity_hints: &'a crate::WorldSystemCapacityHints,
    pub(in crate::world) identity: usize,
    pub(in crate::world) dependent: usize,
    pub(in crate::world) declared_dependencies: &'a [usize],
    pub(in crate::world) instances: &'a [SystemInstance],
    /// The unpublished World; construction cannot advance its time.
    pub world: SystemWorldView<'a>,
    pub(in crate::world) assets:
        &'a mut crate::services::asset_management::service::AssetManagementService,
}

impl SystemInitContext<'_> {
    /// Resolve an explicitly declared predecessor and validate its concrete type.
    pub fn dependency<T: System>(
        &self,
        id: SystemId,
    ) -> Result<SystemDependencyBinding<T>, SystemInitError> {
        let predecessor = self
            .declared_dependencies
            .iter()
            .copied()
            .find(|&index| self.instances[index].id == id)
            .ok_or(SystemInitError::UnavailableDependency(id))?;
        let any: &dyn Any = self.instances[predecessor].system.as_ref();
        if !any.is::<T>() {
            return Err(SystemInitError::DependencyType(id));
        }
        Ok(SystemDependencyBinding {
            world: self.identity,
            dependent: self.dependent,
            slot: predecessor,
            marker: PhantomData,
        })
    }

    /// Inspect the initialized predecessor while constructing dependent state.
    pub fn get<T: System>(&self, binding: SystemDependencyBinding<T>) -> Option<&T> {
        if binding.world != self.identity || binding.dependent != self.dependent {
            return None;
        }
        let any: &dyn Any = self.instances.get(binding.slot)?.system.as_ref();
        any.downcast_ref()
    }

    /// Borrow the Host's shared catalog during initialization.
    pub fn asset_resources(
        &mut self,
    ) -> &mut crate::services::asset_management::AssetManagementService {
        self.assets
    }
}

/// One mutable instance plus non-owning access to the remainder of its World.
pub struct SystemUpdateContext<'a, 'host> {
    /// Scoped world operations. Recursive time advancement is rejected.
    pub world: SystemRuntimeAccess<'a>,
    pub(in crate::world) dt: &'host f64,
    pub(in crate::world) dependent: usize,
}

impl SystemUpdateContext<'_, '_> {
    /// Host-supplied delta; systems do not own the runtime clock.
    pub fn dt(&self) -> f64 {
        *self.dt
    }

    /// Split component storage, declared System reads and concrete Host services.
    /// All returned borrows must end before this context dispatches a lifecycle mutation.
    pub fn inputs(
        &mut self,
    ) -> (
        super::SystemEcsAccess<'_>,
        super::SystemParameterInputs<'_>,
        f64,
    ) {
        let inputs = super::SystemParameterInputs {
            dependencies: SystemDependencies {
                identity: self.world.world.identity,
                dependent: self.dependent,
                instances: self.world.instances.before,
                trailing: None,
            },
            data_sources: Some(self.world.data_sources),
            assets: Some(self.world.asset_acquisition),
        };
        (
            super::SystemEcsAccess {
                world: self.world.world,
            },
            inputs,
            *self.dt,
        )
    }

    /// Borrow a declared predecessor until this read ends, without cloning state.
    pub fn dependency<T: System>(&self, binding: SystemDependencyBinding<T>) -> Option<&T> {
        if binding.world != self.world.world.identity || binding.dependent != self.dependent {
            return None;
        }
        let instance = self.world.instances.before.get(binding.slot)?;
        let any: &dyn Any = instance.system.as_ref();
        any.downcast_ref()
    }
}

/// Synchronous invalidation with old storage retained until every handler finishes.
pub struct SystemCommitContext<'a> {
    pub(in crate::world) world_data: &'a mut WorldSimulationState,
    pub(in crate::world) staged: &'a mut crate::world::WorldMutationState,
    pub(in crate::world) assets: &'a mut crate::services::asset_management::AssetManagementService,
    pub(in crate::world) evaluated: bool,
    pub(in crate::world) cleanup: &'a mut Vec<(crate::EntityId, crate::ComponentValue)>,
}

impl SystemCommitContext<'_> {
    /// Borrow Host resources while validating or invalidating a component change.
    pub fn asset_resources(&self) -> &crate::services::asset_management::AssetManagementService {
        self.assets
    }

    /// Evaluated writes preserve driver baselines and do not publish authored changes.
    pub fn is_evaluated(&self) -> bool {
        self.evaluated
    }

    /// Updated authored metadata and still-live effective storage.
    pub fn world(&self) -> SystemWorldView<'_> {
        SystemWorldView {
            world: self.world_data,
            authored: &self.staged.entities_state,
        }
    }

    /// Every component touched by the operation, including retained partial effects.
    pub fn changed_components(&self) -> impl Iterator<Item = (crate::EntityId, u16)> + '_ {
        self.staged.changed.keys().copied()
    }

    /// Whether an affected component preserves its exact effective incarnation.
    pub fn retains_component(&self, entity: crate::EntityId, component: u16) -> bool {
        let after = self
            .staged
            .entities
            .get(&entity)
            .and_then(|record| record.input(component))
            .map(|input| input.incarnation);
        let before = self
            .staged
            .changed
            .get(&(entity, component))
            .copied()
            .unwrap_or(after);
        before.is_some() && before == after
    }

    /// Queue restoration without dropping any occupied storage during invalidation.
    pub fn restore_evaluated_component(
        &mut self,
        entity: crate::EntityId,
        value: crate::ComponentValue,
    ) {
        self.cleanup.push((entity, value));
    }
}

/// Batched notification before compiled numeric writes. No incarnation, resource
/// reference or buffer ownership can change through this boundary.
pub struct SystemNumericContext<'a> {
    pub(in crate::world) world_data: &'a mut WorldSimulationState,
    pub(in crate::world) changed: &'a [(crate::EntityId, u16)],
}

impl SystemNumericContext<'_> {
    /// Numeric destinations which may be written by the following evaluation.
    pub fn changed_components(&self) -> &[(crate::EntityId, u16)] {
        self.changed
    }

    /// Observe old values before the exclusive numeric write phase begins.
    pub fn world(&self) -> SystemWorldView<'_> {
        SystemWorldView {
            world: self.world_data,
            authored: &self.world_data.state,
        }
    }
}

/// Reverse-order teardown, including cleanup after partial initialization.
pub struct SystemTeardownContext<'a> {
    /// Temporarily borrowed Host generic I/O service.
    pub data_sources: &'a mut crate::services::data_source::DataSourceManagementService,
    /// The ECS remains live until all systems finish teardown.
    pub world: SystemWorldView<'a>,
    pub(in crate::world) identity: usize,
    pub(in crate::world) dependent: usize,
    pub(in crate::world) instances: &'a [SystemInstance],
    pub(in crate::world) assets:
        &'a mut crate::services::asset_management::service::AssetManagementService,
}

impl SystemTeardownContext<'_> {
    /// Earlier dependencies remain live until their dependents have shut down.
    pub fn dependency<T: System>(&self, binding: SystemDependencyBinding<T>) -> Option<&T> {
        if binding.world != self.identity || binding.dependent != self.dependent {
            return None;
        }
        let any: &dyn Any = self.instances.get(binding.slot)?.system.as_ref();
        any.downcast_ref()
    }

    /// Borrow shared services while releasing world-local usage.
    pub fn asset_resources(
        &mut self,
    ) -> &mut crate::services::asset_management::AssetManagementService {
        self.assets
    }
}

/// Scoped World and service access while a resource transition is delivered.
pub struct SystemAssetContext<'a> {
    /// Exclusive access with the receiving instance temporarily excluded.
    pub world: SystemRuntimeAccess<'a>,
    pub(in crate::world) tick: u64,
}

impl SystemAssetContext<'_> {
    /// Observation boundary selected by the Host.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Consumer-scoped client observation; every System still receives the raw transition.
    /// Queue a restoration until every World finishes its resource invalidation handlers.
    pub fn restore_evaluated_component(
        &mut self,
        entity: crate::EntityId,
        value: crate::ComponentValue,
    ) {
        self.world.world.lifecycle_cleanup.push((entity, value));
    }

    /// Resolve this World's owned client observation of a Host resource transition.
    pub fn asset_snapshot(
        &self,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) -> Option<crate::AssetResourceSnapshot> {
        self.world
            .asset_acquisition
            .lifecycle_snapshot(self.world.id(), event)
    }
}

/// Generic command-time observation; interpretation belongs to the receiving System.
pub struct SystemCommandContext<'a> {
    /// Exclusive generic storage and service access; interpretation remains in the System.
    pub world: SystemRuntimeAccess<'a>,
}

/// Applied lifecycle observation. It never borrows transport/session queues.
pub struct SystemLifecycleContext<'a> {
    /// Current World and committed component storage.
    pub world: SystemWorldView<'a>,
}

/// Phase-scoped generic ECS/service borrows, separate from the receiving System's state.
/// This access cannot advance the Host clock or expose another System's private state.
pub struct SystemRuntimeAccess<'a> {
    pub(in crate::world) world: &'a mut WorldSimulationState,
    pub(in crate::world) instances: crate::world::access::SystemInstanceAccess<'a>,
    pub(in crate::world) asset_acquisition:
        &'a mut crate::services::asset_management::AssetManagementService,
    pub(in crate::world) data_sources:
        &'a mut crate::services::data_source::DataSourceManagementService,
}

impl SystemRuntimeAccess<'_> {
    /// Queue private cross-system work for the next ordinary mutation boundary.
    /// The receiving System remains the sole owner of its mutable state.
    #[cfg(feature = "gui")]
    pub(in crate::world) fn enqueue_internal_system_command<T: 'static>(
        &mut self,
        system: SystemId,
        command: T,
    ) -> Result<(), crate::ErrorReason> {
        if self.world.queue.len() >= self.world.limits.max_queued_batches {
            return Err(crate::ErrorReason::Capacity);
        }
        self.world.queue.push_back(crate::world::Ingress::System {
            system,
            session: 0,
            request_id: 0,
            command: Box::new(command),
        });
        Ok(())
    }

    /// Borrow a declared predecessor during command, lifecycle or restoration work.
    pub fn dependency<T: System>(&self, binding: SystemDependencyBinding<T>) -> Option<&T> {
        if binding.world != self.world.identity || binding.dependent != self.instances.before.len()
        {
            return None;
        }
        let instance = self.instances.before.get(binding.slot)?;
        let any: &dyn Any = instance.system.as_ref();
        any.downcast_ref()
    }

    /// Identity of the independently owned World supplying component storage.
    pub fn id(&self) -> crate::WorldId {
        self.world.id
    }

    /// Owned effective observation with no authored values or retained component reference.
    pub fn inspect_effective(
        &self,
        entity: crate::EntityId,
    ) -> Option<SystemEffectiveEntitySnapshot> {
        SystemWorldView {
            world: self.world,
            authored: &self.world.state,
        }
        .inspect_effective(entity)
    }

    /// Borrow immutable Host resource data during this callback.
    pub fn asset_resources(&self) -> &crate::services::asset_management::AssetManagementService {
        self.asset_acquisition
    }
}

/// Borrowed predecessor view with bindings validated once during construction.
pub struct SystemDependencies<'a> {
    pub(in crate::world) identity: usize,
    pub(in crate::world) dependent: usize,
    pub(in crate::world) instances: &'a [SystemInstance],
    pub(in crate::world) trailing: Option<(&'a dyn System, &'a [SystemInstance])>,
}

impl<'a> SystemDependencies<'a> {
    /// Resolve a private, World-scoped typed handle without rebuilding a lookup.
    pub fn get<T: System>(&self, binding: SystemDependencyBinding<T>) -> Option<&'a T> {
        if binding.world != self.identity || binding.dependent != self.dependent {
            return None;
        }
        let value: &dyn Any = if let Some(instance) = self.instances.get(binding.slot) {
            instance.system.as_ref()
        } else {
            let (current, after) = self.trailing?;
            if binding.slot == self.instances.len() {
                current
            } else {
                after
                    .get(binding.slot.checked_sub(self.instances.len() + 1)?)?
                    .system
                    .as_ref()
            }
        };
        value.downcast_ref()
    }
}

/// Ordered operation access; subsystem state remains in the receiving System.
pub struct SystemOperationContext<'a> {
    pub(in crate::world) world_data: &'a mut WorldSimulationState,
    pub(in crate::world) staged: &'a mut crate::world::WorldMutationState,
    pub(in crate::world) command: &'a crate::Command,
    pub(in crate::world) aliases: &'a mut std::collections::BTreeMap<u32, crate::EntityId>,
    pub(in crate::world) assets: &'a mut crate::services::asset_management::AssetManagementService,
    pub(in crate::world) dependencies: SystemDependencies<'a>,
}

impl SystemOperationContext<'_> {
    /// Private restoration defers derived indexes until persistent-state loading.
    pub fn is_restoring(&self) -> bool {
        self.world_data.restoring
    }

    /// Components touched by this operation, independently of accumulated observations.
    pub fn changed_components(&self) -> impl Iterator<Item = (crate::EntityId, u16)> + '_ {
        self.staged.operation_components.iter().copied()
    }

    /// Entities created by this operation, including retained partial effects.
    pub fn created_entities(&self) -> impl Iterator<Item = crate::EntityId> + '_ {
        self.staged.operation_created.iter().copied()
    }

    /// Departing identities, whose occupied storage is still protected by invalidation.
    pub fn deleted_entities(&self) -> impl Iterator<Item = crate::EntityId> + '_ {
        self.staged.operation_deleted.iter().copied()
    }

    /// Mandatory owner departure may deactivate invalid survivors without rollback.
    pub fn is_forced_cleanup(&self) -> bool {
        self.world_data.forced_cleanup
    }

    /// The ordered operation currently being interpreted by subsystem handlers.
    pub fn command(&self) -> &crate::Command {
        self.command
    }

    /// Resolve a handle or this batch's provisional alias against staged identities.
    pub fn resolve_entity(
        &self,
        reference: crate::EntityRef,
    ) -> Result<crate::EntityId, crate::ErrorReason> {
        self.staged.resolve(reference, self.aliases)
    }

    /// Borrow a declared predecessor until this operation read ends.
    pub fn dependency<T: System>(&self, binding: SystemDependencyBinding<T>) -> Option<&T> {
        self.dependencies.get(binding)
    }

    /// Current staged authoring identities with old occupied storage still live.
    pub fn world(&self) -> SystemWorldView<'_> {
        SystemWorldView {
            world: self.world_data,
            authored: &self.staged.entities_state,
        }
    }
}
