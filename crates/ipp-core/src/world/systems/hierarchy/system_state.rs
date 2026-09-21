use super::*;

#[cfg(test)]
thread_local! {
    pub(super) static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[inline]
fn record_visit() {
    #[cfg(test)]
    VISITS.set(VISITS.get() + 1);
}

/// Incremental relationships. Only evaluation materializes the flat propagation order.
#[derive(Default)]
pub(crate) struct HierarchyGraph {
    pub(crate) parents: BTreeMap<EntityId, EntityId>,
    children: BTreeMap<EntityId, BTreeSet<EntityId>>,
    entities: BTreeSet<EntityId>,
    pub(crate) order: Vec<EntityId>,
    pub(crate) invalid: BTreeSet<EntityId>,
    pub(crate) affected: BTreeSet<EntityId>,
    order_dirty: bool,
    pub(super) compiled: super::propagation::HierarchyPropagation,
}

impl HierarchyGraph {
    pub(crate) fn build(world: &WorldSimulationState, state: &WorldEntityState) -> Self {
        let mut graph = Self {
            entities: state.entities.keys().copied().collect(),
            order_dirty: true,
            ..Self::default()
        };
        for &entity in state.entities.keys() {
            record_visit();
            graph.read_parent(world, state, entity);
        }
        graph.affected = graph.entities.clone();
        graph.validate_affected();
        graph
    }

    pub(crate) fn reconcile(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
        components: impl IntoIterator<Item = (EntityId, u16)>,
        created: impl IntoIterator<Item = EntityId>,
        deleted: impl IntoIterator<Item = EntityId>,
    ) {
        self.affected.clear();
        let mut roots = BTreeSet::new();
        for entity in created {
            self.entities.insert(entity);
            self.order_dirty = true;
        }
        for entity in deleted {
            self.entities.remove(&entity);
            self.invalid.remove(&entity);
            self.detach(entity);
            if let Some(children) = self.children.remove(&entity) {
                for child in children {
                    self.parents.remove(&child);
                    roots.insert(child);
                }
            }
            self.order_dirty = true;
        }
        for (entity, component) in components {
            if component == ComponentValue::HIERARCHY && self.entities.contains(&entity) {
                self.read_parent(world, state, entity);
                roots.insert(entity);
            }
        }
        // Removing a valid edge cannot introduce a cycle or invalidate its
        // descendants. Keep root-first mass deletion proportional to removals.
        let mut pending = Vec::new();
        for entity in roots {
            let parent = self.parents.get(&entity);
            if self.invalid.contains(&entity) || parent.is_some() {
                pending.push(entity);
            } else {
                self.affected.insert(entity);
            }
        }
        while let Some(entity) = pending.pop() {
            record_visit();
            if self.affected.insert(entity)
                && let Some(children) = self.children.get(&entity)
            {
                pending.extend(children.iter().copied());
            }
        }
        self.validate_affected();
    }

    fn detach(&mut self, entity: EntityId) {
        if let Some(parent) = self.parents.remove(&entity)
            && let Some(children) = self.children.get_mut(&parent)
        {
            children.remove(&entity);
            if children.is_empty() {
                self.children.remove(&parent);
            }
        }
    }

    fn read_parent(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
        entity: EntityId,
    ) {
        let parent = match state.input_value(&world.components, entity, ComponentValue::HIERARCHY) {
            Some(ComponentValue::Hierarchy(value)) if self.entities.contains(&value.parent) => {
                Some(value.parent)
            }
            _ => None,
        };
        if self.parents.get(&entity).copied() == parent {
            return;
        }
        self.detach(entity);
        if let Some(parent) = parent {
            self.parents.insert(entity, parent);
            self.children.entry(parent).or_default().insert(entity);
        }
        self.order_dirty = true;
    }

    /// Classify changed subtrees in one iterative walk, including cycle dependents.
    fn validate_affected(&mut self) {
        let mut done = BTreeSet::new();
        for &entity in &self.affected {
            let mut path = Vec::new();
            let mut visiting = BTreeSet::new();
            let mut current = entity;
            let invalid = loop {
                record_visit();
                if done.contains(&current) || !self.affected.contains(&current) {
                    break self.invalid.contains(&current);
                }
                if !visiting.insert(current) {
                    break true;
                }
                path.push(current);
                let Some(&parent) = self.parents.get(&current) else {
                    break false;
                };
                current = parent;
            };
            for entity in path {
                done.insert(entity);
                if invalid {
                    self.invalid.insert(entity);
                } else {
                    self.invalid.remove(&entity);
                }
            }
        }
    }

    pub(crate) fn prepare_order(&mut self) {
        if !self.order_dirty {
            return;
        }
        self.order.clear();
        let mut done = BTreeSet::new();
        for &entity in &self.entities {
            let mut path = Vec::new();
            let mut current = entity;
            while done.insert(current) {
                record_visit();
                path.push(current);
                if self.invalid.contains(&current) {
                    break;
                }
                let Some(&parent) = self.parents.get(&current) else {
                    break;
                };
                current = parent;
            }
            self.order.extend(path.into_iter().rev());
        }
        self.order_dirty = false;
    }

    pub(super) fn prepare_access(
        &mut self,
        storage: &crate::components::registry::ComponentStorage,
    ) {
        if crate::compiled_hierarchy_enabled() && !self.compiled.valid {
            let mut compiled = std::mem::take(&mut self.compiled);
            compiled.prepare(self, storage);
            self.compiled = compiled;
        }
    }

    pub(crate) fn propagate(&self, world: &mut WorldSimulationState, aimed: bool) {
        if crate::compiled_hierarchy_enabled() && self.compiled.valid {
            self.compiled.propagate(&mut world.components, aimed);
            return;
        }
        for &entity in &self.order {
            let index = entity.index() as usize;
            if world.components.hierarchy(index).is_none()
                || !world.state.entities.contains_key(&entity)
            {
                continue;
            }
            let result = if self.invalid.contains(&entity) {
                Err(ErrorReason::UnsupportedDependency)
            } else {
                local_affine(world, entity, aimed).and_then(|local| {
                    match self.parents.get(&entity) {
                        Some(&parent) => local.then(&parent_affine(world, entity, parent)?),
                        None => Ok(local),
                    }
                })
            };
            world.components.hierarchy_mut(index).unwrap().runtime.world = result.ok();
        }
    }
}
