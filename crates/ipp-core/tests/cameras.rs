//! Core camera lifetime and ordering invariants; transport/render evidence is external.

mod support;
use support::WorldTestDriver;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, ErrorReason, WorldLimits,
    WorldUpdateReport,
    components::{Camera, Transform},
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

fn insert(entity: EntityRef, value: ComponentValue) -> Command {
    Command::InsertComponentValue {
        entity,
        value,
    }
}

fn camera(world: &mut ipp_core::WorldContext<'_>, name: &str) -> EntityId {
    let report = run(
        world,
        vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata {
                    symbolic_id: Some(name.into()),
                    classes: vec![],
                },
            },
            insert(
                EntityRef::Alias(0),
                ComponentValue::Camera(Camera::default()),
            ),
            insert(
                EntityRef::Alias(0),
                ComponentValue::Transform(Transform::default()),
            ),
        ],
    );
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

#[test]
fn activation_is_explicit_ordered_and_failed_activation_preserves_selection() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    assert_eq!(world.active_camera(), None);
    assert!(world.active_camera_component().is_none());
    assert_eq!(
        world.prepare_camera(100, 100),
        Err(ErrorReason::NoActiveCamera)
    );
    let first = camera(&mut world, "first");
    let second = camera(&mut world, "second");
    world.enqueue_camera_activate(first).unwrap();
    world.enqueue_camera_activate(first).unwrap();
    world
        .enqueue_camera_activate(EntityId::from_bits(u64::MAX))
        .unwrap();
    world.enqueue_camera_activate(second).unwrap();
    assert_eq!(world.active_camera(), None);
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(
        report
            .camera_state_changes
            .iter()
            .map(|v| (v.tick, v.changes.active_camera))
            .collect::<Vec<_>>(),
        vec![(report.tick, Some(first)), (report.tick, Some(second)),]
    );
    assert_eq!(world.active_camera(), Some(second));
    assert_eq!(world.active_camera_component(), Some(&Camera::default()));
    assert_eq!(world.prepare_camera(100, 100).unwrap().entity, second);
    world.enqueue_camera_activate(second).unwrap();
    assert!(
        world
            .update_for_test(0.0)
            .unwrap()
            .camera_state_changes
            .is_empty()
    );
}

#[test]
fn active_deletion_and_required_removal_keep_partial_batch_changes() {
    for component in [
        None,
        Some(ComponentValue::CAMERA),
        Some(ComponentValue::TRANSFORM),
    ] {
        let mut host = ipp_core::HostRuntime::new();
        let id = host.create_world(Default::default()).unwrap();
        let mut world = host.world_mut(id).unwrap();
        let first = camera(&mut world, "first");
        let second = camera(&mut world, "second");
        world.enqueue_camera_activate(first).unwrap();
        world.update_for_test(0.0).unwrap();
        let operation = match component {
            Some(component) => Command::RemoveComponent {
                entity: EntityRef::Handle(first),
                component,
            },
            None => Command::Delete {
                entity: EntityRef::Handle(first),
            },
        };
        let report = run(
            &mut world,
            vec![
                Command::Delete {
                    entity: EntityRef::Handle(second),
                },
                operation,
            ],
        );
        let error = report.outcomes[0].result.as_ref().unwrap_err();
        assert_eq!(
            (error.scope, error.operation, error.reason),
            (
                ipp_core::BatchErrorScope::Commit,
                None,
                ErrorReason::ActiveCamera
            )
        );
        assert!(world.inspect(second).is_none());
        if let Some(component) = component {
            assert!(
                !world
                    .inspect(first)
                    .unwrap()
                    .effective
                    .iter()
                    .any(|value| value.type_id() == component)
            );
        } else {
            assert!(world.inspect(first).is_none());
        }
        assert!(world.prepare_camera(100, 100).is_err());
        assert_eq!(
            world.active_camera_component().is_some(),
            component == Some(ComponentValue::TRANSFORM)
        );
    }
}

#[test]
fn invalid_projection_updates_can_be_corrected_explicitly() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = camera(&mut world, "camera");
    world.enqueue_camera_activate(entity).unwrap();
    world.update_for_test(0.0).unwrap();
    for invalid in [
        Camera {
            projection: 2,
            ..Camera::default()
        },
        Camera {
            near: 0.0,
            ..Camera::default()
        },
        Camera {
            near: 100.0,
            far: 1.0,
            ..Camera::default()
        },
        Camera {
            fov_y: std::f32::consts::PI,
            ..Camera::default()
        },
        Camera {
            ortho_height: 0.0,
            ..Camera::default()
        },
        Camera {
            far: f32::INFINITY,
            ..Camera::default()
        },
        Camera {
            focus_distance: 0.0,
            ..Camera::default()
        },
        Camera {
            focus_distance: -1.0,
            ..Camera::default()
        },
        Camera {
            focus_distance: f32::NAN,
            ..Camera::default()
        },
    ] {
        let report = run(
            &mut world,
            vec![insert(
                EntityRef::Handle(entity),
                ComponentValue::Camera(invalid),
            )],
        );
        assert_eq!(
            report.outcomes[0].result.as_ref().unwrap_err().reason,
            ErrorReason::InvalidValue
        );
        // Invalid activation can remove the effective component. Explicit insertion repairs it.
        assert!(
            run(
                &mut world,
                vec![insert(
                    EntityRef::Handle(entity),
                    ComponentValue::Camera(Camera::default())
                )]
            )
            .outcomes[0]
                .result
                .is_ok()
        );
        assert_eq!(world.prepare_camera(100, 100).unwrap().entity, entity);
    }
    assert!(
        run(
            &mut world,
            vec![insert(
                EntityRef::Handle(entity),
                ComponentValue::Camera(Camera {
                    projection: 1,
                    ..Camera::default()
                })
            )]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!(world.active_camera(), Some(entity));
}

#[test]
fn activation_observes_prior_mutations_and_shares_queue_bounds() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(WorldLimits {
            max_queued_batches: 2,
            ..WorldLimits::default()
        })
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = camera(&mut world, "camera");
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CAMERA,
            }],
        })
        .unwrap();
    world.enqueue_camera_activate(entity).unwrap();
    assert_eq!(
        world.enqueue_camera_activate(entity),
        Err(ErrorReason::Capacity)
    );
    assert_eq!(
        world.update_for_test(f64::NAN),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(world.active_camera(), None);
    let report = world.update_for_test(0.0).unwrap();
    assert!(report.outcomes[0].result.is_ok());
    assert!(report.camera_state_changes.is_empty());
    assert_eq!(world.active_camera(), None);
}

mod overlays {
    use super::*;
    use ipp_core::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    #[test]
    fn failed_owned_camera_cleanup_keeps_applied_release() {
        for cleanup in 0..4 {
            let mut world_host = ipp_core::HostRuntime::new();
            let world_id = world_host
                .create_world(ipp_core::WorldLimits::default())
                .unwrap();
            let mut world = world_host.world_mut(world_id).unwrap();
            let report = run(
                &mut world,
                vec![
                    Command::CreateStateOverlayOwner {
                        alias: 0,
                    },
                    Command::AttachEntityOverlayBinding {
                        owner: StateOverlayRef::Alias(0),
                        alias: 1,
                        symbolic_id: "owned".into(),
                        mode: EntityOverlayMode::Owned,
                    },
                    Command::AttachComponentStateOverlay {
                        owner: StateOverlayRef::Alias(0),
                        binding: StateOverlayRef::Alias(1),
                        alias: 2,
                        component: ComponentValue::CAMERA,
                        mode: ComponentOverlayMode::Owned,
                        fields: vec![],
                    },
                    Command::AttachComponentStateOverlay {
                        owner: StateOverlayRef::Alias(0),
                        binding: StateOverlayRef::Alias(1),
                        alias: 3,
                        component: ComponentValue::TRANSFORM,
                        mode: ComponentOverlayMode::Owned,
                        fields: vec![],
                    },
                ],
            );
            assert!(report.outcomes[0].result.is_ok(), "{report:?}");
            let resources = &report.outcomes[0].state_overlays;
            let owner = StateOverlayRef::Handle(resources[0].id);
            let entity = world.entities()[0].id;
            let alternate = camera(&mut world, "alternate");
            world.enqueue_camera_activate(entity).unwrap();
            world.update_for_test(0.0).unwrap();
            world
                .enqueue_camera_navigate(ipp_core::CameraMotion::Zoom {
                    amount: 0.5,
                })
                .unwrap();
            let report = world.update_for_test(0.0).unwrap();
            assert!(report.camera_state_changes.is_empty());
            assert!(report.diagnostics.is_empty());
            let operation = [
                Command::ReleaseEntityOverlayBinding {
                    owner,
                    binding: StateOverlayRef::Handle(resources[1].id),
                },
                Command::ReleaseStateOverlayOwner {
                    owner,
                },
                Command::ReleaseComponentStateOverlay {
                    owner,
                    overlay: StateOverlayRef::Handle(resources[2].id),
                },
                Command::ReleaseComponentStateOverlay {
                    owner,
                    overlay: StateOverlayRef::Handle(resources[3].id),
                },
            ][cleanup]
                .clone();
            {
                let report = run(&mut world, vec![operation]);
                assert_eq!(
                    report.outcomes[0].result.as_ref().unwrap_err().reason,
                    ErrorReason::ActiveCamera
                );
                assert!(world.prepare_camera(100, 100).is_err());
                assert_eq!(world.active_camera(), Some(entity));
            }
            world.enqueue_camera_activate(alternate).unwrap();
            assert!(
                run(
                    &mut world,
                    vec![Command::ReleaseStateOverlayOwner {
                        owner
                    }]
                )
                .outcomes[0]
                    .result
                    .is_ok()
            );
            assert!(world.inspect(entity).is_none());
        }
    }

    #[test]
    fn auto_fallback_preserves_active_required_components_and_final_release_rejects() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let entity = camera(&mut world, "camera");
        let report = run(
            &mut world,
            vec![
                Command::CreateStateOverlayOwner {
                    alias: 0,
                },
                Command::AttachEntityOverlayBinding {
                    owner: StateOverlayRef::Alias(0),
                    alias: 1,
                    symbolic_id: "camera".into(),
                    mode: EntityOverlayMode::Bound,
                },
                Command::AttachComponentStateOverlay {
                    owner: StateOverlayRef::Alias(0),
                    binding: StateOverlayRef::Alias(1),
                    alias: 2,
                    component: ComponentValue::CAMERA,
                    mode: ComponentOverlayMode::Auto,
                    fields: vec![],
                },
            ],
        );
        let owner = StateOverlayRef::Handle(report.outcomes[0].state_overlays[0].id);
        world.enqueue_camera_activate(entity).unwrap();
        let report = run(
            &mut world,
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CAMERA,
            }],
        );
        assert!(report.outcomes[0].result.is_ok());
        assert_eq!(world.active_camera(), Some(entity));
        let report = run(
            &mut world,
            vec![Command::ReleaseStateOverlayOwner {
                owner,
            }],
        );
        assert_eq!(
            report.outcomes[0].result.as_ref().unwrap_err().reason,
            ErrorReason::ActiveCamera
        );
    }
}
