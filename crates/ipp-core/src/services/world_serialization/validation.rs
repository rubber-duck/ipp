//! Structural validation independent of runtime allocation or asset readiness.
//! Typed resource reference declarations are checked without acquiring their contents.

use super::*;
use crate::services::asset_management::{AssetSource, AssetTypeId};
use crate::{ComponentValue, WorldAttachment, components::schema::FieldValue};
use std::collections::{BTreeMap, BTreeSet};

impl WorldGraphSnapshot {
    pub(crate) fn validate_budget(&self, limits: WorldPersistenceLimits) -> Result<(), String> {
        let mut bytes = 0usize;
        let mut claim = |size: usize| -> Result<(), String> {
            bytes = bytes.checked_add(size).ok_or("Snapshot size overflow")?;
            if bytes > limits.max_bytes {
                return Err("Snapshot byte budget exhausted".into());
            }
            Ok(())
        };
        for node in &self.nodes {
            claim(512)?;
            claim(node.world.metadata.symbolic_id.len())?;
            for system in &node.world.selected_systems {
                claim(64)?;
                claim(system.len())?;
            }
            for (system, hints) in &node.world.capacity_hints.systems {
                claim(128)?;
                claim(system.len())?;
                for key in hints.0.keys() {
                    claim(128)?;
                    claim(key.len())?;
                }
            }
            for entity in &node.world.entities {
                claim(128)?;
                claim(entity.metadata.symbolic_id.as_ref().map_or(0, String::len))?;
                for class in &entity.metadata.classes {
                    claim(64)?;
                    claim(class.len())?;
                }
                for component in &entity.components {
                    claim(512)?;
                    claim(
                        component
                            .retained_bytes()
                            .ok_or("Component size overflow")?,
                    )?;
                }
            }
            claim(
                node.references
                    .len()
                    .checked_mul(128)
                    .ok_or("Reference size overflow")?,
            )?;
            for (system, state) in &node.world.systems {
                claim(128)?;
                claim(system.len())?;
                claim(state.len())?;
            }
        }
        Ok(())
    }

    pub(crate) fn validate_references(&self) -> Result<(), String> {
        let mut nodes = BTreeMap::new();
        let mut names = BTreeSet::new();
        for node in &self.nodes {
            if nodes.insert(node.id, node).is_some() {
                return Err("Duplicate graph node identity".into());
            }
            crate::world::validate_world_symbolic_id(&node.world.metadata.symbolic_id)?;
            if node.world.metadata.persistent_id.0 == 0
                || !names.insert(node.world.metadata.symbolic_id.as_str())
            {
                return Err("Invalid graph World metadata".into());
            }
            node.world.validate_references()?;
        }
        if !nodes.contains_key(&self.root) {
            return Err("Missing graph root".into());
        }

        let entity_sets: BTreeMap<_, BTreeSet<_>> = self
            .nodes
            .iter()
            .map(|node| {
                (
                    node.id,
                    node.world
                        .entities
                        .iter()
                        .map(|entity| entity.persistent_id)
                        .collect(),
                )
            })
            .collect();
        let mut incoming = BTreeMap::new();
        let mut children: BTreeMap<WorldGraphNodeId, Vec<WorldGraphNodeId>> = BTreeMap::new();
        for node in &self.nodes {
            let entities: BTreeMap<_, _> = node
                .world
                .entities
                .iter()
                .map(|entity| (entity.persistent_id, entity))
                .collect();
            if entities.len() != node.world.entities.len()
                || entities
                    .keys()
                    .any(|id| id.0 == 0 || id.0 > node.world.next_entity_id)
            {
                return Err("Invalid or duplicate persistent entity identity".into());
            }
            let mut references = BTreeMap::new();
            let mut referenced_components = BTreeSet::new();
            for reference in &node.references {
                let key = (reference.entity, reference.component, reference.field);
                if references.insert(key, &reference.value).is_some() {
                    return Err("Duplicate graph field reference".into());
                }
                referenced_components.insert((reference.entity, reference.component));
                let component = entities
                    .get(&reference.entity)
                    .and_then(|entity| {
                        entity
                            .components
                            .iter()
                            .find(|value| value.type_id() == reference.component)
                    })
                    .ok_or("Graph reference has no producer component")?;
                let placeholder = component
                    .fields()
                    .into_iter()
                    .find(|(field, _)| *field == reference.field)
                    .map(|(_, value)| value)
                    .ok_or("Graph reference has no field")?;
                match (&reference.value, placeholder) {
                    (WorldSerializedReferenceValue::World(target), FieldValue::World(None)) => {
                        if !nodes.contains_key(target) {
                            return Err("Missing graph World reference".into());
                        }
                    }
                    (WorldSerializedReferenceValue::Output(output), FieldValue::Output(None)) => {
                        let target = entity_sets
                            .get(&output.world)
                            .ok_or("Missing output graph node")?;
                        match (output.kind, output.entity) {
                            (crate::OutputKind::Canvas, None) => {}
                            (crate::OutputKind::Camera, Some(entity))
                                if target.contains(&entity) => {}
                            (crate::OutputKind::Camera, Some(_)) => {
                                return Err("Missing persistent output entity".into());
                            }
                            _ => return Err("Invalid persistent output".into()),
                        }
                    }
                    _ => return Err("Graph reference does not match its typed placeholder".into()),
                }
            }

            for entity in &node.world.entities {
                if entity
                    .components
                    .iter()
                    .filter(|component| crate::systems::surface::is_provider(component.type_id()))
                    .count()
                    > 1
                {
                    return Err("Multiple serialized Surface providers".into());
                }
                let mut types = BTreeSet::new();
                for component in &entity.components {
                    if !types.insert(component.type_id()) {
                        return Err("Duplicate serialized component".into());
                    }
                    if component.fields().iter().any(|(_, value)| {
                        matches!(
                            value,
                            FieldValue::World(Some(_)) | FieldValue::Output(Some(_))
                        )
                    }) {
                        return Err("Runtime reference in durable component".into());
                    }
                    let has_references = referenced_components
                        .contains(&(entity.persistent_id, component.type_id()));
                    if !has_references {
                        component
                            .validate_lifecycle()
                            .map_err(|error| error.to_string())?;
                    }
                    if let ComponentValue::WorldAttachment(value) = component {
                        if value.mode != 0
                            && !entity.components.iter().any(|component| {
                                crate::systems::surface::is_provider(component.type_id())
                            })
                        {
                            return Err("Surface attachment has no authored Surface".into());
                        }
                        let child = references.get(&(
                            entity.persistent_id,
                            component.type_id(),
                            std::mem::offset_of!(WorldAttachment, child) as u32,
                        ));
                        let output = references.get(&(
                            entity.persistent_id,
                            component.type_id(),
                            std::mem::offset_of!(WorldAttachment, output) as u32,
                        ));
                        let child = match child {
                            Some(WorldSerializedReferenceValue::World(child)) => Some(*child),
                            None => None,
                            _ => return Err("Invalid attachment child reference".into()),
                        };
                        match (value.mode, child, output) {
                            (0, _, None) | (1, Some(_), None) => {}
                            (
                                2,
                                Some(child),
                                Some(WorldSerializedReferenceValue::Output(output)),
                            ) if child == output.world
                                && output.kind == crate::OutputKind::Camera => {}
                            _ => return Err("Invalid serialized attachment output".into()),
                        }
                        if let Some(child) = child {
                            if child == self.root || incoming.insert(child, node.id).is_some() {
                                return Err(
                                    "Competing attachment parents or attached graph root".into()
                                );
                            }
                            children.entry(node.id).or_default().push(child);
                        }
                    }
                }
            }
        }

        let mut visited = BTreeSet::new();
        let mut pending = vec![self.root];
        while let Some(node) = pending.pop() {
            if !visited.insert(node) {
                return Err("Cyclic attachment graph".into());
            }
            if let Some(children) = children.get(&node) {
                pending.extend(children);
            }
        }
        if visited.len() != nodes.len() {
            return Err("Disconnected or cyclic attachment graph".into());
        }
        Ok(())
    }
}

impl WorldSnapshot {
    pub(super) fn validate_references(&self) -> Result<(), String> {
        let mut selected = BTreeSet::new();
        for system in &self.selected_systems {
            if system.is_empty() || !selected.insert(system.as_str()) {
                return Err("Invalid or duplicate selected System identity".into());
            }
        }
        if self
            .systems
            .keys()
            .any(|system| !selected.contains(system.as_str()))
        {
            return Err("Persistent System state has no selected System".into());
        }
        let ids: BTreeSet<_> = self
            .entities
            .iter()
            .map(|entity| entity.persistent_id.0)
            .collect();
        let parents: BTreeMap<_, _> = self
            .entities
            .iter()
            .map(|entity| (entity.persistent_id, entity.link.parent))
            .collect();
        let mut completed = BTreeSet::new();
        for entity in &self.entities {
            let mut path = BTreeSet::new();
            let mut current = Some(entity.persistent_id);
            while let Some(id) = current {
                if completed.contains(&id) {
                    break;
                }
                if !path.insert(id) {
                    return Err("Cyclic persistent entity links".into());
                }
                current = *parents
                    .get(&id)
                    .ok_or("Missing persistent parent reference")?;
            }
            completed.extend(path);
        }
        for entity in &self.entities {
            if entity
                .link
                .parent
                .is_some_and(|parent| !ids.contains(&parent.0))
            {
                return Err("Missing persistent parent reference".into());
            }
            crate::EntityOrder::from_value(entity.link.order.value())
                .map_err(|error| error.to_string())?;
            for component in &entity.components {
                for (offset, field) in component.fields() {
                    if let FieldValue::Entity(id) = field
                        && !ids.contains(&id.to_bits())
                        && !(id.to_bits() == 0
                            && ComponentValue::accepts_null_entity(component.type_id(), offset))
                    {
                        return Err("Missing persistent component entity reference".into());
                    }
                }
                component_sources(component)?;
            }
        }
        Ok(())
    }
}

fn component_sources(component: &ComponentValue) -> Result<BTreeSet<AssetSource>, String> {
    let fields: BTreeMap<_, _> = component.fields().into_iter().collect();
    let mut sources = BTreeSet::new();
    for reference in ComponentValue::asset_references(component.type_id()) {
        let Some(FieldValue::String(source)) = fields.get(&reference.source_offset) else {
            return Err("Invalid typed asset source declaration".into());
        };
        let Some(FieldValue::U32(variant)) = fields.get(&reference.variant_offset) else {
            return Err("Invalid typed asset variant declaration".into());
        };
        if !source.is_empty() {
            sources.insert(AssetSource {
                kind: AssetTypeId(reference.kind),
                uri: source.clone(),
                variant: *variant,
            });
        }
    }
    for value in fields.values() {
        if let FieldValue::Dynamic(crate::DynamicValue::Asset(asset)) = value
            && !asset.uri.is_empty()
        {
            sources.insert(AssetSource {
                kind: asset.kind,
                uri: asset.uri.clone(),
                variant: asset.variant,
            });
        }
    }
    component.visit_row_assets(&mut |asset| {
        if !asset.uri.is_empty() {
            sources.insert(asset.clone());
        }
    });
    {
        let mut demand = BTreeSet::new();
        component.resource_demand(&mut demand);
        let demand: BTreeSet<_> = demand
            .into_iter()
            .map(|selection| selection.descriptor())
            .collect();
        if sources != demand {
            return Err(format!(
                "Component {} has undeclared persistent asset references",
                component.type_id()
            ));
        }
    }
    Ok(sources)
}
