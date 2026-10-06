//! Ordered entity links read and placed through System-facing World views.

use super::{EntityLink, EntityPlacement};
use crate::world::mutation::commit_components;
use crate::world::systems::{System, SystemEcsAccess, SystemRuntimeAccess, SystemWorldView};
use crate::{EntityId, ErrorReason};

impl SystemWorldView<'_> {
    /// Borrow a relationship during a scoped lifecycle callback.
    pub fn entity_link(&self, entity: EntityId) -> Option<&EntityLink> {
        self.authored.links.effective(entity)
    }

    /// Traverse current siblings without reconstructing a second tree.
    pub fn entity_children(&self, parent: Option<EntityId>) -> impl Iterator<Item = EntityId> + '_ {
        self.authored.links.children(parent)
    }
}

impl SystemEcsAccess<'_> {
    /// Borrow the ordered link during local evaluation.
    pub fn entity_link(&self, entity: EntityId) -> Option<&EntityLink> {
        self.world.state.links.effective(entity)
    }

    /// Iterate siblings during local evaluation.
    pub fn entity_children(&self, parent: Option<EntityId>) -> impl Iterator<Item = EntityId> + '_ {
        self.world.state.links.children(parent)
    }
}

impl SystemRuntimeAccess<'_> {
    /// Borrow a relationship during this exclusive World callback.
    pub fn entity_link(&self, entity: EntityId) -> Option<&EntityLink> {
        self.world.state.links.effective(entity)
    }

    /// Place an entity with an ordinary link write, then synchronously invalidate
    /// readers. Call only on key, reference or lifecycle changes, not once per frame.
    pub(in crate::world) fn place_entity_link(
        &mut self,
        current: &mut dyn System,
        entity: EntityId,
        placement: EntityPlacement,
    ) -> Result<(), ErrorReason> {
        self.world.state.links.operation_changed.clear();
        let value = self
            .world
            .state
            .links
            .resolve(entity, placement.parent, placement.before)?;
        self.world.state.links.set(entity, value)?;
        let validation = self.world.state.links.reconcile();
        let committed = commit_components(
            self.world,
            &mut self.instances,
            Some(current),
            self.asset_acquisition,
            self.data,
            true,
        );
        validation.and(committed)
    }
}
