//! Component validation supplements the real client/GL lighting scenarios.

mod support;
use support::WorldTestDriver;

use ipp_core::{
    Batch, Command, ComponentValue, EntityRef,
    components::{Light, Transform},
};

#[test]
fn invalid_light_batch_keeps_prior_components_and_light_order_is_stable() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let operations = |light: Light| {
        vec![
            Command::Create {
                alias: 0,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::Light(light),
            },
        ]
    };
    world
        .enqueue(Batch {
            id: 1,
            operations: operations(Light {
                range: -1.0,
                ..Light::default()
            }),
        })
        .unwrap();
    assert!(
        world.update_for_test(0.0).unwrap().outcomes[0]
            .result
            .is_err()
    );
    assert_eq!(world.entities().len(), 1);
    assert!(world.light_items().next().is_none());
    for id in 2..5 {
        world
            .enqueue(Batch {
                id,
                operations: operations(Light::default()),
            })
            .unwrap();
        assert!(
            world.update_for_test(0.0).unwrap().outcomes[0]
                .result
                .is_ok()
        );
    }
    let lights: Vec<_> = world.light_items().collect();
    assert_eq!(lights.len(), 3);
    assert!(lights.windows(2).all(|pair| pair[0].0 < pair[1].0));
    world
        .enqueue(Batch {
            id: 5,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(lights[1].0),
            }],
        })
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(
        world.light_items().map(|item| item.0).collect::<Vec<_>>(),
        vec![lights[0].0, lights[2].0]
    );
}
