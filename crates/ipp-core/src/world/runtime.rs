use super::*;

impl WorldContext<'_> {
    /// Queue a complete batch, or explicitly reject it without changing the world.
    pub fn enqueue(&mut self, batch: Batch) -> Result<(), ErrorReason> {
        if let Some(reason) = self.world.fault {
            return Err(reason);
        }
        if self.world.queue.len() >= self.world.limits.max_queued_batches
            || batch.operations.len() > self.world.limits.max_operations
            || batch_bytes(&batch).is_none_or(|bytes| bytes > self.world.limits.max_batch_bytes)
        {
            #[cfg(feature = "diagnostics")]
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
        self.world.queue.push_back(Ingress::Batch(batch));
        Ok(())
    }
}

impl World {
    pub(crate) fn context<'a>(
        &'a mut self,
        assets: &'a mut crate::services::asset_management::service::AssetManagementService,
        data_sources: &'a mut crate::services::data_source::DataSourceManagementService,
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
        systems::validate_authoring_factories(factories)
            .map_err(WorldConstructionError::Systems)?;
        let mut data = Self::simulation_state(limits).map_err(WorldConstructionError::Limits)?;
        data.id = id;
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
        data.components
            .try_reserve(data.capacity_hints.entities)
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
        if limits.max_operations == 0
            || limits.max_queued_batches == 0
            || limits.max_staging_bytes == 0
        {
            return Err(ErrorReason::Capacity);
        }
        Ok(WorldSimulationState {
            updating: false,
            prepared_frame: false,
            mutation_prepared: false,
            command_stream: None,
            accepting_removals: false,
            forced_cleanup: false,
            restoring: false,
            fault: None,
            deferred_removals: Vec::new(),
            deferred_removal_members: BTreeSet::new(),
            identity: next_system_world_identity()?,
            id: crate::WorldId(0),
            metadata: WorldMetadata::default(),
            limits,
            capacity_hints: WorldCapacityHints::default(),
            state: WorldEntityState {
                activation_budget: limits.max_staging_bytes,
                ..WorldEntityState::default()
            },
            components: registry::ComponentStorage::default(),
            queue: if crate::allocation_optimizations_enabled() {
                VecDeque::with_capacity(64)
            } else {
                VecDeque::new()
            },
            command_buffers: Vec::new(),
            lifecycle_cleanup: Vec::new(),
            tick: 0,
            time: 0.0,
        })
    }
}

pub(super) fn metadata_bytes(metadata: &EntityMetadata) -> Option<usize> {
    let mut bytes = metadata
        .classes
        .capacity()
        .checked_mul(std::mem::size_of::<String>() + 128)?;
    if let Some(symbol) = &metadata.symbolic_id {
        bytes = bytes.checked_add(symbol.capacity())?;
    }
    for class in &metadata.classes {
        bytes = bytes.checked_add(class.capacity())?;
    }
    Some(bytes)
}

pub(super) fn batch_bytes(batch: &Batch) -> Option<usize> {
    let mut bytes = batch
        .operations
        .capacity()
        .checked_mul(std::mem::size_of::<Command>())?;
    for operation in &batch.operations {
        let extra = match operation {
            Command::Create {
                metadata,
                ..
            }
            | Command::SetMetadata {
                metadata,
                ..
            } => metadata_bytes(metadata)?,
            Command::InsertComponentValue {
                value,
                ..
            } => value.retained_bytes()?,
            Command::InsertComponent {
                fields,
                ..
            } => fields
                .capacity()
                .checked_mul(std::mem::size_of::<FieldWrite>())?,
            Command::AttachEntityOverlayBinding {
                symbolic_id,
                ..
            } => symbolic_id.capacity(),
            Command::AttachComponentStateOverlay {
                fields,
                ..
            } => fields
                .capacity()
                .checked_mul(std::mem::size_of::<FieldWrite>())?,
            Command::UpdateComponentStateOverlay {
                fields,
                clear,
                ..
            } => fields
                .capacity()
                .checked_mul(std::mem::size_of::<FieldWrite>())?
                .checked_add(clear.capacity().checked_mul(std::mem::size_of::<u32>())?)?,
            _ => 0,
        };
        bytes = bytes.checked_add(extra)?;
        let fields: &[FieldWrite] = match operation {
            Command::InsertComponent {
                fields,
                ..
            } => fields,
            Command::SetField {
                field,
                ..
            } => std::slice::from_ref(field),
            Command::AttachComponentStateOverlay {
                fields,
                ..
            }
            | Command::UpdateComponentStateOverlay {
                fields,
                ..
            } => fields,
            _ => &[],
        };
        for field in fields {
            let owned = match &field.value {
                FieldValue::String(value) => value.capacity(),
                FieldValue::Bytes(value) => value.capacity(),
                _ => 0,
            };
            bytes = bytes.checked_add(owned)?;
        }
    }
    Some(bytes)
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
}
