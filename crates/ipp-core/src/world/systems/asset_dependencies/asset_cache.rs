use super::*;

pub(in crate::world) fn source_key_from_fields(
    assets: &AssetManagementService,
    world: crate::WorldId,
    previous: Option<crate::services::asset_management::AssetKey>,
    kind: crate::services::asset_management::AssetTypeId,
    source: &str,
    variant: u32,
) -> Option<crate::services::asset_management::AssetKey> {
    #[cfg(feature = "profiling")]
    let _allocation_scope = crate::profiling::AllocationScope::new(202, "assets.source_key");

    if let Some(key) = previous
        && let Some(provider) = assets.get(key)
    {
        let identity = provider.source();
        let same_source = if let Some(path) = source.strip_prefix("asset://") {
            identity
                .uri
                .strip_prefix("producer://")
                .and_then(|scoped| scoped.split_once('/'))
                .is_some_and(|(owner, tail)| {
                    owner.as_bytes().first().is_some_and(u8::is_ascii_digit)
                        && (owner.len() == 1 || !owner.starts_with('0'))
                        && owner.parse::<u64>() == Ok(world.0)
                        && tail == path
                })
        } else {
            identity.uri == source
        };
        if identity.kind == kind && identity.variant == variant && same_source {
            return Some(key);
        }
    }

    if crate::allocation_optimizations_enabled() {
        return assets.find_source(world, kind, source, variant);
    }

    let selection =
        crate::services::asset_management::service::AssetManagementService::scoped_selection(
            world,
            &AssetDemandSelection::new(kind, source, variant),
        );
    assets.find(&selection.descriptor())
}

pub(in crate::world) fn cached_source_key(
    assets: &AssetManagementService,
    world: crate::WorldId,
    cache: &AssetSourceKeyCache,
    entity: EntityId,
    kind: crate::services::asset_management::AssetTypeId,
    source: &str,
    variant: u32,
) -> Option<crate::services::asset_management::AssetKey> {
    let index = entity.index() as usize;
    // End every cache borrow before consulting other world/service state.
    let previous = cache
        .borrow()
        .get(index)
        .copied()
        .flatten()
        .filter(|&(owner, _)| owner == entity)
        .map(|(_, key)| key);
    let key = source_key_from_fields(assets, world, previous, kind, source, variant);
    if key == previous {
        return key;
    }
    let mut entries = cache.borrow_mut();
    if entries.len() <= index {
        entries.resize(index + 1, None);
    }
    entries[index] = key.map(|key| (entity, key));
    key
}
