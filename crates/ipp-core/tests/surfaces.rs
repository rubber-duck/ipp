//! Surface collection, command lifetime, and persistence behavior.
#![cfg(feature = "surfaces")]

use ipp_core::services::asset_management::{AssetSource, AssetTypeId};
use ipp_core::{
    Batch, Command, ComponentValue, EntityRef, HostRuntime, Surface, SurfaceCommand,
    SurfaceItemContent, SurfaceItemId, SurfaceItemPatch, SurfaceItemStyle, WorldLimits,
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
    assert_eq!(report.system_command_outcomes.len(), 1);
    assert_eq!(report.system_command_outcomes[0].request_id, request);
    assert_eq!(report.system_command_outcomes[0].result, Ok(()));
}

#[test]
fn live_edits_preserve_ordered_identity_and_snapshot_high_water() {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let entity = create_surface(&mut host, world);
    edit(
        &mut host,
        world,
        10,
        SurfaceCommand::Insert {
            entity,
            id: SurfaceItemId(1),
            index: 0,
            content: SurfaceItemContent::Label("a\nb".into()),
            style: SurfaceItemStyle::default(),
        },
    );
    edit(
        &mut host,
        world,
        11,
        SurfaceCommand::Insert {
            entity,
            id: SurfaceItemId(2),
            index: 1,
            content: SurfaceItemContent::Drawing,
            style: SurfaceItemStyle::default(),
        },
    );
    let position_name = Surface::property_name(SurfaceItemId(1), "position").unwrap();
    let position_key = host
        .world_mut(world)
        .unwrap()
        .surface(entity)
        .unwrap()
        .properties
        .key(&position_name)
        .unwrap();
    edit(
        &mut host,
        world,
        12,
        SurfaceCommand::Move {
            entity,
            id: SurfaceItemId(1),
            index: 1,
        },
    );
    {
        let context = host.world_mut(world).unwrap();
        let surface = context.surface(entity).unwrap();
        assert_eq!(
            surface
                .items()
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            [SurfaceItemId(2), SurfaceItemId(1)]
        );
        assert_eq!(surface.properties.key(&position_name), Some(position_key));
    }

    edit(
        &mut host,
        world,
        13,
        SurfaceCommand::Update {
            entity,
            id: SurfaceItemId(1),
            patch: SurfaceItemPatch {
                opacity: Some(0.25),
                ..Default::default()
            },
        },
    );
    edit(
        &mut host,
        world,
        14,
        SurfaceCommand::Remove {
            entity,
            id: SurfaceItemId(1),
        },
    );
    {
        let context = host.world_mut(world).unwrap();
        let surface = context.surface(entity).unwrap();
        assert_eq!(surface.next_item_id(), 3);
        assert_eq!(surface.properties.key(&position_name), None);
    }

    let bytes = host.save_world(world, 44, Default::default()).unwrap();
    let restored = host
        .load_world(
            &bytes,
            44,
            ipp_core::services::world_serialization::WorldLoadOptions {
                symbolic_id: Some("surface-copy".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            Default::default(),
        )
        .unwrap();
    let restored_entity = host.world_mut(restored).unwrap().entities()[0].id;
    {
        let restored_context = host.world_mut(restored).unwrap();
        let restored_surface = restored_context.surface(restored_entity).unwrap();
        assert_eq!(restored_surface.next_item_id(), 3);
        assert_eq!(restored_surface.items()[0].id, SurfaceItemId(2));
    }
}

#[test]
fn semantic_style_domains_reject_invalid_live_edits() {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let entity = create_surface(&mut host, world);
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue_surface_command_with_reply(
            1,
            20,
            SurfaceCommand::Insert {
                entity,
                id: SurfaceItemId(1),
                index: 0,
                content: SurfaceItemContent::Drawing,
                style: SurfaceItemStyle {
                    opacity: 1.1,
                    ..Default::default()
                },
            },
        )
        .unwrap();
    let report = context.step(0.0).unwrap();
    assert!(report.system_command_outcomes[0].result.is_err());
    assert!(context.surface(entity).unwrap().items().is_empty());
    drop(context);

    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue_surface_command_with_reply(
            1,
            21,
            SurfaceCommand::Insert {
                entity,
                id: SurfaceItemId(1),
                index: 0,
                content: SurfaceItemContent::Label("wrong asset".into()),
                style: SurfaceItemStyle {
                    asset: Some(AssetSource {
                        kind: AssetTypeId(18),
                        uri: "fixture://drawing.ippd".into(),
                        variant: 0,
                    }),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    let report = context.step(0.0).unwrap();
    assert_eq!(
        report.system_command_outcomes[0].result,
        Err(ipp_core::ErrorReason::InvalidValue)
    );
    assert!(context.surface(entity).unwrap().items().is_empty());
}

#[test]
fn content_update_reuses_the_live_label_allocation() {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let entity = create_surface(&mut host, world);
    edit(
        &mut host,
        world,
        30,
        SurfaceCommand::Insert {
            entity,
            id: SurfaceItemId(1),
            index: 0,
            content: SurfaceItemContent::Label("a deliberately overallocated terminal line".into()),
            style: SurfaceItemStyle::default(),
        },
    );
    let (before, capacity) = {
        let context = host.world_mut(world).unwrap();
        let SurfaceItemContent::Label(label) = &context.surface(entity).unwrap().items()[0].content
        else {
            unreachable!()
        };
        (label.as_ptr(), label.capacity())
    };
    edit(
        &mut host,
        world,
        31,
        SurfaceCommand::Update {
            entity,
            id: SurfaceItemId(1),
            patch: SurfaceItemPatch {
                content: Some(SurfaceItemContent::Label("short line".into())),
                ..Default::default()
            },
        },
    );
    let context = host.world_mut(world).unwrap();
    let SurfaceItemContent::Label(label) = &context.surface(entity).unwrap().items()[0].content
    else {
        unreachable!()
    };
    assert_eq!(label, "short line");
    assert_eq!(label.as_ptr(), before);
    assert_eq!(label.capacity(), capacity);
    drop(context);

    for (request, patch) in [
        (
            32,
            SurfaceItemPatch {
                opacity: Some(0.4),
                ..Default::default()
            },
        ),
        (
            33,
            SurfaceItemPatch {
                color: Some([0.2, 0.3, 0.4, 1.0]),
                ..Default::default()
            },
        ),
    ] {
        edit(
            &mut host,
            world,
            request,
            SurfaceCommand::Update {
                entity,
                id: SurfaceItemId(1),
                patch,
            },
        );
    }
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue_surface_command_with_reply(
            1,
            34,
            SurfaceCommand::Update {
                entity,
                id: SurfaceItemId(1),
                patch: SurfaceItemPatch {
                    content: Some(SurfaceItemContent::Bitmap {
                        size: [0.0, 1.0],
                    }),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    let report = context.step(0.0).unwrap();
    assert_eq!(
        report.system_command_outcomes[0].result,
        Err(ipp_core::ErrorReason::InvalidField)
    );
    let surface = context.surface(entity).unwrap();
    let (_, style) = surface.item(SurfaceItemId(1)).unwrap();
    assert_eq!(style.opacity, 0.4);
    assert_eq!(style.color, [0.2, 0.3, 0.4, 1.0]);
    assert!(matches!(
        &surface.items()[0].content,
        SurfaceItemContent::Label(label) if label == "short line"
    ));
}
