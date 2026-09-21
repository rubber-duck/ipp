//! SkeletonJoint clips share property-player clocks, ordered layers and strict target lifetimes.
#![cfg(all(feature = "skeletal-animation", feature = "builtin-assets"))]

mod support;
use support::WorldTestDriver;

use ipp_core::{
    components::{Scalar, Skeleton, Transform},
    services::asset_management::*,
    systems::animation::*,
    *,
};
use std::mem::offset_of;

fn apply(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> BatchOutcome {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap().outcomes.remove(0)
}

fn create(world: &mut ipp_core::WorldContext<'_>, values: Vec<ComponentValue>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value,
            }),
    );
    apply(world, operations).result.unwrap()[0].1
}

fn upload(world: &mut ipp_core::WorldContext<'_>, kind: AssetTypeId, id: u64, bytes: Vec<u8>) {
    world
        .enqueue_asset(AssetUpload {
            id,
            key: AssetUploadIdentity {
                kind,
                asset: id,
                variant: 0,
            },
            bytes,
        })
        .unwrap();
    assert!(world.update_for_test(0.0).unwrap().assets[0].result.is_ok());
}

fn fixture(host: &mut ipp_core::HostRuntime) -> (ipp_core::WorldContext<'_>, EntityId, EntityId) {
    let world_id = host.create_world(ipp_core::WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let rig = builtin::rig(SKELETON_TYPE, "ipp://skeleton/rig-strip").unwrap();
    upload(&mut world, SKELETON_TYPE, 1, rig.clone());
    let mut other = rig;
    other[64..68].copy_from_slice(&3.0f32.to_le_bytes());
    upload(&mut world, SKELETON_TYPE, 2, other);
    let mut add = || {
        create(
            &mut world,
            vec![
                ComponentValue::Transform(Transform::default()),
                ComponentValue::Scalar(Scalar {
                    value: 7.0,
                }),
                ComponentValue::Skeleton(Skeleton {
                    source: "asset://3/1".into(),
                    ..Default::default()
                }),
            ],
        )
    };
    let a = add();
    let b = add();
    (world, a, b)
}

fn joint(angle: f32) -> Transform {
    let (qz, qw) = (angle * 0.5).sin_cos();
    Transform {
        y: 1.0,
        qz,
        qw,
        ..Default::default()
    }
}

fn pose_key(
    time: f64,
    pose: Vec<Transform>,
    interpolation: AnimationInterpolation,
) -> AnimationKeyframe {
    AnimationKeyframe {
        time,
        value: AnimationValue::Pose(pose),
        interpolation,
    }
}

fn track(joints: Vec<u32>, a: Vec<Transform>, b: Vec<Transform>) -> AnimationTrack {
    AnimationTrack {
        target: AnimationTrackTarget::Joints(joints),
        keys: vec![
            pose_key(0.0, a, AnimationInterpolation::Linear),
            pose_key(2.0, b, AnimationInterpolation::Step),
        ],
    }
}

fn bend() -> AnimationClip {
    AnimationClip::new(
        2.0,
        vec![track(
            vec![1],
            vec![joint(0.0)],
            vec![joint(std::f32::consts::FRAC_PI_2)],
        )],
    )
    .unwrap()
}

#[derive(Clone)]
struct PlaybackSettings {
    weight: f32,
    additive: bool,
    reference_time: f32,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            weight: 1.0,
            additive: false,
            reference_time: 0.0,
        }
    }
}

fn player(
    world: &mut WorldContext<'_>,
    target: EntityId,
    clip: &AnimationClip,
    id: u64,
    settings: PlaybackSettings,
) -> AnimationControllerId {
    upload(world, ANIMATION_TYPE, id, clip.encode());
    world
        .create_animation_controller(AnimationControllerDescription {
            drivers: clip
                .tracks()
                .iter()
                .enumerate()
                .map(|(index, track)| AnimationDriverDescription {
                    target,
                    source: format!("asset://10/{id}"),
                    variant: 0,
                    track: index as u32,
                    property: track.target().clone(),
                    weight: settings.weight,
                    additive: settings.additive,
                    reference_time: settings.reference_time,
                    repeat: false,
                })
                .collect(),
            ..Default::default()
        })
        .unwrap()
}

fn paused(world: &mut ipp_core::WorldContext<'_>, player: AnimationControllerId, time: f64) {
    for control in [
        AnimationPlaybackControl::Seek(time),
        AnimationPlaybackControl::Play,
        AnimationPlaybackControl::Pause,
    ] {
        world.enqueue_playback(player, control).unwrap();
    }
    world.update_for_test(100.0).unwrap();
    assert_eq!(world.animation_controller(player).unwrap().time, time);
    assert_eq!(
        world.animation_controller(player).unwrap().state,
        AnimationPlaybackStatus::Paused
    );
}

fn set(entity: EntityId, component: u16, offset: usize, value: FieldValue) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component,
        field: FieldWrite {
            offset: offset as u32,
            value,
        },
    }
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}

#[test]
fn pose_keys_blend_local_trs_shortest_arc_and_bezier_time() {
    let mut end = joint(std::f32::consts::FRAC_PI_2);
    end.x = 2.0;
    end.sy = 3.0;
    end.qz = -end.qz * 2.0;
    end.qw = -end.qw * 2.0;
    let clip = AnimationClip::new(2.0, vec![track(vec![1], vec![joint(0.0)], vec![end])]).unwrap();
    let encoded = clip.encode();
    assert_eq!(encoded[4], 2);
    assert_eq!(AnimationClip::decode(&encoded).unwrap(), clip);
    let AnimationValue::Pose(mid) = clip.sample(0, 1.0) else {
        panic!()
    };
    close(mid[0].x, 1.0);
    close(mid[0].sy, 2.0);
    close(mid[0].qz, (std::f32::consts::PI / 8.0).sin());
    close(mid[0].qw, (std::f32::consts::PI / 8.0).cos());
    let mut curved = clip.tracks()[0].interchange();
    curved.keys[1].value = curved.keys[0].value.clone();
    let handle = AnimationValue::Pose(vec![Transform {
        x: 8.0,
        ..joint(0.0)
    }]);
    curved.keys[0].interpolation = AnimationInterpolation::Bezier {
        time1: 0.0,
        value1: handle.clone(),
        time2: 0.5,
        value2: handle,
    };
    let clip = AnimationClip::new(2.0, vec![curved]).unwrap();
    let AnimationValue::Pose(mid) = clip.sample(0, 0.4375) else {
        panic!()
    };
    close(mid[0].x, 6.0);
    for size in 0..encoded.len() {
        assert!(AnimationClip::decode(&encoded[..size]).is_err());
    }
    let mut invalid = clip.tracks()[0].interchange();
    invalid.target = AnimationTrackTarget::Joints(vec![1, 0]);
    assert!(AnimationClip::new(2.0, vec![invalid]).is_err());
    assert!(
        AnimationClip::new(
            2.0,
            vec![
                clip.tracks()[0].interchange(),
                clip.tracks()[0].interchange()
            ]
        )
        .is_ok()
    );
}

#[test]
fn independent_players_preserve_base_unkeyed_joints_and_pose_storage() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, b) = fixture(&mut fixture_host);
    let p = player(&mut world, a, &bend(), 100, Default::default());
    let q = player(&mut world, b, &bend(), 101, Default::default());
    let base = world.inspect(a).unwrap().base;
    let address = world.skeleton_pose(a).unwrap().as_ptr();
    paused(&mut world, p, 1.0);
    paused(&mut world, q, 2.0);
    close(
        world.skeleton_pose(a).unwrap()[1].qz,
        (std::f32::consts::PI / 8.0).sin(),
    );
    close(
        world.skeleton_pose(b).unwrap()[1].qz,
        std::f32::consts::FRAC_1_SQRT_2,
    );
    assert_eq!(world.skeleton_pose(a).unwrap()[0], Transform::default());
    assert_eq!(world.skeleton_pose(a).unwrap().as_ptr(), address);
    assert_eq!(world.inspect(a).unwrap().base, base);
    // Typed joint animation writes private local buffers, so both exposed views
    // retain the producer's sparse joint declaration throughout playback.
    let observed = world.inspect(a).unwrap();
    let authored = observed
        .base
        .iter()
        .find(|value| matches!(value, ComponentValue::Skeleton(_)))
        .unwrap();
    let effective = observed
        .effective
        .iter()
        .find(|value| matches!(value, ComponentValue::Skeleton(_)))
        .unwrap();
    assert_eq!(authored, effective);
    world
        .enqueue_playback(p, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(world.skeleton_pose(a).unwrap()[1], joint(0.0));
    close(
        world.skeleton_pose(b).unwrap()[1].qz,
        std::f32::consts::FRAC_1_SQRT_2,
    );
}

#[test]
fn skeleton_payload_residency_freezes_joint_playback_without_replacing_its_buffer() {
    use support::HostWorldTestDriver;
    let mut host = HostRuntime::new();
    let (mut world, entity, _) = fixture(&mut host);
    let world_id = world.id();
    let controller = player(&mut world, entity, &bend(), 100, Default::default());
    paused(&mut world, controller, 1.0);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let address = world.skeleton_pose(entity).unwrap().as_ptr();
    let sampled = world.skeleton_pose(entity).unwrap().to_vec();
    let key = world
        .resolve_asset_key(AssetUploadIdentity {
            kind: SKELETON_TYPE,
            asset: 1,
            variant: 0,
        })
        .unwrap();
    world.asset_resources_mut().unload(key);
    drop(world);
    host.flush_resource_lifecycle();
    {
        let mut world = host.world_mut(world_id).unwrap();
        assert!(
            world
                .asset_resources()
                .get_typed::<SkeletonAsset>(key)
                .is_none()
        );
        world
            .enqueue(Batch {
                id: 80,
                operations: vec![set(
                    entity,
                    ComponentValue::SCALAR,
                    0,
                    FieldValue::F32(77.0),
                )],
            })
            .unwrap();
        world.prepare_update(0.25).unwrap();
        let report = world.step(0.25).unwrap();
        assert!(report.outcomes[0].result.is_ok());
        assert!(
            !report
                .playback_events
                .iter()
                .any(|event| event.kind == AnimationPlaybackEventKind::Invalidated)
        );
        assert_eq!(
            world.animation_controller(controller).unwrap().state,
            AnimationPlaybackStatus::Playing
        );
        assert_eq!(world.animation_controller(controller).unwrap().time, 1.0);
        assert!(world.skeleton_pose(entity).is_none());
    }
    for _ in 0..16 {
        host.update_world_for_test(world_id, 0.0).unwrap();
        if host
            .world_mut(world_id)
            .unwrap()
            .skeleton_pose(entity)
            .is_some()
        {
            break;
        }
    }
    let world = host.world_mut(world_id).unwrap();
    assert_eq!(world.skeleton_pose(entity).unwrap(), sampled);
    assert_eq!(world.skeleton_pose(entity).unwrap().as_ptr(), address);
    assert_eq!(world.animation_controller(controller).unwrap().time, 1.0);
}

#[test]
fn changing_pose_source_suspends_prepared_joint_playback_until_the_new_pose_loads() {
    use support::HostWorldTestDriver;
    let mut host = HostRuntime::new();
    let (mut world, entity, _) = fixture(&mut host);
    let world_id = world.id();
    upload(
        &mut world,
        POSE_TYPE,
        4,
        builtin::rig(POSE_TYPE, "ipp://pose/rig-strip-bent").unwrap(),
    );
    let controller = player(&mut world, entity, &bend(), 100, Default::default());
    paused(&mut world, controller, 1.0);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let key = world
        .resolve_asset_key(AssetUploadIdentity {
            kind: POSE_TYPE,
            asset: 4,
            variant: 0,
        })
        .unwrap();
    world.asset_resources_mut().unload(key);
    drop(world);
    host.flush_resource_lifecycle();
    {
        let mut world = host.world_mut(world_id).unwrap();
        world
            .enqueue(Batch {
                id: 90,
                operations: vec![set(
                    entity,
                    ComponentValue::SKELETON,
                    offset_of!(Skeleton, pose_source),
                    FieldValue::String("asset://4/4".into()),
                )],
            })
            .unwrap();
        world.prepare_update(0.25).unwrap();
        let report = world.step(0.25).unwrap();
        assert!(report.outcomes[0].result.is_ok());
        assert!(report.playback_events.is_empty());
        assert_eq!(world.animation_controller(controller).unwrap().time, 1.0);
        assert!(world.skeleton_pose(entity).is_none());
    }
    for _ in 0..16 {
        host.update_world_for_test(world_id, 0.0).unwrap();
        if host
            .world_mut(world_id)
            .unwrap()
            .skeleton_pose(entity)
            .is_some()
        {
            break;
        }
    }
    let world = host.world_mut(world_id).unwrap();
    assert!(world.skeleton_pose(entity).is_some());
    assert_eq!(world.animation_controller(controller).unwrap().time, 1.0);
}

#[test]
fn discrete_pose_inputs_rebase_unkeyed_locals_and_preserve_ordered_joint_samples() {
    for (pose_first, expected_x) in [(true, 7.0), (false, 4.5)] {
        let mut fixture_host = ipp_core::HostRuntime::new();
        let (mut world, entity, _) = fixture(&mut fixture_host);
        let mut pose = builtin::rig(POSE_TYPE, "ipp://pose/rig-strip-bent").unwrap();
        pose[12..16].copy_from_slice(&5.0f32.to_le_bytes());
        upload(&mut world, POSE_TYPE, 4, pose);
        let source = AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::SKELETON,
                offsets: vec![offset_of!(Skeleton, pose_source) as u32],
            }),
            keys: vec![AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(ipp_core::components::schema::FieldValue::String(
                    "asset://4/4".into(),
                )),
                interpolation: AnimationInterpolation::Step,
            }],
        };
        let target = Transform {
            x: 9.0,
            ..Default::default()
        };
        let joints = track(vec![0], vec![target], vec![target]);
        let tracks = if pose_first {
            vec![source, joints]
        } else {
            vec![joints, source]
        };
        let clip = AnimationClip::new(2.0, tracks).unwrap();
        let controller = player(&mut world, entity, &clip, 100, Default::default());
        let mut description = world.animation_controller(controller).unwrap().description;
        description
            .drivers
            .iter_mut()
            .find(|driver| matches!(driver.property, AnimationTrackTarget::Joints(_)))
            .unwrap()
            .weight = 0.5;
        world
            .update_animation_controller(controller, description)
            .unwrap();
        let address = world.skeleton_pose(entity).unwrap().as_ptr();
        paused(&mut world, controller, 1.0);
        let local = world.skeleton_pose(entity).unwrap();
        close(local[0].x, expected_x);
        close(local[1].qz, std::f32::consts::FRAC_1_SQRT_2);
        assert_eq!(local.as_ptr(), address);
        assert!(world.inspect(entity).unwrap().base.iter().any(|value| matches!(value, ComponentValue::Skeleton(value) if value.pose_source.is_empty() && value.joints.is_empty())));
    }
}

#[test]
fn joint_baseline_tracks_authored_pose_changes_without_accumulating_samples() {
    let mut host = HostRuntime::new();
    let (mut world, entity, _) = fixture(&mut host);
    let mut pose = builtin::rig(POSE_TYPE, "ipp://pose/rig-strip-bent").unwrap();
    pose[12..16].copy_from_slice(&5.0f32.to_le_bytes());
    upload(&mut world, POSE_TYPE, 4, pose);
    let target = Transform {
        x: 9.0,
        ..Default::default()
    };
    let clip = AnimationClip::new(2.0, vec![track(vec![0], vec![target], vec![target])]).unwrap();
    let controller = player(
        &mut world,
        entity,
        &clip,
        100,
        PlaybackSettings {
            weight: 0.5,
            ..Default::default()
        },
    );
    paused(&mut world, controller, 1.0);
    for _ in 0..4 {
        world.update_for_test(0.0).unwrap();
        close(world.skeleton_pose(entity).unwrap()[0].x, 4.5);
    }
    assert!(
        apply(
            &mut world,
            vec![set(
                entity,
                ComponentValue::SKELETON,
                offset_of!(Skeleton, pose_source),
                FieldValue::String("asset://4/4".into()),
            )]
        )
        .result
        .is_ok()
    );
    for _ in 0..4 {
        world.update_for_test(0.0).unwrap();
        close(world.skeleton_pose(entity).unwrap()[0].x, 7.0);
    }
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    close(world.skeleton_pose(entity).unwrap()[0].x, 5.0);
}

#[test]
fn source_replacement_in_failed_batch_invalidates_binding() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, _) = fixture(&mut fixture_host);
    let p = player(&mut world, a, &bend(), 100, Default::default());
    paused(&mut world, p, 1.0);
    let replace = set(
        a,
        ComponentValue::SKELETON,
        offset_of!(Skeleton, source),
        FieldValue::String("asset://3/2".into()),
    );
    assert!(
        apply(
            &mut world,
            vec![
                replace.clone(),
                set(
                    a,
                    ComponentValue::TRANSFORM,
                    offset_of!(Transform, sx),
                    FieldValue::F32(-1.0)
                )
            ]
        )
        .result
        .is_err()
    );
    assert_eq!(
        world.animation_controller(p).unwrap().state,
        AnimationPlaybackStatus::Stopped
    );
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(a),
            value: ComponentValue::Transform(Transform::default()),
        }],
    )
    .result
    .unwrap();
    close(world.skeleton_pose(a).unwrap()[1].y, 3.0);
    paused(&mut world, p, 1.0);
    close(world.skeleton_pose(a).unwrap()[1].y, 1.0);
    apply(
        &mut world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(a),
                component: ComponentValue::SKELETON,
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(a),
                value: ComponentValue::Skeleton(Skeleton {
                    source: "asset://3/1".into(),
                    ..Default::default()
                }),
            },
        ],
    )
    .result
    .unwrap();
    assert_eq!(
        world.animation_controller(p).unwrap().state,
        AnimationPlaybackStatus::Stopped
    );
}

#[test]
fn animated_source_replacement_keeps_partial_samples_without_retrying_clocks() {
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, a, _) = fixture(&mut fixture_host);
    let p = player(&mut world, a, &bend(), 100, Default::default());
    let source = AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::SKELETON,
                offsets: vec![offset_of!(Skeleton, source) as u32],
            }),
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(ipp_core::components::schema::FieldValue::String(
                        "asset://3/1".into(),
                    )),
                    interpolation: AnimationInterpolation::Step,
                },
                AnimationKeyframe {
                    time: 0.5,
                    value: AnimationValue::Field(ipp_core::components::schema::FieldValue::String(
                        "asset://3/2".into(),
                    )),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    let q = player(&mut world, a, &source, 101, Default::default());
    world
        .enqueue_playback(p, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .enqueue_playback(q, AnimationPlaybackControl::Play)
        .unwrap();
    let report = world.update_for_test(0.5).unwrap();
    assert!(
        report
            .playback_events
            .iter()
            .any(|event| event.controller.id == p
                && event.kind == AnimationPlaybackEventKind::Invalidated)
    );
    assert_eq!(
        world.animation_controller(p).unwrap().state,
        AnimationPlaybackStatus::Stopped
    );
    assert_eq!(world.animation_controller(q).unwrap().time, 0.5);
    close(world.skeleton_pose(a).unwrap()[1].y, 3.0);
    close(world.skeleton_pose(a).unwrap()[1].qz, 0.0);
}

#[test]
fn single_sampling_pass_reports_failures_without_rollback_or_retry() {
    use ipp_core::components::schema::FieldValue as Sample;
    let mut fixture_host = ipp_core::HostRuntime::new();
    let (mut world, target, _) = fixture(&mut fixture_host);
    let scalar = |a, b| AnimationTrack {
        target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::SCALAR,
            offsets: vec![0],
        }),
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(Sample::F32(a)),
                interpolation: AnimationInterpolation::Linear,
            },
            AnimationKeyframe {
                time: 2.0,
                value: AnimationValue::Field(Sample::F32(b)),
                interpolation: AnimationInterpolation::Step,
            },
        ],
    };
    let mixed = AnimationClip::new(
        2.0,
        vec![bend().tracks()[0].interchange(), scalar(f32::MAX, f32::MAX)],
    )
    .unwrap();
    let poses = player(&mut world, target, &mixed, 100, Default::default());
    let source = AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::SKELETON,
                offsets: vec![offset_of!(Skeleton, source) as u32],
            }),
            keys: vec![AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(Sample::String("asset://3/2".into())),
                interpolation: AnimationInterpolation::Step,
            }],
        }],
    )
    .unwrap();
    let replacement = player(&mut world, target, &source, 101, Default::default());
    let additive = AnimationClip::new(2.0, vec![scalar(0.0, f32::MAX)]).unwrap();
    let addition = player(
        &mut world,
        target,
        &additive,
        102,
        PlaybackSettings {
            additive: true,
            ..Default::default()
        },
    );

    for entity in [poses, replacement, addition] {
        world
            .enqueue_playback(entity, AnimationPlaybackControl::Seek(2.0))
            .unwrap();
        world
            .enqueue_playback(entity, AnimationPlaybackControl::Play)
            .unwrap();
    }
    let report = world.update_for_test(0.0).unwrap();
    assert!(
        report
            .playback_events
            .iter()
            .any(|event| event.controller.id == poses
                && event.kind == AnimationPlaybackEventKind::Invalidated)
    );
    assert!(
        report
            .playback_events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed)
    );
    assert!(
        world
            .inspect(target)
            .unwrap()
            .effective
            .contains(&ComponentValue::Scalar(Scalar {
                value: f32::MAX
            }))
    );
    assert!(
        world
            .inspect(target)
            .unwrap()
            .base
            .contains(&ComponentValue::Scalar(Scalar {
                value: 7.0
            }))
    );
    world
        .enqueue_playback(addition, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert!(
        world
            .inspect(target)
            .unwrap()
            .effective
            .contains(&ComponentValue::Scalar(Scalar {
                value: 7.0
            }))
    );
}
