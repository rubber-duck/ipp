//! Read-only entity observations at a World boundary.

use crate::world::context::WorldReadContext;
use crate::world::{WorldContext, WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, EntityId, EntityLink, EntityMetadata, ErrorReason};

/// Owned, read-only observation of one live entity.
#[derive(Clone, Debug, PartialEq)]
pub struct EntitySnapshot {
    /// World-local identity.
    pub id: EntityId,
    /// Current normalized metadata.
    pub metadata: EntityMetadata,
    /// Ordered relationship: parent and sibling order.
    pub link: EntityLink,
    /// Stored components in registry order.
    pub components: Vec<ComponentValue>,
}

/// Read a live entity's metadata, link and stored components.
pub(in crate::world) fn inspect_entity(
    world: &WorldSimulationState,
    state: &WorldEntityState,
    id: EntityId,
) -> Option<EntitySnapshot> {
    if !state.allocator.contains(id) {
        return None;
    }
    let record = state.entities.get(&id)?;
    let mut components = Vec::new();
    world
        .components
        .inspect(id.index() as usize, &mut components);
    components.sort_by_key(ComponentValue::type_id);
    Some(EntitySnapshot {
        id,
        metadata: record.metadata.clone(),
        link: state.links.effective(id)?.clone(),
        components,
    })
}

impl<'a> WorldReadContext<'a> {
    /// Read a live entity's metadata, link and stored components.
    pub fn inspect(&self, id: EntityId) -> Option<EntitySnapshot> {
        inspect_entity(self.world, self.state, id)
    }

    /// Snapshot every live entity in ascending handle-bit order.
    pub fn entities(&self) -> Vec<EntitySnapshot> {
        self.state
            .entities
            .keys()
            .filter_map(|&id| self.inspect(id))
            .collect()
    }

    /// Read at most `limit` entities after an exclusive identity cursor.
    pub fn entity_page(&self, after: u64, target: u64, limit: usize) -> Vec<EntitySnapshot> {
        if target != 0 {
            return self
                .inspect(EntityId::from_bits(target))
                .into_iter()
                .collect();
        }
        self.state
            .entities
            .range((
                std::ops::Bound::Excluded(EntityId::from_bits(after)),
                std::ops::Bound::Unbounded,
            ))
            .take(limit)
            .filter_map(|(&id, _)| self.inspect(id))
            .collect()
    }

    /// Current lifetime of one stored component, the identity GUI action
    /// targets and lifecycle baselines name; absent without the component.
    pub fn component_incarnation(&self, entity: EntityId, component: u16) -> Option<u64> {
        self.state
            .entities
            .get(&entity)?
            .input(component)
            .map(|input| input.incarnation)
    }

    /// Resolve a world-unique symbolic ID.
    pub fn lookup_id(&self, symbol: &str) -> Option<EntityId> {
        self.state.symbols.get(symbol).copied()
    }

    /// Resolve a class in ascending handle-bit order.
    pub fn lookup_class(&self, class: &str) -> Vec<EntityId> {
        self.state
            .classes
            .get(class)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Completed frame number, independent of a host session ID.
    pub fn tick(&self) -> u64 {
        self.world.tick
    }

    /// Accumulated simulation seconds.
    pub fn time(&self) -> f64 {
        self.world.time
    }
}

impl WorldContext<'_> {
    /// Unrecoverable invariant failure; state remains inspectable until destruction.
    pub fn fault(&self) -> Option<ErrorReason> {
        self.world.fault
    }
}

impl WorldContext<'_> {
    /// Read a live entity's metadata, link and stored components.
    pub fn inspect(&self, id: EntityId) -> Option<EntitySnapshot> {
        self.read().inspect(id)
    }

    /// Read a bounded entity page after an exclusive cursor, or one exact identity.
    pub fn entity_page(&self, after: u64, target: u64, limit: usize) -> Vec<EntitySnapshot> {
        self.read().entity_page(after, target, limit)
    }

    /// Snapshot every live entity in ascending handle-bit order.
    pub fn entities(&self) -> Vec<EntitySnapshot> {
        self.read().entities()
    }

    /// Current lifetime of one stored component; absent without the component.
    pub fn component_incarnation(&self, entity: EntityId, component: u16) -> Option<u64> {
        self.read().component_incarnation(entity, component)
    }

    /// Resolve a world-unique symbolic ID.
    pub fn lookup_id(&self, symbol: &str) -> Option<EntityId> {
        self.read().lookup_id(symbol)
    }

    /// Resolve a class in ascending handle-bit order.
    pub fn lookup_class(&self, class: &str) -> Vec<EntityId> {
        self.read().lookup_class(class)
    }

    /// Completed frame number, independent of a host session ID.
    pub fn tick(&self) -> u64 {
        self.read().tick()
    }

    /// Accumulated simulation seconds.
    pub fn time(&self) -> f64 {
        self.read().time()
    }
}
