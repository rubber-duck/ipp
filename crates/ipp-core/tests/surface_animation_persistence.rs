//! Surface animation invalidation must prune durable controller bindings.
#![cfg(feature = "surfaces")]

mod support;
use support::WorldTestDriver;

use ipp_core::components::schema::FieldValue;
use ipp_core::services::asset_management::{AssetUpload, AssetUploadIdentity};
use ipp_core::services::world_serialization::WorldLoadOptions;
use ipp_core::systems::animation::{
    ANIMATION_TYPE, AnimationClip, AnimationControllerDescription, AnimationDriverDescription,
    AnimationInterpolation, AnimationKeyframe, AnimationPlaybackControl, AnimationPlaybackStatus,
    AnimationTrack, AnimationTrackTarget, AnimationValue,
};
use ipp_core::{
    Batch, Command, ComponentValue, DynamicValue, EntityRef, ErrorReason, HostRuntime, Surface,
    SurfaceCommand, SurfaceItemContent, SurfaceItemId, SurfaceItemStyle, WorldLimits,
};

fn create_surface(host: &mut HostRuntime, world: ipp_core::WorldId) -> ipp_core::EntityId {
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue(Batch {
            id: 1,
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
    context.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1
}

fn edit(host: &mut HostRuntime, world: ipp_core::WorldId, request: u64, command: SurfaceCommand) {
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue_surface_command_with_reply(1, request, command)
        .unwrap();
    let report = context.step(0.0).unwrap();
    assert_eq!(report.system_command_outcomes[0].result, Ok(()));
}

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
fn removed_surface_properties_are_pruned_from_durable_animation_drivers() {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let entity = create_surface(&mut host, world);
    for id in [1, 2] {
        edit(
            &mut host,
            world,
            10 + u64::from(id),
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
    let clip = AnimationClip::new(1.0, names.iter().cloned().map(opacity_track).collect()).unwrap();
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
                .iter()
                .enumerate()
                .map(|(track, name)| AnimationDriverDescription {
                    source: format!("asset://10/{asset}"),
                    variant: 0,
                    track: track as u32,
                    target: entity,
                    property: AnimationTrackTarget::DynamicProperty {
                        component: ComponentValue::SURFACE,
                        name: name.clone(),
                    },
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
    context.step(0.0).unwrap();
    context.step(0.0).unwrap();
    drop(context);

    edit(
        &mut host,
        world,
        18,
        SurfaceCommand::Update {
            entity,
            id: SurfaceItemId(1),
            patch: ipp_core::SurfaceItemPatch {
                opacity: Some(0.8),
                ..Default::default()
            },
        },
    );
    edit(
        &mut host,
        world,
        19,
        SurfaceCommand::Update {
            entity,
            id: SurfaceItemId(1),
            patch: ipp_core::SurfaceItemPatch {
                color: Some([0.2, 0.3, 0.4, 1.0]),
                ..Default::default()
            },
        },
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
        "a later color edit must not retain the preceding opacity field invalidation"
    );

    edit(
        &mut host,
        world,
        20,
        SurfaceCommand::Remove {
            entity,
            id: SurfaceItemId(1),
        },
    );
    let snapshot = host
        .world_mut(world)
        .unwrap()
        .animation_controller(controller)
        .unwrap();
    assert_eq!(snapshot.description.drivers.len(), 1);
    assert_eq!(
        snapshot.description.drivers[0].property,
        opacity_track(names[1].clone()).target
    );

    edit(
        &mut host,
        world,
        21,
        SurfaceCommand::Remove {
            entity,
            id: SurfaceItemId(2),
        },
    );
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
                symbolic_id: Some("surface-animation-restored".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            Default::default(),
        )
        .unwrap();
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
