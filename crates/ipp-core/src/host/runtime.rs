use super::*;

impl HostRuntime {
    /// Construct services without publishing a world.
    pub fn new() -> Self {
        let host = Self {
            #[cfg(feature = "instrumentation")]
            profile_context: 0,
            worlds: BTreeMap::new(),
            next_world: 0,
            identity_namespace: next_host_namespace(),
            system_factories: SystemFactories::new(compiled_system_factories())
                .expect("valid compiled factory graph"),
            io: crate::services::io::IoService::new(),
            data: crate::services::data::DataService::new(),
            assets: crate::services::asset_management::service::AssetManagementService::new(),
            topology: Default::default(),
            publications: Default::default(),
            frame: 0,
        };
        let mut host = host;
        host.topology.identity = host.publications.identity;
        #[cfg(feature = "instrumentation")]
        {
            host.profile_context = crate::profiling::register_world(host.identity(), 0, 0, 0);
        }
        host.assets.require_lifecycle_barrier();
        host.assets
            .install_io_sources(&mut host.io)
            .expect("private producer source");
        host
    }

    /// Stable identity of this runtime Host, including while it owns no Worlds.
    /// Matches the Host part of runtime publication and root-binding identities;
    /// it is observational metadata, not authority to construct runtime tokens.
    pub fn identity(&self) -> u64 {
        self.topology.identity
    }

    /// Enter shared Host attribution without assigning work to a World.
    #[cfg(feature = "instrumentation")]
    pub fn profile_scope(&self) -> crate::profiling::ContextScope {
        crate::profiling::ContextScope::new(self.profile_context)
    }

    /// Observe an exact live World's current fault without advancing or flushing anything.
    /// Scheduling admission and completed publication availability remain independent.
    pub fn world_fault(&self, world: WorldRef) -> Result<Option<ErrorReason>, ErrorReason> {
        let current = self
            .worlds
            .get(&world.id())
            .filter(|current| current.runtime_ref() == world)
            .ok_or(ErrorReason::InvalidEntity)?;
        Ok(current.ingress_world_view().fault())
    }

    /// Register reusable construction configuration for every World on this Host.
    /// Graph validation runs here; instance initialization runs when creating a World.
    pub fn with_system_factories(
        factories: Vec<Arc<dyn SystemFactory>>,
    ) -> Result<Self, SystemScheduleError> {
        let system_factories = SystemFactories::new(factories)?;
        let mut host = Self::new();
        host.system_factories = system_factories;
        Ok(host)
    }

    /// Whether any World has admitted frame inputs without completing evaluation.
    /// This logical scheduling state is not an active World or service borrow.
    pub fn has_pending_world_updates(&self) -> bool {
        self.worlds.values().any(crate::World::has_prepared_update)
    }

    /// Registered factory order, shared by fresh world instances.
    pub fn system_ids(&self) -> impl ExactSizeIterator<Item = SystemId> + '_ {
        self.system_factories.ids()
    }

    /// Instantiate exactly these registered factories in an automatically named
    /// World; instance ownership is never supplied externally. Publishes only on success.
    pub fn create_world(
        &mut self,
        limits: WorldLimits,
        selected: &[SystemId],
    ) -> Result<WorldId, WorldConstructionError> {
        self.create_world_with_options(
            limits,
            crate::WorldCreateOptions::new(selected.iter().copied()),
        )
    }

    /// Create a named World with its selection and resolved reservation hints,
    /// publishing only on success.
    pub fn create_world_with_options(
        &mut self,
        limits: WorldLimits,
        options: crate::WorldCreateOptions,
    ) -> Result<WorldId, WorldConstructionError> {
        if let Some(canvas) = options.canvas {
            crate::systems::canvas::canvas_state::validate_creation_state(
                canvas,
                &options.selected_systems,
            )
            .map_err(WorldConstructionError::Canvas)?;
        }
        let factories = self
            .system_factories
            .select(&options.selected_systems)
            .map_err(WorldConstructionError::Systems)?;
        let id = self.next_world_id()?;
        let symbolic_id = if options.symbolic_id.is_empty() {
            self.automatic_world_symbol(id)
        } else {
            options.symbolic_id
        };
        self.validate_new_world_symbol(&symbolic_id, None)
            .map_err(WorldConstructionError::Metadata)?;
        let result = World::construct(
            crate::world::WorldConstructionIdentity {
                id,
                #[cfg(feature = "instrumentation")]
                host: self.identity(),
            },
            limits,
            options.capacity_hints,
            &factories,
            &mut self.assets,
            &mut self.io,
            &mut self.data,
        )
        .map(|mut world| {
            if let Some(canvas) = options.canvas {
                world.seed_canvas_state(canvas);
            }
            world.data.metadata = crate::WorldMetadata {
                symbolic_id,
                persistent_id: crate::WorldPersistentId(
                    (u128::from(self.identity_namespace) << 64) | u128::from(id.0),
                ),
            };
            world
        });
        self.publish_world(id, result)
    }

    pub(super) fn next_world_id(&self) -> Result<WorldId, WorldConstructionError> {
        self.next_world
            .checked_add(1)
            .map(WorldId)
            .ok_or(WorldConstructionError::Limits(ErrorReason::Capacity))
    }

    pub(super) fn publish_world(
        &mut self,
        id: WorldId,
        result: Result<World, WorldConstructionError>,
    ) -> Result<WorldId, WorldConstructionError> {
        match result {
            Ok(mut world) => {
                if world.data.metadata.symbolic_id.is_empty() {
                    world.data.metadata = crate::WorldMetadata {
                        symbolic_id: self.automatic_world_symbol(id),
                        persistent_id: crate::WorldPersistentId(
                            (u128::from(self.identity_namespace) << 64) | u128::from(id.0),
                        ),
                    };
                }
                self.topology.worlds.insert(id, world.runtime_ref());
                self.topology.revision += 1;
                self.worlds.insert(id, world);
                self.next_world = id.0;
                Ok(id)
            }
            Err(error) => {
                self.assets.release_world(id);
                Err(error)
            }
        }
    }

    /// Borrow one world and shared services at a safe operation boundary.
    pub fn world_mut(&mut self, id: WorldId) -> Option<WorldContext<'_>> {
        self.flush_resource_lifecycle();
        let mut context = self.worlds.get_mut(&id)?.context(
            &mut self.assets,
            &mut self.io,
            &mut self.data,
            &mut self.topology,
        );
        context.publications = Some(&self.publications);
        Some(context)
    }

    /// Observe the HostRuntime's live world identities in construction order.
    pub fn world_ids(&self) -> impl ExactSizeIterator<Item = WorldId> + '_ {
        self.worlds.keys().copied()
    }

    /// Borrow a published World's immutable selected-system manifest.
    pub fn world_manifest(&self, id: WorldId) -> Option<&crate::systems::WorldManifest> {
        Some(&self.worlds.get(&id)?.data.manifest)
    }

    /// Destroy a world independently of every other world.
    pub fn destroy_world(&mut self, id: WorldId) -> bool {
        let Some(mut world) = self.worlds.remove(&id) else {
            return false;
        };
        self.topology.destroy(id);
        self.publications.latest.remove(&id);
        for entry in self.publications.completed.values_mut() {
            if entry.data.world.id == id {
                entry.available = false;
            }
        }
        self.retire_publications();
        world.teardown(&mut self.assets, &mut self.io, &mut self.data);
        self.assets.release_world(id);
        self.flush_resource_lifecycle();
        true
    }
}

impl HostRuntime {
    /// Shared resource catalog and accounting.
    pub fn asset_resources(&self) -> &crate::services::asset_management::AssetManagementService {
        &self.assets
    }

    /// Exclusive service configuration/residency access at a Host boundary.
    pub fn asset_resources_mut(
        &mut self,
    ) -> &mut crate::services::asset_management::AssetManagementService {
        &mut self.assets
    }

    /// Install a Host-provided URI scheme once for every world.
    pub fn register_stream_resource_provider(&mut self, scheme: &str) -> Result<(), ErrorReason> {
        self.io
            .register_stream(&format!("{scheme}:"))
            .map_err(|_| ErrorReason::InvalidValue)
    }

    /// Pending requests from all worlds, issued by their shared resources.
    pub fn take_resource_requests(&mut self) -> Vec<crate::AssetAcquisitionRequest> {
        self.assets.requests(&self.io)
    }

    /// Cancellations after aggregate demand or Host residency changes.
    pub fn take_resource_cancellations(&mut self) -> Vec<u64> {
        self.io.take_cancellations()
    }

    /// Retain provider bytes independently of any particular world's lifetime.
    pub fn complete_resource(
        &mut self,
        id: u64,
        result: Result<Vec<u8>, String>,
    ) -> Result<(), ErrorReason> {
        self.io
            .complete_read(id, result)
            .map_err(|_| ErrorReason::Capacity)
    }

    /// Feed one bounded HostRuntime-owned input stream.
    pub fn asset_input_chunk(&self, id: u64, bytes: &[u8]) -> Result<bool, String> {
        self.io.input_chunk(id, bytes)
    }

    /// Complete only the matching live stream; stale completions are harmless.
    pub fn asset_input_end(&self, id: u64, result: Result<(), String>) {
        self.io.input_end(id, result);
    }

    /// Retained provider staging across all world consumers.
    pub fn asset_input_bytes(&self) -> usize {
        self.io.input_bytes()
    }

    /// Select GPU-aware polling at the HostRuntime rendering boundary.
    pub fn set_renderer_asset_loading(&mut self, enabled: bool) {
        self.assets.renderer_driven = enabled;
    }

    /// Progress CPU evaluation assets while a graphics context is detached.
    pub fn progress_evaluation_assets(&mut self) {
        self.flush_resource_lifecycle();
        self.assets.poll_evaluation_assets(&mut self.io);
        self.flush_resource_lifecycle();
    }

    /// Advance shared readers/loaders once at the HostRuntime's selected service phase.
    pub fn progress_assets(&mut self) {
        self.flush_resource_lifecycle();
        self.assets.poll(&mut self.io);
        self.flush_resource_lifecycle();
    }
}

fn next_host_namespace() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl HostRuntime {
    /// Supply platform-generated entropy before creating Worlds. Embedded callers may
    /// choose a durable application namespace; the default is process-local for tests.
    pub fn set_identity_namespace(&mut self, namespace: u64) -> Result<(), String> {
        if namespace == 0 || self.next_world != 0 {
            return Err(
                "Host identity namespace must be nonzero and configured before World creation"
                    .into(),
            );
        }
        self.identity_namespace = namespace;
        Ok(())
    }

    pub(super) fn automatic_world_symbol(&self, id: WorldId) -> String {
        let base = format!("world-{}", id.0);
        let mut name = base.clone();
        let mut suffix = 1u64;
        while self
            .worlds
            .values()
            .any(|world| world.data.metadata.symbolic_id == name)
        {
            name = format!("{base}-{suffix}");
            suffix += 1;
        }
        name
    }

    pub(crate) fn validate_new_world_symbol(
        &self,
        symbol: &str,
        replacing: Option<WorldId>,
    ) -> Result<(), String> {
        crate::world::validate_world_symbolic_id(symbol)?;
        if self
            .worlds
            .iter()
            .any(|(id, world)| Some(*id) != replacing && world.data.metadata.symbolic_id == symbol)
        {
            return Err(format!("World symbolic ID already exists: {symbol}"));
        }
        Ok(())
    }

    /// Discover published Worlds in runtime identity order.
    pub fn list_worlds(&self) -> Vec<crate::WorldDescriptor> {
        self.worlds
            .iter()
            .map(|(&id, world)| crate::WorldDescriptor {
                id,
                metadata: world.data.metadata.clone(),
                capacity_hints: world.data.capacity_hints.clone(),
            })
            .collect()
    }

    /// Resolve a live World without granting a session access to it.
    pub fn resolve_world(&self, selector: &crate::WorldSelector) -> Option<WorldId> {
        match selector {
            crate::WorldSelector::Id(id) => self.worlds.contains_key(id).then_some(*id),
            crate::WorldSelector::SymbolicId(symbol) => self
                .worlds
                .iter()
                .find(|(_, world)| world.data.metadata.symbolic_id == *symbol)
                .map(|(&id, _)| id),
        }
    }

    /// Rename atomically; runtime identities, attachments and saved identity stay valid.
    pub fn rename_world(&mut self, id: WorldId, symbolic_id: String) -> Result<(), String> {
        if !self.worlds.contains_key(&id) {
            return Err("World does not exist".into());
        }
        self.validate_new_world_symbol(&symbolic_id, Some(id))?;
        self.worlds
            .get_mut(&id)
            .expect("validated World")
            .data
            .metadata
            .symbolic_id = symbolic_id;
        Ok(())
    }
}

impl HostRuntime {
    /// Borrow generic I/O independently of asset management or World state.
    pub fn io(&self) -> &crate::services::io::IoService {
        &self.io
    }

    /// Configure and use generic data I/O at a Host boundary.
    pub fn io_mut(&mut self) -> &mut crate::services::io::IoService {
        &mut self.io
    }
}

impl HostRuntime {
    /// Finish every World's handlers before releasing shared resource storage.
    /// Exclusive Host access ends live phase borrows; logical frame preparation
    /// cannot delay invalidation. Strengthened release revisions are drained again.
    pub fn flush_resource_lifecycle(&mut self) {
        loop {
            for event in self.assets.take_lifecycle_events() {
                for world in self.worlds.values_mut() {
                    world
                        .context(
                            &mut self.assets,
                            &mut self.io,
                            &mut self.data,
                            &mut self.topology,
                        )
                        .dispatch_asset_lifecycle(&event, false);
                }
                self.assets.finish_lifecycle_event(&event);
            }

            let pending = self.assets.pending_releases();
            if pending.is_empty() {
                break;
            }
            for (_, event) in pending {
                self.invalidate_publication_resources(&event);
                for world in self.worlds.values_mut() {
                    world
                        .context(
                            &mut self.assets,
                            &mut self.io,
                            &mut self.data,
                            &mut self.topology,
                        )
                        .dispatch_asset_lifecycle(&event, true);
                }
                for world in self.worlds.values_mut() {
                    world
                        .context(
                            &mut self.assets,
                            &mut self.io,
                            &mut self.data,
                            &mut self.topology,
                        )
                        .flush_lifecycle_cleanup();
                }
                self.assets.finish_release(&event);
            }
        }
    }
}

impl HostRuntime {
    /// Shared typed datasets, independent of World component and asset storage.
    pub fn data_sources(&self) -> &crate::services::data::DataService {
        &self.data
    }

    /// Local producer admission and Host policy configuration.
    pub fn data_sources_mut(&mut self) -> &mut crate::services::data::DataService {
        &mut self.data
    }
}
