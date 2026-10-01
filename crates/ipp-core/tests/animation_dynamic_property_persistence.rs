//! Dynamic property invalidation must prune durable controller bindings, with
//! CustomMaterial dynamic properties as the real consumer.

mod support;
use support::WorldTestDriver;
use support::selection::RENDER;

use std::mem::offset_of;

use ipp_core::components::CustomMaterial;
use ipp_core::components::schema::FieldValue as SchemaValue;
use ipp_core::services::asset_management::{AssetUpload, AssetUploadIdentity};
use ipp_core::services::world_serialization::WorldLoadOptions;
use ipp_core::systems::animation::{
    ANIMATION_TYPE, AnimationClip, AnimationControllerDescription, AnimationDriverDescription,
    AnimationInterpolation, AnimationKeyframe, AnimationPlaybackControl, AnimationPlaybackStatus,
    AnimationTrack, AnimationTrackTarget, AnimationValue,
};
use ipp_core::{
    Batch, Command, ComponentValue, DynamicValue, EntityId, EntityRef, ErrorReason, FieldValue,
    FieldWrite, HostRuntime, WorldId, WorldLimits,
};

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> Vec<EntityId> {
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue(Batch {
            id: context.tick() + 1,
            operations,
        })
        .unwrap();
    let outcome = context.update_for_test(0.0).unwrap().outcomes.remove(0);
    outcome
        .result
        .unwrap()
        .into_iter()
        .map(|(_, entity)| entity)
        .collect()
}

fn set_property(host: &mut HostRuntime, world: WorldId, entity: EntityId, name: &str, value: f32) {
    apply(
        host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: name.into(),
            value: DynamicValue::F32(value),
        }],
    );
}

fn remove_property(host: &mut HostRuntime, world: WorldId, entity: EntityId, name: &str) {
    apply(
        host,
        world,
        vec![Command::RemoveDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: name.into(),
        }],
    );
}

fn property_target(name: &str) -> AnimationTrackTarget {
    AnimationTrackTarget::DynamicProperty {
        component: ComponentValue::CUSTOM_MATERIAL,
        name: name.into(),
    }
}

/// Adds up to 1 over the clip's second.
fn amount_track(name: &str) -> AnimationTrack {
    AnimationTrack {
        target: property_target(name),
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::F32(0.0))),
                interpolation: AnimationInterpolation::Linear,
            },
            AnimationKeyframe {
                time: 1.0,
                value: AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::F32(1.0))),
                interpolation: AnimationInterpolation::Step,
            },
        ],
    }
}

fn effective_property(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
    name: &str,
) -> Option<DynamicValue> {
    host.world_mut(world)
        .unwrap()
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::CustomMaterial(material) => Some(material.properties.get(name)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn removed_dynamic_properties_are_pruned_from_durable_animation_drivers() {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default(), RENDER).unwrap();
    let entity = apply(
        &mut host,
        world,
        vec![
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
        ],
    )[0];
    let names = ["amount_a", "amount_b"];
    for name in names {
        set_property(&mut host, world, entity, name, 1.0);
    }

    let clip = AnimationClip::new(1.0, names.into_iter().map(amount_track).collect()).unwrap();
    let asset = 91;
    let mut context = host.world_mut(world).unwrap();
    context
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
    assert!(context.await_upload_for_test().assets[0].result.is_ok());
    let controller = context
        .create_animation_controller(AnimationControllerDescription {
            drivers: names
                .into_iter()
                .enumerate()
                .map(|(track, name)| AnimationDriverDescription {
                    source: std::sync::Arc::<str>::from(format!("asset://10/{asset}")),
                    variant: 0,
                    track: track as u32,
                    target: entity,
                    property: property_target(name),
                    entity_bindings: Vec::new(),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                })
                .collect(),
            ..Default::default()
        })
        .unwrap();
    context
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    context
        .enqueue_playback(controller, AnimationPlaybackControl::Seek(0.5))
        .unwrap();
    context.update_for_test(0.0).unwrap();
    context.update_for_test(0.0).unwrap();
    drop(context);
    assert_eq!(
        effective_property(&mut host, world, entity, names[1]),
        Some(DynamicValue::F32(1.5))
    );

    // A same-kind value write and a later unrelated field write keep both
    // properties, so neither may prune a driver.
    set_property(&mut host, world, entity, names[0], 0.8);
    apply(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            field: FieldWrite {
                offset: offset_of!(CustomMaterial, alpha_cutoff) as u32,
                value: FieldValue::F32(0.25),
            },
        }],
    );
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .animation_controller(controller)
            .unwrap()
            .description
            .drivers
            .len(),
        2,
        "a later unrelated edit must not retain the preceding property invalidation"
    );

    remove_property(&mut host, world, entity, names[0]);
    let snapshot = host
        .world_mut(world)
        .unwrap()
        .animation_controller(controller)
        .unwrap();
    assert_eq!(snapshot.description.drivers.len(), 1);
    assert_eq!(
        snapshot.description.drivers[0].property,
        property_target(names[1])
    );

    remove_property(&mut host, world, entity, names[1]);
    let snapshot = host
        .world_mut(world)
        .unwrap()
        .animation_controller(controller)
        .unwrap();
    assert_eq!(snapshot.state, AnimationPlaybackStatus::Stopped);
    assert_eq!(snapshot.time, 0.5);
    assert!(snapshot.description.drivers.is_empty());

    let persistent = host.world_mut(world).unwrap().animation_persistent_state();
    let mut reverse = persistent.clone();
    reverse.controllers[0].description.speed = -1.0;
    host.world_mut(world)
        .unwrap()
        .restore_animation_controllers(reverse)
        .unwrap();
    assert_eq!(
        host.world_mut(world).unwrap().animation_controllers()[0]
            .description
            .speed,
        -1.0
    );

    for speed in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut invalid = persistent.clone();
        invalid.controllers[0].description.speed = speed;
        assert_eq!(
            host.world_mut(world)
                .unwrap()
                .restore_animation_controllers(invalid),
            Err(ErrorReason::InvalidValue)
        );
    }

    let mut invalid = persistent;
    invalid.controllers[0].state = AnimationPlaybackStatus::Playing;
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .restore_animation_controllers(invalid),
        Err(ErrorReason::InvalidValue)
    );

    let bytes = host.save_world(world, 44, Default::default()).unwrap();
    let restored = host
        .load_world(
            &bytes,
            44,
            WorldLoadOptions {
                symbolic_id: Some("dynamic-property-animation-restored".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            Default::default(),
        )
        .unwrap()
        .root
        .id();
    let restored_controllers = host.world_mut(restored).unwrap().animation_controllers();
    assert_eq!(restored_controllers.len(), 1);
    assert_eq!(restored_controllers[0].id, controller);
    assert_eq!(
        restored_controllers[0].state,
        AnimationPlaybackStatus::Stopped
    );
    assert_eq!(restored_controllers[0].time, 0.5);
    assert!(restored_controllers[0].description.drivers.is_empty());
}
