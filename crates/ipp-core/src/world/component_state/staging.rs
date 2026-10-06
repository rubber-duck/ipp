//! Staging of affected components before command application.
//!
//! Hydration, dirty tracking and commit preparation. Ownership lives in
//! [`world`](crate::world); this module only hosts the staging phase.
//!
//! A batch works on one transient staged copy of each component it touches:
//! the component is hydrated from retained storage once, and later operations
//! of the batch write that copy in place. The copy is not a second store: when
//! the batch commits it moves into the commit's `prepared` values, is installed
//! into stable storage after the invalidation hooks, and is dropped. Until then
//! readers use the staged copy. Writes validate before they take effect, so an
//! invalid operation leaves the staged copy unchanged.

use crate::components::registry;
use crate::world::WorldMutationState;
use crate::world::mutation::EntityAliases;
use crate::{Command, EntityId, ErrorReason};

impl WorldMutationState {
    /// Hydrate the staged copy of a present component once per batch, from a
    /// value prepared by an unfinished commit or else from retained storage.
    pub(in crate::world) fn stage_component(
        &mut self,
        components: &registry::ComponentStorage,
        entity: EntityId,
        component: u16,
    ) {
        let state = &mut self.entities_state;
        let hydrated = if let Some(record) = state.entities.get_mut(&entity)
            && let Some(stored) = record.components.get_mut(&component)
            && stored.staged.is_none()
            && let Some(value) = state
                .prepared
                .get(&(entity, component))
                .cloned()
                .or_else(|| components.get(component, entity.index() as usize))
        {
            stored.staged = Some(Box::new(value));
            true
        } else {
            false
        };
        // Hydration materializes the retained value without changing it.
        if hydrated {
            self.mark_component(entity, component);
        }
    }

    pub(in crate::world) fn stage_command_inputs(
        &mut self,
        components: &registry::ComponentStorage,
        command: &Command,
        aliases: &EntityAliases,
    ) -> Result<(), ErrorReason> {
        match command {
            Command::InsertComponent {
                entity,
                component,
                ..
            }
            | Command::SetField {
                entity,
                component,
                ..
            }
            | Command::SetFieldIf {
                entity,
                component,
                ..
            }
            | Command::SetDynamicProperty {
                entity,
                component,
                ..
            }
            | Command::RemoveDynamicProperty {
                entity,
                component,
                ..
            } => {
                let entity = self.resolve(entity, aliases)?;
                self.stage_component(components, entity, *component);
            }
            Command::InsertComponentValue {
                entity,
                value,
            } => {
                let entity = self.resolve(entity, aliases)?;
                self.stage_component(components, entity, value.type_id());
            }
            _ => {}
        }
        Ok(())
    }
}

impl WorldMutationState {
    /// Record that this operation may change the component's value in any way;
    /// its observation compares the whole value.
    pub(in crate::world) fn touch_component(&mut self, id: EntityId, component: u16) {
        self.entities_state
            .operation_untracked
            .insert((id, component));
        self.mark_component(id, component);
    }

    /// Record an in-place staged write whose complete effect is `write`, and
    /// whether it changed the component's value.
    pub(in crate::world) fn touch_component_write(
        &mut self,
        id: EntityId,
        component: u16,
        write: super::observations::ComponentStagedWrite,
        changed: bool,
    ) {
        self.entities_state
            .operation_writes
            .push(((id, component), write, changed));
        self.mark_component(id, component);
    }

    fn mark_component(&mut self, id: EntityId, component: u16) {
        self.entities_state.dirty.insert((id, component));
        self.entities_state
            .operation_components
            .insert((id, component));
        let incarnation = self
            .entities_state
            .entities
            .get(&id)
            .and_then(|record| record.input(component))
            .map(|input| input.incarnation);
        self.entities_state
            .changed
            .insert_if_absent((id, component), incarnation);
    }

    /// Defer the staged copy of each component this operation affected to commit.
    pub(in crate::world) fn prepare_changes(&mut self) {
        for key in std::mem::take(&mut self.entities_state.dirty) {
            self.entities_state.prepared.remove(&key);
            let staged = self
                .entities_state
                .entities
                .get(&key.0)
                .and_then(|record| record.components.get(&key.1))
                .is_some_and(|state| state.staged.is_some());
            if staged {
                self.entities_state.deferred_preparation.insert(key);
            } else {
                self.entities_state.deferred_preparation.remove(&key);
            }
        }
    }

    /// Move the staged copies deferred by earlier operations into the commit's
    /// prepared values, once per commit.
    pub(in crate::world) fn prepare_deferred_components(&mut self) {
        for key in std::mem::take(&mut self.entities_state.deferred_preparation) {
            let Some(value) = self
                .entities_state
                .entities
                .get_mut(&key.0)
                .and_then(|record| record.components.get_mut(&key.1))
                .and_then(|state| state.staged.take())
            else {
                continue;
            };

            // Test-only oracle: ingress validation must already have made
            // every committed value whole-valid.
            #[cfg(feature = "checked-invariants")]
            if let Err(reason) = value.validate_lifecycle() {
                panic!(
                    "checked invariant: component {} of entity {} fails whole validation ({reason})",
                    key.1,
                    key.0.to_bits()
                );
            }

            self.entities_state.prepared.insert(key, *value);
        }
    }
}
