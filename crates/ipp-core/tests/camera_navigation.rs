//! Camera-space navigation and staged producer writes; real transport/frame coverage is external.

mod support;
use support::WorldTestDriver;

use std::mem::offset_of;

use ipp_core::{
    Batch, CameraMotion, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, ErrorReason,
    FieldValue, FieldWrite, WorldLimits, WorldUpdateReport,
    components::{Camera, Transform},
    systems::camera::model_matrix,
};

fn run(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap()
}

fn create(
    world: &mut ipp_core::WorldContext<'_>,
    camera: Camera,
    transform: Transform,
) -> EntityId {
    let name = format!("camera-{}", world.tick());
    let report = run(
        world,
        vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata {
                    symbolic_id: Some(name),
                    classes: vec![],
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::Camera(camera),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::Transform(transform),
            },
        ],
    );
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

fn inputs(
    world: &ipp_core::WorldContext<'_>,
    entity: EntityId,
    effective: bool,
) -> (Camera, Transform) {
    let snapshot = world.inspect(entity).unwrap();
    let values = if effective {
        snapshot.effective
    } else {
        snapshot.base
    };
    let camera = values
        .iter()
        .find_map(|value| match value {
            ComponentValue::Camera(camera) => Some(*camera),
            _ => None,
        })
        .unwrap();
    let transform = values
        .iter()
        .find_map(|value| match value {
            ComponentValue::Transform(transform) => Some(*transform),
            _ => None,
        })
        .unwrap();
    (camera, transform)
}

fn navigate(world: &mut ipp_core::WorldContext<'_>, motion: CameraMotion) {
    world.enqueue_camera_navigate(motion).unwrap();
    let report = world.update_for_test(0.0).unwrap();

    assert!(report.outcomes.is_empty());
    assert!(report.camera_state_changes.is_empty());
    assert!(report.render_state_changes.is_empty());
    assert!(report.diagnostics.is_empty());
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} != {expected}"
    );
}

fn pivot(camera: Camera, transform: Transform) -> [f64; 3] {
    let matrix = model_matrix(&transform).unwrap().map(f64::from);
    std::array::from_fn(|i| matrix[12 + i] - matrix[8 + i] * f64::from(camera.focus_distance))
}

fn project(
    world: &ipp_core::WorldContext<'_>,
    position: [f64; 3],
    width: u32,
    height: u32,
) -> [f64; 2] {
    let matrix = world
        .prepare_camera(width, height)
        .unwrap()
        .view_projection
        .map(f64::from);
    let position = [position[0], position[1], position[2], 1.0];
    let clip: [f64; 4] = std::array::from_fn(|row| {
        (0..4)
            .map(|col| matrix[col * 4 + row] * position[col])
            .sum()
    });
    [
        (clip[0] / clip[3] + 1.0) * 0.5,
        (1.0 - clip[1] / clip[3]) * 0.5,
    ]
}

#[test]
fn repeated_local_orbits_preserve_the_implied_pivot_with_nonuniform_scale() {
    for projection in [0, 1] {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let entity = create(
            &mut world,
            Camera {
                projection,
                ..Camera::default()
            },
            Transform {
                x: 18.0,
                y: 5.0,
                z: 8.0,
                qy: 2.0,
                qw: 2.0,
                sx: 2.0,
                sy: 4.0,
                sz: 3.0,
                ..Transform::default()
            },
        );
        world.enqueue_camera_activate(entity).unwrap();
        world.update_for_test(0.0).unwrap();

        let expected = [0.0, 5.0, 8.0];

        for index in 0..600 {
            navigate(
                &mut world,
                CameraMotion::Rotate {
                    yaw: if index % 2 == 0 {
                        0.04
                    } else {
                        -0.03
                    },
                    pitch: 0.02,
                },
            );
            let (camera, transform) = inputs(&world, entity, false);
            for (actual, expected) in pivot(camera, transform).into_iter().zip(expected) {
                close(actual, expected, 0.0002);
            }
            assert_eq!([transform.sx, transform.sy, transform.sz], [2.0, 4.0, 3.0]);
            close(
                [transform.qx, transform.qy, transform.qz, transform.qw]
                    .into_iter()
                    .map(|value| f64::from(value).powi(2))
                    .sum(),
                1.0,
                0.000001,
            );
        }
        for coordinate in project(&world, expected, 800, 400) {
            close(coordinate, 0.5, 0.00001);
        }
    }
}

#[test]
fn yaw_then_pitch_uses_camera_local_axes_and_reverses_in_reverse_order() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(
        &mut world,
        Camera::default(),
        Transform {
            z: 6.0,
            ..Transform::default()
        },
    );
    world.enqueue_camera_activate(entity).unwrap();
    world.update_for_test(0.0).unwrap();

    navigate(
        &mut world,
        CameraMotion::Rotate {
            yaw: std::f32::consts::FRAC_PI_2,
            pitch: 0.0,
        },
    );
    let (_, transform) = inputs(&world, entity, false);
    close(f64::from(transform.x), 6.0, 0.00001);
    close(f64::from(transform.z), 0.0, 0.00001);

    navigate(
        &mut world,
        CameraMotion::Rotate {
            yaw: 0.0,
            pitch: std::f32::consts::FRAC_PI_2,
        },
    );
    let (_, transform) = inputs(&world, entity, false);
    close(f64::from(transform.x), 0.0, 0.00001);
    close(f64::from(transform.y), -6.0, 0.00001);

    navigate(
        &mut world,
        CameraMotion::Rotate {
            yaw: 0.0,
            pitch: -std::f32::consts::FRAC_PI_2,
        },
    );

    navigate(
        &mut world,
        CameraMotion::Rotate {
            yaw: -std::f32::consts::FRAC_PI_2,
            pitch: 0.0,
        },
    );
    let (_, transform) = inputs(&world, entity, false);
    close(f64::from(transform.x), 0.0, 0.00001);
    close(f64::from(transform.y), 0.0, 0.00001);
    close(f64::from(transform.z), 6.0, 0.00001);
}

#[test]
fn projection_pan_tracks_the_cursor_and_zoom_preserves_the_pivot() {
    for projection in [0, 1] {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let entity = create(
            &mut world,
            Camera {
                projection,
                ortho_height: 5.0,
                ..Camera::default()
            },
            Transform {
                x: 3.0,
                y: 4.0,
                z: 9.0,
                qx: 0.2,
                qy: 0.4,
                qz: 0.3,
                qw: 0.8,
                sx: 2.0,
                sy: 3.0,
                sz: 1.5,
            },
        );
        world.enqueue_camera_activate(entity).unwrap();
        world.update_for_test(0.0).unwrap();

        let (original_camera, original_transform) = inputs(&world, entity, false);
        let expected_pivot = pivot(original_camera, original_transform);
        navigate(
            &mut world,
            CameraMotion::Zoom {
                amount: std::f32::consts::LN_2,
            },
        );
        let (camera, transform) = inputs(&world, entity, false);
        for (actual, expected) in pivot(camera, transform).into_iter().zip(expected_pivot) {
            close(actual, expected, 0.00001);
        }
        if projection == 0 {
            close(f64::from(camera.focus_distance), 12.0, 0.00001);
            assert_eq!(camera.ortho_height, original_camera.ortho_height);
        } else {
            assert_eq!(transform, original_transform);
            assert_eq!(camera.focus_distance, original_camera.focus_distance);
            close(f64::from(camera.ortho_height), 10.0, 0.00001);
        }
        navigate(
            &mut world,
            CameraMotion::Pan {
                x: 0.1,
                y: 0.15,
                width: 800,
                height: 400,
            },
        );
        let screen = project(&world, expected_pivot, 800, 400);
        close(screen[0], 0.6, 0.000001);
        close(screen[1], 0.65, 0.000001);
        navigate(
            &mut world,
            CameraMotion::Pan {
                x: -0.1,
                y: -0.15,
                width: 800,
                height: 400,
            },
        );
        navigate(
            &mut world,
            CameraMotion::Zoom {
                amount: -std::f32::consts::LN_2,
            },
        );
        let (camera, transform) = inputs(&world, entity, false);
        assert_eq!(camera, original_camera);
        for (actual, expected) in [transform.x, transform.y, transform.z].into_iter().zip([
            original_transform.x,
            original_transform.y,
            original_transform.z,
        ]) {
            close(f64::from(actual), f64::from(expected), 0.00001);
        }
    }
}

#[test]
fn navigation_selection_and_base_writes_follow_one_ordered_queue() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let first = create(&mut world, Camera::default(), Transform::default());
    let second = create(&mut world, Camera::default(), Transform::default());
    world
        .enqueue_camera_navigate(CameraMotion::Zoom {
            amount: 1.0,
        })
        .unwrap();
    world.enqueue_camera_activate(first).unwrap();
    world
        .enqueue_camera_navigate(CameraMotion::Zoom {
            amount: std::f32::consts::LN_2,
        })
        .unwrap();
    world.enqueue_camera_activate(second).unwrap();
    world
        .enqueue(Batch {
            id: 99,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(second),
                component: ComponentValue::CAMERA,
                field: FieldWrite {
                    offset: offset_of!(Camera, focus_distance) as u32,
                    value: FieldValue::F32(10.0),
                },
            }],
        })
        .unwrap();
    world
        .enqueue_camera_navigate(CameraMotion::Zoom {
            amount: std::f32::consts::LN_2,
        })
        .unwrap();
    world.enqueue_camera_activate(first).unwrap();
    let report = world.update_for_test(0.0).unwrap();

    assert_eq!(report.outcomes.len(), 1);
    assert!(report.outcomes[0].result.is_ok());
    assert_eq!(
        report
            .camera_state_changes
            .iter()
            .map(|event| event.changes.active_camera)
            .collect::<Vec<_>>(),
        [Some(first), Some(second), Some(first)]
    );
    assert_eq!(inputs(&world, first, false).0.focus_distance, 12.0);
    assert_eq!(inputs(&world, second, false).0.focus_distance, 20.0);
    assert_eq!(inputs(&world, first, false).1.z, 6.0);
    assert_eq!(inputs(&world, second, false).1.z, 10.0);
}

#[test]
fn invalid_and_unrepresentable_navigation_preserves_every_component() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    navigate(
        &mut world,
        CameraMotion::Zoom {
            amount: 1.0,
        },
    );
    assert!(world.entities().is_empty());
    let entity = create(&mut world, Camera::default(), Transform::default());
    world.enqueue_camera_activate(entity).unwrap();
    world.update_for_test(0.0).unwrap();

    let before = world.inspect(entity).unwrap();
    for motion in [
        CameraMotion::Rotate {
            yaw: f32::NAN,
            pitch: 0.0,
        },
        CameraMotion::Rotate {
            yaw: 1.0,
            pitch: f32::INFINITY,
        },
        CameraMotion::Pan {
            x: f32::NAN,
            y: 0.0,
            width: 100,
            height: 100,
        },
        CameraMotion::Pan {
            x: 1.0,
            y: f32::INFINITY,
            width: 100,
            height: 100,
        },
        CameraMotion::Pan {
            x: 1.0,
            y: 0.0,
            width: 0,
            height: 100,
        },
        CameraMotion::Pan {
            x: 1.0,
            y: 0.0,
            width: 100,
            height: 0,
        },
        CameraMotion::Pan {
            x: f32::MAX,
            y: 0.0,
            width: 100,
            height: 100,
        },
        CameraMotion::Pan {
            x: f32::MAX,
            y: 0.0,
            width: 1,
            height: u32::MAX,
        },
        CameraMotion::Zoom {
            amount: f32::NEG_INFINITY,
        },
        CameraMotion::Zoom {
            amount: f32::MAX,
        },
        CameraMotion::Zoom {
            amount: -f32::MAX,
        },
        CameraMotion::Rotate {
            yaw: 0.0,
            pitch: 0.0,
        },
        CameraMotion::Pan {
            x: 0.0,
            y: 0.0,
            width: 100,
            height: 100,
        },
        CameraMotion::Zoom {
            amount: 0.0,
        },
    ] {
        navigate(&mut world, motion);
        assert_eq!(world.inspect(entity), Some(before.clone()));
    }

    for camera in [
        Camera {
            focus_distance: f32::MAX,
            ..Camera::default()
        },
        Camera {
            projection: 1,
            ortho_height: f32::MAX,
            ..Camera::default()
        },
        Camera {
            focus_distance: f32::from_bits(1),
            ..Camera::default()
        },
    ] {
        let report = run(
            &mut world,
            vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::Camera(camera),
            }],
        );
        assert!(report.outcomes[0].result.is_ok());

        let before = world.inspect(entity).unwrap();
        navigate(
            &mut world,
            CameraMotion::Zoom {
                amount: if camera.focus_distance < 1.0 {
                    -4.0
                } else {
                    4.0
                },
            },
        );
        assert_eq!(world.inspect(entity), Some(before));
    }
}

#[test]
fn navigation_shares_queue_bounds_and_invalid_time_preserves_pending_commands() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(WorldLimits {
            max_queued_batches: 1,
            ..WorldLimits::default()
        })
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, Camera::default(), Transform::default());
    world.enqueue_camera_activate(entity).unwrap();
    world.update_for_test(0.0).unwrap();

    world
        .enqueue_camera_navigate(CameraMotion::Zoom {
            amount: 2.0_f32.ln(),
        })
        .unwrap();
    assert_eq!(
        world.enqueue_camera_activate(entity),
        Err(ErrorReason::Capacity)
    );
    assert_eq!(
        world.enqueue_camera_navigate(CameraMotion::Zoom {
            amount: 0.0
        }),
        Err(ErrorReason::Capacity)
    );
    assert_eq!(
        world.update_for_test(f64::NAN),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(inputs(&world, entity, false).0.focus_distance, 6.0);
    world.update_for_test(0.0).unwrap();

    close(
        f64::from(inputs(&world, entity, false).0.focus_distance),
        12.0,
        0.0001,
    );
}

#[test]
fn navigation_retains_bound_declarations_and_uses_hidden_producer_values() {
    use ipp_core::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(
        &mut world,
        Camera::default(),
        Transform {
            z: 6.0,
            ..Transform::default()
        },
    );
    let report = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "camera-0".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::CAMERA,
                mode: ComponentOverlayMode::Bound,
                fields: vec![FieldWrite {
                    offset: offset_of!(Camera, focus_distance) as u32,
                    value: FieldValue::F32(50.0),
                }],
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 3,
                component: ComponentValue::TRANSFORM,
                mode: ComponentOverlayMode::Bound,
                fields: vec![FieldWrite {
                    offset: offset_of!(Transform, z) as u32,
                    value: FieldValue::F32(80.0),
                }],
            },
        ],
    );
    let owner = StateOverlayRef::Handle(report.outcomes[0].state_overlays[0].id);
    world.enqueue_camera_activate(entity).unwrap();
    world.update_for_test(0.0).unwrap();

    navigate(
        &mut world,
        CameraMotion::Zoom {
            amount: std::f32::consts::LN_2,
        },
    );
    let (base_camera, base_transform) = inputs(&world, entity, false);
    assert_eq!((base_camera.focus_distance, base_transform.z), (12.0, 12.0));
    let (effective_camera, effective_transform) = inputs(&world, entity, true);
    assert_eq!(
        (effective_camera.focus_distance, effective_transform.z),
        (50.0, 80.0)
    );

    navigate(
        &mut world,
        CameraMotion::Zoom {
            amount: std::f32::consts::LN_2,
        },
    );
    let (base_camera, base_transform) = inputs(&world, entity, false);
    assert_eq!((base_camera.focus_distance, base_transform.z), (24.0, 24.0));
    let report = run(
        &mut world,
        vec![Command::ReleaseStateOverlayOwner {
            owner,
        }],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert!(report.diagnostics.is_empty());
    assert_eq!(inputs(&world, entity, true), (base_camera, base_transform));
}

#[test]
fn fallback_components_remain_renderable_but_cannot_navigate() {
    use ipp_core::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    for component in [ComponentValue::CAMERA, ComponentValue::TRANSFORM] {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let entity = create(&mut world, Camera::default(), Transform::default());
        let report = run(
            &mut world,
            vec![
                Command::CreateStateOverlayOwner {
                    alias: 0,
                },
                Command::AttachEntityOverlayBinding {
                    owner: StateOverlayRef::Alias(0),
                    alias: 1,
                    symbolic_id: "camera-0".into(),
                    mode: EntityOverlayMode::Bound,
                },
                Command::AttachComponentStateOverlay {
                    owner: StateOverlayRef::Alias(0),
                    binding: StateOverlayRef::Alias(1),
                    alias: 2,
                    component,
                    mode: ComponentOverlayMode::Auto,
                    fields: vec![],
                },
                Command::RemoveComponent {
                    entity: EntityRef::Handle(entity),
                    component,
                },
            ],
        );
        assert!(report.outcomes[0].result.is_ok());
        world.enqueue_camera_activate(entity).unwrap();
        world.update_for_test(0.0).unwrap();

        let before = world.inspect(entity).unwrap();
        for motion in [
            CameraMotion::Rotate {
                yaw: 1.0,
                pitch: 0.0,
            },
            CameraMotion::Pan {
                x: 0.1,
                y: 0.0,
                width: 100,
                height: 100,
            },
            CameraMotion::Zoom {
                amount: 1.0,
            },
        ] {
            navigate(&mut world, motion);
            assert_eq!(world.inspect(entity), Some(before.clone()));
            assert_eq!(world.prepare_camera(100, 100).unwrap().entity, entity);
        }
    }
}

#[test]
fn effective_projection_failure_keeps_navigation_and_can_be_corrected_by_overlay_release() {
    use ipp_core::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, Camera::default(), Transform::default());
    let report = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "camera-0".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::TRANSFORM,
                mode: ComponentOverlayMode::Bound,
                fields: vec![FieldWrite {
                    offset: offset_of!(Transform, sz) as u32,
                    value: FieldValue::F32(1.0e-38),
                }],
            },
        ],
    );
    let owner = StateOverlayRef::Handle(report.outcomes[0].state_overlays[0].id);
    world.enqueue_camera_activate(entity).unwrap();
    world.update_for_test(0.0).unwrap();

    assert_eq!(world.prepare_camera(100, 100).unwrap().entity, entity);

    // The overlay makes the resulting view overflow; authored navigation still applies.
    navigate(
        &mut world,
        CameraMotion::Zoom {
            amount: std::f32::consts::LN_2,
        },
    );
    let expected = (
        Camera {
            focus_distance: 12.0,
            ..Camera::default()
        },
        Transform {
            z: 6.0,
            ..Transform::default()
        },
    );
    assert_eq!(inputs(&world, entity, false), expected);
    let report = run(
        &mut world,
        vec![Command::ReleaseStateOverlayOwner {
            owner,
        }],
    );
    assert!(report.outcomes[0].result.is_ok());
    assert!(report.diagnostics.is_empty());
    assert_eq!(inputs(&world, entity, false), expected);
    assert_eq!(inputs(&world, entity, true), expected);
}
