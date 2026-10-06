//! Rows field writes, validation and animation through Host World command batches.

use super::*;
use crate::world::WorldLimits;
use crate::{
    Batch, BatchOutcome, Command, EntityId, EntityMetadata, EntityRef, HostRuntime, WorldId,
};

fn run(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    let mut world = host.world_mut(world).unwrap();
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.step(0.0).unwrap().outcomes.remove(0)
}

fn set(entity: EntityId, offset: u32, value: crate::FieldValue) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::ROWS_FIXTURE,
        field: crate::FieldWrite {
            offset,
            value,
        },
    }
}

fn state(host: &mut HostRuntime, world: WorldId, entity: EntityId) -> RowsFixture {
    host.world_mut(world)
        .unwrap()
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::RowsFixture(value) => Some(value),
            _ => None,
        })
        .unwrap()
}

#[test]
fn private_row_assignments_validate_only_the_final_table() {
    use crate::components::Scalar;
    use crate::systems::gui::motion::{GuiMotionPart, GuiThemeMotion};

    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::constraints::ConstraintSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
                crate::systems::gui::GuiSystem::ID,
            ],
        )
        .unwrap();
    let table = |easing: u32| {
        let mut parts = Rows::new();
        parts
            .push(GuiMotionPart {
                duration: Some(0.1),
                easing: Some(easing),
                ..Default::default()
            })
            .unwrap();
        parts
    };
    let invalid = table(7);
    let valid = table(2);
    let field = |parts: &Rows<GuiMotionPart>| crate::FieldWrite {
        offset: offset_of!(GuiThemeMotion, parts) as u32,
        value: crate::FieldValue::Rows(parts.encode()),
    };
    let created = run(
        &mut host,
        world,
        vec![Command::Create {
            alias: 0,
            metadata: Default::default(),
            adopt: false,
        }],
    );
    let entity = created.result.unwrap()[0].1;
    let insert = |fields| Command::InsertComponent {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_THEME_MOTION,
        fields,
        adopt: false,
    };
    let accepted = run(
        &mut host,
        world,
        vec![insert(vec![field(&invalid), field(&valid)])],
    );
    assert!(accepted.result.is_ok(), "{accepted:?}");
    let expected = ComponentValue::GuiThemeMotion(GuiThemeMotion {
        parts: valid.clone(),
    });
    assert!(
        host.world_mut(world)
            .unwrap()
            .inspect(entity)
            .unwrap()
            .components
            .contains(&expected)
    );

    let rejected = run(
        &mut host,
        world,
        vec![
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::Scalar(Scalar {
                    value: 7.0,
                }),
            ),
            insert(vec![field(&valid), field(&invalid)]),
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::Scalar(Scalar {
                    value: 99.0,
                }),
            ),
        ],
    );
    assert_eq!(rejected.result.unwrap_err().operation, Some(1));
    let snapshot = host.world_mut(world).unwrap().inspect(entity).unwrap();
    assert!(snapshot.components.contains(&expected));
    assert!(
        snapshot
            .components
            .contains(&ComponentValue::Scalar(Scalar {
                value: 7.0
            }))
    );
    let write = |parts: &Rows<GuiMotionPart>| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_THEME_MOTION,
        field: field(parts),
    };
    let rejected = run(&mut host, world, vec![write(&invalid), write(&valid)]);
    assert_eq!(rejected.result.unwrap_err().operation, Some(0));
    assert!(
        host.world_mut(world)
            .unwrap()
            .inspect(entity)
            .unwrap()
            .components
            .contains(&expected)
    );
    assert!(run(&mut host, world, vec![write(&valid)]).result.is_ok());
}

#[test]
fn malformed_row_assets_reject_without_poisoning_demand_or_losing_prior_writes() {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
            ],
        )
        .unwrap();
    let mut rows = Rows::new();
    rows.insert(5, item(1.0)).unwrap();
    let created = run(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(0),
                ComponentValue::RowsFixture(RowsFixture {
                    items: rows,
                    ..RowsFixture::default()
                }),
            ),
        ],
    );
    let entity = created.result.unwrap()[0].1;
    let rejected = run(
        &mut host,
        world,
        vec![
            set(
                entity,
                item_offset(5, WEIGHT),
                crate::FieldValue::Dynamic(DynamicValue::F32(2.0)),
            ),
            set(
                entity,
                item_offset(5, TEXTURE),
                crate::FieldValue::Dynamic(DynamicValue::Asset(AssetSource {
                    kind: crate::TEXTURE_TYPE,
                    uri: "asset://ordinary-motion-A".into(),
                    variant: 0,
                })),
            ),
            set(
                entity,
                item_offset(5, WEIGHT),
                crate::FieldValue::Dynamic(DynamicValue::F32(3.0)),
            ),
        ],
    );
    assert!(rejected.result.is_err(), "{rejected:?}");
    assert_eq!(rejected.result.as_ref().unwrap_err().operation, Some(1));
    let stored = state(&mut host, world, entity);
    assert_eq!(stored.items.get(5).unwrap().weight, 2.0);
    assert_eq!(stored.items.get(5).unwrap().texture, None);
    assert!(
        host.world_mut(world)
            .unwrap()
            .resource_snapshots()
            .is_empty()
    );

    let corrected = run(
        &mut host,
        world,
        vec![set(
            entity,
            item_offset(5, TEXTURE),
            crate::FieldValue::Dynamic(DynamicValue::Asset(AssetSource {
                kind: crate::TEXTURE_TYPE,
                uri: "asset://2/42".into(),
                variant: 0,
            })),
        )],
    );
    assert!(corrected.result.is_ok(), "{corrected:?}");
    assert_eq!(host.world_mut(world).unwrap().resource_snapshots().len(), 1);

    let mut invalid = state(&mut host, world, entity);
    invalid
        .items
        .get_mut(5)
        .unwrap()
        .texture
        .as_mut()
        .unwrap()
        .uri = "asset://malformed".into();
    for command in [
        set(
            entity,
            offset_of!(RowsFixture, items) as u32,
            crate::FieldValue::Rows(invalid.items.encode()),
        ),
        Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::RowsFixture(invalid),
        ),
    ] {
        let rejected = run(&mut host, world, vec![command]);
        assert!(rejected.result.is_err(), "{rejected:?}");
        assert_eq!(
            &*state(&mut host, world, entity)
                .items
                .get(5)
                .unwrap()
                .texture
                .as_ref()
                .unwrap()
                .uri,
            "asset://2/42"
        );
        assert_eq!(host.world_mut(world).unwrap().resource_snapshots().len(), 1);
    }
}

#[test]
fn world_writes_address_live_row_properties_by_offset() {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default(), &[]).unwrap();
    let mut table = Rows::<RowsFixtureItem>::new();
    table.push(item(1.0)).unwrap();
    table.push(item(2.0)).unwrap();
    table.remove(0);

    let outcome = run(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata {
                    symbolic_id: Some("rows".into()),
                    classes: vec![],
                },
                adopt: false,
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::ROWS_FIXTURE,
                fields: vec![crate::FieldWrite {
                    offset: offset_of!(RowsFixture, items) as u32,
                    value: crate::FieldValue::Rows(table.encode()),
                }],
                adopt: false,
            },
        ],
    );
    let entity = outcome.result.unwrap()[0].1;

    let outcome = run(
        &mut host,
        world,
        vec![
            set(
                entity,
                item_offset(1, WEIGHT),
                crate::FieldValue::Dynamic(DynamicValue::F32(6.0)),
            ),
            set(entity, item_offset(1, OFFSET), crate::FieldValue::Unset),
        ],
    );
    assert!(outcome.result.is_ok());
    let stored = state(&mut host, world, entity);
    assert_eq!(stored.items.get(1).unwrap().weight, 6.0);
    assert_eq!(stored.items.get(1).unwrap().offset, None);

    for rejected in [
        set(
            entity,
            item_offset(0, WEIGHT),
            crate::FieldValue::Dynamic(DynamicValue::F32(1.0)),
        ),
        set(entity, item_offset(1, WEIGHT), crate::FieldValue::Unset),
        set(
            entity,
            item_offset(1, COUNT),
            crate::FieldValue::Dynamic(DynamicValue::F32(1.0)),
        ),
    ] {
        assert!(run(&mut host, world, vec![rejected]).result.is_err());
    }

    // A symbolic reference addresses the same row property by offset.
    let outcome = run(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Symbol("rows".into()),
            component: ComponentValue::ROWS_FIXTURE,
            field: crate::FieldWrite {
                offset: item_offset(1, ROTATION),
                value: crate::FieldValue::Dynamic(DynamicValue::Vec4([0.0, 1.0, 0.0, 0.0])),
            },
        }],
    );
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    let stored = state(&mut host, world, entity);
    assert_eq!(
        stored.items.get(1).unwrap().rotation,
        Some([0.0, 1.0, 0.0, 0.0])
    );
    assert_eq!(stored.items.get(1).unwrap().weight, 6.0);
}

#[test]
fn text_rows_are_written_bounded_and_never_animated() {
    use crate::ErrorReason;
    use crate::systems::animation::{
        AnimationControllerDescription, AnimationDriverDescription, AnimationProperty,
        AnimationTrackTarget,
    };

    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
            ],
        )
        .unwrap();
    let mut table = Rows::<RowsFixtureItem>::new();
    table.push(item(1.0)).unwrap();
    let outcome = run(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata {
                    symbolic_id: Some("text".into()),
                    classes: vec![],
                },
                adopt: false,
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::ROWS_FIXTURE,
                fields: vec![crate::FieldWrite {
                    offset: offset_of!(RowsFixture, items) as u32,
                    value: crate::FieldValue::Rows(table.encode()),
                }],
                adopt: false,
            },
        ],
    );
    let entity = outcome.result.unwrap()[0].1;
    let label = item_offset(0, LABEL);
    let text = |value: &str| crate::FieldValue::String(value.into());

    let outcome = run(&mut host, world, vec![set(entity, label, text("base"))]);
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    assert_eq!(
        state(&mut host, world, entity).items.get(0).unwrap().label,
        Some("base".into())
    );

    // Over-long text is an invalid value; text in a dynamic payload or at a
    // numeric property is an invalid field.
    for (write, reason) in [
        (text(&"x".repeat(17)), ErrorReason::InvalidValue),
        (
            crate::FieldValue::Dynamic(DynamicValue::Text("dynamic".into())),
            ErrorReason::InvalidField,
        ),
    ] {
        let outcome = run(&mut host, world, vec![set(entity, label, write)]);
        assert_eq!(outcome.result.unwrap_err().reason, reason);
    }
    let outcome = run(
        &mut host,
        world,
        vec![set(entity, item_offset(0, WEIGHT), text("1"))],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::InvalidField
    );

    // A rejected write leaves the stored text unchanged.
    assert_eq!(
        state(&mut host, world, entity).items.get(0).unwrap().label,
        Some("base".into())
    );

    // Animation binds numeric row properties but rejects the text property.
    let description = |offset| AnimationControllerDescription {
        drivers: vec![AnimationDriverDescription {
            source: "asset://10/1".into(),
            variant: 0,
            track: 0,
            target: entity,
            property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::ROWS_FIXTURE,
                offsets: vec![offset],
            }),
            entity_bindings: Vec::new(),
            weight: 1.0,
            additive: false,
            reference_time: 0.0,
            repeat: false,
        }],
        ..Default::default()
    };
    let mut world = host.world_mut(world).unwrap();
    assert!(
        world
            .create_animation_controller(description(item_offset(0, WEIGHT)))
            .is_ok()
    );
    assert_eq!(
        world.create_animation_controller(description(label)).err(),
        Some(ErrorReason::InvalidField)
    );
}
