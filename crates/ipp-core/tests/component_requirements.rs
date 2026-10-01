//! Required defaults are ordinary stored components across mutation and persistence.

mod support;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime, WorldContext,
    components::{BoundingGeometry, MeshInstance},
};
use support::WorldTestDriver;
use support::selection::RENDER;

fn apply(world: &mut WorldContext<'_>, operations: Vec<Command>) -> Vec<(u32, EntityId)> {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world
        .update_for_test(0.0)
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
}

fn insert(entity: EntityId, value: ComponentValue) -> Command {
    Command::insert_value(EntityRef::Handle(entity), value)
}

fn remove(entity: EntityId, component: u16) -> Command {
    Command::RemoveComponent {
        entity: EntityRef::Handle(entity),
        component,
    }
}

fn bounding(world: &ipp_core::WorldContext<'_>, entity: EntityId) -> Option<BoundingGeometry> {
    world
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::BoundingGeometry(value) => Some(value),
            _ => None,
        })
}

fn has_mesh(world: &ipp_core::WorldContext<'_>, entity: EntityId) -> bool {
    world
        .inspect(entity)
        .unwrap()
        .components
        .iter()
        .any(|value| matches!(value, ComponentValue::MeshInstance(_)))
}

#[test]
fn required_defaults_are_ordinary_components_that_stay_after_their_dependent_is_removed() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default(), RENDER).unwrap();
    let entity;
    let rendered = BoundingGeometry {
        is_rendered: true,
        ..Default::default()
    };
    {
        let mut world = host.world_mut(id).unwrap();
        entity = apply(
            &mut world,
            vec![Command::Create {
                alias: 0,
                metadata: Default::default(),
                adopt: false,
            }],
        )[0]
        .1;

        // Inserting a renderable inserts its required default as a stored component.
        apply(
            &mut world,
            vec![insert(
                entity,
                ComponentValue::MeshInstance(MeshInstance::default()),
            )],
        );
        assert_eq!(bounding(&world, entity), Some(BoundingGeometry::default()));
        assert!(!BoundingGeometry::default().is_rendered);

        // It is written like any component; removing it re-inserts the default
        // while its dependent remains.
        apply(
            &mut world,
            vec![insert(
                entity,
                ComponentValue::BoundingGeometry(rendered.clone()),
            )],
        );
        assert_eq!(bounding(&world, entity), Some(rendered.clone()));
        apply(
            &mut world,
            vec![remove(entity, ComponentValue::BOUNDING_GEOMETRY)],
        );
        assert_eq!(bounding(&world, entity), Some(BoundingGeometry::default()));

        // Removing the dependent leaves the required component in place.
        apply(
            &mut world,
            vec![
                insert(entity, ComponentValue::BoundingGeometry(rendered.clone())),
                remove(entity, ComponentValue::MESH_INSTANCE),
            ],
        );
        assert!(!has_mesh(&world, entity));
        assert_eq!(bounding(&world, entity), Some(rendered.clone()));
    }

    // Saves carry it as an ordinary component, and restoring the World keeps it
    // without its dependent.
    let bytes = host.save_world(id, 123, Default::default()).unwrap();
    assert!(host.destroy_world(id));
    let restored = host
        .load_world(
            &bytes,
            123,
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .unwrap()
        .root
        .id();
    let mut world = host.world_mut(restored).unwrap();
    world.update_for_test(0.0).unwrap();
    let entity = world.entities().remove(0).id;
    assert!(!has_mesh(&world, entity));
    assert_eq!(bounding(&world, entity), Some(rendered.clone()));

    // A dependent inserted again keeps the present value.
    apply(
        &mut world,
        vec![insert(
            entity,
            ComponentValue::MeshInstance(MeshInstance::default()),
        )],
    );
    assert!(has_mesh(&world, entity));
    assert_eq!(bounding(&world, entity), Some(rendered));
}
