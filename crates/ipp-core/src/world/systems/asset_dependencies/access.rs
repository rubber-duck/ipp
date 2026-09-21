use super::*;

impl AssetDependencySystem {
    pub(in crate::world) fn access<'a>(
        &'a mut self,
        runtime: &'a mut SystemRuntimeAccess<'_>,
    ) -> AssetDependencyAccess<'a> {
        AssetDependencyAccess {
            world: runtime.world,
            state: &mut self.state,
            asset_acquisition: runtime.asset_acquisition,
            data_sources: runtime.data_sources,
        }
    }
}

impl AssetDependencyAccess<'_> {
    /// Drain newly issued provider work once; source I/O belongs to the host.
    pub fn take_resource_requests(&mut self) -> Vec<crate::AssetAcquisitionRequest> {
        self.asset_acquisition.requests(self.data_sources)
    }

    /// Drain cancelled host input streams.
    pub fn take_resource_cancellations(&mut self) -> Vec<u64> {
        self.data_sources.take_cancellations()
    }

    /// Queue owned provider data for the next boundary. Obsolete input streams are harmless.
    pub fn complete_resource(
        &mut self,
        id: u64,
        result: Result<Vec<u8>, String>,
    ) -> Result<(), ErrorReason> {
        self.data_sources
            .complete_read(id, result)
            .map_err(|_| ErrorReason::Capacity)
    }

    pub(in crate::world) fn begin_resource_reconcile(
        &mut self,
    ) -> Result<Vec<crate::AssetResourceSnapshot>, ErrorReason> {
        let changes = self
            .asset_acquisition
            .begin_reconcile(self.world.id)
            .map_err(|_| ErrorReason::Capacity)?;
        let mut prepared = std::mem::take(&mut self.state.prepared_changes);
        prepared.extend(changes);
        Ok(prepared)
    }

    /// Bind a URI scheme to this world's bounded external stream bridge.
    ///
    /// Hosts install only schemes they implement during initialization. Source
    /// reads remain asynchronous and use the ordinary request/cancellation APIs.
    pub fn register_stream_resource_provider(&mut self, scheme: &str) -> Result<(), ErrorReason> {
        self.data_sources
            .register_stream(&format!("{scheme}:"))
            .map_err(|_| ErrorReason::InvalidValue)
    }

    /// Let the renderer drive concrete resource loading during stage 7.
    pub fn set_renderer_asset_loading(&mut self, enabled: bool) {
        self.asset_acquisition.renderer_driven = enabled;
    }

    /// Progress all shared loaders at an embedding Host boundary with an existing
    /// world borrow. Multi-world hosts call Host::progress_assets once instead.
    pub fn poll_all_assets(&mut self) {
        self.asset_acquisition.poll(self.data_sources);
    }

    /// Poll resource-owned loaders during the caller's host execution phase.
    pub fn poll_assets(&mut self) {
        if self.asset_acquisition.renderer_driven {
            self.asset_acquisition
                .poll_evaluation_assets(self.data_sources);
        } else {
            self.asset_acquisition.poll(self.data_sources);
        }
    }

    /// Drain lifecycle observations after renderer work and before frame events.
    pub fn take_asset_events(&mut self) -> Result<Vec<crate::AssetResourceSnapshot>, String> {
        self.asset_acquisition.events(self.world.id)
    }

    /// Queue producer bytes through the same factories used for source references.
    pub fn enqueue_asset(
        &mut self,
        upload: crate::services::asset_management::AssetUpload,
    ) -> Result<(), ErrorReason> {
        if upload.key.asset >= 1 << 63 {
            return Err(ErrorReason::InvalidAsset);
        }
        self.state
            .asset_queue
            .try_reserve(1)
            .map_err(|_| ErrorReason::Capacity)?;
        self.state.asset_queue.push_back(upload);
        Ok(())
    }

    pub(in crate::world) fn accept_asset_uploads(&mut self, _tick: u64) {
        while let Some(upload) = self.state.asset_queue.pop_front() {
            match self
                .asset_acquisition
                .upload_world(self.world.id, upload.key, upload.bytes)
            {
                Ok(key) => self
                    .state
                    .asset_pending
                    .push(system_state::PendingAssetUpload {
                        request_id: upload.id,
                        producer_key: upload.key,
                        resource_key: Some(key),
                        error: None,
                    }),
                Err(error) => self
                    .state
                    .asset_pending
                    .push(system_state::PendingAssetUpload {
                        request_id: upload.id,
                        producer_key: upload.key,
                        resource_key: None,
                        error: Some(error),
                    }),
            }
        }
    }

    /// Complete upload receipts after the resource itself reports availability.
    pub fn take_asset_outcomes(
        &mut self,
    ) -> Vec<crate::services::asset_management::AssetUploadOutcome> {
        let mut outcomes = Vec::new();
        self.state.asset_pending.retain(|pending| {
            let (id, key, error) = (pending.request_id, pending.producer_key, &pending.error);
            if pending
                .resource_key
                .is_some_and(|key| self.asset_acquisition.has_pending_release(key))
            {
                return true;
            }
            let result = if let Some(error) = error {
                Err(error.clone())
            } else {
                match pending
                    .resource_key
                    .and_then(|key| self.asset_acquisition.get(key))
                {
                    Some(asset) => match asset.status() {
                        crate::services::asset_management::AssetLoadStatus::Loaded => {
                            Ok(asset.stats())
                        }
                        crate::services::asset_management::AssetLoadStatus::Failed(error) => {
                            Err(error.clone())
                        }
                        _ => return true,
                    },
                    None => Err("Asset was released".into()),
                }
            };
            outcomes.push(crate::services::asset_management::AssetUploadOutcome {
                id,
                key,
                tick: self.world.tick,
                result,
            });
            false
        });
        outcomes
    }
}

impl AssetDependencyReadAccess<'_> {
    /// Observe current typed demand in kind/source/variant order.
    pub fn resource_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<crate::AssetResourceSnapshot> {
        self.asset_acquisition
            .snapshot_page(self.world.id, after, target, limit)
    }

    /// Observe current demand.
    pub fn resource_snapshots(&self) -> Vec<crate::AssetResourceSnapshot> {
        self.asset_acquisition.snapshots(self.world.id)
    }

    /// Feed a bounded stream chunk; false applies backpressure to the host.
    pub fn asset_input_chunk(&self, id: u64, bytes: &[u8]) -> Result<bool, String> {
        self.data_sources.input_chunk(id, bytes)
    }

    /// End exactly this input reader. Stale readers cannot reach a new load.
    pub fn asset_input_end(&self, id: u64, result: Result<(), String>) {
        self.data_sources.input_end(id, result);
    }

    /// Aggregate retained asynchronous source staging, including unused capacity.
    pub fn asset_input_bytes(&self) -> usize {
        self.data_sources.input_bytes()
    }

    /// Number of upload replies still awaiting a resource-owned result.
    pub fn pending_asset_uploads(&self) -> usize {
        self.state.asset_queue.len() + self.state.asset_pending.len()
    }
}

impl crate::WorldContext<'_> {
    fn asset_read(&self) -> AssetDependencyReadAccess<'_> {
        AssetDependencyReadAccess {
            world: self.world,
            state: &self
                .system::<AssetDependencySystem>(AssetDependencySystem::ID)
                .expect("World requires AssetDependencySystem")
                .state,
            asset_acquisition: self.asset_acquisition,
            data_sources: self.data_sources,
        }
    }

    fn with_asset_dependency<R>(
        &mut self,
        operation: impl FnOnce(&mut AssetDependencyAccess<'_>) -> R,
    ) -> R {
        self.with_system::<AssetDependencySystem, _>(
            AssetDependencySystem::ID,
            |system, runtime| operation(&mut system.access(runtime)),
        )
        .expect("World requires AssetDependencySystem")
    }

    /// Drain newly issued provider work once; source I/O belongs to the host.
    pub fn take_resource_requests(&mut self) -> Vec<crate::AssetAcquisitionRequest> {
        self.with_asset_dependency(|access| access.take_resource_requests())
    }

    /// Drain cancelled host input streams.
    pub fn take_resource_cancellations(&mut self) -> Vec<u64> {
        self.with_asset_dependency(|access| access.take_resource_cancellations())
    }

    /// Queue owned provider data for the next boundary. Obsolete input streams are harmless.
    pub fn complete_resource(
        &mut self,
        id: u64,
        result: Result<Vec<u8>, String>,
    ) -> Result<(), ErrorReason> {
        self.with_asset_dependency(|access| access.complete_resource(id, result))
    }

    /// Observe current typed demand in kind/source/variant order.
    pub fn resource_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<crate::AssetResourceSnapshot> {
        self.asset_read().resource_page(after, target, limit)
    }

    /// Observe current demand.
    pub fn resource_snapshots(&self) -> Vec<crate::AssetResourceSnapshot> {
        self.asset_read().resource_snapshots()
    }

    /// Bind a URI scheme to this world's bounded external stream bridge.
    ///
    /// Hosts install only schemes they implement during initialization. Source
    /// reads remain asynchronous and use the ordinary request/cancellation APIs.
    pub fn register_stream_resource_provider(&mut self, scheme: &str) -> Result<(), ErrorReason> {
        self.with_asset_dependency(|access| access.register_stream_resource_provider(scheme))
    }

    /// Resolve a world-local producer key or a retained Host resource identity.
    pub fn resolve_asset_key(
        &self,
        key: crate::services::asset_management::AssetUploadIdentity,
    ) -> Option<crate::services::asset_management::AssetKey> {
        resolve_asset_key(self.asset_acquisition, self.world.id, key)
    }

    /// Shared generic resources. A rendering host installs its factories before use.
    pub fn asset_resources(&self) -> &crate::services::asset_management::AssetManagementService {
        self.asset_acquisition
    }

    /// Exclusive access to registration, loading and residency at a safe host phase.
    pub fn asset_resources_mut(
        &mut self,
    ) -> &mut crate::services::asset_management::AssetManagementService {
        self.asset_acquisition
    }

    /// Let the renderer drive concrete resource loading during stage 7.
    pub fn set_renderer_asset_loading(&mut self, enabled: bool) {
        self.with_asset_dependency(|access| access.set_renderer_asset_loading(enabled))
    }

    /// Progress all shared loaders at an embedding Host boundary with an existing
    /// world borrow. Multi-world hosts call Host::progress_assets once instead.
    pub fn poll_all_assets(&mut self) {
        self.with_asset_dependency(|access| access.poll_all_assets())
    }

    /// Poll resource-owned loaders during the caller's host execution phase.
    pub fn poll_assets(&mut self) {
        self.with_asset_dependency(|access| access.poll_assets())
    }

    /// Drain lifecycle observations after renderer work and before frame events.
    pub fn take_asset_events(&mut self) -> Result<Vec<crate::AssetResourceSnapshot>, String> {
        self.with_asset_dependency(|access| access.take_asset_events())
    }

    /// Feed a bounded stream chunk; false applies backpressure to the host.
    pub fn asset_input_chunk(&self, id: u64, bytes: &[u8]) -> Result<bool, String> {
        self.asset_read().asset_input_chunk(id, bytes)
    }

    /// End exactly this input reader. Stale readers cannot reach a new load.
    pub fn asset_input_end(&self, id: u64, result: Result<(), String>) {
        self.asset_read().asset_input_end(id, result)
    }

    /// Aggregate retained asynchronous source staging, including unused capacity.
    pub fn asset_input_bytes(&self) -> usize {
        self.asset_read().asset_input_bytes()
    }

    /// Queue producer bytes through the same factories used for source references.
    pub fn enqueue_asset(
        &mut self,
        upload: crate::services::asset_management::AssetUpload,
    ) -> Result<(), ErrorReason> {
        self.with_asset_dependency(|access| access.enqueue_asset(upload))
    }

    /// Complete upload receipts after the resource itself reports availability.
    pub fn take_asset_outcomes(
        &mut self,
    ) -> Vec<crate::services::asset_management::AssetUploadOutcome> {
        self.with_asset_dependency(|access| access.take_asset_outcomes())
    }

    /// Number of upload replies still awaiting a resource-owned result.
    pub fn pending_asset_uploads(&self) -> usize {
        self.asset_read().pending_asset_uploads()
    }
}

impl crate::WorldContext<'_> {
    /// Release one producer's uploaded registration. Remaining World demand retains
    /// the immutable source for its live consumers and context recovery.
    pub fn release_asset_upload(
        &mut self,
        key: crate::services::asset_management::AssetUploadIdentity,
    ) {
        self.with_asset_dependency(|access| access.release_asset_upload(key));
    }
}

/// Resolve a world-local producer key or a retained Host resource identity.
pub(in crate::world) fn resolve_asset_key(
    assets: &AssetManagementService,
    world: crate::WorldId,
    key: crate::services::asset_management::AssetUploadIdentity,
) -> Option<crate::services::asset_management::AssetKey> {
    if key.asset >= 1 << 63 {
        let runtime = crate::services::asset_management::AssetKey::from_u64(key.asset);
        return assets
            .get(runtime)
            .filter(|provider| {
                provider.source().kind == key.kind && provider.source().variant == key.variant
            })
            .map(|_| runtime);
    }
    let selection = AssetDemandSelection::new(
        key.kind,
        &format!("asset://{}/{}", key.kind.0, key.asset),
        key.variant,
    );
    assets.find(
        &crate::services::asset_management::service::AssetManagementService::scoped_selection(
            world, &selection,
        )
        .descriptor(),
    )
}

impl AssetDependencyAccess<'_> {
    fn release_asset_upload(
        &mut self,
        key: crate::services::asset_management::AssetUploadIdentity,
    ) {
        let source = format!(
            "producer://{}/{}/{}",
            self.world.id.0, key.kind.0, key.asset
        );
        if self.state.source_users.keys().any(|selection| {
            let scoped = AssetManagementService::scoped_selection(self.world.id, selection);
            scoped.kind == key.kind && scoped.source == source && scoped.variant == key.variant
        }) {
            return;
        }
        self.asset_acquisition.release_upload(self.world.id, key);
    }
}
