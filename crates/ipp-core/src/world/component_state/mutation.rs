//! Command application against staged component state.
//!
//! Entity identity work lives in [`super::super::entity_state`]; this module
//! only hosts per-command component mutation. Ownership lives in
//! [`super::super`].

use super::super::*;
use super::observations::ComponentStagedWrite;

impl WorldMutationState {
    pub(in crate::world) fn apply(
        &mut self,
        components: &registry::ComponentStorage,
        command: &Command,
        aliases: &mut EntityAliases,
        created: &mut Vec<(u32, EntityId)>,
        limits: WorldLimits,
    ) -> Result<(), ErrorReason> {
        let result = (|| {
            self.stage_command_inputs(components, command, aliases)?;
            match command {
                Command::Create {
                    alias,
                    metadata,
                    adopt,
                } => {
                    self.create_entity(*alias, metadata, *adopt, aliases, created)?;
                }
                Command::Delete {
                    entity,
                } => {
                    let id = self.resolve(entity, aliases)?;
                    self.delete_entity(id);
                }
                Command::SetMetadata {
                    entity,
                    metadata,
                } => {
                    let id = self.resolve(entity, aliases)?;
                    self.set_metadata(id, metadata.clone())?;
                }
                Command::PlaceEntity {
                    entity,
                    placement,
                } => {
                    let entity = self.resolve(entity, aliases)?;
                    let value = self.resolve_placement(entity, placement, aliases)?;
                    self.links.set(entity, value)?;
                }
                Command::DeleteSubtree {
                    root,
                } => {
                    let root = self.resolve(root, aliases)?;
                    for entity in self.links.subtree(root) {
                        self.delete_entity(entity);
                    }
                }
                Command::InsertComponent {
                    entity,
                    component,
                    fields,
                    adopt,
                } => {
                    let id = self.resolve(entity, aliases)?;

                    // Adoption writes the listed fields over an existing component.
                    if *adopt && self.has_component(id, *component) {
                        let fields = fields
                            .iter()
                            .map(|field| self.field(*component, field.clone(), aliases))
                            .collect::<Result<Vec<_>, _>>()?;
                        self.write_component_fields(components, id, *component, &fields)?;
                        self.operation_adopted = true;
                        return Ok(());
                    }
                    let mut value = registry::create(*component)?;
                    for field in fields {
                        registry::assign(
                            &mut value,
                            &self.field(*component, field.clone(), aliases)?,
                        )?;
                    }
                    // A whole insertion validates the complete value once,
                    // independently of the order of its field assignments.
                    value.validate_lifecycle()?;
                    self.insert_component_value(components, id, value)?;
                }
                Command::InsertComponentValue {
                    entity,
                    value,
                } => {
                    let id = self.resolve(entity, aliases)?;
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
                            self.resolve(&EntityRef::Handle(entity), aliases)?;
                        }
                    }
                    value.validate_lifecycle()?;
                    self.insert_component_value(components, id, ComponentValue::clone(value))?;
                }
                Command::SetField {
                    entity,
                    component,
                    field,
                } => {
                    let id = self.resolve(entity, aliases)?;
                    let field = self.field(*component, field.clone(), aliases)?;
                    self.write_component_field(components, id, *component, &field)?;
                }
                Command::SetFieldIf {
                    entity,
                    component,
                    field,
                    expected,
                } => {
                    let id = self.resolve(entity, aliases)?;
                    let field = self.field(*component, field.clone(), aliases)?;
                    let expected = self.field(
                        *component,
                        FieldWrite {
                            offset: field.offset,
                            value: FieldValue::clone(expected),
                        },
                        aliases,
                    )?;
                    self.compare_component_field(components, id, *component, &expected)?;
                    self.write_component_field(components, id, *component, &field)?;
                }
                Command::SetDynamicProperty {
                    entity,
                    component,
                    name,
                    value,
                } => {
                    let id = self.resolve(entity, aliases)?;
                    let (key, old, changed) =
                        self.stage_dynamic_property(components, id, *component, |properties| {
                            // Report whether the value changed so the lifecycle
                            // observation needs no whole-value comparison.
                            let old = properties.key(name);
                            let before = old.and_then(|key| properties.stored_value(key));
                            let key = properties
                                .set(name, value.clone())
                                .map_err(|_| ErrorReason::InvalidValue)?;
                            let changed =
                                Some(key) != old || properties.stored_value(key) != before;
                            Ok((key, old, changed))
                        })?;
                    self.touch_component_write(
                        id,
                        *component,
                        ComponentStagedWrite::SetProperty(name.clone(), value.clone()),
                        changed,
                    );
                    if let Some(old) = old {
                        self.explicit_fields.insert((id, *component, old));
                    }
                    self.explicit_fields.insert((id, *component, key));
                }
                Command::RemoveDynamicProperty {
                    entity,
                    component,
                    name,
                } => {
                    let id = self.resolve(entity, aliases)?;
                    let removed =
                        self.stage_dynamic_property(components, id, *component, |properties| {
                            Ok(properties.remove(name))
                        })?;
                    self.touch_component_write(
                        id,
                        *component,
                        ComponentStagedWrite::RemoveProperty(name.clone()),
                        removed.is_some(),
                    );
                    if let Some(key) = removed {
                        self.explicit_fields.insert((id, *component, key));
                    }
                }
                Command::RemoveComponent {
                    entity,
                    component,
                } => {
                    let id = self.resolve(entity, aliases)?;
                    ComponentValue::field_count(*component)
                        .map_err(|_| ErrorReason::UnknownComponent)?;
                    if !self.has_component(id, *component) {
                        return Err(ErrorReason::MissingComponent);
                    }
                    self.touch_component(id, *component);
                    self.entities_state
                        .entities
                        .get_mut(&id)
                        .expect("resolved entity")
                        .components
                        .remove(component);
                }
                _ => return Err(ErrorReason::InvalidValue),
            }
            Ok(())
        })();
        let _ = limits;
        result
    }

    /// Update one field in place without changing the component incarnation.
    /// A write whose result fails validation has no effect.
    pub(in crate::world) fn write_component_field(
        &mut self,
        components: &registry::ComponentStorage,
        id: EntityId,
        component: u16,
        field: &FieldWrite,
    ) -> Result<(), ErrorReason> {
        ComponentValue::field_count(component).map_err(|_| ErrorReason::UnknownComponent)?;
        self.stage_component(components, id, component);

        let value = self
            .entities_state
            .entities
            .get_mut(&id)
            .and_then(|record| record.components.get_mut(&component))
            .and_then(|state| state.staged.as_deref_mut())
            .ok_or(ErrorReason::MissingComponent)?;
        let changed = registry::write_stored_field(value, field)?;

        // Replacing the dynamic property metadata rebuilds every property, so
        // its observation compares whole values.
        if field.offset == crate::components::dynamic_properties::DYNAMIC_METADATA {
            self.touch_component(id, component);
        } else {
            self.touch_component_write(
                id,
                component,
                ComponentStagedWrite::Field(field.clone()),
                changed,
            );
        }
        self.explicit_fields.insert((id, component, field.offset));
        Ok(())
    }

    /// Whether the entity holds the component.
    fn has_component(&self, id: EntityId, component: u16) -> bool {
        self.entities_state
            .entities
            .get(&id)
            .and_then(|record| record.input(component))
            .is_some()
    }

    /// Fail with [`ErrorReason::ValueMismatch`] unless the stored field equals
    /// `expected`: text by shared reference first, then by content.
    pub(in crate::world) fn compare_component_field(
        &mut self,
        components: &registry::ComponentStorage,
        id: EntityId,
        component: u16,
        expected: &FieldWrite,
    ) -> Result<(), ErrorReason> {
        ComponentValue::field_count(component).map_err(|_| ErrorReason::UnknownComponent)?;
        self.stage_component(components, id, component);

        let value = self
            .entities_state
            .entities
            .get(&id)
            .and_then(|record| record.components.get(&component))
            .and_then(|state| state.staged.as_deref())
            .ok_or(ErrorReason::MissingComponent)?;
        if registry::field_matches(value, expected)? {
            Ok(())
        } else {
            Err(ErrorReason::ValueMismatch)
        }
    }

    /// Write several fields of an existing component in place, keeping its
    /// incarnation. The writes are validated together on a candidate first, so
    /// an invalid write leaves the component unchanged.
    pub(in crate::world) fn write_component_fields(
        &mut self,
        components: &registry::ComponentStorage,
        id: EntityId,
        component: u16,
        fields: &[FieldWrite],
    ) -> Result<(), ErrorReason> {
        let mut candidate = self.component(components, id, component)?;
        for field in fields {
            registry::write_field(&mut candidate, field)?;
        }
        candidate.validate_lifecycle()?;

        self.replace_staged(id, candidate, None);
        for field in fields {
            self.explicit_fields.insert((id, component, field.offset));
        }
        Ok(())
    }

    /// Apply one dynamic-property operation to a present component's staged
    /// copy. A component that validates after every operation checks a
    /// candidate first, so a rejected operation has no effect. Any other
    /// component changes its staged copy in place: `edit` either succeeds or
    /// leaves the properties unchanged, and a property write touches no other
    /// field, so a batch copies the component once however many properties it
    /// writes.
    fn stage_dynamic_property<T>(
        &mut self,
        components: &registry::ComponentStorage,
        id: EntityId,
        component: u16,
        edit: impl FnOnce(&mut crate::DynamicProperties) -> Result<T, ErrorReason>,
    ) -> Result<T, ErrorReason> {
        ComponentValue::field_count(component).map_err(|_| ErrorReason::UnknownComponent)?;
        self.stage_component(components, id, component);
        let staged = self
            .entities_state
            .entities
            .get_mut(&id)
            .and_then(|record| record.components.get_mut(&component))
            .and_then(|state| state.staged.as_deref_mut())
            .ok_or(ErrorReason::MissingComponent)?;
        if staged.validates_after_operation() {
            let mut candidate = staged.clone();
            let result = edit(
                candidate
                    .dynamic_properties_mut()
                    .ok_or(ErrorReason::InvalidField)?,
            )?;
            candidate.validate_lifecycle()?;
            *staged = candidate;
            return Ok(result);
        }
        edit(
            staged
                .dynamic_properties_mut()
                .ok_or(ErrorReason::InvalidField)?,
        )
    }

    /// Replace the staged copy of a present component with a validated
    /// candidate of the same incarnation. `write` reports the candidate's
    /// complete difference as one in-place write; without it the observation
    /// compares whole values.
    fn replace_staged(
        &mut self,
        id: EntityId,
        value: ComponentValue,
        write: Option<(ComponentStagedWrite, bool)>,
    ) {
        let component = value.type_id();
        match write {
            Some((write, changed)) => self.touch_component_write(id, component, write, changed),
            None => self.touch_component(id, component),
        }
        self.entities_state
            .entities
            .get_mut(&id)
            .and_then(|record| record.components.get_mut(&component))
            .expect("present component")
            .staged = Some(Box::new(value));
    }

    pub(in crate::world) fn insert_component_value(
        &mut self,
        _components: &registry::ComponentStorage,
        id: EntityId,
        value: ComponentValue,
    ) -> Result<(), ErrorReason> {
        registry::validate_asset_references(&value)?;
        let component = value.type_id();
        self.touch_component(id, component);
        let incarnation = self.incarnation()?;
        self.entities_state
            .entities
            .get_mut(&id)
            .expect("resolved entity")
            .components
            .insert(
                component,
                WorldComponentState {
                    instance: ComponentStateInstance {
                        incarnation,
                    },
                    staged: Some(Box::new(value)),
                },
            );
        Ok(())
    }

    /// Insert, with its defaults, every missing required component of the
    /// components present on the entities this operation touched. Required
    /// components are ordinary components: they stay when their dependent is
    /// removed, and a client may write or remove them like any other.
    pub(in crate::world) fn insert_required_components(
        &mut self,
        components: &registry::ComponentStorage,
    ) -> Result<(), ErrorReason> {
        let entities: BTreeSet<_> = self
            .operation_components
            .iter()
            .map(|&(entity, _)| entity)
            .collect();
        for entity in entities {
            let Some(record) = self.entities_state.entities.get(&entity) else {
                continue;
            };
            let mut pending: Vec<_> = record
                .components
                .keys()
                .flat_map(|&component| ComponentValue::required_components(component))
                .copied()
                .collect();
            let mut visited = BTreeSet::new();
            let mut missing = Vec::new();
            while let Some(required) = pending.pop() {
                if !visited.insert(required) {
                    continue;
                }
                pending.extend_from_slice(ComponentValue::required_components(required));
                if record.input(required).is_none() {
                    missing.push(required);
                }
            }
            missing.sort_unstable();
            for required in missing {
                self.insert_component_value(components, entity, registry::create(required)?)?;
            }
        }
        Ok(())
    }
}
