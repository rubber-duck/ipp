//! Cost and observable contracts of ordered writes to one staged component.

use crate::components::CustomMaterial;
use crate::components::dynamic_properties::clone_count;
use crate::systems::lifecycle_publisher::{
    ComponentLifecycleKind, LifecycleFilter, LifecycleObservation, LifecyclePublisherCommand,
    LifecyclePublisherOutput, LifecyclePublisherSystem,
};
use crate::world::*;
use crate::{DynamicProperties, DynamicValue, HostRuntime, WorldId};
use std::mem::offset_of;

/// Rendered meshes and materials with the evaluators they require.
const RENDER_SYSTEMS: &[crate::systems::SystemId] = &[
    crate::systems::animation::AnimationSystem::ID,
    crate::systems::asset_dependencies::AssetDependencySystem::ID,
    crate::systems::hierarchy::HierarchySystem::ID,
    crate::systems::look_at::LookAtSystem::ID,
    crate::systems::hierarchy::FinalPropagationSystem::ID,
    crate::systems::geometry::GeometrySystem::ID,
    crate::systems::render::RenderSystem::ID,
];

const SESSION: u64 = 7;

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

fn set(entity: EntityId, name: &str, value: f32) -> Command {
    Command::SetDynamicProperty {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::CUSTOM_MATERIAL,
        name: name.into(),
        value: DynamicValue::F32(value),
    }
}

fn remove(entity: EntityId, name: &str) -> Command {
    Command::RemoveDynamicProperty {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::CUSTOM_MATERIAL,
        name: name.into(),
    }
}

fn alpha_cutoff(entity: EntityId, value: FieldValue) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::CUSTOM_MATERIAL,
        field: FieldWrite {
            offset: offset_of!(CustomMaterial, alpha_cutoff) as u32,
            value,
        },
    }
}

/// A World holding one committed material with a `seed` property.
fn material_world() -> (HostRuntime, WorldId, EntityId) {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[
                RENDER_SYSTEMS,
                &[crate::systems::lifecycle_publisher::LifecyclePublisherSystem::ID],
            ]
            .concat(),
        )
        .unwrap();
    let outcome = run(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata {
                    symbolic_id: Some("material".into()),
                    classes: vec![],
                },
                adopt: false,
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                fields: vec![],
                adopt: false,
            },
        ],
    );
    let entity = outcome.result.unwrap()[0].1;
    assert!(
        run(&mut host, world, vec![set(entity, "seed", 1.0)])
            .result
            .is_ok()
    );
    (host, world, entity)
}

fn material(host: &mut HostRuntime, world: WorldId, entity: EntityId) -> CustomMaterial {
    host.world_mut(world)
        .unwrap()
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::CustomMaterial(value) => Some(value),
            _ => None,
        })
        .unwrap()
}

fn properties(host: &mut HostRuntime, world: WorldId, entity: EntityId) -> DynamicProperties {
    material(host, world, entity).properties
}

/// Write `alpha_cutoff` only if it still holds `expected`.
fn alpha_cutoff_if(entity: EntityId, expected: f32, value: f32) -> Command {
    Command::set_field_if(
        EntityRef::Handle(entity),
        ComponentValue::CUSTOM_MATERIAL,
        FieldWrite {
            offset: offset_of!(CustomMaterial, alpha_cutoff) as u32,
            value: FieldValue::F32(value),
        },
        FieldValue::F32(expected),
    )
}

#[test]
fn batched_property_writes_copy_the_component_independently_of_their_count() {
    let mut copies = Vec::new();
    for count in [16, 512] {
        let (mut host, world, entity) = material_world();
        let writes = (0..count)
            .map(|index| set(entity, &format!("lane_{index}"), index as f32))
            .collect();
        clone_count::take();
        assert!(run(&mut host, world, writes).result.is_ok());
        copies.push(clone_count::take());

        let stored = properties(&mut host, world, entity);
        assert_eq!(stored.descriptors().len(), count + 1);
        assert_eq!(
            stored.get(&format!("lane_{}", count - 1)),
            Some(DynamicValue::F32((count - 1) as f32))
        );
    }
    assert_eq!(
        copies[0], copies[1],
        "a batch copies its staged component a fixed number of times, not once per write"
    );
}

#[test]
fn later_operations_read_the_staged_writes_of_earlier_ones() {
    let (mut host, world, entity) = material_world();
    // A compare-and-set succeeds only if it observes the write staged just
    // before it in the same batch.
    let outcome = run(
        &mut host,
        world,
        vec![
            set(entity, "fresh", 1.0),
            alpha_cutoff(entity, FieldValue::F32(0.25)),
            alpha_cutoff_if(entity, 0.25, 0.75),
        ],
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
    let stored = material(&mut host, world, entity);
    assert_eq!(stored.alpha_cutoff, 0.75);
    assert_eq!(stored.properties.get("fresh"), Some(DynamicValue::F32(1.0)));
}

#[test]
fn a_failed_operation_stops_the_batch_and_keeps_applied_writes() {
    let (mut host, world, entity) = material_world();
    let outcome = run(
        &mut host,
        world,
        vec![
            set(entity, "a", 1.0),
            alpha_cutoff(entity, FieldValue::F32(0.25)),
            set(entity, "b", 2.0),
            alpha_cutoff(entity, FieldValue::String("wrong type".into())),
            set(entity, "c", 3.0),
        ],
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(3));
    let stored = material(&mut host, world, entity);
    assert_eq!(stored.properties.get("a"), Some(DynamicValue::F32(1.0)));
    assert_eq!(stored.properties.get("b"), Some(DynamicValue::F32(2.0)));
    assert_eq!(
        stored.properties.get("c"),
        None,
        "operations after the failure do not apply"
    );
    assert_eq!(
        stored.alpha_cutoff, 0.25,
        "the rejected field write leaves the earlier value"
    );

    // A later batch continues from the applied state.
    assert!(
        run(&mut host, world, vec![set(entity, "c", 3.0)])
            .result
            .is_ok()
    );
    assert_eq!(
        properties(&mut host, world, entity).get("c"),
        Some(DynamicValue::F32(3.0))
    );
}

#[test]
fn ordered_writes_publish_one_update_per_operation_that_changes_the_value() {
    let (mut host, world, entity) = material_world();
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command(
            LifecyclePublisherSystem::ID,
            SESSION,
            LifecyclePublisherCommand::Subscribe {
                subscription: 1,
                filter: LifecycleFilter {
                    entities: false,
                    assets: false,
                    entity: Some(entity),
                    component: Some(ComponentValue::CUSTOM_MATERIAL),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    let unchanged_cutoff = material(&mut host, world, entity).alpha_cutoff;

    let mut operations = vec![
        set(entity, "a", 1.0),
        set(entity, "a", 1.0),
        set(entity, "b", 2.0),
        remove(entity, "missing"),
        remove(entity, "b"),
        alpha_cutoff(entity, FieldValue::F32(unchanged_cutoff)),
        alpha_cutoff(entity, FieldValue::F32(0.125)),
        set(entity, "a", -0.0),
        set(entity, "a", 0.0),
    ];
    // A compare-and-set that writes the value already stored publishes nothing.
    operations.push(alpha_cutoff_if(entity, 0.125, 0.125));
    operations.push(alpha_cutoff_if(entity, 0.125, 0.5));
    operations.push(set(entity, "c", 3.0));
    let expected = [true, false, true, false, true, false, true, true, true]
        .into_iter()
        .chain([false, true, true])
        .filter(|changed| *changed)
        .count();
    assert!(run(&mut host, world, operations).result.is_ok());

    let events: Vec<_> = host
        .world_mut(world)
        .unwrap()
        .drain_system_events::<LifecyclePublisherOutput>(LifecyclePublisherSystem::ID, SESSION)
        .into_iter()
        .flat_map(|LifecyclePublisherOutput(events)| events)
        .map(|event| event.observation)
        .collect();
    let updates = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                LifecycleObservation::Component {
                    kind: ComponentLifecycleKind::Updated,
                    ..
                }
            )
        })
        .count();
    assert_eq!(updates, expected, "{events:?}");
    assert_eq!(updates, events.len(), "{events:?}");
}

mod rows {
    //! The same staging contracts for schema-row properties, addressed by
    //! their field offsets on the test-only rows component.

    use super::*;
    use crate::components::rows::Rows;
    use crate::components::schema::FieldValue as SchemaValue;
    use crate::components::{RowsFixture, RowsFixtureItem};

    const WEIGHT: u32 = 0;
    const MARK: u32 = 8;

    fn offset(slot: u32, property: u32) -> u32 {
        Rows::<RowsFixtureItem>::offset(0, slot, property).unwrap()
    }

    fn write(entity: EntityId, offset: u32, value: FieldValue) -> Command {
        Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::ROWS_FIXTURE,
            field: FieldWrite {
                offset,
                value,
            },
        }
    }

    fn weight(entity: EntityId, slot: u32, value: f32) -> Command {
        write(
            entity,
            offset(slot, WEIGHT),
            FieldValue::Dynamic(DynamicValue::F32(value)),
        )
    }

    /// A World holding one committed rows component with `slots` item rows,
    /// each with a present optional `mark`.
    fn rows_world(slots: u32) -> (HostRuntime, WorldId, EntityId) {
        let mut fixture = RowsFixture::default();
        for slot in 0..slots {
            fixture
                .items
                .insert(
                    slot,
                    RowsFixtureItem {
                        mark: Some(1.0),
                        ..RowsFixtureItem::default()
                    },
                )
                .unwrap();
        }
        let mut host = HostRuntime::new();
        let world = host
            .create_world(
                WorldLimits::default(),
                &[crate::systems::lifecycle_publisher::LifecyclePublisherSystem::ID],
            )
            .unwrap();
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
                Command::insert_value(EntityRef::Alias(1), ComponentValue::RowsFixture(fixture)),
            ],
        );
        let entity = outcome.result.unwrap()[0].1;
        (host, world, entity)
    }

    fn fixture(host: &mut HostRuntime, world: WorldId, entity: EntityId) -> RowsFixture {
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

    fn field(host: &mut HostRuntime, world: WorldId, entity: EntityId, offset: u32) -> SchemaValue {
        ComponentValue::RowsFixture(fixture(host, world, entity))
            .field(offset)
            .unwrap()
    }

    #[test]
    fn batched_row_property_writes_copy_the_component_independently_of_their_count() {
        let mut copies = Vec::new();
        for count in [16, 512] {
            let (mut host, world, entity) = rows_world(512);
            let writes = (0..count)
                .map(|slot| weight(entity, slot, slot as f32))
                .collect();
            RowsFixture::take_clone_count();
            assert!(run(&mut host, world, writes).result.is_ok());
            copies.push(RowsFixture::take_clone_count());

            let stored = fixture(&mut host, world, entity);
            assert_eq!(
                stored.items.get(count - 1).unwrap().weight,
                (count - 1) as f32
            );
        }
        assert_eq!(
            copies[0], copies[1],
            "a batch copies its staged component a fixed number of times, not once per row write"
        );
    }

    #[test]
    fn clearing_an_optional_row_property_is_an_observed_update_that_can_be_undone() {
        let (mut host, world, entity) = rows_world(2);
        host.world_mut(world)
            .unwrap()
            .enqueue_system_command(
                LifecyclePublisherSystem::ID,
                SESSION,
                LifecyclePublisherCommand::Subscribe {
                    subscription: 1,
                    filter: LifecycleFilter {
                        entities: false,
                        assets: false,
                        entity: Some(entity),
                        component: Some(ComponentValue::ROWS_FIXTURE),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        host.world_mut(world).unwrap().step(0.0).unwrap();

        let mark = offset(1, MARK);
        let present = SchemaValue::Dynamic(DynamicValue::F32(1.0));
        let operations = vec![
            write(entity, mark, FieldValue::Unset),
            write(entity, mark, FieldValue::Unset),
            write(entity, mark, FieldValue::Dynamic(DynamicValue::F32(1.0))),
            write(entity, mark, FieldValue::Unset),
        ];
        let expected = [true, false, true, true]
            .into_iter()
            .filter(|changed| *changed)
            .count();
        assert!(run(&mut host, world, operations).result.is_ok());
        assert_eq!(field(&mut host, world, entity, mark), SchemaValue::Unset);

        let updates = host
            .world_mut(world)
            .unwrap()
            .drain_system_events::<LifecyclePublisherOutput>(LifecyclePublisherSystem::ID, SESSION)
            .into_iter()
            .flat_map(|LifecyclePublisherOutput(events)| events)
            .filter(|event| {
                matches!(
                    event.observation,
                    LifecycleObservation::Component {
                        kind: ComponentLifecycleKind::Updated,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(updates, expected);

        // A required property cannot be cleared, and the rejected write keeps
        // the earlier value; a dead slot rejects both clearing and writing.
        let outcome = run(
            &mut host,
            world,
            vec![
                write(entity, mark, FieldValue::Dynamic(DynamicValue::F32(1.0))),
                write(entity, offset(1, WEIGHT), FieldValue::Unset),
            ],
        );
        assert_eq!(outcome.result.unwrap_err().operation, Some(1));
        assert_eq!(field(&mut host, world, entity, mark), present);
        for value in [
            FieldValue::Unset,
            FieldValue::Dynamic(DynamicValue::F32(1.0)),
        ] {
            let outcome = run(
                &mut host,
                world,
                vec![write(entity, offset(5, MARK), value)],
            );
            assert!(outcome.result.is_err());
        }
    }
}
