//! Explicit root and retained-publication CPU queries over real completed Worlds.

mod support;

use support::selection::{ATTACHMENTS, CAMERA, GEOMETRY, select};

use ipp_core::components::{Camera, PickingGeometry, Transform};
use ipp_core::systems::geometry::{GeometryDefinition, GeometryShape};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, ErrorReason, FieldValue, FieldWrite,
    HostRuntime, OutputKind, OutputRef, ViewQueryTarget, WorldAttachment, WorldId, WorldPlane,
    WorldViewport,
};

fn viewport() -> WorldViewport {
    WorldViewport {
        width: 640,
        height: 480,
        device_pixel_ratio: 1.0,
    }
}

fn apply(host: &mut HostRuntime, world: WorldId, commands: Vec<Command>) -> Vec<(u32, EntityId)> {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: commands,
        })
        .unwrap();
    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
}

fn create(host: &mut HostRuntime, world: WorldId, values: Vec<ComponentValue>) -> EntityId {
    let mut commands = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    commands.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    apply(host, world, commands)[0].1
}

fn camera(host: &mut HostRuntime, world: WorldId, horizontal: f32) -> OutputRef {
    let entity = create(
        host,
        world,
        vec![
            ComponentValue::Camera(Camera {
                projection: 1,
                ortho_height: 4.0,
                ..Default::default()
            }),
            ComponentValue::Transform(Transform {
                x: horizontal,
                z: 5.0,
                ..Default::default()
            }),
        ],
    );
    host.bind_output(host.world_ref(world).unwrap(), entity, OutputKind::Camera)
        .unwrap()
}

fn root(output: OutputRef) -> ViewQueryTarget {
    ViewQueryTarget::RootView {
        output,
        expected_viewport: viewport(),
    }
}

fn plane() -> WorldPlane {
    WorldPlane {
        point: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
    }
}

#[test]
fn root_queries_use_exact_selection_and_viewport_not_legacy_camera_state() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), CAMERA).unwrap();
    let selected = camera(&mut host, world, 0.0);
    let other = camera(&mut host, world, 99.0);
    host.world_mut(world)
        .unwrap()
        .enqueue_camera_activate(other.camera_entity().unwrap())
        .unwrap();
    host.set_root_output(selected, viewport()).unwrap();
    host.frame(0.0).unwrap();
    assert_eq!(
        host.world_mut(world).unwrap().active_camera(),
        Some(other.camera_entity().unwrap())
    );

    let (descriptor, position) = host
        .project_view(root(selected), [1.0, 0.5], plane())
        .unwrap();
    assert_eq!(descriptor.output, selected);
    assert_eq!(descriptor.viewport, viewport());
    assert!((position.unwrap()[0] - 8.0 / 3.0).abs() < 1e-6);
    assert_eq!(
        host.resolve_view(root(other)),
        Err(ErrorReason::InvalidEntity)
    );
    assert_eq!(
        host.resolve_view(ViewQueryTarget::RootView {
            output: selected,
            expected_viewport: WorldViewport {
                width: 1,
                height: 1,
                ..viewport()
            },
        }),
        Err(ErrorReason::InvalidViewport)
    );
}

#[test]
fn cleared_root_requires_explicit_history_and_stale_output_never_rebinds() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), CAMERA).unwrap();
    let selected = camera(&mut host, world, 0.0);
    host.set_root_output(selected, viewport()).unwrap();
    host.frame(0.0).unwrap();
    let publication = host.resolve_view(root(selected)).unwrap().publication;
    let (identity, revision) = publication.identity();
    assert_eq!(
        host.resolve_publication_ref(identity, revision),
        Ok(publication)
    );
    assert_eq!(
        host.resolve_publication_ref(identity + 1, revision),
        Err(ErrorReason::InvalidEntity)
    );
    host.clear_root_output(world);
    assert_eq!(
        host.resolve_view(root(selected)),
        Err(ErrorReason::InvalidEntity)
    );

    let historical = ViewQueryTarget::PublicationView {
        output: selected,
        publication,
        viewport: WorldViewport {
            width: 1,
            height: 1,
            ..viewport()
        },
    };
    let (descriptor, position) = host.project_view(historical, [1.0, 0.5], plane()).unwrap();
    assert_eq!(descriptor.publication, publication);
    assert_eq!(position, Some([2.0, 0.0, 0.0]));
    assert!(host.root_output(world).is_none());

    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(selected.camera_entity().unwrap()),
                component: ComponentValue::CAMERA,
            },
            Command::insert_value(
                EntityRef::Handle(selected.camera_entity().unwrap()),
                ComponentValue::Camera(Camera::default()),
            ),
        ],
    );
    let replacement = host
        .bind_output(
            host.world_ref(world).unwrap(),
            selected.camera_entity().unwrap(),
            OutputKind::Camera,
        )
        .unwrap();
    assert_ne!(selected, replacement);
    assert_eq!(
        host.resolve_view(historical),
        Err(ErrorReason::InvalidEntity)
    );
    assert_eq!(
        host.resolve_view(root(replacement)),
        Err(ErrorReason::InvalidEntity)
    );
}

#[test]
fn picks_use_published_child_geometry_and_world_qualified_publication_identity() {
    let mut host = HostRuntime::new();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA]))
        .unwrap();
    let child = host.create_world(Default::default(), GEOMETRY).unwrap();
    let selected = camera(&mut host, parent, 0.0);
    let attachment = WorldAttachment::spatial(host.world_ref(child).unwrap());
    let anchor = create(
        &mut host,
        parent,
        vec![ComponentValue::WorldAttachment(attachment)],
    );
    let sphere = GeometryDefinition::from(GeometryShape::Sphere {
        center: [0.0; 3],
        radius: 1.0,
    })
    .encode()
    .unwrap();
    let entity = create(
        &mut host,
        child,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::PickingGeometry(PickingGeometry {
                geometry: sphere,
                ..Default::default()
            }),
        ],
    );
    host.set_root_output(selected, viewport()).unwrap();
    host.frame(0.0).unwrap();
    let published = host.latest_publication(child).unwrap();
    let (_, hit) = host.pick_view(root(selected), [0.5, 0.5], true).unwrap();
    let hit = hit.unwrap();
    assert_eq!(hit.identity.world, host.world_ref(child).unwrap());
    assert_eq!(hit.identity.entity, entity);
    assert_eq!(hit.identity.publication, published);
    assert!(hit.identity.incarnation > 0);
    assert_eq!(
        hit.identity.path,
        vec![(host.world_ref(parent).unwrap(), anchor)]
    );
    assert_eq!(hit.position, [0.0, 0.0, 1.0]);
    assert_eq!(hit.view_plane.unwrap().point, hit.position);

    apply(
        &mut host,
        child,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::TRANSFORM,
            field: FieldWrite {
                offset: std::mem::offset_of!(Transform, x) as u32,
                value: FieldValue::F32(100.0),
            },
        }],
    );
    assert_eq!(
        host.pick_view(root(selected), [0.5, 0.5], true).unwrap().1,
        None
    );
}
