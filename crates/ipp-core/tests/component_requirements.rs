//! Required defaults share component ownership across mutation and persistence.

mod support;

use ipp_core::{
    Batch, Command, ComponentOverlayMode, ComponentValue, EntityId, EntityOverlayMode, EntityRef,
    FieldValue, FieldWrite, HostRuntime, StateOverlayRef, WorldContext,
    components::{BoundingGeometry, MeshInstance},
};
use support::WorldTestDriver;

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
    Command::InsertComponentValue {
        entity: EntityRef::Handle(entity),
        value,
    }
}

fn remove(entity: EntityId, component: u16) -> Command {
    Command::RemoveComponent {
        entity: EntityRef::Handle(entity),
        component,
    }
}

#[test]
fn renderable_defaults_follow_authored_replacement_removal_and_restoration() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let entity;
    {
        let mut world = host.world_mut(id).unwrap();
        entity = apply(
            &mut world,
            vec![Command::Create {
                alias: 0,
                metadata: Default::default(),
            }],
        )[0]
        .1;
        apply(
            &mut world,
            vec![insert(
                entity,
                ComponentValue::MeshInstance(MeshInstance::default()),
            )],
        );
        let snapshot = world.inspect(entity).unwrap();
        assert!(
            snapshot
                .effective
                .iter()
                .any(|v| matches!(v, ComponentValue::BoundingGeometry(_)))
        );
        assert!(
            !snapshot
                .base
                .iter()
                .any(|v| matches!(v, ComponentValue::BoundingGeometry(_)))
        );
        apply(
            &mut world,
            vec![insert(
                entity,
                ComponentValue::BoundingGeometry(BoundingGeometry {
                    is_rendered: true,
                    ..Default::default()
                }),
            )],
        );
        apply(
            &mut world,
            vec![remove(entity, ComponentValue::BOUNDING_GEOMETRY)],
        );
        let snapshot = world.inspect(entity).unwrap();
        assert!(
            snapshot
                .effective
                .iter()
                .any(|v| matches!(v, ComponentValue::BoundingGeometry(b) if !b.is_rendered))
        );
        apply(
            &mut world,
            vec![remove(entity, ComponentValue::MESH_INSTANCE)],
        );
        assert!(
            !world
                .inspect(entity)
                .unwrap()
                .effective
                .iter()
                .any(|v| matches!(v, ComponentValue::BoundingGeometry(_)))
        );
        apply(
            &mut world,
            vec![insert(
                entity,
                ComponentValue::MeshInstance(MeshInstance::default()),
            )],
        );
    }
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
        .unwrap();
    let mut world = host.world_mut(restored).unwrap();
    world.update_for_test(0.0).unwrap();
    let snapshot = world.entities().remove(0);
    assert!(
        snapshot
            .effective
            .iter()
            .any(|v| matches!(v, ComponentValue::BoundingGeometry(_))),
        "restoration must rebuild requirements"
    );
    assert!(
        !snapshot
            .base
            .iter()
            .any(|v| matches!(v, ComponentValue::BoundingGeometry(_)))
    );
    let entity = snapshot.id;
    apply(
        &mut world,
        vec![
            insert(
                entity,
                ComponentValue::BoundingGeometry(BoundingGeometry {
                    is_rendered: true,
                    ..Default::default()
                }),
            ),
            remove(entity, ComponentValue::MESH_INSTANCE),
        ],
    );
    assert!(
        world
            .inspect(entity)
            .unwrap()
            .base
            .iter()
            .any(|v| matches!(v, ComponentValue::BoundingGeometry(b) if b.is_rendered))
    );
}

#[cfg(feature = "particles")]
#[test]
fn multiple_presentation_dependencies_share_one_fallback() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entity = apply(
        &mut world,
        vec![Command::Create {
            alias: 0,
            metadata: Default::default(),
        }],
    )[0]
    .1;
    apply(
        &mut world,
        vec![
            insert(
                entity,
                ComponentValue::MeshInstance(MeshInstance::default()),
            ),
            insert(entity, ComponentValue::ParticleSprite(Default::default())),
        ],
    );
    apply(
        &mut world,
        vec![remove(entity, ComponentValue::MESH_INSTANCE)],
    );
    assert!(
        world
            .inspect(entity)
            .unwrap()
            .effective
            .iter()
            .any(|v| matches!(v, ComponentValue::BoundingGeometry(_)))
    );
    apply(
        &mut world,
        vec![remove(entity, ComponentValue::PARTICLE_SPRITE)],
    );
    assert!(
        !world
            .inspect(entity)
            .unwrap()
            .effective
            .iter()
            .any(|v| matches!(v, ComponentValue::BoundingGeometry(_)))
    );
}

fn declare(
    world: &mut WorldContext<'_>,
    component: u16,
    mode: ComponentOverlayMode,
    fields: Vec<FieldWrite>,
) -> u64 {
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![
                Command::CreateStateOverlayOwner {
                    alias: 0,
                },
                Command::AttachEntityOverlayBinding {
                    owner: StateOverlayRef::Alias(0),
                    alias: 1,
                    symbolic_id: "target".into(),
                    mode: EntityOverlayMode::Bound,
                },
                Command::AttachComponentStateOverlay {
                    owner: StateOverlayRef::Alias(0),
                    binding: StateOverlayRef::Alias(1),
                    alias: 2,
                    component,
                    mode,
                    fields,
                },
            ],
        })
        .unwrap();
    let outcome = world.update_for_test(0.0).unwrap().outcomes.remove(0);
    outcome.result.unwrap();
    outcome.state_overlays[0].id
}

fn release(world: &mut WorldContext<'_>, owner: u64) {
    apply(
        world,
        vec![Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Handle(owner),
        }],
    );
}

fn bounds(world: &WorldContext<'_>, entity: EntityId) -> Option<bool> {
    world
        .inspect(entity)
        .unwrap()
        .effective
        .into_iter()
        .find_map(|value| {
            if let ComponentValue::BoundingGeometry(value) = value {
                Some(value.is_rendered)
            } else {
                None
            }
        })
}

#[test]
fn required_defaults_share_auto_demand_and_invalidate_strict_bindings() {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entity = apply(
        &mut world,
        vec![Command::Create {
            alias: 0,
            metadata: ipp_core::EntityMetadata {
                symbolic_id: Some("target".into()),
                classes: Vec::new(),
            },
        }],
    )[0]
    .1;
    let visible = || {
        vec![FieldWrite {
            offset: std::mem::offset_of!(BoundingGeometry, is_rendered) as u32,
            value: FieldValue::Bool(true),
        }]
    };

    // Both producer modes imply the dependency. An independent Auto declaration
    // shares its fallback and keeps it alive when the presentation goes away.
    for mode in [ComponentOverlayMode::Auto, ComponentOverlayMode::Owned] {
        let mesh = declare(&mut world, ComponentValue::MESH_INSTANCE, mode, Vec::new());
        assert_eq!(bounds(&world, entity), Some(false));
        let automatic = declare(
            &mut world,
            ComponentValue::BOUNDING_GEOMETRY,
            ComponentOverlayMode::Auto,
            visible(),
        );
        assert_eq!(bounds(&world, entity), Some(true));
        release(&mut world, mesh);
        assert_eq!(bounds(&world, entity), Some(true));
        release(&mut world, automatic);
        assert_eq!(bounds(&world, entity), None);
    }

    let mesh = declare(
        &mut world,
        ComponentValue::MESH_INSTANCE,
        ComponentOverlayMode::Auto,
        Vec::new(),
    );
    let bound = declare(
        &mut world,
        ComponentValue::BOUNDING_GEOMETRY,
        ComponentOverlayMode::Bound,
        visible(),
    );
    assert_eq!(bounds(&world, entity), Some(true));
    release(&mut world, mesh);
    assert_eq!(bounds(&world, entity), None);
    let mesh = declare(
        &mut world,
        ComponentValue::MESH_INSTANCE,
        ComponentOverlayMode::Auto,
        Vec::new(),
    );
    assert_eq!(
        bounds(&world, entity),
        Some(false),
        "strict declaration must not rebind"
    );
    release(&mut world, bound);
    release(&mut world, mesh);
    assert_eq!(bounds(&world, entity), None);
}
