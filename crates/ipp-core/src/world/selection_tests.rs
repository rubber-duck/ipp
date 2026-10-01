use super::*;

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
