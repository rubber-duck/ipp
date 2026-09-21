//! Durable World state with external resource references and no asset acquisition.

pub(crate) mod binary;

pub(crate) mod assets;

mod container;

use crate::{
    ComponentValue, EntityMetadata, EntityPersistentId, WorldCapacityHints, WorldMetadata,
};

/// Host resource policy for a complete owned save/load candidate, independent of hints.
#[derive(Clone, Copy, Debug)]
pub struct WorldPersistenceLimits {
    /// Complete encoded container and captured authored data budget.
    pub max_bytes: usize,
}

impl Default for WorldPersistenceLimits {
    fn default() -> Self {
        Self {
            max_bytes: 64 << 20,
        }
    }
}

/// One persistent producer entity. Components contain authored values only.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSerializedEntity {
    /// Stable reference within the persisted World.
    pub persistent_id: EntityPersistentId,
    /// Editable symbolic ID and normalized classes.
    pub metadata: EntityMetadata,
    /// Registered base components, with durable entity references in field values.
    pub components: Vec<ComponentValue>,
}

/// Owned consistent capture; it retains no World, system, service or GPU borrows.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSnapshot {
    /// World symbolic and persistent identity.
    pub metadata: WorldMetadata,
    /// Configured reservations, independent of allocator spare capacity.
    pub capacity_hints: WorldCapacityHints,
    /// Highest allocated durable entity identity, including subsequently deleted entities.
    pub next_entity_id: u64,
    /// Producer entities in stable storage order; load preserves relative evaluation order.
    pub entities: Vec<WorldSerializedEntity>,
    /// Opaque persistent state produced and consumed by its owning selected System.
    pub systems: std::collections::BTreeMap<String, crate::systems::SystemPersistentState>,
}

/// Optional request overrides. Omitted hints retain the complete saved configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldLoadOptions {
    /// Use a different Host-visible name when loading a copy of a published World.
    pub symbolic_id: Option<String>,
    /// Explicit reservation overrides, merged with saved system reservations.
    pub capacity_hints: crate::WorldCapacityHintsPatch,
}
