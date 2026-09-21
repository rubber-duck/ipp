//! Optional CPU streams through real owned ingress. Native/browser harnesses
//! additionally exercise transport, GPU defaults, shading and context recovery.

mod support;
use support::WorldTestDriver;

use ipp_core::ComponentValue;
use ipp_core::{ErrorReason, MeshKey, MeshStats, MeshUpload};

fn test_world(host: &mut ipp_core::HostRuntime) -> ipp_core::WorldContext<'_> {
    let world_id = host.create_world(ipp_core::WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world.register_stream_resource_provider("http").unwrap();
    world
}

const POSITIONS: [[f32; 3]; 3] = [[-0.5, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
const COLORS: [[f32; 3]; 3] = [[0.25, 0.5, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
const NORMALS: [[f32; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 2.0, 1.0], [1.0, 0.0, 2.0]];
const UVS: [[f32; 2]; 3] = [[-0.5, 2.0], [1.0, 0.0], [0.0, 1.0]];

fn floats<const N: usize>(values: &[[f32; N]]) -> Vec<u8> {
    values
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

fn payload(semantics: &[u8]) -> Vec<u8> {
    let streams = [
        floats(&POSITIONS),
        floats(&COLORS),
        floats(&UVS),
        vec![0, 127, 255],
        floats(&NORMALS),
    ];
    let mut bytes = b"IPPM".to_vec();
    for value in [3, 3, 3, semantics.len() as u32] {
        bytes.extend(value.to_le_bytes());
    }
    for &semantic in semantics {
        bytes.extend([semantic, [1, 1, 2, 3, 1][semantic as usize], 0, 0]);
        bytes.extend((streams[semantic as usize].len() as u32).to_le_bytes());
    }
    for &semantic in semantics {
        bytes.extend(&streams[semantic as usize]);
    }
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}

fn key(asset_id: u64) -> MeshKey {
    MeshKey {
        asset: asset_id,
        variant: 0,
    }
}

fn publish(
    world: &mut ipp_core::WorldContext<'_>,
    asset_id: u64,
    bytes: Vec<u8>,
) -> Result<MeshStats, ErrorReason> {
    let decoded = ipp_core::MeshAsset::decode(&bytes).map(|(_, stats)| stats);
    world.enqueue_mesh(MeshUpload {
        id: asset_id,
        key: key(asset_id),
        bytes,
    })?;
    let result = world.await_upload_for_test().assets[0].result.clone();
    match result {
        Ok(_) => decoded,
        Err(error) if error.contains("Capacity") || error.contains("budget") => {
            Err(ErrorReason::Capacity)
        }
        Err(error) if error == "DuplicateAsset" => Err(ErrorReason::DuplicateAsset),
        Err(_) => Err(ErrorReason::InvalidAsset),
    }
}

#[test]
fn each_supported_layout_retains_only_authored_streams_and_exact_bytes() {
    for semantics in [
        &[0][..],
        &[0, 1],
        &[0, 2],
        &[0, 1, 2],
        &[0, 2, 3],
        &[0, 1, 2, 3],
        &[0, 4],
        &[0, 1, 4],
        &[0, 2, 3, 4],
        &[0, 1, 2, 3, 4],
    ] {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let mut world = test_world(&mut fixture_host);
        let bytes = payload(semantics);
        let source_bytes = bytes.len();
        let result = publish(&mut world, 1, bytes);
        let stats = result.unwrap();
        let mesh = world.mesh(key(1)).unwrap();
        let expected_bytes = semantics
            .iter()
            .map(|&s| [36, 36, 24, 3, 36][s as usize])
            .sum::<usize>();
        assert_eq!(stats.source_bytes as usize, source_bytes);
        assert_eq!(stats.vertex_bytes as usize, expected_bytes);
        assert_eq!(stats.index_bytes, 6);
        assert_eq!((stats.vertices, stats.indices), (3, 3));
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.vertex_bytes(), expected_bytes);
        assert_eq!(mesh.positions(), POSITIONS);
        assert_eq!(
            mesh.colors(),
            semantics.contains(&1).then_some(COLORS.as_slice())
        );
        assert_eq!(mesh.indices(), [0, 1, 2]);
        assert_eq!(
            mesh.normals(),
            semantics.contains(&4).then_some(NORMALS.as_slice())
        );
        {
            assert_eq!(mesh.uvs(), semantics.contains(&2).then_some(UVS.as_slice()));
            assert_eq!(
                mesh.texture_weights(),
                semantics.contains(&3).then_some([0, 127, 255].as_slice())
            );
        }

        let positions = mesh.positions().as_ptr();
        let colors = mesh.colors().map(|v| v.as_ptr());
        let normals = mesh.normals().map(|v| v.as_ptr());
        // A differently laid out asset_id must not replace old CPU storage.
        publish(&mut world, 2, payload(&[0])).unwrap();
        world.update_for_test(0.0).unwrap();
        assert_eq!(world.mesh(key(1)).unwrap().positions().as_ptr(), positions);
        assert_eq!(
            world.mesh(key(1)).unwrap().colors().map(|v| v.as_ptr()),
            colors
        );
        assert_eq!(
            world.mesh(key(1)).unwrap().normals().map(|v| v.as_ptr()),
            normals
        );
        assert_eq!(world.mesh(key(2)).unwrap().vertex_bytes(), 36);
    }
}

#[test]
fn malformed_descriptors_lengths_geometry_and_trailing_data_never_publish() {
    let valid = payload(&[0, 1]);
    let mut cases = Vec::new();
    for end in 0..valid.len() {
        cases.push(valid[..end].to_vec());
    }
    for (offset, value) in [
        (0, u32::from_le_bytes(*b"FAIL")),
        (4, 4),
        (8, 0),
        (8, 65537),
        (8, u32::MAX),
        (12, 0),
        (12, 2),
        (12, u32::MAX - 2),
        (16, 0),
        (16, 5),
        (16, u32::MAX),
        (24, 35),
        (24, 37),
        (24, u32::MAX),
        (32, 35),
        (32, u32::MAX),
        (36, f32::NAN.to_bits()),
        (40, f32::INFINITY.to_bits()),
        (72, (-0.01f32).to_bits()),
        (76, 1.01f32.to_bits()),
        (80, f32::NEG_INFINITY.to_bits()),
    ] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        cases.push(bytes);
    }
    for (offset, value) in [
        (20, 4),
        (21, 2),
        (22, 1),
        (23, 1),
        (28, 0),
        (29, 3),
        (30, 1),
        (31, 1),
    ] {
        let mut bytes = valid.clone();
        bytes[offset] = value;
        cases.push(bytes);
    }
    cases.extend([
        payload(&[1]),
        payload(&[1, 0]),
        payload(&[0, 0]),
        payload(&[0, 3]),
    ]);
    let mut trailing = valid.clone();
    trailing.push(0);
    cases.push(trailing);
    let mut bad_index = valid.clone();
    let end = bad_index.len();
    bad_index[end - 2..].copy_from_slice(&3u16.to_le_bytes());
    cases.push(bad_index);
    let mut degenerate = valid.clone();
    degenerate[36..72].fill(0);
    cases.push(degenerate);

    for (case, bytes) in cases.into_iter().enumerate() {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let mut world = test_world(&mut fixture_host);
        assert_eq!(
            publish(&mut world, 2, bytes),
            Err(ErrorReason::InvalidAsset),
            "case {case}"
        );
        assert!(world.mesh(key(2)).is_none());
        // A rejected payload still pins its name; a different payload uses another name.
        assert!(publish(&mut world, 1, valid.clone()).is_ok(), "case {case}");
        assert_eq!(
            publish(&mut world, 2, valid.clone()),
            Err(ErrorReason::DuplicateAsset)
        );
        assert!(publish(&mut world, 3, valid.clone()).is_ok(), "case {case}");
    }
}

#[test]
fn weighted_payloads_validate_uvs_formats_and_unaligned_index_tail() {
    let valid = payload(&[0, 2, 3]);
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    let stats = publish(&mut world, 1, valid.clone()).unwrap();
    assert_eq!(
        (stats.source_bytes, stats.vertex_bytes, stats.index_bytes),
        (113, 63, 6)
    );
    assert_eq!(world.mesh(key(1)).unwrap().indices(), [0, 1, 2]);

    let mut cases = vec![payload(&[0, 3]), payload(&[0, 1, 3]), payload(&[0, 3, 2])];
    for (offset, value) in [(29, 1), (37, 1), (38, 1), (39, 1)] {
        let mut bytes = valid.clone();
        bytes[offset] = value;
        cases.push(bytes);
    }
    for (offset, value) in [
        (32, 23),
        (40, 4),
        (80, f32::NAN.to_bits()),
        (84, f32::INFINITY.to_bits()),
    ] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        cases.push(bytes);
    }
    for (index, bytes) in cases.into_iter().enumerate() {
        assert_eq!(
            publish(&mut world, index as u64 + 2, bytes),
            Err(ErrorReason::InvalidAsset)
        );
        assert!(world.mesh(key(2)).is_none());
    }
}

#[test]
fn v3_retained_memory_counts_actual_streams_beyond_former_quota() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    assert_eq!(
        publish(&mut world, 1000, vec![0; (1 << 20) + 1]),
        Err(ErrorReason::InvalidAsset)
    );

    // The same 65536 positions use half the retained bytes of legacy RGB vertices.
    // Repeated indices are permitted as long as one triangle has area.
    let mut bytes = b"IPPM".to_vec();
    for value in [3u32, 65536, 3, 1] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 1, 0, 0]);
    bytes.extend((65536u32 * 12).to_le_bytes());
    bytes.extend(floats(&POSITIONS));
    bytes.resize(28 + 65536 * 12, 0);
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    bytes.shrink_to_fit();

    let per_mesh = 65536 * 12 + 6;
    let accepted = (16 << 20) / per_mesh + 2;
    for asset_id in 1..=accepted {
        let stats = publish(&mut world, asset_id as u64, bytes.clone()).unwrap();
        assert_eq!(stats.vertex_bytes, 65536 * 12);
    }
    assert!(world.asset_resources().resident_bytes() > 16 << 20);
    assert_eq!(world.mesh(key(1)).unwrap().vertex_bytes(), 65536 * 12);
}

#[test]
fn optional_color_does_not_block_activation_but_active_texture_requires_uvs() {
    for (material_type, texture_type) in [
        (
            ComponentValue::UNLIT_MATERIAL,
            ComponentValue::UNLIT_TEXTURE,
        ),
        (
            ComponentValue::UNLIT_MATERIAL,
            ComponentValue::BASE_COLOR_TEXTURE,
        ),
        (ComponentValue::PBR_MATERIAL, ComponentValue::UNLIT_TEXTURE),
        (
            ComponentValue::PBR_MATERIAL,
            ComponentValue::BASE_COLOR_TEXTURE,
        ),
    ] {
        texture_activation(material_type, texture_type);
    }
}

fn texture_activation(material_type: u16, texture_type: u16) {
    use ipp_core::{
        Batch, Command, EntityMetadata, EntityRef, FieldValue, FieldWrite, TextureKey,
        TextureUpload,
        components::{MeshInstance, UnlitTexture},
    };
    use std::mem::offset_of;

    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    publish(&mut world, 1, payload(&[0])).unwrap();
    publish(&mut world, 2, payload(&[0, 2])).unwrap();
    publish(&mut world, 3, payload(&[0, 2, 3])).unwrap();
    let texture_key = TextureKey {
        asset: 2,

        variant: 0,
    };
    let mut texture = b"IPPT".to_vec();
    for value in [3u32, 1, 1] {
        texture.extend(value.to_le_bytes());
    }
    texture.extend([32, 64, 128, 255]);
    world
        .enqueue_texture(TextureUpload {
            id: 1,
            key: texture_key,
            bytes: texture.clone(),
        })
        .unwrap();

    let insert = |component, fields| Command::InsertComponent {
        entity: EntityRef::Alias(0),
        component,
        fields,
    };
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata::default(),
                },
                insert(3, vec![]),
                insert(material_type, vec![]),
                insert(
                    5,
                    vec![FieldWrite {
                        offset: offset_of!(MeshInstance, source) as u32,
                        value: FieldValue::String("http://fixture/1".into()),
                    }],
                ),
            ],
        })
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert!(world.render_items().is_empty());
    let deliver = |world: &mut ipp_core::WorldContext<'_>| {
        for request in world.resource_requests_for_test() {
            let bytes = match request.kind {
                ipp_core::AssetResourceKind::Mesh => payload(
                    match request
                        .source
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .parse::<u64>()
                        .unwrap()
                    {
                        1 => &[0],
                        2 => &[0, 2],
                        _ => &[0, 2, 3],
                    },
                ),
                ipp_core::AssetResourceKind::Texture => texture.clone(),
                _ => panic!("unexpected fixture kind"),
            };
            world.complete_resource(request.id, Ok(bytes)).unwrap();
        }
        world.update_for_test(0.0).unwrap();
    };
    deliver(&mut world);
    assert!(
        world
            .resource_snapshots()
            .iter()
            .any(|resource| resource.source == "http://fixture/1")
    );
    assert!(world.mesh(key(1)).unwrap().colors().is_none());

    let texture_command = || Command::InsertComponent {
        entity: EntityRef::Handle(entity),
        component: texture_type,
        fields: vec![FieldWrite {
            offset: offset_of!(UnlitTexture, source) as u32,
            value: FieldValue::String("http://fixture/texture".into()),
        }],
    };
    let select_mesh = |asset_id| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::MESH_INSTANCE,
        field: FieldWrite {
            offset: offset_of!(MeshInstance, source) as u32,
            value: FieldValue::String(format!("http://fixture/{asset_id}")),
        },
    };
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![texture_command()],
        })
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert!(report.outcomes[0].result.is_ok());
    deliver(&mut world);
    assert!(world.render_items().is_empty());
    assert_eq!(world.render_diagnostics()[0].entity, entity);

    world
        .enqueue(Batch {
            id: 3,
            operations: vec![select_mesh(2), texture_command()],
        })
        .unwrap();
    assert!(
        world.update_for_test(0.0).unwrap().outcomes[0]
            .result
            .is_ok()
    );
    deliver(&mut world);
    assert!(world.render_items()[0].texture.is_some());
    world
        .enqueue(Batch {
            id: 4,
            operations: vec![select_mesh(3)],
        })
        .unwrap();
    assert!(
        world.update_for_test(0.0).unwrap().outcomes[0]
            .result
            .is_ok()
    );

    deliver(&mut world);
    world
        .enqueue(Batch {
            id: 5,
            operations: vec![select_mesh(1)],
        })
        .unwrap();
    assert!(
        world.update_for_test(0.0).unwrap().outcomes[0]
            .result
            .is_ok()
    );
    deliver(&mut world);
    assert!(world.render_items().is_empty());
    assert_eq!(
        world.render_diagnostics()[0].reason,
        ErrorReason::InvalidAsset
    );
}

#[test]
fn invalid_normals_reject_without_publishing_partial_geometry() {
    let valid = payload(&[0, 4]);
    let mut cases = Vec::new();
    for normal in [
        [0.0, -0.0, 0.0],
        [f32::NAN, 0.0, 1.0],
        [0.0, f32::INFINITY, 1.0],
    ] {
        let mut bytes = valid.clone();
        bytes[72..84].copy_from_slice(&floats(&[normal]));
        cases.push(bytes);
    }
    for (offset, value) in [(28, 5), (29, 2), (32, 35), (32, 37)] {
        let mut bytes = valid.clone();
        bytes[offset] = value;
        cases.push(bytes);
    }
    cases.push(payload(&[0, 4, 4]));
    cases.push(payload(&[4, 0]));
    for bytes in cases {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let mut world = test_world(&mut fixture_host);
        assert_eq!(
            publish(&mut world, 1, bytes),
            Err(ErrorReason::InvalidAsset)
        );
        assert!(world.mesh(key(1)).is_none());
        publish(&mut world, 2, valid.clone()).unwrap();
    }
}

#[test]
fn mesh_input_above_one_mib_loads_through_owned_ingress() {
    let mut bytes = b"IPPM".to_vec();
    for value in [1u32, 65_536, 3] {
        bytes.extend(value.to_le_bytes());
    }
    for index in 0..65_536 {
        for value in POSITIONS[index % 3].into_iter().chain([1.0; 3]) {
            bytes.extend(value.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    assert!(bytes.len() > 1 << 20);
    let mut host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut host);
    world
        .enqueue_mesh(MeshUpload {
            id: 1,
            key: key(1),
            bytes,
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
    assert_eq!(world.mesh(key(1)).unwrap().vertex_count(), 65_536);
}
