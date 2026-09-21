//! Public-API scalar transition composition, clock, and lifecycle invariants.

mod support;
use support::WorldTestDriver;

use ipp_core::{
    components::Scalar,
    services::asset_management::{AssetUpload, AssetUploadIdentity},
    systems::{animation::*, geometry::BoundingGeometry},
    *,
};
use std::mem::offset_of;

const VALUE_OFFSET: u32 = offset_of!(Scalar, value) as u32;

fn key(time: f64, value: f32) -> AnimationKeyframe {
    AnimationKeyframe {
        time,
        value: AnimationValue::Field(components::schema::FieldValue::F32(value)),
        interpolation: AnimationInterpolation::Linear,
    }
}

fn clip(duration: f64, start: f32, end: f32) -> AnimationClip {
    AnimationClip::new(
        duration,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::SCALAR,
                offsets: vec![VALUE_OFFSET],
            }),
            keys: vec![
                key(0.0, start),
                AnimationKeyframe {
                    interpolation: AnimationInterpolation::Step,
                    ..key(duration, end)
                },
            ],
        }],
    )
    .unwrap()
}

fn submit(world: &mut WorldContext<'_>, operations: Vec<Command>) -> BatchOutcome {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap().outcomes.remove(0)
}

fn create(world: &mut WorldContext<'_>, value: f32) -> EntityId {
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

fn set_base(world: &mut WorldContext<'_>, target: EntityId, value: f32) {
    submit(
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(target),
            component: ComponentValue::SCALAR,
            field: FieldWrite {
                offset: VALUE_OFFSET,
                value: FieldValue::F32(value),
            },
        }],
    )
    .result
    .unwrap();
}

fn upload(world: &mut WorldContext<'_>, asset: u64, clip: &AnimationClip) {
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

fn driver(target: EntityId, asset: u64) -> AnimationDriverDescription {
    AnimationDriverDescription {
        source: format!("asset://10/{asset}"),
        variant: 0,
        track: 0,
        target,
        property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::SCALAR,
            offsets: vec![VALUE_OFFSET],
        }),
        weight: 1.0,
        additive: false,
        reference_time: 0.0,
        repeat: false,
    }
}

fn description(drivers: Vec<AnimationDriverDescription>) -> AnimationControllerDescription {
    AnimationControllerDescription {
        drivers,
        ..Default::default()
    }
}

fn transition(
    description: AnimationControllerDescription,
    duration: f64,
    start_time: AnimationTransitionStartTime,
) -> AnimationControllerTransition {
    AnimationControllerTransition {
        description,
        duration,
        easing: AnimationTransitionEasing::Linear,
        start_time,
    }
}

fn host_world() -> (HostRuntime, WorldId) {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    (host, world)
}

fn scalar(world: &WorldContext<'_>, entity: EntityId) -> (f32, f32) {
    let snapshot = world.inspect(entity).unwrap();
    let value = |values: &[ComponentValue]| {
        values
            .iter()
            .find_map(|value| match value {
                ComponentValue::Scalar(value) => Some(value.value),
                _ => None,
            })
            .unwrap()
    };
    (value(&snapshot.base), value(&snapshot.effective))
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 1.0e-5,
        "expected {expected}, got {actual}"
    );
}

fn play_at(world: &mut WorldContext<'_>, controller: AnimationControllerId, time: f64) {
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Seek(time))
        .unwrap();
    world.update_for_test(0.0).unwrap();
}

#[test]
fn first_sample_is_exact_and_source_destination_and_fade_clocks_advance_independently() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = create(&mut world, 100.0);
    let source = clip(2.0, 0.0, 10.0);
    let destination = clip(2.0, 20.0, 40.0);
    upload(&mut world, 1, &source);
    upload(&mut world, 2, &destination);
    let mut source_description = description(vec![driver(target, 1)]);
    source_description.speed = -0.5;
    let controller = world
        .create_animation_controller(source_description)
        .unwrap();
    play_at(&mut world, controller, 1.0);
    close(scalar(&world, target).1, 5.0);

    let mut destination_description = description(vec![driver(target, 2)]);
    destination_description.speed = 2.0;
    world
        .transition_animation_controller(
            controller,
            transition(
                destination_description,
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    close(scalar(&world, target).1, 5.0);
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.time, 0.0);
    assert_eq!(snapshot.transition.unwrap().elapsed, 0.0);

    world.update_for_test(0.5).unwrap();
    close(scalar(&world, target).1, 16.875);
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.time, 1.0);
    assert_eq!(snapshot.transition.unwrap().elapsed, 0.5);

    world.update_for_test(0.5).unwrap();
    close(scalar(&world, target).1, 40.0);
    assert!(
        world
            .animation_controller(controller)
            .unwrap()
            .transition
            .is_none()
    );
}

#[test]
fn paused_transition_freezes_and_zero_speed_destination_holds_while_fade_progresses() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = create(&mut world, 50.0);
    let source = clip(2.0, 0.0, 10.0);
    let destination = clip(2.0, 20.0, 40.0);
    upload(&mut world, 1, &source);
    upload(&mut world, 2, &destination);
    let controller = world
        .create_animation_controller(description(vec![driver(target, 1)]))
        .unwrap();
    play_at(&mut world, controller, 1.0);
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Pause)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let mut destination_description = description(vec![driver(target, 2)]);
    destination_description.speed = 0.0;
    world
        .transition_animation_controller(
            controller,
            transition(
                destination_description,
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(3.0).unwrap();
    close(scalar(&world, target).1, 5.0);
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.state, AnimationPlaybackStatus::Paused);
    assert_eq!(snapshot.time, 0.0);
    assert_eq!(snapshot.transition.unwrap().elapsed, 0.0);

    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.5).unwrap();
    close(scalar(&world, target).1, 13.75);
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.time, 0.0);
    assert_eq!(snapshot.transition.unwrap().elapsed, 0.5);
}

#[test]
fn destination_start_time_policies_select_restart_preserve_match_phase_and_seek() {
    let cases = [
        (AnimationTransitionStartTime::Restart, 0.0),
        (AnimationTransitionStartTime::Preserve, 1.0),
        (AnimationTransitionStartTime::MatchPhase, 2.0),
        (AnimationTransitionStartTime::Seek(0.75), 0.75),
    ];
    for (index, (policy, expected)) in cases.into_iter().enumerate() {
        let (mut host, world_id) = host_world();
        let mut world = host.world_mut(world_id).unwrap();
        let target = create(&mut world, 50.0);
        let source = clip(2.0, 0.0, 10.0);
        let destination = clip(4.0, 20.0, 60.0);
        upload(&mut world, 1 + index as u64 * 2, &source);
        upload(&mut world, 2 + index as u64 * 2, &destination);
        let controller = world
            .create_animation_controller(description(vec![driver(target, 1 + index as u64 * 2)]))
            .unwrap();
        play_at(&mut world, controller, 1.0);
        world
            .transition_animation_controller(
                controller,
                transition(
                    description(vec![driver(target, 2 + index as u64 * 2)]),
                    1.0,
                    policy,
                ),
            )
            .unwrap();
        world.update_for_test(0.0).unwrap();
        assert_eq!(
            world.animation_controller(controller).unwrap().time,
            expected
        );
        close(scalar(&world, target).1, 5.0);
    }
}

#[test]
fn partial_target_union_tracks_new_producer_values_and_stop_restores_every_target() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let source_only = create(&mut world, 100.0);
    let destination_only = create(&mut world, 50.0);
    let source = clip(2.0, 0.0, 10.0);
    let destination = clip(2.0, 20.0, 40.0);
    upload(&mut world, 1, &source);
    upload(&mut world, 2, &destination);
    let controller = world
        .create_animation_controller(description(vec![driver(source_only, 1)]))
        .unwrap();
    play_at(&mut world, controller, 1.0);
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(destination_only, 2)]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    set_base(&mut world, source_only, 200.0);
    world.update_for_test(0.5).unwrap();
    close(scalar(&world, source_only).1, 103.75);
    close(scalar(&world, destination_only).1, 37.5);

    world
        .control_animation_controller(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, source_only), (200.0, 200.0));
    assert_eq!(scalar(&world, destination_only), (50.0, 50.0));
}

#[test]
fn interruption_starts_from_the_current_composite_without_a_discontinuity() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = create(&mut world, 100.0);
    for (asset, animation) in [
        (1, clip(2.0, 0.0, 10.0)),
        (2, clip(2.0, 20.0, 40.0)),
        (3, clip(2.0, 100.0, 120.0)),
    ] {
        upload(&mut world, asset, &animation);
    }
    let controller = world
        .create_animation_controller(description(vec![driver(target, 1)]))
        .unwrap();
    play_at(&mut world, controller, 1.0);
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(target, 2)]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.5).unwrap();
    let before = scalar(&world, target).1;
    close(before, 16.25);

    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(target, 3)]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    close(scalar(&world, target).1, before);
    assert_eq!(
        world
            .animation_controller(controller)
            .unwrap()
            .transition
            .unwrap()
            .elapsed,
        0.0
    );
}

#[test]
fn frozen_interruption_tracks_new_underlying_values_for_stop_restoration() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let source_only = create(&mut world, 100.0);
    let covered = create(&mut world, 200.0);
    upload(&mut world, 1, &clip(2.0, 0.0, 10.0));
    upload(&mut world, 2, &clip(2.0, 20.0, 40.0));
    let controller = world
        .create_animation_controller(description(vec![
            driver(source_only, 1),
            driver(covered, 1),
        ]))
        .unwrap();
    play_at(&mut world, controller, 1.0);
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(covered, 2)]),
                2.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.5).unwrap();
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(covered, 99)]),
                2.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();

    set_base(&mut world, source_only, 301.0);
    set_base(&mut world, covered, 302.0);
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();

    assert_eq!(scalar(&world, source_only), (301.0, 301.0));
    assert_eq!(scalar(&world, covered), (302.0, 302.0));

    let destination_only = create(&mut world, 300.0);
    upload(&mut world, 3, &clip(2.0, 50.0, 70.0));
    let controller = world
        .create_animation_controller(description(vec![
            driver(source_only, 1),
            driver(covered, 1),
        ]))
        .unwrap();
    play_at(&mut world, controller, 1.0);
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(covered, 2)]),
                2.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.5).unwrap();
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(destination_only, 3)]),
                2.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();

    set_base(&mut world, source_only, 401.0);
    set_base(&mut world, covered, 402.0);
    set_base(&mut world, destination_only, 403.0);
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();

    assert_eq!(scalar(&world, source_only), (401.0, 401.0));
    assert_eq!(scalar(&world, covered), (402.0, 402.0));
    assert_eq!(scalar(&world, destination_only), (403.0, 403.0));
}

#[test]
fn interrupting_with_a_pending_destination_keeps_the_controller_and_held_composite() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = create(&mut world, 100.0);
    let source = clip(2.0, 0.0, 10.0);
    let destination = clip(2.0, 20.0, 40.0);
    upload(&mut world, 1, &source);
    upload(&mut world, 2, &destination);
    let controller = world
        .create_animation_controller(description(vec![driver(target, 1)]))
        .unwrap();
    play_at(&mut world, controller, 1.0);
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(target, 2)]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.5).unwrap();
    let held = scalar(&world, target).1;

    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(target, 99)]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    close(scalar(&world, target).1, held);
    assert!(
        world
            .animation_controller(controller)
            .unwrap()
            .transition
            .unwrap()
            .pending
    );

    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(target, 98)]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(10.0).unwrap();
    close(scalar(&world, target).1, held);
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.id, controller);
    assert!(snapshot.transition.unwrap().pending);

    world.remove_animation_controller(controller).unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, target), (100.0, 100.0));
    assert!(world.animation_controller(controller).is_none());
}

#[test]
fn live_source_holds_while_destination_asset_is_unavailable() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = create(&mut world, 42.0);
    upload(&mut world, 1, &clip(2.0, 0.0, 10.0));
    let controller = world
        .create_animation_controller(description(vec![driver(target, 1)]))
        .unwrap();
    let mut looping = world.animation_controller(controller).unwrap().description;
    looping.speed = -1.0;
    looping.looping = true;
    world
        .update_animation_controller(controller, looping)
        .unwrap();
    play_at(&mut world, controller, 0.001);
    world.update_for_test(0.01).unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Pause)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let held = scalar(&world, target).1;

    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(target, 503)]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();

    close(scalar(&world, target).1, held);
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.state, AnimationPlaybackStatus::Paused);
    assert!(snapshot.transition.unwrap().pending);

    world.update_for_test(0.25).unwrap();
    set_base(&mut world, target, 84.0);
    world.update_for_test(0.0).unwrap();
    close(scalar(&world, target).1, held);
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.state, AnimationPlaybackStatus::Paused);
    let transition = snapshot.transition.unwrap();
    assert!(transition.pending);
    close(transition.elapsed as f32, 0.0);
}

#[test]
fn repeated_interruptions_keep_frozen_metadata_bounded() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = create(&mut world, 100.0);
    upload(&mut world, 1, &clip(2.0, 0.0, 10.0));
    upload(&mut world, 2, &clip(2.0, 20.0, 40.0));
    let controller = world
        .create_animation_controller(description(vec![driver(target, 1)]))
        .unwrap();
    play_at(&mut world, controller, 1.0);

    for index in 0..16 {
        let asset = if index % 2 == 0 {
            2
        } else {
            1
        };
        world
            .transition_animation_controller(
                controller,
                transition(
                    description(vec![driver(target, asset)]),
                    1.0,
                    AnimationTransitionStartTime::Preserve,
                ),
            )
            .unwrap();
        world.update_for_test(0.0).unwrap();
        world.update_for_test(0.05).unwrap();

        let persisted = world.animation_persistent_state();
        let AnimationPersistentTransitionSource::Frozen {
            values,
            bindings,
            ..
        } = &persisted.transitions[0].source
        else {
            if index == 0 {
                continue;
            }
            panic!("interrupted transition must have a frozen origin");
        };
        assert!(bindings.description.drivers.is_empty());
        assert_eq!(values.len(), 1);
    }
}

#[test]
fn prepared_transition_persists_its_advanced_destination_clock() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = create(&mut world, 100.0);
    upload(&mut world, 1, &clip(2.0, 0.0, 10.0));
    upload(&mut world, 2, &clip(2.0, 20.0, 40.0));
    let controller = world
        .create_animation_controller(description(vec![driver(target, 1)]))
        .unwrap();
    play_at(&mut world, controller, 0.5);
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![driver(target, 2)]),
                1.0,
                AnimationTransitionStartTime::Seek(0.25),
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.2).unwrap();
    let time = world.animation_controller(controller).unwrap().time;
    assert!(time > 0.25);
    assert_eq!(
        world.animation_persistent_state().transitions[0].start_time,
        AnimationTransitionStartTime::Seek(time)
    );
}

#[test]
fn weighted_and_additive_sides_blend_their_fully_composed_values() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = create(&mut world, 10.0);
    let source = clip(2.0, 0.0, 10.0);
    let destination = clip(2.0, 20.0, 40.0);
    upload(&mut world, 1, &source);
    upload(&mut world, 2, &destination);
    let mut source_driver = driver(target, 1);
    source_driver.weight = 0.5;
    let controller = world
        .create_animation_controller(description(vec![source_driver]))
        .unwrap();
    play_at(&mut world, controller, 1.0);
    close(scalar(&world, target).1, 7.5);

    let mut destination_driver = driver(target, 2);
    destination_driver.weight = 0.5;
    destination_driver.additive = true;
    destination_driver.reference_time = 0.0;
    world
        .transition_animation_controller(
            controller,
            transition(
                description(vec![destination_driver]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.5).unwrap();
    close(scalar(&world, target).1, 10.625);
}

#[test]
fn rejected_source_hold_keeps_the_existing_controller_installed() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let target = submit(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::BoundingGeometry(BoundingGeometry::default()),
            },
        ],
    )
    .result
    .unwrap()[0]
        .1;
    let destination_target = create(&mut world, 10.0);
    let bool_offset = offset_of!(BoundingGeometry, is_rendered) as u32;
    let bool_property = AnimationProperty {
        component: ComponentValue::BOUNDING_GEOMETRY,
        offsets: vec![bool_offset],
    };
    let discrete = AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(bool_property.clone()),
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(components::schema::FieldValue::Bool(false)),
                    interpolation: AnimationInterpolation::Step,
                },
                AnimationKeyframe {
                    time: 2.0,
                    value: AnimationValue::Field(components::schema::FieldValue::Bool(true)),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    upload(&mut world, 1, &discrete);
    upload(&mut world, 2, &clip(2.0, 20.0, 40.0));
    let source_driver = AnimationDriverDescription {
        property: AnimationTrackTarget::AnimationProperty(bool_property),
        ..driver(target, 1)
    };
    let controller = world
        .create_animation_controller(description(vec![source_driver]))
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    let before = world.animation_controller(controller).unwrap();

    assert_eq!(
        world.transition_animation_controller(
            controller,
            transition(
                description(vec![driver(destination_target, 2)]),
                1.0,
                AnimationTransitionStartTime::Restart,
            ),
        ),
        Err(ErrorReason::InvalidField)
    );

    let after = world.animation_controller(controller).unwrap();
    assert_eq!(after.description, before.description);
    assert_eq!(after.state, before.state);
    assert_eq!(after.time, before.time);
    assert!(after.transition.is_none());
}
