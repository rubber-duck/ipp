//! Component-owned geometry allocations survive ordinary evaluation and authored style edits.

mod support;
use support::WorldTestDriver;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, ErrorReason, FieldValue, FieldWrite,
    HostRuntime, WorldContext,
    components::schema::SchemaComponent,
    components::{BoundingGeometry, Transform},
    systems::geometry::{GeometryDefinition, GeometryShape},
};

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

fn create(world: &mut WorldContext<'_>, geometry: BoundingGeometry, x: f32) -> EntityId {
    apply(
        world,
        vec![
            Command::Create {
                alias: 0,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::Transform(Transform {
                    x,
                    ..Default::default()
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(0),
                value: ComponentValue::BoundingGeometry(geometry),
            },
        ],
    )[0]
    .1
}

fn declaration() -> BoundingGeometry {
    BoundingGeometry {
        geometry: GeometryDefinition::from(GeometryShape::Sphere {
            center: [0.0; 3],
            radius: 1.0,
        })
        .encode()
        .unwrap(),
        is_rendered: true,
        ..Default::default()
    }
}

#[test]
fn evaluated_geometry_reuses_storage_and_authored_copies_get_independent_results() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entity = create(&mut world, declaration(), 0.0);
    let allocation = world.bounding_geometry(entity).unwrap().parts.as_ptr();
    let authored = world
        .inspect(entity)
        .unwrap()
        .base
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::BoundingGeometry(value) => Some(value),
            _ => None,
        })
        .unwrap();
    assert_eq!(authored.fields(), declaration().fields());
    assert_eq!(authored.fields().len(), 11);

    let copy = create(&mut world, authored, 7.0);
    assert_ne!(
        world.bounding_geometry(copy).unwrap().parts.as_ptr(),
        allocation
    );
    assert_eq!(world.debug_render_items().len(), 2);
    assert_eq!(world.debug_render_items()[0].model[12], 0.0);
    assert_eq!(world.debug_render_items()[1].model[12], 7.0);

    for _ in 0..12 {
        world.update_for_test(0.0).unwrap();
        assert_eq!(
            world.bounding_geometry(entity).unwrap().parts.as_ptr(),
            allocation
        );
    }
    apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::BOUNDING_GEOMETRY,
            field: FieldWrite {
                offset: std::mem::offset_of!(BoundingGeometry, outline) as u32,
                value: FieldValue::Bool(true),
            },
        }],
    );
    assert_eq!(
        world.bounding_geometry(entity).unwrap().parts.as_ptr(),
        allocation
    );
    assert!(
        world
            .inspect(entity)
            .unwrap()
            .base
            .iter()
            .any(|value| matches!(value, ComponentValue::BoundingGeometry(value) if value.outline))
    );

    apply(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(entity),
        }],
    );
    let replacement = create(&mut world, declaration(), 3.0);
    assert_ne!(entity, replacement);
    assert_eq!(
        world.bounding_geometry(entity),
        Err(ErrorReason::MissingComponent)
    );
    assert!(world.bounding_geometry(replacement).is_ok());
    assert_eq!(world.debug_render_items().len(), 2);
}

#[test]
fn mesh_generated_geometry_reuses_its_component_allocation() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut bytes = b"IPPM".to_vec();
    for value in [3u32, 3, 3, 1] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 1, 0, 0]);
    bytes.extend(36u32.to_le_bytes());
    for point in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in point {
            bytes.extend(value.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2] {
        bytes.extend(index.to_le_bytes());
    }
    world
        .enqueue_mesh(ipp_core::MeshUpload {
            id: 1,
            key: ipp_core::MeshKey {
                asset: 1,
                variant: 0,
            },
            bytes,
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
    let entity = create(&mut world, BoundingGeometry::default(), 0.0);
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::MeshInstance(ipp_core::components::MeshInstance {
                source: "asset://1/1".into(),
                variant: 0,
            }),
        }],
    );
    let allocation = world.bounding_geometry(entity).unwrap().parts.as_ptr();
    for _ in 0..12 {
        world.update_for_test(0.0).unwrap();
        assert_eq!(
            world.bounding_geometry(entity).unwrap().parts.as_ptr(),
            allocation
        );
    }
}

#[test]
fn compound_geometry_grows_beyond_former_part_quota() {
    let part = GeometryShape::Sphere {
        center: [0.0; 3],
        radius: 1.0,
    }
    .into();
    let definition = GeometryDefinition {
        parts: vec![part; 300],
    };
    let bytes = definition.encode().unwrap();
    assert_eq!(GeometryDefinition::decode(&bytes).unwrap(), definition);
    let mut malformed = b"IPPG".to_vec();
    malformed.extend(1u32.to_le_bytes());
    malformed.extend(u32::MAX.to_le_bytes());
    assert!(GeometryDefinition::decode(&malformed).is_err());
}
