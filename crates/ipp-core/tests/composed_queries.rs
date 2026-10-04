//! Completed composed queries against real Camera, Canvas and spatial publications.

mod support;
use support::task_scheduler::HostTaskTestDriver;

use support::CanvasTestHost;
use support::selection::{
    ANIMATION, ATTACHMENTS, CAMERA, CONSTRAINTS, GUI_LAYOUT, SURFACE, select,
};
use support::world_failures::select_with_failures;

use ipp_core::components::{
    Camera, CanvasStyle, FlatSurface, GuiCheckbox, GuiLayout, PickingGeometry, Transform,
};
use ipp_core::services::gui_input::query::*;
use ipp_core::systems::geometry::{GeometryDefinition, GeometryShape};
use ipp_core::*;

fn submit(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    host.frame_for_test(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> Vec<(u32, EntityId)> {
    submit(host, world, operations).result.unwrap()
}

fn create(
    host: &mut HostRuntime,
    world: WorldId,
    parent: Option<EntityId>,
    values: Vec<ComponentValue>,
) -> EntityId {
    let mut commands = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    commands.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    if let Some(parent) = parent {
        commands.push(Command::PlaceEntity {
            entity: EntityRef::Alias(1),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }
    apply(host, world, commands)[0].1
}

fn camera(host: &mut HostRuntime, world: WorldId) -> OutputRef {
    let entity = create(
        host,
        world,
        None,
        vec![
            ComponentValue::Camera(Camera {
                projection: 1,
                ortho_height: 2.0,
                ..Default::default()
            }),
            ComponentValue::Transform(Transform {
                z: 5.0,
                ..Default::default()
            }),
        ],
    );
    host.bind_output(host.world_ref(world).unwrap(), entity, OutputKind::Camera)
        .unwrap()
}

fn canvas(host: &mut HostRuntime, world: WorldId) -> OutputRef {
    create(
        host,
        world,
        None,
        vec![ComponentValue::GuiLayout(GuiLayout {
            kind: 3,
            ..Default::default()
        })],
    );
    let world = host.world_ref(world).unwrap();
    host.canvas_output(world, [100.0, 100.0], 100.0)
}

fn control(host: &mut HostRuntime, output: OutputRef, style: CanvasStyle) -> EntityId {
    let parent = support::top_level_root(host, output.world().id());
    create(
        host,
        output.world().id(),
        Some(parent),
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 20.0,
                height: 20.0,
                align_x: -1.0,
                align_y: -1.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(style),
        ],
    )
}

fn attach(
    host: &mut HostRuntime,
    world: WorldId,
    output: OutputRef,
    extent: [f32; 2],
    transform: Transform,
) -> EntityId {
    let attachment = WorldAttachment::surface(output);
    let surface = FlatSurface {
        width: extent[0],
        height: extent[1],
        ..Default::default()
    };
    create(
        host,
        world,
        None,
        vec![
            ComponentValue::Transform(transform),
            ComponentValue::FlatSurface(surface),
            ComponentValue::WorldAttachment(attachment),
        ],
    )
}

fn viewport() -> WorldViewport {
    WorldViewport {
        width: 100,
        height: 100,
        device_pixel_ratio: 1.0,
    }
}

fn root(output: OutputRef) -> ViewQueryTarget {
    ViewQueryTarget::RootView {
        output,
        expected_viewport: viewport(),
    }
}

fn frame(host: &mut HostRuntime) {
    let report = host.frame_for_test(0.0).unwrap();
    assert!(
        report.worlds.values().all(Result::is_ok),
        "{:?}",
        report.worlds
    );
    assert!(
        report.publication_errors.is_empty(),
        "{:?}",
        report.publication_errors
    );
}

fn hit(host: &HostRuntime, output: OutputRef, point: [f32; 2]) -> GuiQueryHit<'_> {
    match query_composed_input(host, root(output), point, GuiQueryOptions::default())
        .unwrap()
        .outcome
    {
        GuiQueryOutcome::Hit(hit) => hit,
        GuiQueryOutcome::Unavailable(reason) => panic!("unavailable {reason:?}"),
        GuiQueryOutcome::Blocked {
            reason,
            ..
        } => panic!("blocked {reason:?}"),
        GuiQueryOutcome::Miss => panic!("miss"),
    }
}

#[test]
fn direct_canvas_reverse_paint_clip_and_exact_control_observation() {
    let mut host = crate::support::task_scheduler::host();
    let world = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let output = canvas(&mut host, world);
    let back = control(&mut host, output, CanvasStyle::default());
    let front = control(
        &mut host,
        output,
        CanvasStyle {
            clipped: true,
            clip_max_x: 10.0,
            clip_max_y: 20.0,
            ..Default::default()
        },
    );
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);
    let selected = hit(&host, output, [0.05, 0.05]);
    assert_eq!(selected.hit.target.entity, front);
    assert_eq!(selected.control.unwrap().record.target.entity, front);
    assert_eq!(selected.point, [5.0, 5.0]);
    assert!(selected.path.is_empty());
    assert_eq!(hit(&host, output, [0.15, 0.05]).hit.target.entity, back);
    assert!(matches!(
        query_composed_input(&host, root(output), [0.8, 0.8], GuiQueryOptions::default())
            .unwrap()
            .outcome,
        GuiQueryOutcome::Miss
    ));
}

#[test]
fn nested_camera_physical_aspect_and_spatial_affine_keep_domain_distances() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, SURFACE]))
        .unwrap();
    let spatial = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SURFACE]))
        .unwrap();
    let camera_world = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, SURFACE]))
        .unwrap();
    let panel_world = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let output = camera(&mut host, parent);
    let nested = camera(&mut host, camera_world);
    let panel = canvas(&mut host, panel_world);
    let control = control(&mut host, panel, CanvasStyle::default());
    attach(
        &mut host,
        camera_world,
        panel,
        [0.2, 0.2],
        Transform {
            x: 0.9,
            ..Default::default()
        },
    );
    attach(&mut host, spatial, nested, [3.0, 2.0], Transform::default());
    let child = host.world_ref(spatial).unwrap();
    create(
        &mut host,
        parent,
        None,
        vec![
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child)),
            ComponentValue::Transform(Transform {
                sx: 0.5,
                sy: 0.5,
                sz: 2.0,
                z: 1.0,
                ..Default::default()
            }),
        ],
    );
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);
    let selected = hit(&host, output, [0.725, 0.5]);
    assert_eq!(selected.hit.target.entity, control);
    assert_eq!(selected.output, panel);
    assert!((selected.point[0] - 10.0).abs() < 1e-4);
    assert_eq!(selected.path.len(), 3);
    assert_eq!(selected.path[0].distance, None);
    assert_eq!(selected.path[1].distance, Some(4.0));
    assert_eq!(selected.path[2].distance, Some(5.0));
}

#[test]
fn only_explicit_world_qualified_picking_blocks_panels() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, SURFACE]))
        .unwrap();
    let child = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let output = camera(&mut host, parent);
    let panel = canvas(&mut host, child);
    control(&mut host, panel, CanvasStyle::default());
    attach(&mut host, parent, panel, [0.2; 2], Transform::default());
    let blocker = create(
        &mut host,
        parent,
        None,
        vec![
            ComponentValue::PickingGeometry(PickingGeometry {
                geometry: GeometryDefinition::from(GeometryShape::Box {
                    min: [-1.0, -1.0, 0.0],
                    max: [1.0, 1.0, 1.0],
                })
                .encode()
                .unwrap(),
                ..Default::default()
            }),
            ComponentValue::Transform(Transform::default()),
        ],
    );
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);
    assert_eq!(hit(&host, output, [0.5; 2]).output, panel);
    let identity = host
        .pick_view(root(output), [0.5; 2], false)
        .unwrap()
        .1
        .unwrap()
        .identity;
    assert_eq!(identity.entity, blocker);
    let blockers = [GuiPickingBlocker {
        world: identity.world,
        entity: blocker,
        incarnation: identity.incarnation,
    }];
    assert!(matches!(
        query_composed_input(
            &host,
            root(output),
            [0.5; 2],
            GuiQueryOptions {
                blockers: &blockers,
            }
        )
        .unwrap()
        .outcome,
        GuiQueryOutcome::Blocked {
            reason: GuiQueryBlockReason::PickingGeometry,
            ..
        }
    ));
}

#[test]
fn bound_source_fences_equal_rebind_and_navigation_edits_explicit_producer() {
    use ipp_core::systems::camera::{CameraSystem, CameraViewMotion};
    let mut host = crate::support::task_scheduler::host();
    let world = host.create_world(Default::default(), CAMERA).unwrap();
    let selected = camera(&mut host, world);
    let other = camera(&mut host, world);
    host.world_mut(world)
        .unwrap()
        .enqueue_camera_activate(other.camera_entity().unwrap())
        .unwrap();
    host.set_root_output(selected, viewport()).unwrap();
    frame(&mut host);
    let binding = host.root_output_binding(selected.world()).unwrap().unwrap();
    let source = host.latest_publication(world).unwrap();
    let command = host
        .camera_navigation(
            binding,
            Some(source),
            &[],
            CameraViewMotion::Pan {
                x: 0.25,
                y: 0.0,
            },
        )
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command_with_reply(CameraSystem::ID, 7, 10, command)
        .unwrap();
    let report = host.frame_for_test(0.0).unwrap();
    assert_eq!(
        report.worlds[&world]
            .as_ref()
            .unwrap()
            .system_command_outcomes[0]
            .result,
        Ok(())
    );
    let selected_camera = host
        .output(host.latest_publication(world).unwrap(), selected)
        .unwrap()
        .data::<ipp_core::systems::camera::CameraPublication>()
        .unwrap();
    assert_eq!(selected_camera.pose.point([0.0; 3]), [-0.5, 0.0, 5.0]);
    let other_camera = host
        .output(host.latest_publication(world).unwrap(), other)
        .unwrap()
        .data::<ipp_core::systems::camera::CameraPublication>()
        .unwrap();
    assert_eq!(other_camera.pose.point([0.0; 3]), [0.0, 0.0, 5.0]);
    let source = host.latest_publication(world).unwrap();
    let command = host
        .camera_navigation(
            binding,
            Some(source),
            &[],
            CameraViewMotion::Zoom {
                amount: 1.0,
            },
        )
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command_with_reply(CameraSystem::ID, 7, 11, command)
        .unwrap();
    host.set_root_output(selected, viewport()).unwrap();
    assert!(
        host.resolve_view(ViewQueryTarget::BoundView {
            binding,
            publication: Some(source)
        })
        .is_err()
    );
    let report = host.frame_for_test(0.0).unwrap();
    assert_eq!(
        report.worlds[&world]
            .as_ref()
            .unwrap()
            .system_command_outcomes[0]
            .result,
        Err(ErrorReason::InvalidEntity)
    );
}

#[test]
fn nested_canvas_maps_signed_scale_clip_and_frozen_logical_extent() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, GUI_LAYOUT, SURFACE]),
        )
        .unwrap();
    let child = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let root_canvas = canvas(&mut host, parent);
    let root_canvas_entity = support::top_level_root(&mut host, parent);
    let nested = canvas(&mut host, child);
    let target = control(&mut host, nested, CanvasStyle::default());
    let anchor = attach(&mut host, parent, nested, [0.4, 0.2], Transform::default());
    apply(
        &mut host,
        parent,
        vec![
            Command::PlaceEntity {
                entity: EntityRef::Handle(anchor),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(root_canvas_entity)),
                    before: None,
                },
            },
            Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::CanvasStyle(CanvasStyle {
                    x: 60.0,
                    y: 20.0,
                    scale_x: -1.0,
                    clipped: true,
                    clip_max_x: 40.0,
                    clip_max_y: 10.0,
                    ..Default::default()
                }),
            ),
        ],
    );
    host.set_root_output(root_canvas, viewport()).unwrap();
    frame(&mut host);
    let selected = hit(&host, root_canvas, [0.55, 0.25]);
    assert_eq!(selected.hit.target.entity, target);
    assert!((selected.point[0] - 5.0).abs() < 1e-5);
    assert!((selected.point[1] - 5.0).abs() < 1e-5);
    assert_eq!(selected.path.len(), 1);
    let path = vec![selected.path[0].token.clone()];
    assert!(
        project_composed_point(&host, root(root_canvas), &path, [0.7, 0.25], false, 0)
            .unwrap()
            .is_none()
    );
    let captured = project_composed_point(&host, root(root_canvas), &path, [0.7, 0.25], true, 0)
        .unwrap()
        .unwrap();
    assert!((captured.point[0] + 10.0).abs() < 1e-4);
    assert!((captured.point[1] - 5.0).abs() < 1e-4);
    assert!(matches!(
        query_composed_input(&host, root(root_canvas), [0.55, 0.35], Default::default())
            .unwrap()
            .outcome,
        GuiQueryOutcome::Miss
    ));
}

#[test]
fn nested_navigation_uses_physical_extent_and_rechecks_current_edge() {
    use ipp_core::systems::camera::{CameraPublication, CameraSystem, CameraViewMotion};
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, SURFACE]))
        .unwrap();
    let child = host.create_world(Default::default(), CAMERA).unwrap();
    let output = camera(&mut host, parent);
    let nested = camera(&mut host, child);
    let anchor = attach(&mut host, parent, nested, [3.0, 2.0], Transform::default());
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);
    let binding = host.root_output_binding(output.world()).unwrap().unwrap();
    let source = host.latest_publication(parent).unwrap();
    let token = host.publication(source).unwrap().attachments[0]
        .token
        .clone();
    let command = host
        .camera_navigation(
            binding,
            Some(source),
            std::slice::from_ref(&token),
            CameraViewMotion::Pan {
                x: 0.1,
                y: 0.0,
            },
        )
        .unwrap();
    host.world_mut(child)
        .unwrap()
        .enqueue_system_command_with_reply(CameraSystem::ID, 8, 1, command)
        .unwrap();
    let report = host.frame_for_test(0.0).unwrap();
    assert_eq!(
        report.worlds[&child]
            .as_ref()
            .unwrap()
            .system_command_outcomes[0]
            .result,
        Ok(())
    );
    let pose = &host
        .output(host.latest_publication(child).unwrap(), nested)
        .unwrap()
        .data::<CameraPublication>()
        .unwrap()
        .pose;
    assert!((pose.point([0.0; 3])[0] + 0.3).abs() < 1e-6);
    let source = host.latest_publication(parent).unwrap();
    let command = host
        .camera_navigation(
            binding,
            Some(source),
            &[token],
            CameraViewMotion::Pan {
                x: 0.1,
                y: 0.0,
            },
        )
        .unwrap();
    host.world_mut(child)
        .unwrap()
        .enqueue_system_command_with_reply(CameraSystem::ID, 8, 2, command)
        .unwrap();
    host.world_mut(parent)
        .unwrap()
        .enqueue(Batch {
            id: 33,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::WORLD_ATTACHMENT,
            }],
        })
        .unwrap();
    let report = host.frame_for_test(0.0).unwrap();
    assert_eq!(
        report.worlds[&child]
            .as_ref()
            .unwrap()
            .system_command_outcomes[0]
            .result,
        Err(ErrorReason::InvalidEntity)
    );
}

#[test]
fn current_bound_navigation_survives_advancing_frames_but_exact_source_does_not() {
    use ipp_core::systems::camera::{CameraSystem, CameraViewMotion};
    let mut host = crate::support::task_scheduler::host();
    let world = host.create_world(Default::default(), CAMERA).unwrap();
    let output = camera(&mut host, world);
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);
    let binding = host.root_output_binding(output.world()).unwrap().unwrap();
    let old = host.latest_publication(world).unwrap();
    let current = ViewQueryTarget::BoundView {
        binding,
        publication: None,
    };
    let plane = WorldPlane {
        point: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
    };
    for request in 1..=5 {
        let command = host
            .camera_navigation(
                binding,
                None,
                &[],
                CameraViewMotion::Pan {
                    x: 0.125,
                    y: 0.0,
                },
            )
            .unwrap();
        frame(&mut host);
        frame(&mut host);
        host.world_mut(world)
            .unwrap()
            .enqueue_system_command_with_reply(CameraSystem::ID, 7, request, command)
            .unwrap();
        let report = host.frame_for_test(0.0).unwrap();
        assert_eq!(
            report.worlds[&world]
                .as_ref()
                .unwrap()
                .system_command_outcomes[0]
                .result,
            Ok(())
        );
        let (view, point) = host.project_view(current, [0.5; 2], plane).unwrap();
        assert_eq!(view.publication, host.latest_publication(world).unwrap());
        assert_eq!(point.unwrap()[0], -(request as f32) * 0.25);
    }
    assert!(
        host.resolve_view(ViewQueryTarget::BoundView {
            binding,
            publication: Some(old)
        })
        .is_err()
    );
    assert!(
        host.camera_navigation(
            binding,
            Some(old),
            &[],
            CameraViewMotion::Pan {
                x: 0.1,
                y: 0.0
            }
        )
        .is_err()
    );
    host.set_root_output(output, viewport()).unwrap();
    assert!(host.resolve_view(current).is_err());
    assert!(
        host.camera_navigation(
            binding,
            None,
            &[],
            CameraViewMotion::Pan {
                x: 0.1,
                y: 0.0
            }
        )
        .is_err()
    );
}

#[test]
fn navigation_of_an_animated_camera_lands_and_stop_subtracts_the_contribution() {
    use ipp_core::services::asset_management::{AssetUpload, AssetUploadIdentity};
    use ipp_core::systems::animation::*;
    use ipp_core::systems::camera::{CameraPublication, CameraSystem, CameraViewMotion};
    let mut host = crate::support::task_scheduler::host();
    let world = host
        .create_world(Default::default(), &select(&[ANIMATION, CAMERA]))
        .unwrap();
    let output = camera(&mut host, world);
    let property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::TRANSFORM,
        offsets: vec![0],
    });
    let clip = AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: property.clone(),
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(components::schema::FieldValue::F32(0.0)),
                    interpolation: AnimationInterpolation::Linear,
                },
                AnimationKeyframe {
                    time: 2.0,
                    value: AnimationValue::Field(components::schema::FieldValue::F32(200.0)),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_asset(AssetUpload {
            id: 1,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 1,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    let mut uploaded = false;
    for _ in 0..512 {
        let report = host.frame_for_test(0.0).unwrap();
        if let Some(asset) = report.worlds[&world].as_ref().unwrap().assets.first() {
            assert!(asset.result.is_ok());
            uploaded = true;
            break;
        }
    }
    assert!(uploaded);
    let controller = host
        .world_mut(world)
        .unwrap()
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: "asset://10/1".into(),
                variant: 0,
                track: 0,
                target: output.camera_entity().unwrap(),
                property,
                entity_bindings: vec![],
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            speed: 0.0,
            looping: false,
        })
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    // Halfway through, the controller adds 100 to the camera's x.
    host.world_mut(world)
        .unwrap()
        .control_animation_controller(controller, AnimationPlaybackControl::Seek(1.0))
        .unwrap();
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);
    let pose = |host: &HostRuntime| {
        host.output(host.latest_publication(world).unwrap(), output)
            .unwrap()
            .data::<CameraPublication>()
            .unwrap()
            .pose
            .point([0.0; 3])[0]
    };
    assert_eq!(pose(&host), 100.0);
    let binding = host.root_output_binding(output.world()).unwrap().unwrap();
    let command = host
        .camera_navigation(
            binding,
            None,
            &[],
            CameraViewMotion::Pan {
                x: 0.25,
                y: 0.0,
            },
        )
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command_with_reply(CameraSystem::ID, 7, 1, command)
        .unwrap();
    let report = host.frame_for_test(0.0).unwrap();
    assert_eq!(
        report.worlds[&world]
            .as_ref()
            .unwrap()
            .system_command_outcomes[0]
            .result,
        Ok(())
    );
    // Navigation writes the pose it moved to; the held contribution is not
    // added again, and stopping subtracts it from the navigated pose.
    assert_eq!(pose(&host), 99.5);
    host.world_mut(world)
        .unwrap()
        .control_animation_controller(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    frame(&mut host);
    assert_eq!(pose(&host), -0.5);
}

#[test]
fn faulted_surface_keeps_its_footprint_and_blocks_without_clickthrough() {
    let (mut host, failures) = support::world_failures::host_with_world_failures();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, SURFACE]))
        .unwrap();
    let front_world = host
        .create_world(
            Default::default(),
            &select_with_failures(&[CONSTRAINTS, GUI_LAYOUT]),
        )
        .unwrap();
    let back_world = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let output = camera(&mut host, parent);
    let front = canvas(&mut host, front_world);
    let back = canvas(&mut host, back_world);
    control(&mut host, front, CanvasStyle::default());
    control(&mut host, back, CanvasStyle::default());
    let scalar = create(
        &mut host,
        front_world,
        None,
        vec![ComponentValue::Scalar(Default::default())],
    );
    attach(
        &mut host,
        parent,
        front,
        [0.2; 2],
        Transform {
            z: 1.0,
            ..Default::default()
        },
    );
    attach(&mut host, parent, back, [0.2; 2], Transform::default());
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);
    assert_eq!(hit(&host, output, [0.5; 2]).output, front);

    failures.diverge_scalar_commits(true);
    let failed = submit(
        &mut host,
        front_world,
        vec![Command::insert_value(
            EntityRef::Handle(scalar),
            ComponentValue::Scalar(Default::default()),
        )],
    );
    assert_eq!(
        failed.result.unwrap_err().reason,
        ErrorReason::NonConvergentCommit
    );
    assert!(
        host.world_fault(host.world_ref(front_world).unwrap())
            .unwrap()
            .is_some()
    );
    host.frame_for_test(0.0).unwrap();
    assert!(matches!(
        query_composed_input(&host, root(output), [0.5; 2], GuiQueryOptions::default())
            .unwrap()
            .outcome,
        GuiQueryOutcome::Blocked {
            reason: GuiQueryBlockReason::Unavailable,
            ..
        }
    ));
}

/// A checkbox of `size` at `position` in the canvas root, on plane `layer`.
fn layered_control(
    host: &mut HostRuntime,
    output: OutputRef,
    position: [f32; 2],
    size: [f32; 2],
    layer: u32,
) -> EntityId {
    let parent = support::top_level_root(host, output.world().id());
    create(
        host,
        output.world().id(),
        Some(parent),
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: size[0],
                height: size[1],
                align_x: -1.0,
                align_y: -1.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: position[0],
                y: position[1],
                layer,
                ..Default::default()
            }),
        ],
    )
}

#[test]
fn separated_layers_meet_layer_planes_nearest_first_and_fall_through() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, SURFACE]))
        .unwrap();
    let child = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let output = camera(&mut host, parent);
    let panel = canvas(&mut host, child);
    // The base covers the 1 x 1 m panel; the raised control its left half.
    let base = layered_control(&mut host, panel, [0.0, 0.0], [100.0, 100.0], 0);
    let raised = layered_control(&mut host, panel, [0.0, 0.0], [50.0, 100.0], 1);
    // Turned 45 degrees about +Y, the raised plane 0.25 m in front of the base
    // projects 0.25 m to the left of it: x_local = X / cos - h tan.
    let half = std::f32::consts::FRAC_PI_8;
    let anchor = attach(
        &mut host,
        parent,
        panel,
        [1.0, 1.0],
        Transform {
            qy: half.sin(),
            qw: half.cos(),
            ..Default::default()
        },
    );
    let spacing = |host: &mut HostRuntime, value: f32| {
        apply(
            host,
            parent,
            vec![Command::SetField {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::FLAT_SURFACE,
                field: FieldWrite {
                    offset: std::mem::offset_of!(FlatSurface, layer_spacing) as u32,
                    value: FieldValue::F32(value),
                },
            }],
        );
    };
    spacing(&mut host, 0.25);
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);

    let cos = std::f64::consts::FRAC_1_SQRT_2;
    // Viewport points whose rays meet the base plane at local x 0.1, -0.3 and 0.4.
    let at = |base_x: f64| [((base_x * cos + 1.0) / 2.0) as f32, 0.5];
    let near = |actual: f32, expected: f32| (actual - expected).abs() < 1e-3;

    // The raised plane's control at x -0.15 (content 35) wins over the base at 60.
    let selected = hit(&host, output, at(0.1));
    assert_eq!(selected.hit.target.entity, raised);
    assert_eq!(selected.hit.layer, 1);
    assert!(near(selected.point[0], 35.0) && near(selected.point[1], 50.0));
    let distance = selected.path[0].distance.unwrap();
    // Along the unit ray from z 5: X tan + 5 - h / cos, with X = 0.1 cos and tan 1.
    assert!((distance - (0.1 * cos + 5.0 - 0.25 / cos)).abs() < 1e-4);
    // The raised plane is met outside the panel: the base at content 20 answers,
    // where flat presentation shows the raised control.
    let selected = hit(&host, output, at(-0.3));
    assert_eq!(selected.hit.target.entity, base);
    assert!(near(selected.point[0], 20.0));
    // The raised plane holds no target at content 65: the ray falls through.
    let selected = hit(&host, output, at(0.4));
    assert_eq!(selected.hit.target.entity, base);
    assert!(near(selected.point[0], 90.0));

    // Captured input stays on its target's plane, inside or outside its bounds.
    let path = vec![hit(&host, output, at(0.1)).path[0].token.clone()];
    let on = |host: &HostRuntime, layer| {
        project_composed_point(host, root(output), &path, at(0.1), true, layer)
            .unwrap()
            .unwrap()
            .point
    };
    assert!(near(on(&host, 1)[0], 35.0));
    assert!(near(on(&host, 0)[0], 60.0));

    // Without spacing every layer shares the base plane in reverse painter order.
    spacing(&mut host, 0.0);
    frame(&mut host);
    assert_eq!(hit(&host, output, at(0.1)).hit.target.entity, base);
    assert_eq!(hit(&host, output, at(-0.3)).hit.target.entity, raised);
    assert!(near(on(&host, 1)[0], 60.0));
}

#[test]
fn occupied_layer_ranks_repack_depth_and_capture_when_a_group_disappears() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, SURFACE]))
        .unwrap();
    let child = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let output = camera(&mut host, parent);
    let panel = canvas(&mut host, child);
    // The base covers the panel. Offset 3 covers its left 40 units and
    // offset 1 its right 20; the three occupied levels use ranks 0, 1, 2.
    let base = layered_control(&mut host, panel, [0.0, 0.0], [100.0, 100.0], 0);
    let top = layered_control(&mut host, panel, [0.0, 0.0], [40.0, 100.0], 3);
    let middle = layered_control(&mut host, panel, [80.0, 0.0], [20.0, 100.0], 1);
    // Turned 45 degrees about +Y with 0.1 m between compact ranks, rank n meets
    // a ray n tenths of a metre to the left of where it meets the base.
    let half = std::f32::consts::FRAC_PI_8;
    let anchor = attach(
        &mut host,
        parent,
        panel,
        [1.0, 1.0],
        Transform {
            qy: half.sin(),
            qw: half.cos(),
            ..Default::default()
        },
    );
    apply(
        &mut host,
        parent,
        vec![Command::SetField {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::FLAT_SURFACE,
            field: FieldWrite {
                offset: std::mem::offset_of!(FlatSurface, layer_spacing) as u32,
                value: FieldValue::F32(0.1),
            },
        }],
    );
    host.set_root_output(output, viewport()).unwrap();
    frame(&mut host);

    let cos = std::f64::consts::FRAC_1_SQRT_2;
    let at = |base_x: f64| [((base_x * cos + 1.0) / 2.0) as f32, 0.5];
    let near = |actual: f32, expected: f32| (actual - expected).abs() < 1e-3;
    let layers = |host: &HostRuntime| {
        host.output(host.latest_publication(child).unwrap(), panel)
            .and_then(|chunk| chunk.data::<ipp_core::systems::canvas::CanvasPublication>())
            .unwrap()
            .layers
            .to_vec()
    };
    let path = vec![hit(&host, output, at(-0.05)).path[0].token.clone()];
    let assert_top = |host: &HostRuntime, rank: u32| {
        // Independently intersect parallel planes: base content is 45 and
        // each 0.1 m of normal separation shifts the intersection by 10 units.
        let content_x = 45.0 - 10.0 * rank as f32;
        let offset = f64::from(rank) * 0.1;
        let selected = hit(host, output, at(-0.05));
        assert_eq!(selected.hit.target.entity, top);
        assert_eq!(selected.hit.layer, rank);
        assert!(near(selected.point[0], content_x), "{:?}", selected.point);
        let distance = selected.path[0].distance.unwrap();
        assert!((distance - (-0.05 * cos + 5.0 - offset / cos)).abs() < 1e-4);
        // Captured input projects onto the same plane.
        let captured = project_composed_point(host, root(output), &path, at(-0.05), true, rank)
            .unwrap()
            .unwrap();
        assert!(near(captured.point[0], content_x));
    };

    assert_eq!(layers(&host), [0, 1, 2]);
    assert_top(&host, 2);
    // Where rank 2 holds nothing (content 75), the ray falls through to
    // rank 1's control at content 85.
    let selected = hit(&host, output, at(0.45));
    assert_eq!(selected.hit.target.entity, middle);
    assert!(near(selected.point[0], 85.0));

    // Removing the middle group repacks the top to rank 1, and the other
    // ray reaches the base because the top control does not cover it.
    apply(
        &mut host,
        child,
        vec![Command::Delete {
            entity: EntityRef::Handle(middle),
        }],
    );
    frame(&mut host);
    assert_eq!(layers(&host), [0, 1]);
    assert_top(&host, 1);
    let selected = hit(&host, output, at(0.45));
    assert_eq!(selected.hit.target.entity, base);
    assert!(near(selected.point[0], 95.0));
}

#[test]
fn a_raised_canvas_slot_rises_over_later_base_content() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, GUI_LAYOUT, SURFACE]),
        )
        .unwrap();
    let child = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let root_canvas = canvas(&mut host, parent);
    let root_canvas_entity = support::top_level_root(&mut host, parent);
    let nested = canvas(&mut host, child);
    let target = control(&mut host, nested, CanvasStyle::default());
    let anchor = attach(&mut host, parent, nested, [0.4, 0.4], Transform::default());
    apply(
        &mut host,
        parent,
        vec![
            Command::PlaceEntity {
                entity: EntityRef::Handle(anchor),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(root_canvas_entity)),
                    before: None,
                },
            },
            Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::CanvasStyle(CanvasStyle {
                    layer: 1,
                    ..Default::default()
                }),
            ),
        ],
    );
    // A base control painted after the slot in tree order covers it.
    let cover = layered_control(&mut host, root_canvas, [0.0, 0.0], [100.0, 100.0], 0);
    host.set_root_output(root_canvas, viewport()).unwrap();
    frame(&mut host);
    let selected = hit(&host, root_canvas, [0.05, 0.05]);
    assert_eq!(selected.hit.target.entity, target);
    assert_eq!(selected.path.len(), 1);
    assert_eq!(hit(&host, root_canvas, [0.6, 0.6]).hit.target.entity, cover);
}
