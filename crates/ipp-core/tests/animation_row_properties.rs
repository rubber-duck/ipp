//! Ordinary animation of schema-row properties through the public World API,
//! with shared GuiTheme paint-part rows as the real consumer: binding by row
//! offset, sampling, per-driver invalidation, restoration and clip interchange.
//! Transitions cannot target these rows while GuiTheme declares no numeric row properties.

mod support;
use support::WorldTestDriver;

use ipp_core::components::rows::Rows;
use ipp_core::components::schema::FieldValue as SchemaValue;
use ipp_core::services::asset_management::{AssetUpload, AssetUploadIdentity};
use ipp_core::systems::animation::*;
use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiTheme};
use ipp_core::systems::gui::{GuiPartId, GuiPartProperty, GuiPrimitivePart};
use ipp_core::{
    Batch, Command, ComponentValue, DynamicValue, EntityId, EntityRef, ErrorReason, FieldValue,
    FieldWrite, HostRuntime, WorldContext, WorldId, WorldLimits, WorldUpdateReport,
};

const OPACITY: GuiPartProperty = GuiPartProperty::Opacity;

/// Background row slot: opacity and scale present, border width absent.
const BACKGROUND: u32 = 0;

/// Fill row slot: opacity, scale and an explicit border width.
const FILL: u32 = 1;

fn offset(slot: u32, property: GuiPartProperty) -> u32 {
    Rows::<GuiPaintPart>::offset(0, slot, property.index()).unwrap()
}

/// Exposed offset of the whole `parts` rows table.
fn table_offset() -> u32 {
    let rows: Vec<u32> = ComponentValue::GuiTheme(GuiTheme::default())
        .fields()
        .into_iter()
        .filter(|(_, value)| matches!(value, SchemaValue::Rows(_)))
        .map(|(offset, _)| offset)
        .collect();
    assert_eq!(rows.len(), 1);
    rows[0]
}

fn row(part: GuiPrimitivePart, border_width: Option<f32>) -> GuiPaintPart {
    GuiPaintPart {
        opacity: Some(1.0),
        scale: Some([1.0, 1.0]),
        border_width,
        ..GuiPaintPart::keyed(GuiPartId::base(part)).unwrap()
    }
}

/// Theme rows for the given slots, as the fixture inserts them.
fn parts(slots: &[u32]) -> Rows<GuiPaintPart> {
    let mut parts = Rows::new();
    for &slot in slots {
        let value = match slot {
            BACKGROUND => row(GuiPrimitivePart::Background, None),
            FILL => row(GuiPrimitivePart::Fill, Some(1.0)),
            _ => unreachable!("fixture slot {slot}"),
        };
        parts.insert(slot, value).unwrap();
    }
    parts
}

/// A shared theme with a background row and a fill row.
fn theme_entity(world: &mut WorldContext<'_>) -> EntityId {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::GuiTheme(GuiTheme {
                        parts: parts(&[BACKGROUND, FILL]),
                        ..Default::default()
                    }),
                ),
            ],
        })
        .unwrap();
    let report = update(world, 0.0);
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

fn set_field(
    world: &mut WorldContext<'_>,
    entity: EntityId,
    field: FieldWrite,
) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_THEME,
                field,
            }],
        })
        .unwrap();
    let report = update(world, 0.0);
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    report
}

fn update(world: &mut WorldContext<'_>, dt: f64) -> WorldUpdateReport {
    world.update_for_test(dt).unwrap()
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
        component: ComponentValue::GUI_THEME,
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
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
}

fn description(entity: EntityId, asset: u64, targets: &[u32]) -> AnimationControllerDescription {
    AnimationControllerDescription {
        drivers: targets
            .iter()
            .enumerate()
            .map(|(track, &offset)| AnimationDriverDescription {
                source: std::sync::Arc::<str>::from(format!("asset://10/{asset}")),
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
    let snapshot = world.inspect(entity).unwrap();
    let read = |values: &[ComponentValue]| {
        values
            .iter()
            .find(|value| value.type_id() == ComponentValue::GUI_THEME)
            .unwrap()
            .field(offset)
            .unwrap()
    };
    read(&snapshot.components)
}

fn effective_f32(world: &WorldContext<'_>, entity: EntityId, offset: u32) -> f32 {
    let SchemaValue::Dynamic(DynamicValue::F32(value)) = property(world, entity, offset) else {
        panic!("F32 row property");
    };
    value
}

fn f32_value(value: f32) -> SchemaValue {
    SchemaValue::Dynamic(DynamicValue::F32(value))
}

fn vec2_value(value: [f32; 2]) -> SchemaValue {
    SchemaValue::Dynamic(DynamicValue::Vec2(value))
}

fn world() -> (HostRuntime, WorldId) {
    let mut host = crate::support::task_scheduler::host();
    // Theme rows are asset-backed GUI state; no Canvas is needed to animate them.
    let id = host
        .create_world(
            WorldLimits::default(),
            &[
                ipp_core::systems::animation::AnimationSystem::ID,
                ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
                ipp_core::systems::gui::GuiSystem::ID,
            ],
        )
        .unwrap();
    (host, id)
}

fn has_event(
    report: &WorldUpdateReport,
    id: AnimationControllerId,
    kind: AnimationPlaybackEventKind,
) -> bool {
    report
        .playback_events
        .iter()
        .any(|event| event.controller.id == id && event.kind == kind)
}

#[test]
fn row_targets_bind_by_offset_and_add_the_clip_change_to_the_base() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = theme_entity(&mut world);
    let opacity = offset(BACKGROUND, OPACITY);
    let scale = offset(FILL, GuiPartProperty::Scale);
    let clip = AnimationClip::new(
        2.0,
        vec![
            track(opacity, DynamicValue::F32(0.0), DynamicValue::F32(-1.0)),
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

    // Halfway: 1 + (-1 - 0) / 2 and [1 + 2 / 2, 1 + 4 / 2].
    assert_eq!(property(&world, entity, opacity), f32_value(0.5));
    assert_eq!(property(&world, entity, scale), vec2_value([2.0, 3.0]));
}

#[test]
fn row_targets_reject_absent_dead_and_whole_table_offsets_and_out_of_range_values() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = theme_entity(&mut world);
    let absent = offset(BACKGROUND, GuiPartProperty::BorderWidth);
    let dead = offset(4, OPACITY);
    let table = table_offset();
    upload(&mut world, 1, &clip(&[absent], 0.0, 1.0));

    for rejected in [absent, dead, table] {
        assert_eq!(
            world.create_animation_controller(description(entity, 1, &[rejected])),
            Err(ErrorReason::InvalidField),
            "offset {rejected:#x}"
        );
    }

    // Opacity is F32 in 0..=1; the clip leaves that range after one second.
    let opacity = offset(BACKGROUND, OPACITY);
    upload(&mut world, 3, &clip(&[opacity], 0.0, -2.0));
    let controller = world
        .create_animation_controller(description(entity, 3, &[opacity]))
        .unwrap();
    seek(&mut world, controller, 0.5);
    assert_eq!(property(&world, entity, opacity), f32_value(0.5));

    let report = seek(&mut world, controller, 1.5);
    assert!(
        has_event(&report, controller, AnimationPlaybackEventKind::Failed),
        "{:?}",
        report.playback_events
    );
    assert_eq!(property(&world, entity, opacity), f32_value(0.5));
    assert!((0.0..=1.0).contains(&effective_f32(&world, entity, opacity)));
}

#[test]
fn an_arc_start_animates_past_a_whole_turn() {
    // Arc angles take any finite value, so a clip turning a spinner may carry the
    // start beyond one turn; paint takes it modulo a turn.
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = theme_entity(&mut world);
    let mut table = Rows::new();
    table
        .insert(
            BACKGROUND,
            GuiPaintPart {
                shape: Some(2.0),
                arc_start: Some(0.25),
                ..row(GuiPrimitivePart::Background, None)
            },
        )
        .unwrap();
    set_field(
        &mut world,
        entity,
        FieldWrite {
            offset: table_offset(),
            value: FieldValue::Rows(table.encode()),
        },
    );
    let start = offset(BACKGROUND, GuiPartProperty::ArcStart);
    upload(&mut world, 1, &clip(&[start], 0.0, 1.5));
    let controller = world
        .create_animation_controller(description(entity, 1, &[start]))
        .unwrap();

    // Halfway: 0.25 + 1.5 / 2, then 0.25 + 1.5 * 0.95.
    seek(&mut world, controller, 1.0);
    assert_eq!(property(&world, entity, start), f32_value(1.0));
    let report = seek(&mut world, controller, 1.9);
    assert!(
        !has_event(&report, controller, AnimationPlaybackEventKind::Failed),
        "{:?}",
        report.playback_events
    );
    assert!((effective_f32(&world, entity, start) - 1.675).abs() < 1e-6);
}

#[test]
fn a_fill_hue_animates_past_a_whole_turn_beside_its_checker_cell() {
    // The saturation-value fill's hue takes any finite value, so a clip may turn it
    // through red; its checker's cell side animates as an ordinary length.
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = theme_entity(&mut world);
    let mut table = Rows::new();
    table
        .insert(
            BACKGROUND,
            GuiPaintPart {
                fill_mode: Some(4.0),
                fill_hue: Some(0.5),
                checker_size: Some(4.0),
                ..row(GuiPrimitivePart::Background, None)
            },
        )
        .unwrap();
    set_field(
        &mut world,
        entity,
        FieldWrite {
            offset: table_offset(),
            value: FieldValue::Rows(table.encode()),
        },
    );
    let hue = offset(BACKGROUND, GuiPartProperty::FillHue);
    let cell = offset(BACKGROUND, GuiPartProperty::CheckerSize);
    upload(&mut world, 1, &clip(&[hue, cell], 0.0, 1.5));
    let controller = world
        .create_animation_controller(description(entity, 1, &[hue, cell]))
        .unwrap();

    // Halfway: each base plus 1.5 / 2, then plus 1.5 * 0.95.
    seek(&mut world, controller, 1.0);
    assert_eq!(property(&world, entity, hue), f32_value(1.25));
    assert_eq!(property(&world, entity, cell), f32_value(4.75));
    let report = seek(&mut world, controller, 1.9);
    assert!(
        !has_event(&report, controller, AnimationPlaybackEventKind::Failed),
        "{:?}",
        report.playback_events
    );
    assert!((effective_f32(&world, entity, hue) - 1.925).abs() < 1e-6);
    assert!((effective_f32(&world, entity, cell) - 5.425).abs() < 1e-6);
}

#[test]
fn removing_a_row_or_clearing_its_property_drops_only_its_drivers() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = theme_entity(&mut world);
    let removed = offset(BACKGROUND, OPACITY);
    let cleared = offset(FILL, GuiPartProperty::BorderWidth);
    let survivor = offset(FILL, OPACITY);
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

    // Replacing the table without the background row removes that row; the
    // fill row keeps its slot and its drivers survive.
    let report = set_field(
        &mut world,
        entity,
        FieldWrite {
            offset: table_offset(),
            value: FieldValue::Rows(parts(&[FILL]).encode()),
        },
    );
    assert!(has_event(
        &report,
        controller,
        AnimationPlaybackEventKind::Invalidated
    ));
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.description.drivers.len(), 2);

    // Clearing the optional border width drops that driver; opacity keeps sampling.
    let report = set_field(
        &mut world,
        entity,
        FieldWrite {
            offset: cleared,
            value: FieldValue::Unset,
        },
    );
    assert!(has_event(
        &report,
        controller,
        AnimationPlaybackEventKind::Invalidated
    ));
    let snapshot = world.animation_controller(controller).unwrap();
    assert_eq!(snapshot.description.drivers.len(), 1);
    assert_eq!(snapshot.description.drivers[0].property, target(survivor));

    // The table write reset the fill opacity to 1 with the contribution -0.5
    // still applied; moving to -0.75 subtracts the change of -0.25.
    seek(&mut world, controller, 1.5);
    assert_eq!(property(&world, entity, cleared), SchemaValue::Unset);
    assert_eq!(property(&world, entity, survivor), f32_value(0.75));
}

#[test]
fn removing_a_controller_subtracts_its_row_contributions() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = theme_entity(&mut world);
    let opacity = offset(BACKGROUND, OPACITY);
    let border = offset(FILL, GuiPartProperty::BorderWidth);
    upload(&mut world, 1, &clip(&[opacity, border], 0.0, -0.5));
    let controller = world
        .create_animation_controller(description(entity, 1, &[opacity, border]))
        .unwrap();
    seek(&mut world, controller, 2.0);
    assert_eq!(property(&world, entity, opacity), f32_value(0.5));
    assert_eq!(property(&world, entity, border), f32_value(0.5));

    world.remove_animation_controller(controller).unwrap();
    update(&mut world, 0.0);
    assert_eq!(property(&world, entity, opacity), f32_value(1.0));
    assert_eq!(property(&world, entity, border), f32_value(1.0));
}

#[test]
fn clip_interchange_preserves_row_offset_targets_and_rejects_row_keys() {
    let opacity = offset(7, OPACITY);
    let corner = offset(7, GuiPartProperty::CornerRadius);
    let clip = AnimationClip::new(
        2.0,
        vec![
            track(opacity, DynamicValue::F32(0.0), DynamicValue::F32(1.0)),
            track(
                corner,
                DynamicValue::Vec2([1.0, 2.0]),
                DynamicValue::Vec2([3.0, 4.0]),
            ),
        ],
    )
    .unwrap();

    let decoded = AnimationClip::decode(&clip.encode()).unwrap();
    assert_eq!(decoded.tracks()[0].target(), &target(opacity));
    assert_eq!(decoded.tracks()[1].target(), &target(corner));
    assert_eq!(
        decoded.sample(0, 1.0),
        AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::F32(0.5)))
    );
    assert_eq!(
        decoded.sample(1, 1.0),
        AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::Vec2([2.0, 3.0])))
    );

    // Row absence and whole tables are never keys.
    for value in [
        SchemaValue::Unset,
        SchemaValue::Rows(parts(&[BACKGROUND]).encode()),
    ] {
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

#[test]
fn controller_state_interchange_preserves_row_offset_targets() {
    let (mut host, id) = world();
    let mut world = host.world_mut(id).unwrap();
    let entity = theme_entity(&mut world);
    let opacity = offset(BACKGROUND, OPACITY);
    let scale = offset(FILL, GuiPartProperty::Scale);
    let clip = AnimationClip::new(
        2.0,
        vec![
            track(opacity, DynamicValue::F32(0.0), DynamicValue::F32(-1.0)),
            track(
                scale,
                DynamicValue::Vec2([1.0, 2.0]),
                DynamicValue::Vec2([3.0, 4.0]),
            ),
        ],
    )
    .unwrap();
    upload(&mut world, 1, &clip);
    let controller = world
        .create_animation_controller(description(entity, 1, &[opacity, scale]))
        .unwrap();
    seek(&mut world, controller, 1.0);

    let persistent = world.animation_persistent_state();
    assert_eq!(persistent.controllers.len(), 1);
    let drivers = &persistent.controllers[0].description.drivers;
    assert_eq!(drivers[0].property, target(opacity));
    assert_eq!(drivers[1].property, target(scale));

    // Restoring the exported state rebinds the same rows at the same time and
    // keeps the contributions already in them.
    world.restore_animation_controllers(persistent).unwrap();
    update(&mut world, 0.0);
    let restored = world.animation_controller(controller).unwrap();
    assert_eq!(restored.time, 1.0);
    assert_eq!(restored.description.drivers[0].property, target(opacity));
    assert_eq!(property(&world, entity, opacity), f32_value(0.5));
    assert_eq!(property(&world, entity, scale), vec2_value([2.0, 2.0]));
}
