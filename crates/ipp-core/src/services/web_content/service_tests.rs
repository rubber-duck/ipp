use super::*;
use crate::{ComponentValue, EntityId, WorldId};
use std::collections::BTreeSet;

const DESCRIPTION: WebContentImageDescription = WebContentImageDescription {
    pixel_size: [16, 8],
};

fn target() -> WebContentTarget {
    WebContentTarget {
        world: WorldRef {
            id: WorldId(1),
            incarnation: 1,
        },
        canvas: CanvasTarget {
            entity: EntityId::new(0, 1),
            component: ComponentValue::CANVAS_HTML,
            incarnation: 1,
        },
    }
}

fn request(service: &mut WebContentService) -> WebContentRequestId {
    service.reconcile(target(), "page".into(), [16.0, 8.0]);
    service.requests().next().unwrap().id
}

fn complete(service: &mut WebContentService, request: WebContentRequestId) -> WebContentImageId {
    let image = service.reserve_image(request, DESCRIPTION).unwrap();
    service.complete_image(request, image).unwrap();
    image
}

#[test]
fn reservations_require_current_authority_and_never_resurrect_after_invalidation() {
    let mut service = WebContentService::default();
    service.register_source("page");
    let old = request(&mut service);
    let image = service.reserve_image(old, DESCRIPTION).unwrap();
    assert_eq!(
        service.reserve_image(old, DESCRIPTION),
        Err(WebContentError::ReservationPending)
    );
    service.renderer_invalidated();
    assert!(!service.is_image_retained(image));
    assert_eq!(
        service.complete_image(old, image),
        Err(WebContentError::StaleRequest)
    );
    assert_eq!(
        service.reserve_image(old, DESCRIPTION),
        Err(WebContentError::StaleRequest)
    );
    let next = service.requests().next().unwrap().id;
    assert_ne!(next, old);
    assert_ne!(complete(&mut service, next), image);
}

#[test]
fn publication_leases_pin_exact_versions_across_request_cancellation() {
    let mut service = WebContentService::default();
    service.register_source("page");
    let old = request(&mut service);
    let first = complete(&mut service, old);
    let lease = service.retain_publication(&BTreeSet::from([first]));
    let second = complete(&mut service, old);
    assert_eq!(service.publication_image(lease, first), Some(DESCRIPTION));
    assert_eq!(service.publication_image(lease, second), None);
    service.cancel_request(target());
    assert!(service.is_image_retained(first));
    assert!(!service.is_image_retained(second));
    assert_ne!(request(&mut service), old);
    service.release_publication(lease);
    assert!(!service.is_image_retained(first));
}

#[test]
fn replacing_registration_revokes_frozen_images_without_retargeting_old_owner() {
    let mut service = WebContentService::default();
    let source = service.register_source("page");
    let old = request(&mut service);
    let image = complete(&mut service, old);
    let lease = service.retain_publication(&BTreeSet::from([image]));
    let next = service.register_source("page");
    assert_ne!(next, source);
    assert!(!service.revoke_source(source));
    assert_eq!(service.publication_image(lease, image), None);
    assert_ne!(service.requests().next().unwrap().id, old);
    service.release_publication(lease);
}

#[test]
fn target_invalidation_revokes_all_versions_and_service_namespaces_do_not_collide() {
    let mut first = WebContentService::default();
    let mut second = WebContentService::default();
    first.register_source("page");
    second.register_source("page");
    let a = request(&mut first);
    let b = request(&mut second);
    assert_ne!(a, b);
    assert_eq!(
        first.reserve_image(b, DESCRIPTION),
        Err(WebContentError::StaleRequest)
    );
    let image = complete(&mut first, a);
    let lease = first.retain_publication(&BTreeSet::from([image]));
    first.invalidate_target(target());
    assert_eq!(first.publication_image(lease, image), None);
    assert!(first.requests().next().is_none());
}

#[test]
fn coalescing_and_failed_import_release_unpublished_metadata() {
    let mut service = WebContentService::default();
    service.register_source("page");
    let request = request(&mut service);
    let first = complete(&mut service, request);
    let second = complete(&mut service, request);
    assert!(!service.is_image_retained(first));
    assert!(service.is_image_retained(second));
    let staging = service.reserve_image(request, DESCRIPTION).unwrap();
    assert_eq!(service.image_description(staging), None);
    service.discard_image(staging);
    assert!(!service.is_image_retained(staging));
    service.discard_image(second);
    assert!(service.is_image_retained(second));
}

#[test]
fn late_registration_and_empty_extent_are_explicitly_unavailable() {
    let mut service = WebContentService::default();
    assert_eq!(
        service.reconcile(target(), "page".into(), [16.0, 8.0]),
        WebContentStatus::Unavailable
    );
    service.register_source("page");
    assert_eq!(service.status(target()), WebContentStatus::Pending);
    assert_eq!(
        service.reconcile(target(), "page".into(), [0.0, 8.0]),
        WebContentStatus::Unavailable
    );
    assert!(service.requests().next().is_none());
}

#[test]
fn validity_survives_retirement_and_authored_edits_until_target_invalidation() {
    let mut service = WebContentService::default();
    service.register_source("page");
    let old_request = request(&mut service);
    let first = complete(&mut service, old_request);
    let witness = service.image_validity(first).unwrap();
    let second = complete(&mut service, old_request);
    assert_eq!(witness, service.image_validity(second).unwrap());
    assert!(!service.is_image_retained(first));
    assert!(witness.is_valid());

    service.cancel_request(target());
    service.reconcile(target(), "page".into(), [32.0, 16.0]);
    assert!(!service.is_image_retained(second));
    let new_request = service.requests().next().unwrap().id;
    assert_ne!(old_request, new_request);
    let third = complete(&mut service, new_request);
    assert_eq!(witness, service.image_validity(third).unwrap());
    assert!(witness.is_valid());

    let clone = witness.clone();
    service.invalidate_target(target());
    assert!(!witness.is_valid());
    assert_eq!(clone, witness);
    let replacement_request = request(&mut service);
    let replacement = complete(&mut service, replacement_request);
    let replacement_witness = service.image_validity(replacement).unwrap();
    assert_ne!(replacement_witness, witness);
    assert!(replacement_witness.is_valid());
}

#[test]
fn source_revocation_reaches_retired_images_after_target_changes_source() {
    let mut service = WebContentService::default();
    let old_source = service.register_source("page");
    service.register_source("other");
    let old_request = request(&mut service);
    let first = complete(&mut service, old_request);
    let old_witness = service.image_validity(first).unwrap();

    service.reconcile(target(), "other".into(), [16.0, 8.0]);
    let new_request = service.requests().next().unwrap().id;
    let second = complete(&mut service, new_request);
    let new_witness = service.image_validity(second).unwrap();
    assert!(!service.is_image_retained(first));
    assert!(old_witness.is_valid());
    assert!(new_witness.is_valid());
    assert_ne!(old_witness, new_witness);

    service.revoke_source(old_source);
    assert!(!old_witness.is_valid());
    assert!(new_witness.is_valid());
    service.register_source("other");
    assert!(!new_witness.is_valid());
}

#[test]
fn renderer_invalidation_and_service_drop_revoke_retired_image_witnesses() {
    let mut service = WebContentService::default();
    service.register_source("page");
    let old_request = request(&mut service);
    let first = complete(&mut service, old_request);
    let old_witness = service.image_validity(first).unwrap();
    complete(&mut service, old_request);
    assert!(!service.is_image_retained(first));
    service.renderer_invalidated();
    assert!(!old_witness.is_valid());

    let new_request = service.requests().next().unwrap().id;
    let second = complete(&mut service, new_request);
    let new_witness = service.image_validity(second).unwrap();
    assert_ne!(old_witness, new_witness);
    assert!(new_witness.is_valid());
    complete(&mut service, new_request);
    assert!(!service.is_image_retained(second));
    drop(service);
    assert!(!new_witness.is_valid());
}

#[test]
fn world_invalidation_revokes_witnesses_after_the_last_image_retires() {
    let mut service = WebContentService::default();
    service.register_source("page");
    let request = request(&mut service);
    let image = complete(&mut service, request);
    let witness = service.image_validity(image).unwrap();
    service.cancel_request(target());
    assert!(!service.is_image_retained(image));
    assert!(witness.is_valid());
    service.invalidate_world(target().world);
    assert!(!witness.is_valid());
}

#[test]
fn validity_requires_a_completed_image_and_the_exact_publication_lease() {
    let mut service = WebContentService::default();
    service.register_source("page");
    let request = request(&mut service);
    let image = service.reserve_image(request, DESCRIPTION).unwrap();
    assert!(service.image_validity(image).is_none());
    service.complete_image(request, image).unwrap();
    let selected = service.retain_publication(&BTreeSet::from([image]));
    let other = service.retain_publication(&BTreeSet::new());
    let witness = service.publication_image_validity(selected, image).unwrap();
    assert!(service.publication_image_validity(other, image).is_none());
    service.release_publication(selected);
    service.cancel_request(target());
    assert!(
        service
            .publication_image_validity(selected, image)
            .is_none()
    );
    assert!(!service.is_image_retained(image));
    assert!(witness.is_valid());
    service.invalidate_target(target());
    assert!(!witness.is_valid());
}
