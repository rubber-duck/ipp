//! World construction from selected System factories, scoped context creation
//! and teardown in reverse schedule order.

use std::collections::{BTreeSet, VecDeque};

use crate::components::registry;
use crate::world::context::SystemInstanceAccess;
use crate::world::ingress::WorldLimits;
use crate::world::systems;
use crate::world::{World, WorldContext, WorldEntityState, WorldSimulationState};
use crate::{ErrorReason, WorldCapacityHints, WorldMetadata};

pub(crate) struct WorldConstructionIdentity {
    pub(crate) id: crate::WorldId,
    #[cfg(feature = "instrumentation")]
    pub(crate) host: u64,
}

/// A world is unpublished until its limits and complete selected graph validate.
#[derive(Debug)]
pub enum WorldConstructionError {
    /// Invalid or conflicting Host-visible World metadata.
    Metadata(String),
    /// Invalid ingress bounds, activation allowance or storage reservation.
    Limits(ErrorReason),
    /// Missing state/implementation or an invalid system dependency graph.
    Systems(systems::SystemScheduleError),
    /// A factory failed after graph validation. Earlier instances have been shut down.
    Initialization {
        /// Factory whose initialization failed.
        system: systems::SystemId,
        /// Concrete dependency or factory failure.
        error: systems::SystemInitError,
    },
    /// Invalid Canvas creation state, or Canvas state for a World that does not
    /// select the Canvas System.
    Canvas(ErrorReason),
}

impl std::fmt::Display for WorldConstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Metadata(error) => f.write_str(error),
            Self::Limits(error) => error.fmt(f),
            Self::Systems(error) => error.fmt(f),
            Self::Initialization {
                system,
                error,
            } => write!(f, "initializing {}: {}", system.0, error),
            Self::Canvas(error) => write!(f, "World creation Canvas state: {error}"),
        }
    }
}

impl std::error::Error for WorldConstructionError {}

impl World {
    pub(crate) fn context<'a>(
        &'a mut self,
        assets: &'a mut crate::services::asset_management::service::AssetManagementService,
        io: &'a mut crate::services::io::IoService,
        data: &'a mut crate::services::data::DataService,
        topology: &'a mut crate::host::attachments::topology::HostTopology,
    ) -> WorldContext<'a> {
        WorldContext {
            world: &mut self.data,
            instances: SystemInstanceAccess {
                before: &mut self.schedule.instances,
                current: None,
                after: &mut [],
            },
            asset_acquisition: assets,
            io,
            data,
            topology,
            frame_context: None,
            reference_worlds: None,
            publications: None,
            owns_update: false,
        }
    }

    pub(crate) fn construct(
        identity: WorldConstructionIdentity,
        limits: WorldLimits,
        hints: WorldCapacityHints,
        factories: &systems::SystemFactories,
        assets: &mut crate::services::asset_management::service::AssetManagementService,
        io: &mut crate::services::io::IoService,
        data_service: &mut crate::services::data::DataService,
    ) -> Result<Self, WorldConstructionError> {
        let id = identity.id;

        let mut data = Self::simulation_state(limits).map_err(WorldConstructionError::Limits)?;
        data.id = id;
        data.manifest =
            systems::WorldManifest::resolve(factories).map_err(WorldConstructionError::Systems)?;
        let defaults = WorldCapacityHints {
            systems: factories
                .ordered
                .iter()
                .map(|registration| {
                    (
                        registration.id.0.to_owned(),
                        registration.factory.capacity_hints(),
                    )
                })
                .collect(),
            ..WorldCapacityHints::default()
        };
        data.capacity_hints = hints
            .resolve(&defaults)
            .map_err(WorldConstructionError::Limits)?;
        data.state
            .allocator
            .reserve(data.capacity_hints.entities)
            .map_err(WorldConstructionError::Limits)?;
        let mut instances = Vec::with_capacity(factories.ordered.len());
        for registration in &factories.ordered {
            let mut context = systems::SystemInitContext {
                identity: data.identity,
                dependent: instances.len(),
                declared_dependencies: &registration.predecessors,
                instances: &instances,
                capacity_hints: &data.capacity_hints.systems[registration.id.0],
                world: systems::SystemWorldView {
                    world: &data,
                    authored: &data.state,
                },
                assets,
                io,
                data: data_service,
            };
            let created = registration
                .factory
                .create(&mut context)
                .and_then(|mut system| {
                    system
                        .reserve_capacity(context.capacity_hints)
                        .map_err(|error| systems::SystemInitError::Message(error.to_string()))?;
                    Ok(system)
                });
            match created {
                Ok(system) => instances.push(systems::SystemInstance {
                    id: registration.id,
                    system,
                    #[cfg(feature = "instrumentation")]
                    profile_slot: None,
                }),
                Err(error) => {
                    teardown_instances(&data, &mut instances, assets, io, data_service);
                    return Err(WorldConstructionError::Initialization {
                        system: registration.id,
                        error,
                    });
                }
            }
        }
        if let Err(error) = systems::validate_authoring_instances(&instances) {
            let system = match &error {
                systems::SystemInitError::AuthoringSystemType(id) => *id,
                _ => unreachable!(),
            };
            teardown_instances(&data, &mut instances, assets, io, data_service);
            return Err(WorldConstructionError::Initialization {
                system,
                error,
            });
        }
        #[cfg(feature = "instrumentation")]
        {
            data.profile_context = crate::profiling::register_world(
                identity.host,
                id.0,
                data.identity as u64,
                data.manifest.composition_id(),
            );
            for instance in &mut instances {
                instance.profile_slot = Some(crate::profiling::register_system(
                    instance.id.0,
                    data.manifest.composition_id(),
                    data.profile_context,
                ));
            }
        }
        Ok(Self {
            data,
            schedule: systems::SystemSchedule {
                instances,
            },
        })
    }

    /// Stored execution order for this world's entire lifetime.
    pub fn system_ids(&self) -> impl ExactSizeIterator<Item = systems::SystemId> + '_ {
        self.schedule.ids()
    }

    /// Selected authoring support, fixed for this World's lifetime.
    pub fn manifest(&self) -> &systems::WorldManifest {
        &self.data.manifest
    }

    pub(crate) fn teardown(
        &mut self,
        assets: &mut crate::services::asset_management::service::AssetManagementService,
        io: &mut crate::services::io::IoService,
        data: &mut crate::services::data::DataService,
    ) {
        #[cfg(feature = "instrumentation")]
        let _context = crate::profiling::ContextScope::world(self.data.profile_context);

        teardown_instances(&self.data, &mut self.schedule.instances, assets, io, data);
    }

    fn simulation_state(limits: WorldLimits) -> Result<WorldSimulationState, ErrorReason> {
        if limits.max_operations == 0 || limits.max_queued_batches == 0 {
            return Err(ErrorReason::Capacity);
        }
        Ok(WorldSimulationState {
            #[cfg(feature = "instrumentation")]
            profile_context: 0,
            updating: false,
            prepared_frame: false,
            accepting_removals: false,
            restoring: false,
            fault: None,
            deferred_removals: Vec::new(),
            deferred_removal_members: BTreeSet::new(),
            identity: next_system_world_identity()?,
            id: crate::WorldId(0),
            metadata: WorldMetadata::default(),
            limits,
            capacity_hints: WorldCapacityHints::default(),
            manifest: systems::WorldManifest::default(),
            state: WorldEntityState::default(),
            components: registry::ComponentStorage::default(),
            queue: VecDeque::new(),
            command_buffers: Vec::new(),
            lifecycle_cleanup: Vec::new(),
            tick: 0,
            time: 0.0,
        })
    }
}

fn next_system_world_identity() -> Result<usize, ErrorReason> {
    // Binding identities are never reused, including across independent Hosts.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    NEXT.fetch_update(
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
        |value| value.checked_add(1),
    )
    .map_err(|_| ErrorReason::Capacity)
}

fn teardown_instances(
    world: &WorldSimulationState,
    instances: &mut Vec<systems::SystemInstance>,
    assets: &mut crate::services::asset_management::service::AssetManagementService,
    io: &mut crate::services::io::IoService,
    data: &mut crate::services::data::DataService,
) {
    while let Some(mut current) = instances.pop() {
        current
            .system
            .teardown(&mut systems::SystemTeardownContext {
                world: systems::SystemWorldView {
                    world,
                    authored: &world.state,
                },
                identity: world.identity,
                dependent: instances.len(),
                instances,
                assets,
                io,
                data,
            });
        // Drop each dependent before tearing down its predecessors.
        drop(current);
    }
    data.release_world(crate::WorldRef {
        id: world.id,
        incarnation: world.identity,
    });
}

impl World {
    pub(crate) fn has_prepared_update(&self) -> bool {
        self.data.prepared_frame
    }

    /// Exclusive access to one selected implementation, outside any evaluation.
    pub(crate) fn system_mut<T: systems::System>(
        &mut self,
        id: systems::SystemId,
    ) -> Option<&mut T> {
        let instance = self
            .schedule
            .instances
            .iter_mut()
            .find(|instance| instance.id == id)?;
        let any: &mut dyn std::any::Any = instance.system.as_mut();
        any.downcast_mut()
    }
}
