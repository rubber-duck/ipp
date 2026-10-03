use super::super::AssetReleaseKind;
use super::*;
use crate::services::io::IoService;
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    task::{Context, Waker},
};

struct Blob(Vec<u8>);

impl super::super::Asset for Blob {
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

struct GraphicsBlob {
    bytes: Vec<u8>,
    graphics_resident: bool,
}

impl super::super::Asset for GraphicsBlob {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    fn invalidate_graphics(&mut self) {
        self.graphics_resident = false;
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.graphics_resident)
    }

    fn resident_bytes(&self) -> usize {
        self.bytes.len() + usize::from(self.graphics_resident)
    }
}

fn fixture() -> (AssetManagementService, IoService) {
    let mut assets = AssetManagementService::empty();
    assets
        .register_loader(super::super::AssetTypeId(42), || {
            super::super::BufferedAssetLoader::new(|bytes| Ok(Blob(bytes.to_vec())))
        })
        .unwrap();
    assets
        .register_loader(super::super::AssetTypeId(43), || {
            super::super::BufferedAssetLoader::new(|bytes| {
                Ok(GraphicsBlob {
                    bytes: bytes.to_vec(),
                    graphics_resident: true,
                })
            })
        })
        .unwrap();
    let mut data = IoService::new();
    assets.install_io_sources(&mut data).unwrap();
    data.register_stream("fixture:").unwrap();
    (assets, data)
}

fn source(uri: &str) -> super::super::AssetSource {
    super::super::AssetSource {
        kind: super::super::AssetTypeId(42),
        uri: uri.into(),
        variant: 7,
    }
}

fn poll(assets: &mut AssetManagementService, data: &mut IoService) {
    data.progress();
    assets.poll_loads(data, &mut Context::from_waker(Waker::noop()));
}

fn loaded_bytes(assets: &AssetManagementService, key: AssetKey) -> &[u8] {
    &assets.get_typed::<Blob>(key).unwrap().0
}

fn finish_pending(assets: &mut AssetManagementService) -> Vec<AssetLifecycleEvent> {
    let events: Vec<_> = assets
        .pending_releases()
        .into_iter()
        .map(|(_, event)| event)
        .collect();
    for event in &events {
        assets.finish_release(event);
    }
    events
}

#[test]
fn retiring_publication_strengthens_pending_release_before_storage_can_be_freed() {
    for weak_release in [AssetReleaseKind::Graphics, AssetReleaseKind::Unload] {
        let (mut assets, mut data) = fixture();
        let owner = crate::WorldId(9);
        let source = source("fixture:escalation");
        assets
            .register_client_source(owner, source.clone(), b"old".to_vec())
            .unwrap();
        let key = assets.find(&source).unwrap();
        poll(&mut assets, &mut data);
        let publication = assets.retain_publication([key]).unwrap();
        assets.release_client_source(owner, &source);
        assets.take_lifecycle_events();

        match weak_release {
            AssetReleaseKind::Graphics => assets.invalidate_graphics(key),
            AssetReleaseKind::Unload => assets.unload(key),
            _ => unreachable!(),
        }
        let old_event = assets.pending_releases()[0].1.clone();
        assert_eq!(assets.affected_publications(&old_event), vec![publication]);
        assert!(assets.release_publication(publication));
        assert_eq!(assets.pending_releases()[0].0, AssetReleaseKind::Remove);
        assert!(assets.retain_publication([key]).is_err());

        assert!(!assets.finish_release(&old_event));
        assert_eq!(loaded_bytes(&assets, key), b"old");
        assert_eq!(assets.pending_releases()[0].0, AssetReleaseKind::Remove);
        assert!(assets.take_lifecycle_events().is_empty());

        let strong_event = assets.pending_releases()[0].1.clone();
        assert_eq!(strong_event.kind, super::super::AssetLifecycleKind::Removed);
        assert!(assets.finish_release(&strong_event));
        assert!(assets.get(key).is_none());
        let committed = assets.take_lifecycle_events();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].kind, super::super::AssetLifecycleKind::Removed);
        assert_eq!(committed[0].key, key);

        assets
            .register_client_source(owner, source.clone(), b"new".to_vec())
            .unwrap();
        let replacement = assets.find(&source).unwrap();
        assert_ne!(replacement, key);
        assert!(!assets.finish_release(&old_event));
        assert!(!assets.finish_release(&strong_event));
        poll(&mut assets, &mut data);
        assert_eq!(loaded_bytes(&assets, replacement), b"new");
    }
}

#[test]
fn explicit_unload_of_idle_external_content_cannot_be_canceled_by_a_new_publication() {
    let (mut assets, mut data) = fixture();
    assets.set_idle_resident_bytes_target(usize::MAX);
    assets.require_lifecycle_barrier();
    let source = source("fixture:idle-unload");
    let key = assets.get_or_create(source).unwrap();
    assets.set_used(BTreeSet::from([key]));
    poll(&mut assets, &mut data);
    let request = data.take_requests().pop().unwrap();
    data.input_chunk(request.id, b"idle").unwrap();
    data.input_end(request.id, Ok(()));
    poll(&mut assets, &mut data);
    assets.take_lifecycle_events();
    assets.set_used(BTreeSet::new());
    assets.free_unused();
    assert_eq!(assets.idle_resident_bytes(), 4);

    assets.unload(key);
    let pending = assets.pending_releases();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, AssetReleaseKind::Revoke);
    assert_eq!(pending[0].1.kind, super::super::AssetLifecycleKind::Removed);
    assert!(assets.retain_publication([key]).is_err());
    assert_eq!(assets.pending_releases()[0].0, AssetReleaseKind::Revoke);
    assert_eq!(loaded_bytes(&assets, key), b"idle");

    assert!(assets.finish_release(&pending[0].1));
    assert!(assets.get(key).is_none());
    let committed = assets.take_lifecycle_events();
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].kind, super::super::AssetLifecycleKind::Removed);
}

#[test]
fn two_publications_retain_owned_cpu_content_after_world_demand_and_producer_release() {
    let (mut assets, mut data) = fixture();
    let producer = crate::WorldId(1);
    let sibling = crate::WorldId(2);
    let source = source("fixture:owned");
    assets
        .register_client_source(producer, source.clone(), b"original".to_vec())
        .unwrap();
    let key = assets.find(&source).unwrap();
    let demand =
        super::super::service::AssetDemandSelection::new(source.kind, &source.uri, source.variant);
    assets.update_user_deltas(producer, BTreeMap::from([(demand.clone(), true)]));
    assets.update_user_deltas(sibling, BTreeMap::from([(demand, true)]));
    poll(&mut assets, &mut data);
    assert_eq!(loaded_bytes(&assets, key), b"original");

    let first = assets.retain_publication([key]).unwrap();
    let second = assets.retain_publication([key]).unwrap();
    assets.release_world(producer);
    assets.release_world(sibling);
    assert!(assets.pending_releases().is_empty());
    assert_eq!(loaded_bytes(&assets, key), b"original");
    assert_eq!(
        assets.publication_resource(first, key).unwrap().source(),
        &source
    );
    assert_eq!(
        assets.publication_resource(second, key).unwrap().source(),
        &source
    );

    assert!(assets.release_publication(first));
    assert!(!assets.release_publication(first));
    assert_eq!(loaded_bytes(&assets, key), b"original");
    assert!(assets.release_publication(second));
    let pending = assets.pending_releases();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, AssetReleaseKind::Remove);
    assert!(assets.get(key).is_some());
    assert!(assets.affected_publications(&pending[0].1).is_empty());
    finish_pending(&mut assets);
    assert!(assets.get(key).is_none());
}

#[test]
fn unload_invalidates_all_publications_before_cpu_release_and_recovers_same_input() {
    let (mut assets, mut data) = fixture();
    let owner = crate::WorldId(3);
    let source = source("fixture:recover");
    assets
        .register_client_source(owner, source.clone(), b"same".to_vec())
        .unwrap();
    let key = assets.find(&source).unwrap();
    poll(&mut assets, &mut data);
    let first = assets.retain_publication([key]).unwrap();
    let second = assets.retain_publication([key]).unwrap();
    assets.release_client_source(owner, &source);

    assets.unload(key);
    let pending = assets.pending_releases();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, AssetReleaseKind::Unload);
    assert_eq!(
        assets.affected_publications(&pending[0].1),
        vec![first, second]
    );
    assert_eq!(loaded_bytes(&assets, key), b"same");
    assert!(assets.publication_resource(first, key).is_none());
    assert!(assets.retain_publication([key]).is_err());
    finish_pending(&mut assets);
    assert!(assets.get_typed::<Blob>(key).is_none());
    assert!(assets.get(key).is_some());

    poll(&mut assets, &mut data);
    assert_eq!(loaded_bytes(&assets, key), b"same");
    assert_eq!(assets.publication_resource(second, key).unwrap().key(), key);
    assert!(assets.release_publication(first));
    assert!(assets.release_publication(second));
    finish_pending(&mut assets);
}

#[test]
fn graphics_release_does_not_discard_publication_cpu_data() {
    let (mut assets, mut data) = fixture();
    let owner = crate::WorldId(4);
    let mut source = source("fixture:graphics");
    source.kind = super::super::AssetTypeId(43);
    assets
        .register_client_source(owner, source.clone(), b"pixels".to_vec())
        .unwrap();
    let key = assets.find(&source).unwrap();
    poll(&mut assets, &mut data);
    let publication = assets.retain_publication([key]).unwrap();
    assets.release_client_source(owner, &source);

    assets.invalidate_graphics(key);
    let pending = assets.pending_releases();
    assert_eq!(pending[0].0, AssetReleaseKind::Graphics);
    assert_eq!(
        assets.affected_publications(&pending[0].1),
        vec![publication]
    );
    assert!(
        assets
            .get_typed::<GraphicsBlob>(key)
            .unwrap()
            .graphics_resident
    );
    finish_pending(&mut assets);
    let resource = assets.publication_resource(publication, key).unwrap();
    assert_eq!(resource.key(), key);
    assert_eq!(resource.graphics_ready(), Some(false));
    assert_eq!(
        assets.get_typed::<GraphicsBlob>(key).unwrap().bytes,
        b"pixels"
    );
}

#[test]
fn explicit_revocation_fences_old_leases_and_same_named_replacement() {
    let (mut assets, mut data) = fixture();
    let descriptor = source("fixture:replace");
    let key = assets.get_or_create(descriptor.clone()).unwrap();
    assets.set_used(BTreeSet::from([key]));
    poll(&mut assets, &mut data);
    let request = data.take_requests().pop().unwrap();
    data.input_chunk(request.id, b"old").unwrap();
    data.input_end(request.id, Ok(()));
    poll(&mut assets, &mut data);
    let publication = assets.retain_publication([key]).unwrap();
    assets.set_used(BTreeSet::new());

    assets.revoke_source_prefix("fixture:");
    let pending = assets.pending_releases();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, AssetReleaseKind::Revoke);
    assert_eq!(
        assets.affected_publications(&pending[0].1),
        vec![publication]
    );
    assert_eq!(loaded_bytes(&assets, key), b"old");
    assert!(assets.publication_resource(publication, key).is_none());
    assert!(
        assets
            .register_client_source(crate::WorldId(7), descriptor.clone(), b"new".to_vec())
            .is_err()
    );
    finish_pending(&mut assets);
    assert!(assets.get(key).is_none());
    assert!(data.unregister("fixture:"));
    data.register_stream("fixture:").unwrap();
    assert!(assets.release_publication(publication));

    let replacement = assets.get_or_create(descriptor).unwrap();
    assert_ne!(replacement, key);
    assert_eq!(replacement.slot, key.slot);
    assert!(
        assets
            .publication_resource(publication, replacement)
            .is_none()
    );
    assert!(!assets.release_publication(publication));
    let (mut other, mut other_data) = fixture();
    let other_source = source("fixture:other");
    other
        .register_client_source(crate::WorldId(8), other_source.clone(), b"other".to_vec())
        .unwrap();
    let other_key = other.find(&other_source).unwrap();
    poll(&mut other, &mut other_data);
    assert_eq!(other_key, key);
    assert!(other.publication_resource(publication, other_key).is_none());
    assert!(!other.release_publication(publication));
}

#[test]
fn publication_acquisition_cancels_only_ordinary_orphan_removal() {
    let (mut assets, mut data) = fixture();
    assets.require_lifecycle_barrier();
    let source = source("fixture:late-publication");
    let key = assets
        .upload_named(source.clone(), b"ready".to_vec())
        .unwrap();
    assert!(assets.retain_publication([key]).is_err());
    poll(&mut assets, &mut data);
    assets.release(key);
    assert_eq!(assets.pending_releases()[0].0, AssetReleaseKind::Remove);

    let publication = assets.retain_publication([key]).unwrap();
    assert!(assets.pending_releases().is_empty());
    assert_eq!(loaded_bytes(&assets, key), b"ready");
    assets.revoke_resource(key);
    assert!(assets.retain_publication([key]).is_err());
    assert_eq!(assets.pending_releases()[0].0, AssetReleaseKind::Revoke);
    assert_eq!(
        assets.affected_publications(&assets.pending_releases()[0].1),
        vec![publication]
    );
}

#[test]
fn session_cleanup_revokes_retained_owned_input_after_ordinary_owner_release() {
    let (mut assets, mut data) = fixture();
    let owner = crate::WorldId(5);
    let source = source("fixture:session-private");
    assets
        .register_client_source(owner, source.clone(), b"private".to_vec())
        .unwrap();
    let key = assets.find(&source).unwrap();
    poll(&mut assets, &mut data);
    let publication = assets.retain_publication([key]).unwrap();
    assets.release_world(owner);
    assert_eq!(loaded_bytes(&assets, key), b"private");

    assets.revoke_resource(key);
    let pending = assets.pending_releases();
    assert_eq!(pending[0].0, AssetReleaseKind::Revoke);
    assert_eq!(
        assets.affected_publications(&pending[0].1),
        vec![publication]
    );
    assert_eq!(loaded_bytes(&assets, key), b"private");
    finish_pending(&mut assets);
    assert!(assets.get(key).is_none());
    assert!(assets.publication_resource(publication, key).is_none());
}

#[test]
fn replaced_external_registration_cannot_recover_old_publication() {
    let (mut assets, mut data) = fixture();
    let source = source("fixture:registration");
    let key = assets.get_or_create(source).unwrap();
    assets.set_used(BTreeSet::from([key]));
    poll(&mut assets, &mut data);
    let request = data.take_requests().pop().unwrap();
    data.input_chunk(request.id, b"first").unwrap();
    data.input_end(request.id, Ok(()));
    poll(&mut assets, &mut data);
    let publication = assets.retain_publication([key]).unwrap();
    assets.set_used(BTreeSet::new());

    assets.unload(key);
    let event = assets.pending_releases()[0].1.clone();
    assert_eq!(assets.affected_publications(&event), vec![publication]);
    assets.finish_release(&event);
    assert!(data.unregister("fixture:"));
    data.register_stream("fixture:").unwrap();
    poll(&mut assets, &mut data);
    assert!(
        matches!(assets.get(key).unwrap().status(), super::super::AssetLoadStatus::Failed(message) if message.contains("registration changed"))
    );
    assert!(assets.get_typed::<Blob>(key).is_none());
    assert!(
        assets
            .publication_resource(publication, key)
            .unwrap()
            .data()
            .is_none()
    );
}
