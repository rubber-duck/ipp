//! Indexed terminal dependencies; unrelated mutations do not walk declarations.

use crate::systems::hierarchy::HierarchyGraph;
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, EntityId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct LookAtDependencies {
    pub(super) targets: BTreeMap<EntityId, EntityId>,
    inputs: BTreeMap<EntityId, BTreeSet<EntityId>>,
    readers: BTreeMap<EntityId, BTreeSet<EntityId>>,
    pub(super) invalid: BTreeSet<EntityId>,
}

impl LookAtDependencies {
    pub(super) fn rebuild(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
        graph: &HierarchyGraph,
    ) {
        *self = Self::default();
        self.reconcile(world, state, graph, state.entities.keys().copied());
    }

    pub(super) fn reconcile(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
        graph: &HierarchyGraph,
        changed: impl IntoIterator<Item = EntityId>,
    ) -> bool {
        let changed: BTreeSet<_> = changed.into_iter().collect();
        let mut affected = changed.clone();
        for entity in &changed {
            if let Some(readers) = self.readers.get(entity) {
                affected.extend(readers);
            }
            let target =
                match state.input_value(&world.components, *entity, ComponentValue::LOOK_AT) {
                    Some(ComponentValue::LookAt(value))
                        if value.enabled
                            && value.target.to_bits() != 0
                            && state.entities.contains_key(&value.target) =>
                    {
                        Some(value.target)
                    }
                    _ => None,
                };
            if let Some(target) = target {
                self.targets.insert(*entity, target);
            } else {
                self.targets.remove(entity);
            }
        }
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
            let mut invalid = graph.invalid.contains(&entity);
            for mut current in [Some(target), graph.parents.get(&entity).copied()]
                .into_iter()
                .flatten()
            {
                loop {
                    if !inputs.insert(current) {
                        break;
                    }
                    if self.targets.contains_key(&current) || graph.invalid.contains(&current) {
                        invalid = true;
                    }
                    let Some(&parent) = graph.parents.get(&current) else {
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
}
