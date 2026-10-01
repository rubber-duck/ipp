//! Animation of schema-row properties, with the test-only rows fixture as the
//! consumer: binding by offset, compiled sampling, per-driver invalidation,
//! write-back of kept originals, transitions and clip interchange.

use crate::components::rows::Rows;
use crate::components::schema::FieldValue as SchemaValue;
use crate::components::{RowsFixture, RowsFixtureItem, RowsFixtureTag};
use crate::services::asset_management::{AssetUpload, AssetUploadIdentity};
use crate::systems::animation::*;
use crate::{
    Batch, BatchOutcome, Command, ComponentValue, DynamicValue, EntityId, EntityRef, ErrorReason,
    FieldValue, FieldWrite, HostRuntime, WorldContext, WorldLimits, WorldUpdateReport,
};
use std::mem::offset_of;

const WEIGHT: u32 = 0;
const SIZE: u32 = 7;
const MARK: u32 = RowsFixture::MARK;

/// Offset of one item row property.
fn offset(slot: u32, property: u32) -> u32 {
    Rows::<RowsFixtureItem>::offset(0, slot, property).unwrap()
}

/// Offset of the value of one tag row.
fn tag(slot: u32) -> u32 {
    Rows::<RowsFixtureTag>::offset(1, slot, 0).unwrap()
}

/// Slots 1, 2 and 3 hold weight 1; only slot 3 has a `mark` of 1 and a
/// `size` of `[1, 1]`. Tag slot 0 holds a value.
fn items() -> Rows<RowsFixtureItem> {
    let mut items = Rows::new();
    for slot in 1..=3 {
        items
            .insert(
                slot,
                RowsFixtureItem {
                    weight: 1.0,
                    mark: (slot == 3).then_some(1.0),
                    size: (slot == 3).then_some([1.0, 1.0]),
                    ..RowsFixtureItem::default()
                },
            )
            .unwrap();
    }
    items
}

fn rows_entity(world: &mut WorldContext<'_>) -> EntityId {
    let mut tags = Rows::new();
    tags.insert(
        0,
        RowsFixtureTag {
            value: 4,
        },
    )
    .unwrap();
    submit(
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::RowsFixture(RowsFixture {
                    items: items(),
                    tags,
                    ..RowsFixture::default()
                }),
            ),
        ],
    )
    .result
    .unwrap()[0]
        .1
}

/// Remove item slot 2 by writing the stored table without it.
fn remove_slot_two(world: &mut WorldContext<'_>, entity: EntityId) -> WorldUpdateReport {
    let Some(ComponentValue::RowsFixture(mut fixture)) = world
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find(|value| value.type_id() == ComponentValue::ROWS_FIXTURE)
    else {
        panic!("rows fixture");
    };
    fixture.items.remove(2);
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::ROWS_FIXTURE,
                field: FieldWrite {
                    offset: offset_of!(RowsFixture, items) as u32,
                    value: FieldValue::Rows(fixture.items.encode()),
                },
            }],
        })
        .unwrap();
    let report = update(world, 0.0);
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    report
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
        component: ComponentValue::ROWS_FIXTURE,
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
                source: format!("asset://10/{asset}").into(),
                variant: 0,
                track: track as u32,
                target: entity,
                property: target(offset),
                entity_bindings: Vec::new(),
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

/// Stored value of one row property.
fn property(world: &WorldContext<'_>, entity: EntityId, offset: u32) -> SchemaValue {
    world
        .inspect(entity)
        .unwrap()
        .components
        .iter()
        .find(|value| value.type_id() == ComponentValue::ROWS_FIXTURE)
        .unwrap()
        .field(offset)
        .unwrap()
}

fn f32_value(value: f32) -> SchemaValue {
    SchemaValue::Dynamic(DynamicValue::F32(value))
}

fn world() -> (HostRuntime, crate::WorldId) {
    let mut host = HostRuntime::new();
    let id = host
        .create_world(
            WorldLimits::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
            ],
        )
        .unwrap();
    (host, id)
}

fn invalidated(report: &WorldUpdateReport, id: AnimationControllerId) -> bool {
    report.playback_events.iter().any(|event| {
        event.controller.id == id && event.kind == AnimationPlaybackEventKind::Invalidated
    })
}

#[test]
fn row_targets_bind_by_offset_and_receive_contributions() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = rows_entity(&mut world);
    let weight = offset(2, WEIGHT);
    let size = offset(3, SIZE);
    let clip = AnimationClip::new(
        2.0,
        vec![
            track(weight, DynamicValue::F32(0.0), DynamicValue::F32(1.0)),
            track(
                size,
                DynamicValue::Vec2([1.0, 1.0]),
                DynamicValue::Vec2([3.0, 5.0]),
            ),
        ],
    )
    .unwrap();
    upload(&mut world, 1, &clip);
    let controller = world
        .create_animation_controller(description(entity, 1, &[weight, size]))
        .unwrap();
    seek(&mut world, controller, 1.0);

    // Each row property holds its base plus the clip's change from its first key.
    assert_eq!(property(&world, entity, weight), f32_value(1.5));
    assert_eq!(
        property(&world, entity, size),
        SchemaValue::Dynamic(DynamicValue::Vec2([2.0, 3.0]))
    );
}

#[test]
fn row_targets_reject_absent_writer_owned_table_and_out_of_range_values() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = rows_entity(&mut world);
    let absent = offset(2, MARK);
    let dead = offset(4, WEIGHT);
    let table = offset_of!(RowsFixture, items) as u32;
    upload(&mut world, 1, &clip(&[absent], 0.0, 1.0));

    for rejected in [absent, dead, table] {
        assert_eq!(
            world.create_animation_controller(description(entity, 1, &[rejected])),
            Err(ErrorReason::InvalidField),
            "offset {rejected:#x}"
        );
    }

    // Discrete tracks take the general path; binding still rejects the table
    // the component keeps from animation.
    let owned = tag(0);
    let toggle = AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: target(owned),
            keys: vec![AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::U32(7))),
                interpolation: AnimationInterpolation::Step,
            }],
        }],
    )
    .unwrap();
    upload(&mut world, 2, &toggle);
    assert_eq!(
        world.create_animation_controller(description(entity, 2, &[owned])),
        Err(ErrorReason::InvalidField),
        "offset {owned:#x} is never an animation target"
    );

    let mark = offset(3, MARK);
    upload(&mut world, 3, &clip(&[mark], 0.0, -2.0));
    let controller = world
        .create_animation_controller(description(entity, 3, &[mark]))
        .unwrap();
    seek(&mut world, controller, 0.5);
    assert_eq!(property(&world, entity, mark), f32_value(0.5));
    let report = seek(&mut world, controller, 1.5);
    assert!(
        report
            .playback_events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed),
        "{:?}",
        report.playback_events
    );
    assert_eq!(property(&world, entity, mark), f32_value(0.5));
}

#[test]
fn removing_a_row_or_clearing_its_property_drops_only_its_drivers() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = rows_entity(&mut world);
    let removed = offset(2, WEIGHT);
    let cleared = offset(3, MARK);
    let survivor = offset(3, WEIGHT);
    upload(
        &mut world,
        1,
        &clip(&[removed, cleared, survivor], 0.0, -1.0),
    );
    let controller = world
        .create_animation_controller(description(entity, 1, &[removed, cleared, survivor]))
        .unwrap();
    seek(&mut world, controller, 1.0);
    assert_eq!(property(&world, entity, cleared), f32_value(0.5));

    // Removing slot 2 drops its driver; slot 3's drivers survive.
    let report = remove_slot_two(&mut world, entity);
    assert!(invalidated(&report, controller));
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.description.drivers.len(), 2);

    // Clearing the optional mark drops that driver; weight keeps sampling.
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::ROWS_FIXTURE,
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
    assert_eq!(property(&world, entity, cleared), SchemaValue::Unset);
    assert_eq!(property(&world, entity, survivor), f32_value(0.75));
}

#[test]
fn removing_a_controller_subtracts_its_row_contributions() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = rows_entity(&mut world);
    let weight = offset(2, WEIGHT);
    let mark = offset(3, MARK);
    upload(&mut world, 1, &clip(&[weight, mark], 0.0, -0.5));
    let controller = world
        .create_animation_controller(description(entity, 1, &[weight, mark]))
        .unwrap();
    seek(&mut world, controller, 2.0);
    assert_eq!(property(&world, entity, weight), f32_value(0.5));
    assert_eq!(property(&world, entity, mark), f32_value(0.5));

    world.remove_animation_controller(controller).unwrap();
    update(&mut world, 0.0);
    assert_eq!(property(&world, entity, weight), f32_value(1.0));
    assert_eq!(property(&world, entity, mark), f32_value(1.0));
}

#[test]
fn transitions_blend_row_targets_and_reject_out_of_range_channels() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = rows_entity(&mut world);
    let mark = offset(3, MARK);
    // Reach a constant contribution after one second and hold it to the end.
    let hold = |change: f32| {
        AnimationClip::new(
            4.0,
            vec![AnimationTrack {
                target: target(mark),
                keys: vec![
                    key(0.0, DynamicValue::F32(0.0)),
                    AnimationKeyframe {
                        interpolation: AnimationInterpolation::Step,
                        ..key(1.0, DynamicValue::F32(change))
                    },
                ],
            }],
        )
        .unwrap()
    };
    upload(&mut world, 1, &hold(-0.8));
    upload(&mut world, 2, &hold(-0.2));
    let controller = world
        .create_animation_controller(description(entity, 1, &[mark]))
        .unwrap();
    seek(&mut world, controller, 2.0);
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    update(&mut world, 0.0);
    let SchemaValue::Dynamic(DynamicValue::F32(held)) = property(&world, entity, mark) else {
        panic!("mark row property");
    };
    assert!((held - 0.2).abs() < 1.0e-5, "{held}");

    let transition = |description| AnimationControllerTransition {
        description,
        duration: 1.0,
        easing: AnimationTransitionEasing::Linear,
        start_time: AnimationTransitionStartTime::Seek(2.0),
    };
    world
        .transition_animation_controller(controller, transition(description(entity, 2, &[mark])))
        .unwrap();
    update(&mut world, 0.0);
    update(&mut world, 0.5);
    let SchemaValue::Dynamic(DynamicValue::F32(blended)) = property(&world, entity, mark) else {
        panic!("mark row property");
    };
    assert!((blended - 0.5).abs() < 1.0e-5, "{blended}");
    update(&mut world, 0.5);
    let SchemaValue::Dynamic(DynamicValue::F32(settled)) = property(&world, entity, mark) else {
        panic!("mark row property");
    };
    assert!((settled - 0.8).abs() < 1.0e-5, "{settled}");

    // A channel value outside the property's range fails the controller
    // before any output publishes.
    upload(&mut world, 3, &hold(0.5));
    world
        .transition_animation_controller(controller, transition(description(entity, 3, &[mark])))
        .unwrap();
    let mut failed = false;
    for _ in 0..4 {
        let report = update(&mut world, 0.5);
        failed |= report
            .playback_events
            .iter()
            .any(|event| event.kind == AnimationPlaybackEventKind::Failed);
        let SchemaValue::Dynamic(DynamicValue::F32(value)) = property(&world, entity, mark) else {
            panic!("mark row property");
        };
        assert!((0.0..=1.0).contains(&value), "{value}");
    }
    assert!(failed);
}

#[test]
fn clip_and_controller_interchange_preserve_row_offset_targets() {
    let weight = offset(7, WEIGHT);
    let size = offset(7, SIZE);
    let clip = AnimationClip::new(
        2.0,
        vec![
            track(weight, DynamicValue::F32(0.0), DynamicValue::F32(1.0)),
            track(
                size,
                DynamicValue::Vec2([1.0, 2.0]),
                DynamicValue::Vec2([3.0, 4.0]),
            ),
        ],
    )
    .unwrap();

    let decoded = AnimationClip::decode(&clip.encode()).unwrap();
    assert_eq!(decoded.tracks()[0].target(), &target(weight));
    assert_eq!(decoded.tracks()[1].target(), &target(size));
    assert_eq!(
        decoded.sample(0, 1.0),
        AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::F32(0.5)))
    );

    // Row absence and whole tables are never keys.
    for value in [SchemaValue::Unset, SchemaValue::Rows(items().encode())] {
        assert!(
            AnimationClip::new(
                1.0,
                vec![AnimationTrack {
                    target: target(weight),
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
