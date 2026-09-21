//! Slot allocation, immutable source lookup and explicit retention accounting.

use super::{
    resource::{AssetLoaderConstructor, TypedAssetLoader},
    *,
};
use crate::services::data_source::{DataSourceManagementService, MemoryDataSource};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
    task::Context,
};

pub(super) struct AssetSlot {
    pub(super) generation: u32,
    pub(super) provider: Option<AssetProvider>,
}

impl AssetManagementService {
    /// Construct an empty catalog for custom compiled payloads.
    pub fn empty() -> Self {
        Self {
            demand_users: BTreeMap::new(),
            consumers: BTreeMap::new(),
            assets: Vec::new(),
            free: Vec::new(),
            sources: BTreeMap::new(),
            loaders: BTreeMap::new(),
            graphics_loaders: BTreeSet::new(),
            used: BTreeSet::new(),
            owned: BTreeSet::new(),
            idle: BTreeMap::new(),
            idle_resident_bytes: 0,
            idle_epoch: 0,
            idle_resident_bytes_target: Self::DEFAULT_IDLE_RESIDENT_BYTES_TARGET,
            memory: MemoryDataSource::default(),
            events: VecDeque::new(),
            lifecycle_barrier: false,
            lifecycle_recipients: BTreeMap::new(),
            pending_releases: BTreeMap::new(),
            lifecycle_events: VecDeque::new(),
            renderer_driven: false,
        }
    }

    /// Install the private producer-input namespace in the Host's generic router.
    pub fn install_data_sources(
        &self,
        sources: &mut DataSourceManagementService,
    ) -> Result<(), String> {
        sources.register("asset-memory:", self.memory.clone())
    }

    /// Register a decoder constructor before any resources of the type exist.
    pub fn register_loader<L: AssetLoader>(
        &mut self,
        kind: AssetTypeId,
        make: impl Fn() -> L + 'static,
    ) -> Result<(), String> {
        self.register_loader_with_ownership(kind, make, false)
    }

    /// Register a decoder whose polling requires a live graphics context.
    pub fn register_graphics_loader<L: AssetLoader>(
        &mut self,
        kind: AssetTypeId,
        make: impl Fn() -> L + 'static,
    ) -> Result<(), String> {
        self.register_loader_with_ownership(kind, make, true)
    }

    fn register_loader_with_ownership<L: AssetLoader>(
        &mut self,
        kind: AssetTypeId,
        make: impl Fn() -> L + 'static,
        graphics: bool,
    ) -> Result<(), String> {
        if kind.0 == 0 || self.iter().any(|provider| provider.source().kind == kind) {
            return Err("Loader cannot change while resources of that type exist".into());
        }
        let constructor: AssetLoaderConstructor =
            Rc::new(move || Box::new(TypedAssetLoader(make())));
        self.loaders.insert(kind, constructor);
        if graphics {
            self.graphics_loaders.insert(kind);
        } else {
            self.graphics_loaders.remove(&kind);
        }
        Ok(())
    }

    /// Register producer bytes through ordinary generic input and typed decoding.
    pub fn upload(
        &mut self,
        identity: AssetUploadIdentity,
        bytes: Vec<u8>,
    ) -> Result<AssetKey, String> {
        if identity.asset == 0 || identity.asset >= 1 << 63 {
            return Err("InvalidAsset".into());
        }
        self.upload_named(
            AssetSource {
                kind: identity.kind,
                uri: format!("asset://{}/{}", identity.kind.0, identity.asset),
                variant: identity.variant,
            },
            bytes,
        )
    }

    pub(crate) fn upload_named(
        &mut self,
        source: AssetSource,
        bytes: Vec<u8>,
    ) -> Result<AssetKey, String> {
        let existing = self.find(&source);
        let key = self.get_or_create(source)?;
        if self
            .get(key)
            .is_some_and(|provider| provider.owned_input.is_some())
        {
            return Err("DuplicateAsset".into());
        }
        let identifier = format!("asset-memory:{}:{}", key.slot, key.generation);
        if let Err(error) = self.memory.insert(identifier.clone(), bytes) {
            if existing.is_none() {
                self.remove_slot(key);
            }
            return Err(error);
        }
        // Adopt the stable identity before invalidating its external payload. This
        // removes idle-cache membership and cancels a deferred orphan removal.
        self.retain_owned(key);
        self.unload(key);
        self.assets[key.slot as usize]
            .provider
            .as_mut()
            .expect("live resource")
            .adopt_owned_input(identifier);
        Ok(key)
    }

    /// Release one producer registration; retained consumer demand remains authoritative.
    pub fn release(&mut self, key: AssetKey) {
        self.owned.remove(&key);
        self.free_unused();
    }

    pub(super) fn retain_owned(&mut self, key: AssetKey) {
        if self.pending_releases.get(&key) == Some(&super::AssetReleaseKind::Remove) {
            self.pending_releases.remove(&key);
        }
        self.remove_idle(key);
        self.owned.insert(key);
    }

    /// Share immutable typed content, allocating a fresh generation when a slot is reused.
    pub fn get_or_create(&mut self, source: AssetSource) -> Result<AssetKey, String> {
        validate_owned_source(&source)?;
        if let Some(key) = self.find(&source) {
            return Ok(key);
        }
        let loader = self
            .loaders
            .get(&source.kind)
            .cloned()
            .ok_or("Asset type is unavailable in this build")?;
        let index = if let Some(index) = self.free.pop() {
            index
        } else {
            if self.assets.len() >= 1 << 31 {
                return Err("Asset slot index space exhausted".into());
            }
            let index = self.assets.len() as u32;
            self.assets
                .try_reserve(1)
                .map_err(|error| error.to_string())?;
            self.assets.push(AssetSlot {
                generation: 1,
                provider: None,
            });
            index
        };
        let slot = &mut self.assets[index as usize];
        let key = AssetKey {
            slot: index,
            generation: slot.generation,
        };
        slot.provider = Some(AssetProvider::new(key, source.clone(), loader));
        self.sources.insert(source, key);
        Ok(key)
    }

    /// Loaded allocation estimate across all providers.
    pub fn resident_bytes(&self) -> usize {
        self.iter()
            .map(|provider| provider.stats().resident_bytes)
            .sum()
    }

    /// Configure the soft memory target for completed resources without active users.
    /// Active demand and producer ownership are never constrained by this target.
    pub fn set_idle_resident_bytes_target(&mut self, target: usize) {
        self.idle_resident_bytes_target = target;
        self.prune_idle();
    }

    /// Current soft memory target for completed resources without active users.
    pub fn idle_resident_bytes_target(&self) -> usize {
        self.idle_resident_bytes_target
    }

    /// Measured decoded and graphics storage currently retained without active users.
    pub fn idle_resident_bytes(&self) -> usize {
        self.idle_resident_bytes
    }

    /// Resolve an immutable source once, without creating it.
    pub fn find(&self, source: &AssetSource) -> Option<AssetKey> {
        self.sources.get(source).copied()
    }

    /// Indexed generation-checked lookup; no source tree lookup occurs.
    pub fn get(&self, key: AssetKey) -> Option<&AssetProvider> {
        let slot = self.assets.get(key.slot as usize)?;
        if slot.generation != key.generation {
            return None;
        }
        slot.provider.as_ref()
    }

    /// Borrow retained decoded data or metadata after validating slot generation.
    /// Graphics readiness is not required for CPU access.
    pub fn get_typed<T: Any>(&self, key: AssetKey) -> Option<&T> {
        let data = self.get(key)?.data()?;
        data.decoded()
            .downcast_ref()
            .or_else(|| data.metadata().downcast_ref())
    }

    /// Visit live providers in deterministic slot order.
    pub fn iter(&self) -> impl Iterator<Item = &AssetProvider> {
        self.assets.iter().filter_map(|slot| slot.provider.as_ref())
    }

    /// Validate proposed aggregate demand before its owner commits.
    pub fn validate_sources(&self, sources: &BTreeSet<AssetSource>) -> Result<(), String> {
        for source in sources {
            validate_owned_source(source)?;
            if !self.loaders.contains_key(&source.kind) {
                return Err("Asset type is unavailable".into());
            }
        }
        Ok(())
    }

    /// Explicit retained consumer set. Copying a key never changes ownership.
    pub fn set_used(&mut self, used: BTreeSet<AssetKey>) {
        self.pending_releases.retain(|key, release| {
            *release != super::AssetReleaseKind::Remove || !used.contains(key)
        });
        for key in &used {
            self.remove_idle(*key);
        }
        self.used = used;
    }

    pub(super) fn remove_slot(&mut self, key: AssetKey) {
        self.remove_idle(key);
        if self.defer_release(key, super::AssetReleaseKind::Remove) {
            return;
        }
        self.remove_slot_released(key);
    }

    pub(super) fn remove_slot_released(&mut self, key: AssetKey) {
        self.remove_idle(key);
        let Some(slot) = self
            .assets
            .get_mut(key.slot as usize)
            .filter(|slot| slot.generation == key.generation)
        else {
            return;
        };
        let Some(mut provider) = slot.provider.take() else {
            return;
        };
        // Invalidate before freeing loaded allocations or making the slot reusable.
        slot.generation = slot.generation.wrapping_add(1);
        let mut events = Vec::new();
        provider.unload(&mut events);
        self.sources.remove(provider.source());
        if let Some(identifier) = &provider.owned_input {
            self.memory.remove(identifier);
        }
        self.free.push(key.slot);
        for event in events {
            self.load_progress(event);
        }
    }

    /// Request destruction of resources absent from every committed retention set.
    pub fn free_unused(&mut self) {
        let stale: Vec<_> = self
            .iter()
            .map(AssetProvider::key)
            .filter(|key| !self.used.contains(key) && !self.owned.contains(key))
            .collect();
        for key in stale {
            self.retain_idle_or_remove(key);
        }
        self.prune_idle();
    }

    pub(super) fn retain_idle_or_remove(&mut self, key: AssetKey) {
        let cacheable = self.get(key).is_some_and(|provider| {
            provider.owned_input.is_none()
                && provider.status() == &AssetLoadStatus::Loaded
                && provider.stats().resident_bytes > 0
        });
        if cacheable {
            if !self.idle.contains_key(&key) {
                self.idle_epoch = self.idle_epoch.saturating_add(1);
                let resident_bytes = self
                    .get(key)
                    .expect("cacheable resource")
                    .stats()
                    .resident_bytes;
                self.idle.insert(
                    key,
                    super::service::IdleAsset {
                        last_used: self.idle_epoch,
                        resident_bytes,
                    },
                );
                self.idle_resident_bytes = self.idle_resident_bytes.saturating_add(resident_bytes);
            }
        } else {
            self.remove_slot(key);
        }
    }

    pub(super) fn prune_idle(&mut self) {
        if self.idle_resident_bytes <= self.idle_resident_bytes_target {
            return;
        }
        let now = self.idle_epoch.saturating_add(1);
        let mut candidates: Vec<_> = self
            .idle
            .iter()
            .filter(|(key, _)| !self.used.contains(key) && !self.owned.contains(key))
            .map(|(&key, entry)| {
                let age = now.saturating_sub(entry.last_used).max(1);
                let score = (entry.resident_bytes as u128).saturating_mul(age as u128);
                (score, entry.resident_bytes, age, key)
            })
            .collect();
        candidates.sort_unstable_by(|left, right| right.cmp(left));
        for (_, _, _, key) in candidates {
            if self.idle_resident_bytes <= self.idle_resident_bytes_target {
                break;
            }
            self.remove_slot(key);
        }
    }

    pub(super) fn remove_idle(&mut self, key: AssetKey) -> Option<super::service::IdleAsset> {
        let entry = self.idle.remove(&key)?;
        self.idle_resident_bytes = self
            .idle_resident_bytes
            .saturating_sub(entry.resident_bytes);
        Some(entry)
    }

    fn refresh_idle_bytes(&mut self, key: AssetKey, resident_bytes: usize) {
        let Some(entry) = self.idle.get_mut(&key) else {
            return;
        };
        self.idle_resident_bytes = self
            .idle_resident_bytes
            .saturating_sub(entry.resident_bytes)
            .saturating_add(resident_bytes);
        entry.resident_bytes = resident_bytes;
    }

    /// Progress loading at the Host's selected phase with borrowed generic I/O.
    pub fn poll_loads(&mut self, sources: &mut DataSourceManagementService, cx: &mut Context<'_>) {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(211, "assets.poll");
        if crate::allocation_optimizations_enabled() {
            // Polling cannot insert, remove or reuse catalog slots. Loader callbacks
            // receive only their resource and the I/O service; lifecycle work is queued.
            for slot in 0..self.assets.len() {
                if let Some(key) = self.assets[slot].provider.as_ref().map(AssetProvider::key) {
                    self.poll_load_key(sources, key, cx);
                }
            }
        } else {
            let keys = self.iter().map(AssetProvider::key).collect();
            self.poll_selected_loads(sources, &keys, cx);
        }
    }

    /// Progress selected representations without duplicating lifecycle state.
    pub fn poll_selected_loads(
        &mut self,
        sources: &mut DataSourceManagementService,
        keys: &BTreeSet<AssetKey>,
        cx: &mut Context<'_>,
    ) {
        for &key in keys {
            self.poll_load_key(sources, key, cx);
        }
    }

    fn poll_load_key(
        &mut self,
        sources: &mut DataSourceManagementService,
        key: AssetKey,
        cx: &mut Context<'_>,
    ) {
        if self.pending_releases.contains_key(&key) || self.get(key).is_none() {
            return;
        }
        let mut events = Vec::new();
        self.assets[key.slot as usize]
            .provider
            .as_mut()
            .expect("checked slot")
            .poll_load(sources, cx, &mut events);
        for event in events {
            self.load_progress(event);
        }
    }

    /// Request payload release. Referenced or owned identities remain available for
    /// recovery; an unreferenced idle identity is discarded to prevent reacquisition.
    /// Host-owned services finish release after synchronous World invalidation.
    pub fn unload(&mut self, key: AssetKey) {
        if self.idle.contains_key(&key) {
            self.remove_slot(key);
            return;
        }
        if self.defer_release(key, super::AssetReleaseKind::Unload) {
            return;
        }
        self.unload_released(key);
    }

    /// Invalidate only graphics storage through the synchronous Host barrier.
    pub fn invalidate_graphics(&mut self, key: AssetKey) {
        if !self.defer_release(key, super::AssetReleaseKind::Graphics) {
            self.invalidate_graphics_released(key);
        }
    }

    pub(super) fn invalidate_graphics_released(&mut self, key: AssetKey) {
        let Some(slot) = self
            .assets
            .get_mut(key.slot as usize)
            .filter(|slot| slot.generation == key.generation)
        else {
            return;
        };
        let mut events = Vec::new();
        if let Some(provider) = &mut slot.provider {
            provider.invalidate_graphics(&mut events);
        }
        for event in events {
            self.load_progress(event);
        }
    }

    pub(super) fn unload_released(&mut self, key: AssetKey) {
        let Some(slot) = self
            .assets
            .get_mut(key.slot as usize)
            .filter(|slot| slot.generation == key.generation)
        else {
            return;
        };
        let mut events = Vec::new();
        if let Some(provider) = &mut slot.provider {
            provider.unload(&mut events);
        }
        for event in events {
            self.load_progress(event);
        }
    }

    /// Unload every representation at an exclusive Host boundary.
    pub fn unload_all(&mut self) {
        let keys: Vec<_> = self.iter().map(AssetProvider::key).collect();
        for key in keys {
            self.unload(key);
        }
    }

    pub(super) fn load_progress(&mut self, report: AssetLoadProgress) {
        let key = report.key;
        let completed = report.status == AssetLoadStatus::Loaded;
        let resident_bytes = self
            .get(key)
            .map_or(0, |provider| provider.stats().resident_bytes);
        self.refresh_idle_bytes(key, resident_bytes);
        self.record_lifecycle(&report);
        if matches!(report.status, AssetLoadStatus::Progress { .. })
            && self.events.back().is_some_and(|previous| {
                previous.key == report.key
                    && matches!(previous.status, AssetLoadStatus::Progress { .. })
            })
        {
            self.events.pop_back();
        }
        self.events.push_back(report);
        if completed && !self.used.contains(&key) && !self.owned.contains(&key) {
            self.retain_idle_or_remove(key);
            self.prune_idle();
        }
    }

    /// Drain lifecycle observations in their committed order.
    pub fn take_events(&mut self) -> Result<Vec<AssetLoadProgress>, String> {
        Ok(self.events.drain(..).collect())
    }

    pub(crate) fn take_reconciled_events(&mut self) -> Vec<AssetLoadProgress> {
        self.take_events()
            .expect("validated reconciliation event reservation")
    }
}

fn validate_owned_source(source: &AssetSource) -> Result<(), String> {
    if let Some(path) = source.uri.strip_prefix("producer://") {
        let parts: Vec<_> = path.split('/').collect();
        if parts.len() != 3 {
            return Err("InvalidAsset".into());
        }
        let world: u64 = parts[0].parse().map_err(|_| "InvalidAsset")?;
        let kind: u16 = parts[1].parse().map_err(|_| "InvalidAsset")?;
        let asset: u64 = parts[2].parse().map_err(|_| "InvalidAsset")?;
        if world == 0
            || kind != source.kind.0
            || asset == 0
            || asset >= 1 << 63
            || source.uri != format!("producer://{world}/{kind}/{asset}")
        {
            return Err("InvalidAsset".into());
        }
        return Ok(());
    }
    let Some(path) = source.uri.strip_prefix("asset://") else {
        return Ok(());
    };
    let (kind, asset) = path.split_once('/').ok_or("Invalid owned asset URI")?;
    let kind: u16 = kind.parse().map_err(|_| "Invalid owned asset URI")?;
    let asset: u64 = asset.parse().map_err(|_| "Invalid owned asset URI")?;
    if kind != source.kind.0
        || asset == 0
        || asset >= 1 << 63
        || source.uri != format!("asset://{kind}/{asset}")
    {
        return Err("Invalid owned asset URI".into());
    }
    Ok(())
}
