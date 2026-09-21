//! Entity identity, metadata and index operations.
//!
//! Creation, deletion, symbolic/class index maintenance and reference
//! resolution. Component value staging and mutation live in
//! [`component_state`](super::component_state). Ownership and orchestration
//! stay in [`super`]; this module only hosts entity-level behavior.

use super::*;

impl WorldMutationState {
    pub(in crate::world) fn incarnation(&mut self) -> Result<u64, ErrorReason> {
        self.entities_state.next_incarnation = self
            .entities_state
            .next_incarnation
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        Ok(self.entities_state.next_incarnation)
    }

    pub(crate) fn resolve(
        &self,
        reference: EntityRef,
        aliases: &BTreeMap<u32, EntityId>,
    ) -> Result<EntityId, ErrorReason> {
        let id = match reference {
            EntityRef::Handle(id) => id,
            EntityRef::Alias(alias) => *aliases.get(&alias).ok_or(ErrorReason::UnknownAlias)?,
        };
        if self.entities_state.allocator.contains(id) {
            Ok(id)
        } else {
            Err(ErrorReason::InvalidEntity)
        }
    }

    pub(in crate::world) fn field(
        &self,
        component: u16,
        field: FieldWrite,
        aliases: &BTreeMap<u32, EntityId>,
    ) -> Result<FieldWrite, ErrorReason> {
        let value = match field.value {
            FieldValue::Entity(EntityRef::Handle(id))
                if id.to_bits() == 0
                    && ComponentValue::accepts_null_entity(component, field.offset) =>
            {
                field.value.clone()
            }
            FieldValue::Entity(reference) => {
                FieldValue::Entity(EntityRef::Handle(self.resolve(reference, aliases)?))
            }
            FieldValue::F32(value) if !value.is_finite() => return Err(ErrorReason::InvalidValue),
            value => value,
        };
        Ok(FieldWrite {
            offset: field.offset,
            value,
        })
    }

    pub(in crate::world) fn set_metadata(
        &mut self,
        id: EntityId,
        mut metadata: EntityMetadata,
    ) -> Result<(), ErrorReason> {
        if let Some(symbol) = &metadata.symbolic_id
            && self
                .entities_state
                .symbols
                .get(symbol)
                .is_some_and(|&other| other != id)
        {
            return Err(ErrorReason::DuplicateSymbolicId);
        }

        metadata.classes.sort();
        metadata.classes.dedup();

        let changed = self.entities_state.entities[&id].metadata != metadata;
        self.remove_indexes(id);
        if let Some(symbol) = &metadata.symbolic_id {
            self.entities_state.symbols.insert(symbol.clone(), id);
        }
        for class in &metadata.classes {
            self.entities_state
                .classes
                .entry(class.clone())
                .or_default()
                .insert(id);
        }

        self.entities_state
            .entities
            .get_mut(&id)
            .expect("validated entity")
            .metadata = metadata;
        if changed {
            self.lifecycle_effects.push(
                systems::lifecycle_publisher::LifecycleObservation::Entity {
                    entity: id,
                    kind: systems::lifecycle_publisher::EntityLifecycleKind::MetadataChanged,
                },
            );
        }

        Ok(())
    }

    pub(in crate::world) fn remove_indexes(&mut self, id: EntityId) {
        let metadata = &self.entities_state.entities[&id].metadata;
        if let Some(symbol) = &metadata.symbolic_id {
            self.entities_state.symbols.remove(symbol);
        }
        for class in &metadata.classes {
            if let Some(entities) = self.entities_state.classes.get_mut(class) {
                entities.remove(&id);
                if entities.is_empty() {
                    self.entities_state.classes.remove(class);
                }
            }
        }
    }

    pub(in crate::world) fn create_entity(
        &mut self,
        alias: u32,
        metadata: &EntityMetadata,
        aliases: &mut BTreeMap<u32, EntityId>,
        created: &mut Vec<(u32, EntityId)>,
    ) -> Result<(), ErrorReason> {
        if aliases.contains_key(&alias) {
            return Err(ErrorReason::DuplicateAlias);
        }
        let id = self
            .entities_state
            .allocator
            .allocate()
            .ok_or(ErrorReason::Capacity)?;
        let persistent_id = self.entities_state.allocate_persistent_id()?;
        self.entities_state.entities.insert(
            id,
            WorldEntityRecord {
                persistent_id,
                ..WorldEntityRecord::default()
            },
        );
        self.operation_created.insert(id);
        self.lifecycle_effects
            .push(systems::lifecycle_publisher::LifecycleObservation::Entity {
                entity: id,
                kind: systems::lifecycle_publisher::EntityLifecycleKind::Created,
            });
        #[cfg(feature = "diagnostics")]
        self.record_entity_effect("entity.create", id);
        aliases.insert(alias, id);
        created.push((alias, id));
        let observation_count = self.lifecycle_effects.len();
        let metadata_result = self.set_metadata(id, metadata.clone());
        self.lifecycle_effects.truncate(observation_count);
        metadata_result
    }

    pub(in crate::world) fn delete_entity(&mut self, id: EntityId) {
        let components: Vec<_> = self.entities_state.entities[&id]
            .layers
            .keys()
            .copied()
            .collect();
        for component in components {
            self.touch_component(id, component);
        }
        self.remove_indexes(id);
        self.entities_state.entities.remove(&id);
        self.entities_state.retired_entities.push(id);
        self.entities_state.operation_deleted.insert(id);
        self.lifecycle_effects
            .push(systems::lifecycle_publisher::LifecycleObservation::Entity {
                entity: id,
                kind: systems::lifecycle_publisher::EntityLifecycleKind::Deleted,
            });
        #[cfg(feature = "diagnostics")]
        self.record_entity_effect("entity.delete", id);
    }
}

impl WorldEntityState {
    pub(in crate::world) fn allocate_persistent_id(
        &mut self,
    ) -> Result<EntityPersistentId, ErrorReason> {
        self.next_persistent_entity_id = self
            .next_persistent_entity_id
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        Ok(EntityPersistentId(self.next_persistent_entity_id))
    }
}
