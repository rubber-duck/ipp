//! Sparse producer-reference lookup for mandatory deletion cleanup.

use super::SystemOperationContext;
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{EntityId, EntityRef, FieldValue, FieldWrite};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct ProducerReferenceIndex {
    targets: BTreeMap<EntityId, EntityId>,
    users: BTreeMap<EntityId, BTreeSet<EntityId>>,
}

impl ProducerReferenceIndex {
    fn set(&mut self, entity: EntityId, target: Option<EntityId>) {
        if self.targets.get(&entity).copied() == target {
            return;
        }
        if let Some(old) = self.targets.remove(&entity)
            && let Some(users) = self.users.get_mut(&old)
        {
            users.remove(&entity);
            if users.is_empty() {
                self.users.remove(&old);
            }
        }
        if let Some(target) = target {
            self.targets.insert(entity, target);
            self.users.entry(target).or_default().insert(entity);
        }
    }

    fn read(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
        entity: EntityId,
        component: u16,
        offset: u32,
    ) {
        let target = state
            .producer_value(&world.components, entity, component)
            .and_then(|value| {
                value
                    .fields()
                    .into_iter()
                    .find(|(field, _)| *field == offset)
            })
            .and_then(|(_, value)| match value {
                crate::components::schema::FieldValue::Entity(target) if target.to_bits() != 0 => {
                    Some(target)
                }
                _ => None,
            });
        self.set(entity, target);
    }

    pub(super) fn rebuild(
        &mut self,
        world: &WorldSimulationState,
        state: &WorldEntityState,
        component: u16,
        offset: u32,
    ) {
        *self = Self::default();
        for &entity in state.entities.keys() {
            self.read(world, state, entity, component, offset);
        }
    }

    pub(super) fn reconcile(
        &mut self,
        context: &mut SystemOperationContext<'_>,
        component: u16,
        offset: u32,
    ) {
        for &entity in &context.staged.operation_deleted {
            self.set(entity, None);
        }
        let users: BTreeSet<_> = context
            .staged
            .operation_deleted
            .iter()
            .filter_map(|id| self.users.get(id))
            .flatten()
            .copied()
            .collect();
        for entity in users {
            if !context.staged.entities.contains_key(&entity) {
                continue;
            }
            let field = FieldWrite {
                offset,
                value: FieldValue::Entity(EntityRef::Handle(EntityId::from_bits(0))),
            };
            context
                .staged
                .write_component_field(&context.world_data.components, entity, component, &field)
                .expect("indexed live producer reference");
            let inputs = &mut context
                .staged
                .entities
                .get_mut(&entity)
                .unwrap()
                .layers
                .get_mut(&component)
                .unwrap()
                .inputs;
            if let Some((_, value)) = inputs
                .hidden_fields
                .iter_mut()
                .find(|(field, _)| *field == offset)
            {
                *value = crate::components::schema::FieldValue::Entity(EntityId::from_bits(0));
            } else if let Some(value) = inputs.layered_value_mut() {
                value
                    .set_field(
                        offset,
                        crate::components::schema::FieldValue::Entity(EntityId::from_bits(0)),
                    )
                    .expect("typed reference");
            }
        }
        for &(entity, kind) in &context.staged.operation_components {
            if kind == component {
                self.read(
                    context.world_data,
                    context.staged,
                    entity,
                    component,
                    offset,
                );
            }
        }
    }
}
