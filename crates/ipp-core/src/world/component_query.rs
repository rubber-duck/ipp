//! Retained typed iteration, maintained by the component lifecycle barrier.

use super::{WorldSimulationState, component_binding::ComponentBinding};
use crate::{EntityId, components::registry::ComponentStorage, systems::SystemCommitContext};
use std::ptr::NonNull;

type Resolver<T> = fn(&ComponentStorage, usize) -> Option<NonNull<T>>;

pub(in crate::world) struct ComponentQuery<T> {
    entries: Vec<(EntityId, ComponentBinding<T>)>,
    initialized: bool,
}

impl<T> Default for ComponentQuery<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            initialized: false,
        }
    }
}

impl<T> ComponentQuery<T> {
    /// Call before storage destruction from the owning System's lifecycle hook.
    pub(in crate::world) fn before_commit(
        &mut self,
        context: &SystemCommitContext<'_>,
        component: u16,
    ) {
        for (entity, changed) in context.changed_components() {
            if changed == component
                && !context.retains_component(entity, component)
                && let Ok(index) = self.entries.binary_search_by_key(&entity, |entry| entry.0)
            {
                self.entries.remove(index);
            }
        }
    }

    /// Resolve changed membership after installation, outside numeric evaluation.
    pub(in crate::world) fn after_commit(
        &mut self,
        context: &SystemCommitContext<'_>,
        component: u16,
        resolve: Resolver<T>,
    ) {
        if !self.initialized {
            return;
        }
        for (entity, changed) in context.changed_components() {
            if changed == component {
                self.install(&context.world_data.components, entity, resolve);
            }
        }
    }

    /// The first preparation also covers construction/restoration before any
    /// ordinary mutation. Thereafter only lifecycle hooks change this list.
    pub(in crate::world) fn prepare(&mut self, world: &WorldSimulationState, resolve: Resolver<T>) {
        if self.initialized {
            return;
        }
        for &entity in world.state.entities.keys() {
            self.install(&world.components, entity, resolve);
        }
        self.initialized = true;
    }

    fn install(&mut self, storage: &ComponentStorage, entity: EntityId, resolve: Resolver<T>) {
        let Some(pointer) = resolve(storage, entity.index() as usize) else {
            return;
        };
        // SAFETY: Resolvers are generated stable-cell accessors for this T. The
        // owning System invokes before_commit before incarnation destruction, and
        // accesses entries only while borrowing the same World's storage.
        let binding = unsafe { ComponentBinding::new(pointer) };
        match self.entries.binary_search_by_key(&entity, |entry| entry.0) {
            Ok(index) => self.entries[index].1 = binding,
            Err(index) => self.entries.insert(index, (entity, binding)),
        }
    }

    #[inline]
    pub(in crate::world) fn entries(&self) -> &[(EntityId, ComponentBinding<T>)] {
        &self.entries
    }
}
