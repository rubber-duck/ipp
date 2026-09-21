//! Update-originated removals. Stable identities survive the callback, borrows do not.
//!
//! Requests are deduplicated and bounded by max_operations. They drain once after
//! every scheduled update, in request order. Stale generations/incarnations are
//! skipped. Lifecycle and finish callbacks cannot enqueue cascades; callers receive
//! InvalidValue outside evaluation, so cleanup cannot create an unbounded drain.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum DeferredRemoval {
    Entity(EntityId),
    Component {
        entity: EntityId,
        component: u16,
        incarnation: u64,
    },
}

impl WorldSimulationState {
    fn request_removal(&mut self, request: DeferredRemoval) -> Result<(), ErrorReason> {
        if !self.accepting_removals {
            return Err(ErrorReason::InvalidValue);
        }
        if self.deferred_removal_members.contains(&request) {
            return Ok(());
        }
        if self.deferred_removals.len() >= self.limits.max_operations {
            return Err(ErrorReason::Capacity);
        }
        self.deferred_removal_members.insert(request);
        self.deferred_removals.push(request);
        Ok(())
    }

    pub(in crate::world) fn defer_remove_entity(
        &mut self,
        entity: EntityId,
    ) -> Result<(), ErrorReason> {
        if !self.accepting_removals {
            return Err(ErrorReason::InvalidValue);
        }
        if !self.state.entities.contains_key(&entity) {
            return Err(ErrorReason::InvalidEntity);
        }
        self.request_removal(DeferredRemoval::Entity(entity))
    }

    pub(in crate::world) fn defer_remove_component(
        &mut self,
        entity: EntityId,
        component: u16,
    ) -> Result<(), ErrorReason> {
        if !self.accepting_removals {
            return Err(ErrorReason::InvalidValue);
        }
        let record = self
            .state
            .entities
            .get(&entity)
            .ok_or(ErrorReason::InvalidEntity)?;
        let incarnation = record
            .input(component)
            .ok_or(ErrorReason::MissingComponent)?
            .incarnation;
        self.request_removal(DeferredRemoval::Component {
            entity,
            component,
            incarnation,
        })
    }
}

impl systems::SystemEcsAccess<'_> {
    /// Queue removal of this World's original entity after every System update returns.
    /// Duplicate requests are idempotent; the target stays readable during evaluation.
    pub fn defer_remove_entity(&mut self, entity: EntityId) -> Result<(), ErrorReason> {
        self.world.defer_remove_entity(entity)
    }

    /// Queue removal of the current component incarnation; replacements are preserved.
    pub fn defer_remove_component(
        &mut self,
        entity: EntityId,
        component: u16,
    ) -> Result<(), ErrorReason> {
        self.world.defer_remove_component(entity, component)
    }
}

impl systems::SystemRuntimeAccess<'_> {
    /// Queue an update-originated removal without recursively dispatching lifecycle hooks.
    pub fn defer_remove_entity(&mut self, entity: EntityId) -> Result<(), ErrorReason> {
        self.world.defer_remove_entity(entity)
    }

    /// Queue the current incarnation for removal after the scheduled update phase.
    pub fn defer_remove_component(
        &mut self,
        entity: EntityId,
        component: u16,
    ) -> Result<(), ErrorReason> {
        self.world.defer_remove_component(entity, component)
    }
}

impl WorldContext<'_> {
    pub(super) fn drain_deferred_removals(&mut self) {
        let mut requests = std::mem::take(&mut self.world.deferred_removals);
        self.world.deferred_removal_members.clear();
        let mut aliases = BTreeMap::new();
        let mut created = Vec::new();
        for request in requests.drain(..) {
            let command = match request {
                DeferredRemoval::Entity(entity)
                    if self.world.state.entities.contains_key(&entity) =>
                {
                    Command::Delete {
                        entity: EntityRef::Handle(entity),
                    }
                }
                DeferredRemoval::Component {
                    entity,
                    component,
                    incarnation,
                } if self
                    .world
                    .state
                    .entities
                    .get(&entity)
                    .and_then(|record| record.input(component))
                    .is_some_and(|input| input.incarnation == incarnation) =>
                {
                    Command::RemoveComponent {
                        entity: EntityRef::Handle(entity),
                        component,
                    }
                }
                _ => continue,
            };
            let result =
                self.runtime_access()
                    .apply_operation(None, &command, &mut aliases, &mut created);
            let result = result.and(self.commit_pending_changes());
            if let Err(_error) = result {
                crate::diagnostic!(
                    Debug,
                    "world={} deferred-removal.completed error={}",
                    self.world.id.0,
                    _error
                );
            }
        }
        self.world.deferred_removals = requests;
    }
}

#[cfg(test)]
#[path = "removals_tests.rs"]
mod tests;
