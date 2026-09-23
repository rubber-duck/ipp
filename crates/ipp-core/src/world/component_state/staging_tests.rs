//! Cost and observable contracts of ordered writes to one staged component.

use crate::components::dynamic_properties::clone_count;
use crate::components::{BufferCounters, CustomMaterial, PreparedBuffer};
use crate::systems::lifecycle_publisher::{
    ComponentLifecycleKind, LifecycleFilter, LifecycleObservation, LifecyclePublisherCommand,
    LifecyclePublisherOutput, LifecyclePublisherSystem,
};
use crate::world::*;
use crate::{
    ComponentOverlayMode, DynamicProperties, DynamicValue, EntityOverlayMode, HostRuntime,
    StateOverlayRef, WorldId,
};
use std::{mem::offset_of, rc::Rc};

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
    let world = host.create_world(WorldLimits::default()).unwrap();
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
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                fields: vec![],
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

fn properties(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
    base: bool,
) -> DynamicProperties {
    let snapshot = host.world_mut(world).unwrap().inspect(entity).unwrap();
    let values = if base {
        snapshot.base
    } else {
        snapshot.effective
    };
    values
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::CustomMaterial(value) => Some(value.properties),
            _ => None,
        })
        .unwrap()
}

/// Attach a Bound overlay to the material; returns (owner, overlay) aliases.
fn attach_overlay() -> Vec<Command> {
    vec![
        Command::CreateStateOverlayOwner {
            alias: 1,
        },
        Command::AttachEntityOverlayBinding {
            owner: StateOverlayRef::Alias(1),
            alias: 2,
            symbolic_id: "material".into(),
            mode: EntityOverlayMode::Bound,
        },
        Command::AttachComponentStateOverlay {
            owner: StateOverlayRef::Alias(1),
            binding: StateOverlayRef::Alias(2),
            alias: 3,
            component: ComponentValue::CUSTOM_MATERIAL,
            mode: ComponentOverlayMode::Bound,
            fields: vec![],
        },
    ]
}

fn override_property(name: &str, value: f32) -> Command {
    Command::UpdateDynamicComponentStateOverlay {
        owner: StateOverlayRef::Alias(1),
        overlay: StateOverlayRef::Alias(3),
        properties: vec![(name.into(), DynamicValue::F32(value))],
        clear: vec![],
    }
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

        let effective = properties(&mut host, world, entity, false);
        assert_eq!(effective.descriptors().len(), count + 1);
        assert_eq!(
            effective.get(&format!("lane_{}", count - 1)),
            Some(DynamicValue::F32((count - 1) as f32))
        );
    }
    assert_eq!(
        copies[0], copies[1],
        "a batch copies its staged component a fixed number of times, not once per write"
    );
}

#[test]
fn resource_owning_components_still_prepare_after_each_write_without_copies() {
    let counters = Rc::new(BufferCounters::default());
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let entity = run(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 0,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::PreparedBuffer(PreparedBuffer {
                    length: 4,
                    counters: counters.clone(),
                    allocation: None,
                }),
            },
        ],
    )
    .result
    .unwrap()[0]
        .1;
    let length = |value: u32| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::PREPARED_BUFFER,
        field: FieldWrite {
            offset: offset_of!(PreparedBuffer, length) as u32,
            value: FieldValue::U32(value),
        },
    };

    let mut clones = Vec::new();
    for count in [4u32, 64] {
        counters.clones.set(0);
        let prepared = counters.prepared.get();
        let writes = (0..count).map(|index| length(100 + index)).collect();
        assert!(run(&mut host, world, writes).result.is_ok());
        clones.push(counters.clones.get());
        assert_eq!(
            counters.prepared.get() - prepared,
            count as usize,
            "fallible activation stays attributed to each operation"
        );
    }
    assert_eq!(clones[0], clones[1]);

    // An activation failure stops the batch at the operation that caused it.
    let outcome = run(&mut host, world, vec![length(20), length(13), length(21)]);
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(1));
    assert_eq!(error.reason, ErrorReason::InvalidValue);
}

#[test]
fn overlay_layering_in_one_batch_matches_separate_batches() {
    let operations = || {
        let mut operations = attach_overlay();
        operations.push(override_property("seed", 5.0));
        operations.push(set(EntityId::from_bits(0), "seed", 2.0));
        operations.push(set(EntityId::from_bits(0), "other", 3.0));
        operations
    };
    let bind = |entity: EntityId, command: Command| match command {
        Command::SetDynamicProperty {
            component,
            name,
            value,
            ..
        } => Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component,
            name,
            value,
        },
        command => command,
    };

    let (mut joined, joined_world, joined_entity) = material_world();
    let outcome = run(
        &mut joined,
        joined_world,
        operations()
            .into_iter()
            .map(|command| bind(joined_entity, command))
            .collect(),
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
    let owner = StateOverlayRef::Handle(outcome.state_overlays[0].id);

    let (mut split, split_world, split_entity) = material_world();
    let mut aliases = Vec::new();
    let all = operations();
    let (attachment, writes) = all.split_at(4);
    aliases.extend(
        run(
            &mut split,
            split_world,
            attachment
                .iter()
                .cloned()
                .map(|command| bind(split_entity, command))
                .collect(),
        )
        .state_overlays,
    );
    for command in writes {
        assert!(
            run(
                &mut split,
                split_world,
                vec![bind(split_entity, command.clone())]
            )
            .result
            .is_ok()
        );
    }

    for base in [false, true] {
        assert_eq!(
            properties(&mut joined, joined_world, joined_entity, base),
            properties(&mut split, split_world, split_entity, base)
        );
    }
    let effective = properties(&mut joined, joined_world, joined_entity, false);
    assert_eq!(effective.get("seed"), Some(DynamicValue::F32(5.0)));
    assert_eq!(effective.get("other"), Some(DynamicValue::F32(3.0)));
    let base = properties(&mut joined, joined_world, joined_entity, true);
    assert_eq!(base.get("seed"), Some(DynamicValue::F32(2.0)));

    assert!(
        run(
            &mut joined,
            joined_world,
            vec![Command::ReleaseStateOverlayOwner {
                owner,
            }],
        )
        .result
        .is_ok()
    );
    let released = properties(&mut joined, joined_world, joined_entity, false);
    assert_eq!(released.get("seed"), Some(DynamicValue::F32(2.0)));
    assert_eq!(
        released,
        properties(&mut joined, joined_world, joined_entity, true)
    );
}

#[test]
fn later_operations_read_the_staged_writes_of_earlier_ones() {
    let (mut host, world, entity) = material_world();
    let mut operations = attach_overlay();
    // A Bound overlay may only override an existing property, so the update
    // succeeds only if it observes the producer write staged just before it.
    operations.push(set(entity, "fresh", 1.0));
    operations.push(override_property("fresh", 7.0));
    let outcome = run(&mut host, world, operations);
    assert!(outcome.result.is_ok(), "{outcome:?}");
    assert_eq!(
        properties(&mut host, world, entity, false).get("fresh"),
        Some(DynamicValue::F32(7.0))
    );
    assert_eq!(
        properties(&mut host, world, entity, true).get("fresh"),
        Some(DynamicValue::F32(1.0))
    );
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
    for base in [false, true] {
        let values = properties(&mut host, world, entity, base);
        assert_eq!(values.get("a"), Some(DynamicValue::F32(1.0)));
        assert_eq!(values.get("b"), Some(DynamicValue::F32(2.0)));
        assert_eq!(
            values.get("c"),
            None,
            "operations after the failure do not apply"
        );
    }
    let snapshot = host.world_mut(world).unwrap().inspect(entity).unwrap();
    let cutoff = snapshot
        .effective
        .iter()
        .find_map(|value| match value {
            ComponentValue::CustomMaterial(value) => Some(value.alpha_cutoff),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        cutoff, 0.25,
        "the rejected field write leaves the earlier value"
    );

    // A later batch continues from the applied state.
    assert!(
        run(&mut host, world, vec![set(entity, "c", 3.0)])
            .result
            .is_ok()
    );
    assert_eq!(
        properties(&mut host, world, entity, false).get("c"),
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
    let snapshot = host.world_mut(world).unwrap().inspect(entity).unwrap();
    let unchanged_cutoff = snapshot
        .effective
        .iter()
        .find_map(|value| match value {
            ComponentValue::CustomMaterial(value) => Some(value.alpha_cutoff),
            _ => None,
        })
        .unwrap();

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
    // Overlay operations compare whole values against the value observed after
    // the in-place writes above; an unchanged attachment publishes nothing.
    operations.extend(attach_overlay());
    operations.push(override_property("a", 9.0));
    operations.push(set(entity, "c", 3.0));
    let expected = [true, false, true, false, true, false, true, true, true]
        .into_iter()
        .chain([false, false, false, true, true])
        .filter(|changed| *changed)
        .count();
    assert!(run(&mut host, world, operations).result.is_ok());

    let events: Vec<_> = host
        .world_mut(world)
        .unwrap()
        .drain_system_events::<LifecyclePublisherOutput>(LifecyclePublisherSystem::ID, SESSION)
        .into_iter()
        .flat_map(|output| match output {
            LifecyclePublisherOutput::Events(events) => events,
            LifecyclePublisherOutput::Overflow {
                ..
            } => panic!("bounded fixture overflowed"),
        })
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
