//! Durable World state with external resource references and no asset acquisition.

pub(crate) mod binary;

mod container;
mod graph;
mod validation;

pub use graph::{
    WorldGraphDescriptor, WorldGraphLoadResult, WorldGraphNode, WorldGraphNodeDescriptor,
    WorldGraphNodeId, WorldGraphSnapshot, WorldPersistenceError, WorldSerializedOutput,
    WorldSerializedReference, WorldSerializedReferenceValue, inspect_world_graph,
};

use crate::{
    ComponentValue, EntityMetadata, EntityPersistentId, WorldCapacityHints, WorldMetadata,
};

pub(crate) struct WorldCaptureReferences {
    pub worlds: std::collections::BTreeMap<crate::WorldRef, WorldGraphNodeId>,
    pub outputs: std::collections::BTreeMap<crate::OutputRef, WorldSerializedOutput>,
}

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

/// One persistent entity with its stored components and link.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSerializedEntity {
    /// Stable reference within the persisted World.
    pub persistent_id: EntityPersistentId,
    /// Stored relationship with typed durable references.
    pub link: WorldSerializedEntityLink,
    /// Editable symbolic ID and normalized classes.
    pub metadata: EntityMetadata,
    /// Stored components, with durable entity references in field values.
    pub components: Vec<ComponentValue>,
}

/// An entity relationship scoped to one serialized World instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldSerializedEntityLink {
    /// Durable parent within the containing World instance.
    pub parent: Option<EntityPersistentId>,
    /// Persisted order label; durable entity identity breaks ties.
    pub order: crate::EntityOrder,
}

/// Local authored state within a graph node, also supplied to World-local durable hooks.
/// Non-null World/output fields reside in the containing node's typed reference table.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSnapshot {
    /// World symbolic and persistent identity.
    pub metadata: WorldMetadata,
    /// Configured reservations, independent of allocator spare capacity.
    pub capacity_hints: WorldCapacityHints,
    /// Immutable selected factory identities in resolved construction order.
    pub selected_systems: Vec<String>,
    /// Highest allocated durable entity identity, including subsequently deleted entities.
    pub next_entity_id: u64,
    /// Entities in stable storage order; load preserves relative evaluation order.
    pub entities: Vec<WorldSerializedEntity>,
    /// Opaque persistent state produced and consumed by its owning selected System.
    pub systems: std::collections::BTreeMap<String, crate::systems::SystemPersistentState>,
}

/// Optional request overrides. Omitted hints retain the complete saved configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldLoadOptions {
    /// Use a different Host-visible name when loading a copy of a published World.
    pub symbolic_id: Option<String>,
    /// Explicit names for nodes identified by inspection of this exact graph container.
    pub world_names: std::collections::BTreeMap<WorldGraphNodeId, String>,
    /// Explicit reservation overrides, merged with saved system reservations.
    pub capacity_hints: crate::WorldCapacityHintsPatch,
}
