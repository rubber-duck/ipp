//! CPU picking invariants; maintained host harnesses provide transport/render coverage.

mod support;
use support::WorldTestDriver;

use ipp_core::{
    AssetResourceStatus, Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef,
    ErrorReason, GeometryPickHit, GeometryPickQuery, WorldUpdateReport,
    components::{Camera, MeshInstance, PickingGeometry, Transform},
    systems::geometry::{GeometryDefinition, GeometryShape},
};

fn run(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    report
}

fn create(
    world: &mut ipp_core::WorldContext<'_>,
    name: &str,
    values: Vec<ComponentValue>,
) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: EntityMetadata {
            symbolic_id: Some(name.into()),
            classes: vec![],
        },
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value,
            }),
    );
    run(world, operations).outcomes[0].result.as_ref().unwrap()[0].1
}

fn camera(
    world: &mut ipp_core::WorldContext<'_>,
    projection: Camera,
    transform: Transform,
) -> EntityId {
    create(
        world,
        &format!("camera{}", world.tick()),
        vec![
            ComponentValue::Camera(projection),
            ComponentValue::Transform(transform),
        ],
    )
}

fn target(
    world: &mut ipp_core::WorldContext<'_>,
    shape: PickingGeometry,
    transform: Transform,
) -> EntityId {
    create(
        world,
        &format!("target{}", world.tick()),
        vec![
            ComponentValue::PickingGeometry(shape),
            ComponentValue::Transform(transform),
        ],
    )
}

fn setup(host: &mut ipp_core::HostRuntime) -> (ipp_core::WorldContext<'_>, EntityId) {
    let world_id = host.create_world(ipp_core::WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world.register_stream_resource_provider("https").unwrap();
    let camera = camera(
        &mut world,
        Camera {
            projection: 1,
            ortho_height: 4.0,
            ..Camera::default()
        },
        Transform {
            z: 6.0,
            ..Transform::default()
        },
    );
    world.enqueue_camera_activate(camera).unwrap();
    world.update_for_test(0.0).unwrap();
    (world, camera)
}

fn query(x: f32, y: f32) -> GeometryPickQuery {
    GeometryPickQuery {
        x,
        y,
        width: 100,
        height: 100,
        include_view_plane: false,
    }
}

#[test]
fn requested_view_planes_use_camera_forward_independent_of_pointer_and_scale() {
    for projection in [0, 1] {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let (mut world, active) = setup(&mut fixture_host);
        let selected = target(
            &mut world,
            unit_geometry(),
            Transform {
                sx: 4.0,
                sy: 4.0,
                sz: 4.0,
                ..Transform::default()
            },
        );
        run(
            &mut world,
            vec![
                Command::InsertComponentValue {
                    entity: EntityRef::Handle(active),
                    value: ComponentValue::Camera(Camera {
                        projection,
                        ortho_height: 4.0,
                        ..Camera::default()
                    }),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Handle(active),
                    value: ComponentValue::Transform(Transform {
                        x: 6.0,
                        qy: std::f32::consts::FRAC_1_SQRT_2,
                        qw: std::f32::consts::FRAC_1_SQRT_2,
                        sx: 2.0,
                        sy: 3.0,
                        sz: 4.0,
                        ..Transform::default()
                    }),
                },
            ],
        );
        let ordinary = pick(&mut world, query(0.52, 0.48)).unwrap().unwrap();
        assert_eq!(ordinary.entity, selected);
        assert!(ordinary.view_plane.is_none());
        let included = pick(
            &mut world,
            GeometryPickQuery {
                include_view_plane: true,
                ..query(0.52, 0.48)
            },
        )
        .unwrap()
        .unwrap();
        let plane = included.view_plane.unwrap();
        assert_eq!(plane.point, ordinary.position);
        assert_eq!(
            GeometryPickHit {
                view_plane: None,
                ..included
            },
            ordinary
        );
        for (value, expected) in plane.normal.into_iter().zip([-1.0, 0.0, 0.0]) {
            assert!((value - expected).abs() < 1e-6);
        }
    }
}

#[test]
fn view_plane_uses_final_camera_when_activation_follows_query() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    let second = camera(
        &mut world,
        Camera {
            projection: 1,
            ortho_height: 4.0,
            ..Camera::default()
        },
        Transform {
            x: 6.0,
            qy: std::f32::consts::FRAC_1_SQRT_2,
            qw: std::f32::consts::FRAC_1_SQRT_2,
            ..Transform::default()
        },
    );
    target(&mut world, unit_geometry(), Transform::default());
    world
        .enqueue_geometry_pick(
            123,
            GeometryPickQuery {
                include_view_plane: true,
                ..query(0.5, 0.5)
            },
        )
        .unwrap();
    world.enqueue_camera_activate(second).unwrap();
    let report = world.update_for_test(0.0).unwrap();
    let result = &report.geometry_picks[0];
    assert_eq!(
        (result.request_id, result.tick, result.camera),
        (123, report.tick, Some(second))
    );
    let hit = result.result.unwrap().unwrap();
    assert_eq!(hit.view_plane.unwrap().point, hit.position);
    assert!((hit.view_plane.unwrap().normal[0] + 1.0).abs() < 1e-6);
}

fn pick(
    world: &mut ipp_core::WorldContext<'_>,
    query: GeometryPickQuery,
) -> Result<Option<GeometryPickHit>, ErrorReason> {
    world.enqueue_geometry_pick(100, query).unwrap();
    let report = world.update_for_test(0.0).unwrap();
    let result = &report.geometry_picks[0];
    assert_eq!(
        (result.request_id, result.tick, result.camera),
        (100, report.tick, world.active_camera())
    );
    result.result
}

fn unit_geometry() -> PickingGeometry {
    PickingGeometry {
        geometry: GeometryDefinition::from(GeometryShape::default())
            .encode()
            .unwrap(),
        ..PickingGeometry::default()
    }
}

fn ring() -> Vec<u8> {
    GeometryDefinition {
        parts: vec![
            GeometryShape::Box {
                min: [-1.0, -1.0, 0.0],
                max: [1.0, -0.3, 0.0],
            }
            .into(),
            GeometryShape::Box {
                min: [0.3, -0.3, 0.0],
                max: [1.0, 0.3, 0.0],
            }
            .into(),
            GeometryShape::Box {
                min: [-1.0, 0.3, 0.0],
                max: [1.0, 1.0, 0.0],
            }
            .into(),
            GeometryShape::Box {
                min: [-1.0, -0.3, 0.0],
                max: [-0.3, 0.3, 0.0],
            }
            .into(),
        ],
    }
    .encode()
    .unwrap()
}

fn resource_geometry(source: &str) -> PickingGeometry {
    PickingGeometry {
        source: source.into(),
        ..PickingGeometry::default()
    }
}

#[test]
fn queries_observe_final_camera_and_mutations_regardless_of_submission_position() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, first) = setup(&mut fixture_host);
    let second = camera(
        &mut world,
        Camera {
            projection: 1,
            ortho_height: 4.0,
            ..Camera::default()
        },
        Transform {
            x: 4.0,
            z: 6.0,
            ..Transform::default()
        },
    );
    target(&mut world, unit_geometry(), Transform::default());
    let selected = target(
        &mut world,
        unit_geometry(),
        Transform {
            x: 4.0,
            ..Transform::default()
        },
    );
    world.enqueue_geometry_pick(20, query(0.5, 0.5)).unwrap();
    world.enqueue_camera_activate(first).unwrap();
    world.enqueue_camera_activate(second).unwrap();
    let report = run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(selected),
            value: ComponentValue::Transform(Transform {
                x: 4.0,
                z: 2.0,
                ..Transform::default()
            }),
        }],
    );
    let result = &report.geometry_picks[0];
    assert_eq!(result.camera, Some(second));
    let hit = result.result.unwrap().unwrap();
    assert_eq!(
        (hit.entity, hit.position, hit.distance, hit.part),
        (selected, [4.0, 0.0, 2.5], 3.5, 0)
    );
    assert_eq!(
        report
            .camera_state_changes
            .iter()
            .map(|v| v.changes.active_camera)
            .collect::<Vec<_>>(),
        vec![Some(second)]
    );
}

#[test]
fn box_nearest_distance_scale_and_entity_ties_are_world_space() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    let first = target(
        &mut world,
        unit_geometry(),
        Transform {
            z: 1.0,
            sz: 4.0,
            sx: 0.5,
            sy: 3.0,
            ..Transform::default()
        },
    );
    let second = target(
        &mut world,
        unit_geometry(),
        Transform {
            z: 2.0,
            sz: 2.0,
            ..Transform::default()
        },
    );
    let hit = pick(&mut world, query(0.5, 0.5)).unwrap().unwrap();
    assert_eq!(
        (hit.entity, hit.position, hit.distance),
        (first.min(second), [0.0, 0.0, 3.0], 3.0)
    );
    assert_eq!(pick(&mut world, query(0.9, 0.9)), Ok(None));
    assert!(
        world.render_items().is_empty(),
        "interaction geometry is independent of rendering"
    );
}

#[test]
fn clipping_uses_camera_planes_and_does_not_invent_clipped_box_surfaces() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, camera) = setup(&mut fixture_host);
    let target = target(
        &mut world,
        unit_geometry(),
        Transform {
            z: 3.0,
            sz: 4.0,
            ..Transform::default()
        },
    );
    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(camera),
            value: ComponentValue::Camera(Camera {
                projection: 1,
                near: 2.0,
                far: 4.0,
                ortho_height: 4.0,
                ..Camera::default()
            }),
        }],
    );
    // Box occupies distance 1..5, entirely across the clipping interval: neither surface is visible.
    assert_eq!(pick(&mut world, query(0.5, 0.5)), Ok(None));
    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(camera),
            value: ComponentValue::Camera(Camera {
                projection: 1,
                near: 2.0,
                far: 6.0,
                ortho_height: 4.0,
                ..Camera::default()
            }),
        }],
    );
    let hit = pick(&mut world, query(0.5, 0.5)).unwrap().unwrap();
    assert_eq!(
        (hit.entity, hit.distance, hit.position),
        (target, 5.0, [0.0, 0.0, 1.0])
    );
}

#[test]
fn perspective_aspect_and_rotated_pose_agree_with_shared_projection() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, camera) = setup(&mut fixture_host);
    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(camera),
            value: ComponentValue::Camera(Camera {
                fov_y: std::f32::consts::FRAC_PI_2,
                near: 0.1,
                far: 5.0,
                ..Camera::default()
            }),
        }],
    );
    let target = target(
        &mut world,
        unit_geometry(),
        Transform {
            x: 4.0,
            z: 2.0,
            ..Transform::default()
        },
    );
    let hit = pick(
        &mut world,
        GeometryPickQuery {
            width: 200,
            ..query(0.75, 0.5)
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(hit.entity, target);
    assert!((hit.distance - 3.5 * 2.0f32.sqrt()).abs() < 0.00001);
    let matrix = world.prepare_camera(200, 100).unwrap().view_projection;
    let homogeneous = [hit.position[0], hit.position[1], hit.position[2], 1.0];
    let clip: [f32; 4] = std::array::from_fn(|row| {
        (0..4)
            .map(|column| matrix[column * 4 + row] * homogeneous[column])
            .sum()
    });
    assert!((clip[0] / clip[3] - 0.5).abs() < 0.00001);

    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(camera),
            value: ComponentValue::Transform(Transform {
                x: 6.0,
                qy: 0.5,
                qw: 0.5,
                ..Transform::default()
            }),
        }],
    );
    target_at_origin(&mut world);
    assert!(
        pick(&mut world, query(0.5, 0.5)).unwrap().is_none(),
        "far plane excludes origin at distance 6"
    );
}

fn target_at_origin(world: &mut ipp_core::WorldContext<'_>) {
    target(world, unit_geometry(), Transform::default());
}

#[test]
fn compound_geometry_preserves_holes_and_authored_part_identity_after_transforms() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    let entity = target(
        &mut world,
        PickingGeometry {
            geometry: ring(),
            ..PickingGeometry::default()
        },
        Transform::default(),
    );
    assert_eq!(pick(&mut world, query(0.5, 0.5)), Ok(None));
    let hit = pick(&mut world, query(0.7, 0.5)).unwrap().unwrap();
    assert_eq!((hit.entity, hit.distance, hit.part), (entity, 6.0, 1));
    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::Transform(Transform {
                qy: 1.0,
                qw: 0.0,
                sx: 2.0,
                sy: 2.0,
                sz: 3.0,
                ..Transform::default()
            }),
        }],
    );
    let hit = pick(&mut world, query(0.7, 0.5)).unwrap().unwrap();
    assert_eq!(hit.part, 3, "the rotated left rim now intersects the ray");
}

#[test]
fn missing_geometry_cannot_prove_a_miss_and_failed_payloads_remain_unavailable() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    let entity = target(
        &mut world,
        resource_geometry("https://fixture/geometry"),
        Transform::default(),
    );
    assert_eq!(
        pick(&mut world, query(0.7, 0.5)),
        Err(ErrorReason::GeometryUnavailable)
    );
    assert_eq!(
        pick(&mut world, query(0.9, 0.9)),
        Err(ErrorReason::GeometryUnavailable)
    );
    let request = world.resource_requests_for_test().pop().unwrap();
    world.complete_resource(request.id, Ok(ring())).unwrap();
    assert!(pick(&mut world, query(0.7, 0.5)).unwrap().is_some());
    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::PickingGeometry(resource_geometry("https://fixture/invalid")),
        }],
    );
    let request = world.resource_requests_for_test().pop().unwrap();
    world
        .complete_resource(request.id, Ok(vec![1, 2, 3]))
        .unwrap();
    assert_eq!(
        pick(&mut world, query(0.7, 0.5)),
        Err(ErrorReason::GeometryUnavailable)
    );
}

#[test]
fn picking_loads_shared_geometry_even_when_renderer_drives_other_resources() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    world.set_renderer_asset_loading(true);
    let first = target(
        &mut world,
        resource_geometry("https://fixture/shared"),
        Transform::default(),
    );
    let second = target(
        &mut world,
        resource_geometry("https://fixture/shared"),
        Transform::default(),
    );
    create(
        &mut world,
        "visual-only",
        vec![ComponentValue::MeshInstance(MeshInstance {
            source: "https://fixture/visual".into(),
            variant: 0,
        })],
    );
    let requests = world.resource_requests_for_test();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .any(|request| request.source == "https://fixture/visual"),
        "automatic visual bounds also request CPU mesh metadata"
    );
    let geometry = requests
        .iter()
        .find(|request| request.source == "https://fixture/shared")
        .unwrap();
    world.complete_resource(geometry.id, Ok(ring())).unwrap();
    world.enqueue_geometry_pick(100, query(0.4, 0.6)).unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(
        report.geometry_picks[0].result.unwrap().unwrap().entity,
        first.min(second)
    );
    assert!(
        report
            .resource_changes
            .iter()
            .any(|resource| resource.source == "https://fixture/shared"
                && resource.status == AssetResourceStatus::Loaded)
    );
    assert!(world.take_asset_events().unwrap().is_empty());
    let snapshots = world.resource_snapshots();
    assert_eq!(snapshots.len(), 2);
    assert!(
        snapshots
            .iter()
            .any(|resource| resource.source == "https://fixture/visual"
                && resource.status == AssetResourceStatus::Start)
    );
}

#[test]
fn query_validation_is_correlated_and_observes_surface_size_at_evaluation() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    assert_eq!(
        pick(&mut world, query(0.5, 0.5)),
        Err(ErrorReason::NoActiveCamera)
    );
    for invalid in [
        query(f32::NAN, 0.5),
        query(-0.1, 0.5),
        query(0.5, 1.1),
        GeometryPickQuery {
            width: 0,
            ..query(0.5, 0.5)
        },
    ] {
        assert_eq!(pick(&mut world, invalid), Err(ErrorReason::InvalidViewport));
    }
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    world.enqueue_geometry_pick(3, query(0.5, 0.5)).unwrap();
    world.set_render_viewport(Some((200, 100)));
    let result = &world.update_for_test(0.0).unwrap().geometry_picks[0];
    assert_eq!(
        (result.request_id, result.result),
        (3, Err(ErrorReason::InvalidViewport))
    );
    world.set_render_viewport(None);
    assert_eq!(pick(&mut world, query(0.5, 0.5)), Ok(None));
}

#[test]
fn effective_overlay_transforms_control_interaction_without_changing_base() {
    use ipp_core::{
        ComponentOverlayMode, EntityOverlayMode, FieldValue, FieldWrite, StateOverlayRef,
    };
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    let entity = target(&mut world, unit_geometry(), Transform::default());
    let name = world.inspect(entity).unwrap().metadata.symbolic_id.unwrap();
    let report = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: name,
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::TRANSFORM,
                mode: ComponentOverlayMode::Bound,
                fields: vec![FieldWrite {
                    offset: std::mem::offset_of!(Transform, x) as u32,
                    value: FieldValue::F32(1.0),
                }],
            },
        ],
    );
    let owner = StateOverlayRef::Handle(report.outcomes[0].state_overlays[0].id);
    assert_eq!(pick(&mut world, query(0.5, 0.5)), Ok(None));
    assert_eq!(
        pick(&mut world, query(0.75, 0.5)).unwrap().unwrap().entity,
        entity
    );
    run(
        &mut world,
        vec![Command::ReleaseStateOverlayOwner {
            owner,
        }],
    );
    assert_eq!(
        pick(&mut world, query(0.5, 0.5)).unwrap().unwrap().entity,
        entity
    );
}

#[test]
fn camera_projection_preserves_plane_coordinates_for_both_projections_and_outside_capture() {
    for projection in [0, 1] {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let (mut world, active) = setup(&mut fixture_host);
        run(
            &mut world,
            vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(active),
                value: ComponentValue::Camera(Camera {
                    projection,
                    ortho_height: 4.0,
                    fov_y: std::f32::consts::FRAC_PI_2,
                    far: 3.0,
                    ..Camera::default()
                }),
            }],
        );
        let query = ipp_core::CameraProjectQuery {
            x: 1.5,
            y: -0.5,
            width: 100,
            height: 100,
            plane: ipp_core::WorldPlane {
                point: [0.0; 3],
                normal: [0.0, 0.0, -2.0],
            },
        };
        world.enqueue_camera_project(201, query).unwrap();
        let report = world.update_for_test(0.0).unwrap();
        let result = &report.camera_projections[0];
        assert_eq!(
            (result.request_id, result.tick, result.camera),
            (201, report.tick, Some(active))
        );
        let position = result.result.unwrap().unwrap();
        let expected = if projection == 0 {
            12.0
        } else {
            4.0
        };
        assert!((position[0] - expected).abs() < 1e-5);
        assert!((position[1] - expected).abs() < 1e-5);
        assert!(position[2].abs() < 1e-5);
        assert!(
            world.entities().len() == 1,
            "Projection neither needs nor creates target geometry"
        );
    }
}

#[test]
fn camera_projection_handles_invalid_parallel_and_backward_planes_without_mutation() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    let before = world.entities();
    for (point, normal, expected) in [
        ([0.0; 3], [0.0; 3], Err(ErrorReason::InvalidValue)),
        (
            [f32::NAN, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            Err(ErrorReason::InvalidValue),
        ),
        ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], Ok(None)),
        ([0.0; 3], [1.0, 0.0, 0.0], Ok(None)),
        ([0.0, 0.0, 7.0], [0.0, 0.0, 1.0], Ok(None)),
    ] {
        world
            .enqueue_camera_project(
                202,
                ipp_core::CameraProjectQuery {
                    x: 0.5,
                    y: 0.5,
                    width: 100,
                    height: 100,
                    plane: ipp_core::WorldPlane {
                        point,
                        normal,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            world.update_for_test(0.0).unwrap().camera_projections[0].result,
            expected
        );
        assert_eq!(world.entities(), before);
    }
}

#[test]
fn automatic_picking_bounds_start_mesh_cpu_loading_without_a_renderer_poll() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, _) = setup(&mut fixture_host);
    world.set_renderer_asset_loading(true);
    let entity = create(
        &mut world,
        "automatic-mesh",
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::MeshInstance(MeshInstance {
                source: "https://fixture/automatic".into(),
                variant: 0,
            }),
            ComponentValue::PickingGeometry(PickingGeometry::default()),
        ],
    );
    assert_eq!(
        pick(&mut world, query(0.5, 0.5)),
        Err(ErrorReason::GeometryUnavailable)
    );
    let requests = world.resource_requests_for_test();
    assert_eq!(requests.len(), 1);
    let mut bytes = b"IPPM".to_vec();
    for value in [1u32, 3, 3] {
        bytes.extend(value.to_le_bytes());
    }
    for position in [[-1.0f32, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in position {
            bytes.extend(value.to_le_bytes());
        }
        for value in [1.0f32; 3] {
            bytes.extend(value.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    world.complete_resource(requests[0].id, Ok(bytes)).unwrap();
    assert_eq!(
        pick(&mut world, query(0.5, 0.5)).unwrap().unwrap().entity,
        entity
    );
}
