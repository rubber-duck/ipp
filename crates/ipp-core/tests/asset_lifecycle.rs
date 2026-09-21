//! Real slot-backed assets and generic asynchronous data flow.
use ipp_core::services::{asset_management::*, data_source::*};
use std::{
    any::Any,
    cell::Cell,
    collections::BTreeSet,
    rc::Rc,
    task::{Context, Waker},
};

struct Blob(Vec<u8>);

impl Asset for Blob {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        self.0.len()
    }
}

fn blob_loader() -> impl AssetLoader<Data = Blob> {
    BufferedAssetLoader::new(|bytes| Ok(Blob(bytes.to_vec())))
}

struct GraphicsBlob {
    bytes: Vec<u8>,
    graphics_bytes: usize,
}

impl Asset for GraphicsBlob {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    fn invalidate_graphics(&mut self) {
        self.graphics_bytes = 0;
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.graphics_bytes > 0)
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(self.graphics_bytes)
    }

    fn resident_bytes(&self) -> usize {
        self.bytes.len() + self.graphics_bytes
    }
}

fn graphics_blob_loader() -> impl AssetLoader<Data = GraphicsBlob> {
    BufferedAssetLoader::new(|bytes| {
        Ok(GraphicsBlob {
            bytes: bytes.to_vec(),
            graphics_bytes: 16,
        })
    })
}

fn fixture() -> (AssetManagementService, DataSourceManagementService) {
    let mut assets = AssetManagementService::empty();
    // Retention mechanics need an explicit budget now that the Host default
    // evicts unused resources to preserve lifecycle retirement.
    assets.set_idle_resident_bytes_target(usize::MAX);
    assets
        .register_loader(AssetTypeId(42), blob_loader)
        .unwrap();
    let mut data = DataSourceManagementService::new();
    assets.install_data_sources(&mut data).unwrap();
    data.register_stream("http:").unwrap();
    (assets, data)
}

#[test]
fn idle_cache_defaults_to_eviction_and_remains_host_configurable() {
    let mut assets = AssetManagementService::empty();
    assert_eq!(assets.idle_resident_bytes_target(), 0);

    assets.set_idle_resident_bytes_target(3 * 1024 * 1024 * 1024);
    assert_eq!(assets.idle_resident_bytes_target(), 3 * 1024 * 1024 * 1024);
}

fn source(uri: &str) -> AssetSource {
    AssetSource {
        kind: AssetTypeId(42),
        uri: uri.into(),
        variant: 0,
    }
}

fn poll(assets: &mut AssetManagementService, data: &mut DataSourceManagementService) {
    data.progress();
    assets.poll_loads(data, &mut Context::from_waker(Waker::noop()));
}

#[test]
fn detached_progress_runs_cpu_loaders_but_defers_graphics_owned_loaders() {
    let mut host = ipp_core::HostRuntime::new();
    let cpu_polls = Rc::new(Cell::new(0));
    let graphics_polls = Rc::new(Cell::new(0));
    host.asset_resources_mut()
        .register_loader(AssetTypeId(42), {
            let polls = cpu_polls.clone();
            move || {
                let polls = polls.clone();
                BufferedAssetLoader::new(move |bytes| {
                    polls.set(polls.get() + 1);
                    Ok(Blob(bytes.to_vec()))
                })
            }
        })
        .unwrap();
    host.asset_resources_mut()
        .register_graphics_loader(AssetTypeId(43), {
            let polls = graphics_polls.clone();
            move || {
                let polls = polls.clone();
                BufferedAssetLoader::new(move |bytes| {
                    polls.set(polls.get() + 1);
                    Ok(Blob(bytes.to_vec()))
                })
            }
        })
        .unwrap();
    let cpu = host
        .asset_resources_mut()
        .upload(
            AssetUploadIdentity {
                kind: AssetTypeId(42),
                asset: 1,
                variant: 0,
            },
            vec![1],
        )
        .unwrap();
    let graphics = host
        .asset_resources_mut()
        .upload(
            AssetUploadIdentity {
                kind: AssetTypeId(43),
                asset: 2,
                variant: 0,
            },
            vec![2],
        )
        .unwrap();

    for _ in 0..4 {
        host.progress_evaluation_assets();
    }

    assert_eq!(cpu_polls.get(), 1);
    assert_eq!(graphics_polls.get(), 0);
    assert!(host.asset_resources().get_typed::<Blob>(cpu).is_some());
    assert!(host.asset_resources().get_typed::<Blob>(graphics).is_none());

    host.progress_assets();

    assert_eq!(graphics_polls.get(), 1);
    assert!(host.asset_resources().get_typed::<Blob>(graphics).is_some());
}

#[test]
fn payload_is_unavailable_until_eof_and_recovery_keeps_the_key() {
    let (mut assets, mut data) = fixture();
    let key = assets
        .get_or_create(source("http://fixture/immutable"))
        .unwrap();
    assert_eq!(
        assets
            .get_or_create(source("http://fixture/immutable"))
            .unwrap(),
        key
    );
    assets.set_used(BTreeSet::from([key]));
    poll(&mut assets, &mut data);
    let first = data.take_requests().pop().unwrap();
    assert!(!first.recovery);
    assert!(data.input_chunk(first.id, b"first").unwrap());
    poll(&mut assets, &mut data);
    assert!(assets.get_typed::<Blob>(key).is_none());
    data.input_end(first.id, Ok(()));
    poll(&mut assets, &mut data);
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"first");
    assert_eq!(
        assets
            .take_events()
            .unwrap()
            .iter()
            .map(|event| event.status.clone())
            .collect::<Vec<_>>(),
        vec![
            AssetLoadStatus::Start,
            AssetLoadStatus::Progress {
                completed: 5,
                total: None
            },
            AssetLoadStatus::Loaded
        ]
    );

    assets.unload(key);
    assert!(assets.get_typed::<Blob>(key).is_none());
    assert_eq!(assets.get(key).unwrap().stats().resident_bytes, 0);
    poll(&mut assets, &mut data);
    let recovery = data.take_requests().pop().unwrap();
    assert!(recovery.recovery);
    assert_ne!(recovery.id, first.id);
    assert!(data.input_chunk(first.id, b"obsolete").unwrap());
    data.input_chunk(recovery.id, b"first").unwrap();
    data.input_end(recovery.id, Ok(()));
    poll(&mut assets, &mut data);
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"first");
    assets.free_unused();
    assert!(assets.get(key).is_some());
    assets.set_used(BTreeSet::new());
    assets.free_unused();
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"first");
    assets.set_idle_resident_bytes_target(0);
    assert!(assets.get(key).is_none());
}

fn load_external(
    assets: &mut AssetManagementService,
    data: &mut DataSourceManagementService,
    uri: &str,
    bytes: &[u8],
    used: &mut BTreeSet<AssetKey>,
) -> AssetKey {
    let key = assets.get_or_create(source(uri)).unwrap();
    used.insert(key);
    assets.set_used(used.clone());
    poll(assets, data);
    let request = data.take_requests().pop().unwrap();
    data.input_chunk(request.id, bytes).unwrap();
    data.input_end(request.id, Ok(()));
    poll(assets, data);
    key
}

#[test]
fn completed_external_resource_is_reused_after_an_idle_interval() {
    let (mut assets, mut data) = fixture();
    let mut used = BTreeSet::new();
    let key = load_external(
        &mut assets,
        &mut data,
        "http://fixture/warm",
        b"warm",
        &mut used,
    );

    used.remove(&key);
    assets.set_used(used.clone());
    assets.free_unused();
    assert_eq!(assets.idle_resident_bytes(), 4);
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"warm");

    used.insert(key);
    assets.set_used(used);
    poll(&mut assets, &mut data);
    assert!(data.take_requests().is_empty());
    assert_eq!(assets.idle_resident_bytes(), 0);
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"warm");
}

#[test]
fn idle_pressure_uses_size_weighted_recency_and_protects_active_data() {
    let (mut assets, mut data) = fixture();
    let mut used = BTreeSet::new();
    let older_large = load_external(
        &mut assets,
        &mut data,
        "http://fixture/older-large",
        &[1; 8],
        &mut used,
    );
    let active = load_external(
        &mut assets,
        &mut data,
        "http://fixture/active",
        &[2; 32],
        &mut used,
    );
    used.remove(&older_large);
    assets.set_used(used.clone());
    assets.free_unused();

    let newer_small = load_external(
        &mut assets,
        &mut data,
        "http://fixture/newer-small",
        &[3; 4],
        &mut used,
    );
    used.remove(&newer_small);
    assets.set_used(used);
    assets.free_unused();
    assert_eq!(assets.idle_resident_bytes(), 12);

    assets.set_idle_resident_bytes_target(4);
    assert!(assets.get(older_large).is_none());
    assert!(assets.get(newer_small).is_some());
    assert_eq!(assets.idle_resident_bytes(), 4);
    assert_eq!(assets.get_typed::<Blob>(active).unwrap().0, [2; 32]);

    assets.set_idle_resident_bytes_target(0);
    assert!(assets.get(newer_small).is_none());
    assert!(assets.get(active).is_some());
    assert_eq!(assets.resident_bytes(), 32);
}

#[test]
fn equal_size_idle_resources_evict_the_least_recently_used_first() {
    let (mut assets, mut data) = fixture();
    let mut used = BTreeSet::new();
    let older = load_external(
        &mut assets,
        &mut data,
        "http://fixture/older",
        &[1; 4],
        &mut used,
    );
    used.remove(&older);
    assets.set_used(used.clone());
    assets.free_unused();
    let newer = load_external(
        &mut assets,
        &mut data,
        "http://fixture/newer",
        &[2; 4],
        &mut used,
    );
    used.remove(&newer);
    assets.set_used(used);
    assets.free_unused();

    assets.set_idle_resident_bytes_target(4);
    assert!(assets.get(older).is_none());
    assert!(assets.get(newer).is_some());
}

#[test]
fn larger_idle_resource_can_outweigh_more_idle_age() {
    let (mut assets, mut data) = fixture();
    let mut used = BTreeSet::new();
    let older_small = load_external(
        &mut assets,
        &mut data,
        "http://fixture/older-small",
        &[1; 2],
        &mut used,
    );
    used.remove(&older_small);
    assets.set_used(used.clone());
    assets.free_unused();
    let newer_large = load_external(
        &mut assets,
        &mut data,
        "http://fixture/newer-large",
        &[2; 10],
        &mut used,
    );
    used.remove(&newer_large);
    assets.set_used(used);
    assets.free_unused();

    assets.set_idle_resident_bytes_target(2);
    assert!(assets.get(older_small).is_some());
    assert!(assets.get(newer_large).is_none());
}

#[test]
fn promoting_cached_external_content_to_world_ownership_protects_it() {
    let (mut assets, mut data) = fixture();
    let mut used = BTreeSet::new();
    let source = source("http://fixture/promoted");
    let key = load_external(&mut assets, &mut data, &source.uri, b"promoted", &mut used);
    used.clear();
    assets.set_used(used);
    assets.free_unused();
    assert_eq!(assets.idle_resident_bytes(), 8);

    assets
        .retain_prepared_source(ipp_core::WorldId(1), &source)
        .unwrap();
    assert_eq!(assets.idle_resident_bytes(), 0);
    assets.set_idle_resident_bytes_target(0);
    assert!(assets.get(key).is_some());
    assets.release_client_source(ipp_core::WorldId(1), &source);
    assert!(assets.get(key).is_none());
}

#[test]
fn cached_external_identity_can_become_a_standalone_producer_upload() {
    let (mut assets, mut data) = fixture();
    data.register_stream("asset:").unwrap();
    let identity = AssetUploadIdentity {
        kind: AssetTypeId(42),
        asset: 7,
        variant: 0,
    };
    let source = source("asset://42/7");
    let key = assets.get_or_create(source.clone()).unwrap();
    assets.set_used(BTreeSet::from([key]));
    poll(&mut assets, &mut data);
    let request = data.take_requests().pop().unwrap();
    data.input_chunk(request.id, b"external").unwrap();
    data.input_end(request.id, Ok(()));
    poll(&mut assets, &mut data);
    assets.set_used(BTreeSet::new());
    assets.free_unused();
    assert_eq!(assets.find(&source), Some(key));

    assert_eq!(assets.upload(identity, b"producer".to_vec()).unwrap(), key);
    assert_eq!(assets.idle_resident_bytes(), 0);
    poll(&mut assets, &mut data);
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"producer");
}

#[test]
fn cached_external_identity_can_become_a_producer_upload_across_host_barriers() {
    let mut host = ipp_core::HostRuntime::new();
    host.asset_resources_mut()
        .set_idle_resident_bytes_target(usize::MAX);
    host.asset_resources_mut()
        .register_loader(AssetTypeId(42), blob_loader)
        .unwrap();
    host.register_stream_resource_provider("asset").unwrap();
    let identity = AssetUploadIdentity {
        kind: AssetTypeId(42),
        asset: 8,
        variant: 0,
    };
    let source = source("asset://42/8");
    let key = host
        .asset_resources_mut()
        .get_or_create(source.clone())
        .unwrap();
    host.asset_resources_mut().set_used(BTreeSet::from([key]));
    host.progress_assets();
    let request = host.take_resource_requests().pop().unwrap();
    host.complete_resource(request.id, Ok(b"external".to_vec()))
        .unwrap();
    host.progress_assets();
    host.asset_resources_mut().set_used(BTreeSet::new());
    host.asset_resources_mut().free_unused();
    assert_eq!(host.asset_resources().find(&source), Some(key));

    assert_eq!(
        host.asset_resources_mut()
            .upload(identity, b"producer".to_vec())
            .unwrap(),
        key
    );
    host.flush_resource_lifecycle();
    host.progress_assets();
    assert_eq!(
        host.asset_resources().get_typed::<Blob>(key).unwrap().0,
        b"producer"
    );
}

#[test]
fn explicit_unload_of_idle_content_does_not_restart_acquisition() {
    let (mut assets, mut data) = fixture();
    let mut used = BTreeSet::new();
    let key = load_external(
        &mut assets,
        &mut data,
        "http://fixture/unload-idle",
        b"idle",
        &mut used,
    );
    used.clear();
    assets.set_used(used);
    assets.free_unused();

    assets.unload(key);
    assert!(assets.get(key).is_none());
    poll(&mut assets, &mut data);
    assert!(data.take_requests().is_empty());
}

#[test]
fn idle_accounting_refreshes_when_graphics_residency_is_invalidated() {
    let (mut assets, mut data) = fixture();
    assets
        .register_loader(AssetTypeId(43), graphics_blob_loader)
        .unwrap();
    let source = AssetSource {
        kind: AssetTypeId(43),
        uri: "http://fixture/graphics-idle".into(),
        variant: 0,
    };
    let key = assets.get_or_create(source).unwrap();
    assets.set_used(BTreeSet::from([key]));
    poll(&mut assets, &mut data);
    let request = data.take_requests().pop().unwrap();
    data.input_chunk(request.id, b"cpu").unwrap();
    data.input_end(request.id, Ok(()));
    poll(&mut assets, &mut data);
    assets.set_used(BTreeSet::new());
    assets.free_unused();
    assert_eq!(assets.idle_resident_bytes(), 19);

    assets.invalidate_graphics(key);
    assert_eq!(assets.idle_resident_bytes(), 3);
    assert_eq!(assets.get(key).unwrap().graphics_bytes(), Some(0));
}

#[test]
fn input_backpressure_and_reader_drop_bound_staging() {
    let (mut assets, mut data) = fixture();
    let key = assets
        .get_or_create(source("http://fixture/bounded"))
        .unwrap();
    poll(&mut assets, &mut data);
    let request = data.take_requests().pop().unwrap();
    assert!(
        data.input_chunk(request.id, &vec![7; STREAM_CAPACITY])
            .unwrap()
    );
    assert!(!data.input_chunk(request.id, &[8]).unwrap());
    assert_eq!(data.input_bytes(), STREAM_CAPACITY);
    assets.free_unused();
    assert!(assets.get(key).is_none());
    assert_eq!(data.input_bytes(), 0);
    assert_eq!(data.take_cancellations(), vec![request.id]);
    assert!(data.input_chunk(request.id, b"stale").unwrap());
}

#[test]
fn slot_reuse_invalidates_old_keys_and_duplicate_release_is_harmless() {
    let (mut assets, _) = fixture();
    let first = assets
        .get_or_create(source("http://fixture/first"))
        .unwrap();
    assets.free_unused();
    assets.release(first);
    let replacement = assets
        .get_or_create(source("http://fixture/replacement"))
        .unwrap();
    assert_eq!(first.slot, replacement.slot);
    assert_ne!(first.generation, replacement.generation);
    assert!(assets.get(first).is_none());
    assert!(assets.get_typed::<Blob>(first).is_none());
    assert!(assets.get(replacement).is_some());
    assets.unload(first);
    assert!(assets.get(replacement).is_some());
}

#[test]
fn slot_growth_preserves_loaded_payload_addresses() {
    let (mut assets, mut data) = fixture();
    let key = assets
        .upload(
            AssetUploadIdentity {
                kind: AssetTypeId(42),
                asset: 7,
                variant: 0,
            },
            b"stable".to_vec(),
        )
        .unwrap();
    poll(&mut assets, &mut data);
    let address = assets.get_typed::<Blob>(key).unwrap() as *const Blob;
    for index in 0..1000 {
        assets
            .get_or_create(source(&format!("http://fixture/{index}")))
            .unwrap();
    }
    assert_eq!(
        assets.get_typed::<Blob>(key).unwrap() as *const Blob,
        address
    );
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"stable");
}

#[test]
fn producer_uploads_preserve_preexisting_references_and_explicit_retention() {
    let (mut assets, mut data) = fixture();
    let identity = AssetUploadIdentity {
        kind: AssetTypeId(42),
        asset: 7,
        variant: 3,
    };
    let key = assets
        .get_or_create(AssetSource {
            variant: 3,
            ..source("asset://42/7")
        })
        .unwrap();
    assets.set_used(BTreeSet::from([key]));
    assert_eq!(assets.upload(identity, b"immutable".to_vec()).unwrap(), key);
    poll(&mut assets, &mut data);
    assert!(assets.upload(identity, b"changed".to_vec()).is_err());
    assets.unload(key);
    poll(&mut assets, &mut data);
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"immutable");
    assets.release(key);
    assert!(assets.get(key).is_some());
    assets.set_used(BTreeSet::new());
    assets.free_unused();
    assert!(assets.get(key).is_none());
}

#[test]
fn typed_source_variants_and_producer_namespaces_do_not_alias() {
    let (mut assets, mut data) = fixture();
    assets
        .register_loader(AssetTypeId(43), blob_loader)
        .unwrap();
    let mut keys = BTreeSet::new();
    for kind in [42, 43] {
        for variant in [0, 1] {
            for producer in [1, 2] {
                let key = assets
                    .get_or_create(AssetSource {
                        kind: AssetTypeId(kind),
                        uri: format!("producer://{producer}/{kind}/1"),
                        variant,
                    })
                    .unwrap();
                assert!(keys.insert(key));
            }
        }
    }
    assert_eq!(keys.len(), 8);
    poll(&mut assets, &mut data);
    assert!(
        keys.into_iter()
            .all(|key| assets.get_typed::<Blob>(key).is_none())
    );
}

#[test]
fn source_preflight_validates_without_allocating() {
    let (mut assets, _) = fixture();
    let key = assets
        .upload(
            AssetUploadIdentity {
                kind: AssetTypeId(42),
                asset: 7,
                variant: 0,
            },
            b"owned".to_vec(),
        )
        .unwrap();
    let demand = BTreeSet::from([source("http://fixture/new")]);
    assert!(assets.validate_sources(&demand).is_ok());
    assert!(assets.find(demand.first().unwrap()).is_none());
    assert!(assets.take_events().unwrap().is_empty());
    assets.release(key);
    assert!(assets.validate_sources(&demand).is_ok());
}

#[test]
fn recovery_cannot_silently_bind_to_a_replacement_source_registration() {
    let (mut assets, mut data) = fixture();
    let key = assets
        .get_or_create(source("http://fixture/immutable"))
        .unwrap();
    assets.set_used(BTreeSet::from([key]));
    poll(&mut assets, &mut data);
    let first = data.take_requests().pop().unwrap();
    data.input_chunk(first.id, b"first").unwrap();
    data.input_end(first.id, Ok(()));
    poll(&mut assets, &mut data);
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"first");
    assets.unload(key);
    assert!(data.unregister("http:"));
    data.register_stream("http:").unwrap();
    poll(&mut assets, &mut data);
    assert!(
        matches!(assets.get(key).unwrap().status(), AssetLoadStatus::Failed(error) if error.contains("registration changed"))
    );
    assert!(assets.get_typed::<Blob>(key).is_none());
    assert!(data.take_requests().is_empty());
}

#[test]
fn named_client_sources_prepare_without_components_and_survive_owner_release() {
    let (mut assets, mut data) = fixture();
    let world = ipp_core::WorldId(1);
    let source = source("client://7/scene/glow#one");
    assets
        .register_client_source(world, source.clone(), b"original".to_vec())
        .unwrap();
    let key = assets.find(&source).unwrap();
    assert_eq!(
        assets.get(key).unwrap().status(),
        &AssetLoadStatus::Unloaded
    );
    poll(&mut assets, &mut data);
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"original");

    assets.set_used(BTreeSet::from([key]));
    assets.release_client_source(world, &source);
    assert!(
        assets
            .register_client_source(world, source.clone(), b"replacement".to_vec())
            .is_err()
    );
    assets.unload(key);
    poll(&mut assets, &mut data);
    assert_eq!(assets.find(&source), Some(key));
    assert_eq!(assets.get_typed::<Blob>(key).unwrap().0, b"original");

    assets.set_used(BTreeSet::new());
    assets.free_unused();
    assert!(assets.find(&source).is_none());
}

#[test]
fn named_source_ownership_is_shared_and_revision_identifiers_are_literal() {
    let (mut assets, mut data) = fixture();
    let first = source("client://7/scene/glow#one");
    let second = source("client://7/scene/glow#two");
    assets
        .register_client_source(ipp_core::WorldId(1), first.clone(), vec![1])
        .unwrap();
    assets
        .register_client_source(ipp_core::WorldId(1), second.clone(), vec![2])
        .unwrap();
    assets
        .retain_prepared_source(ipp_core::WorldId(2), &first)
        .unwrap();
    assets.release_client_source(ipp_core::WorldId(1), &first);
    poll(&mut assets, &mut data);
    let a = assets.find(&first).unwrap();
    let b = assets.find(&second).unwrap();
    assert_ne!(a, b);
    assert_eq!(assets.get_typed::<Blob>(a).unwrap().0, [1]);
    assert_eq!(assets.get_typed::<Blob>(b).unwrap().0, [2]);
    assets.release_client_source(ipp_core::WorldId(2), &first);
    assert!(assets.find(&first).is_none());
    assert!(assets.find(&second).is_some());
}

#[test]
fn shared_world_ownership_survives_one_teardown_and_private_content_dies_with_the_last() {
    let mut host = ipp_core::HostRuntime::new();
    host.asset_resources_mut()
        .register_loader(AssetTypeId(42), blob_loader)
        .unwrap();
    let first_world = host.create_world(Default::default()).unwrap();
    let second_world = host.create_world(Default::default()).unwrap();
    let source = source("client://shared/private");
    host.asset_resources_mut()
        .register_client_source(first_world, source.clone(), b"private".to_vec())
        .unwrap();
    host.asset_resources_mut()
        .retain_prepared_source(second_world, &source)
        .unwrap();
    host.progress_assets();
    let key = host.asset_resources().find(&source).unwrap();

    assert!(host.destroy_world(first_world));
    assert_eq!(
        host.asset_resources().get_typed::<Blob>(key).unwrap().0,
        b"private"
    );
    assert!(host.destroy_world(second_world));
    assert!(host.asset_resources().get(key).is_none());
}

#[test]
fn private_preparation_reclaimed_before_the_release_barrier_preserves_identity() {
    let mut host = ipp_core::HostRuntime::new();
    host.asset_resources_mut()
        .register_loader(AssetTypeId(42), blob_loader)
        .unwrap();
    let world = host.create_world(Default::default()).unwrap();
    let source = source("ipp-render://fixture/recipe");
    host.asset_resources_mut()
        .prepare_internal_sources(world, [(source.clone(), vec![7])])
        .unwrap();
    host.progress_assets();
    let key = host.asset_resources().find(&source).unwrap();
    assert!(
        host.world_mut(world)
            .unwrap()
            .resource_snapshots()
            .is_empty()
    );
    host.asset_resources_mut()
        .prepare_internal_sources(world, [])
        .unwrap();
    host.asset_resources_mut()
        .prepare_internal_sources(world, [(source.clone(), vec![7])])
        .unwrap();
    host.progress_assets();
    assert_eq!(host.asset_resources().find(&source), Some(key));
    assert_eq!(
        host.asset_resources().get_typed::<Blob>(key).unwrap().0,
        [7]
    );
    host.destroy_world(world);
    host.progress_assets();
    assert!(host.asset_resources().find(&source).is_none());
}

#[test]
fn headless_shader_sources_are_retained_without_claiming_gpu_readiness() {
    use shader::{SHADER_TYPE, ShaderBackendSource, ShaderDefinition};
    let mut host = ipp_core::HostRuntime::new();
    let world = host.create_world(Default::default()).unwrap();
    let source = AssetSource {
        kind: SHADER_TYPE,
        uri: "client://1/scene/glow#guid".into(),
        variant: 0,
    };
    let definition = ShaderDefinition {
        backends: [(
            "glsl-es-300".into(),
            ShaderBackendSource {
                vertex: String::new(),
                fragment: "vec4 materialFragment() { return vec4(1); }".into(),
            },
        )]
        .into(),
        ..Default::default()
    };
    host.asset_resources_mut()
        .register_client_source(world, source.clone(), definition.encode().unwrap())
        .unwrap();
    host.progress_assets();
    let key = host.asset_resources().find(&source).unwrap();
    assert!(
        matches!(host.asset_resources().get(key).unwrap().status(), AssetLoadStatus::Failed(error) if error.contains("rendering Host"))
    );
    assert!(host.asset_resources().get(key).unwrap().data().is_none());
    assert!(
        host.asset_resources()
            .get(key)
            .unwrap()
            .stats()
            .source_bytes
            > 0
    );
    host.asset_resources_mut().unload(key);
    host.progress_assets();
    assert_eq!(host.asset_resources().find(&source), Some(key));
}
