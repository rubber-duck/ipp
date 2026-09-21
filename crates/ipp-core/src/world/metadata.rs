//! World discovery metadata and creation policy.

use crate::{WorldCapacityHints, WorldId};

/// Durable World identity, assigned by the Host and retained through file round trips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorldPersistentId(pub u128);

/// Durable entity identity within one persisted World, independent of slot reuse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityPersistentId(pub u64);

/// World-owned metadata. The symbolic ID is unique among published Worlds on a Host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldMetadata {
    /// Editable lookup name; renaming preserves existing sessions.
    pub symbolic_id: String,
    /// Stable saved identity; loading a copy retains this identity with fresh runtime handles.
    pub persistent_id: WorldPersistentId,
}

/// A published World descriptor; this never retains the World or its systems.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldDescriptor {
    /// Identity for runtime routing.
    pub id: WorldId,
    /// Persistent and symbolic metadata.
    pub metadata: WorldMetadata,
    /// Resolved World and system reservations.
    pub capacity_hints: WorldCapacityHints,
}

/// Empty-World construction configuration. Reservations do not cap live state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldCreateOptions {
    /// Empty requests an automatically assigned Host-unique name.
    pub symbolic_id: String,
    /// Reservations to merge with selected system defaults.
    pub capacity_hints: WorldCapacityHints,
}

/// World selection at the Host attachment boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldSelector {
    /// Live runtime identity.
    Id(WorldId),
    /// Host-unique editable name.
    SymbolicId(String),
}

pub(crate) fn validate_world_symbolic_id(symbol: &str) -> Result<(), String> {
    if symbol.is_empty()
        || symbol.len() > 2048
        || symbol.trim() != symbol
        || symbol.chars().any(char::is_control)
    {
        return Err("World symbolic ID must be nonempty, at most 2048 UTF-8 bytes, and contain no surrounding whitespace or control characters".into());
    }
    Ok(())
}

impl crate::WorldContext<'_> {
    /// The owning World's durable metadata.
    pub fn metadata(&self) -> &WorldMetadata {
        &self.world.metadata
    }

    /// Durable identity for a live entity.
    pub fn entity_persistent_id(&self, entity: crate::EntityId) -> Option<EntityPersistentId> {
        self.world
            .state
            .entities
            .get(&entity)
            .map(|record| record.persistent_id)
    }
}
