use super::*;

#[test]
fn resolved_asset_cache_checks_source_type_variant_entity_and_slot_generation() {
    use crate::services::asset_management::AssetSource;

    let mut host = crate::HostRuntime::new();
    let world_id = host.create_world(Default::default()).unwrap();
    let source = AssetSource {
        kind: crate::MESH_TYPE,
        uri: "archive!/mesh?name=opaque text".into(),
        variant: 0,
    };
    let first = host
        .asset_resources_mut()
        .get_or_create(source.clone())
        .unwrap();
    let mut variant = source.clone();
    variant.variant = 1;
    let other = host.asset_resources_mut().get_or_create(variant).unwrap();
    let entity = EntityId::new(3, 1);
    let replacement_entity = EntityId::new(3, 2);
    let cache = AssetSourceKeyCache::default();
    assert!(cache.borrow().is_empty());
    {
        let assets = host.asset_resources();
        assert_eq!(
            cached_source_key(
                assets,
                world_id,
                &cache,
                entity,
                source.kind,
                &source.uri,
                0
            ),
            Some(first)
        );
        assert_eq!(
            cached_source_key(
                assets,
                world_id,
                &cache,
                entity,
                source.kind,
                &source.uri,
                0
            ),
            Some(first)
        );
        assert_eq!(cache.borrow()[3], Some((entity, first)));
        assert_eq!(
            cached_source_key(
                assets,
                world_id,
                &cache,
                replacement_entity,
                source.kind,
                &source.uri,
                0
            ),
            Some(first)
        );
        assert_eq!(cache.borrow()[3], Some((replacement_entity, first)));
        assert_eq!(
            cached_source_key(
                assets,
                world_id,
                &cache,
                replacement_entity,
                source.kind,
                &source.uri,
                1
            ),
            Some(other)
        );
        assert_eq!(
            cached_source_key(
                assets,
                world_id,
                &cache,
                replacement_entity,
                super::super::geometry::GEOMETRY_TYPE,
                &source.uri,
                1
            ),
            None
        );
        assert_eq!(cache.borrow()[3], None);
    }

    host.asset_resources_mut().unload(first);
    {
        let assets = host.asset_resources();
        assert_eq!(
            cached_source_key(
                assets,
                world_id,
                &cache,
                entity,
                source.kind,
                &source.uri,
                0
            ),
            Some(first)
        );
        assert!(assets.get(first).unwrap().data().is_none());
    }
    host.asset_resources_mut().free_unused();
    host.flush_resource_lifecycle();
    host.asset_resources_mut()
        .get_or_create(AssetSource {
            uri: "archive!/fill-the-neighbor-slot".into(),
            ..source.clone()
        })
        .unwrap();
    let replacement = host
        .asset_resources_mut()
        .get_or_create(source.clone())
        .unwrap();
    assert_eq!(first.slot, replacement.slot);
    assert_ne!(first.generation, replacement.generation);
    {
        let world = host.world_mut(world_id).unwrap();
        assert_eq!(
            cached_source_key(
                world.asset_resources(),
                world.id(),
                &cache,
                entity,
                source.kind,
                &source.uri,
                0
            ),
            Some(replacement)
        );
        assert_eq!(cache.borrow()[3], Some((entity, replacement)));
        assert_eq!(
            cached_source_key(
                world.asset_resources(),
                world.id(),
                &cache,
                entity,
                source.kind,
                "archive!/changed",
                0
            ),
            None
        );
        assert_eq!(cache.borrow()[3], None);
    }
}

#[test]
fn resolved_asset_cache_preserves_exact_producer_namespace() {
    use crate::services::asset_management::AssetSource;

    let mut host = crate::HostRuntime::new();
    let world_id = host.create_world(Default::default()).unwrap();
    let path = "1/7";
    let canonical = host
        .asset_resources_mut()
        .get_or_create(AssetSource {
            kind: crate::MESH_TYPE,
            uri: format!("producer://{}/{path}", world_id.0),
            variant: 0,
        })
        .unwrap();
    let foreign = host
        .asset_resources_mut()
        .get_or_create(AssetSource {
            kind: crate::MESH_TYPE,
            uri: format!("producer://{}/{path}", world_id.0 + 1),
            variant: 0,
        })
        .unwrap();
    let world = host.world_mut(world_id).unwrap();
    for previous in [None, Some(canonical), Some(foreign)] {
        assert_eq!(
            crate::systems::asset_dependencies::source_key_from_fields(
                world.asset_resources(),
                world.id(),
                previous,
                crate::MESH_TYPE,
                "asset://1/7",
                0
            ),
            Some(canonical)
        );
    }
    assert_eq!(
        crate::systems::asset_dependencies::source_key_from_fields(
            world.asset_resources(),
            world.id(),
            Some(canonical),
            crate::MESH_TYPE,
            "asset://1/70",
            0
        ),
        None
    );
}
