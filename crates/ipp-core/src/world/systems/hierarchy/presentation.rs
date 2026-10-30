use crate::{EntityId, world::WorldSimulationState};

// A bounded, direct-mapped phase memo avoids allocating a tree node and a path
// collection for every entity. Collisions only evict derived answers: they never
// affect eligibility. Sparse consumers can bypass the memo entirely.
const MEMO_SLOTS: usize = 256;

/// Derived eligibility memo scoped to one immutable presentation read phase.
/// Recreate after mutation: no authored value or generation survives a phase.
pub(crate) struct PresentationEligibility {
    evaluated: [Option<(EntityId, bool)>; MEMO_SLOTS],
}

impl Default for PresentationEligibility {
    fn default() -> Self {
        Self {
            evaluated: [None; MEMO_SLOTS],
        }
    }
}

impl PresentationEligibility {
    /// Independent bounded walk for the memo's test oracle.
    #[cfg(test)]
    pub(crate) fn open_unmemoized(world: &WorldSimulationState, entity: EntityId) -> bool {
        let mut current = entity;
        for _ in 0..world.state.entities.len() {
            if !Self::admits(&world.components, &world.state, current) {
                return false;
            }
            match world.state.links.parent(current) {
                Some(parent) => current = parent,
                None => return true,
            }
        }
        false
    }

    fn admits(
        components: &crate::components::registry::ComponentStorage,
        state: &crate::world::WorldEntityState,
        entity: EntityId,
    ) -> bool {
        state.entities.contains_key(&entity)
            && !state.links.invalid.contains(&entity)
            // Ordered batch actions must see an earlier staged gate edit. Read
            // only the boolean field, avoiding a full component clone.
            && !matches!(
                state.input_field(
                    components,
                    entity,
                    crate::ComponentValue::PRESENTATION_GATE,
                    std::mem::offset_of!(crate::components::PresentationGate, open) as u32,
                ),
                Some(crate::components::schema::FieldValue::Bool(false))
            )
    }

    pub(crate) fn open(&mut self, world: &WorldSimulationState, entity: EntityId) -> bool {
        self.open_inputs(&world.components, &world.state, entity)
    }

    pub(crate) fn open_inputs(
        &mut self,
        components: &crate::components::registry::ComponentStorage,
        state: &crate::world::WorldEntityState,
        entity: EntityId,
    ) -> bool {
        let mut current = entity;
        let mut steps = 0;
        let open = loop {
            if let Some((cached, open)) = self.evaluated[current.index() as usize % MEMO_SLOTS]
                && cached == current
            {
                break open;
            }
            if steps == state.entities.len() || !Self::admits(components, state, current) {
                break false;
            }
            steps += 1;
            match state.links.parent(current) {
                Some(parent) => current = parent,
                None => break true,
            }
        };
        // Keep the nearest unresolved prefix. Walking beyond the cache size
        // would evict these answers with distant ancestors on deep branches.
        // The first pass already bounded cycles and missing links.
        current = entity;
        for _ in 0..steps.min(MEMO_SLOTS) {
            self.evaluated[current.index() as usize % MEMO_SLOTS] = Some((current, open));
            match state.links.parent(current) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        open
    }
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
