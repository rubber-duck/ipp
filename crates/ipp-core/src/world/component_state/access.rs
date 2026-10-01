//! Read access to staged and retained component values.
//!
//! Pure reads used by commit validation, lifecycle observers and systems.
//! Ownership lives in [`super::super`]; this module only hosts the read path.
//! A component's current value is its batch's staged copy while one exists,
//! the commit's prepared value while a commit installs it, and otherwise the
//! retained storage.

use super::super::*;

impl WorldEntityState {
    fn prepared_value(&self, key: (EntityId, u16)) -> Option<&ComponentValue> {
        (!self.dirty.contains(&key))
            .then(|| self.prepared.get(&key))
            .flatten()
    }

    fn staged_value(&self, entity: EntityId, component: u16) -> Option<&ComponentValue> {
        self.entities
            .get(&entity)?
            .components
            .get(&component)?
            .staged
            .as_deref()
    }

    pub(in crate::world) fn input_value(
        &self,
        components: &registry::ComponentStorage,
        entity: EntityId,
        component: u16,
    ) -> Option<ComponentValue> {
        self.entities.get(&entity)?.input(component)?;
        if let Some(value) = self.prepared_value((entity, component)) {
            return Some(value.clone());
        }
        let mut value = self
            .staged_value(entity, component)
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
        self.entities.get(&entity)?.input(component)?;
        let prepared = self.prepared_value((entity, component));
        if prepared.is_none()
            && self.evaluated_target == Some((entity, component))
            && let Some((_, value)) = self
                .evaluated_properties
                .iter()
                .find(|(key, _)| *key == offset)
        {
            return Some(value.clone());
        }
        match prepared.or_else(|| self.staged_value(entity, component)) {
            Some(value) => value.field(offset).ok(),
            None => components.field(component, entity.index() as usize, offset),
        }
    }

    pub(in crate::world) fn input_skeleton<'a>(
        &'a self,
        components: &'a registry::ComponentStorage,
        entity: EntityId,
    ) -> Option<std::borrow::Cow<'a, crate::components::Skeleton>> {
        self.entities
            .get(&entity)?
            .input(ComponentValue::SKELETON)?;
        let value = match self
            .prepared_value((entity, ComponentValue::SKELETON))
            .or_else(|| self.staged_value(entity, ComponentValue::SKELETON))
        {
            Some(ComponentValue::Skeleton(value)) => value,
            Some(_) => return None,
            None => components.skeleton(entity.index() as usize)?,
        };
        Some(std::borrow::Cow::Borrowed(value))
    }
}

impl WorldMutationState {
    /// The current value of a present component, or the reason it cannot be read.
    pub(in crate::world) fn component(
        &self,
        components: &registry::ComponentStorage,
        id: EntityId,
        component: u16,
    ) -> Result<ComponentValue, ErrorReason> {
        ComponentValue::field_count(component).map_err(|_| ErrorReason::UnknownComponent)?;
        self.entities_state
            .input_value(components, id, component)
            .ok_or(ErrorReason::MissingComponent)
    }
}
