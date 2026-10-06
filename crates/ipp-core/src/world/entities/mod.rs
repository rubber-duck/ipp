//! Entity identities, entity-level state operations and ordered entity links.

mod entity_state;
mod identity;
mod link_access;
pub(in crate::world) mod links;

pub(crate) use identity::Allocator;
pub use identity::{EntityId, EntityPersistentId};
pub use links::{EntityLink, EntityOrder, EntityPlacement, EntityPlacementRef};
