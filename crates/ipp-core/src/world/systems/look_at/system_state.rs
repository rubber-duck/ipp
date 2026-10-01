//! Indexed terminal dependencies; unrelated mutations do not walk declarations.

use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, EntityId};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
thread_local! {
    pub(crate) static LOOK_AT_CHECKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Default)]
pub(super) struct LookAtDependencies {
    pub(super) targets: BTreeMap<EntityId, EntityId>,
    inputs: BTreeMap<EntityId, BTreeSet<EntityId>>,
    readers: BTreeMap<EntityId, BTreeSet<EntityId>>,
    pub(super) invalid: BTreeSet<EntityId>,
}

impl LookAtDependencies {
    pub(super) fn rebuild(&mut self, world: &WorldSimulationState, state: &WorldEntityState) {
        *self = Self::default();
        self.reconcile(world, state, state.entities.keys().copied());
    }

    pub(super) fn reconcile(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
        changed: impl IntoIterator<Item = EntityId>,
    ) -> bool {
        let changed: BTreeSet<_> = changed.into_iter().collect();
        let mut affected = changed.clone();
        for entity in &changed {
            if let Some(readers) = self.readers.get(entity) {
                affected.extend(readers);
            }
            let target = Self::target(world, state, *entity);
            if let Some(target) = target {
                self.targets.insert(*entity, target);
            } else {
                self.targets.remove(entity);
            }
        }
        #[cfg(test)]
        LOOK_AT_CHECKS.set(LOOK_AT_CHECKS.get() + affected.len());
        let mut rejected = false;
        for entity in affected {
            if let Some(inputs) = self.inputs.remove(&entity) {
                for input in inputs {
                    let readers = self.readers.get_mut(&input).expect("indexed dependency");
                    readers.remove(&entity);
                    if readers.is_empty() {
                        self.readers.remove(&input);
                    }
                }
            }
            self.invalid.remove(&entity);
            let Some(&target) = self.targets.get(&entity) else {
                continue;
            };
            let mut inputs = BTreeSet::new();
            let mut invalid = state.links.invalid.contains(&entity);
            for mut current in [Some(target), state.links.parent(entity)]
                .into_iter()
                .flatten()
            {
                loop {
                    if !inputs.insert(current) {
                        break;
                    }
                    if self.targets.contains_key(&current) || state.links.invalid.contains(&current)
                    {
                        invalid = true;
                    }
                    let Some(parent) = state.links.parent(current) else {
                        break;
                    };
                    current = parent;
                }
            }
            // Watch the declaration's own parent and liveness, without treating
            // its own output as a terminal input.
            inputs.insert(entity);
            for &input in &inputs {
                self.readers.entry(input).or_default().insert(entity);
            }
            self.inputs.insert(entity, inputs);
            if invalid {
                self.invalid.insert(entity);
                rejected = true;
            }
        }
        rejected
    }

    pub(super) fn would_reject(
        &self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
        changed: impl IntoIterator<Item = EntityId>,
    ) -> bool {
        let changed: BTreeSet<_> = changed.into_iter().collect();
        let mut affected = changed.clone();
        for entity in &changed {
            if let Some(readers) = self.readers.get(entity) {
                affected.extend(readers);
            }
        }
        #[cfg(test)]
        LOOK_AT_CHECKS.set(LOOK_AT_CHECKS.get() + affected.len());
        affected.into_iter().any(|entity| {
            let target = if changed.contains(&entity) {
                Self::target(world, state, entity)
            } else {
                self.targets.get(&entity).copied()
            };
            let Some(target) = target else {
                return false;
            };
            if state.links.invalid.contains(&entity) {
                return true;
            }
            let mut visited = BTreeSet::new();
            for mut current in [Some(target), state.links.parent(entity)]
                .into_iter()
                .flatten()
            {
                loop {
                    if !visited.insert(current) {
                        break;
                    }
                    let aimed = if changed.contains(&current) {
                        Self::target(world, state, current).is_some()
                    } else {
                        self.targets.contains_key(&current)
                    };
                    if aimed || state.links.invalid.contains(&current) {
                        return true;
                    }
                    let Some(parent) = state.links.parent(current) else {
                        break;
                    };
                    current = parent;
                }
            }
            false
        })
    }

    fn target(
        world: &WorldSimulationState,
        state: &WorldEntityState,
        entity: EntityId,
    ) -> Option<EntityId> {
        match state.input_value(&world.components, entity, ComponentValue::LOOK_AT) {
            Some(ComponentValue::LookAt(value))
                if value.enabled
                    && value.target.to_bits() != 0
                    && state.entities.contains_key(&value.target) =>
            {
                Some(value.target)
            }
            _ => None,
        }
    }
}
