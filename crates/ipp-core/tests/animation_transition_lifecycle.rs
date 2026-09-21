//! Transition lifetime and restoration through public World and asset APIs.

mod support;

use std::mem::offset_of;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime, WorldContext, WorldLimits,
    components::Scalar,
    components::schema::FieldValue,
    services::asset_management::{AssetUpload, AssetUploadIdentity},
    systems::animation::{
        ANIMATION_TYPE, AnimationClip, AnimationControllerDescription, AnimationControllerId,
        AnimationControllerTransition, AnimationDriverDescription, AnimationInterpolation,
        AnimationKeyframe, AnimationPersistentState, AnimationPersistentTransitionSource,
        AnimationPlaybackControl, AnimationPlaybackStatus, AnimationProperty, AnimationTrack,
        AnimationTrackTarget, AnimationTransitionEasing, AnimationTransitionStartTime,
        AnimationValue,
    },
};
use support::WorldTestDriver;

#[cfg(feature = "surfaces")]
use ipp_core::{
    DynamicValue, Surface, SurfaceCommand, SurfaceItemContent, SurfaceItemId, SurfaceItemStyle,
};

fn create(world: &mut WorldContext<'_>, alias: u32, value: f32) -> EntityId {
    world
        .enqueue(Batch {
            id: u64::from(alias),
            operations: vec![
                Command::Create {
                    alias,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(alias),
                    value: ComponentValue::Scalar(Scalar {
                        value,
                    }),
                },
            ],
        })
        .unwrap();
    world.update_for_test(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1
}

fn clip(start: f32, end: f32) -> AnimationClip {
    AnimationClip::new(
        2.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::SCALAR,
                offsets: vec![offset_of!(Scalar, value) as u32],
            }),
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(FieldValue::F32(start)),
                    interpolation: AnimationInterpolation::Linear,
                },
                AnimationKeyframe {
                    time: 2.0,
                    value: AnimationValue::Field(FieldValue::F32(end)),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap()
}

fn upload(world: &mut WorldContext<'_>, id: u64, clip: &AnimationClip) {
    world
        .enqueue_asset(AssetUpload {
            id,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: id,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
}

fn description(asset: u64, targets: &[EntityId], speed: f32) -> AnimationControllerDescription {
    AnimationControllerDescription {
        drivers: targets
            .iter()
            .map(|&target| AnimationDriverDescription {
                source: format!("asset://10/{asset}"),
                variant: 0,
                track: 0,
                target,
                property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: ComponentValue::SCALAR,
                    offsets: vec![offset_of!(Scalar, value) as u32],
                }),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            })
            .collect(),
        speed,
        looping: false,
    }
}

fn transition(
    world: &mut WorldContext<'_>,
    controller: AnimationControllerId,
    description: AnimationControllerDescription,
    duration: f64,
) {
    world
        .transition_animation_controller(
            controller,
            AnimationControllerTransition {
                description,
                duration,
                easing: AnimationTransitionEasing::Linear,
                start_time: AnimationTransitionStartTime::Seek(1.0),
            },
        )
        .unwrap();
    // The first ready evaluation samples both sides at elapsed zero.
    world.update_for_test(0.0).unwrap();
    let state = world.animation_controller(controller).unwrap();
    assert_eq!(state.transition.unwrap().elapsed, 0.0);
}

fn seek_and_play(world: &mut WorldContext<'_>, controller: AnimationControllerId) {
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Seek(1.0))
        .unwrap();
    world.update_for_test(0.0).unwrap();
}

#[allow(irrefutable_let_patterns)]
fn scalar(world: &WorldContext<'_>, target: EntityId) -> (f32, f32) {
    let snapshot = world.inspect(target).unwrap();
    let read = |values: &[ComponentValue]| {
        values
            .iter()
            .find_map(|value| {
                if let ComponentValue::Scalar(value) = value {
                    Some(value.value)
                } else {
                    None
                }
            })
            .unwrap()
    };
    (read(&snapshot.base), read(&snapshot.effective))
}

#[cfg(feature = "surfaces")]
fn edit_surface(world: &mut WorldContext<'_>, request: u64, command: SurfaceCommand) {
    world
        .enqueue_surface_command_with_reply(1, request, command)
        .unwrap();
    assert_eq!(
        world.update_for_test(0.0).unwrap().system_command_outcomes[0].result,
        Ok(())
    );
}

#[cfg(feature = "surfaces")]
fn opacity_track(name: String) -> AnimationTrack {
    AnimationTrack {
        target: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::SURFACE,
            name,
        },
        keys: vec![AnimationKeyframe {
            time: 0.0,
            value: AnimationValue::Field(FieldValue::Dynamic(DynamicValue::F32(0.5))),
            interpolation: AnimationInterpolation::Step,
        }],
    }
}

#[test]
fn removing_and_reusing_a_source_only_component_does_not_rebind_the_transition() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let removed = create(&mut world, 1, 100.0);
    let survivor = create(&mut world, 2, 200.0);
    upload(&mut world, 1, &clip(0.0, 10.0));
    upload(&mut world, 2, &clip(20.0, 40.0));
    let controller = world
        .create_animation_controller(description(1, &[removed, survivor], 0.0))
        .unwrap();
    seek_and_play(&mut world, controller);
    transition(
        &mut world,
        controller,
        description(2, &[survivor], 0.0),
        1.0,
    );
    world.update_for_test(0.5).unwrap();

    world
        .enqueue(Batch {
            id: 20,
            operations: vec![
                Command::RemoveComponent {
                    entity: EntityRef::Handle(removed),
                    component: ComponentValue::SCALAR,
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Handle(removed),
                    value: ComponentValue::Scalar(Scalar {
                        value: 333.0,
                    }),
                },
            ],
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, removed), (333.0, 333.0));
    let stopped = world.animation_controller(controller).unwrap();
    assert_eq!(stopped.state, AnimationPlaybackStatus::Stopped);
    assert_eq!(stopped.transition, None);
    assert_eq!(scalar(&world, survivor), (200.0, 200.0));

    world.update_for_test(0.5).unwrap();
    assert_eq!(scalar(&world, removed), (333.0, 333.0));
    assert_eq!(scalar(&world, survivor), (200.0, 200.0));
}

#[test]
fn pending_frozen_source_only_replacement_invalidates_the_held_program() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let removed = create(&mut world, 1, 100.0);
    let survivor = create(&mut world, 2, 200.0);
    upload(&mut world, 1, &clip(0.0, 10.0));
    upload(&mut world, 2, &clip(20.0, 40.0));
    let controller = world
        .create_animation_controller(description(1, &[removed, survivor], 0.0))
        .unwrap();
    seek_and_play(&mut world, controller);
    transition(
        &mut world,
        controller,
        description(2, &[survivor], 0.0),
        2.0,
    );
    world.update_for_test(0.5).unwrap();
    transition(
        &mut world,
        controller,
        description(999, &[survivor], 0.0),
        2.0,
    );
    assert!(
        world
            .animation_controller(controller)
            .unwrap()
            .transition
            .unwrap()
            .pending
    );

    world
        .enqueue(Batch {
            id: 21,
            operations: vec![
                Command::RemoveComponent {
                    entity: EntityRef::Handle(removed),
                    component: ComponentValue::SCALAR,
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Handle(removed),
                    value: ComponentValue::Scalar(Scalar {
                        value: 333.0,
                    }),
                },
            ],
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();

    assert_eq!(scalar(&world, removed), (333.0, 333.0));
    assert_eq!(scalar(&world, survivor), (200.0, 200.0));
    let stopped = world.animation_controller(controller).unwrap();
    assert_eq!(stopped.state, AnimationPlaybackStatus::Stopped);
    assert_eq!(stopped.transition, None);
}

#[cfg(feature = "surfaces")]
#[test]
fn pending_frozen_dynamic_property_reuse_does_not_write_through_old_descriptor() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    world
        .enqueue(Batch {
            id: 30,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Surface(Surface::default()),
                },
            ],
        })
        .unwrap();
    let entity = world.update_for_test(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    for id in [1, 2] {
        edit_surface(
            &mut world,
            30 + u64::from(id),
            SurfaceCommand::Insert {
                entity,
                id: SurfaceItemId(id),
                index: id - 1,
                content: SurfaceItemContent::Drawing,
                style: SurfaceItemStyle::default(),
            },
        );
    }
    let names = [
        Surface::property_name(SurfaceItemId(1), "opacity").unwrap(),
        Surface::property_name(SurfaceItemId(2), "opacity").unwrap(),
    ];
    let source =
        AnimationClip::new(1.0, names.iter().cloned().map(opacity_track).collect()).unwrap();
    let destination = AnimationClip::new(1.0, vec![opacity_track(names[1].clone())]).unwrap();
    upload(&mut world, 101, &source);
    upload(&mut world, 102, &destination);
    let driver = |asset, track, name: String| AnimationDriverDescription {
        source: format!("asset://10/{asset}"),
        variant: 0,
        track,
        target: entity,
        property: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::SURFACE,
            name,
        },
        weight: 1.0,
        additive: false,
        reference_time: 0.0,
        repeat: false,
    };
    let controller = world
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![
                driver(101, 0, names[0].clone()),
                driver(101, 1, names[1].clone()),
            ],
            ..Default::default()
        })
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    transition(
        &mut world,
        controller,
        AnimationControllerDescription {
            drivers: vec![driver(102, 0, names[1].clone())],
            ..Default::default()
        },
        2.0,
    );
    world.update_for_test(0.5).unwrap();
    transition(
        &mut world,
        controller,
        AnimationControllerDescription {
            drivers: vec![driver(999, 0, names[1].clone())],
            ..Default::default()
        },
        2.0,
    );
    edit_surface(
        &mut world,
        40,
        SurfaceCommand::Remove {
            entity,
            id: SurfaceItemId(1),
        },
    );
    edit_surface(
        &mut world,
        41,
        SurfaceCommand::Insert {
            entity,
            id: SurfaceItemId(3),
            index: 0,
            content: SurfaceItemContent::Drawing,
            style: SurfaceItemStyle {
                opacity: 0.9,
                ..Default::default()
            },
        },
    );
    assert_eq!(
        world.animation_controller(controller).unwrap().state,
        AnimationPlaybackStatus::Stopped
    );
    assert_eq!(
        world
            .surface(entity)
            .unwrap()
            .properties
            .get(&Surface::property_name(SurfaceItemId(3), "opacity").unwrap()),
        Some(DynamicValue::F32(0.9))
    );
}

#[test]
fn interrupted_disjoint_union_restores_every_original_on_stop_and_delete() {
    for delete in [false, true] {
        let mut host = HostRuntime::new();
        let world_id = host.create_world(WorldLimits::default()).unwrap();
        let mut world = host.world_mut(world_id).unwrap();
        let a = create(&mut world, 1, 101.0);
        let b = create(&mut world, 2, 202.0);
        let c = create(&mut world, 3, 303.0);
        upload(&mut world, 1, &clip(0.0, 10.0));
        upload(&mut world, 2, &clip(20.0, 40.0));
        upload(&mut world, 3, &clip(50.0, 70.0));
        let controller = world
            .create_animation_controller(description(1, &[a, b], 0.0))
            .unwrap();
        seek_and_play(&mut world, controller);
        transition(&mut world, controller, description(2, &[b, c], 0.0), 2.0);
        world.update_for_test(0.5).unwrap();
        transition(&mut world, controller, description(3, &[c], 0.0), 2.0);
        world.update_for_test(0.5).unwrap();
        assert_ne!(scalar(&world, a).1, 101.0);
        assert_ne!(scalar(&world, b).1, 202.0);
        assert_ne!(scalar(&world, c).1, 303.0);

        if delete {
            world.remove_animation_controller(controller).unwrap();
        } else {
            world
                .control_animation_controller(controller, AnimationPlaybackControl::Stop)
                .unwrap();
        }
        world.update_for_test(0.0).unwrap();
        assert_eq!(scalar(&world, a), (101.0, 101.0));
        assert_eq!(scalar(&world, b), (202.0, 202.0));
        assert_eq!(scalar(&world, c), (303.0, 303.0));
    }
}

#[test]
fn outgoing_asset_unload_holds_transition_until_reload_then_target_removal_stops_it() {
    use support::HostWorldTestDriver;

    let mut host = HostRuntime::new();
    let world_id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let source_only = create(&mut world, 1, 100.0);
    let survivor = create(&mut world, 2, 200.0);
    upload(&mut world, 1, &clip(0.0, 10.0));
    upload(&mut world, 2, &clip(20.0, 40.0));
    let controller = world
        .create_animation_controller(description(1, &[source_only, survivor], 0.0))
        .unwrap();
    seek_and_play(&mut world, controller);
    transition(
        &mut world,
        controller,
        description(2, &[survivor], 0.0),
        2.0,
    );
    world.update_for_test(0.5).unwrap();
    let held_source = scalar(&world, source_only).1;
    let held_survivor = scalar(&world, survivor).1;
    let before = world.animation_controller(controller).unwrap();
    let key = world
        .resolve_asset_key(AssetUploadIdentity {
            kind: ANIMATION_TYPE,
            asset: 1,
            variant: 0,
        })
        .unwrap();
    world.asset_resources_mut().unload(key);
    drop(world);
    host.flush_resource_lifecycle();

    let mut world = host.world_mut(world_id).unwrap();
    world.step(0.5).unwrap();
    let pending = world.animation_controller(controller).unwrap();
    assert!(pending.transition.unwrap().pending);
    assert_eq!(
        pending.transition.unwrap().elapsed,
        before.transition.unwrap().elapsed
    );
    assert_eq!(pending.time, before.time);
    assert_eq!(scalar(&world, source_only).1, held_source);
    assert_eq!(scalar(&world, survivor).1, held_survivor);
    drop(world);

    for _ in 0..16 {
        host.update_world_for_test(world_id, 0.0).unwrap();
        let world = host.world_mut(world_id).unwrap();
        if world
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_some()
        {
            break;
        }
    }
    let mut world = host.world_mut(world_id).unwrap();
    assert!(
        world
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_some()
    );
    world.update_for_test(0.5).unwrap();
    let resumed = world.animation_controller(controller).unwrap();
    assert!(!resumed.transition.unwrap().pending);
    assert!(resumed.transition.unwrap().elapsed > before.transition.unwrap().elapsed);

    world
        .enqueue(Batch {
            id: 30,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(source_only),
                component: ComponentValue::SCALAR,
            }],
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();
    let stopped = world.animation_controller(controller).unwrap();
    assert_eq!(stopped.state, AnimationPlaybackStatus::Stopped);
    assert_eq!(stopped.transition, None);
    assert_eq!(scalar(&world, survivor), (200.0, 200.0));
    world.update_for_test(2.0).unwrap();
    assert_eq!(scalar(&world, survivor), (200.0, 200.0));
}

#[test]
fn interrupted_disjoint_union_round_trips_with_only_destination_asset_and_resumes() {
    let mut host = HostRuntime::new();
    let world_id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let a = create(&mut world, 1, 101.0);
    let b = create(&mut world, 2, 202.0);
    let c = create(&mut world, 3, 303.0);
    upload(&mut world, 1, &clip(0.0, 10.0));
    upload(&mut world, 2, &clip(20.0, 40.0));
    upload(&mut world, 3, &clip(50.0, 70.0));
    let controller = world
        .create_animation_controller(description(1, &[a], 0.0))
        .unwrap();
    seek_and_play(&mut world, controller);
    transition(&mut world, controller, description(2, &[b], 0.0), 2.0);
    world.update_for_test(0.5).unwrap();
    transition(&mut world, controller, description(3, &[c], 0.0), 2.0);
    world.update_for_test(0.5).unwrap();
    let held = [
        scalar(&world, a).1,
        scalar(&world, b).1,
        scalar(&world, c).1,
    ];
    let saved = world.animation_persistent_state();
    let decoded =
        AnimationPersistentState::decode(&saved.encode(1 << 20).unwrap(), 1 << 20).unwrap();
    assert_eq!(decoded.controllers[0].description.drivers.len(), 1);
    assert_eq!(decoded.controllers[0].description.drivers[0].target, c);

    drop(world);

    let mut restored_host = HostRuntime::new();
    let restored_world_id = restored_host.create_world(WorldLimits::default()).unwrap();
    let mut world = restored_host.world_mut(restored_world_id).unwrap();
    let restored_a = create(&mut world, 1, 101.0);
    let restored_b = create(&mut world, 2, 202.0);
    let restored_c = create(&mut world, 3, 303.0);
    upload(&mut world, 3, &clip(50.0, 70.0));
    let remap = |target: &mut EntityId| {
        *target = if *target == a {
            restored_a
        } else if *target == b {
            restored_b
        } else if *target == c {
            restored_c
        } else {
            panic!("unexpected persisted transition target")
        };
    };
    let mut decoded = decoded;
    for controller in &mut decoded.controllers {
        for driver in &mut controller.description.drivers {
            remap(&mut driver.target);
        }
    }
    for transition in &mut decoded.transitions {
        match &mut transition.source {
            AnimationPersistentTransitionSource::Live(source) => {
                for driver in &mut source.description.drivers {
                    remap(&mut driver.target);
                }
            }
            AnimationPersistentTransitionSource::Frozen {
                values,
                bindings,
                ..
            } => {
                for value in values {
                    remap(&mut value.target);
                }
                for driver in &mut bindings.description.drivers {
                    remap(&mut driver.target);
                }
            }
        }
    }
    for asset in [1, 2] {
        assert!(
            world
                .resolve_asset_key(AssetUploadIdentity {
                    kind: ANIMATION_TYPE,
                    asset,
                    variant: 0,
                })
                .is_none()
        );
    }
    world.restore_animation_controllers(decoded).unwrap();
    world.update_for_test(0.0).unwrap();
    let restored = world.animation_controller(controller).unwrap();
    assert!(!restored.transition.unwrap().pending);
    assert_eq!(
        [
            scalar(&world, restored_a).1,
            scalar(&world, restored_b).1,
            scalar(&world, restored_c).1
        ],
        held
    );
    let elapsed = restored.transition.unwrap().elapsed;
    world.update_for_test(0.5).unwrap();
    let resumed = world.animation_controller(controller).unwrap();
    assert!(!resumed.transition.unwrap().pending);
    assert!(resumed.transition.unwrap().elapsed > elapsed);
    assert_ne!(
        [
            scalar(&world, restored_a).1,
            scalar(&world, restored_b).1,
            scalar(&world, restored_c).1
        ],
        held
    );
    for asset in [1, 2] {
        assert!(
            world
                .resolve_asset_key(AssetUploadIdentity {
                    kind: ANIMATION_TYPE,
                    asset,
                    variant: 0,
                })
                .is_none()
        );
    }

    world
        .control_animation_controller(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, restored_a), (101.0, 101.0));
    assert_eq!(scalar(&world, restored_b), (202.0, 202.0));
    assert_eq!(scalar(&world, restored_c), (303.0, 303.0));
}
