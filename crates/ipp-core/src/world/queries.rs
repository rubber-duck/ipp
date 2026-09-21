use super::*;

pub(in crate::world) fn inspect_entity(
    world: &WorldSimulationState,
    state: &WorldEntityState,
    id: EntityId,
) -> Option<EntitySnapshot> {
    if !state.allocator.contains(id) {
        return None;
    }
    let record = state.entities.get(&id)?;
    let mut base = Vec::new();
    let mut effective = Vec::new();
    for &component in record.layers.keys() {
        if let Some(value) = state.producer_value(&world.components, id, component) {
            base.push(value);
        }
    }
    world
        .components
        .inspect(id.index() as usize, &mut effective);
    base.sort_by_key(ComponentValue::type_id);
    effective.sort_by_key(ComponentValue::type_id);
    Some(EntitySnapshot {
        id,
        metadata: record.metadata.clone(),
        base,
        effective,
    })
}

impl<'a> crate::world::WorldReadContext<'a> {
    /// Recover the pre-evaluation input while keeping effective overlay fields.
    pub(in crate::world) fn underlying_input_component(
        &self,
        entity: EntityId,
        component: u16,
    ) -> Option<ComponentValue> {
        let layer = self.state.entities.get(&entity)?.layers.get(&component)?;
        let _input = layer.input()?;
        let value = self
            .state
            .input_value(&self.world.components, entity, component)?;
        let mut value = value;
        for instance in self.after.iter().rev().chain(self.before.iter().rev()) {
            instance
                .system
                .restore_component_input(entity, _input.incarnation, &mut value);
        }
        Some(value)
    }

    /// Recover producer input through only the systems that control this component.
    pub(in crate::world) fn producer_component(
        &self,
        entity: EntityId,
        component: u16,
    ) -> Option<ComponentValue> {
        let layer = self.state.entities.get(&entity)?.layers.get(&component)?;
        layer.inputs.base()?;
        if let Some(value) = layer.inputs.base_value() {
            return Some(value.clone());
        }
        let mut value = self.underlying_input_component(entity, component)?;
        self.state.restore_producer_value(entity, &mut value);
        Some(value)
    }

    /// Read a live entity's actual retained and evaluated components.
    pub fn inspect(&self, id: EntityId) -> Option<EntitySnapshot> {
        let mut snapshot = inspect_entity(self.world, self.state, id)?;
        snapshot.base = self.state.entities[&id]
            .layers
            .keys()
            .filter_map(|&component| self.producer_component(id, component))
            .collect();
        Some(snapshot)
    }

    /// Snapshot every live entity in ascending handle-bit order.
    pub fn entities(&self) -> Vec<EntitySnapshot> {
        self.state
            .entities
            .keys()
            .filter_map(|&id| self.inspect(id))
            .collect()
    }

    /// Read at most `limit` entities after an exclusive identity cursor.
    pub fn entity_page(&self, after: u64, target: u64, limit: usize) -> Vec<EntitySnapshot> {
        if target != 0 {
            return self
                .inspect(EntityId::from_bits(target))
                .into_iter()
                .collect();
        }
        self.state
            .entities
            .range((
                std::ops::Bound::Excluded(EntityId::from_bits(after)),
                std::ops::Bound::Unbounded,
            ))
            .take(limit)
            .filter_map(|(&id, _)| self.inspect(id))
            .collect()
    }

    /// Resolve a world-unique symbolic ID.
    pub fn lookup_id(&self, symbol: &str) -> Option<EntityId> {
        self.state.symbols.get(symbol).copied()
    }

    /// Resolve a class in ascending handle-bit order.
    pub fn lookup_class(&self, class: &str) -> Vec<EntityId> {
        self.state
            .classes
            .get(class)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Completed frame number, independent of a host session ID.
    pub fn tick(&self) -> u64 {
        self.world.tick
    }

    /// Accumulated simulation seconds.
    pub fn time(&self) -> f64 {
        self.world.time
    }
}

impl WorldContext<'_> {
    /// Unrecoverable invariant failure; state remains inspectable until destruction.
    pub fn fault(&self) -> Option<ErrorReason> {
        self.world.fault
    }
}

impl crate::WorldContext<'_> {
    /// Read a live entity's actual retained and evaluated components.
    pub fn inspect(&self, id: EntityId) -> Option<EntitySnapshot> {
        self.read().inspect(id)
    }

    /// Read a bounded entity page after an exclusive cursor, or one exact identity.
    pub fn entity_page(&self, after: u64, target: u64, limit: usize) -> Vec<EntitySnapshot> {
        self.read().entity_page(after, target, limit)
    }

    /// Snapshot every live entity in ascending handle-bit order.
    pub fn entities(&self) -> Vec<EntitySnapshot> {
        self.read().entities()
    }

    /// Resolve a world-unique symbolic ID.
    pub fn lookup_id(&self, symbol: &str) -> Option<EntityId> {
        self.read().lookup_id(symbol)
    }

    /// Resolve a class in ascending handle-bit order.
    pub fn lookup_class(&self, class: &str) -> Vec<EntityId> {
        self.read().lookup_class(class)
    }

    /// Completed frame number, independent of a host session ID.
    pub fn tick(&self) -> u64 {
        self.read().tick()
    }

    /// Accumulated simulation seconds.
    pub fn time(&self) -> f64 {
        self.read().time()
    }
}
