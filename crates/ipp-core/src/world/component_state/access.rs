//! Read access to staged and retained component values.
//!
//! Pure reads used by commit validation, lifecycle observers and systems.
//! Ownership lives in [`super::super`]; this module only hosts the read path.

use super::super::*;

impl WorldEntityState {
    pub(in crate::world) fn input_value(
        &self,
        components: &registry::ComponentStorage,
        entity: EntityId,
        component: u16,
    ) -> Option<ComponentValue> {
        let layer = self.entities.get(&entity)?.layers.get(&component)?;
        layer.input()?;
        if !self.dirty.contains(&(entity, component))
            && let Some(value) = self.prepared.get(&(entity, component))
        {
            return Some(value.clone());
        }
        let mut value = layer
            .inputs
            .input_value()
            .cloned()
            .or_else(|| components.get(component, entity.index() as usize))?;
        if self.evaluated_target == Some((entity, component)) {
            for (offset, property) in &self.evaluated_properties {
                value.set_field(*offset, property.clone()).ok()?;
            }
        }
        Some(value)
    }

    pub(in crate::world) fn input_field(
        &self,
        components: &registry::ComponentStorage,
        entity: EntityId,
        component: u16,
        offset: u32,
    ) -> Option<crate::components::schema::FieldValue> {
        let layer = self.entities.get(&entity)?.layers.get(&component)?;
        layer.input()?;
        let prepared = (!self.dirty.contains(&(entity, component)))
            .then(|| self.prepared.get(&(entity, component)))
            .flatten();
        if prepared.is_none()
            && self.evaluated_target == Some((entity, component))
            && let Some((_, value)) = self
                .evaluated_properties
                .iter()
                .find(|(key, _)| *key == offset)
        {
            return Some(value.clone());
        }
        match prepared.or_else(|| layer.inputs.input_value()) {
            Some(value) => value.field(offset).ok(),
            None => components.field(component, entity.index() as usize, offset),
        }
    }

    #[cfg(feature = "skeletal-animation")]
    pub(in crate::world) fn input_skeleton<'a>(
        &'a self,
        components: &'a registry::ComponentStorage,
        entity: EntityId,
    ) -> Option<std::borrow::Cow<'a, crate::components::Skeleton>> {
        if !crate::allocation_optimizations_enabled() {
            return match self.input_value(components, entity, ComponentValue::SKELETON)? {
                ComponentValue::Skeleton(value) => Some(std::borrow::Cow::Owned(value)),
                _ => None,
            };
        }
        let layer = self
            .entities
            .get(&entity)?
            .layers
            .get(&ComponentValue::SKELETON)?;
        layer.input()?;
        let prepared = (!self.dirty.contains(&(entity, ComponentValue::SKELETON)))
            .then(|| self.prepared.get(&(entity, ComponentValue::SKELETON)))
            .flatten();
        let value = match prepared.or_else(|| layer.inputs.input_value()) {
            Some(ComponentValue::Skeleton(value)) => value,
            Some(_) => return None,
            None => components.skeleton(entity.index() as usize)?,
        };
        Some(std::borrow::Cow::Borrowed(value))
    }

    pub(in crate::world) fn producer_value(
        &self,
        components: &registry::ComponentStorage,
        entity: EntityId,
        component: u16,
    ) -> Option<ComponentValue> {
        let layer = self.entities.get(&entity)?.layers.get(&component)?;
        layer.inputs.base()?;
        if let Some(value) = layer.inputs.base_value() {
            return Some(value.clone());
        }
        let mut value = components.get(component, entity.index() as usize)?;
        layer.inputs.restore_producer(&mut value);
        Some(value)
    }

    /// Apply sparse overlay originals after animation/constraint restoration.
    pub(in crate::world) fn restore_producer_value(
        &self,
        entity: EntityId,
        value: &mut ComponentValue,
    ) {
        if let Some(layer) = self
            .entities
            .get(&entity)
            .and_then(|record| record.layers.get(&value.type_id()))
        {
            layer.inputs.restore_producer(value);
        }
    }
}

impl WorldMutationState {
    pub(in crate::world) fn component(
        &self,
        components: &registry::ComponentStorage,
        id: EntityId,
        component: u16,
    ) -> Result<ComponentValue, ErrorReason> {
        ComponentValue::field_count(component).map_err(|_| ErrorReason::UnknownComponent)?;
        self.entities_state
            .producer_value(components, id, component)
            .ok_or(ErrorReason::MissingComponent)
    }
}
