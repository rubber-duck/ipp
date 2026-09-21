//! Surface rectangles participate in visual bounds without enabling picking.
#![cfg(feature = "surfaces")]

mod support;
use support::WorldTestDriver;

use ipp_core::{
    Batch, Command, ComponentValue, EntityRef, FieldValue, FieldWrite, HostRuntime, Surface,
    WorldLimits,
    components::{BoundingGeometry, MeshInstance, Transform},
    systems::{
        camera,
        geometry::{self, GeometryBounds, GeometryDefinition, GeometryShape},
    },
};

fn apply(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap();
}

#[test]
fn surface_bounds_follow_transform_and_dimension_changes_without_picking() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut mesh_bytes = b"IPPM".to_vec();
    for value in [3u32, 3, 3, 1] {
        mesh_bytes.extend(value.to_le_bytes());
    }
    mesh_bytes.extend([0, 1, 0, 0]);
    mesh_bytes.extend(36u32.to_le_bytes());
    for point in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in point {
            mesh_bytes.extend(value.to_le_bytes());
        }
    }
    for index in [0u16, 1, 2] {
        mesh_bytes.extend(index.to_le_bytes());
    }
    world
        .enqueue_mesh(ipp_core::MeshUpload {
            id: 1,
            key: ipp_core::MeshKey {
                asset: 1,
                variant: 0,
            },
            bytes: mesh_bytes,
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());

    let mut surface = Surface::default();
    surface.width = 4.0;
    surface.height = 2.0;
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Transform(Transform {
                    x: 3.0,
                    ..Default::default()
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(surface),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::MeshInstance(MeshInstance {
                    source: "asset://1/1".into(),
                    variant: 0,
                }),
            },
        ],
    );
    let entity = world.entities()[0].id;
    assert_eq!(world.bounding_geometry(entity).unwrap().parts.len(), 2);
    assert_eq!(
        world.bounding_geometry(entity).unwrap().bounds().unwrap(),
        [[1.0, -1.0, 0.0], [5.0, 1.0, 0.0]]
    );
    assert!(world.picking_geometry(entity).is_err());

    let planes = geometry::frustum_planes(
        camera::prepare(
            entity,
            &ipp_core::components::Camera {
                projection: 1,
                ortho_height: 4.0,
                ..Default::default()
            },
            &Transform {
                z: 6.0,
                ..Default::default()
            },
            100,
            100,
        )
        .unwrap()
        .view_projection,
    );
    assert!(world.geometry_visible(entity, &planes));

    apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::SURFACE,
            field: FieldWrite {
                offset: std::mem::offset_of!(Surface, width) as u32,
                value: FieldValue::F32(1.0),
            },
        }],
    );
    assert_eq!(
        world.bounding_geometry(entity).unwrap().bounds().unwrap(),
        [[2.5, -1.0, 0.0], [4.0, 1.0, 0.0]]
    );

    apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::TRANSFORM,
            field: FieldWrite {
                offset: std::mem::offset_of!(Transform, x) as u32,
                value: FieldValue::F32(100.0),
            },
        }],
    );
    assert!(!world.geometry_visible(entity, &planes));

    let authored = GeometryDefinition::from(GeometryShape::Box {
        min: [0.0, 0.0, 0.0],
        max: [1.0, 1.0, 0.0],
    })
    .encode()
    .unwrap();
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::BoundingGeometry(BoundingGeometry {
                geometry: authored,
                ..Default::default()
            }),
        }],
    );
    assert!(
        world.culling_geometry(entity).is_none(),
        "authored bounds that omit part of the Surface must not cull it"
    );
    assert_eq!(
        world.render_geometry(entity).mesh_bounds,
        Some([[99.5, -1.0, 0.0], [101.0, 1.0, 0.0]])
    );

    // Visual bounds must keep following all presentation components while the
    // independently authored culling shape stays unchanged.
    apply(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::SURFACE,
            field: FieldWrite {
                offset: std::mem::offset_of!(Surface, width) as u32,
                value: FieldValue::F32(4.0),
            },
        }],
    );
    assert_eq!(
        world.render_geometry(entity).mesh_bounds,
        Some([[98.0, -1.0, 0.0], [102.0, 1.0, 0.0]])
    );
    apply(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::SURFACE,
        }],
    );
    assert_eq!(
        world.render_geometry(entity).mesh_bounds,
        Some([[100.0, 0.0, 0.0], [101.0, 1.0, 0.0]])
    );
    assert!(world.culling_geometry(entity).is_some());
    apply(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::MESH_INSTANCE,
        }],
    );
    assert_eq!(world.render_geometry(entity).mesh_bounds, None);
    assert!(world.culling_geometry(entity).is_none());
}
