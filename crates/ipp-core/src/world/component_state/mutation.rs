//! Command application against staged component state.
//!
//! Entity identity work lives in [`super::super::entity_state`]; this module
//! only hosts per-command component mutation. Ownership lives in
//! [`super::super`].

use super::super::*;

impl WorldMutationState {
    pub(in crate::world) fn apply(
        &mut self,
        components: &registry::ComponentStorage,
        command: &Command,
        aliases: &mut BTreeMap<u32, EntityId>,
        created: &mut Vec<(u32, EntityId)>,
        limits: WorldLimits,
    ) -> Result<(), ErrorReason> {
        let result = (|| {
            self.stage_command_inputs(components, command, aliases)?;
            match command {
                Command::Create {
                    alias,
                    metadata,
                } => {
                    self.create_entity(*alias, metadata, aliases, created)?;
                }
                Command::Delete {
                    entity,
                } => {
                    let id = self.resolve(*entity, aliases)?;
                    self.delete_entity(id);
                }
                Command::SetMetadata {
                    entity,
                    metadata,
                } => {
                    let id = self.resolve(*entity, aliases)?;
                    self.set_metadata(id, metadata.clone())?;
                }
                Command::InsertComponent {
                    entity,
                    component,
                    fields,
                } => {
                    let id = self.resolve(*entity, aliases)?;
                    let mut value = registry::create(*component)?;
                    for field in fields {
                        registry::write(
                            &mut value,
                            &self.field(*component, field.clone(), aliases)?,
                        )?;
                    }
                    self.insert_component_value(components, id, value)?;
                }
                Command::InsertComponentValue {
                    entity,
                    value,
                } => {
                    let id = self.resolve(*entity, aliases)?;
                    for (offset, field) in value.fields() {
                        value.validate_field_lifecycle(offset)?;
                        if matches!(field, crate::components::schema::FieldValue::F32(value) if !value.is_finite())
                        {
                            return Err(ErrorReason::InvalidValue);
                        }
                        if let crate::components::schema::FieldValue::Entity(entity) = field
                            && !(entity.to_bits() == 0
                                && ComponentValue::accepts_null_entity(value.type_id(), offset))
                        {
                            self.resolve(EntityRef::Handle(entity), aliases)?;
                        }
                    }
                    self.insert_component_value(components, id, value.clone())?;
                }
                Command::SetField {
                    entity,
                    component,
                    field,
                } => {
                    let id = self.resolve(*entity, aliases)?;
                    let field = self.field(*component, field.clone(), aliases)?;
                    self.write_component_field(components, id, *component, &field)?;
                }
                Command::SetDynamicProperty {
                    entity,
                    component,
                    name,
                    value,
                } => {
                    let id = self.resolve(*entity, aliases)?;
                    self.stage_component(components, id, *component);
                    let layer = self
                        .entities_state
                        .entities
                        .get_mut(&id)
                        .and_then(|entity| entity.layers.get_mut(component))
                        .ok_or(ErrorReason::MissingComponent)?;
                    let properties = layer
                        .inputs
                        .base_value_mut()
                        .ok_or(ErrorReason::MissingComponent)?
                        .dynamic_properties_mut()
                        .ok_or(ErrorReason::InvalidField)?;
                    let old = properties.key(name);
                    let key = properties
                        .set(name, value.clone())
                        .map_err(|_| ErrorReason::InvalidValue)?;
                    if let Some(old) = old {
                        self.explicit_fields.insert((id, *component, old));
                    }
                    self.explicit_fields.insert((id, *component, key));
                    self.touch_component(id, *component);
                }
                Command::RemoveDynamicProperty {
                    entity,
                    component,
                    name,
                } => {
                    let id = self.resolve(*entity, aliases)?;
                    self.stage_component(components, id, *component);
                    let layer = self
                        .entities_state
                        .entities
                        .get_mut(&id)
                        .and_then(|entity| entity.layers.get_mut(component))
                        .ok_or(ErrorReason::MissingComponent)?;
                    let properties = layer
                        .inputs
                        .base_value_mut()
                        .ok_or(ErrorReason::MissingComponent)?
                        .dynamic_properties_mut()
                        .ok_or(ErrorReason::InvalidField)?;
                    if let Some(key) = properties.remove(name) {
                        self.explicit_fields.insert((id, *component, key));
                    }
                    self.touch_component(id, *component);
                }
                Command::RemoveComponent {
                    entity,
                    component,
                } => {
                    let id = self.resolve(*entity, aliases)?;
                    self.touch_component(id, *component);
                    self.component(components, id, *component)?;
                    let layer = self
                        .entities_state
                        .entities
                        .get_mut(&id)
                        .unwrap()
                        .layers
                        .get_mut(component)
                        .unwrap();
                    layer.inputs.replace_base(None);
                }
                _ => return Err(ErrorReason::InvalidValue),
            }
            Ok(())
        })();
        let _ = limits;
        result
    }

    /// Update one producer field without changing incarnation or ownership.
    /// Errors leave prior writes applied; callers finish affected lifecycle work.
    pub(in crate::world) fn write_component_field(
        &mut self,
        components: &registry::ComponentStorage,
        id: EntityId,
        component: u16,
        field: &FieldWrite,
    ) -> Result<(), ErrorReason> {
        self.stage_component(components, id, component);
        self.touch_component(id, component);
        let mut value = self.component(components, id, component)?;
        registry::write(&mut value, field)?;
        *self
            .entities_state
            .entities
            .get_mut(&id)
            .unwrap()
            .layers
            .get_mut(&component)
            .unwrap()
            .inputs
            .base_value_mut()
            .unwrap() = value;
        self.explicit_fields.insert((id, component, field.offset));
        Ok(())
    }

    pub(in crate::world) fn insert_component_value(
        &mut self,
        _components: &registry::ComponentStorage,
        id: EntityId,
        value: ComponentValue,
    ) -> Result<(), ErrorReason> {
        let component = value.type_id();
        self.touch_component(id, component);
        let incarnation = self.incarnation()?;
        let layer = self
            .entities_state
            .entities
            .get_mut(&id)
            .unwrap()
            .layers
            .entry(component)
            .or_default();
        layer.inputs.replace_base(Some(ComponentStateInstance {
            base: value,
            incarnation,
        }));
        Ok(())
    }
}
