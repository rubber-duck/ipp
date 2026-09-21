//! Local graph/math/lifetime checks supplement the real hierarchy transport/render suite.
mod support;
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, ErrorReason, WorldContext,
    components::{Hierarchy, LookAt, Transform},
};
use support::WorldTestDriver;

fn run(world: &mut WorldContext<'_>, operations: Vec<Command>) -> ipp_core::WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap()
}

fn ok(world: &mut WorldContext<'_>, operations: Vec<Command>) {
    let report = run(world, operations);
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
}

fn create(world: &mut WorldContext<'_>, transform: Option<Transform>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: EntityMetadata::default(),
    }];
    if let Some(transform) = transform {
        operations.push(Command::InsertComponentValue {
            entity: EntityRef::Alias(1),
            value: ComponentValue::Transform(transform),
        });
    }
    let report = run(world, operations);
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

fn put(entity: EntityId, value: ComponentValue) -> Command {
    Command::InsertComponentValue {
        entity: EntityRef::Handle(entity),
        value,
    }
}

fn parent(entity: EntityId, parent: EntityId) -> Command {
    put(
        entity,
        ComponentValue::Hierarchy(Hierarchy {
            parent,
            ..Default::default()
        }),
    )
}

fn aim(entity: EntityId, target: EntityId) -> Command {
    put(
        entity,
        ComponentValue::LookAt(LookAt {
            target,
            ..Default::default()
        }),
    )
}

fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.iter().zip(expected) {
        assert!((a - b).abs() < 2e-5, "{actual:?} != {expected:?}");
    }
}

#[test]
fn nested_affine_composition_preserves_shear_and_local_values_in_reverse_allocation_order() {
    let mut host = ipp_core::HostRuntime::default();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let child_local = Transform {
        x: 1.0,
        qz: (std::f32::consts::PI / 8.0).sin(),
        qw: (std::f32::consts::PI / 8.0).cos(),
        ..Default::default()
    };
    let child = create(&mut world, Some(child_local));
    let middle = create(&mut world, None);
    let outer = Transform {
        x: 10.0,
        sx: 2.0,
        sy: 3.0,
        ..Default::default()
    };
    let root = create(&mut world, Some(outer));
    ok(
        &mut world,
        vec![parent(child, middle), parent(middle, root)],
    );
    let actual = world.world_matrix(child).unwrap();
    let c = std::f32::consts::FRAC_1_SQRT_2;
    close(
        &actual,
        &[
            2.0 * c,
            3.0 * c,
            0.0,
            0.0,
            -2.0 * c,
            3.0 * c,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            12.0,
            0.0,
            0.0,
            1.0,
        ],
    );
    assert!(
        world
            .inspect(child)
            .unwrap()
            .effective
            .contains(&ComponentValue::Transform(child_local))
    );
    let other = create(
        &mut world,
        Some(Transform {
            x: 20.0,
            ..Default::default()
        }),
    );
    ok(&mut world, vec![parent(middle, other)]);
    close(
        &world.world_matrix(child).unwrap()[12..15],
        &[21.0, 0.0, 0.0],
    );
    ok(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(other),
        }],
    );
    close(
        &world.world_matrix(child).unwrap()[12..15],
        &[1.0, 0.0, 0.0],
    );
    let snapshot = world.inspect(middle).unwrap();
    assert!(
        snapshot
            .base
            .contains(&ComponentValue::Hierarchy(Hierarchy::default()))
    );
    let replacement = create(
        &mut world,
        Some(Transform {
            x: 99.0,
            ..Default::default()
        }),
    );
    assert_ne!(replacement, other);
    close(
        &world.world_matrix(child).unwrap()[12..15],
        &[1.0, 0.0, 0.0],
    );
}

#[test]
fn terminal_aim_tracks_through_affine_parent_and_finalizes_children_without_changing_trs() {
    let mut host = ipp_core::HostRuntime::default();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let root = create(
        &mut world,
        Some(Transform {
            x: 1.0,
            sx: 2.0,
            sy: 0.75,
            qz: 0.3,
            qw: 0.95,
            ..Default::default()
        }),
    );
    let local = Transform {
        x: 0.5,
        sx: 0.8,
        sz: 1.3,
        ..Default::default()
    };
    let tracker = create(&mut world, Some(local));
    let child = create(
        &mut world,
        Some(Transform {
            z: -1.0,
            ..Default::default()
        }),
    );
    let target = create(
        &mut world,
        Some(Transform {
            x: -2.0,
            y: 3.0,
            z: -4.0,
            ..Default::default()
        }),
    );
    ok(
        &mut world,
        vec![
            parent(tracker, root),
            parent(child, tracker),
            aim(tracker, target),
        ],
    );
    for position in [[-2.0, 3.0, -4.0], [4.0, -1.0, 2.0], [0.0, 6.0, 0.0]] {
        ok(
            &mut world,
            vec![put(
                target,
                ComponentValue::Transform(Transform {
                    x: position[0],
                    y: position[1],
                    z: position[2],
                    ..Default::default()
                }),
            )],
        );
        let matrix = world.world_matrix(tracker).unwrap();
        let delta: [f32; 3] = std::array::from_fn(|i| position[i] - matrix[12 + i]);
        let forward = [-matrix[8], -matrix[9], -matrix[10]];
        let normalize = |v: [f32; 3]| {
            let n = v.iter().map(|v| v * v).sum::<f32>().sqrt();
            v.map(|v| v / n)
        };
        close(&normalize(forward), &normalize(delta));
        close(
            &world.world_matrix(child).unwrap()[12..15],
            &std::array::from_fn::<_, 3, _>(|i| matrix[12 + i] - matrix[8 + i]),
        );
        assert!(
            world
                .inspect(tracker)
                .unwrap()
                .effective
                .contains(&ComponentValue::Transform(local))
        );
    }
    ok(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(target),
        }],
    );
    let expected = ipp_core::systems::camera::multiply(
        world.world_matrix(root).unwrap(),
        ipp_core::systems::camera::model_matrix(&local).unwrap(),
    );
    close(&world.world_matrix(tracker).unwrap(), &expected);
}

#[test]
fn cycles_and_terminal_dependencies_retain_changes_and_recover_on_correction() {
    let mut host = ipp_core::HostRuntime::default();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let a = create(&mut world, Some(Transform::default()));
    let b = create(
        &mut world,
        Some(Transform {
            x: 2.0,
            ..Default::default()
        }),
    );
    let c = create(
        &mut world,
        Some(Transform {
            z: -3.0,
            ..Default::default()
        }),
    );
    let descendant = create(&mut world, Some(Transform::default()));
    ok(&mut world, vec![parent(b, a), parent(descendant, b)]);
    let report = run(&mut world, vec![parent(a, b)]);
    assert_eq!(
        report.outcomes[0].result.as_ref().unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
    assert!(world.world_matrix(a).is_err());
    assert!(world.world_matrix(b).is_err());
    assert!(world.world_matrix(descendant).is_err());
    assert!(world.world_matrix(c).is_ok());
    ok(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(a),
            component: ComponentValue::HIERARCHY,
        }],
    );
    let report = run(&mut world, vec![aim(a, b)]);
    assert_eq!(
        report.outcomes[0].result.as_ref().unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
    assert!(world.world_matrix(a).is_err());
    assert!(world.world_matrix(b).is_err());
    assert!(world.world_matrix(descendant).is_err());
    assert!(world.world_matrix(c).is_ok());
    ok(&mut world, vec![aim(a, c)]);
    assert!(world.world_matrix(a).is_ok());
    assert!(world.world_matrix(descendant).is_ok());
    let report = run(&mut world, vec![aim(b, c)]);
    assert_eq!(
        report.outcomes[0].result.as_ref().unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
    ok(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(b),
            component: ComponentValue::LOOK_AT,
        }],
    );
}

#[test]
fn coincident_and_pole_targets_are_finite_and_runtime_fields_are_not_serialized() {
    let mut host = ipp_core::HostRuntime::default();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let tracker = create(&mut world, Some(Transform::default()));
    let target = create(&mut world, Some(Transform::default()));
    ok(&mut world, vec![aim(tracker, target)]);
    close(
        &world.world_matrix(tracker).unwrap(),
        &ipp_core::systems::camera::model_matrix(&Transform::default()).unwrap(),
    );
    for y in [1.0, -1.0] {
        ok(
            &mut world,
            vec![put(
                target,
                ComponentValue::Transform(Transform {
                    y,
                    ..Default::default()
                }),
            )],
        );
        let first = world.world_matrix(tracker).unwrap();
        assert!(first.iter().all(|v| v.is_finite()));
        world.update_for_test(0.0).unwrap();
        assert_eq!(world.world_matrix(tracker).unwrap(), first);
        close(&[-first[8], -first[9], -first[10]], &[0.0, y, 0.0]);
    }
    assert_eq!(
        ComponentValue::Hierarchy(Hierarchy::default())
            .fields()
            .len(),
        2
    );
    assert_eq!(ComponentValue::LookAt(LookAt::default()).fields().len(), 2);
}

#[test]
fn discrete_parent_animation_reconciles_graph_and_restores_authored_relationship() {
    use ipp_core::{
        services::asset_management::{AssetUpload, AssetUploadIdentity},
        systems::animation::*,
    };
    let mut host = ipp_core::HostRuntime::default();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let parent_id = create(
        &mut world,
        Some(Transform {
            x: 10.0,
            ..Default::default()
        }),
    );
    let child = create(
        &mut world,
        Some(Transform {
            x: 1.0,
            ..Default::default()
        }),
    );
    ok(&mut world, vec![parent(child, EntityId::from_bits(0))]);
    let property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::HIERARCHY,
        offsets: vec![std::mem::offset_of!(Hierarchy, parent) as u32],
    });
    let clip = AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: property.clone(),
            keys: vec![AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(ipp_core::components::schema::FieldValue::Entity(
                    parent_id,
                )),
                interpolation: AnimationInterpolation::Step,
            }],
        }],
    )
    .unwrap();
    world
        .enqueue_asset(AssetUpload {
            id: 601,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 601,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
    let player = world
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: "asset://10/601".into(),
                variant: 0,
                track: 0,
                target: child,
                property,
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            ..Default::default()
        })
        .unwrap();
    world
        .enqueue_playback(player, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    close(
        &world.world_matrix(child).unwrap()[12..15],
        &[11.0, 0.0, 0.0],
    );
    assert!(
        world
            .inspect(child)
            .unwrap()
            .base
            .contains(&ComponentValue::Hierarchy(Hierarchy::default()))
    );
    world
        .enqueue_playback(player, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    close(
        &world.world_matrix(child).unwrap()[12..15],
        &[1.0, 0.0, 0.0],
    );
}

#[test]
fn deferred_parent_and_target_removal_refreshes_surviving_final_placement() {
    use ipp_core::systems::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Removal(Arc<Mutex<Vec<EntityId>>>);

    impl SystemFactory for Removal {
        fn id(&self) -> SystemId {
            SystemId("test.hierarchy-removal")
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(self.clone()))
        }
    }

    impl System for Removal {
        fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
            for entity in self.0.lock().unwrap().drain(..) {
                context.world.defer_remove_entity(entity).unwrap();
            }
        }
    }

    let removal = Removal::default();
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(removal.clone()));
    let mut host = ipp_core::HostRuntime::with_system_factories(factories).unwrap();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let root = create(
        &mut world,
        Some(Transform {
            x: 10.0,
            ..Default::default()
        }),
    );
    let tracker = create(&mut world, Some(Transform::default()));
    let tip = create(
        &mut world,
        Some(Transform {
            z: -1.0,
            ..Default::default()
        }),
    );
    let target = create(
        &mut world,
        Some(Transform {
            x: 12.0,
            ..Default::default()
        }),
    );
    ok(
        &mut world,
        vec![
            parent(tracker, root),
            parent(tip, tracker),
            aim(tracker, target),
        ],
    );
    close(&world.world_matrix(tip).unwrap()[12..15], &[11.0, 0.0, 0.0]);
    *removal.0.lock().unwrap() = vec![root, target];
    world.update_for_test(0.0).unwrap();
    assert!(world.inspect(root).is_none());
    assert!(world.inspect(target).is_none());
    close(&world.world_matrix(tip).unwrap()[12..15], &[0.0, 0.0, -1.0]);
    assert!(
        world
            .inspect(tracker)
            .unwrap()
            .base
            .contains(&ComponentValue::Hierarchy(Hierarchy::default()))
    );
}

#[cfg(all(feature = "skeletal-animation", feature = "builtin-assets"))]
#[test]
fn bone_parent_composes_pose_and_offset_and_recovers_without_object_fallback() {
    use ipp_core::{
        components::Skeleton,
        services::asset_management::{AssetUpload, AssetUploadIdentity, builtin},
    };
    let mut host = ipp_core::HostRuntime::default();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let rig = create(
        &mut world,
        Some(Transform {
            x: 3.0,
            sx: 2.0,
            ..Default::default()
        }),
    );
    let child = create(
        &mut world,
        Some(Transform {
            y: 0.5,
            ..Default::default()
        }),
    );
    let tip = create(
        &mut world,
        Some(Transform {
            y: 0.25,
            ..Default::default()
        }),
    );
    ok(
        &mut world,
        vec![
            put(
                rig,
                ComponentValue::Skeleton(Skeleton {
                    source: "asset://3/971".into(),
                    ..Default::default()
                }),
            ),
            put(
                child,
                ComponentValue::Hierarchy(Hierarchy {
                    parent: rig,
                    parent_bone: 1,
                    ..Default::default()
                }),
            ),
            parent(tip, child),
        ],
    );
    assert!(world.world_matrix(child).is_err());
    assert!(world.world_matrix(tip).is_err());
    world
        .enqueue_asset(AssetUpload {
            id: 971,
            key: AssetUploadIdentity {
                kind: ipp_core::SKELETON_TYPE,
                asset: 971,
                variant: 0,
            },
            bytes: builtin::rig(ipp_core::SKELETON_TYPE, "ipp://skeleton/rig-strip").unwrap(),
        })
        .unwrap();
    for _ in 0..32 {
        world.update_for_test(0.0).unwrap();
    }
    close(
        &world.world_matrix(child).unwrap()[12..15],
        &[3.0, 1.5, 0.0],
    );
    close(&world.world_matrix(tip).unwrap()[12..15], &[3.0, 1.75, 0.0]);

    // Independent 90-degree Z joint rotation: the child's +Y offset becomes -X.
    world
        .enqueue_asset(AssetUpload {
            id: 972,
            key: AssetUploadIdentity {
                kind: ipp_core::POSE_TYPE,
                asset: 972,
                variant: 0,
            },
            bytes: builtin::rig(ipp_core::POSE_TYPE, "ipp://pose/rig-strip-bent").unwrap(),
        })
        .unwrap();
    for _ in 0..32 {
        world.update_for_test(0.0).unwrap();
    }
    ok(
        &mut world,
        vec![put(
            rig,
            ComponentValue::Skeleton(Skeleton {
                source: "asset://3/971".into(),
                pose_source: "asset://4/972".into(),
                ..Default::default()
            }),
        )],
    );
    let moved = world.world_matrix(child).unwrap();
    assert!(moved[12] < 2.9, "{moved:?}");
    assert!(world.world_matrix(tip).unwrap()[12] < moved[12]);

    ok(
        &mut world,
        vec![put(
            child,
            ComponentValue::Hierarchy(Hierarchy {
                parent: rig,
                parent_bone: 30,
                ..Default::default()
            }),
        )],
    );
    assert!(world.world_matrix(child).is_err());
    assert!(world.world_matrix(tip).is_err());
    ok(
        &mut world,
        vec![put(
            child,
            ComponentValue::Hierarchy(Hierarchy {
                parent: rig,
                parent_bone: 1,
                ..Default::default()
            }),
        )],
    );
    close(&world.world_matrix(child).unwrap(), &moved);

    let target = create(
        &mut world,
        Some(Transform {
            x: 5.0,
            y: 2.0,
            z: -3.0,
            ..Default::default()
        }),
    );
    ok(&mut world, vec![aim(child, target)]);
    let matrix = world.world_matrix(child).unwrap();
    let direction = [5.0 - matrix[12], 2.0 - matrix[13], -3.0 - matrix[14]];
    let forward = [-matrix[8], -matrix[9], -matrix[10]];
    let length = |v: [f32; 3]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    close(
        &forward.map(|x| x / length(forward)),
        &direction.map(|x| x / length(direction)),
    );

    ok(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(rig),
        }],
    );
    // Parent deletion preserves the local offset and ignores the now-orphaned bone selector.
    close(
        &world.world_matrix(child).unwrap()[12..15],
        &[0.0, 0.5, 0.0],
    );
}
