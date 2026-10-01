use super::*;

impl WorldContext<'_> {
    /// Selected authoring support without allocating or cloning it.
    pub fn manifest(&self) -> &systems::WorldManifest {
        &self.world.manifest
    }

    /// Queue a complete batch, or explicitly reject it without changing the world.
    pub fn enqueue(&mut self, batch: Batch) -> Result<(), ErrorReason> {
        self.enqueue_admitted(batch, None)
    }

    /// Queue ingress with operation-time receipt validation and reliable delivery admission.
    pub fn enqueue_with_effect_sink(
        &mut self,
        batch: Batch,
        effect_sink: Box<dyn crate::OperationEffectSink>,
    ) -> Result<(), ErrorReason> {
        self.enqueue_admitted(batch, Some(effect_sink))
    }

    fn enqueue_admitted(
        &mut self,
        batch: Batch,
        effect_sink: Option<Box<dyn crate::OperationEffectSink>>,
    ) -> Result<(), ErrorReason> {
        if let Some(reason) = self.world.fault {
            return Err(reason);
        }
        // An unlimited byte quota admits every size, so skip the walk over every
        // operation and inserted value.
        let max_bytes = self.world.limits.max_batch_bytes;
        if self.world.queue.len() >= self.world.limits.max_queued_batches
            || batch.operations.len() > self.world.limits.max_operations
            || (max_bytes != usize::MAX
                && batch_bytes(&batch).is_none_or(|bytes| bytes > max_bytes))
        {
            if !batch.operations.is_empty() {
                crate::diagnostic!(
                    Warn,
                    "[IPP core] batch.reject batch={} operations={} reason=enqueue_budget",
                    batch.id,
                    batch.operations.len()
                );
            }
            return Err(ErrorReason::Capacity);
        }
        self.world.queue.push_back(Ingress::Batch {
            batch,
            effect_sink,
        });
        Ok(())
    }
}

impl World {
    pub(crate) fn queued_reference_worlds(
        &self,
    ) -> crate::host::reference_resolution::WorldReferenceRequests {
        let mut references = crate::host::reference_resolution::referenced_worlds(
            self.data.queue.iter().flat_map(|ingress| match ingress {
                Ingress::Batch {
                    batch,
                    ..
                } => batch.operations.as_slice(),
                _ => &[],
            }),
        );
        for ingress in &self.data.queue {
            let (system, commands) = match ingress {
                Ingress::System {
                    system,
                    command,
                    ..
                } => (*system, std::slice::from_ref(command)),
                Ingress::SystemBatch {
                    system,
                    commands,
                    ..
                } => (*system, commands.as_slice()),
                Ingress::Batch {
                    ..
                } => continue,
            };
            if let Some(instance) = self
                .schedule
                .instances
                .iter()
                .find(|instance| instance.id == system)
            {
                for command in commands {
                    instance
                        .system
                        .command_world_references(command.as_ref(), &mut |world| {
                            references.include(world)
                        });
                }
            }
        }
        references
    }

    pub(crate) fn context<'a>(
        &'a mut self,
        assets: &'a mut crate::services::asset_management::service::AssetManagementService,
        data_sources: &'a mut crate::services::data_source::DataSourceManagementService,
        topology: &'a mut crate::host::topology::HostTopology,
    ) -> WorldContext<'a> {
        WorldContext {
            world: &mut self.data,
            instances: SystemInstanceAccess {
                before: &mut self.schedule.instances,
                current: None,
                after: &mut [],
            },
            asset_acquisition: assets,
            data_sources,
            topology,
            frame_context: None,
            reference_worlds: None,
            publications: None,
            owns_update: false,
        }
    }

    pub(crate) fn construct(
        id: crate::WorldId,
        limits: WorldLimits,
        hints: WorldCapacityHints,
        factories: &systems::SystemFactories,
        assets: &mut crate::services::asset_management::service::AssetManagementService,
        data_sources: &mut crate::services::data_source::DataSourceManagementService,
    ) -> Result<Self, WorldConstructionError> {
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
                data_sources,
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
                Ok(system) => instances.push(systems::scheduler::SystemInstance {
                    id: registration.id,
                    system,
                    #[cfg(feature = "instrumentation")]
                    profile_slot: None,
                }),
                Err(error) => {
                    teardown_instances(&data, &mut instances, assets, data_sources);
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
            teardown_instances(&data, &mut instances, assets, data_sources);
            return Err(WorldConstructionError::Initialization {
                system,
                error,
            });
        }
        #[cfg(feature = "instrumentation")]
        for instance in &mut instances {
            instance.profile_slot = Some(crate::profiling::register_system(
                instance.id.0,
                data.manifest.composition_id(),
            ));
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
        data_sources: &mut crate::services::data_source::DataSourceManagementService,
    ) {
        teardown_instances(
            &self.data,
            &mut self.schedule.instances,
            assets,
            data_sources,
        );
    }

    fn simulation_state(limits: WorldLimits) -> Result<WorldSimulationState, ErrorReason> {
        if limits.max_operations == 0 || limits.max_queued_batches == 0 {
            return Err(ErrorReason::Capacity);
        }
        Ok(WorldSimulationState {
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

pub(super) fn batch_bytes(batch: &Batch) -> Option<usize> {
    batch.operations.iter().try_fold(
        batch
            .operations
            .capacity()
            .checked_mul(std::mem::size_of::<Command>())?,
        |bytes, operation| bytes.checked_add(operation.retained_heap_bytes()?),
    )
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
    data: &WorldSimulationState,
    instances: &mut Vec<systems::scheduler::SystemInstance>,
    assets: &mut crate::services::asset_management::service::AssetManagementService,
    data_sources: &mut crate::services::data_source::DataSourceManagementService,
) {
    while let Some(mut current) = instances.pop() {
        current
            .system
            .teardown(&mut systems::SystemTeardownContext {
                world: systems::SystemWorldView {
                    world: data,
                    authored: &data.state,
                },
                identity: data.identity,
                dependent: instances.len(),
                instances,
                assets,
                data_sources,
            });
        // Drop each dependent before tearing down its predecessors.
        drop(current);
    }
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
