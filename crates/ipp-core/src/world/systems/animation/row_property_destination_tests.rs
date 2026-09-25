//! Animation of schema-row properties, with GuiRoot node style and node data
//! rows as the real consumer: binding by offset, compiled sampling, per-driver
//! invalidation, restoration, transitions and clip interchange.

use crate::components::rows::Rows;
use crate::components::schema::{FieldValue as SchemaValue, SchemaComponent};
use crate::services::asset_management::{AssetUpload, AssetUploadIdentity};
use crate::systems::animation::*;
use crate::systems::gui::{
    GuiCommand, GuiContainerKind, GuiNodeData, GuiNodeDataProperty, GuiNodeDataRow, GuiNodeId,
    GuiNodeStyle, GuiNodeStyleProperty, GuiNodeStyleRow, GuiRoot,
};
use crate::{
    Batch, BatchOutcome, Command, ComponentValue, DynamicValue, EntityId, EntityRef, ErrorReason,
    FieldValue, FieldWrite, HostRuntime, WorldContext, WorldLimits, WorldUpdateReport,
};

const OPACITY: GuiNodeStyleProperty = GuiNodeStyleProperty::Opacity;

fn offset(node: u32, property: GuiNodeStyleProperty) -> u32 {
    GuiRoot::node_style_offset(GuiNodeId(node), property).unwrap()
}

/// Real offsets of the exposed rows fields: `node_style`, `node_data`,
/// `theme_parts`, then `part_state`.
fn rows_fields() -> [u32; 4] {
    let rows: Vec<u32> = GuiRoot::default()
        .fields()
        .into_iter()
        .filter(|(_, value)| matches!(value, SchemaValue::Rows(_)))
        .map(|(offset, _)| offset)
        .collect();
    rows.try_into().unwrap()
}

/// Node 3 has an explicit width; every other node keeps optional sizes absent.
fn style(node: u32) -> GuiNodeStyle {
    GuiNodeStyle {
        width: (node == 3).then_some(1.0),
        ..GuiNodeStyle::default()
    }
}

/// Node style rows for `nodes`, as their GUI insertion produces them.
fn style_table(nodes: &[u32]) -> Vec<u8> {
    let mut table = Rows::<GuiNodeStyleRow>::new();
    for &node in nodes {
        table
            .insert(node, GuiNodeStyleRow::from(&style(node)))
            .unwrap();
    }
    table.encode()
}

fn contents() -> [(u32, Option<u32>, GuiNodeData, GuiNodeDataRow); 3] {
    [
        (
            1,
            None,
            GuiNodeData::Container(GuiContainerKind::Column),
            GuiNodeDataRow::default(),
        ),
        (
            2,
            Some(1),
            GuiNodeData::Checkbox,
            GuiNodeDataRow::checkbox(false),
        ),
        (
            3,
            Some(1),
            GuiNodeData::Image,
            GuiNodeDataRow::image([1.0, 2.0]),
        ),
    ]
}

/// A GUI root with a column (1), a checkbox (2) and an image (3), inserted
/// through GUI commands, which also insert the node rows.
fn gui_entity(world: &mut WorldContext<'_>) -> EntityId {
    let entity = submit(
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::Surface(crate::components::Surface::default()),
            ),
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::GuiRoot(GuiRoot::default()),
            ),
        ],
    )
    .result
    .unwrap()[0]
        .1;
    let root_incarnation = world
        .inspect_gui(entity, None, 1, 1)
        .unwrap()
        .root_incarnation;
    for (id, parent, data, values) in contents() {
        gui(
            world,
            GuiCommand::InsertNode {
                entity,
                root_incarnation,
                id: GuiNodeId(id),
                parent: parent.map(GuiNodeId),
                index: u32::MAX,
                data,
                values,
                style: style(id),
            },
        );
    }

    entity
}

fn gui(world: &mut WorldContext<'_>, command: GuiCommand) {
    world.enqueue_gui_command_with_reply(1, 1, command).unwrap();
    let report = update(world, 0.0);
    assert_eq!(report.system_command_outcomes[0].result, Ok(()));
}

/// Remove node 2 through its GUI command, which also removes its rows; returns
/// the frame reports.
fn remove_node_two(world: &mut WorldContext<'_>, entity: EntityId) -> Vec<WorldUpdateReport> {
    let mut reports = Vec::new();
    let root_incarnation = world
        .inspect_gui(entity, None, 1, 1)
        .unwrap()
        .root_incarnation;
    world
        .enqueue_gui_command_with_reply(
            1,
            1,
            GuiCommand::RemoveNode {
                handle: crate::GuiNodeHandle::new(1, entity, root_incarnation, GuiNodeId(2)),
            },
        )
        .unwrap();
    let report = update(world, 0.0);
    assert_eq!(report.system_command_outcomes[0].result, Ok(()));
    reports.push(report);
    reports
}

fn submit(world: &mut WorldContext<'_>, operations: Vec<Command>) -> BatchOutcome {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    update(world, 0.0).outcomes.remove(0)
}

fn update(world: &mut WorldContext<'_>, dt: f64) -> WorldUpdateReport {
    world.prepare_update(dt).unwrap();
    world.poll_assets();
    world.step(dt).unwrap()
}

fn key(time: f64, value: DynamicValue) -> AnimationKeyframe {
    AnimationKeyframe {
        time,
        value: AnimationValue::Field(SchemaValue::Dynamic(value)),
        interpolation: AnimationInterpolation::Linear,
    }
}

fn target(offset: u32) -> AnimationTrackTarget {
    AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::GUI_ROOT,
        offsets: vec![offset],
    })
}

/// Linear from `start` to `end` over two seconds.
fn track(offset: u32, start: DynamicValue, end: DynamicValue) -> AnimationTrack {
    AnimationTrack {
        target: target(offset),
        keys: vec![
            key(0.0, start),
            AnimationKeyframe {
                interpolation: AnimationInterpolation::Step,
                ..key(2.0, end)
            },
        ],
    }
}

/// One F32 track per target, linear from `start` to `end` over two seconds.
fn clip(targets: &[u32], start: f32, end: f32) -> AnimationClip {
    AnimationClip::new(
        2.0,
        targets
            .iter()
            .map(|&offset| track(offset, DynamicValue::F32(start), DynamicValue::F32(end)))
            .collect(),
    )
    .unwrap()
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
    for _ in 0..512 {
        if !update(world, 0.0).assets.is_empty() {
            return;
        }
    }
    panic!("animation clip upload did not finish");
}

fn description(entity: EntityId, asset: u64, targets: &[u32]) -> AnimationControllerDescription {
    AnimationControllerDescription {
        drivers: targets
            .iter()
            .enumerate()
            .map(|(track, &offset)| AnimationDriverDescription {
                source: format!("asset://10/{asset}"),
                variant: 0,
                track: track as u32,
                target: entity,
                property: target(offset),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            })
            .collect(),
        ..Default::default()
    }
}

fn seek(world: &mut WorldContext<'_>, id: AnimationControllerId, time: f64) -> WorldUpdateReport {
    for control in [
        AnimationPlaybackControl::Play,
        AnimationPlaybackControl::Seek(time),
        AnimationPlaybackControl::Pause,
    ] {
        world.enqueue_playback(id, control).unwrap();
    }
    update(world, 0.0)
}

/// (base, effective) value of one row property.
fn property(world: &WorldContext<'_>, entity: EntityId, offset: u32) -> (SchemaValue, SchemaValue) {
    let snapshot = world.inspect(entity).unwrap();
    let read = |values: &[ComponentValue]| {
        values
            .iter()
            .find(|value| value.type_id() == ComponentValue::GUI_ROOT)
            .unwrap()
            .field(offset)
            .unwrap()
    };
    (read(&snapshot.base), read(&snapshot.effective))
}

fn f32_value(value: f32) -> SchemaValue {
    SchemaValue::Dynamic(DynamicValue::F32(value))
}

fn world() -> (HostRuntime, crate::WorldId) {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    (host, id)
}

fn invalidated(report: &WorldUpdateReport, id: AnimationControllerId) -> bool {
    report.playback_events.iter().any(|event| {
        event.controller.id == id && event.kind == AnimationPlaybackEventKind::Invalidated
    })
}

#[test]
fn row_targets_bind_by_offset_and_sample_through_a_compiled_destination() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = gui_entity(&mut world);
    let opacity = offset(2, OPACITY);
    let scale = offset(3, GuiNodeStyleProperty::Scale);
    let clip = AnimationClip::new(
        2.0,
        vec![
            track(opacity, DynamicValue::F32(0.0), DynamicValue::F32(1.0)),
            track(
                scale,
                DynamicValue::Vec2([1.0, 1.0]),
                DynamicValue::Vec2([3.0, 5.0]),
            ),
        ],
    )
    .unwrap();
    upload(&mut world, 1, &clip);
    let controller = world
        .create_animation_controller(description(entity, 1, &[opacity, scale]))
        .unwrap();
    seek(&mut world, controller, 1.0);

    assert_eq!(
        property(&world, entity, opacity),
        (f32_value(1.0), f32_value(0.5))
    );
    assert_eq!(
        property(&world, entity, scale),
        (
            SchemaValue::Dynamic(DynamicValue::Vec2([1.0, 1.0])),
            SchemaValue::Dynamic(DynamicValue::Vec2([2.0, 3.0]))
        )
    );

    let system = world
        .system::<AnimationSystem>(AnimationSystem::ID)
        .unwrap();
    let drivers = &system.state.controllers[&controller].drivers;
    assert!(
        drivers.iter().all(|driver| driver.has_numeric_binding()),
        "numeric row properties write through the compiled row destination"
    );
}

#[test]
fn row_targets_reject_absent_command_owned_table_and_out_of_range_values() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = gui_entity(&mut world);
    let width = offset(2, GuiNodeStyleProperty::Width);
    let checked = GuiRoot::node_data_offset(GuiNodeId(2), GuiNodeDataProperty::Checked).unwrap();
    let dead = offset(4, OPACITY);
    let table = rows_fields()[0];
    upload(&mut world, 1, &clip(&[width], 0.0, 1.0));

    for rejected in [width, dead, table] {
        assert_eq!(
            world.create_animation_controller(description(entity, 1, &[rejected])),
            Err(ErrorReason::InvalidField),
            "offset {rejected:#x}"
        );
    }

    // Discrete tracks take the general path; binding still rejects committed
    // control values and the other properties GuiRoot keeps from animation.
    let enabled = offset(2, GuiNodeStyleProperty::Enabled);
    for (asset, rejected) in [(2, checked), (4, enabled)] {
        let toggle = AnimationClip::new(
            1.0,
            vec![AnimationTrack {
                target: target(rejected),
                keys: vec![AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::Bool(true))),
                    interpolation: AnimationInterpolation::Step,
                }],
            }],
        )
        .unwrap();
        upload(&mut world, asset, &toggle);
        assert_eq!(
            world.create_animation_controller(description(entity, asset, &[rejected])),
            Err(ErrorReason::InvalidField),
            "offset {rejected:#x} is never an animation target"
        );
    }

    let opacity = offset(2, OPACITY);
    upload(&mut world, 3, &clip(&[opacity], 0.0, 2.0));
    let controller = world
        .create_animation_controller(description(entity, 3, &[opacity]))
        .unwrap();
    seek(&mut world, controller, 0.5);
    assert_eq!(property(&world, entity, opacity).1, f32_value(0.5));
    let report = seek(&mut world, controller, 1.5);
    assert!(
        report
            .playback_events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed),
        "{:?}",
        report.playback_events
    );
    assert_ne!(property(&world, entity, opacity).1, f32_value(1.5));
}

#[test]
fn removing_a_row_or_clearing_its_property_drops_only_its_drivers() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = gui_entity(&mut world);
    let removed = offset(2, OPACITY);
    let cleared = offset(3, GuiNodeStyleProperty::Width);
    let survivor = offset(3, OPACITY);
    upload(
        &mut world,
        1,
        &clip(&[removed, cleared, survivor], 0.0, 1.0),
    );
    let controller = world
        .create_animation_controller(description(entity, 1, &[removed, cleared, survivor]))
        .unwrap();
    seek(&mut world, controller, 1.0);
    assert_eq!(property(&world, entity, cleared).1, f32_value(0.5));

    // Removing node 2 removes its rows; node 3's drivers survive.
    let reports = remove_node_two(&mut world, entity);
    assert!(reports.iter().any(|report| invalidated(report, controller)));
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.description.drivers.len(), 2);

    // Clearing the optional width drops that driver; opacity keeps sampling.
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_ROOT,
                field: FieldWrite {
                    offset: cleared,
                    value: FieldValue::Unset,
                },
            }],
        })
        .unwrap();
    let report = update(&mut world, 0.0);
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    assert!(invalidated(&report, controller));
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.description.drivers.len(), 1);
    assert_eq!(snapshot.description.drivers[0].property, target(survivor));

    seek(&mut world, controller, 0.5);
    assert_eq!(
        property(&world, entity, cleared),
        (SchemaValue::Unset, SchemaValue::Unset)
    );
    assert_eq!(property(&world, entity, survivor).1, f32_value(0.25));
}

#[test]
fn removing_a_controller_restores_underlying_row_values() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = gui_entity(&mut world);
    let opacity = offset(2, OPACITY);
    let width = offset(3, GuiNodeStyleProperty::Width);
    upload(&mut world, 1, &clip(&[opacity, width], 0.0, 0.5));
    let controller = world
        .create_animation_controller(description(entity, 1, &[opacity, width]))
        .unwrap();
    seek(&mut world, controller, 2.0);
    assert_eq!(property(&world, entity, opacity).1, f32_value(0.5));
    assert_eq!(property(&world, entity, width).1, f32_value(0.5));

    world.remove_animation_controller(controller).unwrap();
    update(&mut world, 0.0);
    assert_eq!(
        property(&world, entity, opacity),
        (f32_value(1.0), f32_value(1.0))
    );
    assert_eq!(
        property(&world, entity, width),
        (f32_value(1.0), f32_value(1.0))
    );
}

#[test]
fn transitions_blend_row_targets_and_reject_out_of_range_channels() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = gui_entity(&mut world);
    let opacity = offset(2, OPACITY);
    upload(&mut world, 1, &clip(&[opacity], 0.2, 0.2));
    upload(&mut world, 2, &clip(&[opacity], 0.8, 0.8));
    let controller = world
        .create_animation_controller(description(entity, 1, &[opacity]))
        .unwrap();
    seek(&mut world, controller, 0.0);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    update(&mut world, 0.0);
    assert_eq!(property(&world, entity, opacity).1, f32_value(0.2));

    world
        .transition_animation_controller(
            controller,
            AnimationControllerTransition {
                description: description(entity, 2, &[opacity]),
                duration: 1.0,
                easing: AnimationTransitionEasing::Linear,
                start_time: AnimationTransitionStartTime::Restart,
            },
        )
        .unwrap();
    update(&mut world, 0.0);
    update(&mut world, 0.5);
    let SchemaValue::Dynamic(DynamicValue::F32(blended)) = property(&world, entity, opacity).1
    else {
        panic!("opacity row property");
    };
    assert!((blended - 0.5).abs() < 1.0e-5, "{blended}");
    update(&mut world, 0.5);
    assert_eq!(property(&world, entity, opacity).1, f32_value(0.8));
    assert_eq!(property(&world, entity, opacity).0, f32_value(1.0));

    // A channel value outside the property's range fails the controller
    // before any output publishes.
    upload(&mut world, 3, &clip(&[opacity], 1.5, 1.5));
    world
        .transition_animation_controller(
            controller,
            AnimationControllerTransition {
                description: description(entity, 3, &[opacity]),
                duration: 1.0,
                easing: AnimationTransitionEasing::Linear,
                start_time: AnimationTransitionStartTime::Restart,
            },
        )
        .unwrap();
    let mut failed = false;
    for _ in 0..4 {
        let report = update(&mut world, 0.5);
        failed |= report
            .playback_events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed);
        let SchemaValue::Dynamic(DynamicValue::F32(value)) = property(&world, entity, opacity).1
        else {
            panic!("opacity row property");
        };
        assert!((0.0..=1.0).contains(&value), "{value}");
    }
    assert!(failed);
}

#[test]
fn clip_and_controller_interchange_preserve_row_offset_targets() {
    let opacity = offset(7, OPACITY);
    let image = GuiRoot::node_data_offset(GuiNodeId(7), GuiNodeDataProperty::ImageSize).unwrap();
    let clip = AnimationClip::new(
        2.0,
        vec![
            track(opacity, DynamicValue::F32(0.0), DynamicValue::F32(1.0)),
            track(
                image,
                DynamicValue::Vec2([1.0, 2.0]),
                DynamicValue::Vec2([3.0, 4.0]),
            ),
        ],
    )
    .unwrap();

    let decoded = AnimationClip::decode(&clip.encode()).unwrap();
    assert_eq!(decoded.tracks()[0].target(), &target(opacity));
    assert_eq!(decoded.tracks()[1].target(), &target(image));
    assert_eq!(
        decoded.sample(0, 1.0),
        AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::F32(0.5)))
    );

    // Row absence and whole tables are never keys.
    for value in [SchemaValue::Unset, SchemaValue::Rows(style_table(&[2]))] {
        assert!(
            AnimationClip::new(
                1.0,
                vec![AnimationTrack {
                    target: target(opacity),
                    keys: vec![AnimationKeyframe {
                        time: 0.0,
                        value: AnimationValue::Field(value),
                        interpolation: AnimationInterpolation::Step,
                    }],
                }],
            )
            .is_err()
        );
    }
}
