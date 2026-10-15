use crate::{EntityId, world::WorldSimulationState};
use std::collections::{BTreeMap, BTreeSet};

/// Derived eligibility memo scoped to one immutable presentation read phase.
/// Recreate after mutation: no authored value or generation survives a phase.
#[derive(Default)]
pub(crate) struct PresentationEligibility {
    evaluated: BTreeMap<EntityId, bool>,
}

impl PresentationEligibility {
    pub(crate) fn open(&mut self, world: &WorldSimulationState, entity: EntityId) -> bool {
        self.open_inputs(&world.components, &world.state, entity)
    }

    pub(crate) fn open_inputs(
        &mut self,
        components: &crate::components::registry::ComponentStorage,
        state: &crate::world::WorldEntityState,
        entity: EntityId,
    ) -> bool {
        let mut path = Vec::new();
        let mut visited = BTreeSet::new();
        let mut current = entity;
        let open = loop {
            if let Some(&open) = self.evaluated.get(&current) {
                break open;
            }
            if !state.entities.contains_key(&current)
                || state.links.invalid.contains(&current)
                || !visited.insert(current)
            {
                break false;
            }
            path.push(current);
            if matches!(state.input_value(components, current, crate::ComponentValue::PRESENTATION_GATE), Some(crate::ComponentValue::PresentationGate(gate)) if !gate.open)
            {
                break false;
            }
            match state.links.parent(current) {
                Some(parent) => current = parent,
                None => break true,
            }
        };
        for entity in path {
            self.evaluated.insert(entity, open);
        }
        open
    }
}
