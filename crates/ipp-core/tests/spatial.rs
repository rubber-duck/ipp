//! Direct-core scene/asset invariants. The parent harness owns transport and GPU evidence.

mod support;
use support::WorldTestDriver;
use support::selection::RENDER;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, ErrorReason, FieldValue,
    FieldWrite, MeshKey, MeshUpload, WorldUpdateReport,
    components::{MeshInstance, Transform, UnlitMaterial},
};
use std::mem::offset_of;

fn test_world(host: &mut ipp_core::HostRuntime) -> ipp_core::WorldContext<'_> {
    let world_id = host
        .create_world(ipp_core::WorldLimits::default(), RENDER)
        .unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world.register_stream_resource_provider("http").unwrap();
    world
}

fn mesh_bytes() -> Vec<u8> {
    let mut bytes = b"IPPM".to_vec();
    for n in [1u32, 3, 3] {
        bytes.extend(n.to_le_bytes());
    }
    for vertex in [
        [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    ] {
        for v in vertex {
            bytes.extend(v.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}

fn selection(world: &ipp_core::WorldContext<'_>, key: MeshKey) -> MeshKey {
    let asset = world
        .asset_resources()
        .get(ipp_core::services::asset_management::AssetKey::from_u64(
            key.asset,
        ))
        .unwrap();
    let name = asset
        .source()
        .uri
        .rsplit('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    MeshKey {
        asset: name,
        variant: key.variant,
    }
}

fn key(asset_id: u64, variant: u32) -> MeshKey {
    MeshKey {
        asset: asset_id,
        variant,
    }
}

fn upload(world: &mut ipp_core::WorldContext<'_>, key: MeshKey) {
    world
        .enqueue_mesh(MeshUpload {
            id: 23,
            key,
            bytes: mesh_bytes(),
        })
        .unwrap();
}

fn run(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap()
}

fn ok(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    let report = run(world, operations);
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    // These existing state/overlay tests use immediate fixture delivery. The
    // The asset-runtime suite controls pending publication explicitly.
    for request in world.resource_requests_for_test() {
        world
            .complete_resource(request.id, Ok(mesh_bytes()))
            .unwrap();
    }
    world.update_for_test(0.0).unwrap();
    report
}

fn reject(
    world: &mut ipp_core::WorldContext<'_>,
    operations: Vec<Command>,
    reason: ErrorReason,
) -> WorldUpdateReport {
    let report = run(world, operations);
    assert_eq!(
        report.outcomes[0].result.as_ref().unwrap_err().reason,
        reason
    );
    report
}

fn f32_field(offset: usize, value: f32) -> FieldWrite {
    FieldWrite {
        offset: offset as u32,
        value: FieldValue::F32(value),
    }
}

fn mesh_fields(key: MeshKey) -> Vec<FieldWrite> {
    vec![
        FieldWrite {
            offset: offset_of!(MeshInstance, source) as u32,
            value: FieldValue::String(std::sync::Arc::<str>::from(format!(
                "http://fixture/{}",
                key.asset
            ))),
        },
        FieldWrite {
            offset: offset_of!(MeshInstance, variant) as u32,
            value: FieldValue::U32(key.variant),
        },
    ]
}

fn insert(entity: EntityRef, component: u16, fields: Vec<FieldWrite>) -> Command {
    Command::InsertComponent {
        entity: entity.clone(),
        component,
        fields,
        adopt: false,
    }
}

fn create(world: &mut ipp_core::WorldContext<'_>) -> EntityId {
    let report = ok(
        world,
        vec![Command::Create {
            alias: 0,
            metadata: EntityMetadata {
                symbolic_id: Some("cube".into()),
                classes: vec![],
            },
            adopt: false,
        }],
    );
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

fn renderable(world: &mut ipp_core::WorldContext<'_>) -> EntityId {
    upload(world, key(1, 0));
    let id = create(world);
    ok(
        world,
        vec![
            insert(EntityRef::Handle(id), ComponentValue::TRANSFORM, vec![]),
            insert(
                EntityRef::Handle(id),
                ComponentValue::UNLIT_MATERIAL,
                vec![],
            ),
            insert(
                EntityRef::Handle(id),
                ComponentValue::MESH_INSTANCE,
                mesh_fields(key(1, 0)),
            ),
        ],
    );
    id
}

#[test]
fn uploads_precede_batches_and_old_assets_remain_immutable_through_staging() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    let id = renderable(&mut world);
    let mesh = world.mesh(key(1, 0)).unwrap() as *const _;
    assert_eq!(world.render_items()[0].entity, id);
    assert_eq!(world.render_items()[0].transform, Transform::default());
    assert_eq!(world.render_items()[0].material, UnlitMaterial::default());
    assert_eq!(world.mesh(key(1, 0)).unwrap().positions().len(), 3);
    assert_eq!(world.mesh(key(1, 0)).unwrap().indices(), &[0, 1, 2]);

    upload(&mut world, key(2, 0));
    upload(&mut world, key(2, 1));
    upload(&mut world, key(1, 1));
    upload(&mut world, key(1, 0));
    let report = ok(
        &mut world,
        vec![insert(
            EntityRef::Handle(id),
            ComponentValue::MESH_INSTANCE,
            mesh_fields(key(2, 1)),
        )],
    );
    let stats = report.assets[0].result.as_ref().copied().unwrap();
    assert_eq!(
        (stats.source_bytes, stats.resident_bytes),
        (94, 78 + support::unskinned_mesh_metadata_bytes(3))
    );
    assert!(report.assets[1].result.is_ok());
    assert!(report.assets[2].result.is_ok());
    assert_eq!(report.assets[3].result, Err("DuplicateAsset".into()));
    assert!(
        report
            .assets
            .iter()
            .all(|o| o.tick == report.tick && o.id == 23)
    );
    assert_eq!(world.mesh(key(1, 0)).unwrap() as *const _, mesh);
    assert_eq!(selection(&world, world.render_items()[0].mesh), key(2, 1));

    ok(
        &mut world,
        vec![insert(
            EntityRef::Handle(id),
            ComponentValue::MESH_INSTANCE,
            mesh_fields(key(1, 0)),
        )],
    );
    let report = run(
        &mut world,
        vec![insert(
            EntityRef::Handle(id),
            ComponentValue::MESH_INSTANCE,
            mesh_fields(key(9, 0)),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert!(world.render_items().is_empty());
    assert_eq!(
        world
            .resource_snapshots()
            .iter()
            .find(|resource| resource.source == std::sync::Arc::<str>::from("http://fixture/9"))
            .unwrap()
            .status,
        ipp_core::AssetResourceStatus::Unloaded
    );
    assert_eq!(world.mesh(key(1, 0)).unwrap() as *const _, mesh);
}

#[test]
fn malformed_meshes_reject_without_publishing_or_consuming_asset_id_identity() {
    let valid = mesh_bytes();
    let mut invalid = vec![vec![], valid[..15].to_vec(), valid[..93].to_vec()];
    let mut trailing = valid.clone();
    trailing.push(0);
    invalid.push(trailing);
    for (offset, bytes) in [
        (0, *b"FAIL"),
        (4, 2u32.to_le_bytes()),
        (8, 0u32.to_le_bytes()),
        (8, u32::MAX.to_le_bytes()),
        (12, 2u32.to_le_bytes()),
        (16, f32::NAN.to_le_bytes()),
        (28, 1.1f32.to_le_bytes()),
    ] {
        let mut byteset = valid.clone();
        byteset[offset..offset + 4].copy_from_slice(&bytes);
        invalid.push(byteset);
    }
    let mut bad_index = valid.clone();
    bad_index[88..90].copy_from_slice(&3u16.to_le_bytes());
    invalid.push(bad_index);
    let mut degenerate = valid.clone();
    degenerate[88..94].fill(0);
    invalid.push(degenerate);
    for bytes in invalid {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let mut world = test_world(&mut fixture_host);
        world
            .enqueue_mesh(MeshUpload {
                id: 1,
                key: key(2, 0),
                bytes,
            })
            .unwrap();
        assert_eq!(
            asset_report(&mut world).assets[0].result,
            Err("InvalidAsset".into())
        );
        assert!(world.mesh(key(2, 0)).is_none());
        upload(&mut world, key(1, 0));
        assert!(asset_report(&mut world).assets[0].result.is_ok());
    }
    for bad in [
        MeshKey {
            asset: 0,
            ..key(1, 0)
        },
        key(0, 0),
    ] {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let mut world = test_world(&mut fixture_host);
        upload(&mut world, bad);
        assert_eq!(
            asset_report(&mut world).assets[0].result,
            Err("InvalidAsset".into())
        );
    }
}

#[test]
fn upload_admission_grows_and_invalid_time_preserves_pending_work() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    // Cross the former queue limit and give every upload distinct correlation.
    for asset_id in 1..=17 {
        world
            .enqueue_mesh(MeshUpload {
                id: asset_id,
                key: key(asset_id, 0),
                bytes: mesh_bytes(),
            })
            .unwrap();
    }
    assert_eq!(world.update_for_test(-1.0), Err(ErrorReason::InvalidValue));
    assert!(world.mesh(key(1, 0)).is_none());
    let report = asset_report(&mut world);
    assert_eq!(
        report
            .assets
            .iter()
            .map(|outcome| outcome.id)
            .collect::<Vec<_>>(),
        (1..=17).collect::<Vec<_>>()
    );
    assert!(report.assets.iter().all(|outcome| outcome.result.is_ok()));

    // Spare allocation capacity does not invalidate an otherwise valid asset.
    let mut bytes = Vec::with_capacity((1 << 20) + 1);
    bytes.extend(mesh_bytes());
    world
        .enqueue_mesh(MeshUpload {
            id: 18,
            key: key(18, 0),
            bytes,
        })
        .unwrap();
    assert!(asset_report(&mut world).assets[0].result.is_ok());

    // Retaining more than the former count limit preserves every prior asset.
    for asset_id in 19..=257 {
        upload(&mut world, key(asset_id, 0));
        assert!(asset_report(&mut world).assets[0].result.is_ok());
    }
    for asset_id in 1..=257 {
        assert_eq!(world.mesh(key(asset_id, 0)).unwrap().vertex_count(), 3);
    }
}

#[test]
fn scene_activation_failures_allow_explicit_component_correction() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    let id = renderable(&mut world);
    let entity = EntityRef::Handle(id);
    for field in [
        f32_field(offset_of!(Transform, sx), 0.0),
        f32_field(offset_of!(Transform, sy), -1.0),
        f32_field(offset_of!(Transform, qw), 0.0),
    ] {
        reject(
            &mut world,
            vec![Command::SetField {
                entity: entity.clone(),
                component: ComponentValue::TRANSFORM,
                field,
            }],
            ErrorReason::InvalidValue,
        );
        ok(
            &mut world,
            vec![insert(entity.clone(), ComponentValue::TRANSFORM, vec![])],
        );
    }
    for field in [
        f32_field(offset_of!(UnlitMaterial, r), 1.1),
        f32_field(offset_of!(UnlitMaterial, g), -0.1),
        f32_field(offset_of!(UnlitMaterial, b), f32::INFINITY),
    ] {
        reject(
            &mut world,
            vec![Command::SetField {
                entity: entity.clone(),
                component: ComponentValue::UNLIT_MATERIAL,
                field,
            }],
            ErrorReason::InvalidValue,
        );
        ok(
            &mut world,
            vec![insert(
                entity.clone(),
                ComponentValue::UNLIT_MATERIAL,
                vec![],
            )],
        );
    }
    for field in [
        FieldWrite {
            offset: 1,
            value: FieldValue::F32(0.0),
        },
        FieldWrite {
            offset: 0,
            value: FieldValue::U64(1),
        },
    ] {
        reject(
            &mut world,
            vec![Command::SetField {
                entity: entity.clone(),
                component: ComponentValue::TRANSFORM,
                field,
            }],
            ErrorReason::InvalidField,
        );
    }
    ok(
        &mut world,
        vec![insert(
            entity.clone(),
            ComponentValue::MESH_INSTANCE,
            vec![],
        )],
    );
    assert!(world.render_items().is_empty());
    assert!(world.resource_requests_for_test().is_empty());
    ok(
        &mut world,
        vec![insert(
            entity.clone(),
            ComponentValue::MESH_INSTANCE,
            mesh_fields(key(1, 0)),
        )],
    );
    ok(
        &mut world,
        vec![insert(
            entity.clone(),
            3,
            vec![
                f32_field(offset_of!(Transform, qw), 0.0),
                f32_field(offset_of!(Transform, qx), 2.0),
            ],
        )],
    );
    assert_eq!(world.render_items()[0].transform.qx, 2.0);
    ok(
        &mut world,
        vec![Command::RemoveComponent {
            entity: entity.clone(),
            component: ComponentValue::UNLIT_MATERIAL,
        }],
    );
    assert!(world.render_items().is_empty());
    assert!(world.inspect(id).is_some());
    ok(
        &mut world,
        vec![insert(
            entity.clone(),
            ComponentValue::UNLIT_MATERIAL,
            vec![],
        )],
    );
    assert_eq!(world.render_items().len(), 1);
    ok(
        &mut world,
        vec![Command::Delete {
            entity: entity.clone(),
        }],
    );
    assert!(world.render_items().is_empty());
    assert!(world.mesh(key(1, 0)).is_some());
}

#[test]
fn generated_registry_ids_and_field_types_match_exact_target_offsets() {
    let cases = [
        (3, ComponentValue::Transform(Transform::default())),
        (4, ComponentValue::UnlitMaterial(UnlitMaterial::default())),
        (5, ComponentValue::MeshInstance(MeshInstance::default())),
    ];
    for (id, value) in cases {
        assert_eq!(value.type_id(), id);
        assert_eq!(ipp_core::components::registry::create(id).unwrap(), value);
        for (offset, field) in value.fields() {
            ComponentValue::validate_field(id, offset, field.kind()).unwrap();
        }
    }
    let fields = ComponentValue::MeshInstance(MeshInstance::default()).fields();
    assert_eq!(
        fields,
        vec![
            (
                offset_of!(MeshInstance, source) as u32,
                ipp_core::components::schema::FieldValue::String(std::sync::Arc::<str>::default())
            ),
            (
                offset_of!(MeshInstance, variant) as u32,
                ipp_core::components::schema::FieldValue::U32(0)
            ),
        ]
    );
}

#[test]
fn render_order_is_stable_after_reuse_and_shared_mesh_selection_is_per_entity() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    let first = renderable(&mut world);
    upload(&mut world, key(2, 0));
    world.update_for_test(0.0).unwrap();
    let report = ok(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata::default(),
                adopt: false,
            },
            insert(
                EntityRef::Alias(1),
                ComponentValue::TRANSFORM,
                vec![f32_field(0, 2.0)],
            ),
            insert(
                EntityRef::Alias(1),
                ComponentValue::UNLIT_MATERIAL,
                vec![f32_field(0, 0.2)],
            ),
            insert(
                EntityRef::Alias(1),
                ComponentValue::MESH_INSTANCE,
                mesh_fields(key(2, 0)),
            ),
        ],
    );
    let second = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert_eq!(
        world
            .render_items()
            .iter()
            .map(|item| selection(&world, item.mesh))
            .collect::<Vec<_>>(),
        vec![key(1, 0), key(2, 0)]
    );
    let report = ok(
        &mut world,
        vec![
            Command::Delete {
                entity: EntityRef::Handle(first),
            },
            Command::Create {
                alias: 1,
                metadata: EntityMetadata::default(),
                adopt: false,
            },
            insert(EntityRef::Alias(1), ComponentValue::TRANSFORM, vec![]),
            insert(EntityRef::Alias(1), ComponentValue::UNLIT_MATERIAL, vec![]),
            insert(
                EntityRef::Alias(1),
                ComponentValue::MESH_INSTANCE,
                mesh_fields(key(2, 0)),
            ),
        ],
    );
    let replacement = report.outcomes[0].result.as_ref().unwrap()[0].1;
    let mut expected = vec![second, replacement];
    expected.sort();
    assert_eq!(
        world
            .render_items()
            .iter()
            .map(|item| item.entity)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(world.render_items().len(), 2);
}

#[test]
fn retained_mesh_storage_grows_beyond_former_byte_quota() {
    let mut bytes = b"IPPM".to_vec();
    for value in [1u32, 43000, 3] {
        bytes.extend(value.to_le_bytes());
    }
    let triangle = mesh_bytes();
    bytes.extend(&triangle[16..88]);
    bytes.resize(16 + 43000 * 24, 0);
    bytes.extend(&triangle[88..]);
    bytes.shrink_to_fit();
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    for asset_id in 1..=16 {
        world
            .enqueue_mesh(MeshUpload {
                id: asset_id,
                key: key(asset_id, 0),
                bytes: bytes.clone(),
            })
            .unwrap();
        assert!(asset_report(&mut world).assets[0].result.is_ok());
    }
    world
        .enqueue_mesh(MeshUpload {
            id: 17,
            key: key(17, 0),
            bytes,
        })
        .unwrap();
    assert!(asset_report(&mut world).assets[0].result.is_ok());
    // The seventeenth megabyte-sized payload crossed the old retained-byte quota.
    for asset_id in 1..=17 {
        assert_eq!(world.mesh(key(asset_id, 0)).unwrap().vertex_count(), 43000);
    }
    upload(&mut world, key(18, 0));
    assert!(asset_report(&mut world).assets[0].result.is_ok());
}

fn asset_report(world: &mut ipp_core::WorldContext<'_>) -> WorldUpdateReport {
    for _ in 0..512 {
        let report = world.update_for_test(0.0).unwrap();
        if !report.assets.is_empty() {
            return report;
        }
    }
    panic!("asset loading did not finish");
}
