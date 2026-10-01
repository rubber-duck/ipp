use super::*;

fn container(version: u32, body: &[u8]) -> Vec<u8> {
    let mut graph = WorldBinaryWriter::new(4096);
    graph.u32(0).unwrap();
    graph.u32(1).unwrap();
    graph.u32(0).unwrap();
    graph.raw(body).unwrap();
    graph.u32(0).unwrap();
    let body = &graph.bytes;
    let mut writer = WorldBinaryWriter::new(4096);
    writer.raw(b"IPPW").unwrap();
    writer.u32(version).unwrap();
    writer.u64(123).unwrap();
    writer.u64((32 + body.len()) as u64).unwrap();
    writer.u64(checksum(body)).unwrap();
    writer.raw(body).unwrap();
    writer.bytes
}

fn body_with_selected(selected: &[&str]) -> Vec<u8> {
    let mut writer = WorldBinaryWriter::new(4096);
    writer.string("legacy").unwrap();
    writer.raw(&1u128.to_le_bytes()).unwrap();
    writer.u64(0).unwrap();
    writer.u32(0).unwrap(); // Entity reservation.
    writer.u32(0).unwrap(); // System reservations.
    writer.count(selected.len()).unwrap();
    for system in selected {
        writer.string(system).unwrap();
    }
    writer.u32(0).unwrap(); // Authored entities.
    writer.bytes
}

fn empty_body() -> Vec<u8> {
    body_with_selected(&["extension.first", "extension.second"])
}

#[test]
fn selected_systems_reject_duplicate_and_truncated_sections() {
    let duplicate = body_with_selected(&["extension.first", "extension.first"]);
    assert!(
        WorldGraphSnapshot::decode(&container(7, &duplicate), 123, Default::default())
            .unwrap_err()
            .contains("duplicate selected System")
    );

    let mut truncated = body_with_selected(&["extension.first"]);
    truncated.truncate(truncated.len() - 7);
    let error =
        WorldGraphSnapshot::decode(&container(7, &truncated), 123, Default::default()).unwrap_err();
    assert!(error.contains("Truncated World file"), "{error}");

    let mut oversized = body_with_selected(&[]);
    let count_offset = 4 + "legacy".len() + 16 + 8 + 4 + 4;
    oversized[count_offset..count_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        WorldGraphSnapshot::decode(&container(7, &oversized), 123, Default::default())
            .unwrap_err()
            .contains("World count exceeds remaining data")
    );
}

#[test]
fn obsolete_containers_are_rejected_without_compatibility_decoding() {
    let mut body = empty_body();
    {
        body.extend_from_slice(&1u64.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
    }
    for version in [2, 3, 4, 5] {
        assert_eq!(
            WorldGraphSnapshot::decode(&container(version, &body), 123, Default::default())
                .unwrap_err(),
            "Unsupported World container"
        );
    }
}

#[test]
fn rows_fields_persist_as_one_table_record_each() {
    use crate::components::{RowsFixture, RowsFixtureItem, RowsFixtureTag};
    use crate::services::asset_management::{AssetSource, AssetTypeId};

    let mut fixture = RowsFixture::default();
    fixture
        .items
        .push(RowsFixtureItem {
            weight: 2.0,
            texture: Some(AssetSource {
                kind: AssetTypeId(4),
                uri: "textures/row.png".into(),
                variant: 1,
            }),
            ..Default::default()
        })
        .unwrap();
    fixture
        .items
        .push(RowsFixtureItem {
            label: Some("zoë ✓".into()),
            ..Default::default()
        })
        .unwrap();
    fixture.items.remove(0);
    fixture
        .tags
        .insert(
            3,
            RowsFixtureTag {
                value: 7,
            },
        )
        .unwrap();

    let component = ComponentValue::RowsFixture(fixture);
    assert_eq!(
        component
            .fields()
            .iter()
            .filter(|(_, value)| matches!(value, FieldValue::Rows(_)))
            .count(),
        2
    );

    let snapshot = WorldSnapshot {
        metadata: crate::WorldMetadata {
            symbolic_id: "rows".into(),
            persistent_id: WorldPersistentId(1),
        },
        capacity_hints: Default::default(),
        selected_systems: vec!["ipp.animation".into()],
        next_entity_id: 1,
        entities: vec![WorldSerializedEntity {
            persistent_id: EntityPersistentId(1),
            link: WorldSerializedEntityLink {
                parent: None,
                order: crate::EntityOrder::from_value(1).unwrap(),
            },
            metadata: EntityMetadata::default(),
            components: vec![component],
        }],
        systems: BTreeMap::new(),
    };
    let snapshot = WorldGraphSnapshot {
        root: WorldGraphNodeId(0),
        nodes: vec![WorldGraphNode {
            id: WorldGraphNodeId(0),
            world: snapshot,
            references: Vec::new(),
        }],
    };
    let encoded = snapshot.encode(5, Default::default()).unwrap();
    let decoded = WorldGraphSnapshot::decode(&encoded, 5, Default::default()).unwrap();
    assert_eq!(decoded, snapshot);
    let ComponentValue::RowsFixture(restored) = &decoded.nodes[0].world.entities[0].components[0]
    else {
        panic!("restored component type");
    };
    assert_eq!(restored.items.next_slot(), 2);
    assert!(!restored.items.is_live(0));
    assert_eq!(
        restored.items.get(1).unwrap().label.as_deref(),
        Some("zoë ✓")
    );
    assert_eq!(restored.tags.next_slot(), 4);

    // A table that fails row validation rejects the whole candidate.
    let table = restored.tags.encode();
    let at = encoded
        .windows(table.len())
        .position(|window| window == table)
        .unwrap();
    let mut corrupt = encoded.clone();
    corrupt[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
    let digest = checksum(&corrupt[32..]);
    corrupt[24..32].copy_from_slice(&digest.to_le_bytes());
    assert!(
        WorldGraphSnapshot::decode(&corrupt, 5, Default::default())
            .unwrap_err()
            .contains("Invalid persistent field")
    );
}

#[test]
fn v7_round_trips_opaque_extension_sections_and_rejects_duplicate_keys() {
    let mut body = WorldBinaryWriter::new(4096);
    body.raw(&empty_body()).unwrap();
    body.u32(2).unwrap();
    body.string("extension.first").unwrap();
    body.blob(&[0xff, 0, 7]).unwrap();
    body.string("extension.second").unwrap();
    body.blob(&[1, 2]).unwrap();
    let bytes = container(7, &body.bytes);
    let snapshot = WorldGraphSnapshot::decode(&bytes, 123, Default::default()).unwrap();
    assert_eq!(
        snapshot.nodes[0].world.systems["extension.first"],
        [0xff, 0, 7]
    );
    assert_eq!(snapshot.encode(123, Default::default()).unwrap(), bytes);

    let mut duplicate = WorldBinaryWriter::new(4096);
    duplicate.raw(&empty_body()).unwrap();
    duplicate.u32(2).unwrap();
    for _ in 0..2 {
        duplicate.string("extension.first").unwrap();
        duplicate.blob(&[]).unwrap();
    }
    assert!(
        WorldGraphSnapshot::decode(&container(7, &duplicate.bytes), 123, Default::default())
            .unwrap_err()
            .contains("duplicate persistent System")
    );
}
