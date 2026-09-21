//! IPPW v3: generic System payloads, with compatible-build v2 decoding.

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

impl WorldSnapshot {
    /// Encode bounded owned state without opening or rewriting resource references.
    pub fn encode(&self, contract: u64, limits: WorldPersistenceLimits) -> Result<Vec<u8>, String> {
        self.validate_references()?;
        let mut writer = WorldBinaryWriter::new(limits.max_bytes);
        writer.raw(b"IPPW")?;
        writer.u32(3)?;
        writer.u64(contract)?;
        writer.u64(0)?;
        writer.u64(0)?;
        encode_snapshot(&mut writer, self)?;

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
        if !matches!(version, 2 | 3) {
            return Err("Unsupported World container".into());
        }
        if reader.u64()? != contract {
            return Err("World file target contract mismatch".into());
        }
        if reader.u64()? != bytes.len() as u64 || reader.u64()? != checksum(&bytes[32..]) {
            return Err("World file length or checksum mismatch".into());
        }
        let snapshot = decode_snapshot(&mut reader, limits.max_bytes, version)?;
        reader.end()?;
        snapshot.validate_references()?;
        Ok(snapshot)
    }

    fn validate_references(&self) -> Result<(), String> {
        let ids: BTreeSet<_> = self
            .entities
            .iter()
            .map(|entity| entity.persistent_id.0)
            .collect();
        for entity in &self.entities {
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
    writer.count(snapshot.entities.len())?;
    for entity in &snapshot.entities {
        writer.u64(entity.persistent_id.0)?;
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
    version: u32,
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
    let count = reader.count(17)?;
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
            component
                .validate_lifecycle()
                .map_err(|error| error.to_string())?;
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
            metadata: EntityMetadata {
                symbolic_id,
                classes,
            },
            components,
        });
    }
    let mut systems = BTreeMap::new();
    if version == 2 {
        {
            let state = crate::systems::animation::decode_legacy_persistent_state(reader)?;
            let bytes = state.encode(max_bytes)?;
            reader.claim(bytes.len())?;
            systems.insert(
                crate::systems::animation::AnimationSystem::ID.0.to_owned(),
                bytes,
            );
        }
    } else {
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
    }
    Ok(WorldSnapshot {
        metadata: WorldMetadata {
            symbolic_id,
            persistent_id,
        },
        capacity_hints,
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
        FieldValue::Dynamic(value) => writer.blob(&value.encode()),
        FieldValue::F32(value) => writer.u32(value.to_bits()),
        FieldValue::U32(value) => writer.u32(value),
        FieldValue::U64(value) => writer.u64(value),
        FieldValue::Entity(value) => writer.u64(value.to_bits()),
        FieldValue::String(value) => writer.string(&value),
        FieldValue::Bytes(value) => writer.blob(&value),
        FieldValue::Bool(value) => writer.u8(u8::from(value)),
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
        5 => FieldValue::String(reader.string()?),
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
        _ => return Err("Unknown serialized field kind".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(version: u32, body: &[u8]) -> Vec<u8> {
        let mut writer = WorldBinaryWriter::new(4096);
        writer.raw(b"IPPW").unwrap();
        writer.u32(version).unwrap();
        writer.u64(123).unwrap();
        writer.u64((32 + body.len()) as u64).unwrap();
        writer.u64(checksum(body)).unwrap();
        writer.raw(body).unwrap();
        writer.bytes
    }

    fn empty_body() -> Vec<u8> {
        let mut writer = WorldBinaryWriter::new(4096);
        writer.string("legacy").unwrap();
        writer.raw(&1u128.to_le_bytes()).unwrap();
        writer.u64(0).unwrap();
        writer.u32(0).unwrap(); // Entity reservation.
        writer.u32(0).unwrap(); // System reservations.
        writer.u32(0).unwrap(); // Authored entities.
        writer.bytes
    }

    #[test]
    fn legacy_v2_decodes_into_system_owned_payloads() {
        #[allow(unused_mut)]
        let mut body = empty_body();
        {
            body.extend_from_slice(&1u64.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
        }
        let snapshot =
            WorldSnapshot::decode(&container(2, &body), 123, Default::default()).unwrap();
        assert_eq!(snapshot.metadata.symbolic_id, "legacy");
        assert!(snapshot.entities.is_empty());
        assert_eq!(
            crate::systems::animation::AnimationPersistentState::decode(
                &snapshot.systems[crate::systems::animation::AnimationSystem::ID.0],
                4096,
            )
            .unwrap(),
            crate::systems::animation::AnimationPersistentState::default(),
        );
        let encoded = snapshot.encode(123, Default::default()).unwrap();
        assert_eq!(u32::from_le_bytes(encoded[4..8].try_into().unwrap()), 3);
        assert_eq!(
            WorldSnapshot::decode(&encoded, 123, Default::default()).unwrap(),
            snapshot
        );
    }

    #[test]
    fn v3_round_trips_opaque_extension_sections_and_rejects_duplicate_keys() {
        let mut body = WorldBinaryWriter::new(4096);
        body.raw(&empty_body()).unwrap();
        body.u32(2).unwrap();
        body.string("extension.first").unwrap();
        body.blob(&[0xff, 0, 7]).unwrap();
        body.string("extension.second").unwrap();
        body.blob(&[1, 2]).unwrap();
        let bytes = container(3, &body.bytes);
        let snapshot = WorldSnapshot::decode(&bytes, 123, Default::default()).unwrap();
        assert_eq!(snapshot.systems["extension.first"], [0xff, 0, 7]);
        assert_eq!(snapshot.encode(123, Default::default()).unwrap(), bytes);

        let mut duplicate = WorldBinaryWriter::new(4096);
        duplicate.raw(&empty_body()).unwrap();
        duplicate.u32(2).unwrap();
        for _ in 0..2 {
            duplicate.string("extension.first").unwrap();
            duplicate.blob(&[]).unwrap();
        }
        assert!(
            WorldSnapshot::decode(&container(3, &duplicate.bytes), 123, Default::default())
                .unwrap_err()
                .contains("duplicate persistent System")
        );
    }
}
