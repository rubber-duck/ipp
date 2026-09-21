use crate::world::*;
use crate::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

#[test]
fn sparse_updates_and_fallback_transitions_preserve_unaffected_occupied_storage() {
    let mut world_host = crate::HostRuntime::new();
    let world_id = world_host
        .create_world(crate::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let scalar = ComponentValue::SCALAR;
    world
        .enqueue(Batch {
            id: 0,
            operations: vec![
                Command::CreateStateOverlayOwner {
                    alias: 0,
                },
                Command::AttachEntityOverlayBinding {
                    owner: StateOverlayRef::Alias(0),
                    alias: 1,
                    symbolic_id: "owned".into(),
                    mode: EntityOverlayMode::Owned,
                },
                Command::AttachComponentStateOverlay {
                    owner: StateOverlayRef::Alias(0),
                    binding: StateOverlayRef::Alias(1),
                    alias: 2,
                    component: scalar,
                    mode: ComponentOverlayMode::Auto,
                    fields: vec![],
                },
            ],
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let resources = &report.outcomes[0].state_overlays;
    let owner = resources[0].id;
    let overlay = resources[2].id;
    let entity = resources[2].entity.unwrap();
    let address = world
        .world
        .components
        .scalar(entity.index() as usize)
        .unwrap() as *const Scalar;

    let mut operations = vec![Command::UpdateComponentStateOverlay {
        owner: StateOverlayRef::Handle(owner),
        overlay: StateOverlayRef::Handle(overlay),
        fields: vec![FieldWrite {
            offset: std::mem::offset_of!(Scalar, value) as u32,
            value: FieldValue::F32(7.0),
        }],
        clear: vec![],
    }];
    for alias in 0..100 {
        operations.push(Command::Create {
            alias,
            metadata: EntityMetadata::default(),
        });
        operations.push(Command::InsertComponent {
            entity: EntityRef::Alias(alias),
            component: scalar,
            fields: vec![],
        });
    }
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap();
    assert_eq!(
        address,
        world
            .world
            .components
            .scalar(entity.index() as usize)
            .unwrap() as *const Scalar
    );
    assert_eq!(
        world
            .world
            .components
            .scalar(entity.index() as usize)
            .unwrap()
            .value,
        7.0
    );

    let neighbor = world
        .world
        .state
        .entities
        .keys()
        .copied()
        .find(|&id| id != entity)
        .unwrap();
    let neighbor_address = world
        .world
        .components
        .scalar(neighbor.index() as usize)
        .unwrap() as *const Scalar;
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![
                Command::InsertComponent {
                    entity: EntityRef::Handle(entity),
                    component: scalar,
                    fields: vec![],
                },
                Command::RemoveComponent {
                    entity: EntityRef::Handle(entity),
                    component: scalar,
                },
            ],
        })
        .unwrap();
    world.step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap();
    assert_eq!(
        neighbor_address,
        world
            .world
            .components
            .scalar(neighbor.index() as usize)
            .unwrap() as *const Scalar
    );
    assert_eq!(
        world
            .world
            .components
            .scalar(entity.index() as usize)
            .unwrap()
            .value,
        7.0
    );
}
