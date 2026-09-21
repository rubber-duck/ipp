//! Resource release invalidates only draw rows that reference the released identity.
#![cfg(feature = "builtin-assets")]

use ipp_core::{components::MeshInstance, services::asset_management::AssetKey, *};

fn host_with_meshes() -> HostRuntime {
    let mut host = HostRuntime::new();
    let memory = services::data_source::MemoryDataSource::default();
    for width in [1, 2] {
        memory
            .insert(
                format!("fixture:cube{width}"),
                services::asset_management::builtin::mesh(&format!(
                    "ipp://mesh/cube?width={width}&height=1&length=1"
                ))
                .unwrap(),
            )
            .unwrap();
    }
    host.data_sources_mut()
        .register("fixture:", memory)
        .unwrap();
    host
}

fn add_cube(host: &mut HostRuntime, world: WorldId, width: u32) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
    }];
    for component in [
        ComponentValue::TRANSFORM,
        ComponentValue::UNLIT_MATERIAL,
        ComponentValue::LIGHT,
    ] {
        operations.push(Command::InsertComponent {
            entity: EntityRef::Alias(0),
            component,
            fields: Vec::new(),
        });
    }
    operations.push(Command::InsertComponent {
        entity: EntityRef::Alias(0),
        component: ComponentValue::MESH_INSTANCE,
        fields: vec![FieldWrite {
            offset: std::mem::offset_of!(MeshInstance, source) as u32,
            value: FieldValue::String(format!("fixture:cube{width}")),
        }],
    });
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap()[0]
        .1
}

fn update_worlds(host: &mut HostRuntime, worlds: &[WorldId]) {
    for &world in worlds {
        host.world_mut(world).unwrap().prepare_update(0.0).unwrap();
    }
    host.progress_assets();
    for &world in worlds {
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    host.flush_resource_lifecycle();
}

#[test]
fn releasing_one_worlds_last_mesh_keeps_peer_prepared_output() {
    let mut host = host_with_meshes();
    let first = host.create_world(Default::default()).unwrap();
    let peer = host.create_world(Default::default()).unwrap();
    let entity = add_cube(&mut host, first, 1);
    add_cube(&mut host, peer, 2);
    for _ in 0..8 {
        update_worlds(&mut host, &[first, peer]);
    }
    let expected = host.world_mut(peer).unwrap().render_items().to_vec();
    assert_eq!(expected.len(), 1);
    assert_eq!(host.world_mut(peer).unwrap().light_items().count(), 1);
    let released = AssetKey::from_u64(host.world_mut(first).unwrap().render_items()[0].mesh.asset);
    host.world_mut(first)
        .unwrap()
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(entity),
            }],
        })
        .unwrap();
    update_worlds(&mut host, &[first, peer]);
    assert!(
        host.asset_resources().get(released).is_none(),
        "released content is evicted after final demand"
    );
    assert_eq!(
        host.world_mut(peer).unwrap().render_items(),
        expected,
        "an unrelated World release must preserve its peer's completed frame"
    );
    assert_eq!(host.world_mut(peer).unwrap().light_items().count(), 1);

    host.asset_resources_mut().set_idle_resident_bytes_target(0);
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(released).is_none());
    assert_eq!(
        host.world_mut(peer).unwrap().render_items(),
        expected,
        "idle pressure must preserve an unrelated World's completed frame"
    );
    assert_eq!(host.world_mut(peer).unwrap().light_items().count(), 1);
}

#[test]
fn payload_unload_withdraws_only_draws_using_that_resource() {
    let mut host = host_with_meshes();
    let world = host.create_world(Default::default()).unwrap();
    let first = add_cube(&mut host, world, 1);
    let second = add_cube(&mut host, world, 2);
    for _ in 0..8 {
        update_worlds(&mut host, &[world]);
    }
    let before = host.world_mut(world).unwrap().render_items().to_vec();
    assert_eq!(before.len(), 2);
    let released = AssetKey::from_u64(
        before
            .iter()
            .find(|item| item.entity == first)
            .unwrap()
            .mesh
            .asset,
    );
    let expected = vec![*before.iter().find(|item| item.entity == second).unwrap()];
    host.asset_resources_mut().unload(released);
    host.flush_resource_lifecycle();
    assert!(
        host.asset_resources()
            .get(released)
            .unwrap()
            .data()
            .is_none()
    );
    assert_eq!(host.world_mut(world).unwrap().render_items(), expected);
}
