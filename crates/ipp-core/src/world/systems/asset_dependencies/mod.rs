//! Generic resource demand and owned ingress, retained by each world.

use crate::world::{WorldEntityState, WorldSimulationState, systems::SystemRuntimeAccess};
use crate::{
    EntityId, ErrorReason,
    services::asset_management::service::{AssetDemandSelection, AssetManagementService},
};
use std::collections::BTreeSet;

pub(in crate::world) type AssetSourceKeyCache =
    std::cell::RefCell<Vec<Option<(crate::EntityId, crate::services::asset_management::AssetKey)>>>;

pub(in crate::world) struct AssetDependencyAccess<'a> {
    world: &'a mut WorldSimulationState,
    state: &'a mut AssetDependencySystemState,
    asset_acquisition: &'a mut AssetManagementService,
    data_sources: &'a mut crate::services::data_source::DataSourceManagementService,
}

pub(in crate::world) struct AssetDependencyReadAccess<'a> {
    world: &'a WorldSimulationState,
    state: &'a AssetDependencySystemState,
    asset_acquisition: &'a AssetManagementService,
    data_sources: &'a crate::services::data_source::DataSourceManagementService,
}

mod demand;

mod system_state;
pub use system_state::AssetDependencySystemState;

mod system;
pub use system::{AssetDependencySystem, AssetDependencySystemFactory};

#[cfg(test)]
mod asset_cache_tests;

mod asset_cache;
pub(in crate::world) use asset_cache::{cached_source_key, source_key_from_fields};

mod access;
pub(in crate::world) use access::resolve_asset_key;
