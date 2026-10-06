//! The World: one Host-owned ECS with ordered mutation, stable component
//! storage and fixed-order evaluation of its selected Systems.

mod asset_lifecycle;
mod capacity;
mod component_state;
mod construction;
mod context;
mod direct_bindings;
mod entities;
mod host_ingress;
mod ingress;
mod metadata;
mod mutation;
pub(crate) mod outputs;
mod queries;
mod removals;
mod serialization;
pub mod systems;
mod update;
mod world_state;

#[cfg(test)]
mod storage_tests;

pub use capacity::{WorldCapacityHints, WorldCapacityHintsPatch, WorldSystemCapacityHints};
pub use construction::WorldConstructionError;
pub(crate) use construction::WorldConstructionIdentity;
pub use context::WorldContext;
pub use entities::{
    EntityId, EntityLink, EntityOrder, EntityPersistentId, EntityPlacement, EntityPlacementRef,
};
pub use ingress::{RECYCLED_COMMAND_BUFFER_COMMANDS, WorldLimits};
pub(crate) use metadata::validate_world_symbolic_id;
pub use metadata::{
    WorldCreateOptions, WorldDescriptor, WorldMetadata, WorldPersistentId, WorldSelector,
};
pub(in crate::world) use mutation::EntityAliases;
pub use queries::EntitySnapshot;
pub use update::WorldUpdateReport;
pub use world_state::WorldSimulationState;
pub(crate) use world_state::{WorldEntityState, WorldMutationState};

/// Single mutation owner of a headless ECS.
///
/// Batches mutate metadata and affected values without snapshots or rollback.
/// Occupied components stay in stable boxed pages, one value per field.
/// Systems own evaluation, dependency validation and lifecycle cleanup.
pub struct World {
    pub(crate) data: WorldSimulationState,
    schedule: systems::SystemSchedule,
}

#[cfg(feature = "instrumentation")]
impl Drop for World {
    fn drop(&mut self) {
        crate::profiling::retire_world(self.data.profile_context);
    }
}
