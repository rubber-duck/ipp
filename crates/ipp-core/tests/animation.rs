//! Deterministic sampler, playback, lifecycle and constraint invariants.

mod support;
use support::WorldTestDriver;

use ipp_core::{
    components::Scalar,
    services::asset_management::{AssetUpload, AssetUploadIdentity},
    systems::animation::*,
    *,
};
use std::mem::offset_of;

#[test]
fn baked_numeric_clip_does_not_retain_interchange_enum_capacity() {
    use ipp_core::services::asset_management::Asset;

    let clip = AnimationClip::new(
        10.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::SCALAR,
                offsets: vec![offset_of!(Scalar, value) as u32],
            }),
            keys: (0..=240)
                .map(|frame| {
                    key(
                        f64::from(frame) / 24.0,
                        frame as f32,
                        if frame == 240 {
                            AnimationInterpolation::Step
                        } else {
                            AnimationInterpolation::Linear
                        },
                    )
                })
                .collect(),
        }],
    )
    .unwrap();
    let encoded = clip.encode();
    let decoded = AnimationClip::decode(&encoded).unwrap();
    // Typed interpolation stores more metadata than the wire. Four times the
    // wire size allows that layout, but rejects retained generic enum capacity.
    assert!(decoded.resident_bytes() < encoded.len() * 4);
    assert_eq!(decoded.encode(), encoded);
    assert_eq!(decoded.typed_track::<f32>(0).unwrap().sample(3.25), 78.0);
    assert_eq!(decoded.typed_track::<f32>(0).unwrap().sample(10.0), 240.0);
}

fn key(time: f64, value: f32, interpolation: AnimationInterpolation) -> AnimationKeyframe {
    AnimationKeyframe {
        time,
        value: AnimationValue::Field(components::schema::FieldValue::F32(value)),
        interpolation,
    }
}

fn curve(interpolation: AnimationInterpolation) -> AnimationClip {
    AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::SCALAR,
                offsets: vec![offset_of!(Scalar, value) as u32],
            }),
            keys: vec![
                key(0.0, 0.0, interpolation),
                key(2.0, 10.0, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap()
}

fn submit(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> BatchOutcome {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap().outcomes.remove(0)
}

fn create(world: &mut ipp_core::WorldContext<'_>, value: f32) -> EntityId {
    submit(
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Scalar(Scalar {
                    value,
                }),
            },
        ],
    )
    .result
    .unwrap()[0]
        .1
}

fn description(
    target: EntityId,
    clip: &AnimationClip,
    asset: u64,
) -> AnimationControllerDescription {
    AnimationControllerDescription {
        drivers: clip
            .tracks()
            .iter()
            .enumerate()
            .map(|(index, track)| AnimationDriverDescription {
                source: format!("asset://10/{asset}"),
                variant: 0,
                track: index as u32,
                target,
                property: track.target().clone(),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            })
            .collect(),
        ..Default::default()
    }
}

fn upload(world: &mut WorldContext<'_>, clip: &AnimationClip, asset: u64) {
    world
        .enqueue_asset(AssetUpload {
            id: asset,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
}

fn player(
    world: &mut WorldContext<'_>,
    target: EntityId,
    clip: &AnimationClip,
    asset: u64,
) -> AnimationControllerId {
    upload(world, clip, asset);
    world
        .create_animation_controller(description(target, clip, asset))
        .unwrap()
}

#[allow(irrefutable_let_patterns)]
fn scalar(world: &ipp_core::WorldContext<'_>, entity: EntityId) -> (f32, f32) {
    let snapshot = world.inspect(entity).unwrap();
    fn value(values: &[ComponentValue]) -> f32 {
        values
            .iter()
            .find_map(|v| {
                if let ComponentValue::Scalar(v) = v {
                    Some(v.value)
                } else {
                    None
                }
            })
            .unwrap()
    }
    (value(&snapshot.base), value(&snapshot.effective))
}

fn seek(world: &mut WorldContext<'_>, id: AnimationControllerId, time: f64) -> WorldUpdateReport {
    world
        .enqueue_playback(id, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .enqueue_playback(id, AnimationPlaybackControl::Seek(time))
        .unwrap();
    world
        .enqueue_playback(id, AnimationPlaybackControl::Pause)
        .unwrap();
    world.update_for_test(0.0).unwrap()
}

#[test]
fn player_clock_pauses_seeks_resumes_completes_and_stops_without_changing_base() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let target = create(&mut world, 99.0);
    let player = player(
        &mut world,
        target,
        &curve(AnimationInterpolation::Linear),
        1,
    );
    world
        .enqueue_playback(player, AnimationPlaybackControl::Play)
        .unwrap();
    assert_eq!(
        world.update_for_test(0.5).unwrap().playback_events[0].kind,
        AnimationPlaybackEventKind::Started
    );
    assert_eq!(scalar(&world, target), (99.0, 2.5));
    world
        .enqueue_playback(player, AnimationPlaybackControl::Pause)
        .unwrap();
    assert_eq!(
        world.update_for_test(5.0).unwrap().playback_events[0].kind,
        AnimationPlaybackEventKind::Paused
    );
    assert_eq!(scalar(&world, target), (99.0, 2.5));
    world
        .enqueue_playback(player, AnimationPlaybackControl::Seek(1.25))
        .unwrap();
    assert!(
        world
            .update_for_test(10.0)
            .unwrap()
            .playback_events
            .is_empty()
    );
    assert_eq!(world.animation_controller(player).unwrap().time, 1.25);
    assert_eq!(scalar(&world, target), (99.0, 6.25));
    world
        .enqueue_playback(player, AnimationPlaybackControl::Play)
        .unwrap();
    let report = world.update_for_test(1.0).unwrap();
    assert_eq!(
        report
            .playback_events
            .iter()
            .map(|e| e.kind)
            .collect::<Vec<_>>(),
        vec![
            AnimationPlaybackEventKind::Started,
            AnimationPlaybackEventKind::Completed
        ]
    );
    assert_eq!(scalar(&world, target), (99.0, 10.0));
    assert!(
        world
            .update_for_test(1.0)
            .unwrap()
            .playback_events
            .is_empty()
    );
    world
        .enqueue_playback(player, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(1.0).unwrap();
    assert_eq!(scalar(&world, target), (99.0, 99.0));
}

#[test]
fn bezier_solves_time_and_uses_value_handles_even_with_equal_endpoints() {
    let clip = AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: 1,
                offsets: vec![0],
            }),
            keys: vec![
                key(
                    0.0,
                    0.0,
                    AnimationInterpolation::Bezier {
                        time1: 0.0,
                        time2: 0.25,
                        value1: AnimationValue::Field(components::schema::FieldValue::F32(8.0)),
                        value2: AnimationValue::Field(components::schema::FieldValue::F32(8.0)),
                    },
                ),
                key(1.0, 0.0, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap();
    // u=.5 has x=.21875, y=6. Naively treating time as u gives a different result.
    assert_eq!(
        clip.sample(0, 0.21875),
        AnimationValue::Field(components::schema::FieldValue::F32(6.0))
    );
    assert_eq!(AnimationClip::decode(&clip.encode()).unwrap(), clip);
    let mut bytes = clip.encode();
    bytes.push(0);
    assert!(AnimationClip::decode(&bytes).is_err());
}

#[test]
fn replacement_in_failed_batch_invalidates_playback() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let target = create(&mut world, 90.0);
    let player = player(
        &mut world,
        target,
        &curve(AnimationInterpolation::Linear),
        1,
    );
    world
        .enqueue_playback(player, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.5).unwrap();
    let replacement = Command::InsertComponentValue {
        entity: EntityRef::Handle(target),
        value: ComponentValue::Scalar(Scalar {
            value: 7.0,
        }),
    };
    let rejected = submit(
        &mut world,
        vec![
            replacement.clone(),
            Command::RemoveComponent {
                entity: EntityRef::Handle(target),
                component: 65535,
            },
        ],
    );
    assert!(rejected.result.is_err());
    assert_eq!(scalar(&world, target), (7.0, 7.0));
    assert_eq!(
        world.animation_controller(player).unwrap().state,
        AnimationPlaybackStatus::Stopped
    );
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![replacement],
        })
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert!(report.playback_events.is_empty());
    assert_eq!(scalar(&world, target), (7.0, 7.0));
    world
        .enqueue_playback(player, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (7.0, 2.5));
}

#[test]
fn integer_interpolation_preserves_adjacent_u64_endpoints() {
    let start = u64::MAX - 1;
    let mut track = curve(AnimationInterpolation::Linear).tracks()[0].interchange();
    track.keys[0].value = AnimationValue::Field(components::schema::FieldValue::U64(start));
    track.keys[1].value = AnimationValue::Field(components::schema::FieldValue::U64(u64::MAX));
    let clip = AnimationClip::new(2.0, vec![track]).unwrap();
    assert_eq!(
        clip.sample(0, 0.5),
        AnimationValue::Field(components::schema::FieldValue::U64(start))
    );
    assert_eq!(
        clip.sample(0, 1.0),
        AnimationValue::Field(components::schema::FieldValue::U64(u64::MAX))
    );
}

#[test]
fn drivers_consume_animation_after_sampling() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let source = create(&mut world, 0.0);
    let target = create(&mut world, 20.0);
    submit(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(target),
            value: ComponentValue::LinearDriver(components::LinearDriver {
                source,
                scale: 2.0,
                bias: 1.0,
            }),
        }],
    )
    .result
    .unwrap();
    let player = player(
        &mut world,
        source,
        &curve(AnimationInterpolation::Linear),
        1,
    );
    world
        .enqueue_playback(player, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.5).unwrap();
    assert_eq!(scalar(&world, target), (20.0, 6.0));
}

#[test]
fn overlapping_animation_and_constraint_outputs_restore_in_reverse_order() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let source = create(&mut world, 3.0);
    let target = create(&mut world, 20.0);
    submit(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(target),
            value: ComponentValue::LinearDriver(components::LinearDriver {
                source,
                scale: 2.0,
                bias: 1.0,
            }),
        }],
    )
    .result
    .unwrap();
    let controller = player(
        &mut world,
        target,
        &curve(AnimationInterpolation::Linear),
        1,
    );
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    for _ in 0..3 {
        world.update_for_test(0.5).unwrap();
        assert_eq!(scalar(&world, target), (20.0, 7.0));
    }
    assert!(
        world
            .capture_world(Default::default())
            .unwrap()
            .entities
            .iter()
            .flat_map(|entity| &entity.components)
            .any(|value| *value
                == ComponentValue::Scalar(Scalar {
                    value: 20.0
                }))
    );
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    submit(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(target),
            component: ComponentValue::LINEAR_DRIVER,
        }],
    )
    .result
    .unwrap();
    assert_eq!(scalar(&world, target), (20.0, 20.0));
}

#[test]
fn invalid_clips_reject_counts_curves_types_and_owned_size() {
    let clip = curve(AnimationInterpolation::Linear);
    let mut track = clip.tracks()[0].interchange();
    track.keys[1].time = 0.0;
    assert!(AnimationClip::new(2.0, vec![track]).is_err());
    let mut track = clip.tracks()[0].interchange();
    track.keys[0].interpolation = AnimationInterpolation::Bezier {
        time1: 1.5,
        time2: 1.0,
        value1: AnimationValue::Field(components::schema::FieldValue::F32(1.0)),
        value2: AnimationValue::Field(components::schema::FieldValue::F32(2.0)),
    };
    assert!(AnimationClip::new(2.0, vec![track]).is_err());
    let mut track = clip.tracks()[0].interchange();
    track.keys[0].value = AnimationValue::Field(components::schema::FieldValue::Bool(true));
    track.keys[1].value = AnimationValue::Field(components::schema::FieldValue::Bool(false));
    assert!(AnimationClip::new(2.0, vec![track.clone()]).is_err());
    track.keys[0].interpolation = AnimationInterpolation::Step;
    track.keys[0].value = AnimationValue::Field(components::schema::FieldValue::Bytes(vec![
        0;
        (1 << 20)
            + 1
    ]));
    track.keys.truncate(1);
    assert_eq!(
        AnimationClip::new(2.0, vec![track.clone()])
            .unwrap()
            .tracks()[0]
            .interchange(),
        track
    );
    let bytes = clip.encode();
    for end in 0..bytes.len() {
        assert!(AnimationClip::decode(&bytes[..end]).is_err());
    }
}

#[test]
fn discrete_asset_sources_and_continuous_fields_keep_partial_updates() {
    use components::{MeshInstance, Transform};
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    world.register_stream_resource_provider("https").unwrap();
    let target = create(&mut world, 30.0);
    submit(
        &mut world,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Handle(target),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(target),
                value: ComponentValue::MeshInstance(MeshInstance {
                    source: "https://example.test/base.mesh".into(),
                    variant: 0,
                }),
            },
        ],
    )
    .result
    .unwrap();
    let clip = AnimationClip::new(
        2.0,
        vec![
            AnimationTrack {
                target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: ComponentValue::MESH_INSTANCE,
                    offsets: vec![offset_of!(MeshInstance, source) as u32],
                }),
                keys: vec![AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(components::schema::FieldValue::String(
                        "https://example.test/animated.mesh".into(),
                    )),
                    interpolation: AnimationInterpolation::Step,
                }],
            },
            AnimationTrack {
                target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: ComponentValue::TRANSFORM,
                    offsets: vec![offset_of!(Transform, sx) as u32],
                }),
                keys: vec![
                    key(0.0, 1.0, AnimationInterpolation::Linear),
                    key(2.0, -1.0, AnimationInterpolation::Step),
                ],
            },
            curve(AnimationInterpolation::Linear).tracks()[0].interchange(),
        ],
    )
    .unwrap();
    let p = player(&mut world, target, &clip, 1);
    seek(&mut world, p, 0.5);
    assert_eq!(scalar(&world, target), (30.0, 2.5));
    assert!(
        world
            .resource_snapshots()
            .iter()
            .any(|r| r.source == "https://example.test/animated.mesh")
    );
    let failed = seek(&mut world, p, 2.0);
    assert_eq!(
        failed
            .playback_events
            .iter()
            .filter(|e| e.kind == AnimationPlaybackEventKind::Failed)
            .count(),
        1
    );
    assert_eq!(scalar(&world, target), (30.0, 10.0));
    let snapshot = world.inspect(target).unwrap();
    assert!(snapshot.effective.iter().any(|c| matches!(c, ComponentValue::MeshInstance(m) if m.source == "https://example.test/animated.mesh")));
    assert!(
        world
            .resource_snapshots()
            .iter()
            .any(|r| r.source == "https://example.test/animated.mesh")
    );
    assert!(
        world
            .update_for_test(0.5)
            .unwrap()
            .playback_events
            .is_empty()
    );
    seek(&mut world, p, 0.5);
    assert_eq!(scalar(&world, target).1, 2.5);
}

#[test]
fn quaternion_tracks_normalize_endpoints_and_coordinate_writes_as_one_rotation() {
    use components::Transform;
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let target = create(&mut world, 0.0);
    submit(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(target),
            value: ComponentValue::Transform(Transform::default()),
        }],
    )
    .result
    .unwrap();
    let coordinates = [
        offset_of!(Transform, qx),
        offset_of!(Transform, qy),
        offset_of!(Transform, qz),
        offset_of!(Transform, qw),
    ];
    let track = AnimationTrack {
        target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::TRANSFORM,
            offsets: coordinates.map(|v| v as u32).to_vec(),
        }),
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Rotation([0.0, 0.0, 0.0, 2.0]),
                interpolation: AnimationInterpolation::Linear,
            },
            AnimationKeyframe {
                time: 2.0,
                value: AnimationValue::Rotation([0.0, 0.0, 2.0, 0.0]),
                interpolation: AnimationInterpolation::Step,
            },
        ],
    };
    let clip = AnimationClip::new(2.0, vec![track.clone()]).unwrap();
    assert_eq!(
        clip.sample(0, 0.0),
        AnimationValue::Rotation([0.0, 0.0, 0.0, 1.0])
    );
    let p = player(&mut world, target, &clip, 1);
    seek(&mut world, p, 1.0);
    let snapshot = world.inspect(target).unwrap();
    let rotation = snapshot
        .effective
        .iter()
        .find_map(|c| {
            if let ComponentValue::Transform(t) = c {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert!((rotation.qz - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((rotation.qw - rotation.qz).abs() < 1e-6);
    world
        .enqueue_playback(p, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    // Author W first, transiently producing zero; validation must see the whole sample.
    let single = |offset, value| AnimationTrack {
        target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::TRANSFORM,
            offsets: vec![offset as u32],
        }),
        keys: vec![key(0.0, value, AnimationInterpolation::Step)],
    };
    let q = player(
        &mut world,
        target,
        &AnimationClip::new(
            2.0,
            vec![single(coordinates[3], 0.0), single(coordinates[2], 3.0)],
        )
        .unwrap(),
        2,
    );
    seek(&mut world, q, 0.0);
    assert!(
        world
            .inspect(target)
            .unwrap()
            .effective
            .iter()
            .any(|c| matches!(c, ComponentValue::Transform(t) if t.qz == 1.0 && t.qw == 0.0))
    );
    assert!(AnimationClip::new(2.0, vec![track, single(coordinates[2], 1.0)]).is_ok());
}

#[test]
fn discrete_driver_binding_and_continuous_source_share_the_frozen_seek_time() {
    use components::LinearDriver;
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let source = create(&mut world, 2.0);
    let other = create(&mut world, 8.0);
    let target = create(&mut world, 0.0);
    submit(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(target),
            value: ComponentValue::LinearDriver(LinearDriver {
                source,
                scale: 1.0,
                bias: 0.0,
            }),
        }],
    )
    .result
    .unwrap();
    let clip = AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::LINEAR_DRIVER,
                offsets: vec![offset_of!(LinearDriver, source) as u32],
            }),
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(components::schema::FieldValue::Entity(source)),
                    interpolation: AnimationInterpolation::Step,
                },
                AnimationKeyframe {
                    time: 1.0,
                    value: AnimationValue::Field(components::schema::FieldValue::Entity(other)),
                    interpolation: AnimationInterpolation::Step,
                },
                AnimationKeyframe {
                    time: 2.0,
                    value: AnimationValue::Field(components::schema::FieldValue::Entity(target)),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    let p = player(&mut world, target, &clip, 1);
    let continuous = player(&mut world, other, &curve(AnimationInterpolation::Linear), 2);
    seek(&mut world, continuous, 1.0);
    seek(&mut world, p, 1.0);
    assert_eq!(scalar(&world, target), (0.0, 5.0));
    #[cfg(debug_assertions)]
    {
        let invalid = seek(&mut world, p, 2.0);
        assert!(invalid.playback_events.iter().any(|event| {
            event.controller.id == p
                && event.kind == AnimationPlaybackEventKind::Failed
                && event.reason == Some(ErrorReason::UnsupportedDependency)
        }));
    }
    world
        .enqueue_playback(p, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (0.0, 2.0));
}

#[test]
fn one_controller_samples_multiple_entities_at_one_exact_time() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let a = create(&mut world, 40.0);
    let b = create(&mut world, 80.0);
    let clip = curve(AnimationInterpolation::Linear);
    upload(&mut world, &clip, 1);
    let mut parameters = description(a, &clip, 1);
    parameters.drivers.extend(description(b, &clip, 1).drivers);
    let controller = world.create_animation_controller(parameters).unwrap();
    seek(&mut world, controller, 1.25);
    assert_eq!(scalar(&world, a), (40.0, 6.25));
    assert_eq!(scalar(&world, b), (80.0, 6.25));
    world.update_for_test(100.0).unwrap();
    assert_eq!(world.animation_controller(controller).unwrap().time, 1.25);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, a), (40.0, 40.0));
    assert_eq!(scalar(&world, b), (80.0, 80.0));
}

#[test]
fn queued_controllers_follow_entity_mutations_and_return_correlated_failures() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 50.0);
    let clip = curve(AnimationInterpolation::Linear);
    upload(&mut world, &clip, 1);
    world
        .enqueue_animation_controller(
            11,
            AnimationControllerCommand::Create(description(target, &clip, 1)),
        )
        .unwrap();
    world
        .enqueue(Batch {
            id: 12,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(target),
            }],
        })
        .unwrap();
    world
        .enqueue_animation_controller(
            13,
            AnimationControllerCommand::Create(description(target, &clip, 1)),
        )
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.animation_controller_outcomes.len(), 2);
    let controller = report.animation_controller_outcomes[0]
        .result
        .unwrap()
        .unwrap();
    assert_eq!(report.animation_controller_outcomes[0].request_id, 11);
    assert_eq!(report.animation_controller_outcomes[1].request_id, 13);
    assert_eq!(
        report.animation_controller_outcomes[1].result,
        Err(ErrorReason::InvalidEntity)
    );
    assert!(world.animation_controller(controller).is_some());
}

#[test]
fn active_and_batched_new_bindings_inherit_originals_and_paused_survivors_reapply() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 91.0);
    let clip = curve(AnimationInterpolation::Linear);
    let first = player(&mut world, target, &clip, 1);
    seek(&mut world, first, 1.0);
    let second = world
        .create_animation_controller(description(target, &clip, 1))
        .unwrap();
    let third = world
        .create_animation_controller(description(target, &clip, 1))
        .unwrap();
    for (id, time) in [(second, 1.5), (third, 2.0)] {
        world
            .enqueue_playback(id, AnimationPlaybackControl::Play)
            .unwrap();
        world
            .enqueue_playback(id, AnimationPlaybackControl::Seek(time))
            .unwrap();
        world
            .enqueue_playback(id, AnimationPlaybackControl::Pause)
            .unwrap();
    }
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (91.0, 10.0));
    world
        .enqueue_animation_controller(
            1,
            AnimationControllerCommand::Delete {
                id: third,
            },
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (91.0, 7.5));
    world
        .enqueue_animation_controller(
            2,
            AnimationControllerCommand::Delete {
                id: second,
            },
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (91.0, 5.0));
    world
        .enqueue_playback(first, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (91.0, 91.0));
}

#[test]
fn producer_write_then_stop_in_the_same_boundary_preserves_latest_underlying_value() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 91.0);
    let controller = player(
        &mut world,
        target,
        &curve(AnimationInterpolation::Linear),
        1,
    );
    seek(&mut world, controller, 1.0);
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(target),
                component: ComponentValue::SCALAR,
                field: FieldWrite {
                    offset: offset_of!(Scalar, value) as u32,
                    value: FieldValue::F32(123.0),
                },
            }],
        })
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (123.0, 123.0));
}

#[test]
fn typed_tracks_retain_concrete_storage_and_indices_across_decode() {
    let numeric = AnimationTrack::<f32> {
        target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: 1,
            offsets: vec![0],
        }),
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: 3.0,
                interpolation: AnimationInterpolation::Linear,
            },
            AnimationKeyframe {
                time: 2.0,
                value: 7.0,
                interpolation: AnimationInterpolation::Step,
            },
        ],
    };
    let discrete = AnimationTrack::<String> {
        target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: 2,
            offsets: vec![0],
        }),
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: "first".into(),
                interpolation: AnimationInterpolation::Step,
            },
            AnimationKeyframe {
                time: 2.0,
                value: "last".into(),
                interpolation: AnimationInterpolation::Step,
            },
        ],
    };
    let tracks: Vec<Box<dyn AnimationTrackData>> = vec![Box::new(numeric), Box::new(discrete)];
    let clip = AnimationClip::new(2.0, tracks).unwrap();
    assert_eq!(clip.typed_track::<f32>(0).unwrap().sample(1.0), 5.0);
    assert!(clip.typed_track::<String>(0).is_none());
    assert!(clip.typed_track::<f32>(1).is_none());
    for _ in 0..3 {
        let reload = AnimationClip::decode(&clip.encode()).unwrap();
        assert_eq!(reload.typed_track::<f32>(0).unwrap().sample(1.0), 5.0);
        assert_eq!(reload.typed_track::<String>(1).unwrap().sample(2.0), "last");
    }
    assert!(
        std::mem::size_of::<AnimationKeyframe<f32>>() < std::mem::size_of::<AnimationKeyframe>()
    );
}

#[test]
fn controller_update_rejects_track_type_mismatch_without_changing_clock_or_binding() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 91.0);
    let clip = curve(AnimationInterpolation::Linear);
    let controller = player(&mut world, target, &clip, 1);
    seek(&mut world, controller, 1.0);
    let other = AnimationClip::new(
        2.0,
        vec![AnimationTrack::<u32> {
            target: clip.tracks()[0].target().clone(),
            keys: vec![AnimationKeyframe {
                time: 0.0,
                value: 7,
                interpolation: AnimationInterpolation::Step,
            }],
        }],
    )
    .unwrap();
    upload(&mut world, &other, 2);
    let before = world.animation_controller(controller).unwrap();
    assert_eq!(
        world.update_animation_controller(controller, description(target, &other, 2)),
        Err(ErrorReason::InvalidField)
    );
    assert_eq!(world.animation_controller(controller).unwrap(), before);
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (91.0, 5.0));
}

#[test]
fn persistent_controllers_preserve_deleted_id_high_water_and_first_saved_sample() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 91.0);
    let clip = curve(AnimationInterpolation::Linear);
    let controller = player(&mut world, target, &clip, 1);
    let deleted = world
        .create_animation_controller(description(target, &clip, 1))
        .unwrap();
    world.remove_animation_controller(deleted).unwrap();
    assert!(world.animation_controller(deleted).is_none());
    assert_eq!(world.animation_controllers().len(), 1);
    seek(&mut world, controller, 1.0);
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    let persistent = world.animation_persistent_state();
    world
        .restore_animation_controllers(persistent.clone())
        .unwrap();
    world.update_for_test(0.5).unwrap();
    assert_eq!(scalar(&world, target), (91.0, 5.0));
    assert_eq!(world.animation_controller(controller).unwrap().time, 1.0);
    world.update_for_test(0.25).unwrap();
    assert_eq!(world.animation_controller(controller).unwrap().time, 1.25);
    let fresh = world
        .create_animation_controller(description(target, &clip, 1))
        .unwrap();
    assert!(fresh.to_bits() > deleted.to_bits());
    let mut duplicate = persistent;
    duplicate.controllers.push(duplicate.controllers[0].clone());
    assert_eq!(
        world.restore_animation_controllers(duplicate),
        Err(ErrorReason::InvalidValue)
    );
    assert!(world.animation_controller(fresh).is_some());
}

#[test]
fn settings_updates_retain_clock_and_additive_driver_uses_its_reference() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 10.0);
    let clip = curve(AnimationInterpolation::Linear);
    let controller = player(&mut world, target, &clip, 1);
    let mut parameters = world.animation_controller(controller).unwrap().description;
    parameters.looping = true;
    parameters.drivers[0].additive = true;
    parameters.drivers[0].weight = 0.5;
    parameters.drivers[0].reference_time = 0.5;
    world
        .update_animation_controller(controller, parameters)
        .unwrap();
    seek(&mut world, controller, 1.0);
    assert_eq!(scalar(&world, target), (10.0, 11.25));
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(1.75).unwrap();
    assert_eq!(world.animation_controller(controller).unwrap().time, 0.75);
    assert_eq!(scalar(&world, target), (10.0, 10.625));
}
#[test]
fn pending_clip_waits_without_advancing_and_samples_the_saved_seek_when_ready() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 99.0);
    let clip = curve(AnimationInterpolation::Linear);
    let controller = world
        .create_animation_controller(description(target, &clip, 99))
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Seek(0.5))
        .unwrap();
    world.update_for_test(100.0).unwrap();
    assert_eq!(world.animation_controller(controller).unwrap().time, 0.5);
    assert_eq!(scalar(&world, target), (99.0, 99.0));
    upload(&mut world, &clip, 99);
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (99.0, 2.5));
    world.update_for_test(0.25).unwrap();
    assert_eq!(world.animation_controller(controller).unwrap().time, 0.75);
}

#[test]
fn controller_descriptions_grow_while_ingress_remains_bounded() {
    let mut host = HostRuntime::new();
    let id = host
        .create_world(WorldLimits {
            max_batch_bytes: 1024,
            ..Default::default()
        })
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 7.0);
    let clip = curve(AnimationInterpolation::Linear);
    let mut parameters = description(target, &clip, 1);
    parameters.drivers[0].source = format!("https://example.test/{}", "x".repeat(1500));
    assert_eq!(
        world.enqueue_animation_controller(
            1,
            AnimationControllerCommand::Create(parameters.clone())
        ),
        Err(ErrorReason::Capacity)
    );
    assert!(world.animation_controllers().is_empty());

    // Reuse one source and target so the fixture isolates retained descriptions
    // from asset demand and entity-count limits. Actual source strings exceed 1 MiB.
    parameters.drivers = vec![parameters.drivers[0].clone(); 128];
    for _ in 0..8 {
        world
            .create_animation_controller(parameters.clone())
            .unwrap();
    }
    assert_eq!(world.animation_controllers().len(), 8);

    {
        use services::world_serialization::{WorldLoadOptions, WorldPersistenceLimits};

        drop(world);
        let limits = WorldPersistenceLimits::default();
        let bytes = host.save_world(id, 1, limits).unwrap();
        let restored = host
            .load_world(
                &bytes,
                1,
                WorldLoadOptions {
                    symbolic_id: Some("restored-controllers".into()),
                    ..Default::default()
                },
                WorldLimits::default(),
                limits,
            )
            .unwrap();
        assert_eq!(
            host.world_mut(restored)
                .unwrap()
                .animation_controllers()
                .len(),
            8
        );
    }
}

#[test]
fn reservation_hints_have_no_estimated_byte_ceiling() {
    let hints = |controllers| WorldCapacityHints {
        entities: 1,
        systems: std::collections::BTreeMap::from([(
            "ipp.animation".into(),
            WorldSystemCapacityHints::new([("controllers", controllers)]),
        )]),
    };
    let mut host = HostRuntime::new();
    // The AnimationSystem uses a BTreeMap and currently needs no preallocation.
    // Its former estimate rejected these hints despite reserving no controller storage.
    let id = host
        .create_world_with_options(
            WorldLimits::default(),
            WorldCreateOptions {
                capacity_hints: hints(1 << 20),
                ..Default::default()
            },
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    assert_eq!(
        world.capacity_hints().systems["ipp.animation"].get("controllers"),
        1 << 20
    );
    world.set_capacity_hints(hints(2 << 20)).unwrap();
    assert_eq!(
        world.capacity_hints().systems["ipp.animation"].get("controllers"),
        2 << 20
    );
    let target = create(&mut world, 7.0);
    assert_eq!(scalar(&world, target), (7.0, 7.0));
}

#[test]
fn bound_animation_state_does_not_consume_the_activation_budget() {
    let mut host = HostRuntime::new();
    let id = host
        .create_world(WorldLimits {
            max_staging_bytes: 1,
            ..Default::default()
        })
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 7.0);
    let clip = curve(AnimationInterpolation::Linear);
    let controller = player(&mut world, target, &clip, 1);
    let report = seek(&mut world, controller, 1.0);
    assert!(
        report
            .playback_events
            .iter()
            .all(|event| event.reason.is_none())
    );
    assert_eq!(scalar(&world, target), (7.0, 5.0));

    submit(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(target),
            component: ComponentValue::SCALAR,
            field: FieldWrite {
                offset: offset_of!(Scalar, value) as u32,
                value: FieldValue::F32(9.0),
            },
        }],
    )
    .result
    .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (9.0, 9.0));
}

#[test]
fn repeated_source_hints_allow_distinct_curves_for_distinct_targets() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let a = create(&mut world, 50.0);
    let b = create(&mut world, 70.0);
    let first = curve(AnimationInterpolation::Linear).tracks()[0].interchange();
    let mut second = first.clone();
    second.keys[1].value = AnimationValue::Field(components::schema::FieldValue::F32(20.0));
    let clip = AnimationClip::new(2.0, vec![first, second]).unwrap();
    upload(&mut world, &clip, 1);
    let mut parameters = description(a, &clip, 1);
    parameters.drivers[1].target = b;
    let controller = world.create_animation_controller(parameters).unwrap();
    seek(&mut world, controller, 1.0);
    assert_eq!(scalar(&world, a), (50.0, 5.0));
    assert_eq!(scalar(&world, b), (70.0, 10.0));
}

#[test]
fn failed_controller_keeps_successful_samples_without_restoring_prior_contributions() {
    use components::Transform;
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 91.0);
    submit(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(target),
            value: ComponentValue::Transform(Transform::default()),
        }],
    )
    .result
    .unwrap();
    let first = player(
        &mut world,
        target,
        &curve(AnimationInterpolation::Linear),
        1,
    );
    seek(&mut world, first, 1.0);
    let invalid = AnimationClip::new(
        2.0,
        vec![
            curve(AnimationInterpolation::Linear).tracks()[0].interchange(),
            AnimationTrack {
                target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: ComponentValue::TRANSFORM,
                    offsets: vec![offset_of!(Transform, sx) as u32],
                }),
                keys: vec![key(0.0, -1.0, AnimationInterpolation::Step)],
            },
        ],
    )
    .unwrap();
    let second = player(&mut world, target, &invalid, 2);
    let report = seek(&mut world, second, 2.0);
    assert_eq!(
        report
            .playback_events
            .iter()
            .filter(|event| event.kind == AnimationPlaybackEventKind::Failed)
            .count(),
        1
    );
    assert_eq!(scalar(&world, target), (91.0, 10.0));
    assert!(
        world
            .inspect(target)
            .unwrap()
            .effective
            .contains(&ComponentValue::Transform(Transform::default()))
    );
    assert!(
        world
            .update_for_test(0.0)
            .unwrap()
            .playback_events
            .is_empty()
    );
}
#[test]
fn indexed_clip_binding_survives_payload_unload_and_checked_reload() {
    use support::HostWorldTestDriver;
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 91.0);
    let clip = curve(AnimationInterpolation::Linear);
    let controller = player(&mut world, target, &clip, 1);
    seek(&mut world, controller, 1.0);
    let key = world
        .resolve_asset_key(AssetUploadIdentity {
            kind: ANIMATION_TYPE,
            asset: 1,
            variant: 0,
        })
        .unwrap();
    let old_track = std::sync::Arc::downgrade(
        &world
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .unwrap()
            .tracks()[0],
    );
    world.asset_resources_mut().unload(key);
    assert!(
        world
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_some()
    );
    drop(world);
    host.flush_resource_lifecycle();
    let mut world = host.world_mut(id).unwrap();
    assert!(world.asset_resources().get(key).is_some());
    assert!(
        world
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_none()
    );
    assert!(
        old_track.upgrade().is_none(),
        "suspended drivers must release old track storage before unload completes"
    );
    world.step(0.25).unwrap();
    assert_eq!(world.animation_controller(controller).unwrap().time, 1.0);
    drop(world);
    for _ in 0..16 {
        host.update_world_for_test(id, 0.0).unwrap();
        if host
            .world_mut(id)
            .unwrap()
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_some()
        {
            break;
        }
    }
    let world = host.world_mut(id).unwrap();
    let loaded = world
        .asset_resources()
        .get_typed::<AnimationClip>(key)
        .unwrap();
    assert_eq!(loaded.typed_track::<f32>(0).unwrap().sample(1.0), 5.0);
    assert_eq!(scalar(&world, target), (91.0, 5.0));
}

#[test]
fn overlays_remain_authored_inputs_through_animation_and_withdrawal() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let target = submit(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata {
                    symbolic_id: Some("layered".into()),
                    classes: vec![],
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Scalar(Scalar {
                    value: 20.0,
                }),
            },
        ],
    )
    .result
    .unwrap()[0]
        .1;
    let outcome = submit(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 1,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(1),
                alias: 2,
                symbolic_id: "layered".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                binding: StateOverlayRef::Alias(2),
                alias: 3,
                component: ComponentValue::SCALAR,
                mode: ComponentOverlayMode::Bound,
                fields: vec![FieldWrite {
                    offset: offset_of!(Scalar, value) as u32,
                    value: FieldValue::F32(40.0),
                }],
            },
        ],
    );
    outcome.result.unwrap();
    let owner = outcome
        .state_overlays
        .iter()
        .find(|r| r.alias == 1)
        .unwrap()
        .id;
    let p = player(
        &mut world,
        target,
        &curve(AnimationInterpolation::Linear),
        1,
    );
    let mut parameters = world.animation_controller(p).unwrap().description;
    parameters.drivers[0].weight = 0.5;
    world.update_animation_controller(p, parameters).unwrap();
    seek(&mut world, p, 2.0);
    assert_eq!(scalar(&world, target), (20.0, 25.0));
    world
        .enqueue_playback(p, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (20.0, 40.0));
    seek(&mut world, p, 2.0);
    submit(
        &mut world,
        vec![Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Handle(owner),
        }],
    )
    .result
    .unwrap();
    assert_eq!(scalar(&world, target), (20.0, 15.0));
    assert_eq!(
        world.animation_controller(p).unwrap().state,
        AnimationPlaybackStatus::Paused
    );
}

#[test]
fn external_clip_source_hints_cannot_bypass_sampled_producer_namespace_isolation() {
    use components::{MeshInstance, Transform};
    let mut host = HostRuntime::new();
    let external = services::data_source::MemoryDataSource::default();
    host.data_sources_mut()
        .register("https://example.test/", external.clone())
        .unwrap();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 30.0);
    submit(
        &mut world,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Handle(target),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(target),
                value: ComponentValue::MeshInstance(MeshInstance {
                    source: "https://example.test/base.mesh".into(),
                    variant: 0,
                }),
            },
        ],
    )
    .result
    .unwrap();
    let scalar_track = curve(AnimationInterpolation::Linear).tracks()[0].interchange();
    let clip = AnimationClip::new(
        2.0,
        vec![
            AnimationTrack {
                target: scalar_track.target.clone(),
                keys: vec![AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(components::schema::FieldValue::String(
                        "producer://999999/1/123".into(),
                    )),
                    interpolation: AnimationInterpolation::Step,
                }],
            },
            scalar_track,
        ],
    )
    .unwrap();
    let clip_source = "https://example.test/external.ippa";
    external.insert(clip_source.into(), clip.encode()).unwrap();
    let mut parameters = description(target, &clip, 1);
    for driver in &mut parameters.drivers {
        driver.source = clip_source.into();
    }
    parameters.drivers[0].property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::MESH_INSTANCE,
        offsets: vec![offset_of!(MeshInstance, source) as u32],
    });
    let controller = world.create_animation_controller(parameters).unwrap();
    let mut events = seek(&mut world, controller, 1.0).playback_events;
    for _ in 0..16 {
        events.extend(world.update_for_test(0.0).unwrap().playback_events);
        if events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed)
        {
            break;
        }
    }
    assert!(
        events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed
                && event.reason == Some(ErrorReason::InvalidAsset))
    );
    assert_eq!(scalar(&world, target), (30.0, 30.0));
    assert!(world.inspect(target).unwrap().effective.iter().any(|value| matches!(value, ComponentValue::MeshInstance(mesh) if mesh.source == "https://example.test/base.mesh")));
    assert!(
        !world
            .resource_snapshots()
            .iter()
            .any(|resource| resource.source.starts_with("producer://999999/"))
    );
}

#[test]
fn restored_producer_clip_preserves_its_original_nested_source_namespace() {
    use components::{MeshInstance, Transform};
    let mut host = HostRuntime::new();
    let first_id = host.create_world(WorldLimits::default()).unwrap();
    let nested_source = format!("producer://{}/1/123", first_id.0);
    let clip = AnimationClip::new(
        2.0,
        vec![
            AnimationTrack {
                target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: ComponentValue::MESH_INSTANCE,
                    offsets: vec![offset_of!(MeshInstance, source) as u32],
                }),
                keys: vec![AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(components::schema::FieldValue::String(
                        nested_source.clone(),
                    )),
                    interpolation: AnimationInterpolation::Step,
                }],
            },
            curve(AnimationInterpolation::Linear).tracks()[0].interchange(),
        ],
    )
    .unwrap();
    {
        let mut first = host.world_mut(first_id).unwrap();
        upload(&mut first, &clip, 1);
    }
    let second_id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(second_id).unwrap();
    let target = create(&mut world, 30.0);
    submit(
        &mut world,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Handle(target),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(target),
                value: ComponentValue::MeshInstance(MeshInstance {
                    source: "https://example.test/base.mesh".into(),
                    variant: 0,
                }),
            },
        ],
    )
    .result
    .unwrap();
    let mut parameters = description(target, &clip, 1);
    for driver in &mut parameters.drivers {
        driver.source = format!("producer://{}/10/1", first_id.0);
    }
    let controller = AnimationControllerId::from_bits(1);
    world
        .restore_animation_controllers(AnimationPersistentState {
            next_id: 2,
            controllers: vec![AnimationControllerSnapshot {
                id: controller,
                description: parameters,
                state: AnimationPlaybackStatus::Playing,
                time: 1.0,
                transition: None,
            }],
            transitions: Vec::new(),
            directional_starts: Vec::new(),
        })
        .unwrap();
    let report = world.update_for_test(5.0).unwrap();
    assert!(
        !report
            .playback_events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed)
    );
    assert_eq!(world.animation_controller(controller).unwrap().time, 1.0);
    assert_eq!(scalar(&world, target), (30.0, 5.0));
    assert!(world.inspect(target).unwrap().effective.iter().any(
        |value| matches!(value, ComponentValue::MeshInstance(mesh) if mesh.source == nested_source)
    ));
}

#[test]
fn animation_tracks_and_keys_grow_beyond_former_quotas() {
    let mut track = curve(AnimationInterpolation::Linear).tracks()[0].interchange();
    track.keys = (0..33)
        .map(|index| key(index as f64, index as f32, AnimationInterpolation::Linear))
        .collect();
    track.keys.last_mut().unwrap().interpolation = AnimationInterpolation::Step;
    let clip = AnimationClip::new(32.0, vec![track; 300]).unwrap();
    let decoded = AnimationClip::decode(&clip.encode()).unwrap();
    assert_eq!(decoded.tracks().len(), 300);
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 99.0);
    upload(&mut world, &clip, 1);
    let mut driver = description(target, &clip, 1);
    driver.drivers = vec![driver.drivers.pop().unwrap()];
    let controller = world.create_animation_controller(driver).unwrap();
    seek(&mut world, controller, 16.0);
    assert_eq!(scalar(&world, target), (99.0, 16.0));
}

#[test]
fn repeating_driver_uses_shared_clock_and_persists_its_sampling_policy() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let short_target = create(&mut world, 99.0);
    let long_target = create(&mut world, 88.0);
    let short = curve(AnimationInterpolation::Linear);
    let long = AnimationClip::new(
        6.0,
        vec![AnimationTrack {
            target: short.tracks()[0].target().clone(),
            keys: vec![
                key(0.0, 0.0, AnimationInterpolation::Linear),
                key(6.0, 12.0, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap();
    upload(&mut world, &short, 991);
    upload(&mut world, &long, 992);
    let mut request = description(short_target, &short, 991);
    request.drivers[0].repeat = true;
    request
        .drivers
        .extend(description(long_target, &long, 992).drivers);
    let controller = world.create_animation_controller(request).unwrap();
    seek(&mut world, controller, 5.0);
    assert_eq!(scalar(&world, short_target), (99.0, 5.0));
    assert_eq!(scalar(&world, long_target), (88.0, 10.0));
    world.update_for_test(10.0).unwrap();
    assert_eq!(scalar(&world, short_target).1, 5.0);
    let saved = world.animation_persistent_state();
    let decoded =
        AnimationPersistentState::decode(&saved.encode(1 << 20).unwrap(), 1 << 20).unwrap();
    assert!(decoded.controllers[0].description.drivers[0].repeat);
    assert!(!decoded.controllers[0].description.drivers[1].repeat);
    assert_eq!(decoded.controllers[0].time, 5.0);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, short_target).1, 99.0);
    assert_eq!(scalar(&world, long_target).1, 88.0);
}

#[test]
fn compiled_numeric_frames_notify_once_per_batch_without_commit_hooks() {
    use ipp_core::systems::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Trace {
        commits: usize,
        batches: Vec<Vec<f32>>,
    }
    struct Observer(Arc<Mutex<Trace>>);

    struct Factory(Arc<Mutex<Trace>>);

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            SystemId("test.compiled-numeric")
        }

        fn dependencies(&self) -> &[SystemDependency] {
            &[]
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Observer(self.0.clone())))
        }
    }

    impl System for Observer {
        fn validate_commit(&self, _: &SystemCommitContext<'_>) -> Result<(), ErrorReason> {
            self.0.lock().unwrap().commits += 1;
            Ok(())
        }

        fn before_numeric_update(&mut self, context: &mut SystemNumericContext<'_>) {
            let values = context
                .changed_components()
                .iter()
                .map(|&(entity, component)| {
                    let ComponentValue::Scalar(value) = context
                        .world()
                        .effective_component(entity, component)
                        .unwrap()
                    else {
                        panic!("scalar batch");
                    };
                    value.value
                })
                .collect();
            self.0.lock().unwrap().batches.push(values);
        }

        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}
    }

    let trace = Arc::new(Mutex::new(Trace::default()));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(trace.clone())));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 99.0);
    let clip = curve(AnimationInterpolation::Linear);
    let controller = player(&mut world, target, &clip, 801);
    seek(&mut world, controller, 1.0);
    *trace.lock().unwrap() = Trace::default();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (99.0, 5.0));
    let observed = trace.lock().unwrap();
    assert_eq!(observed.commits, 0);
    assert_eq!(observed.batches, [vec![5.0]]);
    drop(observed);
    // Same-incarnation producer edits preserve the location and refresh the sparse
    // original. Explicit replacement drops the driver before the slot is reused.
    assert!(
        submit(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(target),
                component: ComponentValue::SCALAR,
                field: FieldWrite {
                    offset: offset_of!(Scalar, value) as u32,
                    value: FieldValue::F32(42.0)
                },
            }]
        )
        .result
        .is_ok()
    );
    assert_eq!(scalar(&world, target), (42.0, 5.0));
    *trace.lock().unwrap() = Trace::default();
    world.update_for_test(0.0).unwrap();
    assert_eq!(trace.lock().unwrap().batches, [vec![5.0]]);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (42.0, 42.0));
    seek(&mut world, controller, 1.0);
    assert!(
        submit(
            &mut world,
            vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(target),
                value: ComponentValue::Scalar(Scalar {
                    value: 123.0
                }),
            }]
        )
        .result
        .is_ok()
    );
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (123.0, 123.0));
    let mut weighted = description(target, &clip, 801);
    weighted.drivers[0].weight = 0.5;
    world
        .update_animation_controller(controller, weighted)
        .unwrap();
    seek(&mut world, controller, 1.0);
    *trace.lock().unwrap() = Trace::default();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (123.0, 64.0));
    assert_eq!(trace.lock().unwrap().commits, 0);
}

#[test]
fn retained_numeric_output_withdraws_on_advance_failure_and_controller_restoration() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, 99.0);
    let clip = curve(AnimationInterpolation::Linear);
    let controller = player(&mut world, target, &clip, 890);
    let mut request = description(target, &clip, 890);
    request.speed = f32::MAX;
    request.looping = true;
    world
        .update_animation_controller(controller, request)
        .unwrap();
    seek(&mut world, controller, 1.0);
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (99.0, 5.0));
    let report = world.update_for_test(1e300).unwrap();
    assert!(report.playback_events.iter().any(|event| {
        event.controller.id == controller && event.kind == AnimationPlaybackEventKind::Failed
    }));
    assert_eq!(scalar(&world, target), (99.0, 99.0));
    seek(&mut world, controller, 1.0);
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (99.0, 5.0));
    let mut replacement = world.animation_persistent_state();
    replacement.controllers.clear();
    world.restore_animation_controllers(replacement).unwrap();
    assert_eq!(scalar(&world, target), (99.0, 99.0));
}

#[test]
#[cfg(all(feature = "particles", feature = "mesh-poses"))]
fn resource_owning_numeric_lanes_preserve_state_without_commit_hooks() {
    use ipp_core::{components::*, systems::*};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Observer(Arc<AtomicUsize>);

    struct Factory(Arc<AtomicUsize>);

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            SystemId("test.numeric-resource-lanes")
        }

        fn dependencies(&self) -> &[SystemDependency] {
            &[]
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Observer(self.0.clone())))
        }
    }

    impl System for Observer {
        fn validate_commit(&self, _: &SystemCommitContext<'_>) -> Result<(), ErrorReason> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}
    }

    let commits = Arc::new(AtomicUsize::new(0));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(commits.clone())));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let source = create(&mut world, 2.0);
    let target = create(&mut world, 0.0);
    let components = [
        (
            ComponentValue::LinearDriver(LinearDriver {
                source,
                ..Default::default()
            }),
            offset_of!(LinearDriver, scale),
        ),
        (
            ComponentValue::BoundingGeometry(BoundingGeometry::default()),
            offset_of!(BoundingGeometry, stroke),
        ),
        (
            ComponentValue::PickingGeometry(PickingGeometry::default()),
            offset_of!(PickingGeometry, r),
        ),
        (
            ComponentValue::MeshPose(MeshPose::default()),
            offset_of!(MeshPose, weight),
        ),
        (
            ComponentValue::ParticleEmitter(ParticleEmitter {
                burst: 1,
                rate: 0.0,
                ..Default::default()
            }),
            offset_of!(ParticleEmitter, size),
        ),
        (
            ComponentValue::ParticleSprite(ParticleSprite::default()),
            offset_of!(ParticleSprite, opacity),
        ),
    ];
    let original: Vec<_> = components
        .iter()
        .map(|(value, offset)| {
            let components::schema::FieldValue::F32(original) =
                value.field(*offset as u32).unwrap()
            else {
                unreachable!()
            };
            (value.type_id(), *offset as u32, original)
        })
        .collect();
    assert!(
        submit(
            &mut world,
            components
                .into_iter()
                .map(|(value, _)| Command::InsertComponentValue {
                    entity: EntityRef::Handle(target),
                    value,
                })
                .collect()
        )
        .result
        .is_ok()
    );
    let clip = AnimationClip::new(
        2.0,
        original
            .iter()
            .map(|&(component, offset, _)| AnimationTrack {
                target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component,
                    offsets: vec![offset],
                }),
                keys: vec![
                    key(0.0, 0.2, AnimationInterpolation::Linear),
                    key(2.0, 0.8, AnimationInterpolation::Step),
                ],
            })
            .collect(),
    )
    .unwrap();
    let controller = player(&mut world, target, &clip, 802);
    seek(&mut world, controller, 1.0);

    // Unit-weight curves prove every lane's range when bound. Weighted curves
    // use retained patches and guard the combined numeric result before writes.
    for weight in [1.0, 0.25] {
        let mut description = description(target, &clip, 802);
        for driver in &mut description.drivers {
            driver.weight = weight;
        }
        world
            .update_animation_controller(controller, description)
            .unwrap();
        seek(&mut world, controller, 1.0);
        let particle = &world.particles(target).unwrap()[0];
        let (particle_id, age, address) = (particle.id, particle.age, particle as *const _);
        commits.store(0, Ordering::Relaxed);
        world.update_for_test(0.1).unwrap();
        assert_eq!(commits.load(Ordering::Relaxed), 0);
        let snapshot = world.inspect(target).unwrap();
        for &(component, offset, base) in &original {
            let value = snapshot
                .effective
                .iter()
                .find(|v| v.type_id() == component)
                .unwrap();
            let components::schema::FieldValue::F32(actual) = value.field(offset).unwrap() else {
                unreachable!()
            };
            assert!(
                (actual - (base * (1.0 - weight) + 0.5 * weight)).abs() < 1e-6,
                "component {component}: {actual}"
            );
        }
        assert!((scalar(&world, target).1 - 2.0 * (1.0 - 0.5 * weight)).abs() < 1e-6);
        assert_eq!(world.particles(target).unwrap().len(), 1);
        let particle = &world.particles(target).unwrap()[0];
        assert_eq!(particle.id, particle_id);
        assert!((particle.age - age - 0.1).abs() < 1e-9);
        assert_eq!(particle as *const _, address);
    }
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let snapshot = world.inspect(target).unwrap();
    for &(component, offset, base) in &original {
        let value = snapshot
            .effective
            .iter()
            .find(|v| v.type_id() == component)
            .unwrap();
        assert_eq!(
            value.field(offset),
            Ok(components::schema::FieldValue::F32(base))
        );
    }

    // An out-of-range arithmetic result rejects the complete component patch;
    // it must neither clone geometry storage nor publish a valid sibling lane.
    let invalid_clip = AnimationClip::new(
        2.0,
        [
            (offset_of!(BoundingGeometry, r), 0.3),
            (offset_of!(BoundingGeometry, stroke), -0.1),
        ]
        .into_iter()
        .map(|(offset, value)| AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::BOUNDING_GEOMETRY,
                offsets: vec![offset as u32],
            }),
            keys: vec![
                key(0.0, value, AnimationInterpolation::Linear),
                key(2.0, value, AnimationInterpolation::Step),
            ],
        })
        .collect(),
    )
    .unwrap();
    let invalid = player(&mut world, target, &invalid_clip, 803);
    let report = seek(&mut world, invalid, 1.0);
    assert!(
        report
            .playback_events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed
                && event.reason == Some(ErrorReason::InvalidValue))
    );
    let snapshot = world.inspect(target).unwrap();
    let geometry = snapshot
        .effective
        .iter()
        .find(|v| v.type_id() == ComponentValue::BOUNDING_GEOMETRY)
        .unwrap();
    assert_eq!(
        geometry.field(offset_of!(BoundingGeometry, r) as u32),
        Ok(components::schema::FieldValue::F32(1.0))
    );
    assert_eq!(
        geometry.field(offset_of!(BoundingGeometry, stroke) as u32),
        Ok(components::schema::FieldValue::F32(0.04))
    );
}

#[test]
fn mixed_resource_and_numeric_drivers_commit_only_resource_transitions() {
    use ipp_core::{
        components::CustomMaterial, services::asset_management::AssetSource, systems::*,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Observer(Arc<AtomicUsize>);
    struct Factory(Arc<AtomicUsize>);

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            SystemId("test.mixed-animation")
        }

        fn dependencies(&self) -> &[SystemDependency] {
            &[]
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Observer(self.0.clone())))
        }
    }

    impl System for Observer {
        fn validate_commit(&self, _: &SystemCommitContext<'_>) -> Result<(), ErrorReason> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}
    }

    let commits = Arc::new(AtomicUsize::new(0));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(commits.clone())));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let world_id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create(&mut world, 9.0);
    let asset = |id| {
        DynamicValue::Asset(AssetSource {
            kind: MESH_TYPE,
            uri: format!("asset://1/{id}"),
            variant: 0,
        })
    };
    let mut material = CustomMaterial::default();
    for index in 0..if cfg!(miri) {
        8
    } else {
        128
    } {
        material
            .properties
            .set(&format!("padding_{index}"), DynamicValue::F32(index as f32))
            .unwrap();
    }
    material
        .properties
        .set("amount", DynamicValue::F32(0.25))
        .unwrap();
    material.properties.set("image", asset(1)).unwrap();
    submit(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::CustomMaterial(material),
        }],
    )
    .result
    .unwrap();
    let dynamic_track = |name: &str, a, b| AnimationTrack {
        target: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            name: name.into(),
        },
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(components::schema::FieldValue::Dynamic(a)),
                interpolation: AnimationInterpolation::Step,
            },
            AnimationKeyframe {
                time: 1.0,
                value: AnimationValue::Field(components::schema::FieldValue::Dynamic(b)),
                interpolation: AnimationInterpolation::Step,
            },
        ],
    };
    let clip = AnimationClip::new(
        2.0,
        vec![
            dynamic_track("amount", DynamicValue::F32(0.5), DynamicValue::F32(0.75)),
            dynamic_track("image", asset(2), asset(3)),
            AnimationTrack {
                target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: ComponentValue::SCALAR,
                    offsets: vec![offset_of!(Scalar, value) as u32],
                }),
                keys: vec![
                    key(0.0, 2.0, AnimationInterpolation::Linear),
                    key(2.0, 4.0, AnimationInterpolation::Step),
                ],
            },
        ],
    )
    .unwrap();
    let controller = player(&mut world, entity, &clip, 950);
    seek(&mut world, controller, 0.5);
    let inspect = |world: &WorldContext<'_>| {
        let snapshot = world.inspect(entity).unwrap();
        snapshot
            .effective
            .into_iter()
            .find_map(|v| match v {
                ComponentValue::CustomMaterial(m) => Some(m.properties),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(inspect(&world).get("image"), Some(asset(2)));
    assert_eq!(inspect(&world).get("amount"), Some(DynamicValue::F32(0.5)));
    commits.store(0, Ordering::Relaxed);
    for _ in 0..8 {
        world.update_for_test(0.01).unwrap();
    }
    assert_eq!(
        commits.load(Ordering::Relaxed),
        0,
        "held resource must not restore/recommit every frame"
    );
    assert_eq!(scalar(&world, entity), (9.0, 2.5));
    seek(&mut world, controller, 1.5);
    assert_eq!(inspect(&world).get("image"), Some(asset(3)));
    assert_eq!(inspect(&world).get("amount"), Some(DynamicValue::F32(0.75)));
    // A producer edit refreshes originals, but the paused contribution survives it.
    submit(
        &mut world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "amount".into(),
            value: DynamicValue::F32(0.1),
        }],
    )
    .result
    .unwrap();
    assert_eq!(inspect(&world).get("amount"), Some(DynamicValue::F32(0.75)));
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(inspect(&world).get("image"), Some(asset(1)));
    assert_eq!(inspect(&world).get("amount"), Some(DynamicValue::F32(0.1)));
    assert_eq!(scalar(&world, entity), (9.0, 9.0));
}
