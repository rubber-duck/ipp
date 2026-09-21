//! World-owned declaration updates; errors retain partial changes.

use super::registry::{ComponentStateOverlay, EntityOverlayBinding, StateOverlayOwner};
use super::{StateOverlayBatch, StateOverlayEntry, StateOverlayFields};
use crate::world::{ComponentStateInstance, WorldEntityRecord, WorldLimits};
use crate::{
    Command, ComponentOverlayMode, EntityId, EntityMetadata, EntityOverlayMode, ErrorReason,
    FieldWrite, StateOverlayHandleKind, StateOverlayLifecycleDiagnostic,
    StateOverlayLifecycleReason, components::registry,
};
use std::collections::BTreeMap;

// Duplicate writes replace one entry. The generated component field count bounds
// retained storage without coupling state_overlays to whichever component is largest today.
fn write_fields(
    component: u16,
    prototype: &crate::ComponentValue,
    mut values: StateOverlayFields,
    fields: &[FieldWrite],
    clear: &[u32],
) -> Result<StateOverlayFields, ErrorReason> {
    let field_count = prototype.fields().len();
    values.retain(|field| {
        !crate::components::dynamic_properties::is_dynamic_field(field.offset)
            || prototype
                .dynamic_properties()
                .is_some_and(|p| p.get_key(field.offset).is_some())
    });
    for &offset in clear {
        if !crate::ComponentValue::has_field(component, offset) {
            return Err(ErrorReason::InvalidField);
        }
        values.retain(|field| field.offset != offset);
    }

    for field in fields {
        registry::write(&mut prototype.clone(), field)?;
        if let Some(value) = values.iter_mut().find(|value| value.offset == field.offset) {
            *value = field.clone();
        } else {
            if values.len() >= field_count {
                return Err(ErrorReason::Capacity);
            }
            values.push(field.clone());
        }
    }
    Ok(values)
}

impl super::StateOverlayMutationAccess<'_> {
    fn owner(&self, id: u64, live: bool) -> Result<(), ErrorReason> {
        match self.state_overlays.registry.get(id)? {
            Some(StateOverlayEntry::Owner(StateOverlayOwner)) => Ok(()),
            None if !live => Ok(()),
            _ => Err(ErrorReason::InvalidStateOverlay),
        }
    }

    fn scoped(&self, owner: u64, resource: u64) -> Result<Option<StateOverlayEntry>, ErrorReason> {
        let value = self.state_overlays.registry.get(resource)?;
        if let Some(value) = &value {
            if value.owner().is_none() {
                return Err(ErrorReason::InvalidStateOverlay);
            }
            if value.owner() != Some(owner) {
                return Err(ErrorReason::StateOverlayOwnershipMismatch);
            }
        }

        Ok(value)
    }

    pub(in crate::world) fn apply_state_overlay(
        &mut self,
        command: &Command,
        aliases: &BTreeMap<u32, EntityId>,
        batch: &mut StateOverlayBatch,
        _limits: WorldLimits,
    ) -> Result<(), ErrorReason> {
        match command {
            Command::CreateStateOverlayOwner {
                alias,
            } => {
                batch.vacant(*alias)?;
                let id = self
                    .state_overlays
                    .registry
                    .insert(StateOverlayEntry::Owner(StateOverlayOwner))?;
                batch.add(*alias, id, StateOverlayHandleKind::Owner, None);
            }
            Command::ReleaseStateOverlayOwner {
                owner,
            } => {
                let owner = batch.resolve(*owner)?;
                self.owner(owner, false)?;
                if self.state_overlays.registry.get(owner)?.is_some() {
                    let bindings: Vec<_> = self
                        .state_overlays
                        .registry
                        .iter()
                        .filter_map(|(id, resource)| match resource {
                            StateOverlayEntry::EntityBinding(EntityOverlayBinding {
                                owner: scope,
                                ..
                            }) if *scope == owner => Some(id),
                            _ => None,
                        })
                        .collect();
                    for binding in bindings {
                        self.release_entity(owner, binding)?;
                    }

                    self.state_overlays.registry.release(owner);
                }
            }
            Command::AttachEntityOverlayBinding {
                owner,
                alias,
                symbolic_id,
                mode,
            } => {
                let owner = batch.resolve(*owner)?;
                self.owner(owner, true)?;
                batch.vacant(*alias)?;

                let entity = match mode {
                    EntityOverlayMode::Bound => *self
                        .staged
                        .entities_state
                        .symbols
                        .get(symbolic_id)
                        .ok_or(ErrorReason::MissingSymbolicId)?,
                    EntityOverlayMode::Owned => {
                        if self.staged.entities_state.symbols.contains_key(symbolic_id) {
                            return Err(ErrorReason::DuplicateSymbolicId);
                        }
                        let entity = self
                            .staged
                            .entities_state
                            .allocator
                            .allocate()
                            .ok_or(ErrorReason::Capacity)?;
                        let persistent_id = self.staged.entities_state.allocate_persistent_id()?;
                        self.staged.entities_state.entities.insert(
                            entity,
                            WorldEntityRecord {
                                persistent_id,
                                ..WorldEntityRecord::default()
                            },
                        );
                        #[cfg(feature = "diagnostics")]
                        self.record_entity_effect("entity.create", entity);
                        self.staged.lifecycle_effects.push(crate::systems::lifecycle_publisher::LifecycleObservation::Entity { entity, kind: crate::systems::lifecycle_publisher::EntityLifecycleKind::Created });
                        self.staged.operation_created.insert(entity);
                        let observation_count = self.staged.lifecycle_effects.len();
                        self.set_metadata(
                            entity,
                            EntityMetadata {
                                symbolic_id: Some(symbolic_id.clone()),
                                classes: Vec::new(),
                            },
                        )?;
                        self.staged.lifecycle_effects.truncate(observation_count);
                        entity
                    }
                };

                let id = self
                    .state_overlays
                    .registry
                    .insert(StateOverlayEntry::EntityBinding(EntityOverlayBinding {
                        owner,
                        entity,
                        mode: *mode,
                        active: true,
                    }))?;
                batch.add(
                    *alias,
                    id,
                    StateOverlayHandleKind::EntityOverlayBinding,
                    Some(entity),
                );
            }
            Command::ReleaseEntityOverlayBinding {
                owner,
                binding,
            } => {
                let owner = batch.resolve(*owner)?;
                self.owner(owner, false)?;
                self.release_entity(owner, batch.resolve(*binding)?)?;
            }
            Command::AttachComponentStateOverlay {
                owner,
                binding,
                alias,
                component,
                mode,
                fields,
            } => {
                let owner = batch.resolve(*owner)?;
                self.owner(owner, true)?;
                let binding = batch.resolve(*binding)?;
                let Some(StateOverlayEntry::EntityBinding(EntityOverlayBinding {
                    entity,
                    active: true,
                    ..
                })) = self.scoped(owner, binding)?
                else {
                    return Err(ErrorReason::InvalidStateOverlay);
                };
                batch.vacant(*alias)?;
                #[cfg(feature = "gui")]
                crate::systems::gui::validate_overlay_declaration(
                    &self.staged.entities_state.entities[&entity],
                    *component,
                    *mode != ComponentOverlayMode::Bound,
                    fields.iter().map(|field| field.offset),
                )?;
                self.touch_component(entity, *component);
                let fields = fields
                    .iter()
                    .cloned()
                    .map(|field| self.field(*component, field, aliases))
                    .collect::<Result<Vec<_>, _>>()?;
                crate::ComponentValue::field_count(*component)
                    .map_err(|_| ErrorReason::UnknownComponent)?;
                let record = &self.staged.entities_state.entities[&entity];
                let incarnation = match mode {
                    ComponentOverlayMode::Bound => {
                        record
                            .input(*component)
                            .ok_or(ErrorReason::MissingComponent)?
                            .incarnation
                    }
                    ComponentOverlayMode::Owned => {
                        if record
                            .layers
                            .get(component)
                            .is_some_and(|l| l.inputs.base.is_some())
                            || record.input(*component).is_some()
                        {
                            return Err(ErrorReason::ComponentExists);
                        }
                        let incarnation = self.incarnation()?;
                        self.staged
                            .entities_state
                            .entities
                            .get_mut(&entity)
                            .unwrap()
                            .layers
                            .entry(*component)
                            .or_default()
                            .inputs
                            .replace_base(Some(ComponentStateInstance {
                                base: registry::create(*component)?,
                                incarnation,
                            }));
                        incarnation
                    }
                    ComponentOverlayMode::Auto => {
                        record.input(*component).map_or(0, |v| v.incarnation)
                    }
                };

                let prototype = self.staged.entities_state.entities[&entity]
                    .layers
                    .get(component)
                    .and_then(|layer| {
                        layer
                            .inputs
                            .input_value()
                            .or_else(|| layer.inputs.base_value())
                    })
                    .cloned()
                    .map(Ok)
                    .unwrap_or_else(|| registry::create(*component))?;
                let fields = write_fields(*component, &prototype, Vec::new(), &fields, &[])?;
                for field in &fields {
                    self.staged
                        .explicit_fields
                        .insert((entity, *component, field.offset));
                }

                let order = self.state_overlays.registry.order()?;
                let id = self
                    .state_overlays
                    .registry
                    .insert(StateOverlayEntry::Component(ComponentStateOverlay {
                        owner,
                        binding,
                        entity,
                        mode: *mode,
                        incarnation,
                        order,
                        component: *component,
                        fields,
                        active: true,
                    }))?;
                self.staged
                    .entities_state
                    .entities
                    .get_mut(&entity)
                    .unwrap()
                    .layers
                    .entry(*component)
                    .or_default()
                    .inputs
                    .overlay_handles
                    .insert(id);
                if *mode == ComponentOverlayMode::Owned {
                    self.staged
                        .entities_state
                        .entities
                        .get_mut(&entity)
                        .unwrap()
                        .layers
                        .get_mut(component)
                        .unwrap()
                        .inputs
                        .creating_overlay = Some(id);
                }

                batch.add(
                    *alias,
                    id,
                    StateOverlayHandleKind::ComponentStateOverlay,
                    Some(entity),
                );
            }
            Command::UpdateDynamicComponentStateOverlay {
                owner,
                overlay,
                properties,
                clear,
            } => {
                let owner_id = batch.resolve(*owner)?;
                self.owner(owner_id, true)?;
                let id = batch.resolve(*overlay)?;
                let Some(StateOverlayEntry::Component(declaration)) = self.scoped(owner_id, id)?
                else {
                    return Err(ErrorReason::InvalidStateOverlay);
                };
                if !declaration.active {
                    return Err(ErrorReason::InvalidStateOverlay);
                }
                self.touch_component(declaration.entity, declaration.component);
                let layer = self
                    .staged
                    .entities_state
                    .entities
                    .get_mut(&declaration.entity)
                    .and_then(|e| e.layers.get_mut(&declaration.component))
                    .ok_or(ErrorReason::MissingComponent)?;
                let owns_properties = layer.inputs.creating_overlay == Some(id)
                    || (declaration.mode == ComponentOverlayMode::Auto
                        && layer.inputs.base.is_none());
                let mut value = if owns_properties {
                    layer
                        .inputs
                        .base_value()
                        .or(layer.inputs.fallback_value.as_deref())
                } else {
                    layer.inputs.input_value()
                }
                .ok_or(ErrorReason::MissingComponent)?
                .clone();
                #[cfg(feature = "surfaces")]
                let removed_surface_properties = if owns_properties
                    && declaration.component == crate::ComponentValue::SURFACE
                {
                    let mut declared = value.clone();
                    for field in &declaration.fields {
                        if !crate::components::dynamic_properties::is_dynamic_field(field.offset) {
                            registry::write(&mut declared, field)?;
                        }
                    }
                    if let crate::ComponentValue::Surface(surface) = &declared {
                        clear
                            .iter()
                            .filter(|name| surface.is_removed_item_property(name))
                            .cloned()
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                #[cfg(feature = "gui")]
                let removed_gui_properties = if owns_properties
                    && declaration.component == crate::ComponentValue::GUI_ROOT
                {
                    let mut declared = value.clone();
                    for field in &declaration.fields {
                        if !crate::components::dynamic_properties::is_dynamic_field(field.offset) {
                            registry::write(&mut declared, field)?;
                        }
                    }
                    if let crate::ComponentValue::GuiRoot(gui_root) = &declared {
                        clear
                            .iter()
                            .filter(|name| gui_root.is_removed_node_property(name))
                            .cloned()
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                let descriptors = value
                    .dynamic_properties_mut()
                    .ok_or(ErrorReason::InvalidField)?;
                let mut fields = Vec::new();
                for (name, authored) in properties {
                    if descriptors
                        .get(name)
                        .is_none_or(|value| value.kind() != authored.kind())
                    {
                        if !owns_properties {
                            return Err(ErrorReason::InvalidField);
                        }
                        descriptors
                            .set(name, authored.clone())
                            .map_err(|_| ErrorReason::InvalidValue)?;
                    }
                    fields.push(FieldWrite {
                        offset: descriptors.key(name).ok_or(ErrorReason::InvalidField)?,
                        value: crate::FieldValue::Dynamic(authored.clone()),
                    });
                }
                // Resolve overlay field identities before an owned declaration removes
                // their authoritative descriptors. Bound overlays only withdraw sparse
                // overrides and leave the producer's properties intact.
                let clear_fields = clear
                    .iter()
                    .filter_map(|name| descriptors.key(name))
                    .collect();
                #[cfg(feature = "surfaces")]
                for name in &removed_surface_properties {
                    descriptors.remove(name);
                }
                #[cfg(feature = "gui")]
                for name in &removed_gui_properties {
                    descriptors.remove(name);
                }
                if owns_properties {
                    let properties = descriptors.clone();
                    for input in [
                        &mut layer.inputs.base_value,
                        &mut layer.inputs.fallback_value,
                        &mut layer.inputs.resolved_value,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        *input
                            .dynamic_properties_mut()
                            .ok_or(ErrorReason::InvalidField)? = properties.clone();
                    }
                }
                self.apply_state_overlay(
                    &Command::UpdateComponentStateOverlay {
                        owner: *owner,
                        overlay: *overlay,
                        fields,
                        clear: clear_fields,
                    },
                    aliases,
                    batch,
                    _limits,
                )?;
            }
            Command::UpdateComponentStateOverlay {
                owner,
                overlay,
                fields,
                clear,
            } => {
                let owner = batch.resolve(*owner)?;
                self.owner(owner, true)?;
                let id = batch.resolve(*overlay)?;
                let Some(
                    mut resource @ StateOverlayEntry::Component(ComponentStateOverlay {
                        active: true,
                        ..
                    }),
                ) = self.scoped(owner, id)?
                else {
                    return Err(ErrorReason::InvalidStateOverlay);
                };

                if let StateOverlayEntry::Component(ComponentStateOverlay {
                    component,
                    entity,
                    fields: values,
                    ..
                }) = &mut resource
                {
                    #[cfg(feature = "gui")]
                    crate::systems::gui::validate_overlay_declaration(
                        &self.staged.entities_state.entities[entity],
                        *component,
                        false,
                        fields.iter().map(|field| field.offset),
                    )?;
                    self.touch_component(*entity, *component);
                    for offset in fields
                        .iter()
                        .map(|field| field.offset)
                        .chain(clear.iter().copied())
                    {
                        self.staged
                            .explicit_fields
                            .insert((*entity, *component, offset));
                    }
                    let fields = fields
                        .iter()
                        .cloned()
                        .map(|field| self.field(*component, field, aliases))
                        .collect::<Result<Vec<_>, _>>()?;
                    let prototype = self.staged.entities_state.entities[entity]
                        .layers
                        .get(component)
                        .and_then(|layer| layer.inputs.input_value())
                        .ok_or(ErrorReason::MissingComponent)?;
                    *values = write_fields(*component, prototype, values.clone(), &fields, clear)?;
                }
                self.state_overlays.registry.set(id, resource);
            }
            Command::ReleaseComponentStateOverlay {
                owner,
                overlay,
            } => {
                let owner = batch.resolve(*owner)?;
                self.owner(owner, false)?;
                self.release_component_state_overlay(owner, batch.resolve(*overlay)?)?;
            }
            _ => unreachable!("ordinary commands are handled by WorldMutationState::apply"),
        }

        Ok(())
    }

    fn release_component_state_overlay(&mut self, owner: u64, id: u64) -> Result<(), ErrorReason> {
        let Some(resource) = self.scoped(owner, id)? else {
            return Ok(());
        };
        let StateOverlayEntry::Component(ComponentStateOverlay {
            entity,
            mode,
            component,
            incarnation,
            active,
            ..
        }) = resource
        else {
            return Err(ErrorReason::InvalidStateOverlay);
        };

        self.touch_component(entity, component);
        if let Some(layer) = self
            .staged
            .entities_state
            .entities
            .get_mut(&entity)
            .and_then(|record| record.layers.get_mut(&component))
        {
            layer.inputs.overlay_handles.remove(&id);
        }
        if active
            && mode == ComponentOverlayMode::Owned
            && let Some(record) = self.staged.entities_state.entities.get_mut(&entity)
            && let Some(layer) = record.layers.get_mut(&component)
            && layer.inputs.creating_overlay == Some(id)
            && layer
                .inputs
                .base
                .as_ref()
                .is_some_and(|v| v.incarnation == incarnation)
        {
            layer.inputs.base = None;
            layer.inputs.base_value = None;
            layer.inputs.creating_overlay = None;
        }

        self.state_overlays.registry.release(id);
        Ok(())
    }

    fn release_entity(&mut self, owner: u64, id: u64) -> Result<(), ErrorReason> {
        let Some(resource) = self.scoped(owner, id)? else {
            return Ok(());
        };
        let StateOverlayEntry::EntityBinding(EntityOverlayBinding {
            entity,
            mode,
            active,
            ..
        }) = resource
        else {
            return Err(ErrorReason::InvalidStateOverlay);
        };

        let state_overlays: Vec<_> = self
            .state_overlays
            .registry
            .iter()
            .filter_map(|(handle, resource)| {
                matches!(resource, StateOverlayEntry::Component(ComponentStateOverlay { binding, .. }) if *binding == id)
                    .then_some(handle)
            })
            .collect();

        for overlay in state_overlays {
            self.release_component_state_overlay(owner, overlay)?;
        }

        self.state_overlays.registry.release(id);
        if active
            && mode == EntityOverlayMode::Owned
            && self.staged.entities_state.allocator.contains(entity)
        {
            self.invalidate_overlay_bindings(entity);
            self.staged.delete_entity(entity);
        }

        Ok(())
    }

    pub(in crate::world) fn resolve_layers(
        &mut self,
        components: &registry::ComponentStorage,
    ) -> Result<(), ErrorReason> {
        self.resolve_requirements(components);
        let keys: Vec<_> = self.staged.entities_state.dirty.iter().copied().collect();
        for (entity, component) in keys {
            let Some(layer) = self
                .staged
                .entities_state
                .entities
                .get(&entity)
                .and_then(|record| record.layers.get(&component))
            else {
                continue;
            };
            let handles: Vec<_> = layer.inputs.overlay_handles.iter().copied().collect();
            let auto = handles.iter().any(|id| {
                matches!(
                    self.state_overlays.registry.borrow(*id),
                    Some(StateOverlayEntry::Component(ComponentStateOverlay {
                        mode: ComponentOverlayMode::Auto,
                        active: true,
                        ..
                    }))
                )
            });
            let (fallback, fallback_value) =
                if layer.inputs.base.is_none() && (auto || layer.inputs.required) {
                    match layer.inputs.fallback {
                        Some(value) => (Some(value), layer.inputs.fallback_value.clone()),
                        None => {
                            let value = registry::create(component)?;
                            let incarnation = self.incarnation()?;
                            (
                                Some(ComponentStateInstance {
                                    base: (),
                                    incarnation,
                                }),
                                Some(Box::new(value)),
                            )
                        }
                    }
                } else {
                    (None, None)
                };
            let layer = self
                .staged
                .entities_state
                .entities
                .get_mut(&entity)
                .unwrap()
                .layers
                .get_mut(&component)
                .unwrap();
            layer.inputs.fallback = fallback;
            layer.inputs.fallback_value = fallback_value;
            layer.inputs.resolved = layer.inputs.base.or(layer.inputs.fallback);
            layer.inputs.resolved_value = layer
                .inputs
                .base_value
                .clone()
                .or_else(|| layer.inputs.fallback_value.clone());
            layer.inputs.hidden_fields.clear();
            let current = layer.input().map(|value| value.incarnation);

            for &id in &handles {
                let Some(StateOverlayEntry::Component(ComponentStateOverlay {
                    mode,
                    incarnation,
                    active: true,
                    ..
                })) = self.state_overlays.registry.borrow(id)
                else {
                    continue;
                };
                if *mode == ComponentOverlayMode::Auto && current != Some(*incarnation) {
                    let mut resource = self.state_overlays.registry.borrow(id).unwrap().clone();
                    if let StateOverlayEntry::Component(overlay) = &mut resource {
                        // Static fields follow Auto replacement. Dynamic identities belong
                        // to one incarnation and must never address a new local slot.
                        overlay.fields.retain(|field| {
                            !crate::components::dynamic_properties::is_dynamic_field(field.offset)
                        });
                        overlay.incarnation = current.unwrap_or(0);
                    }
                    self.state_overlays.registry.set(id, resource);
                } else if *mode != ComponentOverlayMode::Auto && current != Some(*incarnation) {
                    let mut resource = self.state_overlays.registry.borrow(id).unwrap().clone();
                    if let StateOverlayEntry::Component(ComponentStateOverlay {
                        active,
                        fields,
                        owner,
                        ..
                    }) = &mut resource
                    {
                        *active = false;
                        fields.clear();
                        self.state_overlays
                            .diagnostics
                            .push(StateOverlayLifecycleDiagnostic {
                                owner: *owner,
                                state_overlay: id,
                                entity,
                                component: Some(component),
                                reason: if current.is_some() {
                                    StateOverlayLifecycleReason::ComponentReplaced
                                } else {
                                    StateOverlayLifecycleReason::ComponentRemoved
                                },
                            });
                    }
                    self.state_overlays.registry.set(id, resource);
                }
            }
            let mut winners = BTreeMap::new();
            for id in handles {
                let Some(StateOverlayEntry::Component(ComponentStateOverlay {
                    order,
                    fields,
                    active: true,
                    ..
                })) = self.state_overlays.registry.borrow(id)
                else {
                    continue;
                };
                for field in fields {
                    if crate::components::dynamic_properties::is_dynamic_field(field.offset)
                        && !layer
                            .inputs
                            .resolved_value
                            .as_ref()
                            .and_then(|v| v.dynamic_properties())
                            .is_some_and(|p| p.get_key(field.offset).is_some())
                    {
                        continue;
                    }
                    let winner = winners.entry(field.offset).or_insert((*order, field));
                    if *order > winner.0 {
                        *winner = (*order, field);
                    }
                }
            }
            let layer = self
                .staged
                .entities_state
                .entities
                .get_mut(&entity)
                .unwrap()
                .layers
                .get_mut(&component)
                .unwrap();
            if let Some(value) = layer.inputs.resolved_value.as_ref() {
                layer.inputs.hidden_fields = value
                    .fields()
                    .into_iter()
                    .filter(|(offset, _)| winners.contains_key(offset))
                    .collect();
            }
            for (_, (_, field)) in winners {
                registry::write(
                    layer
                        .inputs
                        .resolved_value
                        .as_mut()
                        .expect("active declaration has effective input"),
                    field,
                )?;
            }
        }
        Ok(())
    }
}
