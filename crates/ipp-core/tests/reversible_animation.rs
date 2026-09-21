//! Deterministic signed playback through the public World and asset APIs.

mod support;

use std::mem::offset_of;

use support::WorldTestDriver;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime, WorldContext, WorldLimits,
    components::Scalar,
    components::schema::FieldValue,
    services::asset_management::{AssetUpload, AssetUploadIdentity},
    systems::animation::{
        ANIMATION_TYPE, AnimationClip, AnimationControllerDescription, AnimationControllerId,
        AnimationDriverDescription, AnimationInterpolation, AnimationKeyframe,
        AnimationPersistentState, AnimationPlaybackControl, AnimationPlaybackStatus,
        AnimationProperty, AnimationTrack, AnimationTrackTarget, AnimationValue,
    },
};

fn key(time: f64, value: f32) -> AnimationKeyframe {
    AnimationKeyframe {
        time,
        value: AnimationValue::Field(FieldValue::F32(value)),
        interpolation: AnimationInterpolation::Linear,
    }
}

fn fixture() -> (
    HostRuntime,
    ipp_core::WorldId,
    EntityId,
    AnimationControllerId,
) {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Scalar(Scalar {
                        value: 99.0,
                    }),
                },
            ],
        })
        .unwrap();
    let target = world.update_for_test(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    let property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::SCALAR,
        offsets: vec![offset_of!(Scalar, value) as u32],
    });
    let clip = AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: property.clone(),
            keys: vec![
                key(0.0, 0.0),
                AnimationKeyframe {
                    time: 2.0,
                    value: AnimationValue::Field(FieldValue::F32(10.0)),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    world
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
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
    let controller = world
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: "asset://10/1".into(),
                variant: 0,
                track: 0,
                target,
                property,
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            ..Default::default()
        })
        .unwrap();
    drop(world);
    (host, world_id, target, controller)
}

fn seek(world: &mut WorldContext<'_>, controller: AnimationControllerId, time: f64) {
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Pause)
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Seek(time))
        .unwrap();
    world.update_for_test(0.0).unwrap();
}

fn state(
    world: &WorldContext<'_>,
    controller: AnimationControllerId,
) -> (f64, AnimationPlaybackStatus) {
    let state = world.animation_controller(controller).unwrap();
    (state.time, state.state)
}

#[allow(irrefutable_let_patterns)]
fn effective_scalar(world: &WorldContext<'_>, target: EntityId) -> f32 {
    world
        .inspect(target)
        .unwrap()
        .effective
        .iter()
        .find_map(|value| {
            if let ComponentValue::Scalar(value) = value {
                Some(value.value)
            } else {
                None
            }
        })
        .unwrap()
}

#[test]
fn delayed_reverse_restart_uses_ready_duration_and_explicit_seek_is_preserved() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Scalar(Scalar {
                        value: 99.0,
                    }),
                },
            ],
        })
        .unwrap();
    let target = world.update_for_test(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    let property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::SCALAR,
        offsets: vec![offset_of!(Scalar, value) as u32],
    });
    let clip = AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: property.clone(),
            keys: vec![
                key(0.0, 0.0),
                AnimationKeyframe {
                    time: 2.0,
                    value: AnimationValue::Field(FieldValue::F32(10.0)),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    let delayed_description = |asset| AnimationControllerDescription {
        drivers: vec![AnimationDriverDescription {
            source: format!("asset://10/{asset}"),
            variant: 0,
            track: 0,
            target,
            property: property.clone(),
            weight: 1.0,
            additive: false,
            reference_time: 0.0,
            repeat: false,
        }],
        speed: -1.0,
        looping: false,
    };

    let restarted = world
        .create_animation_controller(delayed_description(91))
        .unwrap();
    world
        .enqueue_playback(restarted, AnimationPlaybackControl::Restart)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(state(&world, restarted).0, 0.0);
    world
        .enqueue_asset(AssetUpload {
            id: 91,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 91,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
    world.update_for_test(0.0).unwrap();
    assert_eq!(state(&world, restarted).0, 2.0);
    assert_eq!(effective_scalar(&world, target), 10.0);

    world
        .enqueue_playback(restarted, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let sought = world
        .create_animation_controller(delayed_description(92))
        .unwrap();
    world
        .enqueue_playback(sought, AnimationPlaybackControl::Seek(0.75))
        .unwrap();
    world
        .enqueue_playback(sought, AnimationPlaybackControl::PlayAtSpeed(-1.0))
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(state(&world, sought).0, 0.75);
    let saved = world.animation_persistent_state();
    let saved = AnimationPersistentState::decode(&saved.encode(1 << 20).unwrap(), 1 << 20).unwrap();
    assert!(saved.directional_starts.is_empty());
    world
        .enqueue_asset(AssetUpload {
            id: 92,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 92,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
    world.update_for_test(0.0).unwrap();
    assert_eq!(state(&world, sought).0, 0.75);
    assert_eq!(effective_scalar(&world, target), 3.75);
    world.update_for_test(0.25).unwrap();
    assert_eq!(state(&world, sought).0, 0.5);
    assert_eq!(effective_scalar(&world, target), 2.5);

    let stopped = world
        .create_animation_controller(delayed_description(93))
        .unwrap();
    world
        .enqueue_playback(stopped, AnimationPlaybackControl::Restart)
        .unwrap();
    world
        .enqueue_playback(stopped, AnimationPlaybackControl::Pause)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let paused = world.animation_persistent_state();
    assert_eq!(paused.directional_starts, vec![stopped]);
    assert!(AnimationPersistentState::decode(&paused.encode(1 << 20).unwrap(), 1 << 20).is_ok());
    world
        .enqueue_playback(stopped, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert!(
        !world
            .animation_persistent_state()
            .directional_starts
            .contains(&stopped)
    );
}

#[test]
fn signed_nonlooping_playback_holds_endpoints_and_resumes_inward() {
    let (mut host, world_id, target, controller) = fixture();
    let mut world = host.world_mut(world_id).unwrap();
    seek(&mut world, controller, 2.0);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::PlayAtSpeed(-1.0))
        .unwrap();
    world.update_for_test(0.5).unwrap();
    assert_eq!(
        state(&world, controller),
        (1.5, AnimationPlaybackStatus::Playing)
    );
    assert_eq!(effective_scalar(&world, target), 7.5);
    world.update_for_test(2.0).unwrap();
    assert_eq!(
        state(&world, controller),
        (0.0, AnimationPlaybackStatus::Completed)
    );
    assert_eq!(effective_scalar(&world, target), 0.0);

    world
        .enqueue_playback(controller, AnimationPlaybackControl::PlayAtSpeed(1.0))
        .unwrap();
    world.update_for_test(0.25).unwrap();
    assert_eq!(
        state(&world, controller),
        (0.25, AnimationPlaybackStatus::Playing)
    );
    assert_eq!(effective_scalar(&world, target), 1.25);

    seek(&mut world, controller, 2.0);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::PlayAtSpeed(-1.0))
        .unwrap();
    world.update_for_test(0.25).unwrap();
    assert_eq!(state(&world, controller).0, 1.75);
}

#[test]
fn signed_looping_wraps_in_both_directions() {
    let (mut host, world_id, target, controller) = fixture();
    let mut world = host.world_mut(world_id).unwrap();
    let mut description = world.animation_controller(controller).unwrap().description;
    description.looping = true;
    world
        .update_animation_controller(controller, description)
        .unwrap();

    seek(&mut world, controller, 0.25);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::PlayAtSpeed(-1.0))
        .unwrap();
    world.update_for_test(0.5).unwrap();
    assert_eq!(
        state(&world, controller),
        (1.75, AnimationPlaybackStatus::Playing)
    );
    assert_eq!(effective_scalar(&world, target), 8.75);

    world
        .enqueue_playback(controller, AnimationPlaybackControl::PlayAtSpeed(1.0))
        .unwrap();
    world.update_for_test(0.5).unwrap();
    assert_eq!(
        state(&world, controller),
        (0.25, AnimationPlaybackStatus::Playing)
    );
    assert_eq!(effective_scalar(&world, target), 1.25);
}

#[test]
fn restart_and_plain_play_choose_the_endpoint_for_signed_direction() {
    let (mut host, world_id, target, controller) = fixture();
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::PlayAtSpeed(-1.0))
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Restart)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(state(&world, controller).0, 2.0);
    assert_eq!(effective_scalar(&world, target), 10.0);
    world.update_for_test(0.5).unwrap();
    assert_eq!(state(&world, controller).0, 1.5);
    assert_eq!(effective_scalar(&world, target), 7.5);

    world.update_for_test(2.0).unwrap();
    assert_eq!(
        state(&world, controller),
        (0.0, AnimationPlaybackStatus::Completed)
    );
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(state(&world, controller).0, 2.0);
    assert_eq!(effective_scalar(&world, target), 10.0);
    world.update_for_test(0.5).unwrap();
    assert_eq!(state(&world, controller).0, 1.5);
    assert_eq!(effective_scalar(&world, target), 7.5);

    seek(&mut world, controller, 2.0);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::PlayAtSpeed(1.0))
        .unwrap();
    world.update_for_test(0.1).unwrap();
    assert_eq!(
        state(&world, controller),
        (2.0, AnimationPlaybackStatus::Completed)
    );
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(state(&world, controller).0, 0.0);
    assert_eq!(effective_scalar(&world, target), 0.0);
    world.update_for_test(0.5).unwrap();
    assert_eq!(state(&world, controller).0, 0.5);
    assert_eq!(effective_scalar(&world, target), 2.5);
}

#[test]
fn zero_speed_and_speed_only_update_preserve_the_current_contribution() {
    let (mut host, world_id, target, controller) = fixture();
    let mut world = host.world_mut(world_id).unwrap();
    seek(&mut world, controller, 1.0);
    let mut description = world.animation_controller(controller).unwrap().description;
    description.speed = -2.0;
    world
        .update_animation_controller(controller, description)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(
        state(&world, controller),
        (1.0, AnimationPlaybackStatus::Paused)
    );
    assert_eq!(effective_scalar(&world, target), 5.0);

    world
        .enqueue_playback(controller, AnimationPlaybackControl::PlayAtSpeed(0.0))
        .unwrap();
    world.update_for_test(10.0).unwrap();
    assert_eq!(
        state(&world, controller),
        (1.0, AnimationPlaybackStatus::Playing)
    );
    assert_eq!(effective_scalar(&world, target), 5.0);
}
