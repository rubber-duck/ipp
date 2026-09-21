//! Reusable ordered staging for only the components affected by one controller.
//! Draining drops/moves every value; retained capacity contains no component mirror.

use crate::{ComponentValue, EntityId, ErrorReason, components::schema::FieldValue};
use std::collections::BTreeMap;

type Key = (EntityId, u16);
pub(super) type ComponentScratch = Vec<(Key, Option<ComponentValue>)>;
pub(super) type PropertyScratch = Vec<((Key, u32), FieldValue)>;

pub(super) struct AnimationComponentValues {
    values: ComponentScratch,
    properties: PropertyScratch,
    legacy: BTreeMap<Key, ComponentValue>,
    reuse: bool,
}

impl AnimationComponentValues {
    pub fn new(values: ComponentScratch, properties: PropertyScratch) -> Self {
        debug_assert!(values.is_empty());
        Self {
            values,
            properties,
            legacy: BTreeMap::new(),
            reuse: crate::allocation_optimizations_enabled(),
        }
    }

    pub fn get_or_insert(
        &mut self,
        key: Key,
        create: impl FnOnce() -> Result<ComponentValue, ErrorReason>,
    ) -> Result<&mut ComponentValue, ErrorReason> {
        if !self.reuse {
            return match self.legacy.entry(key) {
                std::collections::btree_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let mut value = create()?;
                    for ((target, offset), property) in &self.properties {
                        if *target == key {
                            value
                                .set_field(*offset, property.clone())
                                .map_err(|_| ErrorReason::InvalidField)?;
                        }
                    }
                    Ok(entry.insert(value))
                }
            };
        }
        let index = match self.values.binary_search_by_key(&key, |(key, _)| *key) {
            Ok(index) => index,
            Err(index) => {
                let value = create()?;
                self.values.insert(index, (key, Some(value)));
                return Ok(self.values[index].1.as_mut().unwrap());
            }
        };
        if self.values[index].1.is_none() {
            let mut value = create()?;
            for ((target, offset), property) in &self.properties {
                if *target == key {
                    value
                        .set_field(*offset, property.clone())
                        .map_err(|_| ErrorReason::InvalidField)?;
                }
            }
            self.values[index].1 = Some(value);
        }
        Ok(self.values[index].1.as_mut().unwrap())
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut ComponentValue> {
        self.values
            .iter_mut()
            .filter_map(|(_, value)| value.as_mut())
            .chain(self.legacy.values_mut())
    }

    /// The patch contains only owned numeric lanes. Reads see earlier writes in
    /// declaration order, including a whole-value edit to the same component.
    pub fn numeric_current(
        &self,
        key: Key,
        offset: u32,
        storage: &crate::components::registry::ComponentStorage,
    ) -> Option<FieldValue> {
        if !crate::components::registry::ComponentStorage::supports_numeric_property(key.1, offset)
            && !(crate::direct_numeric_updates_enabled()
                && super::numeric_fields::range(key.1, offset).is_some())
        {
            return None;
        }
        let value = if let Ok(index) = self.values.binary_search_by_key(&key, |(key, _)| *key)
            && let Some(value) = &self.values[index].1
        {
            value.field(offset).ok()?
        } else if let Some((_, value)) = self
            .properties
            .iter()
            .find(|(target, _)| *target == (key, offset))
        {
            value.clone()
        } else {
            storage.field(key.1, key.0.index() as usize, offset)?
        };
        match value {
            FieldValue::Dynamic(crate::DynamicValue::Asset(_))
            | FieldValue::Bytes(_)
            | FieldValue::String(_)
            | FieldValue::Entity(_) => None,
            value => Some(value),
        }
    }

    pub fn set_numeric(
        &mut self,
        key: Key,
        offset: u32,
        value: FieldValue,
    ) -> Result<(), ErrorReason> {
        match &value {
            FieldValue::Dynamic(value) => {
                value.validate().map_err(|_| ErrorReason::InvalidValue)?
            }
            FieldValue::F32(value) if !value.is_finite() => return Err(ErrorReason::InvalidValue),
            _ => {}
        }
        if !self.reuse
            && let Some(component) = self.legacy.get_mut(&key)
        {
            return component
                .set_field(offset, value)
                .map_err(|_| ErrorReason::InvalidField);
        }
        let index = match self.values.binary_search_by_key(&key, |(key, _)| *key) {
            Ok(index) => index,
            Err(index) => {
                self.values.insert(index, (key, None));
                index
            }
        };
        if let Some(component) = &mut self.values[index].1 {
            return component
                .set_field(offset, value)
                .map_err(|_| ErrorReason::InvalidField);
        }
        if let Some((_, previous)) = self
            .properties
            .iter_mut()
            .find(|(target, _)| *target == (key, offset))
        {
            *previous = value;
        } else {
            self.properties.push(((key, offset), value));
        }
        Ok(())
    }

    pub fn drain(
        &mut self,
    ) -> (
        &PropertyScratch,
        impl Iterator<Item = (Key, Option<ComponentValue>)> + '_,
    ) {
        (
            &self.properties,
            self.values.drain(..).chain(
                std::mem::take(&mut self.legacy)
                    .into_iter()
                    .map(|(key, value)| (key, Some(value))),
            ),
        )
    }

    pub fn into_scratch(mut self) -> (ComponentScratch, PropertyScratch) {
        debug_assert!(self.values.is_empty());
        self.properties.clear();
        (self.values, self.properties)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DynamicValue;

    #[test]
    fn mixed_numeric_and_whole_value_updates_preserve_declaration_order() {
        let entity = EntityId::from_bits(1 << 32);
        let key = (entity, ComponentValue::CUSTOM_MATERIAL);
        let mut original = crate::components::CustomMaterial::default();
        let offset = original
            .properties
            .set("number", DynamicValue::F32(1.0))
            .unwrap();
        let mut storage = crate::components::registry::ComponentStorage::default();
        storage.reserve(1);
        storage.set(0, ComponentValue::CustomMaterial(original));
        let mut values = AnimationComponentValues::new(Vec::new(), Vec::new());
        // This invariant concerns reusable numeric staging.
        values.reuse = true;
        values
            .set_numeric(key, offset, FieldValue::Dynamic(DynamicValue::F32(2.0)))
            .unwrap();
        let ComponentValue::CustomMaterial(value) = values
            .get_or_insert(key, || Ok(storage.get(key.1, 0).unwrap()))
            .unwrap()
        else {
            unreachable!();
        };
        assert_eq!(value.properties.get("number"), Some(DynamicValue::F32(2.0)));
        value.alpha_cutoff = 0.25;
        values
            .set_numeric(key, offset, FieldValue::Dynamic(DynamicValue::F32(3.0)))
            .unwrap();
        assert_eq!(
            values.numeric_current(key, offset, &storage),
            Some(FieldValue::Dynamic(DynamicValue::F32(3.0)))
        );
        let (_, mut updates) = values.drain();
        let (_, Some(ComponentValue::CustomMaterial(value))) = updates.next().unwrap() else {
            unreachable!();
        };
        assert_eq!(value.alpha_cutoff, 0.25);
        assert_eq!(value.properties.get("number"), Some(DynamicValue::F32(3.0)));
        assert!(updates.next().is_none());
        drop(updates);
        let (components, properties) = values.into_scratch();
        assert!(components.is_empty() && properties.is_empty());
        assert!(components.capacity() > 0 && properties.capacity() > 0);
    }
}
