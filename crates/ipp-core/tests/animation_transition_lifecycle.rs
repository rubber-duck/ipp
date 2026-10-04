//! Transition lifetime and restoration through public World and asset APIs.

mod support;

use std::mem::offset_of;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, WorldContext, WorldLimits,
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
use support::selection::{ASSETS, CONSTRAINTS, RENDER, select};

fn create(world: &mut WorldContext<'_>, alias: u32, value: f32) -> EntityId {
    world
        .enqueue(Batch {
            id: u64::from(alias),
            operations: vec![
                Command::Create {
                    alias,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(alias),
                    ComponentValue::Scalar(Scalar {
                        value,
                    }),
                ),
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
                source: std::sync::Arc::<str>::from(format!("asset://10/{asset}")),
                variant: 0,
                track: 0,
                target,
                property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: ComponentValue::SCALAR,
                    offsets: vec![offset_of!(Scalar, value) as u32],
                }),
                entity_bindings: Vec::new(),
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

/// The stored Scalar: authored, or sampled while a driver binds it.
fn scalar(world: &WorldContext<'_>, target: EntityId) -> f32 {
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
    read(&snapshot.components)
}

#[test]
fn removing_and_reusing_a_source_only_component_does_not_rebind_the_transition() {
    let mut host = crate::support::task_scheduler::host();
    let world_id = host
        .create_world(WorldLimits::default(), &select(&[ASSETS, CONSTRAINTS]))
        .unwrap();
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
                Command::insert_value(
                    EntityRef::Handle(removed),
                    ComponentValue::Scalar(Scalar {
                        value: 333.0,
                    }),
                ),
            ],
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(scalar(&world, removed), 333.0);
    let stopped = world.animation_controller(controller).unwrap();
    assert_eq!(stopped.state, AnimationPlaybackStatus::Stopped);
    assert_eq!(stopped.transition, None);
    assert_eq!(scalar(&world, survivor), 200.0);

    world.update_for_test(0.5).unwrap();
    assert_eq!(scalar(&world, removed), 333.0);
    assert_eq!(scalar(&world, survivor), 200.0);
}

#[test]
fn pending_frozen_source_only_replacement_invalidates_the_held_program() {
    let mut host = crate::support::task_scheduler::host();
    let world_id = host
        .create_world(WorldLimits::default(), &select(&[ASSETS, CONSTRAINTS]))
        .unwrap();
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
                Command::insert_value(
                    EntityRef::Handle(removed),
                    ComponentValue::Scalar(Scalar {
                        value: 333.0,
                    }),
                ),
            ],
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();

    assert_eq!(scalar(&world, removed), 333.0);
    assert_eq!(scalar(&world, survivor), 200.0);
    let stopped = world.animation_controller(controller).unwrap();
    assert_eq!(stopped.state, AnimationPlaybackStatus::Stopped);
    assert_eq!(stopped.transition, None);
}

fn material_track(name: &str) -> AnimationTrack {
    AnimationTrack {
        target: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            name: name.into(),
        },
        keys: vec![AnimationKeyframe {
            time: 0.0,
            value: AnimationValue::Field(FieldValue::Dynamic(ipp_core::DynamicValue::F32(0.5))),
            interpolation: AnimationInterpolation::Step,
        }],
    }
}

fn material_properties(world: &WorldContext<'_>, entity: EntityId) -> ipp_core::DynamicProperties {
    let snapshot = world.inspect(entity).unwrap();
    let read = |values: Vec<ComponentValue>| {
        values
            .into_iter()
            .find_map(|value| match value {
                ComponentValue::CustomMaterial(material) => Some(material.properties),
                _ => None,
            })
            .unwrap()
    };
    read(snapshot.components)
}

#[test]
fn pending_frozen_material_property_reuse_does_not_write_through_old_descriptor() {
    let mut host = crate::support::task_scheduler::host();
    let world_id = host.create_world(WorldLimits::default(), RENDER).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let set = |name: &str, value: f32| Command::SetDynamicProperty {
        entity: EntityRef::Alias(1),
        component: ComponentValue::CUSTOM_MATERIAL,
        name: name.into(),
        value: ipp_core::DynamicValue::F32(value),
    };
    world
        .enqueue(Batch {
            id: 30,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    fields: vec![],
                    adopt: false,
                },
                set("amount_a", 1.0),
                set("amount_b", 1.0),
            ],
        })
        .unwrap();
    let entity = world.update_for_test(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    let reused_offset = material_properties(&world, entity).descriptors()["amount_a"].offset;

    let source = AnimationClip::new(
        1.0,
        vec![material_track("amount_a"), material_track("amount_b")],
    )
    .unwrap();
    let destination = AnimationClip::new(1.0, vec![material_track("amount_b")]).unwrap();
    upload(&mut world, 101, &source);
    upload(&mut world, 102, &destination);
    let driver = |asset, track, name: &str| AnimationDriverDescription {
        source: std::sync::Arc::<str>::from(format!("asset://10/{asset}")),
        variant: 0,
        track,
        target: entity,
        property: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            name: name.into(),
        },
        entity_bindings: Vec::new(),
        weight: 1.0,
        additive: false,
        reference_time: 0.0,
        repeat: false,
    };
    let controller = world
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![driver(101, 0, "amount_a"), driver(101, 1, "amount_b")],
            ..Default::default()
        })
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();

    // Interrupt a live transition with one whose destination never loads, so
    // the controller holds a frozen source that still covers `amount_a`.
    transition(
        &mut world,
        controller,
        AnimationControllerDescription {
            drivers: vec![driver(102, 0, "amount_b")],
            ..Default::default()
        },
        2.0,
    );
    world.update_for_test(0.5).unwrap();
    transition(
        &mut world,
        controller,
        AnimationControllerDescription {
            drivers: vec![driver(999, 0, "amount_b")],
            ..Default::default()
        },
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

    // Remove the property, then define a new one of the same kind in its bytes.
    world
        .enqueue(Batch {
            id: 40,
            operations: vec![Command::RemoveDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "amount_a".into(),
            }],
        })
        .unwrap();
    assert!(
        world.update_for_test(0.0).unwrap().outcomes[0]
            .result
            .is_ok()
    );
    world
        .enqueue(Batch {
            id: 41,
            operations: vec![Command::SetDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "amount_c".into(),
                value: ipp_core::DynamicValue::F32(0.9),
            }],
        })
        .unwrap();
    assert!(
        world.update_for_test(0.0).unwrap().outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!(
        material_properties(&world, entity).descriptors()["amount_c"].offset,
        reused_offset,
        "the new property reuses the removed property's storage"
    );

    assert_eq!(
        world.animation_controller(controller).unwrap().state,
        AnimationPlaybackStatus::Stopped
    );
    world.update_for_test(0.5).unwrap();
    assert_eq!(
        material_properties(&world, entity).get("amount_c"),
        Some(ipp_core::DynamicValue::F32(0.9))
    );
}

#[test]
fn interrupted_disjoint_union_restores_every_original_on_stop_and_delete() {
    for delete in [false, true] {
        let mut host = crate::support::task_scheduler::host();
        let world_id = host
            .create_world(WorldLimits::default(), &select(&[ASSETS, CONSTRAINTS]))
            .unwrap();
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
        assert_ne!(scalar(&world, a), 101.0);
        assert_ne!(scalar(&world, b), 202.0);
        assert_ne!(scalar(&world, c), 303.0);

        if delete {
            world.remove_animation_controller(controller).unwrap();
        } else {
            world
                .control_animation_controller(controller, AnimationPlaybackControl::Stop)
                .unwrap();
        }
        world.update_for_test(0.0).unwrap();
        assert_eq!(scalar(&world, a), 101.0);
        assert_eq!(scalar(&world, b), 202.0);
        assert_eq!(scalar(&world, c), 303.0);
    }
}

#[test]
fn outgoing_asset_unload_holds_transition_until_reload_then_target_removal_stops_it() {
    use support::HostWorldTestDriver;

    let mut host = crate::support::task_scheduler::host();
    let world_id = host
        .create_world(WorldLimits::default(), &select(&[ASSETS, CONSTRAINTS]))
        .unwrap();
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
    let held_source = scalar(&world, source_only);
    let held_survivor = scalar(&world, survivor);
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
    assert_eq!(scalar(&world, source_only), held_source);
    assert_eq!(scalar(&world, survivor), held_survivor);
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
    assert_eq!(scalar(&world, survivor), 200.0);
    world.update_for_test(2.0).unwrap();
    assert_eq!(scalar(&world, survivor), 200.0);
}

#[test]
fn interrupted_disjoint_union_round_trips_with_only_destination_asset_and_resumes() {
    let mut host = crate::support::task_scheduler::host();
    let world_id = host
        .create_world(WorldLimits::default(), &select(&[ASSETS, CONSTRAINTS]))
        .unwrap();
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
    let held = [scalar(&world, a), scalar(&world, b), scalar(&world, c)];
    let saved = world.animation_persistent_state();
    let decoded =
        AnimationPersistentState::decode(&saved.encode(1 << 20).unwrap(), 1 << 20).unwrap();
    assert_eq!(decoded.controllers[0].description.drivers.len(), 1);
    assert_eq!(decoded.controllers[0].description.drivers[0].target, c);

    drop(world);

    let mut restored_host = crate::support::task_scheduler::host();
    let restored_world_id = restored_host
        .create_world(WorldLimits::default(), &select(&[ASSETS, CONSTRAINTS]))
        .unwrap();
    let mut world = restored_host.world_mut(restored_world_id).unwrap();
    // Fields hold what was saved, contributions included, as a loaded World's do.
    let restored_a = create(&mut world, 1, held[0]);
    let restored_b = create(&mut world, 2, held[1]);
    let restored_c = create(&mut world, 3, held[2]);
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
    for contribution in &mut decoded.contributions {
        remap(&mut contribution.target);
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
            scalar(&world, restored_a),
            scalar(&world, restored_b),
            scalar(&world, restored_c)
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
            scalar(&world, restored_a),
            scalar(&world, restored_b),
            scalar(&world, restored_c)
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
    assert_eq!(scalar(&world, restored_a), 101.0);
    assert_eq!(scalar(&world, restored_b), 202.0);
    assert_eq!(scalar(&world, restored_c), 303.0);
}
