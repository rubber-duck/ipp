//! Retention assertions exercise real lifecycle allocations, including lean builds.

use super::*;
use crate::components::{BufferCounters, PreparedBuffer};
use std::rc::Rc;

#[test]
fn ordinary_components_retain_exactly_one_payload_after_commit_and_unrelated_updates() {
    let counters = Rc::new(BufferCounters::default());
    let mut host = crate::HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: (0..64)
                .flat_map(|alias| {
                    [
                        Command::Create {
                            alias,
                            metadata: EntityMetadata::default(),
                        },
                        Command::InsertComponentValue {
                            entity: EntityRef::Alias(alias),
                            value: ComponentValue::PreparedBuffer(PreparedBuffer {
                                length: 4096,
                                counters: counters.clone(),
                                allocation: None,
                            }),
                        },
                        Command::InsertComponent {
                            entity: EntityRef::Alias(alias),
                            component: ComponentValue::SCALAR,
                            fields: Vec::new(),
                        },
                    ]
                })
                .collect(),
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let entities = report.outcomes[0].result.as_ref().unwrap();
    let addresses: Vec<_> = entities
        .iter()
        .map(|(_, entity)| {
            let buffer = world
                .world
                .components
                .prepared_buffer(entity.index() as usize)
                .unwrap();
            assert_eq!(Rc::strong_count(buffer.allocation.as_ref().unwrap()), 1);
            (
                buffer as *const PreparedBuffer,
                Rc::as_ptr(buffer.allocation.as_ref().unwrap()),
            )
        })
        .collect();
    // One counter reference in each live component and its owned allocation;
    // neither an authored component nor a resolved component remains alongside it.
    assert_eq!(Rc::strong_count(&counters), 1 + entities.len() * 2);
    assert!(
        world
            .world
            .state
            .entities
            .values()
            .flat_map(|record| record.layers.values())
            .all(|layer| layer.inputs.retained_inputs().next().is_none())
    );

    counters.clones.set(0);
    world
        .enqueue(Batch {
            id: 2,
            operations: entities
                .iter()
                .map(|(_, entity)| Command::SetField {
                    entity: EntityRef::Handle(*entity),
                    component: ComponentValue::SCALAR,
                    field: FieldWrite {
                        offset: std::mem::offset_of!(Scalar, value) as u32,
                        value: FieldValue::F32(9.0),
                    },
                })
                .collect(),
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    assert_eq!(counters.clones.get(), 0);
    assert_eq!(Rc::strong_count(&counters), 1 + entities.len() * 2);
    for ((_, entity), expected) in entities.iter().zip(addresses) {
        let buffer = world
            .world
            .components
            .prepared_buffer(entity.index() as usize)
            .unwrap();
        assert_eq!(
            (
                buffer as *const PreparedBuffer,
                Rc::as_ptr(buffer.allocation.as_ref().unwrap())
            ),
            expected
        );
    }
    assert!(
        world
            .world
            .state
            .entities
            .values()
            .flat_map(|record| record.layers.values())
            .all(|layer| layer.inputs.retained_inputs().next().is_none())
    );
}

#[test]
fn scalar_constraints_keep_restoration_only_for_bound_targets() {
    let mut host = crate::HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut operations: Vec<_> = (0..64)
        .flat_map(|alias| {
            [
                Command::Create {
                    alias,
                    metadata: EntityMetadata::default(),
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(alias),
                    component: ComponentValue::SCALAR,
                    fields: vec![FieldWrite {
                        offset: std::mem::offset_of!(Scalar, value) as u32,
                        value: FieldValue::F32(alias as f32),
                    }],
                },
            ]
        })
        .collect();
    operations.push(Command::InsertComponent {
        entity: EntityRef::Alias(1),
        component: ComponentValue::LINEAR_DRIVER,
        fields: vec![FieldWrite {
            offset: std::mem::offset_of!(crate::components::LinearDriver, source) as u32,
            value: FieldValue::Entity(EntityRef::Alias(0)),
        }],
    });
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let entities = report.outcomes[0].result.as_ref().unwrap();
    let target = entities[1].1;
    assert_eq!(
        world
            .system::<systems::constraints::ConstraintSystem>(
                systems::constraints::ConstraintSystem::ID
            )
            .unwrap()
            .state
            .restores
            .len(),
        1
    );
    assert_eq!(
        world
            .system::<systems::constraints::ConstraintSystem>(
                systems::constraints::ConstraintSystem::ID
            )
            .unwrap()
            .state
            .restores[&target]
            .base
            .value,
        1.0
    );
    let snapshot = world.inspect(target).unwrap();
    assert!(
        snapshot
            .base
            .iter()
            .any(|value| matches!(value, ComponentValue::Scalar(value) if value.value == 1.0))
    );
    assert!(
        snapshot
            .effective
            .iter()
            .any(|value| matches!(value, ComponentValue::Scalar(value) if value.value == 0.0))
    );
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(target),
                component: ComponentValue::LINEAR_DRIVER,
            }],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    assert!(
        world
            .system::<systems::constraints::ConstraintSystem>(
                systems::constraints::ConstraintSystem::ID
            )
            .unwrap()
            .state
            .restores
            .is_empty()
    );
    assert_eq!(
        world
            .world
            .components
            .scalar(target.index() as usize)
            .unwrap()
            .value,
        1.0
    );
}

#[test]
fn forced_overlay_cleanup_preserves_unrelated_evaluation_and_latest_producer_input() {
    use crate::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    let mut host = crate::HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut operations: Vec<_> = (0..4)
        .flat_map(|alias| {
            [
                Command::Create {
                    alias,
                    metadata: EntityMetadata {
                        symbolic_id: Some(format!("entity-{alias}")),
                        classes: Vec::new(),
                    },
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(alias),
                    component: ComponentValue::SCALAR,
                    fields: vec![FieldWrite {
                        offset: std::mem::offset_of!(Scalar, value) as u32,
                        value: FieldValue::F32(alias as f32),
                    }],
                },
            ]
        })
        .collect();
    for target in [1, 3] {
        operations.push(Command::InsertComponent {
            entity: EntityRef::Alias(target),
            component: ComponentValue::LINEAR_DRIVER,
            fields: vec![FieldWrite {
                offset: std::mem::offset_of!(crate::components::LinearDriver, source) as u32,
                value: FieldValue::Entity(EntityRef::Alias(target - 1)),
            }],
        });
    }
    operations.extend([
        Command::CreateStateOverlayOwner {
            alias: 0,
        },
        Command::AttachEntityOverlayBinding {
            owner: StateOverlayRef::Alias(0),
            alias: 1,
            symbolic_id: "entity-1".into(),
            mode: EntityOverlayMode::Bound,
        },
        Command::AttachComponentStateOverlay {
            owner: StateOverlayRef::Alias(0),
            binding: StateOverlayRef::Alias(1),
            alias: 2,
            component: ComponentValue::SCALAR,
            mode: ComponentOverlayMode::Bound,
            fields: vec![FieldWrite {
                offset: std::mem::offset_of!(Scalar, value) as u32,
                value: FieldValue::F32(9.0),
            }],
        },
    ]);
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let entities = report.outcomes[0].result.as_ref().unwrap();
    let owner = report.outcomes[0].state_overlays[0].id;
    let target = entities[1].1;
    let other = entities[3].1;
    assert_eq!(
        world
            .world
            .components
            .scalar(target.index() as usize)
            .unwrap()
            .value,
        0.0
    );
    assert_eq!(
        world
            .world
            .components
            .scalar(other.index() as usize)
            .unwrap()
            .value,
        2.0
    );

    world.release_state_overlay_owners([owner]);
    assert_eq!(
        world
            .world
            .components
            .scalar(target.index() as usize)
            .unwrap()
            .value,
        1.0
    );
    assert_eq!(
        world
            .world
            .components
            .scalar(other.index() as usize)
            .unwrap()
            .value,
        2.0
    );
    assert!(
        world
            .inspect(target)
            .unwrap()
            .base
            .iter()
            .any(|value| matches!(value, ComponentValue::Scalar(value) if value.value == 1.0))
    );
    world.step(0.0).unwrap();
    assert_eq!(
        world
            .world
            .components
            .scalar(target.index() as usize)
            .unwrap()
            .value,
        0.0
    );
    world.release_state_overlay_owners([owner]);
    assert_eq!(
        world
            .world
            .components
            .scalar(target.index() as usize)
            .unwrap()
            .value,
        0.0,
        "repeated cleanup must not withdraw an unrelated constraint contribution"
    );
}

#[test]
fn forced_overlay_cleanup_releases_departing_asset_demand_after_preparation() {
    use crate::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    let mut host = crate::HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata {
                        symbolic_id: Some("mesh".into()),
                        classes: Vec::new(),
                    },
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(0),
                    component: ComponentValue::MESH_INSTANCE,
                    fields: Vec::new(),
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
                    mode: ComponentOverlayMode::Bound,
                    fields: vec![FieldWrite {
                        offset: std::mem::offset_of!(crate::components::MeshInstance, source)
                            as u32,
                        value: FieldValue::String(
                            "ipp://mesh/cube?width=1&height=1&length=1".into(),
                        ),
                    }],
                },
            ],
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    assert!(report.outcomes[0].result.is_ok());
    let owner = report.outcomes[0].state_overlays[0].id;
    assert_eq!(
        world
            .system::<systems::asset_dependencies::AssetDependencySystem>(
                systems::asset_dependencies::AssetDependencySystem::ID
            )
            .unwrap()
            .state
            .authored_demand()
            .len(),
        1
    );
    world.release_state_overlay_owners([owner]);
    assert!(
        world
            .system::<systems::asset_dependencies::AssetDependencySystem>(
                systems::asset_dependencies::AssetDependencySystem::ID
            )
            .unwrap()
            .state
            .authored_demand()
            .is_empty(),
        "the resolved payload and its demand must leave with the overlay owner"
    );
}

#[test]
fn commit_growth_failed_preparation_and_neighbor_reuse_preserve_occupied_address() {
    let mut host = crate::HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let scalar_id = ComponentValue::SCALAR;
    let make = |alias| Command::Create {
        alias,
        metadata: EntityMetadata::default(),
    };
    let insert = |entity| Command::InsertComponent {
        entity,
        component: scalar_id,
        fields: vec![],
    };
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![make(0), insert(EntityRef::Alias(0))],
        })
        .unwrap();
    let id = world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    let address = world.world.components.scalar(id.index() as usize).unwrap() as *const Scalar;
    let mut operations = Vec::new();
    for alias in 1..100 {
        operations.push(make(alias));
        operations.push(insert(EntityRef::Alias(alias)));
    }
    world
        .enqueue(Batch {
            id: 2,
            operations,
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let neighbor = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert_eq!(
        address,
        world.world.components.scalar(id.index() as usize).unwrap() as *const Scalar
    );
    world
        .enqueue(Batch {
            id: 3,
            operations: vec![
                Command::Delete {
                    entity: EntityRef::Handle(EntityId::from_bits(u64::MAX)),
                },
                Command::Delete {
                    entity: EntityRef::Handle(EntityId::from_bits(u64::MAX)),
                },
            ],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_err());
    world
        .enqueue(Batch {
            id: 4,
            operations: vec![
                Command::Delete {
                    entity: EntityRef::Handle(neighbor),
                },
                make(0),
                insert(EntityRef::Alias(0)),
            ],
        })
        .unwrap();
    world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap();
    assert_eq!(
        address,
        world.world.components.scalar(id.index() as usize).unwrap() as *const Scalar
    );
}

#[test]
fn typed_scene_slots_survive_growth_failed_preparation_and_neighbor_reuse() {
    let mut host = crate::HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let create = |alias| Command::Create {
        alias,
        metadata: EntityMetadata::default(),
    };
    let insert = |entity| Command::InsertComponent {
        entity,
        component: crate::ComponentValue::TRANSFORM,
        fields: vec![],
    };
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![create(0), insert(EntityRef::Alias(0))],
        })
        .unwrap();
    let entity = world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    let address = world
        .world
        .components
        .transform(entity.index() as usize)
        .unwrap() as *const Transform;
    let mut operations = Vec::new();
    for alias in 0..100 {
        operations.push(create(alias));
        operations.push(insert(EntityRef::Alias(alias)));
    }
    world
        .enqueue(Batch {
            id: 2,
            operations,
        })
        .unwrap();
    let neighbor = world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1;
    assert_eq!(
        world
            .world
            .components
            .transform(entity.index() as usize)
            .unwrap() as *const Transform,
        address
    );

    world
        .enqueue(Batch {
            id: 3,
            operations: vec![
                Command::SetField {
                    entity: EntityRef::Handle(entity),
                    component: crate::ComponentValue::TRANSFORM,
                    field: FieldWrite {
                        offset: 0,
                        value: FieldValue::F32(7.0),
                    },
                },
                Command::Delete {
                    entity: EntityRef::Handle(neighbor),
                },
                create(0),
                insert(EntityRef::Alias(0)),
            ],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    assert_eq!(
        world
            .world
            .components
            .transform(entity.index() as usize)
            .unwrap() as *const Transform,
        address
    );
    assert_eq!(
        world
            .world
            .components
            .transform(entity.index() as usize)
            .unwrap()
            .x,
        7.0
    );

    world
        .enqueue(Batch {
            id: 4,
            operations: vec![
                Command::Delete {
                    entity: EntityRef::Handle(entity),
                },
                create(0),
                insert(EntityRef::Alias(0)),
                Command::Delete {
                    entity: EntityRef::Handle(entity),
                },
            ],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_err());
    assert_eq!(
        world
            .world
            .components
            .transform(entity.index() as usize)
            .unwrap() as *const Transform,
        address
    );
    assert_eq!(
        world
            .world
            .components
            .transform(entity.index() as usize)
            .unwrap()
            .x,
        0.0
    );
}

use crate::{FieldValue, FieldWrite, components::Transform};

#[test]
fn recycled_command_buffers_bound_retained_capacity_and_release_payloads() {
    let mut host = crate::HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut bounded = Vec::with_capacity(256);
    bounded.push(Command::Create {
        alias: 1,
        metadata: EntityMetadata::default(),
    });
    let pointer = bounded.as_ptr();
    world.recycle_command_buffer(bounded);
    let reused = world.take_command_buffer();
    assert!(reused.is_empty());
    assert_eq!(reused.capacity(), 256);
    assert_eq!(reused.as_ptr(), pointer);
    world.recycle_command_buffer(reused);

    // An unusually large direct-core batch must not enlarge the reusable pool.
    world.recycle_command_buffer(Vec::with_capacity(100_000));
    let reused = world.take_command_buffer();
    assert_eq!(reused.as_ptr(), pointer);
    assert!(world.take_command_buffer().capacity() <= 256);
}
