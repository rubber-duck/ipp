//! Source references backed by the generic resource manager. Hosts own I/O.

use super::{catalog::AssetSlot, resource::AssetLoaderConstructor};
use crate::ErrorReason;
use crate::services::asset_management::*;
use crate::services::data_source::DataSourceManagementService;
use crate::services::data_source::MemoryDataSource;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::task::{Context, Waker};

/// Compiled resource type identity, extensible through registered factories.
pub type AssetResourceKind = AssetTypeId;

#[allow(non_upper_case_globals)]
impl AssetTypeId {
    /// Mesh payload type.
    pub const Mesh: Self = crate::MESH_TYPE;

    /// Texture payload type.
    pub const Texture: Self = crate::TEXTURE_TYPE;
}

/// Authoritative resource status, including load/unload observations.
pub type AssetResourceStatus = AssetLoadStatus;

/// Host-owned input stream request; its correlation never defines resource identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetAcquisitionRequest {
    /// Private input stream correlation.
    pub id: u64,
    /// Expected compiled payload type.
    pub kind: AssetResourceKind,
    /// Named immutable source.
    pub source: String,
    /// Requested immutable variant.
    pub variant: u32,
    /// Recover previously accepted immutable content.
    pub recovery: bool,
}

/// Observation of a stable resource object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetResourceSnapshot {
    /// Decoded and graphics representations at the observation boundary.
    pub representation: super::AssetRepresentationStatus,
    /// Stable Host-wide resource identity, visible only to subscribed worlds.
    pub id: u64,
    /// Compiled payload type.
    pub kind: AssetResourceKind,
    /// Named immutable source.
    pub source: String,
    /// Immutable variant.
    pub variant: u32,
    /// AssetProvider-owned availability.
    pub status: AssetResourceStatus,
}

pub(crate) fn validate_source(source: &str) -> Result<(), ErrorReason> {
    u32::try_from(source.len())
        .map(|_| ())
        .map_err(|_| ErrorReason::Capacity)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AssetDemandSelection {
    pub(crate) kind: AssetResourceKind,
    pub(crate) source: String,
    pub(crate) variant: u32,
}

impl AssetDemandSelection {
    pub(crate) fn new(kind: AssetResourceKind, source: &str, variant: u32) -> Self {
        Self {
            kind,
            source: source.to_owned(),
            variant,
        }
    }

    pub(crate) fn insert_into(
        demand: &mut BTreeSet<Self>,
        kind: AssetTypeId,
        source: &str,
        variant: u32,
    ) {
        if crate::allocation_optimizations_enabled() {
            let query = super::source_lookup::AssetSourceLookup {
                kind,
                uri: [source, "", "", ""],
                variant,
            };
            if demand.contains(&query as &dyn super::source_lookup::AssetSourceIdentity) {
                return;
            }
        }
        demand.insert(Self::new(kind, source, variant));
    }

    pub(crate) fn descriptor(&self) -> AssetSource {
        AssetSource {
            kind: self.kind,
            uri: self.source.clone(),
            variant: self.variant,
        }
    }
}

/// Host-owned acquisition, provider streams and per-world resource subscriptions.
pub struct AssetManagementService {
    pub(super) demand_users: BTreeMap<AssetDemandSelection, usize>,
    pub(super) consumers: BTreeMap<crate::WorldId, AssetConsumerState>,
    pub(super) assets: Vec<AssetSlot>,
    pub(super) free: Vec<u32>,
    pub(super) sources: BTreeMap<AssetSource, AssetKey>,
    pub(super) loaders: BTreeMap<AssetTypeId, AssetLoaderConstructor>,
    pub(super) graphics_loaders: BTreeSet<AssetTypeId>,
    pub(super) used: BTreeSet<AssetKey>,
    pub(super) owned: BTreeSet<AssetKey>,
    pub(super) idle: BTreeMap<AssetKey, IdleAsset>,
    pub(super) idle_resident_bytes: usize,
    pub(super) idle_epoch: u64,
    pub(super) idle_resident_bytes_target: usize,
    pub(super) memory: MemoryDataSource,
    pub(super) events: VecDeque<AssetLoadProgress>,
    pub(super) lifecycle_barrier: bool,
    pub(super) lifecycle_recipients: BTreeMap<AssetKey, BTreeSet<crate::WorldId>>,
    pub(super) pending_releases: BTreeMap<AssetKey, super::lifecycle::AssetReleaseKind>,
    pub(super) lifecycle_events: VecDeque<super::lifecycle::AssetLifecycleEvent>,
    pub(crate) renderer_driven: bool,
}

#[derive(Clone, Copy)]
pub(super) struct IdleAsset {
    pub(super) last_used: u64,
    pub(super) resident_bytes: usize,
}

#[derive(Default)]
pub(super) struct AssetConsumerState {
    /// Aggregate World demand published to observation and catalog accounting.
    demand: BTreeSet<AssetDemandSelection>,
    /// Independently replaced demand from each World System. A source remains
    /// in the aggregate until every local consumer releases it.
    #[cfg(feature = "gui")]
    system_demand: BTreeMap<&'static str, BTreeSet<AssetDemandSelection>>,
    observations_dirty: bool,
    evaluation_meshes: BTreeSet<AssetDemandSelection>,
    pub(super) owned: BTreeSet<AssetKey>,
    pub(super) internal: BTreeSet<AssetKey>,
    pub(super) observed_sources: BTreeSet<AssetSource>,
    events: Vec<AssetResourceSnapshot>,
}

impl Default for AssetManagementService {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetManagementService {
    /// Default soft memory target for completed resources without active users.
    /// Eviction preserves lifecycle retirement and generational freshness;
    /// Hosts opt into idle retention with an explicit target.
    pub const DEFAULT_IDLE_RESIDENT_BYTES_TARGET: usize = 0;

    /// Construct the compiled loaders and Host input bridge.
    pub fn new() -> Self {
        let mut manager = Self::empty();
        #[cfg(feature = "particles")]
        manager
            .register_loader(
                crate::systems::particles::PARTICLE_SURFACE_TYPE,
                crate::systems::particles::particle_surface_loader,
            )
            .expect("particle surface loader");
        #[cfg(feature = "particles")]
        manager
            .register_loader(
                crate::systems::particles::PARTICLE_CACHE_TYPE,
                crate::systems::particles::particle_cache_loader,
            )
            .expect("particle cache loader");
        manager
            .register_loader(
                crate::MESH_TYPE,
                crate::services::asset_management::mesh::cpu_mesh_loader,
            )
            .expect("compiled factory");
        manager
            .register_loader(
                crate::systems::geometry::GEOMETRY_TYPE,
                crate::systems::geometry::geometry_asset_loader,
            )
            .expect("compiled factory");
        manager
            .register_loader(
                crate::TEXTURE_TYPE,
                crate::services::asset_management::texture::cpu_texture_loader,
            )
            .expect("compiled factory");
        #[cfg(feature = "surfaces")]
        manager
            .register_loader(super::font::FONT_TYPE, super::font::cpu_font_loader)
            .expect("surface font loader");
        #[cfg(feature = "surfaces")]
        manager
            .register_loader(
                super::drawing::DRAWING_TYPE,
                super::drawing::cpu_drawing_loader,
            )
            .expect("surface drawing loader");
        manager
            .register_loader(
                super::shader::SHADER_TYPE,
                super::shader::shader_asset_loader,
            )
            .expect("unique shader loader");
        manager
            .register_loader(
                crate::systems::animation::ANIMATION_TYPE,
                crate::systems::animation::animation_asset_loader,
            )
            .expect("compiled factory");
        #[cfg(feature = "skeletal-animation")]
        {
            manager
                .register_loader(
                    crate::SKELETON_TYPE,
                    crate::services::asset_management::skeleton::skeleton_asset_loader,
                )
                .expect("compiled factory");
            manager
                .register_loader(
                    crate::POSE_TYPE,
                    crate::services::asset_management::skeleton::pose_asset_loader,
                )
                .expect("compiled factory");
        }
        #[cfg(feature = "skeletal-animation")]
        manager
            .register_loader(
                crate::SKIN_TYPE,
                crate::services::asset_management::skin_binding::skin_asset_loader,
            )
            .expect("compiled factory");
        manager
    }
}

impl AssetManagementService {
    pub(crate) fn scoped_selection(
        world: crate::WorldId,
        selection: &AssetDemandSelection,
    ) -> AssetDemandSelection {
        let mut selection = selection.clone();
        if let Some(path) = selection.source.strip_prefix("asset://") {
            selection.source = format!("producer://{}/{path}", world.0);
        }
        selection
    }

    pub(crate) fn validate_users(
        &self,
        world: crate::WorldId,
        demand: BTreeSet<AssetDemandSelection>,
    ) -> Result<(), String> {
        let sources = demand
            .iter()
            .map(|selection| Self::scoped_selection(world, selection).descriptor())
            .collect();
        self.validate_sources(&sources)
    }

    pub(crate) fn validate_additional_users(
        &self,
        world: crate::WorldId,
        additional: impl IntoIterator<Item = AssetDemandSelection>,
    ) -> Result<(), String> {
        self.validate_users(world, additional.into_iter().collect())
    }

    pub(crate) fn upload_world(
        &mut self,
        world: crate::WorldId,
        key: AssetUploadIdentity,
        bytes: Vec<u8>,
    ) -> Result<AssetKey, String> {
        let source = Self::scoped_selection(
            world,
            &AssetDemandSelection::new(
                key.kind,
                &format!("asset://{}/{}", key.kind.0, key.asset),
                key.variant,
            ),
        )
        .descriptor();
        let resource = self.upload_named(source.clone(), bytes)?;
        let consumer = self.consumers.entry(world).or_default();
        consumer.observations_dirty = true;
        consumer.owned.insert(resource);
        consumer.observed_sources.insert(source);
        Ok(resource)
    }

    /// Register immutable client content and prepare it without component demand.
    pub fn register_client_source(
        &mut self,
        world: crate::WorldId,
        source: AssetSource,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        let resource = self.upload_named(source.clone(), bytes)?;
        let consumer = self.consumers.entry(world).or_default();
        consumer.observations_dirty = true;
        consumer.owned.insert(resource);
        consumer.observed_sources.insert(source);
        Ok(())
    }

    /// Retain an existing prepared immutable source for another World consumer.
    pub fn retain_prepared_source(
        &mut self,
        world: crate::WorldId,
        source: &AssetSource,
    ) -> Result<(), String> {
        let key = self.find(source).ok_or("Unknown prepared source")?;
        self.retain_owned(key);
        let consumer = self.consumers.entry(world).or_default();
        consumer.observations_dirty = true;
        consumer.owned.insert(key);
        consumer.observed_sources.insert(source.clone());
        Ok(())
    }

    /// Prepare renderer-private recipes without publishing client resource records.
    pub fn prepare_internal_source(
        &mut self,
        world: crate::WorldId,
        source: AssetSource,
        bytes: Vec<u8>,
    ) -> Result<AssetKey, String> {
        let key = match self.find(&source) {
            Some(key) => key,
            None => self.upload_named(source, bytes)?,
        };
        self.retain_owned(key);
        let consumer = self.consumers.entry(world).or_default();
        let new_owned = consumer.owned.insert(key);
        let new_internal = consumer.internal.insert(key);
        consumer.observations_dirty |= new_owned || new_internal;
        Ok(key)
    }

    /// Replace private preparation ownership using already-resolved live keys.
    /// Unchanged demand retains its existing sets; departures still use shared ownership cleanup.
    pub fn retain_internal_sources(
        &mut self,
        world: crate::WorldId,
        keys: &[AssetKey],
    ) -> Result<(), String> {
        if keys.iter().any(|key| self.get(*key).is_none()) {
            return Err("Unknown private resource".into());
        }
        let removed: Vec<_> = self
            .consumers
            .get(&world)
            .into_iter()
            .flat_map(|consumer| consumer.internal.iter())
            .filter(|key| !keys.contains(key))
            .filter_map(|key| self.get(*key).map(|resource| resource.source().clone()))
            .collect();
        for &key in keys {
            self.retain_owned(key);
            let consumer = self.consumers.entry(world).or_default();
            let new_owned = consumer.owned.insert(key);
            let new_internal = consumer.internal.insert(key);
            consumer.observations_dirty |= new_owned || new_internal;
        }
        for source in removed {
            self.release_client_source(world, &source);
        }
        Ok(())
    }

    /// Replace a World's private preparation demand; teardown uses ordinary World ownership.
    pub fn prepare_internal_sources(
        &mut self,
        world: crate::WorldId,
        sources: impl IntoIterator<Item = (AssetSource, Vec<u8>)>,
    ) -> Result<(), String> {
        let mut desired = BTreeSet::new();
        for (source, bytes) in sources {
            desired.insert(self.prepare_internal_source(world, source, bytes)?);
        }
        let removed: Vec<_> = self
            .consumers
            .get(&world)
            .into_iter()
            .flat_map(|consumer| consumer.internal.difference(&desired))
            .filter_map(|key| self.get(*key).map(|resource| resource.source().clone()))
            .collect();
        for source in removed {
            self.release_client_source(world, &source);
        }
        Ok(())
    }

    /// Drop producer preparation ownership; remaining consumers retain source bytes.
    pub fn release_client_source(&mut self, world: crate::WorldId, source: &AssetSource) {
        let Some(key) = self.find(source) else {
            return;
        };
        if let Some(consumer) = self.consumers.get_mut(&world) {
            consumer.observations_dirty = true;
            consumer.owned.remove(&key);
            consumer.internal.remove(&key);
        }
        if !self
            .consumers
            .values()
            .any(|consumer| consumer.owned.contains(&key))
        {
            self.release(key);
        }
    }

    #[cfg(not(feature = "gui"))]
    fn update_users(&mut self, world: crate::WorldId, demand: BTreeSet<AssetDemandSelection>) {
        let next: BTreeSet<_> = demand
            .iter()
            .map(|selection| Self::scoped_selection(world, selection))
            .collect();
        let previous = self.consumers.get(&world).map(|consumer| &consumer.demand);
        let mut changes = BTreeMap::new();
        if let Some(previous) = previous {
            changes.extend(
                previous
                    .difference(&next)
                    .cloned()
                    .map(|source| (source, false)),
            );
            changes.extend(
                next.difference(previous)
                    .cloned()
                    .map(|source| (source, true)),
            );
        } else {
            changes.extend(next.into_iter().map(|source| (source, true)));
        }
        self.update_user_deltas(world, changes);
    }

    /// Replace one System's World-local demand without disturbing sibling consumers.
    #[cfg(feature = "gui")]
    pub(crate) fn update_system_users(
        &mut self,
        world: crate::WorldId,
        system: &'static str,
        demand: BTreeSet<AssetDemandSelection>,
    ) {
        let next: BTreeSet<_> = demand
            .iter()
            .map(|selection| Self::scoped_selection(world, selection))
            .collect();
        let previous = self
            .consumers
            .get(&world)
            .and_then(|consumer| consumer.system_demand.get(system));
        let mut changes = BTreeMap::new();
        if let Some(previous) = previous {
            changes.extend(
                previous
                    .difference(&next)
                    .cloned()
                    .map(|source| (source, false)),
            );
            changes.extend(
                next.difference(previous)
                    .cloned()
                    .map(|source| (source, true)),
            );
        } else {
            changes.extend(next.into_iter().map(|source| (source, true)));
        }
        self.update_system_user_deltas(world, system, changes, true);
    }

    #[cfg(feature = "gui")]
    pub(crate) fn update_user_deltas(
        &mut self,
        world: crate::WorldId,
        changes: BTreeMap<AssetDemandSelection, bool>,
    ) {
        self.update_system_user_deltas(world, "ipp.asset-dependencies", changes, false);
    }

    #[cfg(feature = "gui")]
    fn update_system_user_deltas(
        &mut self,
        world: crate::WorldId,
        system: &'static str,
        changes: BTreeMap<AssetDemandSelection, bool>,
        scoped: bool,
    ) {
        if changes.is_empty() {
            return;
        }
        let mut consumer = self.consumers.remove(&world).unwrap_or_default();
        let mut system_demand = consumer.system_demand.remove(system).unwrap_or_default();

        // Apply additions before removals, keeping shared identities occupied
        // across changes and across independent System demand lanes.
        let mut aggregate_additions = Vec::new();
        for (selection, _) in changes.iter().filter(|(_, retained)| **retained) {
            let selection = if scoped {
                selection.clone()
            } else {
                Self::scoped_selection(world, selection)
            };
            if system_demand.insert(selection.clone()) && !consumer.demand.contains(&selection) {
                aggregate_additions.push(selection);
            }
        }
        let mut aggregate_removals = Vec::new();
        for (selection, _) in changes.iter().filter(|(_, retained)| !**retained) {
            let selection = if scoped {
                selection.clone()
            } else {
                Self::scoped_selection(world, selection)
            };
            if system_demand.remove(&selection)
                && !consumer
                    .system_demand
                    .values()
                    .any(|demand| demand.contains(&selection))
            {
                aggregate_removals.push(selection);
            }
        }
        if !system_demand.is_empty() {
            consumer.system_demand.insert(system, system_demand);
        }
        self.apply_aggregate_user_deltas(world, consumer, aggregate_additions, aggregate_removals);
    }

    #[cfg(not(feature = "gui"))]
    pub(crate) fn update_user_deltas(
        &mut self,
        world: crate::WorldId,
        changes: BTreeMap<AssetDemandSelection, bool>,
    ) {
        if changes.is_empty() {
            return;
        }
        let consumer = self.consumers.remove(&world).unwrap_or_default();
        let additions = changes
            .iter()
            .filter(|(_, retained)| **retained)
            .map(|(selection, _)| Self::scoped_selection(world, selection));
        let removals = changes
            .iter()
            .filter(|(_, retained)| !**retained)
            .map(|(selection, _)| Self::scoped_selection(world, selection));
        self.apply_aggregate_user_deltas(world, consumer, additions, removals);
    }

    /// Apply already-aggregated World demand to observations and the catalog.
    /// Callers keep their own representation and publish additions first so a
    /// replacement never makes a shared source transiently idle.
    fn apply_aggregate_user_deltas(
        &mut self,
        world: crate::WorldId,
        mut consumer: AssetConsumerState,
        additions: impl IntoIterator<Item = AssetDemandSelection>,
        removals: impl IntoIterator<Item = AssetDemandSelection>,
    ) {
        consumer.observations_dirty = true;
        let mut released = Vec::new();
        for selection in additions {
            if !consumer.demand.insert(selection.clone()) {
                continue;
            }
            let descriptor = selection.descriptor();
            let existing = self.find(&descriptor);
            if !consumer.observed_sources.contains(&descriptor) {
                if let Some(resource) = existing.and_then(|key| self.get(key)) {
                    consumer.events.push(AssetResourceSnapshot {
                        id: resource.key().to_u64(),
                        kind: resource.source().kind,
                        source: Self::local_source(world, &resource.source().uri),
                        variant: resource.source().variant,
                        status: resource.status().clone(),
                        representation: resource.representation(),
                    });
                }
                consumer.observed_sources.insert(descriptor.clone());
            }
            *self.demand_users.entry(selection).or_default() += 1;
            let key = self
                .get_or_create(descriptor)
                .expect("resource references validated before commit");
            self.used.insert(key);
            self.remove_idle(key);
            if self.pending_releases.get(&key) == Some(&super::AssetReleaseKind::Remove) {
                self.pending_releases.remove(&key);
            }
        }
        for selection in removals {
            if !consumer.demand.remove(&selection) {
                continue;
            }
            let count = self
                .demand_users
                .get_mut(&selection)
                .expect("indexed consumer demand");
            *count -= 1;
            if *count == 0 {
                self.demand_users.remove(&selection);
                if let Some(key) = self.find(&selection.descriptor()) {
                    released.push(key);
                }
            }
        }
        self.consumers.insert(world, consumer);
        for key in released {
            self.used.remove(&key);
            if !self.owned.contains(&key) {
                self.retain_idle_or_remove(key);
            }
        }
        self.prune_idle();
    }

    /// Release only a destroyed world's demand, producer content and event queue.
    pub(crate) fn release_world(&mut self, world: crate::WorldId) {
        #[cfg(feature = "gui")]
        {
            let systems: Vec<_> = self
                .consumers
                .get(&world)
                .into_iter()
                .flat_map(|consumer| consumer.system_demand.keys().copied())
                .collect();
            for system in systems {
                self.update_system_users(world, system, BTreeSet::new());
            }
        }
        #[cfg(not(feature = "gui"))]
        self.update_users(world, BTreeSet::new());
        if let Some(consumer) = self.consumers.remove(&world) {
            for key in consumer.owned {
                if !self
                    .consumers
                    .values()
                    .any(|consumer| consumer.owned.contains(&key))
                {
                    self.release(key);
                }
            }
        }
    }

    pub(crate) fn requests(
        &self,
        data: &DataSourceManagementService,
    ) -> Vec<AssetAcquisitionRequest> {
        data.take_selected_requests(|id| self.iter().any(|asset| asset.request_id() == Some(id)))
            .into_iter()
            .filter_map(|request| {
                let source = self
                    .iter()
                    .find(|asset| asset.request_id() == Some(request.id))?
                    .source();
                Some(AssetAcquisitionRequest {
                    id: request.id,
                    kind: source.kind,
                    source: request.identifier,
                    variant: source.variant,
                    recovery: request.recovery,
                })
            })
            .collect()
    }

    pub(crate) fn poll(&mut self, data: &mut DataSourceManagementService) {
        data.progress();
        self.poll_loads(data, &mut Context::from_waker(Waker::noop()));
    }

    pub(crate) fn set_evaluation_meshes(
        &mut self,
        world: crate::WorldId,
        demand: BTreeSet<AssetDemandSelection>,
    ) {
        #[cfg(feature = "profiling")]
        let _allocation_scope =
            crate::profiling::AllocationScope::new(204, "assets.set_evaluation_meshes");

        self.consumers.entry(world).or_default().evaluation_meshes = demand
            .iter()
            .map(|selection| Self::scoped_selection(world, selection))
            .collect();
    }

    pub(crate) fn poll_evaluation_assets(&mut self, data: &mut DataSourceManagementService) {
        data.progress();
        let keys = self
            .iter()
            .filter(|_asset| {
                if self.graphics_loaders.contains(&_asset.source().kind) {
                    return false;
                }
                if _asset.source().kind == crate::MESH_TYPE {
                    return self.consumers.values().any(|consumer| {
                        consumer.evaluation_meshes.iter().any(|selection| {
                            if crate::evaluation_scratch_reuse_enabled() {
                                let source = _asset.source();
                                selection.kind == source.kind
                                    && selection.variant == source.variant
                                    && selection.source == source.uri
                            } else {
                                selection.descriptor() == *_asset.source()
                            }
                        })
                    });
                }
                if _asset.source().kind == crate::TEXTURE_TYPE {
                    return false;
                }
                true
            })
            .map(|asset| asset.key())
            .collect();
        self.poll_selected_loads(data, &keys, &mut Context::from_waker(Waker::noop()));
    }

    pub(crate) fn snapshots(&self, world: crate::WorldId) -> Vec<AssetResourceSnapshot> {
        self.snapshot_page(world, 0, 0, usize::MAX)
    }

    pub(crate) fn snapshot_page(
        &self,
        world: crate::WorldId,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<AssetResourceSnapshot> {
        let cursor = if target != 0 {
            target
        } else {
            after
        };
        let start = if cursor & (1 << 63) == 0 {
            0
        } else {
            super::AssetKey::from_u64(cursor).slot as usize
        };
        let end = if target == 0 {
            self.assets.len()
        } else {
            start.saturating_add(1).min(self.assets.len())
        };
        // Runtime identities sort by slot, then incarnation. Resume directly in
        // stable storage; exact targets inspect just one slot.
        self.assets
            .get(start..end)
            .unwrap_or(&[])
            .iter()
            .filter_map(|slot| slot.provider.as_ref())
            .filter(|resource| {
                resource.key().to_u64() > after
                    && (target == 0 || resource.key().to_u64() == target)
            })
            .filter(|resource| {
                self.consumers.get(&world).is_some_and(|consumer| {
                    consumer.demand.contains(&AssetDemandSelection::new(
                        resource.source().kind,
                        &resource.source().uri,
                        resource.source().variant,
                    )) || (consumer.owned.contains(&resource.key())
                        && !consumer.internal.contains(&resource.key()))
                })
            })
            .take(limit)
            .map(|resource| AssetResourceSnapshot {
                id: resource.key().to_u64(),
                kind: resource.source().kind,
                source: Self::local_source(world, &resource.source().uri),
                variant: resource.source().variant,
                status: resource.status().clone(),
                representation: resource.representation(),
            })
            .collect()
    }

    fn local_source(world: crate::WorldId, source: &str) -> String {
        source
            .strip_prefix(&format!("producer://{}/", world.0))
            .map_or_else(|| source.to_owned(), |path| format!("asset://{path}"))
    }

    fn distribute_events(&mut self, events: Vec<AssetResourceSnapshot>) {
        for event in events {
            let source = AssetSource {
                kind: event.kind,
                uri: event.source.clone(),
                variant: event.variant,
            };
            for (&world, consumer) in &mut self.consumers {
                if consumer.observed_sources.contains(&source) {
                    let mut event = event.clone();
                    event.source = Self::local_source(world, &event.source);
                    consumer.events.push(event);
                }
            }
        }
    }

    fn take_consumer_events(
        &mut self,
        world: crate::WorldId,
    ) -> Result<Vec<AssetResourceSnapshot>, String> {
        if crate::allocation_optimizations_enabled()
            && self
                .consumers
                .get(&world)
                .is_some_and(|consumer| !consumer.observations_dirty)
        {
            return Ok(std::mem::take(
                &mut self.consumers.get_mut(&world).unwrap().events,
            ));
        }
        let Some(mut consumer) = self.consumers.remove(&world) else {
            return Ok(Vec::new());
        };
        consumer.observations_dirty = false;
        consumer.observed_sources = consumer
            .demand
            .iter()
            .map(AssetDemandSelection::descriptor)
            .collect();
        consumer.observed_sources.extend(
            consumer
                .owned
                .iter()
                .filter(|key| !consumer.internal.contains(key))
                .filter_map(|key| self.get(*key).map(|asset| asset.source().clone())),
        );
        let events = std::mem::take(&mut consumer.events);
        self.consumers.insert(world, consumer);
        Ok(events)
    }

    pub(crate) fn events(
        &mut self,
        world: crate::WorldId,
    ) -> Result<Vec<AssetResourceSnapshot>, String> {
        let events = Self::snapshots_from_events(self.take_events()?);
        self.distribute_events(events);
        self.take_consumer_events(world)
    }

    pub(crate) fn begin_reconcile(
        &mut self,
        world: crate::WorldId,
    ) -> Result<Vec<AssetResourceSnapshot>, String> {
        #[cfg(feature = "profiling")]
        let _allocation_scope =
            crate::profiling::AllocationScope::new(205, "assets.begin_reconcile");

        self.events(world)
    }

    pub(crate) fn finish_reconcile(&mut self, world: crate::WorldId) -> Vec<AssetResourceSnapshot> {
        #[cfg(feature = "profiling")]
        let _allocation_scope =
            crate::profiling::AllocationScope::new(206, "assets.finish_reconcile");

        let events = Self::snapshots_from_events(self.take_reconciled_events());
        self.distribute_events(events);
        self.take_consumer_events(world)
            .expect("resource observation queue is available")
    }

    fn snapshots_from_events(events: Vec<AssetLoadProgress>) -> Vec<AssetResourceSnapshot> {
        events
            .into_iter()
            .map(|event| AssetResourceSnapshot {
                id: event.key.to_u64(),
                kind: event.source.kind,
                source: event.source.uri,
                variant: event.source.variant,
                status: event.status,
                representation: event.representation,
            })
            .collect()
    }
}

impl AssetManagementService {
    pub(crate) fn release_upload(&mut self, world: crate::WorldId, key: AssetUploadIdentity) {
        let source = Self::scoped_selection(
            world,
            &AssetDemandSelection::new(
                key.kind,
                &format!("asset://{}/{}", key.kind.0, key.asset),
                key.variant,
            ),
        )
        .descriptor();
        if let Some(key) = self.find(&source) {
            if let Some(consumer) = self.consumers.get_mut(&world) {
                consumer.observations_dirty = true;
                consumer.owned.remove(&key);
            }
            if !self
                .consumers
                .values()
                .any(|consumer| consumer.owned.contains(&key))
            {
                self.release(key);
            }
        }
    }
}

impl AssetManagementService {
    pub(crate) fn lifecycle_snapshot(
        &self,
        world: crate::WorldId,
        event: &super::AssetLifecycleEvent,
    ) -> Option<AssetResourceSnapshot> {
        let retained_recipient = self
            .lifecycle_recipients
            .get(&event.key)
            .is_some_and(|worlds| worlds.contains(&world))
            && (event.kind == super::AssetLifecycleKind::Removed
                || event.status == AssetLoadStatus::Unloaded);
        let consumer = self.consumers.get(&world)?;
        if !retained_recipient
            && !consumer.observed_sources.contains(&event.source)
            && !consumer
                .demand
                .iter()
                .any(|demand| demand.descriptor() == event.source)
            && !(consumer.owned.contains(&event.key) && !consumer.internal.contains(&event.key))
        {
            return None;
        }
        Some(AssetResourceSnapshot {
            id: event.key.to_u64(),
            kind: event.source.kind,
            source: Self::local_source(world, &event.source.uri),
            variant: event.source.variant,
            status: event.status.clone(),
            representation: event.representation,
        })
    }
}

#[cfg(feature = "skeletal-animation")]
impl AssetManagementService {
    /// Resolve immutable typed content through the owning World's resource namespace.
    pub(crate) fn source_data<T: 'static>(
        &self,
        world: crate::WorldId,
        kind: AssetTypeId,
        source: &str,
        variant: u32,
    ) -> Option<(AssetKey, &T)> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(201, "assets.source_data");

        if crate::allocation_optimizations_enabled() {
            let key = self.find_source(world, kind, source, variant)?;
            return Some((key, self.get_typed(key)?));
        }
        let selection = AssetDemandSelection::new(kind, source, variant);
        let key = self.find(&Self::scoped_selection(world, &selection).descriptor())?;
        Some((key, self.get_typed(key)?))
    }
}
