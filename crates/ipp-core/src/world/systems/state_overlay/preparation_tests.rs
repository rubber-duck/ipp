use crate::world::*;
use crate::{
    ComponentOverlayMode, EntityOverlayMode, StateOverlayRef,
    components::schema::SchemaComponent,
    components::{BufferCounters, PreparedBuffer},
};
use std::{mem::offset_of, rc::Rc};

fn run(world: &mut crate::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.step(0.0).unwrap()
}

fn insert(entity: EntityId, counters: &Rc<BufferCounters>, length: u32) -> Command {
    Command::InsertComponentValue {
        entity: EntityRef::Handle(entity),
        value: ComponentValue::PreparedBuffer(PreparedBuffer {
            length,
            counters: counters.clone(),
            allocation: None,
        }),
    }
}

fn length(value: u32) -> FieldWrite {
    FieldWrite {
        offset: offset_of!(PreparedBuffer, length) as u32,
        value: FieldValue::U32(value),
    }
}

#[test]
fn noncreatable_component_prepares_privately_binds_and_releases_owned_effective_resources() {
    let counters = Rc::new(BufferCounters::default());
    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let report = run(
        &mut world,
        vec![Command::Create {
            alias: 0,
            metadata: EntityMetadata {
                symbolic_id: Some("native".into()),
                classes: vec![],
            },
        }],
    );
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert!(PreparedBuffer::create().is_none());
    assert!(ComponentValue::has_field(
        ComponentValue::PREPARED_BUFFER,
        offset_of!(PreparedBuffer, length) as u32
    ));
    assert_eq!(
        registry::create(ComponentValue::PREPARED_BUFFER),
        Err(ErrorReason::MissingCreationContract)
    );
    assert!(
        run(&mut world, vec![insert(entity, &counters, 64)]).outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!((counters.prepared.get(), counters.released.get()), (1, 0));
    let address = world
        .world
        .components
        .prepared_buffer(entity.index() as usize)
        .unwrap() as *const PreparedBuffer;
    let allocation = Rc::downgrade(
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap()
            .allocation
            .as_ref()
            .unwrap(),
    );

    // A failed later operation retains the applied replacement and releases the old allocation.
    let report = run(
        &mut world,
        vec![
            insert(entity, &counters, 128),
            Command::Delete {
                entity: EntityRef::Alias(99),
            },
        ],
    );
    assert!(report.outcomes[0].result.is_err());
    assert_eq!((counters.prepared.get(), counters.released.get()), (2, 1));
    assert!(allocation.upgrade().is_none());
    let report = run(&mut world, vec![insert(entity, &counters, 13)]);
    assert!(report.outcomes[0].result.is_err());
    assert_eq!((counters.prepared.get(), counters.released.get()), (3, 3));
    assert!(allocation.upgrade().is_none());

    assert!(
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .is_none()
    );
    run(&mut world, vec![insert(entity, &counters, 64)]).outcomes[0]
        .result
        .as_ref()
        .unwrap();
    let report = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "native".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::PREPARED_BUFFER,
                mode: ComponentOverlayMode::Bound,
                fields: vec![length(256)],
            },
        ],
    );
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    assert!(allocation.upgrade().is_none());
    assert_eq!(
        address,
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap() as *const PreparedBuffer
    );
    assert_eq!(
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap()
            .allocation
            .as_ref()
            .unwrap()
            .bytes
            .len(),
        256
    );
    let inputs =
        &world.world.state.entities[&entity].layers[&ComponentValue::PREPARED_BUFFER].inputs;
    assert_eq!(
        inputs.hidden_fields.len(),
        1,
        "only the overridden length is retained"
    );
    assert!(inputs.retained_inputs().next().is_none());
    assert_eq!(
        Rc::strong_count(&counters),
        3,
        "one live component and its allocation"
    );
    let resources = &report.outcomes[0].state_overlays;
    let owner = StateOverlayRef::Handle(resources[0].id);
    let overlay = StateOverlayRef::Handle(resources[2].id);
    assert!(
        run(
            &mut world,
            vec![Command::UpdateComponentStateOverlay {
                owner,
                overlay,
                fields: vec![length(512)],
                clear: vec![]
            }]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    assert!(
        run(
            &mut world,
            vec![Command::ReleaseStateOverlayOwner {
                owner
            }]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!(
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap()
            .length,
        64
    );
    assert!(
        run(
            &mut world,
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::PREPARED_BUFFER
            }]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!(counters.prepared.get(), counters.released.get());
    assert!(
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .is_none()
    );
}

#[test]
fn scalar_batches_do_not_clone_prepare_or_replace_unrelated_payloads() {
    let counters = Rc::new(BufferCounters::default());
    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let report = run(
        &mut world,
        (0..128)
            .map(|alias| Command::Create {
                alias,
                metadata: Default::default(),
            })
            .collect(),
    );
    let entities: Vec<_> = report.outcomes[0]
        .result
        .as_ref()
        .unwrap()
        .iter()
        .map(|(_, entity)| *entity)
        .collect();
    for &entity in &entities {
        assert!(
            run(
                &mut world,
                vec![
                    insert(entity, &counters, 4096),
                    Command::InsertComponent {
                        entity: EntityRef::Handle(entity),
                        component: ComponentValue::SCALAR,
                        fields: vec![]
                    }
                ]
            )
            .outcomes[0]
                .result
                .is_ok()
        );
    }
    let addresses: Vec<_> = entities
        .iter()
        .map(|entity| {
            let buffer = world
                .world
                .components
                .prepared_buffer(entity.index() as usize)
                .unwrap();
            (
                buffer as *const PreparedBuffer,
                Rc::as_ptr(buffer.allocation.as_ref().unwrap()),
            )
        })
        .collect();
    counters.clones.set(0);
    let prepared = counters.prepared.get();
    let released = counters.released.get();
    for _ in 0..2 {
        let operations = (0..256)
            .map(|i| Command::SetField {
                entity: EntityRef::Handle(entities[i % entities.len()]),
                component: ComponentValue::SCALAR,
                field: FieldWrite {
                    offset: offset_of!(Scalar, value) as u32,
                    value: FieldValue::F32(i as f32),
                },
            })
            .collect();
        assert!(run(&mut world, operations).outcomes[0].result.is_ok());
    }
    world.step(0.0).unwrap();
    assert_eq!(
        counters.clones.get(),
        0,
        "unrelated authored/effective values must not be cloned"
    );
    assert_eq!(counters.prepared.get(), prepared);
    assert_eq!(counters.released.get(), released);
    for (entity, addresses) in entities.iter().zip(addresses) {
        let buffer = world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap();
        assert_eq!(
            addresses,
            (
                buffer as *const PreparedBuffer,
                Rc::as_ptr(buffer.allocation.as_ref().unwrap())
            )
        );
    }
}

#[test]
fn noncreatable_auto_follows_native_base_but_cannot_invent_a_fallback() {
    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let counters = Rc::new(BufferCounters::default());
    let report = run(
        &mut world,
        vec![Command::Create {
            alias: 0,
            metadata: EntityMetadata {
                symbolic_id: Some("native".into()),
                classes: vec![],
            },
        }],
    );
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert!(
        run(&mut world, vec![insert(entity, &counters, 32)]).outcomes[0]
            .result
            .is_ok()
    );
    let report = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "native".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::PREPARED_BUFFER,
                mode: ComponentOverlayMode::Auto,
                fields: vec![length(64)],
            },
        ],
    );
    assert!(report.outcomes[0].result.is_ok());
    let owner = StateOverlayRef::Handle(report.outcomes[0].state_overlays[0].id);
    let binding = StateOverlayRef::Handle(report.outcomes[0].state_overlays[1].id);
    let result = run(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::PREPARED_BUFFER,
        }],
    );
    assert_eq!(
        result.outcomes[0].result.as_ref().unwrap_err().reason,
        ErrorReason::MissingCreationContract
    );
    assert_eq!(
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap()
            .length,
        64
    );
    assert!(
        run(&mut world, vec![insert(entity, &counters, 128)]).outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!(
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap()
            .length,
        64
    );
    assert!(
        run(
            &mut world,
            vec![Command::ReleaseStateOverlayOwner {
                owner
            }]
        )
        .outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!(
        world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap()
            .length,
        128
    );
    // The old released association cannot be reused to acquire another component.
    assert!(
        run(
            &mut world,
            vec![Command::AttachComponentStateOverlay {
                owner,
                binding,
                alias: 3,
                component: ComponentValue::PREPARED_BUFFER,
                mode: ComponentOverlayMode::Owned,
                fields: vec![]
            }]
        )
        .outcomes[0]
            .result
            .is_err()
    );
}

#[test]
fn activation_budget_failure_keeps_earlier_components_and_releases_failed_work() {
    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits {
            max_staging_bytes: 2 << 20,
            ..Default::default()
        })
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let counters = Rc::new(BufferCounters::default());
    let report = run(
        &mut world,
        (0..64)
            .map(|alias| Command::Create {
                alias,
                metadata: Default::default(),
            })
            .collect(),
    );
    let entities: Vec<_> = report.outcomes[0]
        .result
        .as_ref()
        .unwrap()
        .iter()
        .map(|(_, entity)| *entity)
        .collect();
    let before = world.world.state.activation_budget;
    assert!(before > 65_536 && before < 64 * 65_536);
    let report = run(
        &mut world,
        entities
            .iter()
            .map(|&entity| insert(entity, &counters, 65_536))
            .collect(),
    );
    assert_eq!(
        report.outcomes[0].result.as_ref().unwrap_err().reason,
        ErrorReason::Capacity
    );
    let applied = entities
        .iter()
        .filter(|entity| {
            world
                .world
                .components
                .prepared_buffer(entity.index() as usize)
                .is_some()
        })
        .count();
    assert_eq!(applied, before / 65_536);
    assert_eq!(counters.prepared.get() - counters.released.get(), applied);
    assert!(
        run(&mut world, vec![insert(entities[0], &counters, 65_536)]).outcomes[0]
            .result
            .is_ok()
    );
}

#[test]
fn native_insertion_checks_field_policy_even_when_an_overlay_hides_the_base() {
    use crate::components::MeshInstance;

    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let report = run(
        &mut world,
        vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata {
                    symbolic_id: Some("mesh".into()),
                    classes: vec![],
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::MeshInstance(MeshInstance {
                    source: "bundle!mesh?flavour=opaque text".into(),
                    variant: 0,
                }),
            },
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "mesh".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(0),
                binding: StateOverlayRef::Alias(1),
                alias: 2,
                component: ComponentValue::MESH_INSTANCE,
                mode: ComponentOverlayMode::Auto,
                fields: vec![FieldWrite {
                    offset: offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String("archive!/mesh?part=opaque text".into()),
                }],
            },
        ],
    );
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    let report = run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::MeshInstance(MeshInstance {
                source: "x".repeat(4097),
                variant: 0,
            }),
        }],
    );
    assert!(report.outcomes[0].result.is_ok());
    let ComponentValue::MeshInstance(base) = world
        .read()
        .producer_component(entity, ComponentValue::MESH_INSTANCE)
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(base.source, "x".repeat(4097));
    assert_eq!(
        world
            .world
            .components
            .mesh_instance(entity.index() as usize)
            .unwrap()
            .source,
        "archive!/mesh?part=opaque text"
    );
}
