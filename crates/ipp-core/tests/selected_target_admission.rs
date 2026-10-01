//! Real core mutation, binding and private restoration in selected Worlds.

use ipp_core::{
    Batch, BatchOutcome, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef,
    ErrorReason, HostRuntime, WorldContext, WorldLimits,
    components::Scalar,
    services::{
        asset_management::AssetSource,
        world_serialization::{WorldGraphSnapshot, WorldLoadOptions, WorldPersistenceLimits},
    },
    systems::{
        WorldOperation,
        animation::*,
        constraints::{ConstraintSystem, LinearDriver},
    },
};

fn apply(world: &mut WorldContext<'_>, operations: Vec<Command>) -> BatchOutcome {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world.step(0.0).unwrap().outcomes.remove(0)
}

fn create(world: &mut WorldContext<'_>, values: Vec<ComponentValue>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    apply(world, operations).result.unwrap()[0].1
}

fn description(target: EntityId, property: AnimationTrackTarget) -> AnimationControllerDescription {
    AnimationControllerDescription {
        drivers: vec![AnimationDriverDescription {
            source: "asset://10/1".into(),
            variant: 0,
            track: 0,
            target,
            property,
            entity_bindings: Vec::new(),
            weight: 1.0,
            additive: false,
            reference_time: 0.0,
            repeat: false,
        }],
        ..Default::default()
    }
}

fn property(component: u16) -> AnimationTrackTarget {
    AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component,
        offsets: vec![0],
    })
}

fn persistent(description: AnimationControllerDescription) -> AnimationPersistentState {
    AnimationPersistentState {
        next_id: 2,
        controllers: vec![AnimationControllerSnapshot {
            id: AnimationControllerId::from_bits(1),
            description,
            state: AnimationPlaybackStatus::Stopped,
            time: 0.0,
            transition: None,
        }],
        ..Default::default()
    }
}

#[test]
fn absent_animation_rejects_direct_and_queued_mutation_without_panicking() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default(), &[]).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, Vec::new());
    let parameters = description(target, AnimationTrackTarget::EntityLink);
    let controller = AnimationControllerId::from_bits(1);
    assert_eq!(
        world.create_animation_controller(parameters.clone()),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(
        world.update_animation_controller(controller, parameters.clone()),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(
        world.control_animation_controller(controller, AnimationPlaybackControl::Play),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(
        world.remove_animation_controller(controller),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(
        world.restore_animation_controllers(persistent(parameters.clone())),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(
        world.enqueue_animation_controller(2, AnimationControllerCommand::Create(parameters)),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(
        world.enqueue_playback(controller, AnimationPlaybackControl::Play),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert!(world.animation_controllers().is_empty());
    assert!(world.animation_controller_page(0, 0, 10).is_empty());
    assert!(world.animation_controller(controller).is_none());
    assert!(world.animation_persistent_state().controllers.is_empty());
}

#[test]
fn selected_animation_rejects_unsupported_targets_before_resource_readiness() {
    let mut host = HostRuntime::new();
    let id = host
        .create_world(WorldLimits::default(), &[AnimationSystem::ID])
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let target = create(&mut world, Vec::new());
    let structural = description(target, AnimationTrackTarget::EntityLink);
    let controller = world
        .create_animation_controller(structural.clone())
        .unwrap();
    let mut unsupported = vec![
        property(ComponentValue::SCALAR),
        property(ComponentValue::TRANSFORM),
        property(ComponentValue::LINEAR_DRIVER),
        AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::TRANSFORM,
            name: "value".into(),
        },
    ];
    unsupported.push(AnimationTrackTarget::Joints(vec![0]));
    unsupported.push(property(ComponentValue::LOOK_AT));
    for target_property in unsupported {
        let parameters = description(target, target_property);
        assert_eq!(
            world.create_animation_controller(parameters.clone()),
            Err(ErrorReason::UnsupportedDependency)
        );
        assert_eq!(
            world.update_animation_controller(controller, parameters.clone()),
            Err(ErrorReason::UnsupportedDependency)
        );
        assert_eq!(
            world.enqueue_animation_controller(
                3,
                AnimationControllerCommand::Create(parameters.clone())
            ),
            Err(ErrorReason::UnsupportedDependency)
        );
        assert_eq!(
            world.restore_animation_controllers(persistent(parameters)),
            Err(ErrorReason::UnsupportedDependency)
        );
        assert_eq!(
            world.animation_controller(controller).unwrap().description,
            structural
        );
    }
}

#[test]
fn structural_binding_runs_without_component_evaluators_and_leaves_its_last_placement() {
    let mut host = HostRuntime::new();
    let id = host
        .create_world(WorldLimits::default(), &[AnimationSystem::ID])
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    assert_eq!(world.manifest().components().len(), 0);
    let child = create(&mut world, Vec::new());
    let parent = create(&mut world, Vec::new());
    let producer = create(&mut world, Vec::new());
    let clip = AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::EntityLink,
            keys: vec![AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::EntityPlacement(AnimationEntityPlacementKey {
                    parent: Some(0),
                    before: None,
                }),
                interpolation: AnimationInterpolation::Step,
            }],
        }],
    )
    .unwrap();
    drop(world);
    let source = AssetSource {
        kind: ANIMATION_TYPE,
        uri: "https://example.test/structural.ippanim".into(),
        variant: 0,
    };
    host.asset_resources_mut()
        .register_client_source(id, source.clone(), clip.encode())
        .unwrap();
    let key = host.asset_resources().find(&source).unwrap();
    for _attempt in 0..512 {
        host.progress_assets();
        if host
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_some()
        {
            break;
        }
    }
    assert!(
        host.asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_some()
    );
    let mut world = host.world_mut(id).unwrap();
    let mut parameters = description(child, AnimationTrackTarget::EntityLink);
    parameters.drivers[0].source = source.uri;
    parameters.drivers[0].entity_bindings.push(parent);
    let controller = world.create_animation_controller(parameters).unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.step(0.0).unwrap();
    assert_eq!(world.entity_link(child).unwrap().parent, Some(parent));
    apply(
        &mut world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(child),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(producer)),
                before: None,
            },
        }],
    )
    .result
    .unwrap();
    // The driver placed the child again after the client's placement; stop
    // leaves its last placement.
    assert_eq!(world.entity_link(child).unwrap().parent, Some(parent));
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    assert_eq!(world.entity_link(child).unwrap().parent, Some(parent));
}

#[test]
fn scalar_constraints_require_selection_and_survive_private_restore() {
    let mut host = HostRuntime::new();
    let limited = host.create_world(WorldLimits::default(), &[]).unwrap();
    let mut world = host.world_mut(limited).unwrap();
    let source = create(&mut world, Vec::new());
    let target = create(&mut world, Vec::new());
    let result = apply(
        &mut world,
        vec![Command::insert_value(
            EntityRef::Handle(target),
            ComponentValue::Scalar(Scalar {
                value: 1.0,
            }),
        )],
    );
    assert_eq!(
        result.result.unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
    let result = apply(
        &mut world,
        vec![Command::insert_value(
            EntityRef::Handle(target),
            ComponentValue::LinearDriver(LinearDriver {
                source,
                scale: 2.0,
                bias: 1.0,
            }),
        )],
    );
    assert_eq!(
        result.result.unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
    drop(world);

    let id = host
        .create_world(WorldLimits::default(), &[ConstraintSystem::ID])
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let source = create(
        &mut world,
        vec![ComponentValue::Scalar(Scalar {
            value: 3.0,
        })],
    );
    create(
        &mut world,
        vec![
            ComponentValue::Scalar(Scalar {
                value: 1.0,
            }),
            ComponentValue::LinearDriver(LinearDriver {
                source,
                scale: 2.0,
                bias: 1.0,
            }),
        ],
    );
    world.step(0.0).unwrap();
    assert!(
        world
            .entities()
            .iter()
            .any(
                |entity| entity.components.contains(&ComponentValue::Scalar(Scalar {
                    value: 7.0
                }))
            )
    );
    drop(world);
    let limits = WorldPersistenceLimits::default();
    let bytes = host.save_world(id, 123, limits).unwrap();
    let restored = host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions {
                symbolic_id: Some("constraints-copy".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            limits,
        )
        .unwrap()
        .root
        .id();
    let mut world = host.world_mut(restored).unwrap();
    world.step(0.0).unwrap();
    assert!(
        world
            .entities()
            .iter()
            .any(
                |entity| entity.components.contains(&ComponentValue::Scalar(Scalar {
                    value: 7.0
                }))
            )
    );
    drop(world);
    let mut snapshot = WorldGraphSnapshot::decode(&bytes, 123, limits).unwrap();
    snapshot.nodes[0]
        .world
        .selected_systems
        .retain(|system| system != ConstraintSystem::ID.0);
    snapshot.nodes[0].world.capacity_hints.systems.clear();
    let invalid = snapshot.encode(123, limits).unwrap();
    let count = host.world_ids().len();
    assert!(
        host.load_world(
            &invalid,
            123,
            WorldLoadOptions {
                symbolic_id: Some("rejected-constraints".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            limits
        )
        .is_err()
    );
    assert_eq!(host.world_ids().len(), count);
}

#[test]
fn selected_animation_snapshot_rebuilds_bindings_and_rejects_missing_target_evaluator() {
    let mut host = HostRuntime::new();
    let id = host
        .create_world(
            WorldLimits::default(),
            // Scalar values are admitted with the constraints that drive them.
            &[ConstraintSystem::ID, AnimationSystem::ID],
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let removed = create(&mut world, Vec::new());
    let target = create(
        &mut world,
        vec![ComponentValue::Scalar(Scalar {
            value: 5.0,
        })],
    );
    apply(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(removed),
        }],
    )
    .result
    .unwrap();
    let controller = world
        .create_animation_controller(description(target, property(ComponentValue::SCALAR)))
        .unwrap();
    drop(world);
    let limits = WorldPersistenceLimits::default();
    let bytes = host.save_world(id, 123, limits).unwrap();
    let restored = host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions {
                symbolic_id: Some("animation-copy".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            limits,
        )
        .unwrap()
        .root
        .id();
    let mut world = host.world_mut(restored).unwrap();
    let snapshot = world.animation_controller(controller).unwrap();
    assert_ne!(snapshot.description.drivers[0].target, target);
    assert!(
        world
            .manifest()
            .supports_operation(WorldOperation::Animation)
    );
    assert!(
        world
            .inspect(snapshot.description.drivers[0].target)
            .is_some()
    );
    let mut invalid = world.animation_persistent_state();
    invalid.controllers[0].description.drivers[0].property = property(ComponentValue::TRANSFORM);
    assert_eq!(
        world.restore_animation_controllers(invalid),
        Err(ErrorReason::UnsupportedDependency)
    );
    assert_eq!(world.animation_controller(controller), Some(snapshot));
}

#[test]
fn joint_targets_require_joint_animation_and_parent_joint_requires_hierarchy() {
    use ipp_core::systems::{asset_dependencies::AssetDependencySystem, skeleton::SkeletonSystem};

    let mut host = HostRuntime::new();
    let id = host
        .create_world(
            WorldLimits::default(),
            &[
                AnimationSystem::ID,
                AssetDependencySystem::ID,
                SkeletonSystem::ID,
            ],
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    assert!(
        world
            .manifest()
            .supports_operation(WorldOperation::JointAnimation)
    );
    let target = create(
        &mut world,
        vec![ComponentValue::Skeleton(Default::default())],
    );
    let controller = world
        .create_animation_controller(description(target, AnimationTrackTarget::Joints(vec![0])))
        .unwrap();
    let saved = world.animation_persistent_state();
    world.restore_animation_controllers(saved).unwrap();
    assert!(world.animation_controller(controller).is_some());
    let rejected = apply(
        &mut world,
        vec![Command::InsertComponent {
            entity: EntityRef::Handle(target),
            component: ComponentValue::PARENT_JOINT,
            fields: Vec::new(),
            adopt: false,
        }],
    );
    assert_eq!(
        rejected.result.unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
}
