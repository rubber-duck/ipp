//! Host-boundary access to the shared asset, I/O and data services, and resource
//! lifecycle delivery to every World.

use super::*;

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
