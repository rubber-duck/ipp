//! IPPW v7: one bounded authored graph with graph-local typed references.

use std::collections::{BTreeMap, BTreeSet};

use super::{binary::*, *};
use crate::{
    ComponentValue, EntityId, EntityPersistentId, WorldPersistentId, WorldSystemCapacityHints,
    components::schema::{ContractHash, ContractSink, FieldValue},
};

pub(crate) fn checksum(bytes: &[u8]) -> u64 {
    let mut hash = ContractHash::default();
    hash.write(bytes);
    hash.0
}

impl WorldGraphSnapshot {
    /// Encode bounded owned state without opening or rewriting resource references.
    pub fn encode(&self, contract: u64, limits: WorldPersistenceLimits) -> Result<Vec<u8>, String> {
        self.validate_budget(limits)?;
        self.validate_references()?;
        let mut writer = WorldBinaryWriter::new(limits.max_bytes);
        writer.raw(b"IPPW")?;
        writer.u32(7)?;
        writer.u64(contract)?;
        writer.u64(0)?;
        writer.u64(0)?;
        writer.u32(self.root.0)?;
        writer.count(self.nodes.len())?;
        for node in &self.nodes {
            writer.u32(node.id.0)?;
            encode_snapshot(&mut writer, &node.world)?;
            writer.count(node.references.len())?;
            for reference in &node.references {
                writer.u64(reference.entity.0)?;
                writer.u16(reference.component)?;
                writer.u32(reference.field)?;
                match &reference.value {
                    WorldSerializedReferenceValue::World(world) => {
                        writer.u8(0)?;
                        writer.u32(world.0)?;
                    }
                    WorldSerializedReferenceValue::Output(output) => {
                        writer.u8(1)?;
                        writer.u32(output.world.0)?;
                        match (output.kind, output.entity) {
                            (crate::OutputKind::Canvas, None) => writer.u8(0)?,
                            (crate::OutputKind::Camera, Some(entity)) => {
                                writer.u8(1)?;
                                writer.u64(entity.0)?;
                            }
                            _ => return Err("Invalid persistent output".into()),
                        }
                    }
                }
            }
        }

        let length = writer.bytes.len() as u64;
        let digest = checksum(&writer.bytes[32..]);
        writer.bytes[16..24].copy_from_slice(&length.to_le_bytes());
        writer.bytes[24..32].copy_from_slice(&digest.to_le_bytes());
        Ok(writer.bytes)
    }

    /// Validate a complete candidate before any World or service state is published.
    pub fn decode(
        bytes: &[u8],
        contract: u64,
        limits: WorldPersistenceLimits,
    ) -> Result<Self, String> {
        if bytes.len() > limits.max_bytes {
            return Err("World file byte budget exhausted".into());
        }
        let mut reader = WorldBinaryReader::new(bytes, limits.max_bytes);
        if reader.raw(4)? != b"IPPW" {
            return Err("Unsupported World container".into());
        }
        let version = reader.u32()?;
        if version != 7 {
            return Err("Unsupported World container".into());
        }
        if reader.u64()? != contract {
            return Err("World file target contract mismatch".into());
        }
        if reader.u64()? != bytes.len() as u64 || reader.u64()? != checksum(&bytes[32..]) {
            return Err("World file length or checksum mismatch".into());
        }
        let root = WorldGraphNodeId(reader.u32()?);
        let count = reader.count(8)?;
        reader.claim(count.saturating_mul(512))?;
        let mut nodes = Vec::new();
        for _ in 0..count {
            let id = WorldGraphNodeId(reader.u32()?);
            let world = decode_snapshot(&mut reader, limits.max_bytes)?;
            let count = reader.count(19)?;
            reader.claim(count.saturating_mul(128))?;
            let mut references = Vec::new();
            for _ in 0..count {
                let entity = EntityPersistentId(reader.u64()?);
                let component = reader.u16()?;
                let field = reader.u32()?;
                let value = match reader.u8()? {
                    0 => WorldSerializedReferenceValue::World(WorldGraphNodeId(reader.u32()?)),
                    1 => {
                        let world = WorldGraphNodeId(reader.u32()?);
                        let (kind, entity) = match reader.u8()? {
                            0 => (crate::OutputKind::Canvas, None),
                            1 => (
                                crate::OutputKind::Camera,
                                Some(EntityPersistentId(reader.u64()?)),
                            ),
                            _ => return Err("Invalid persistent output kind".into()),
                        };
                        WorldSerializedReferenceValue::Output(WorldSerializedOutput {
                            world,
                            kind,
                            entity,
                        })
                    }
                    _ => return Err("Invalid graph reference kind".into()),
                };
                references.push(WorldSerializedReference {
                    entity,
                    component,
                    field,
                    value,
                });
            }
            nodes.push(WorldGraphNode {
                id,
                world,
                references,
            });
        }
        let snapshot = Self {
            root,
            nodes,
        };
        reader.end()?;
        snapshot.validate_budget(limits)?;
        snapshot.validate_references()?;
        Ok(snapshot)
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
                super::assets::component_sources(component)?;
            }
        }
        Ok(())
    }
}

fn encode_snapshot(writer: &mut WorldBinaryWriter, snapshot: &WorldSnapshot) -> Result<(), String> {
    writer.string(&snapshot.metadata.symbolic_id)?;
    writer.raw(&snapshot.metadata.persistent_id.0.to_le_bytes())?;
    writer.u64(snapshot.next_entity_id)?;
    encode_hints(writer, &snapshot.capacity_hints)?;
    writer.count(snapshot.selected_systems.len())?;
    for system in &snapshot.selected_systems {
        if system.is_empty() {
            return Err("Missing selected System identity".into());
        }
        writer.string(system)?;
    }
    writer.count(snapshot.entities.len())?;
    for entity in &snapshot.entities {
        writer.u64(entity.persistent_id.0)?;
        writer.u64(entity.link.parent.map_or(0, |parent| parent.0))?;
        writer.raw(&entity.link.order.value().to_le_bytes())?;
        writer.u8(u8::from(entity.metadata.symbolic_id.is_some()))?;
        if let Some(symbol) = &entity.metadata.symbolic_id {
            writer.string(symbol)?;
        }
        writer.count(entity.metadata.classes.len())?;
        for class in &entity.metadata.classes {
            writer.string(class)?;
        }
        writer.count(entity.components.len())?;
        for component in &entity.components {
            writer.u16(component.type_id())?;
            let fields = component.fields();
            writer.count(fields.len())?;
            for (offset, field) in fields {
                writer.u32(offset)?;
                encode_field(writer, field)?;
            }
        }
    }
    writer.count(snapshot.systems.len())?;
    for (system, state) in &snapshot.systems {
        if system.is_empty() {
            return Err("Missing persistent System identity".into());
        }
        writer.string(system)?;
        writer.blob(state)?;
    }
    Ok(())
}

fn decode_snapshot(
    reader: &mut WorldBinaryReader<'_>,
    max_bytes: usize,
) -> Result<WorldSnapshot, String> {
    let symbolic_id = reader.string()?;
    crate::world::validate_world_symbolic_id(&symbolic_id)?;
    let persistent_id = WorldPersistentId(u128::from_le_bytes(
        reader.raw(16)?.try_into().expect("checked length"),
    ));
    if persistent_id.0 == 0 {
        return Err("Missing persistent World identity".into());
    }
    let next_entity_id = reader.u64()?;
    let capacity_hints = decode_hints(reader)?;
    let selected_count = reader.count(4)?;
    reader.claim(selected_count.saturating_mul(128))?;
    let mut selected_systems = Vec::new();
    let mut selected_set = BTreeSet::new();
    for _ in 0..selected_count {
        let system = reader.string()?;
        if system.is_empty() || !selected_set.insert(system.clone()) {
            return Err("Invalid or duplicate selected System identity".into());
        }
        selected_systems.push(system);
    }
    let count = reader.count(41)?;
    reader.claim(count.saturating_mul(128))?;
    let mut retained = 0usize;
    let mut entities = Vec::new();
    let mut identities = BTreeSet::new();
    let mut symbols = BTreeSet::new();
    for _ in 0..count {
        let persistent_id = EntityPersistentId(reader.u64()?);
        if persistent_id.0 == 0
            || persistent_id.0 > next_entity_id
            || !identities.insert(persistent_id)
        {
            return Err("Invalid or duplicate persistent entity identity".into());
        }
        let parent = match reader.u64()? {
            0 => None,
            identity => Some(EntityPersistentId(identity)),
        };
        let order = crate::EntityOrder::from_value(u128::from_le_bytes(
            reader.raw(16)?.try_into().expect("checked length"),
        ))
        .map_err(|error| error.to_string())?;
        let symbolic_id = match reader.u8()? {
            0 => None,
            1 => Some(reader.string()?),
            _ => return Err("Invalid optional symbolic ID".into()),
        };
        if let Some(symbol) = &symbolic_id
            && !symbols.insert(symbol.clone())
        {
            return Err("Duplicate entity symbolic ID".into());
        }
        let count = reader.count(4)?;
        let mut classes = Vec::new();
        for _ in 0..count {
            classes.push(reader.string()?);
        }
        if classes.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("Unnormalized entity classes".into());
        }
        let count = reader.count(6)?;
        reader.claim(count.saturating_mul(512))?;
        let mut components = Vec::new();
        let mut types = BTreeSet::new();
        for _ in 0..count {
            let kind = reader.u16()?;
            if !types.insert(kind) {
                return Err("Duplicate serialized component".into());
            }
            let mut component = ComponentValue::create(kind)
                .map_err(|error| format!("Unsupported persistent component {kind}: {error:?}"))?;
            let count = reader.count(5)?;
            let mut fields = BTreeSet::new();
            for _ in 0..count {
                let offset = reader.u32()?;
                if !fields.insert(offset) {
                    return Err("Duplicate serialized field".into());
                }
                component
                    .set_field(offset, decode_field(reader)?)
                    .map_err(|error| format!("Invalid persistent field: {error:?}"))?;
            }
            if fields
                != component
                    .fields()
                    .into_iter()
                    .map(|(offset, _)| offset)
                    .collect()
            {
                return Err("Incomplete serialized component".into());
            }
            components.push(component);
        }
        let metadata_bytes = symbolic_id.as_ref().map_or(0, String::len)
            + classes.iter().map(|value| value.len() + 64).sum::<usize>();
        retained = retained
            .checked_add(128 + metadata_bytes)
            .ok_or("World decode size overflow")?;
        for component in &components {
            retained = retained
                .checked_add(512)
                .and_then(|bytes| bytes.checked_add(component.retained_bytes()?))
                .ok_or("World decode size overflow")?;
        }
        if retained > max_bytes {
            return Err("World decoded state byte budget exhausted".into());
        }
        entities.push(WorldSerializedEntity {
            persistent_id,
            link: WorldSerializedEntityLink {
                parent,
                order,
            },
            metadata: EntityMetadata {
                symbolic_id,
                classes,
            },
            components,
        });
    }
    let mut systems = BTreeMap::new();
    let count = reader.count(8)?;
    reader.claim(count.saturating_mul(128))?;
    for _ in 0..count {
        let system = reader.string()?;
        if system.is_empty()
            || systems
                .last_key_value()
                .is_some_and(|(previous, _)| previous >= &system)
        {
            return Err("Unordered or duplicate persistent System identity".into());
        }
        let bytes = reader.blob()?;
        reader.claim(bytes.len())?;
        systems.insert(system, bytes.to_vec());
    }
    Ok(WorldSnapshot {
        metadata: WorldMetadata {
            symbolic_id,
            persistent_id,
        },
        capacity_hints,
        selected_systems,
        next_entity_id,
        entities,
        systems,
    })
}

pub(crate) fn encode_hints(
    writer: &mut WorldBinaryWriter,
    hints: &WorldCapacityHints,
) -> Result<(), String> {
    writer.count(hints.entities)?;
    writer.count(hints.systems.len())?;
    for (system, values) in &hints.systems {
        writer.string(system)?;
        writer.count(values.0.len())?;
        for (key, &value) in &values.0 {
            writer.string(key)?;
            writer.count(value)?;
        }
    }
    Ok(())
}

pub(crate) fn decode_hints(
    reader: &mut WorldBinaryReader<'_>,
) -> Result<WorldCapacityHints, String> {
    let entities = reader.u32()? as usize;
    let count = reader.count(8)?;
    reader.claim(count.saturating_mul(128))?;
    let mut systems = BTreeMap::new();
    for _ in 0..count {
        let system = reader.string()?;
        let count = reader.count(8)?;
        reader.claim(count.saturating_mul(128))?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let key = reader.string()?;
            let value = reader.u32()? as usize;
            if values.insert(key, value).is_some() {
                return Err("Duplicate system capacity hint".into());
            }
        }
        if systems
            .insert(system, WorldSystemCapacityHints(values))
            .is_some()
        {
            return Err("Duplicate system capacity hints".into());
        }
    }
    Ok(WorldCapacityHints {
        entities,
        systems,
    })
}

fn encode_field(writer: &mut WorldBinaryWriter, field: FieldValue) -> Result<(), String> {
    writer.u8(field.kind() as u8)?;
    match field {
        FieldValue::World(None) | FieldValue::Output(None) => Ok(()),
        FieldValue::World(Some(_)) | FieldValue::Output(Some(_)) => {
            Err("Runtime reference in durable component".into())
        }
        FieldValue::Dynamic(value) => writer.blob(&value.encode()),
        FieldValue::F32(value) => writer.u32(value.to_bits()),
        FieldValue::U32(value) => writer.u32(value),
        FieldValue::U64(value) => writer.u64(value),
        FieldValue::Entity(value) => writer.u64(value.to_bits()),
        FieldValue::String(value) => writer.string(&value),
        FieldValue::Bytes(value) => writer.blob(&value),
        FieldValue::Bool(value) => writer.u8(u8::from(value)),
        FieldValue::Rows(value) => writer.blob(&value),
        FieldValue::Unset => Err("Row property absence is not a serialized field".into()),
    }
}

fn decode_field(reader: &mut WorldBinaryReader<'_>) -> Result<FieldValue, String> {
    Ok(match reader.u8()? {
        1 => {
            let value = f32::from_bits(reader.u32()?);
            if !value.is_finite() {
                return Err("Nonfinite serialized field".into());
            }
            FieldValue::F32(value)
        }
        2 => FieldValue::Entity(EntityId::from_bits(reader.u64()?)),
        3 => FieldValue::U32(reader.u32()?),
        4 => FieldValue::U64(reader.u64()?),
        5 => FieldValue::String(reader.text()?),
        6 => {
            let bytes = reader.blob()?;
            reader.claim(bytes.len())?;
            FieldValue::Bytes(bytes.to_vec())
        }
        11 => {
            let bytes = reader.blob()?;
            reader.claim(bytes.len())?;
            FieldValue::Dynamic(
                crate::DynamicValue::decode(bytes).map_err(|_| "Invalid dynamic value")?,
            )
        }
        7 => FieldValue::Bool(match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err("Invalid serialized boolean".into()),
        }),
        // A whole rows table; the component decodes and validates it on restore.
        8 => {
            let bytes = reader.blob()?;
            reader.claim(bytes.len())?;
            FieldValue::Rows(bytes.to_vec())
        }
        12 => FieldValue::World(None),
        13 => FieldValue::Output(None),
        _ => return Err("Unknown serialized field kind".into()),
    })
}

#[cfg(test)]
#[path = "container_tests.rs"]
mod tests;
