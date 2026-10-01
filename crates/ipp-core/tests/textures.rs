//! Core mutation/ownership evidence; transport and captured-frame evidence belongs
//! to the maintained browser/native rendering harness run by the integrator.

mod support;
use support::WorldTestDriver;
use support::selection::RENDER;

use ipp_core::{
    AssetResourceKind, AssetResourceStatus, Batch, Command, ComponentValue, EntityId,
    EntityMetadata, EntityRef, ErrorReason, FieldValue, FieldWrite, MeshKey, MeshUpload,
    TextureKey, TextureUpload, WorldUpdateReport, components::UnlitTexture,
};

fn test_world(host: &mut ipp_core::HostRuntime) -> ipp_core::WorldContext<'_> {
    let world_id = host
        .create_world(ipp_core::WorldLimits::default(), RENDER)
        .unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world.register_stream_resource_provider("http").unwrap();
    world
}

use std::mem::offset_of;

fn texture_selection(world: &ipp_core::WorldContext<'_>, key: TextureKey) -> TextureKey {
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
    TextureKey {
        asset: name,
        variant: key.variant,
    }
}

fn key(asset_id: u64, variant: u32) -> TextureKey {
    TextureKey {
        asset: asset_id,
        variant,
    }
}

fn mesh_key(uv: bool) -> MeshKey {
    MeshKey {
        asset: 17,

        variant: u32::from(uv),
    }
}

fn pixels(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + width as usize * height as usize * 4);
    bytes.extend(b"IPPT");
    for value in [3, width, height] {
        bytes.extend(value.to_le_bytes());
    }
    for _ in 0..width * height {
        bytes.extend([32, 64, 128, 255]);
    }
    bytes
}

fn mesh_bytes(uv: bool) -> Vec<u8> {
    let mut bytes = b"IPPM".to_vec();
    for value in [
        if uv {
            2u32
        } else {
            1
        },
        3,
        3,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    for (position, coords) in [
        ([0.0f32, 0.0, 0.0], [-0.5f32, 2.0]),
        ([1.0, 0.0, 0.0], [1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 1.0]),
    ] {
        for value in position.into_iter().chain([1.0, 1.0, 1.0]) {
            bytes.extend(value.to_le_bytes());
        }
        if uv {
            for value in coords {
                bytes.extend(value.to_le_bytes());
            }
        }
    }
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}

fn upload(world: &mut ipp_core::WorldContext<'_>, key: TextureKey, bytes: Vec<u8>) {
    world
        .enqueue_texture(TextureUpload {
            id: 23,
            key,
            bytes,
        })
        .unwrap();
}

fn run(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: 1,
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
            .complete_resource(
                request.id,
                Ok(match request.kind {
                    ipp_core::AssetResourceKind::Mesh => mesh_bytes(request.variant != 0),
                    ipp_core::AssetResourceKind::Texture => pixels(1, 1),
                    _ => panic!("unexpected fixture kind"),
                }),
            )
            .unwrap();
    }
    world.update_for_test(0.0).unwrap();
    report
}

fn fields(key: TextureKey) -> Vec<FieldWrite> {
    vec![
        FieldWrite {
            offset: offset_of!(UnlitTexture, source) as u32,
            value: FieldValue::String(std::sync::Arc::<str>::from(format!(
                "http://fixture/{}",
                key.asset
            ))),
        },
        FieldWrite {
            offset: offset_of!(UnlitTexture, variant) as u32,
            value: FieldValue::U32(key.variant),
        },
    ]
}

fn insert(entity: EntityId, component: u16, fields: Vec<FieldWrite>) -> Command {
    Command::InsertComponent {
        entity: EntityRef::Handle(entity),
        component,
        fields,
        adopt: false,
    }
}

fn mesh_fields(uv: bool) -> Vec<FieldWrite> {
    let key = mesh_key(uv);
    fields(TextureKey {
        asset: key.asset,

        variant: key.variant,
    })
}

fn renderable(world: &mut ipp_core::WorldContext<'_>, uv: bool) -> EntityId {
    for uv in [false, true] {
        world
            .enqueue_mesh(MeshUpload {
                id: 1,
                key: mesh_key(uv),
                bytes: mesh_bytes(uv),
            })
            .unwrap();
    }
    upload(world, key(1, 0), pixels(2, 1));
    upload(world, key(2, 1), pixels(1, 2));
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
    let id = report.outcomes[0].result.as_ref().unwrap()[0].1;
    ok(
        world,
        vec![
            insert(id, ComponentValue::TRANSFORM, vec![]),
            insert(id, ComponentValue::UNLIT_MATERIAL, vec![]),
            insert(id, ComponentValue::MESH_INSTANCE, mesh_fields(uv)),
        ],
    );
    id
}

#[test]
fn boundary_publishes_exact_owned_assets_before_batches_and_preserves_old_pixels() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    let id = renderable(&mut world, true);
    let old = world.texture(key(1, 0)).unwrap() as *const _;
    let mut producer = pixels(1, 1);
    upload(&mut world, key(3, 1), producer.clone());
    producer[16..].fill(0);
    let report = ok(
        &mut world,
        vec![insert(id, ComponentValue::UNLIT_TEXTURE, fields(key(3, 1)))],
    );
    let outcome = &report.assets[0];
    assert_eq!(
        (
            outcome.id,
            outcome.key.asset,
            outcome.key.variant,
            outcome.tick
        ),
        (23, 3, 1, report.tick)
    );
    let stats = outcome.result.as_ref().copied().unwrap();
    assert_eq!((stats.source_bytes, stats.resident_bytes), (20, 4));
    assert_eq!(
        world.render_items()[0]
            .texture
            .map(|key| texture_selection(&world, key)),
        Some(key(3, 1))
    );
    assert_eq!(
        world.texture(key(3, 1)).unwrap().pixels(),
        &[32, 64, 128, 255]
    );
    assert_eq!(world.texture(key(1, 0)).unwrap() as *const _, old);
    assert!(world.mesh(mesh_key(true)).is_some());

    upload(&mut world, key(3, 1), producer.clone());
    upload(&mut world, key(2, 7), producer.clone());
    upload(&mut world, key(3, 2), producer);
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.assets[0].result, Err("DuplicateAsset".into()));
    assert!(report.assets[1].result.is_ok());
    assert!(report.assets[2].result.is_ok());
    assert_eq!(
        world.texture(key(3, 1)).unwrap().pixels(),
        &[32, 64, 128, 255]
    );
    assert!(
        test_world(&mut ipp_core::HostRuntime::new())
            .texture(key(3, 1))
            .is_none()
    );
}

#[test]
fn malformed_and_version_one_payloads_do_not_publish_and_all_rgba_channels_are_retained() {
    let valid = pixels(1, 1);
    let mut invalid = vec![vec![], valid[..15].to_vec(), valid[..18].to_vec()];
    let mut trailing = valid.clone();
    trailing.push(0);
    invalid.push(trailing);
    for (offset, value) in [
        (0, *b"NOPE"),
        (4, 1u32.to_le_bytes()),
        (4, 2u32.to_le_bytes()),
        (8, 0u32.to_le_bytes()),
        (12, 0u32.to_le_bytes()),
        (8, 1025u32.to_le_bytes()),
        (12, u32::MAX.to_le_bytes()),
    ] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(&value);
        invalid.push(bytes);
    }
    let mut oversized = valid.clone();
    oversized[8..12].copy_from_slice(&1024u32.to_le_bytes());
    oversized[12..16].copy_from_slice(&1024u32.to_le_bytes());
    invalid.push(oversized);
    for bytes in invalid {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let mut world = test_world(&mut fixture_host);
        upload(&mut world, key(2, 0), bytes);
        assert_eq!(
            asset_report(&mut world).assets[0].result,
            Err("InvalidAsset".into())
        );
        assert!(world.texture(key(2, 0)).is_none());
        upload(&mut world, key(1, 0), valid.clone());
        assert!(asset_report(&mut world).assets[0].result.is_ok());
    }
    for key in [
        TextureKey {
            asset: 0,
            ..key(1, 0)
        },
        key(0, 0),
    ] {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let mut world = test_world(&mut fixture_host);
        upload(&mut world, key, valid.clone());
        assert_eq!(
            asset_report(&mut world).assets[0].result,
            Err("InvalidAsset".into())
        );
    }

    let mut bytes = pixels(2, 1);
    bytes[16..].copy_from_slice(&[0, 255, 42, 31, 255, 0, 17, 127]);
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    upload(&mut world, key(1, 0), bytes.clone());
    assert!(asset_report(&mut world).assets[0].result.is_ok());
    let texture = world.texture(key(1, 0)).unwrap();
    assert_eq!((texture.width(), texture.height()), (2, 1));
    assert_eq!(texture.pixels(), &bytes[16..]);
}

#[test]
fn queued_uploads_and_asset_count_grow_past_former_quotas() {
    let mut host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut host);
    for asset in 1..=300 {
        let mut bytes = pixels(1, 1);
        bytes.reserve_exact(1024);
        upload(&mut world, key(asset, 0), bytes);
    }
    assert_eq!(
        world.update_for_test(f64::NAN),
        Err(ErrorReason::InvalidValue)
    );
    assert!(world.texture(key(1, 0)).is_none());
    let report = asset_report(&mut world);
    assert_eq!(report.assets.len(), 300);
    assert!(report.assets.iter().all(|outcome| outcome.result.is_ok()));
    for asset in 1..=300 {
        assert_eq!(
            world.texture(key(asset, 0)).unwrap().pixels(),
            &[32, 64, 128, 255]
        );
    }
}

#[test]
fn texture_input_and_residency_grow_past_former_byte_quotas() {
    let mut host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut host);
    let payload = pixels(3072, 2048);
    assert!(payload.len() > 16 << 20);
    for asset in 1..=2 {
        upload(&mut world, key(asset, 0), payload.clone());
    }
    let report = asset_report(&mut world);
    assert_eq!(report.assets.len(), 2);
    assert!(report.assets.iter().all(|outcome| outcome.result.is_ok()));
    assert_eq!(
        world.asset_resources().resident_bytes(),
        2 * 3072 * 2048 * 4
    );
    for asset in 1..=2 {
        let texture = world.texture(key(asset, 0)).unwrap();
        assert_eq!((texture.width(), texture.height()), (3072, 2048));
        assert_eq!(texture.pixels(), &payload[16..]);
    }
}

#[test]
fn source_texture_terminal_changes_publish_once() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    let id = renderable(&mut world, true);
    let report = run(
        &mut world,
        vec![insert(
            id,
            ComponentValue::UNLIT_TEXTURE,
            fields(key(99, 0)),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    let request = world.resource_requests_for_test().pop().unwrap();
    assert_eq!(request.kind, AssetResourceKind::Texture);
    let started = world.take_asset_events().unwrap();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].status, AssetResourceStatus::Start);
    world
        .complete_resource(request.id, Ok(pixels(3, 1)))
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.resource_changes.len(), 2);
    assert!(matches!(
        report.resource_changes[0].status,
        AssetResourceStatus::Progress { .. }
    ));
    assert_eq!(report.resource_changes[1].source, request.source);
    assert_eq!(
        report.resource_changes[1].status,
        AssetResourceStatus::Loaded
    );
    assert!(
        world
            .update_for_test(0.0)
            .unwrap()
            .resource_changes
            .is_empty()
    );

    let report = run(
        &mut world,
        vec![insert(
            id,
            ComponentValue::UNLIT_TEXTURE,
            fields(key(100, 0)),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    let request = world.resource_requests_for_test().pop().unwrap();
    let started = world.take_asset_events().unwrap();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].status, AssetResourceStatus::Start);
    world
        .complete_resource(request.id, Err("fixture decode failed".into()))
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.resource_changes.len(), 1);
    assert_eq!(
        report.resource_changes[0].status,
        AssetResourceStatus::Failed("fixture decode failed".into())
    );
    assert!(
        world
            .update_for_test(0.0)
            .unwrap()
            .resource_changes
            .is_empty()
    );
}

#[test]
fn mesh_streams_and_retained_metadata_are_accounted_and_uvs_are_finite_and_separate() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    for uv in [false, true] {
        world
            .enqueue_mesh(MeshUpload {
                id: 1,
                key: mesh_key(uv),
                bytes: mesh_bytes(uv),
            })
            .unwrap();
    }
    let report = world.update_for_test(0.0).unwrap();
    for (i, expected) in [(94, 78), (118, 102)].into_iter().enumerate() {
        let stats = report.assets[i].result.as_ref().copied().unwrap();
        assert_eq!(
            (stats.source_bytes, stats.resident_bytes),
            (
                expected.0,
                expected.1 + support::unskinned_mesh_metadata_bytes(3)
            )
        );
    }
    assert_eq!(
        world.mesh(mesh_key(false)).unwrap().positions(),
        world.mesh(mesh_key(true)).unwrap().positions()
    );
    assert_eq!(
        world.mesh(mesh_key(false)).unwrap().colors(),
        world.mesh(mesh_key(true)).unwrap().colors()
    );
    assert!(
        world
            .mesh(mesh_key(false))
            .unwrap()
            .texture_weights()
            .is_none()
    );
    assert!(
        world
            .mesh(mesh_key(true))
            .unwrap()
            .texture_weights()
            .is_none()
    );
    assert!(world.mesh(mesh_key(false)).unwrap().uvs().is_none());
    assert_eq!(
        world.mesh(mesh_key(true)).unwrap().uvs().unwrap()[0],
        [-0.5, 2.0]
    );
    for offset in [40, 44] {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut bytes = mesh_bytes(true);
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            let mut fixture_host = ipp_core::HostRuntime::new();
            let mut world = test_world(&mut fixture_host);
            world
                .enqueue_mesh(MeshUpload {
                    id: 1,
                    key: mesh_key(true),
                    bytes,
                })
                .unwrap();
            assert_eq!(
                asset_report(&mut world).assets[0].result,
                Err("InvalidAsset".into())
            );
        }
    }
}

#[test]
fn texture_activation_is_pending_and_uv_incompatibility_is_per_use() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    let id = renderable(&mut world, false);
    let report = run(
        &mut world,
        vec![insert(
            id,
            ComponentValue::UNLIT_TEXTURE,
            fields(key(99, 0)),
        )],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert!(world.render_items().is_empty());
    assert_eq!(world.render_diagnostics()[0].entity, id);
    assert_eq!(
        world.render_diagnostics()[0].reason,
        ErrorReason::InvalidAsset
    );
    ok(
        &mut world,
        vec![insert(id, ComponentValue::MESH_INSTANCE, mesh_fields(true))],
    );
    assert_eq!(world.render_items().len(), 1);
    assert!(world.render_diagnostics().is_empty());
    ok(
        &mut world,
        vec![insert(
            id,
            ComponentValue::MESH_INSTANCE,
            mesh_fields(false),
        )],
    );
    assert!(world.render_items().is_empty());
    assert_eq!(world.render_diagnostics().len(), 1);
    ok(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(id),
            component: ComponentValue::UNLIT_TEXTURE,
        }],
    );
    assert_eq!(world.render_items()[0].texture, None);
}

#[test]
fn deleted_texture_slots_do_not_leak_into_reused_entities() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let mut world = test_world(&mut fixture_host);
    let id = renderable(&mut world, true);
    ok(
        &mut world,
        vec![
            insert(id, ComponentValue::UNLIT_TEXTURE, fields(key(1, 0))),
            Command::Delete {
                entity: EntityRef::Handle(id),
            },
        ],
    );
    let report = ok(
        &mut world,
        vec![Command::Create {
            alias: 0,
            metadata: EntityMetadata::default(),
            adopt: false,
        }],
    );
    let next = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert_eq!(id.index(), next.index());
    assert!(world.inspect(next).unwrap().components.is_empty());
    ok(
        &mut world,
        vec![
            insert(next, ComponentValue::TRANSFORM, vec![]),
            insert(next, ComponentValue::UNLIT_MATERIAL, vec![]),
            insert(next, ComponentValue::MESH_INSTANCE, mesh_fields(false)),
        ],
    );
    assert_eq!(world.render_items()[0].texture, None);
    assert!(world.texture(key(1, 0)).is_some());
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
