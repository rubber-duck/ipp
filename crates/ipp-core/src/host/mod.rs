//! Direct ownership of headless services and independently updatable worlds.

use std::collections::BTreeMap;

use crate::systems::{
    SystemFactories, SystemFactory, SystemId, SystemScheduleError, compiled_system_factories,
};
use crate::{ErrorReason, World, WorldConstructionError, WorldContext, WorldLimits};
use std::sync::Arc;

/// Host-local world identity. Never reused during the Host lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldId(pub u64);

/// Headless runtime owned directly by a Host alongside its platform services.
/// Worlds are destroyed before services, including during ordinary Rust drop.
pub struct HostRuntime {
    worlds: BTreeMap<WorldId, World>,
    next_world: u64,
    identity_namespace: u64,
    system_factories: SystemFactories,
    data_sources: crate::services::data_source::DataSourceManagementService,
    assets: crate::services::asset_management::service::AssetManagementService,
}

impl Default for HostRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HostRuntime {
    fn drop(&mut self) {
        while let Some(id) = self.worlds.keys().next().copied() {
            self.destroy_world(id);
        }
    }
}

mod persistence;

mod runtime;
