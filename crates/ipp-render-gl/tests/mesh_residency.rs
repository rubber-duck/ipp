//! Focused failure injection for real resource/renderer policies; GL harnesses own image evidence.

mod support;

#[cfg(feature = "shadows")]
use ipp_core::{Batch, Command, EntityRef};
use ipp_core::{
    ComponentValue, EntityId, MeshAsset, MeshKey, MeshUpload, WorldContext,
    components::{MeshInstance, Transform, UnlitMaterial},
    services::asset_management::{
        Asset, AssetLoadStatus, AssetTypeId, AssetUploadIdentity, BufferedAssetLoader,
    },
};
#[cfg(feature = "gui")]
use ipp_render_gl::RenderError;
use ipp_render_gl::{RenderDevice, RenderService};
use std::{any::Any, rc::Rc};
use support::*;

fn triangle() -> Vec<u8> {
    let mut bytes = b"IPPM".to_vec();
    for value in [1u32, 3, 3] {
        bytes.extend(value.to_le_bytes());
    }
    for position in [[-1.0f32, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in position.into_iter().chain([1.0, 1.0, 1.0]) {
            bytes.extend(value.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}

fn renderable(world: &mut WorldContext<'_>, asset: u64, x: f32) -> EntityId {
    let values = vec![
        ComponentValue::MeshInstance(MeshInstance {
            source: format!("asset://1/{asset}"),
            variant: 0,
        }),
        ComponentValue::Transform(Transform {
            x,
            ..Transform::default()
        }),
        ComponentValue::UnlitMaterial(UnlitMaterial::default()),
    ];
    let values = {
        let mut values = values;
        values.push(ComponentValue::PickingGeometry(
            ipp_core::components::PickingGeometry::default(),
        ));
        values
    };
    create(world, values)
}

fn upload(world: &mut WorldContext<'_>, asset: u64) {
    world
        .enqueue_mesh(MeshUpload {
            id: asset + 100,
            key: MeshKey {
                asset,
                variant: 0,
            },
            bytes: triangle(),
        })
        .unwrap();
    update(world).unwrap();
}

fn assert_cpu_usable(world: &mut WorldContext<'_>, asset: u64, _entity: EntityId) {
    let key = MeshKey {
        asset,
        variant: 0,
    };
    assert!(
        world.mesh(key).is_none(),
        "GPU meshes release bulk CPU streams"
    );
    assert_eq!(world.mesh_metadata(key).unwrap().vertex_count(), 3);
    {
        world
            .enqueue_geometry_pick(
                99,
                ipp_core::GeometryPickQuery {
                    x: 0.5,
                    y: 0.5,
                    width: 100,
                    height: 100,
                    include_view_plane: false,
                },
            )
            .unwrap();
        let report = update(world).unwrap();
        let hit = report.geometry_picks[0].result.unwrap().unwrap();
        assert_eq!((hit.entity, hit.part, hit.distance), (_entity, 0, 5.0));
    }
}

#[test]
fn frame_accounting_reports_between_frame_uploads_once() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut world, mut renderer, _) = setup(&mut host);
    upload(&mut world, 1);

    assert_eq!(renderer.begin_frame(), 78);
    assert_eq!(renderer.begin_frame(), 0);
}

#[test]
fn failed_shared_upload_is_cached_preserves_cpu_and_does_not_block_ready_draws() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut world, mut renderer, state) = setup(&mut host);
    state.failed_attempts_remaining.set(1);
    upload(&mut world, 1);
    upload(&mut world, 2);
    let first = renderable(&mut world, 1, 0.0);
    // Keep both failure-injection cases inside the camera frustum so this
    // checks resource readiness independently of spatial culling.
    renderable(&mut world, 1, -2.0);
    renderable(&mut world, 2, 2.0);
    assert_eq!(
        state.mesh_attempts.get(),
        2,
        "loading attempts each shared mesh before any draw"
    );

    let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((stats.draw_calls, stats.failed_draw_calls), (1, 2));
    assert_eq!((state.mesh_attempts.get(), state.live_meshes.get()), (2, 1));
    assert_eq!(
        non_program_residency(&world),
        2 * metadata_bytes() + 78,
        "compact CPU metadata and one successful GPU allocation"
    );
    let failed = world
        .resolve_asset_key(AssetUploadIdentity {
            kind: ipp_core::MESH_TYPE,
            asset: 1,
            variant: 0,
        })
        .unwrap();
    assert!(matches!(
        world.asset_resources().get(failed).unwrap().status(),
        AssetLoadStatus::Failed(_)
    ));
    assert_cpu_usable(&mut world, 1, first);
    for _ in 0..3 {
        let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
        assert_eq!(
            (
                stats.draw_calls,
                stats.failed_draw_calls,
                stats.uploaded_bytes
            ),
            (1, 2, 0)
        );
    }
    assert_eq!(
        state.mesh_attempts.get(),
        2,
        "same failed shared resource must not retry per instance/frame"
    );
    assert_eq!(
        state.ended_frames.get(),
        5,
        "the initial program preparation frame and ready frames all complete"
    );

    let world_id = world.id();
    drop(world);
    renderer.unload_host(&mut host);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(world_id).unwrap();
    assert_eq!(state.live_meshes.get(), 0);
    let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((stats.draw_calls, stats.failed_draw_calls), (3, 0));
    assert_eq!((state.mesh_attempts.get(), state.live_meshes.get()), (4, 2));
    drop(world);
    renderer.unload_host(&mut host);
    host.flush_resource_lifecycle();
    assert_eq!(state.live_meshes.get(), 0);
}

struct Blob(Vec<u8>);

impl Asset for Blob {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        self.0.len()
    }
}

#[test]
fn cpu_residency_above_former_quota_does_not_suppress_mesh_upload() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut world, mut renderer, state) = setup(&mut host);
    let bytes = 17 << 20;
    let blob = AssetUploadIdentity {
        kind: AssetTypeId(42),
        asset: 7,
        variant: 0,
    };
    world
        .asset_resources_mut()
        .register_loader(blob.kind, move || {
            BufferedAssetLoader::new(move |_| Ok(Blob(vec![0; bytes])))
        })
        .unwrap();
    let blob = world.asset_resources_mut().upload(blob, vec![0]).unwrap();
    upload(&mut world, 1);
    let entity = renderable(&mut world, 1, 0.0);

    let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(
        (
            stats.draw_calls,
            stats.failed_draw_calls,
            stats.uploaded_bytes
        ),
        (1, 0, 0)
    );
    assert_eq!(state.mesh_attempts.get(), 1);
    assert_eq!(non_program_residency(&world), bytes + metadata_bytes() + 78);
    assert_cpu_usable(&mut world, 1, entity);

    world.asset_resources_mut().release(blob);
    let world_id = world.id();
    drop(world);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(world_id).unwrap();
    let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(
        (stats.draw_calls, stats.failed_draw_calls),
        (1, 0),
        "releasing other CPU data preserves the loaded mesh"
    );
    assert_eq!(state.mesh_attempts.get(), 1);
    let world_id = world.id();
    drop(world);
    renderer.unload_host(&mut host);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(world_id).unwrap();
    let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((stats.draw_calls, stats.failed_draw_calls), (1, 0));
    assert_eq!(state.mesh_attempts.get(), 2);
    assert_eq!(non_program_residency(&world), metadata_bytes() + 78);
}

#[test]
fn detached_demanded_mesh_publishes_cpu_metadata_and_requeues_gpu_recovery() {
    use ipp_core::services::asset_management::{AssetSource, mesh_metadata::MeshMetadata};

    let mut host = ipp_core::HostRuntime::new();
    host.register_stream_resource_provider("https").unwrap();
    let (mut world, mut renderer, state) = setup(&mut host);
    renderer.set_asset_context_active(false);
    let source = "https://fixture/detached-picking.ippm";
    create(
        &mut world,
        vec![
            ComponentValue::MeshInstance(MeshInstance {
                source: source.into(),
                variant: 0,
            }),
            ComponentValue::PickingGeometry(ipp_core::components::PickingGeometry::default()),
            ComponentValue::Transform(Transform::default()),
        ],
    );
    drop(world);
    host.progress_evaluation_assets();
    let request = host
        .take_resource_requests()
        .into_iter()
        .find(|request| request.source == source)
        .expect("demanded mesh request");
    host.complete_resource(request.id, Ok(triangle())).unwrap();

    host.progress_evaluation_assets();

    let key = host
        .asset_resources()
        .find(&AssetSource {
            kind: ipp_core::MESH_TYPE,
            uri: source.into(),
            variant: 0,
        })
        .unwrap();
    let resource = host.asset_resources().get(key).unwrap();
    assert_eq!(resource.status(), &AssetLoadStatus::Loaded);
    assert!(resource.representation().decoded);
    assert_eq!(resource.representation().graphics_ready, Some(false));
    assert!(
        host.asset_resources()
            .get_typed::<MeshMetadata>(key)
            .is_some()
    );
    assert_eq!(state.mesh_attempts.get(), 0);

    renderer.unload_host(&mut host);
    host.flush_resource_lifecycle();
    assert!(
        host.asset_resources()
            .get_typed::<MeshMetadata>(key)
            .is_some(),
        "context invalidation retains evaluation metadata"
    );
    renderer.set_asset_context_active(true);
    host.progress_assets();
    let recovery = host.take_resource_requests().pop().unwrap();
    assert!(recovery.recovery);
    host.complete_resource(recovery.id, Ok(triangle())).unwrap();
    host.progress_assets();

    assert_eq!(state.mesh_attempts.get(), 1);
    let resource = host.asset_resources().get(key).unwrap();
    assert_eq!(resource.status(), &AssetLoadStatus::Loaded);
    assert_eq!(resource.representation().graphics_ready, Some(true));
}

#[cfg(feature = "surfaces")]
#[test]
fn detached_progress_defers_pending_font_and_drawing_gpu_preparation() {
    use ipp_core::services::asset_management::{
        AssetLoadStatus, drawing::DRAWING_TYPE, font::FONT_TYPE,
    };

    let mut host = ipp_core::HostRuntime::new();
    let state = Rc::new(DeviceState::default());
    let renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(&mut host).unwrap();
    let font = host
        .asset_resources_mut()
        .upload(
            AssetUploadIdentity {
                kind: FONT_TYPE,
                asset: 1,
                variant: 0,
            },
            surface_font(),
        )
        .unwrap();
    let drawing = host
        .asset_resources_mut()
        .upload(
            AssetUploadIdentity {
                kind: DRAWING_TYPE,
                asset: 2,
                variant: 0,
            },
            surface_drawing(),
        )
        .unwrap();
    state.context_lost.set(true);

    host.progress_assets();
    assert_eq!(state.surface_path_attempts.get(), 2);
    for _ in 0..4 {
        host.progress_evaluation_assets();
    }
    assert_eq!(
        state.surface_path_attempts.get(),
        2,
        "detached progress must not retry graphics-owned Surface preparation"
    );
    assert_ne!(
        host.asset_resources().get(font).unwrap().status(),
        &AssetLoadStatus::Loaded
    );
    assert_ne!(
        host.asset_resources().get(drawing).unwrap().status(),
        &AssetLoadStatus::Loaded
    );

    state.context_lost.set(false);
    host.progress_assets();
    assert_eq!(state.surface_path_attempts.get(), 4);
    assert_eq!(
        host.asset_resources().get(font).unwrap().status(),
        &AssetLoadStatus::Loaded
    );
    assert_eq!(
        host.asset_resources().get(drawing).unwrap().status(),
        &AssetLoadStatus::Loaded
    );
}

#[cfg(feature = "surfaces")]
#[test]
fn surface_draw_failure_restores_mesh_culling_state() {
    use ipp_core::services::asset_management::{AssetSource, drawing::DRAWING_TYPE};
    use ipp_core::{Surface, SurfaceItemContent, SurfaceItemStyle};

    let mut host = ipp_core::HostRuntime::new();
    host.data_sources_mut()
        .register_stream("fixture://")
        .unwrap();
    let (mut world, mut renderer, state) = setup(&mut host);
    renderable(&mut world, 41, 0.0);
    upload(&mut world, 41);
    let world_id = world.id();
    let mut surface = Surface::default();
    surface
        .insert_item(
            0,
            SurfaceItemContent::Drawing,
            SurfaceItemStyle {
                asset: Some(AssetSource {
                    kind: DRAWING_TYPE,
                    uri: "fixture:///drawing.ippd".into(),
                    variant: 0,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Surface(surface),
        ],
    );
    drop(world);
    for _ in 0..16 {
        host.progress_assets();
        for request in host.take_resource_requests() {
            host.complete_resource(request.id, Ok(surface_drawing()))
                .unwrap();
        }
        update(&mut host.world_mut(world_id).unwrap()).unwrap();
        if host.world_mut(world_id).unwrap().surface_render_items()[0]
            .primitives
            .len()
            == 1
        {
            break;
        }
    }
    let mut world = host.world_mut(world_id).unwrap();
    assert_eq!(world.surface_render_items().len(), 1);
    assert_eq!(world.surface_render_items()[0].primitives.len(), 1);

    state.fail_surface_state_start.set(true);
    let error = render_frame(&mut renderer, &mut world, 100, 100).unwrap_err();
    assert!(error.contains("injected Surface state failure"), "{error}");
    assert!(!state.surface_double_sided.get());
    assert_eq!(&*state.surface_state_changes.borrow(), &[true, false]);
    assert_eq!(state.ended_frames.get(), 1);

    state.fail_surface_state_start.set(false);
    state.surface_state_changes.borrow_mut().clear();
    state.fail_surface_draw.set(true);
    let error = render_frame(&mut renderer, &mut world, 100, 100).unwrap_err();
    assert!(error.contains("injected Surface draw failure"), "{error}");
    assert!(!state.surface_double_sided.get());
    assert_eq!(&*state.surface_state_changes.borrow(), &[true, false]);
    assert_eq!(state.ended_frames.get(), 2);

    state.fail_surface_draw.set(false);
    let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(stats.draw_calls, 2);
    assert_eq!(
        &*state.surface_state_changes.borrow(),
        &[true, false, true, false]
    );
    assert_eq!(state.ended_frames.get(), 3);
    assert!(!state.surface_double_sided.get());
}

#[cfg(feature = "shadows")]
#[test]
fn shadow_pass_restores_target_after_error_reuses_storage_and_releases_on_unload() {
    use ipp_core::components::{Light, PbrMaterial};
    let mut host = ipp_core::HostRuntime::new();
    let (mut world, mut renderer, state) = setup(&mut host);
    create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::MeshInstance(MeshInstance {
                source: "asset://1/22".into(),
                variant: 0,
            }),
            ComponentValue::PbrMaterial(PbrMaterial::default()),
        ],
    );
    let spot = create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform {
                z: 4.0,
                ..Transform::default()
            }),
            ComponentValue::Light(Light {
                kind: 2,
                cast_shadows: true,
                ..Light::default()
            }),
        ],
    );
    upload(&mut world, 22);
    let world_id = world.id();
    drop(world);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(world_id).unwrap();
    state.fail_shadow_draw.set(true);
    assert!(render_frame(&mut renderer, &mut world, 100, 100).is_err());
    assert!(
        !state.shadow_pass.get(),
        "Failed depth draw must leave the shadow target"
    );
    assert_eq!(state.live_shadow_maps.get(), 1);
    assert_eq!(state.ended_frames.get(), 2);
    state.fail_shadow_draw.set(false);
    let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(stats.shadow_draw_calls, 1);
    assert_eq!(stats.shadow_resident_bytes, 4 * 1024 * 1024);
    assert_eq!(state.live_shadow_maps.get(), 1);
    let world_id = world.id();
    drop(world);
    renderer.unload_host(&mut host);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(world_id).unwrap();
    assert_eq!(state.live_shadow_maps.get(), 0);
    // Removing the sole caster light also releases a subsequently recreated map.
    upload(&mut world, 22);
    let world_id = world.id();
    drop(world);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(world_id).unwrap();
    render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    world
        .enqueue(Batch {
            id: 99,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(spot),
            }],
        })
        .unwrap();
    update(&mut world).unwrap();
    render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(state.live_shadow_maps.get(), 0);
}

#[test]
fn excess_lights_keep_drawing_with_per_draw_shader_capacity() {
    use ipp_core::components::{Light, PbrMaterial};
    let mut host = ipp_core::HostRuntime::new();
    let (mut world, mut renderer, _) = setup(&mut host);
    create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::MeshInstance(MeshInstance {
                source: "asset://1/23".into(),
                variant: 0,
            }),
            ComponentValue::PbrMaterial(PbrMaterial::default()),
        ],
    );
    upload(&mut world, 23);
    for _ in 0..8 {
        create(
            &mut world,
            vec![
                ComponentValue::Transform(Transform::default()),
                ComponentValue::Light(Light::default()),
            ],
        );
    }
    assert_eq!(
        render_frame(&mut renderer, &mut world, 100, 100)
            .unwrap()
            .draw_calls,
        1
    );
    create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Light(Light::default()),
        ],
    );
    assert_eq!(
        render_frame(&mut renderer, &mut world, 100, 100)
            .unwrap()
            .draw_calls,
        1
    );
}

#[test]
fn device_replacement_releases_old_payloads_before_swapping_and_waits_for_worlds() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut world, mut renderer, old) = setup(&mut host);
    upload(&mut world, 1);
    renderable(&mut world, 1, 0.0);
    assert_eq!(
        render_frame(&mut renderer, &mut world, 100, 100)
            .unwrap()
            .draw_calls,
        1
    );
    assert_eq!(old.live_meshes.get(), 1);
    let world_id = world.id();
    drop(world);

    let peer = host.create_world(Default::default()).unwrap();
    host.world_mut(peer).unwrap().prepare_update(0.0).unwrap();
    let next = Rc::new(DeviceState::default());
    assert!(
        renderer
            .replace_device(&mut host, TestDevice(next.clone()))
            .is_err()
    );
    assert_eq!(
        old.live_meshes.get(),
        1,
        "a prepared peer must defer the entire transition"
    );
    assert_eq!(next.live_meshes.get(), 0);
    host.world_mut(peer).unwrap().step(0.0).unwrap();

    renderer
        .replace_device(&mut host, TestDevice(next.clone()))
        .unwrap();
    assert_eq!(
        old.live_meshes.get(),
        0,
        "old GPU handles must be destroyed by the old device"
    );
    assert_eq!(
        next.live_meshes.get(),
        0,
        "the new device cannot receive old-handle destruction"
    );
    let mut world = host.world_mut(world_id).unwrap();
    let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((stats.draw_calls, stats.failed_draw_calls), (1, 0));
    assert_eq!(old.mesh_attempts.get(), 1);
    assert_eq!((next.mesh_attempts.get(), next.live_meshes.get()), (1, 1));
}

fn metadata_bytes() -> usize {
    let (mesh, _) = MeshAsset::decode(&triangle()).unwrap();
    ipp_core::services::asset_management::mesh_metadata::MeshMetadata::from_owned_mesh(mesh)
        .resident_bytes()
}

/// Resource readiness must reach every World through the Host lifecycle boundary.
fn render_host_frame<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut ipp_core::HostRuntime,
    world_id: ipp_core::WorldId,
) -> ipp_render_gl::RenderStats {
    renderer.begin_frame();
    for _ in 0..8 {
        host.world_mut(world_id)
            .unwrap()
            .prepare_update(0.0)
            .unwrap();
        host.progress_assets();
        let mut world = host.world_mut(world_id).unwrap();
        world.step(0.0).unwrap();
        let stats = renderer.render(&mut world, 100, 100).unwrap();
        if !world.asset_resources().iter().any(|resource| {
            resource.source().kind == AssetTypeId(14)
                && *resource.status() == AssetLoadStatus::Unloaded
        }) {
            return stats;
        }
    }
    panic!("built-in program readiness did not settle");
}

fn non_program_residency(world: &WorldContext<'_>) -> usize {
    world
        .asset_resources()
        .iter()
        .filter(|resource| resource.source().kind != AssetTypeId(14))
        .map(|resource| resource.stats().resident_bytes)
        .sum()
}

#[test]
fn graphics_loss_and_failed_recovery_keep_cpu_picking_and_metadata_available() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut world, mut renderer, state) = setup(&mut host);
    let entity = renderable(&mut world, 71, 0.0);
    upload(&mut world, 71);
    let world_id = world.id();
    drop(world);
    assert_eq!(
        render_host_frame(&mut renderer, &mut host, world_id).draw_calls,
        1
    );
    renderer.unload_host(&mut host);
    host.flush_resource_lifecycle();
    let provider = host
        .asset_resources()
        .iter()
        .find(|provider| provider.source().kind == ipp_core::MESH_TYPE)
        .unwrap();
    let key = provider.key();
    assert!(provider.decoded_available());
    assert_eq!(provider.graphics_ready(), Some(false));
    assert_eq!(provider.graphics_bytes(), Some(0));
    assert_eq!(provider.stats().resident_bytes, metadata_bytes());
    state.failed_attempts_remaining.set(1);
    let mut world = host.world_mut(world_id).unwrap();
    assert_cpu_usable(&mut world, 71, entity);
    drop(world);
    let stats = render_host_frame(&mut renderer, &mut host, world_id);
    assert_eq!((stats.draw_calls, stats.failed_draw_calls), (0, 1));
    let mut world = host.world_mut(world_id).unwrap();
    assert_cpu_usable(&mut world, 71, entity);
    let provider = world.asset_resources().get(key).unwrap();
    assert!(matches!(provider.status(), AssetLoadStatus::Failed(_)));
    assert!(provider.decoded_available());
    assert_eq!(provider.stats().resident_bytes, metadata_bytes());
    drop(world);
    renderer.unload_host(&mut host);
    host.flush_resource_lifecycle();
    assert_eq!(
        render_host_frame(&mut renderer, &mut host, world_id).draw_calls,
        1
    );
    let world = host.world_mut(world_id).unwrap();
    assert_eq!(
        world.asset_resources().get(key).unwrap().graphics_ready(),
        Some(true)
    );
}

#[cfg(feature = "shadows")]
#[test]
fn atlas_allocation_failure_keeps_lighting_and_does_not_retry_every_frame() {
    use ipp_core::components::{Light, PbrMaterial};
    let mut host = ipp_core::HostRuntime::new();
    let (mut world, mut renderer, state) = setup(&mut host);
    create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::MeshInstance(MeshInstance {
                source: "asset://1/72".into(),
                variant: 0,
            }),
            ComponentValue::PbrMaterial(PbrMaterial::default()),
        ],
    );
    create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform {
                z: 4.0,
                ..Default::default()
            }),
            ComponentValue::Light(Light {
                kind: 2,
                cast_shadows: true,
                ..Default::default()
            }),
        ],
    );
    upload(&mut world, 72);
    let world_id = world.id();
    drop(world);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(world_id).unwrap();
    state.fail_shadow_allocation.set(true);
    for _ in 0..3 {
        let stats = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
        assert_eq!(
            stats.draw_calls,
            1,
            "{stats:?} {:?}",
            world
                .asset_resources()
                .iter()
                .map(|r| (r.source(), r.status()))
                .collect::<Vec<_>>()
        );
        assert_eq!(stats.shadow_draw_calls, 0);
        assert_eq!(stats.unshadowed_lights, 1);
    }
    assert_eq!(state.shadow_attempts.get(), 1);
    assert_eq!(state.live_shadow_maps.get(), 0);
}

#[cfg(feature = "gui")]
#[test]
fn culled_text_surface_reuses_retained_glyphs_when_visible_again() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = text_surface_scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    let cold = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((cold.glyph_populates, cold.gui_batches), (1, 1), "{cold:?}");
    let uploads = state.glyph_batch_uploads.get();
    let populations = state.atlas_populations.get();

    // Behind the camera for one frame: nothing is submitted and nothing is released.
    place(&mut world, entity, 10.0);
    let culled = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(
        (culled.draw_calls, culled.gui_batches),
        (0, 0),
        "{culled:?}"
    );
    assert_eq!(culled.glyph_pages, 1);
    assert!(culled.gui_resident_bytes > 0);

    place(&mut world, entity, 0.0);
    let visible = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(visible.gui_batches, 1, "{visible:?}");
    assert_eq!(visible.uploaded_bytes, 0);
    assert_eq!(visible.gui_rebuilds, 0);
    assert_eq!(visible.gui_allocations, 0);
    assert_eq!(visible.glyph_misses, 0);
    assert_eq!(visible.glyph_populates, 0);
    assert_eq!(state.glyph_batch_uploads.get(), uploads);
    assert_eq!(state.atlas_populations.get(), populations);
}

#[cfg(feature = "gui")]
#[test]
fn recoverable_glyph_population_failure_keeps_analytic_text_and_backs_off() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = text_surface_scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    state
        .fail_atlas_begin
        .replace(Some(RenderError::RenderDevice(
            "injected atlas failure".into(),
        )));
    let failed = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(failed.glyph_population_failures, 1, "{failed:?}");
    assert_eq!((failed.glyph_populates, failed.gui_batches), (0, 0));
    assert_eq!((failed.draw_calls, failed.failed_draw_calls), (1, 0));
    assert_eq!(state.analytic_glyph_draws.get(), 1);
    assert!(!state.atlas_target_bound.get());

    // The glyph waits before retrying, so a persistent failure is not paid every frame.
    state.fail_atlas_begin.replace(None);
    let waiting = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(waiting.glyph_population_failures, 0);
    assert_eq!(waiting.draw_calls, 1);
    assert_eq!(state.atlas_populations.get(), 1);
    assert_eq!(state.analytic_glyph_draws.get(), 2);

    let populated = (0..8)
        .map(|_| render_frame(&mut renderer, &mut world, 100, 100).unwrap())
        .find(|stats| stats.glyph_populates == 1)
        .expect("backed-off glyph is populated again");
    assert_eq!(populated.gui_batches, 1);
    assert_eq!(state.atlas_populations.get(), 2);
}

#[cfg(feature = "gui")]
#[test]
fn glyph_population_context_loss_and_restore_failures_fail_the_frame() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = text_surface_scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    state
        .fail_atlas_begin
        .replace(Some(RenderError::ContextLost));
    let error = render_frame(&mut renderer, &mut world, 100, 100).unwrap_err();
    assert_eq!(error, RenderError::ContextLost.to_string());
    assert!(!state.atlas_target_bound.get());

    // A failure to restore the host target must fail the frame, not hide as a miss.
    state.fail_atlas_begin.replace(None);
    state.fail_atlas_end.replace(Some(RenderError::RenderDevice(
        "injected atlas restore failure".into(),
    )));
    let attempts = state.atlas_populations.get();
    let error = (0..8)
        .find_map(|_| render_frame(&mut renderer, &mut world, 100, 100).err())
        .expect("retried population reports the restore failure");
    assert!(error.contains("injected atlas restore failure"), "{error}");
    assert_eq!(state.atlas_populations.get(), attempts + 1);
}

#[cfg(feature = "gui")]
#[test]
fn cold_glyph_population_binds_each_atlas_page_once_and_restores_once() {
    let mut host = ipp_core::HostRuntime::new();
    // Each glyph fills more than half a page, so three glyphs need three pages.
    let (mut renderer, state, world_id, _) =
        text_run_scene(&mut host, glyph_font(3, 10, 130.0), &[0, 1, 2]);
    let mut world = host.world_mut(world_id).unwrap();

    let cold = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(
        (cold.glyph_misses, cold.glyph_populates),
        (3, 3),
        "{cold:?}"
    );
    assert_eq!(cold.glyph_pages, 3);
    assert_eq!(
        state.atlas_populations.get(),
        3,
        "one target switch per page"
    );
    assert_eq!(
        state.atlas_restores.get(),
        1,
        "the host target is restored once"
    );
    assert_eq!(cold.gui_batches, 3, "one glyph batch per page");
    assert_eq!(state.analytic_glyph_draws.get(), 0);

    let warm = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((warm.glyph_misses, warm.glyph_populates), (0, 0));
    assert_eq!((warm.uploaded_bytes, warm.gui_rebuilds), (0, 0));
    assert_eq!(warm.gui_batches, 3);
    assert_eq!(state.atlas_populations.get(), 3);
    assert_eq!(state.atlas_restores.get(), 1);
}

#[cfg(feature = "gui")]
#[test]
fn population_budget_defers_misses_and_resumes_next_frame() {
    let floor = ipp_render_gl::glyph_atlas::MIN_POPULATES_PER_FRAME as u32;
    let ids: Vec<u32> = (0..floor + 8).collect();
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) =
        text_run_scene(&mut host, glyph_font(floor + 8, 1000, 1.0), &ids);
    renderer.set_glyph_population_budget_ms(0.0);
    let mut world = host.world_mut(world_id).unwrap();

    // Without time beyond the floor, population stops there; the incomplete run
    // stays analytic this frame.
    let first = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(first.glyph_misses, floor + 8, "{first:?}");
    assert_eq!(first.glyph_populates, floor);
    assert_eq!(first.gui_batches, 0);
    assert_eq!(state.analytic_glyph_draws.get(), 1);

    // The next frame populates the remainder and switches the run to the atlas.
    let second = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((second.glyph_misses, second.glyph_populates), (8, 8));
    assert_eq!(second.gui_batches, 1);
    assert_eq!(state.analytic_glyph_draws.get(), 1);

    let warm = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((warm.glyph_misses, warm.glyph_populates), (0, 0));
    assert_eq!(warm.uploaded_bytes, 0);
}

#[cfg(feature = "gui")]
#[test]
fn cold_text_within_the_time_budget_reaches_the_atlas_in_one_frame() {
    // More glyphs than the per-frame floor; the scene's Surface shows up to 50.
    let count = ipp_render_gl::glyph_atlas::MIN_POPULATES_PER_FRAME as u32 + 16;
    let ids: Vec<u32> = (0..count).collect();
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) =
        text_run_scene(&mut host, glyph_font(count, 1000, 1.0), &ids);
    let mut world = host.world_mut(world_id).unwrap();

    // The default budget covers them at the estimated cost: one population pass,
    // and the run samples the atlas in its first frame.
    let cold = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((cold.glyph_misses, cold.glyph_populates), (count, count));
    assert_eq!(cold.gui_batches, 1, "{cold:?}");
    assert_eq!(state.analytic_glyph_draws.get(), 0);
    assert_eq!(state.atlas_restores.get(), 1);
}

#[cfg(feature = "gui")]
#[test]
fn context_loss_during_glyph_population_reaches_recovery_and_repopulates() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = text_surface_scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    state
        .fail_atlas_begin
        .replace(Some(RenderError::ContextLost));
    let error = render_frame(&mut renderer, &mut world, 100, 100).unwrap_err();
    assert_eq!(error, RenderError::ContextLost.to_string());
    assert_eq!(
        state.analytic_glyph_draws.get(),
        0,
        "loss never falls back to analytic text"
    );
    assert!(!state.atlas_target_bound.get());
    drop(world);

    state.fail_atlas_begin.replace(None);
    recover_context(&mut renderer, &mut host, world_id, &surface_font());
    let pages = state.atlas_pages_created.get();
    let mut world = host.world_mut(world_id).unwrap();
    let recovered = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(
        (recovered.glyph_populates, recovered.gui_batches),
        (1, 1),
        "{recovered:?}"
    );
    assert_eq!(recovered.glyph_population_failures, 0);
    assert_eq!(recovered.glyph_pages, 1);
    assert_eq!(
        state.atlas_pages_created.get(),
        pages + 1,
        "a fresh page after recovery"
    );
    assert_eq!(state.analytic_glyph_draws.get(), 0);
}

#[cfg(feature = "gui")]
#[test]
fn context_loss_during_glyph_batch_upload_reaches_recovery() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = text_surface_scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    state
        .fail_glyph_batch
        .replace(Some(RenderError::ContextLost));
    let error = render_frame(&mut renderer, &mut world, 100, 100).unwrap_err();
    assert_eq!(error, RenderError::ContextLost.to_string());
    assert_eq!(state.analytic_glyph_draws.get(), 0);
    drop(world);

    state.fail_glyph_batch.replace(None);
    recover_context(&mut renderer, &mut host, world_id, &surface_font());
    let mut world = host.world_mut(world_id).unwrap();
    let recovered = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!(
        (recovered.glyph_populates, recovered.gui_batches),
        (1, 1),
        "{recovered:?}"
    );
    assert_eq!(recovered.gui_resident_bytes, 6 * 32);
    let warm = render_frame(&mut renderer, &mut world, 100, 100).unwrap();
    assert_eq!((warm.uploaded_bytes, warm.gui_batches), (0, 1));
}
