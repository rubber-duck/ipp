use super::*;

#[derive(Default)]
pub(crate) struct HierarchyGraph {
    pub(crate) order: Vec<EntityId>,
    pub(crate) invalid: BTreeSet<EntityId>,
    pub(crate) affected: BTreeSet<EntityId>,
    order_dirty: bool,
    pub(super) compiled: super::propagation::HierarchyPropagation,
}

impl HierarchyGraph {
    pub(crate) fn build(_world: &WorldSimulationState, state: &WorldEntityState) -> Self {
        Self {
            invalid: state.links.invalid.clone(),
            affected: state.entities.keys().copied().collect(),
            order_dirty: true,
            ..Self::default()
        }
    }

    pub(crate) fn reconcile(&mut self, state: &WorldEntityState) {
        self.affected.clone_from(&state.links.affected);
        if !state.links.operation_changed.is_empty() {
            self.order_dirty = true;
            self.invalid.clone_from(&state.links.invalid);
            self.compiled.invalidate();
        }
    }

    pub(crate) fn prepare_order(&mut self, state: &WorldEntityState) {
        if !self.order_dirty {
            return;
        }
        self.order.clear();
        let mut pending: Vec<_> = state.links.children(None).collect();
        pending.reverse();
        let mut visited = BTreeSet::new();
        while let Some(entity) = pending.pop() {
            if visited.insert(entity) {
                self.order.push(entity);
                let children: Vec<_> = state.links.children(Some(entity)).collect();
                pending.extend(children.into_iter().rev());
            }
        }
        self.order.extend(
            state
                .entities
                .keys()
                .filter(|entity| !visited.contains(entity))
                .copied(),
        );
        self.order_dirty = false;
    }

    pub(super) fn prepare_access(&mut self, world: &WorldSimulationState) {
        if !self.compiled.valid {
            let mut compiled = std::mem::take(&mut self.compiled);
            compiled.prepare(self, world);
            self.compiled = compiled;
        }
    }

    pub(crate) fn propagate(&self, world: &mut WorldSimulationState, aimed: bool) {
        self.compiled.propagate(world, aimed);
    }
}
