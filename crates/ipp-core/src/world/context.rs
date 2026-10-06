//! Borrowed World access: the Host's exclusive [`WorldContext`], read-only
//! views, and lookup of selected System instances around the current one.

use crate::world::systems::{
    self, System, SystemDependencies, SystemInstance, SystemRuntimeAccess,
};
use crate::world::{WorldEntityState, WorldSimulationState};

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
    pub(crate) io: &'a mut crate::services::io::IoService,
    pub(crate) data: &'a mut crate::services::data::DataService,
    pub(crate) topology: &'a mut crate::host::attachments::topology::HostTopology,
    pub(crate) frame_context: Option<&'a crate::WorldFrameContext>,
    pub(crate) reference_worlds: Option<crate::host::reference_resolution::ReferenceWorlds<'a>>,
    pub(crate) publications: Option<&'a crate::host::publication::HostPublications>,
    pub(in crate::world) owns_update: bool,
}

impl Drop for WorldContext<'_> {
    fn drop(&mut self) {
        if self.owns_update {
            self.data.end_evaluation();
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

    /// Selected authoring support without allocating or cloning it.
    pub fn manifest(&self) -> &systems::WorldManifest {
        &self.world.manifest
    }

    pub(in crate::world) fn read(&self) -> WorldReadContext<'_> {
        WorldReadContext {
            world: self.world,
            state: &self.world.state,
        }
    }
}

pub(in crate::world) struct WorldReadContext<'a> {
    pub world: &'a WorldSimulationState,
    pub state: &'a WorldEntityState,
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
                io: self.io,
                data: self.data,
                topology: self.topology,
                frame_context: self.frame_context,
                reference_worlds: self.reference_worlds.as_ref(),
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
    pub(in crate::world) fn runtime_access(&mut self) -> SystemRuntimeAccess<'_> {
        SystemRuntimeAccess {
            world: self.world,
            instances: SystemInstanceAccess {
                before: self.instances.before,
                current: self.instances.current,
                after: self.instances.after,
            },
            asset_acquisition: self.asset_acquisition,
            io: self.io,
            data: self.data,
            topology: self.topology,
            frame_context: self.frame_context,
            reference_worlds: self.reference_worlds.as_ref(),
        }
    }

    pub(in crate::world) fn with_system_dyn<R>(
        &mut self,
        id: systems::SystemId,
        operation: impl FnOnce(&mut dyn System, SystemRuntimeAccess<'_>) -> R,
    ) -> Option<R> {
        let index = self
            .instances
            .before
            .iter()
            .position(|instance| instance.id == id)?;
        let (before, tail) = self.instances.before.split_at_mut(index);
        let (instance, after) = tail.split_first_mut()?;
        Some(operation(
            instance.system.as_mut(),
            SystemRuntimeAccess {
                world: self.world,
                instances: SystemInstanceAccess {
                    before,
                    current: Some(id),
                    after,
                },
                asset_acquisition: self.asset_acquisition,
                io: self.io,
                data: self.data,
                topology: self.topology,
                frame_context: self.frame_context,
                reference_worlds: self.reference_worlds.as_ref(),
            },
        ))
    }
}

impl SystemInstanceAccess<'_> {
    pub(in crate::world) fn visit(
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

    pub(in crate::world) fn visit_scoped(
        &mut self,
        identity: usize,
        mut current: Option<&mut dyn System>,
        mut callback: impl FnMut(&mut dyn System, SystemDependencies<'_>),
    ) {
        for index in 0..self.before.len() {
            let (before, tail) = self.before.split_at_mut(index);
            let (instance, _) = tail.split_first_mut().expect("schedule index");
            callback(
                instance.system.as_mut(),
                SystemDependencies {
                    identity,
                    dependent: index,
                    instances: before,
                    trailing: None,
                },
            );
        }
        if let Some(system) = current.as_deref_mut() {
            callback(
                system,
                SystemDependencies {
                    identity,
                    dependent: self.before.len(),
                    instances: self.before,
                    trailing: None,
                },
            );
        }
        for index in 0..self.after.len() {
            let (prefix, tail) = self.after.split_at_mut(index);
            let (instance, _) = tail.split_first_mut().expect("schedule index");
            callback(
                instance.system.as_mut(),
                SystemDependencies {
                    identity,
                    dependent: self.before.len() + 1 + index,
                    instances: self.before,
                    trailing: current.as_deref().map(|system| (system, &*prefix)),
                },
            );
        }
    }
}
