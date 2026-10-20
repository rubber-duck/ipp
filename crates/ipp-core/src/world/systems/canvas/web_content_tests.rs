//! Headless lifecycle/publication invariants. Real GPU/transport evidence belongs
//! to the maintained native and browser rendering scenarios.

use super::*;
use crate::services::web_content::{
    WebContentError, WebContentImageDescription, WebContentRequestId,
};
use crate::systems::canvas::{CanvasHtml, CanvasPublication, CanvasSystem};
use crate::{
    Batch, Command, EntityId, EntityRef, HostRuntime, OutputRef, WorldId, WorldLimits,
    WorldPublicationId,
};

fn frame(host: &mut HostRuntime) {
    let report = host.frame(0.0).unwrap();
    assert!(report.publication_errors.is_empty(), "{report:?}");
    for result in report.worlds.values() {
        let report = result.as_ref().unwrap();
        assert!(
            report.outcomes.iter().all(|outcome| outcome.result.is_ok()),
            "{report:?}"
        );
    }
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    frame(host);
}

fn fixture() -> (HostRuntime, WorldId, EntityId) {
    let mut host = HostRuntime::new();
    host.web_content_mut().register_source("page");
    let world = host
        .create_world(WorldLimits::default(), &[CanvasSystem::ID])
        .unwrap();
    apply(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::CanvasHtml(CanvasHtml {
                    source: "page".into(),
                    width: 16.0,
                    height: 8.0,
                }),
            ),
        ],
    );
    let entity = host
        .web_content()
        .requests()
        .next()
        .unwrap()
        .target
        .canvas
        .entity;
    (host, world, entity)
}

fn request(host: &HostRuntime) -> WebContentRequestId {
    host.web_content().requests().next().unwrap().id
}

fn complete(host: &mut HostRuntime) -> WebContentImageId {
    let request = request(host);
    let image = host
        .web_content_mut()
        .reserve_image(
            request,
            WebContentImageDescription {
                pixel_size: [16, 8],
            },
        )
        .unwrap();
    host.web_content_mut()
        .complete_image(request, image)
        .unwrap();
    image
}

fn canvas(host: &HostRuntime, world: WorldId) -> (WorldPublicationId, CanvasPublication) {
    let id = host.latest_publication(world).unwrap();
    let output = OutputRef::canvas(host.world_ref(world).unwrap());
    (
        id,
        host.output(id, output)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
            .clone(),
    )
}

#[test]
fn web_repaint_changes_exact_resource_without_layout_or_component_writes() {
    let (mut host, world, _) = fixture();
    let before = canvas(&host, world).1;
    let first = complete(&mut host);
    frame(&mut host);
    let (publication, painted) = canvas(&host, world);
    assert_eq!(images(&painted.entries), BTreeSet::from([first]));
    assert_eq!(painted.layout_revision, before.layout_revision);
    assert_ne!(painted.resource_revision, before.resource_revision);
    let reader = host.retain_publication_read(publication).unwrap();

    let second = complete(&mut host);
    assert_eq!(images(&painted.entries), BTreeSet::from([first]));
    frame(&mut host);
    let (next, repainted) = canvas(&host, world);
    assert_eq!(images(&repainted.entries), BTreeSet::from([second]));
    assert_eq!(repainted.layout_revision, painted.layout_revision);
    assert_ne!(repainted.resource_revision, painted.resource_revision);
    assert!(host.publication_web_image(publication, first).is_some());
    assert!(host.publication_web_image(publication, second).is_none());
    assert!(host.publication_web_image(next, first).is_none());
    let witness = host
        .publication_web_image_validity(publication, first)
        .unwrap();
    assert!(
        host.publication_web_image_validity(publication, second)
            .is_none()
    );
    assert!(host.publication_web_image_validity(next, first).is_none());
    drop(reader);
    frame(&mut host);
    assert!(!host.web_content().is_image_retained(first));
    assert!(witness.is_valid());
    host.web_content_mut().renderer_invalidated();
    assert!(!witness.is_valid());
}

#[test]
fn source_edits_cancel_requests_but_component_replacement_revokes_frozen_images() {
    let (mut host, world, entity) = fixture();
    let first = complete(&mut host);
    let old_request = request(&host);
    frame(&mut host);
    let (publication, _) = canvas(&host, world);
    let _reader = host.retain_publication_read(publication).unwrap();
    host.web_content_mut().register_source("other");
    apply(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CANVAS_HTML,
            field: crate::FieldWrite {
                offset: std::mem::offset_of!(CanvasHtml, source) as u32,
                value: crate::FieldValue::String("other".into()),
            },
        }],
    );
    assert_ne!(request(&host), old_request);
    assert!(host.publication_web_image(publication, first).is_some());
    assert_eq!(
        host.web_content_mut().reserve_image(
            old_request,
            WebContentImageDescription {
                pixel_size: [16, 8]
            }
        ),
        Err(WebContentError::StaleRequest)
    );
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CANVAS_HTML,
        }],
    );
    assert!(host.publication_web_image(publication, first).is_none());
}

#[test]
fn context_loss_revokes_frozen_images_and_active_world_publishes_fresh_identity() {
    let (mut host, world, _) = fixture();
    let first = complete(&mut host);
    frame(&mut host);
    let (publication, frozen) = canvas(&host, world);
    let _reader = host.retain_publication_read(publication).unwrap();
    let old_request = request(&host);
    host.web_content_mut().renderer_invalidated();
    assert_eq!(images(&frozen.entries), BTreeSet::from([first]));
    assert!(host.publication_web_image(publication, first).is_none());
    assert_ne!(request(&host), old_request);
    let second = complete(&mut host);
    frame(&mut host);
    assert_eq!(
        images(&canvas(&host, world).1.entries),
        BTreeSet::from([second])
    );
    assert!(host.publication_web_image(publication, second).is_none());
}

#[test]
fn world_destruction_cancels_pending_and_published_images() {
    let (mut host, world, _) = fixture();
    let first = complete(&mut host);
    frame(&mut host);
    let (publication, _) = canvas(&host, world);
    let _reader = host.retain_publication_read(publication).unwrap();
    let request = request(&host);
    let staging = host
        .web_content_mut()
        .reserve_image(
            request,
            WebContentImageDescription {
                pixel_size: [16, 8],
            },
        )
        .unwrap();
    assert!(host.destroy_world(world));
    assert!(!host.web_content().is_image_retained(first));
    assert!(!host.web_content().is_image_retained(staging));
    assert_eq!(
        host.web_content_mut().complete_image(request, staging),
        Err(WebContentError::StaleRequest)
    );
}
