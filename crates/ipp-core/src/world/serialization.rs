//! Snapshot access at an exclusive World boundary, using ordinary typed lifecycle paths.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::services::world_serialization::{
    WorldCaptureReferences, WorldPersistenceLimits, WorldSerializedEntity,
    WorldSerializedEntityLink, WorldSerializedReference, WorldSerializedReferenceValue,
    WorldSnapshot,
};

impl WorldContext<'_> {
    /// Capture stored components and links as they are, for every live entity.
    /// Entity references become durable IDs; missing targets cause an explicit failure.
    pub fn capture_world(&self, limits: WorldPersistenceLimits) -> Result<WorldSnapshot, String> {
        self.capture_world_with_references(
            &WorldCaptureReferences {
                worlds: BTreeMap::new(),
                outputs: BTreeMap::new(),
            },
            &mut Vec::new(),
            &mut 0,
            limits,
        )
    }

    fn snapshot_entity_ids(
        &self,
        max_bytes: usize,
    ) -> Result<BTreeMap<EntityId, EntityPersistentId>, String> {
        let mut ids = BTreeMap::new();
        for (&id, entity) in &self.world.state.entities {
            if ids.len() >= max_bytes / 128 {
                return Err("Snapshot identity budget exhausted".into());
            }
            ids.insert(id, entity.persistent_id);
        }
        Ok(ids)
    }

    pub(crate) fn snapshot_children(
        &self,
        max_bytes: usize,
    ) -> Result<Vec<crate::WorldRef>, String> {
        let mut children = Vec::new();
        for entity in self.snapshot_entity_ids(max_bytes)?.keys() {
            if self.world.state.entities[entity]
                .input(ComponentValue::WORLD_ATTACHMENT)
                .is_none()
            {
                continue;
            }
            let Some(ComponentValue::WorldAttachment(value)) = self.world.state.input_value(
                &self.world.components,
                *entity,
                ComponentValue::WORLD_ATTACHMENT,
            ) else {
                return Err("Missing stored attachment".into());
            };
            if let Some(child) = value.child() {
                children.push(child);
            }
        }
        Ok(children)
    }

    pub(crate) fn snapshot_outputs(
        &self,
        max_bytes: usize,
    ) -> Result<Vec<(crate::OutputRef, Option<EntityPersistentId>)>, String> {
        let mut outputs = Vec::new();
        if let Ok(canvas) = self.bind_output_target(crate::OutputTarget::Canvas) {
            outputs.push((canvas, None));
        }
        for (entity, persistent) in self.snapshot_entity_ids(max_bytes)? {
            if let Ok(camera) = self.bind_output(entity, crate::OutputKind::Camera) {
                outputs.push((camera, Some(persistent)));
            }
        }
        Ok(outputs)
    }

    pub(crate) fn capture_world_with_references(
        &self,
        graph: &WorldCaptureReferences,
        references: &mut Vec<WorldSerializedReference>,
        used_bytes: &mut usize,
        limits: WorldPersistenceLimits,
    ) -> Result<WorldSnapshot, String> {
        if self.world.updating {
            return Err("World capture requires a mutation boundary".into());
        }
        let ids = self.snapshot_entity_ids(limits.max_bytes.saturating_sub(*used_bytes))?;
        let mut entities = Vec::new();
        let mut bytes = used_bytes
            .checked_add(512 + self.world.metadata.symbolic_id.len())
            .ok_or("Snapshot size overflow")?;
        for system in self.world.manifest.systems() {
            bytes = bytes
                .checked_add(64 + system.0.len())
                .ok_or("Snapshot size overflow")?;
        }
        for (system, hints) in &self.world.capacity_hints.systems {
            bytes = bytes
                .checked_add(128 + system.len())
                .ok_or("Snapshot size overflow")?;
            for key in hints.0.keys() {
                bytes = bytes
                    .checked_add(128 + key.len())
                    .ok_or("Snapshot size overflow")?;
            }
        }
        if bytes > limits.max_bytes {
            return Err("Snapshot byte budget exhausted".into());
        }
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
            for &component in entity.components.keys() {
                let mut value = self
                    .world
                    .state
                    .input_value(&self.world.components, id, component)
                    .ok_or("Missing stored component")?;
                bytes = bytes
                    .checked_add(value.retained_bytes().ok_or("Component size overflow")?)
                    .and_then(|bytes| bytes.checked_add(512))
                    .ok_or("Snapshot size overflow")?;
                if bytes > limits.max_bytes {
                    return Err("Snapshot byte budget exhausted".into());
                }
                for (offset, field) in value.fields() {
                    let reference = match &field {
                        crate::components::schema::FieldValue::World(Some(target)) => Some((
                            WorldSerializedReferenceValue::World(
                                *graph
                                    .worlds
                                    .get(target)
                                    .ok_or("World reference outside the serializable graph")?,
                            ),
                            crate::components::schema::FieldValue::World(None),
                        )),
                        crate::components::schema::FieldValue::Output(Some(target)) => Some((
                            WorldSerializedReferenceValue::Output(
                                graph
                                    .outputs
                                    .get(target)
                                    .ok_or("Output reference targets an unavailable producer")?
                                    .clone(),
                            ),
                            crate::components::schema::FieldValue::Output(None),
                        )),
                        _ => None,
                    };
                    if let Some((reference, placeholder)) = reference {
                        bytes = bytes.checked_add(128).ok_or("Snapshot size overflow")?;
                        if bytes > limits.max_bytes {
                            return Err("Snapshot byte budget exhausted".into());
                        }
                        references.push(WorldSerializedReference {
                            entity: persistent_id,
                            component,
                            field: offset,
                            value: reference,
                        });
                        value
                            .set_field(offset, placeholder)
                            .map_err(|error| format!("Persistent reference: {error:?}"))?;
                    }
                    if let crate::components::schema::FieldValue::Entity(target) = field {
                        let durable = if target.to_bits() == 0
                            && ComponentValue::accepts_null_entity(value.type_id(), offset)
                        {
                            0
                        } else {
                            ids.get(&target)
                                .ok_or_else(|| {
                                    format!(
                                        "Entity {} component {} references missing entity {}",
                                        id.to_bits(),
                                        value.type_id(),
                                        target.to_bits()
                                    )
                                })?
                                .0
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
            let link = self
                .world
                .state
                .links
                .effective(id)
                .ok_or("Missing entity link")?;
            let parent = link
                .parent
                .map(|parent| {
                    ids.get(&parent)
                        .copied()
                        .ok_or("Entity link references a missing parent")
                })
                .transpose()?;
            entities.push(WorldSerializedEntity {
                persistent_id,
                link: WorldSerializedEntityLink {
                    parent,
                    order: link.order,
                },
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
        *used_bytes = bytes;
        Ok(WorldSnapshot {
            metadata: self.world.metadata.clone(),
            capacity_hints: self.world.capacity_hints.clone(),
            selected_systems: self
                .world
                .manifest
                .systems()
                .iter()
                .map(|system| system.0.to_owned())
                .collect(),
            next_entity_id: self.world.state.next_persistent_entity_id,
            entities,
            systems,
        })
    }

    pub(crate) fn restore_world_entities(
        &mut self,
        snapshot: &WorldSnapshot,
    ) -> Result<BTreeMap<EntityPersistentId, EntityId>, String> {
        if self.world.updating || !self.world.state.entities.is_empty() {
            return Err("Restore requires an unpublished empty World".into());
        }
        self.world.restoring = true;
        let mut aliases = EntityAliases::default();
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
                        adopt: false,
                    },
                    &mut aliases,
                    &mut created,
                    &mut Vec::new(),
                )
                .map_err(|error| error.to_string())?;
            let id = aliases
                .identity(&EntityRef::Alias(alias), &self.world.state.symbols)
                .expect("created entity alias");
            self.world
                .state
                .entities
                .get_mut(&id)
                .expect("created entity")
                .persistent_id = entity.persistent_id;
            self.world
                .state
                .links
                .restore_identity(id, entity.persistent_id);
            ids.insert(entity.persistent_id, id);
        }
        self.world.state.links.operation_changed.clear();
        for entity in &snapshot.entities {
            let parent = entity
                .link
                .parent
                .map(|parent| ids.get(&parent).copied().ok_or("Missing persistent parent"))
                .transpose()?;
            self.world
                .state
                .links
                .set(
                    ids[&entity.persistent_id],
                    EntityLink {
                        parent,
                        order: entity.link.order,
                    },
                )
                .map_err(|error| error.to_string())?;
        }
        self.world
            .state
            .links
            .reconcile()
            .map_err(|error| error.to_string())?;
        self.world.state.next_persistent_entity_id = snapshot.next_entity_id;
        Ok(ids)
    }

    pub(crate) fn restore_world_components(
        &mut self,
        snapshot: &WorldSnapshot,
        ids: &BTreeMap<EntityPersistentId, EntityId>,
        deferred: &BTreeSet<(EntityPersistentId, u16)>,
    ) -> Result<(), String> {
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
                if deferred.contains(&(entity.persistent_id, component_type)) {
                    continue;
                }
                self.restore_world_component(ids[&entity.persistent_id], component.clone(), ids)?;
            }
        }
        self.finish_restored_components()
    }

    pub(crate) fn restore_world_component(
        &mut self,
        entity: EntityId,
        mut value: ComponentValue,
        ids: &BTreeMap<EntityPersistentId, EntityId>,
    ) -> Result<(), String> {
        for (offset, field) in value.fields() {
            if let crate::components::schema::FieldValue::Entity(target) = field {
                let target = if target.to_bits() == 0
                    && ComponentValue::accepts_null_entity(value.type_id(), offset)
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
                &Command::insert_value(EntityRef::Handle(entity), value),
                &mut EntityAliases::default(),
                &mut Vec::new(),
                &mut Vec::new(),
            )
            .map_err(|error| error.to_string())
    }

    pub(crate) fn finish_restored_components(&mut self) -> Result<(), String> {
        self.world
            .components
            .try_reserve(self.world.state.allocator.slots())
            .map_err(|error| error.to_string())?;
        self.commit_pending_changes()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn restore_world_systems(
        &mut self,
        snapshot: &WorldSnapshot,
        ids: &BTreeMap<EntityPersistentId, EntityId>,
        persistence_limits: WorldPersistenceLimits,
    ) -> Result<(), String> {
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
                    io: self.io,
                    data: self.data,
                    topology: self.topology,
                    frame_context: self.frame_context,
                    reference_worlds: self.reference_worlds.as_ref(),
                },
                ids,
                max_bytes: persistence_limits.max_bytes,
            };
            instance
                .system
                .load_persistent_state(&mut context, snapshot.systems.get(instance.id.0))?;
        }
        self.world.restoring = false;
        Ok(())
    }
}
