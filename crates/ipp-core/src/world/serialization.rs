//! Snapshot access at an exclusive World boundary, using ordinary typed lifecycle paths.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::services::world_serialization::{
    WorldPersistenceLimits, WorldSerializedEntity, WorldSnapshot,
};

impl WorldContext<'_> {
    /// Capture retained authored base data, excluding owned entities, overlays and fallbacks.
    /// Entity references become durable IDs; excluded targets cause an explicit failure.
    pub fn capture_world(&self, limits: WorldPersistenceLimits) -> Result<WorldSnapshot, String> {
        if self.world.updating {
            return Err("World capture requires a mutation boundary".into());
        }
        let ids: BTreeMap<_, _> = self
            .world
            .state
            .entities
            .iter()
            .filter(|(id, _)| {
                self.instances
                    .before
                    .iter()
                    .chain(self.instances.after.iter())
                    .all(|instance| instance.system.include_entity_in_snapshot(**id))
            })
            .map(|(&id, entity)| (id, entity.persistent_id))
            .collect();
        let mut entities = Vec::new();
        let mut bytes = 0usize;
        for (&id, &persistent_id) in &ids {
            if persistent_id.0 == 0 {
                return Err("Entity has no persistent identity".into());
            }
            let entity = &self.world.state.entities[&id];
            bytes = bytes
                .checked_add(128)
                .and_then(|bytes| bytes.checked_add(metadata_bytes(&entity.metadata)?))
                .ok_or("Snapshot size overflow")?;
            if bytes > limits.max_bytes {
                return Err("Snapshot byte budget exhausted".into());
            }
            let mut components = Vec::new();
            for (&component, layer) in &entity.layers {
                let Some(_base) = layer.inputs.authored_base() else {
                    continue;
                };
                let mut value = self
                    .read()
                    .producer_component(id, component)
                    .ok_or("Missing producer component")?;
                bytes = bytes
                    .checked_add(value.retained_bytes().ok_or("Component size overflow")?)
                    .and_then(|bytes| bytes.checked_add(512))
                    .ok_or("Snapshot size overflow")?;
                if bytes > limits.max_bytes {
                    return Err("Snapshot byte budget exhausted".into());
                }
                for (offset, field) in value.fields() {
                    if let crate::components::schema::FieldValue::Entity(target) = field {
                        let durable = if target.to_bits() == 0
                            && ComponentValue::accepts_null_entity(value.type_id(), offset)
                        {
                            0
                        } else {
                            ids.get(&target).ok_or_else(|| format!("Entity {} component {} references excluded or missing entity {}", id.to_bits(), value.type_id(), target.to_bits()))?.0
                        };
                        value
                            .set_field(
                                offset,
                                crate::components::schema::FieldValue::Entity(EntityId::from_bits(
                                    durable,
                                )),
                            )
                            .map_err(|error| format!("Persistent reference: {error:?}"))?;
                    }
                }
                components.push(value);
            }
            entities.push(WorldSerializedEntity {
                persistent_id,
                metadata: entity.metadata.clone(),
                components,
            });
        }
        let mut systems = BTreeMap::new();
        for instance in self
            .instances
            .before
            .iter()
            .chain(self.instances.after.iter())
        {
            let before = bytes;
            let mut context = crate::systems::SystemSaveContext {
                ids: &ids,
                entities: &entities,
                bytes: &mut bytes,
                max_bytes: limits.max_bytes,
            };
            if let Some(state) = instance.system.save_persistent_state(&mut context)? {
                let minimum = before
                    .checked_add(instance.id.0.len())
                    .and_then(|size| size.checked_add(state.len()))
                    .ok_or("System contribution size overflow")?;
                bytes = bytes.max(minimum);
                if bytes > limits.max_bytes {
                    return Err("System contribution exceeds snapshot byte budget".into());
                }
                systems.insert(instance.id.0.to_owned(), state);
            }
        }
        Ok(WorldSnapshot {
            metadata: self.world.metadata.clone(),
            capacity_hints: self.world.capacity_hints.clone(),
            next_entity_id: self.world.state.next_persistent_entity_id,
            entities,
            systems,
        })
    }

    pub(crate) fn restore_world_snapshot(
        &mut self,
        snapshot: &WorldSnapshot,
        persistence_limits: WorldPersistenceLimits,
    ) -> Result<BTreeMap<EntityPersistentId, EntityId>, String> {
        if self.world.updating || !self.world.state.entities.is_empty() {
            return Err("Restore requires an unpublished empty World".into());
        }
        self.world.restoring = true;
        let result = self.restore_world_contents(snapshot, persistence_limits);
        self.world.restoring = false;
        result
    }

    fn restore_world_contents(
        &mut self,
        snapshot: &WorldSnapshot,
        persistence_limits: WorldPersistenceLimits,
    ) -> Result<BTreeMap<EntityPersistentId, EntityId>, String> {
        let mut aliases = BTreeMap::new();
        let mut created = Vec::new();
        let mut ids = BTreeMap::new();
        for (index, entity) in snapshot.entities.iter().enumerate() {
            if entity.persistent_id.0 == 0
                || entity.persistent_id.0 > snapshot.next_entity_id
                || ids.contains_key(&entity.persistent_id)
            {
                return Err("Invalid or duplicate persistent entity identity".into());
            }
            let alias = u32::try_from(index).map_err(|_| "Too many serialized entities")?;
            self.runtime_access()
                .apply_operation(
                    None,
                    &Command::Create {
                        alias,
                        metadata: entity.metadata.clone(),
                    },
                    &mut aliases,
                    &mut created,
                )
                .map_err(|error| error.to_string())?;
            let id = aliases[&alias];
            self.world
                .state
                .entities
                .get_mut(&id)
                .expect("created entity")
                .persistent_id = entity.persistent_id;
            ids.insert(entity.persistent_id, id);
        }
        let types: BTreeSet<_> = snapshot
            .entities
            .iter()
            .flat_map(|entity| entity.components.iter().map(ComponentValue::type_id))
            .collect();
        for component_type in types {
            for entity in &snapshot.entities {
                let Some(component) = entity
                    .components
                    .iter()
                    .find(|component| component.type_id() == component_type)
                else {
                    continue;
                };
                let mut value = component.clone();
                for (offset, field) in value.fields() {
                    if let crate::components::schema::FieldValue::Entity(target) = field {
                        let target = if target.to_bits() == 0
                            && ComponentValue::accepts_null_entity(component_type, offset)
                        {
                            target
                        } else {
                            *ids.get(&EntityPersistentId(target.to_bits()))
                                .ok_or("Missing persistent entity reference")?
                        };
                        value
                            .set_field(
                                offset,
                                crate::components::schema::FieldValue::Entity(target),
                            )
                            .map_err(|error| format!("Persistent reference: {error:?}"))?;
                    }
                }
                self.runtime_access()
                    .apply_operation(
                        None,
                        &Command::InsertComponentValue {
                            entity: EntityRef::Handle(ids[&entity.persistent_id]),
                            value,
                        },
                        &mut aliases,
                        &mut created,
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        self.world.state.next_persistent_entity_id = snapshot.next_entity_id;
        self.world
            .components
            .try_reserve(self.world.state.allocator.slots())
            .map_err(|error| error.to_string())?;
        self.commit_pending_changes()
            .map_err(|error| error.to_string())?;
        for id in snapshot.systems.keys() {
            if !self.system_ids().any(|selected| selected.0 == id) {
                return Err(format!("Snapshot system {id} is not selected"));
            }
        }
        for index in 0..self.instances.before.len() {
            let (before, tail) = self.instances.before.split_at_mut(index);
            let (instance, after) = tail.split_first_mut().expect("selected system");
            let mut context = crate::systems::SystemLoadContext {
                world: crate::systems::SystemRuntimeAccess {
                    world: self.world,
                    instances: super::access::SystemInstanceAccess {
                        before,
                        current: Some(instance.id),
                        after,
                    },
                    asset_acquisition: self.asset_acquisition,
                    data_sources: self.data_sources,
                },
                ids: &ids,
                max_bytes: persistence_limits.max_bytes,
            };
            instance
                .system
                .load_persistent_state(&mut context, snapshot.systems.get(instance.id.0))?;
        }
        Ok(ids)
    }
}
