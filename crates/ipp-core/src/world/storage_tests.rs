//! Component-page storage: retention assertions exercise real lifecycle
//! allocations, in every build, and pages are allocated only on demand.

use crate::components::{CustomMaterial, Scalar, Transform, dynamic_properties::clone_count};
use crate::world::systems;
use crate::{
    Batch, Command, ComponentValue, DynamicProperties, DynamicValue, EntityId, EntityMetadata,
    EntityRef, ErrorReason, FieldValue, FieldWrite, WorldLimits,
};

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

/// Transforms and final World-space propagation.
const SPATIAL_SYSTEMS: &[crate::systems::SystemId] = &[
    crate::systems::hierarchy::HierarchySystem::ID,
    crate::systems::look_at::LookAtSystem::ID,
    crate::systems::hierarchy::FinalPropagationSystem::ID,
];

/// Addresses of a material's stable slot and of its heap-owned payloads.
fn material_addresses(world: &crate::WorldContext<'_>, entity: EntityId) -> [usize; 2] {
    let material = world
        .world
        .components
        .custom_material(entity.index() as usize)
        .unwrap();
    [
        material as *const CustomMaterial as usize,
        material.source.as_ptr() as usize,
    ]
}

#[test]
fn ordinary_components_retain_exactly_one_payload_after_commit_and_unrelated_updates() {
    let mut properties = DynamicProperties::default();
    properties.set("seed", DynamicValue::F32(1.0)).unwrap();
    let mut host = crate::HostRuntime::new();
    let id = host
        .create_world(
            WorldLimits::default(),
            &[
                &[crate::systems::constraints::ConstraintSystem::ID],
                RENDER_SYSTEMS,
            ]
            .concat(),
        )
        .unwrap();
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
                            adopt: false,
                        },
                        Command::insert_value(
                            EntityRef::Alias(alias),
                            ComponentValue::CustomMaterial(CustomMaterial {
                                source: format!("file:///materials/{alias}.shader").into(),
                                properties: properties.clone(),
                                ..CustomMaterial::default()
                            }),
                        ),
                        Command::InsertComponent {
                            entity: EntityRef::Alias(alias),
                            component: ComponentValue::SCALAR,
                            fields: Vec::new(),
                            adopt: false,
                        },
                    ]
                })
                .collect(),
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let entities = report.outcomes[0].result.as_ref().unwrap().clone();
    let addresses: Vec<_> = entities
        .iter()
        .map(|(_, entity)| material_addresses(&world, *entity))
        .collect();
    // No staged copy remains beside the single stored value after commit.
    assert!(
        world
            .world
            .state
            .entities
            .values()
            .flat_map(|record| record.components.values())
            .all(|state| state.staged.is_none())
    );

    clone_count::take();
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
    // Unrelated updates neither copy nor replace the material payloads.
    assert_eq!(clone_count::take(), 0);
    for ((_, entity), expected) in entities.iter().zip(addresses) {
        assert_eq!(material_addresses(&world, *entity), expected);
    }
    assert!(
        world
            .world
            .state
            .entities
            .values()
            .flat_map(|record| record.components.values())
            .all(|state| state.staged.is_none())
    );
}

#[test]
fn removing_a_component_releases_its_asset_demand() {
    let mut host = crate::HostRuntime::new();
    let id = host
        .create_world(WorldLimits::default(), RENDER_SYSTEMS)
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let demand = |world: &crate::WorldContext<'_>| {
        world
            .system::<systems::asset_dependencies::AssetDependencySystem>(
                systems::asset_dependencies::AssetDependencySystem::ID,
            )
            .unwrap()
            .state
            .authored_demand()
            .len()
    };
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata::default(),
                    adopt: false,
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(0),
                    component: ComponentValue::MESH_INSTANCE,
                    fields: vec![FieldWrite {
                        offset: std::mem::offset_of!(crate::components::MeshInstance, source)
                            as u32,
                        value: FieldValue::String(
                            "ipp://mesh/cube?width=1&height=1&length=1".into(),
                        ),
                    }],
                    adopt: false,
                },
            ],
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    assert_eq!(demand(&world), 1);

    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::MESH_INSTANCE,
            }],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    assert_eq!(
        demand(&world),
        0,
        "the removed payload's demand leaves with the component"
    );
}

#[test]
fn commit_growth_failed_preparation_and_neighbor_reuse_preserve_occupied_address() {
    let mut host = crate::HostRuntime::new();
    let id = host
        .create_world(
            Default::default(),
            &[crate::systems::constraints::ConstraintSystem::ID],
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let scalar_id = ComponentValue::SCALAR;
    let make = |alias| Command::Create {
        alias,
        metadata: EntityMetadata::default(),
        adopt: false,
    };
    let insert = |entity| Command::InsertComponent {
        entity,
        component: scalar_id,
        fields: vec![],
        adopt: false,
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
    let id = host
        .create_world(Default::default(), SPATIAL_SYSTEMS)
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let create = |alias| Command::Create {
        alias,
        metadata: EntityMetadata::default(),
        adopt: false,
    };
    let insert = |entity| Command::InsertComponent {
        entity,
        component: crate::ComponentValue::TRANSFORM,
        fields: vec![],
        adopt: false,
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

#[test]
fn failed_component_reservation_keeps_prior_deletion_fences_complete() {
    let mut host = crate::HostRuntime::new();
    let id = host
        .create_world(
            WorldLimits::default(),
            &[
                crate::systems::constraints::ConstraintSystem::ID,
                crate::systems::hierarchy::HierarchySystem::ID,
            ],
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::SCALAR,
                    fields: Vec::new(),
                    adopt: false,
                },
                Command::Create {
                    alias: 2,
                    metadata: Default::default(),
                    adopt: false,
                },
            ],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    let first = world.entities()[0].id;
    let second = world.entities()[1].id;
    crate::components::storage::fail_next_reservation();
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![
                Command::Delete {
                    entity: EntityRef::Handle(first),
                },
                Command::InsertComponent {
                    entity: EntityRef::Handle(second),
                    component: ComponentValue::TRANSFORM,
                    fields: Vec::new(),
                    adopt: false,
                },
            ],
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    assert_eq!(
        report.outcomes[0].result.as_ref().unwrap_err().reason,
        ErrorReason::Capacity
    );
    assert!(world.entities().iter().all(|entity| entity.id != first));
    assert!(
        world
            .world
            .components
            .scalar(first.index() as usize)
            .is_none()
    );
    assert!(
        world
            .world
            .components
            .transform(second.index() as usize)
            .is_none()
    );
    assert_eq!(world.entities()[0].id, second);
    assert!(world.entities()[0].components.is_empty());
}

#[test]
fn empty_worlds_do_not_allocate_component_pages_until_their_first_value() {
    let mut host = crate::HostRuntime::new();
    let worlds: Vec<_> = (0..64)
        .map(|_| {
            // The constraints System admits the Scalar inserted below.
            host.create_world(
                WorldLimits::default(),
                &[crate::systems::constraints::ConstraintSystem::ID],
            )
            .unwrap()
        })
        .collect();
    for id in &worlds {
        let mut world = host.world_mut(*id).unwrap();
        world
            .enqueue(Batch {
                id: 1,
                operations: vec![Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                }],
            })
            .unwrap();
        assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
        assert_eq!(
            world
                .world
                .components
                .allocated_pages(ComponentValue::SCALAR),
            0
        );
        assert_eq!(
            world
                .world
                .components
                .allocated_pages(ComponentValue::TRANSFORM),
            0
        );
    }
    let mut world = host.world_mut(worlds[0]).unwrap();
    let entity = world.entities()[0].id;
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::InsertComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::SCALAR,
                fields: Vec::new(),
                adopt: false,
            }],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    assert_eq!(world.entities()[0].components.len(), 1);
    assert_eq!(
        world
            .world
            .components
            .allocated_pages(ComponentValue::SCALAR),
        1
    );
    assert_eq!(
        world
            .world
            .components
            .allocated_pages(ComponentValue::TRANSFORM),
        0
    );
    let original = world
        .world
        .components
        .scalar(entity.index() as usize)
        .unwrap() as *const _;
    world
        .world
        .components
        .try_reserve_component(ComponentValue::SCALAR, 4096)
        .unwrap();
    let after = world
        .world
        .components
        .scalar(entity.index() as usize)
        .unwrap() as *const _;
    assert_eq!(original, after);
}
