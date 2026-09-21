//! Owned system contributions and scoped capture/restore access.

use super::SystemRuntimeAccess;
use std::collections::BTreeMap;

/// An owned subsystem payload; its codec and runtime reconstruction belong to the System.
pub type SystemPersistentState = Vec<u8>;

/// Shared identity mapping and bounded capture accounting.
pub struct SystemSaveContext<'a> {
    /// Live producer identities selected for this capture.
    pub ids: &'a BTreeMap<crate::EntityId, crate::EntityPersistentId>,
    /// Captured producer components, with durable references.
    pub entities: &'a [crate::services::world_serialization::WorldSerializedEntity],
    /// Bytes already retained by this capture, updated before allocating a contribution.
    pub bytes: &'a mut usize,
    /// Maximum total capture size.
    pub max_bytes: usize,
}

/// Restore runs after generic entity identity and component reconstruction.
pub struct SystemLoadContext<'a, 'ids> {
    /// Scoped component, dependency and service access.
    pub world: SystemRuntimeAccess<'a>,
    /// Maximum payload bytes accepted at this restore boundary.
    pub max_bytes: usize,
    /// Durable identities mapped to the unpublished World's live handles.
    pub ids: &'ids BTreeMap<crate::EntityPersistentId, crate::EntityId>,
}
