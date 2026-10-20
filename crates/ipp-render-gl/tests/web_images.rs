//! Real Host/Canvas publications with a failure-injecting GPU device. Native and
//! browser producer suites separately establish actual GPU pixels and imports.

mod support;

use std::rc::Rc;

use ipp_core::services::web_content::{
    WebContentImageDescription, WebContentImageId, WebContentRequestId,
};
use ipp_core::{ComponentValue, HostRuntime, WorldId, components::CanvasHtml};
use ipp_render_gl::{RenderError, RenderService, WebImageImportError};
use support::{DeviceState, RenderFrameStats, TestDevice, canvas::CanvasSurface};

struct Fixture {
    host: HostRuntime,
    renderer: RenderService<TestDevice>,
    state: Rc<DeviceState>,
    root: WorldId,
    surface: CanvasSurface,
}

impl Fixture {
    fn new() -> Self {
        let mut host = support::task_scheduler::host();
        let (world, renderer, state) = support::setup(&mut host);
        let root = world.id();
        drop(world);
        state.texture_readback_enabled.set(true);
        host.web_content_mut().register_source("panel");
        let surface = CanvasSurface::new(
            &mut host,
            root,
            0.0,
            vec![ComponentValue::CanvasHtml(CanvasHtml {
                source: "panel".into(),
                width: 1.0,
                height: 1.0,
            })],
        );
        let mut fixture = Self {
            host,
            renderer,
            state,
            root,
            surface,
        };
        fixture.frame();
        fixture
    }

    fn frame(&mut self) {
        self.host.frame(0.016).unwrap();
        let selected = self
            .host
            .root_output(self.root)
            .map(|(output, _, publication)| (output, publication));
        self.renderer.prepare(&mut self.host, selected).unwrap();
        support::progress_assets(&mut self.host);
    }

    fn request(&self) -> WebContentRequestId {
        self.host
            .web_content()
            .requests()
            .find(|request| request.target.canvas.entity == self.surface.content)
            .unwrap()
            .id
    }

    fn import(&mut self) -> Result<WebContentImageId, WebImageImportError> {
        let request = self.request();
        let ticket = self.renderer.reserve_web_image(
            &mut self.host,
            request,
            WebContentImageDescription {
                pixel_size: [64, 64],
            },
            64 * 64 * 4,
        )?;
        self.renderer
            .import_reserved_web_image(&mut self.host, ticket, |_, _| Ok(()))
    }

    fn image(&self) -> Option<WebContentImageId> {
        self.surface
            .publication(&self.host)
            .entries
            .iter()
            .find_map(|entry| match entry.as_ref() {
                ipp_core::systems::canvas::CanvasPaintEntry::Primitive {
                    primitive,
                    ..
                } => primitive.web_image(),
                _ => None,
            })
    }
}

#[test]
fn only_completed_publications_select_a_new_image() {
    let mut fixture = Fixture::new();
    let first = fixture.import().unwrap();
    assert_eq!(fixture.image(), None);
    fixture.frame();
    assert_eq!(fixture.image(), Some(first));
    let first_publication = fixture
        .host
        .latest_publication(fixture.surface.output.world().id())
        .unwrap();
    let second = fixture.import().unwrap();
    assert_ne!(first, second);
    assert_eq!(fixture.image(), Some(first));
    assert!(
        fixture
            .host
            .publication_web_image(first_publication, first)
            .is_some()
    );
    assert!(
        fixture
            .host
            .publication_web_image(first_publication, second)
            .is_none()
    );
    fixture.frame();
    assert_eq!(fixture.image(), Some(second));
    assert_eq!(
        fixture
            .renderer
            .draw_stats(&fixture.host, fixture.root, 100, 100)
            .unwrap()
            .failed_draw_calls,
        0
    );
}

#[test]
fn retired_images_keep_their_budget_until_gpu_completion() {
    let mut fixture = Fixture::new();
    fixture.renderer.set_web_image_limits(3, 3 * 64 * 64 * 4);
    fixture.import().unwrap();
    fixture.frame();
    fixture.import().unwrap();
    assert!(matches!(
        fixture.import(),
        Err(WebImageImportError::Capacity)
    ));
    fixture.frame();
    let usage = fixture.renderer.web_image_usage();
    assert_eq!(usage.images, 2);
    assert_eq!(usage.retiring_images, 1);
    assert_eq!(fixture.state.textures_deleted.get(), 0);
    assert!(matches!(
        fixture.import(),
        Err(WebImageImportError::Capacity)
    ));
    fixture.state.completion_fences_ready.set(true);
    fixture
        .renderer
        .refresh_web_images(&mut fixture.host)
        .unwrap();
    assert_eq!(fixture.renderer.web_image_usage().images, 1);
    assert_eq!(fixture.state.textures_deleted.get(), 1);
    fixture.import().unwrap();
}

#[test]
fn failed_private_import_preserves_the_published_image() {
    let mut fixture = Fixture::new();
    let first = fixture.import().unwrap();
    fixture.frame();
    let request = fixture.request();
    let ticket = fixture
        .renderer
        .reserve_web_image(
            &mut fixture.host,
            request,
            WebContentImageDescription {
                pixel_size: [64, 64],
            },
            64 * 64 * 4,
        )
        .unwrap();
    let failure = fixture
        .renderer
        .import_reserved_web_image(&mut fixture.host, ticket, |_, _| {
            Err(RenderError::RenderDevice(
                "injected partial import failure".into(),
            ))
        });
    assert!(matches!(failure, Err(WebImageImportError::Device(_))));
    assert_eq!(fixture.image(), Some(first));
    fixture.frame();
    assert_eq!(fixture.image(), Some(first));
    assert_eq!(fixture.renderer.web_image_usage().retiring_images, 1);
}

#[test]
fn revocation_and_context_loss_cannot_revalidate_old_images() {
    let mut fixture = Fixture::new();
    let first = fixture.import().unwrap();
    fixture.frame();
    let publication = fixture
        .host
        .latest_publication(fixture.surface.output.world().id())
        .unwrap();
    let stale = fixture.request();
    fixture.host.web_content_mut().register_source("panel");
    assert!(
        fixture
            .host
            .publication_web_image(publication, first)
            .is_none()
    );
    assert!(matches!(
        fixture.renderer.reserve_web_image(
            &mut fixture.host,
            stale,
            WebContentImageDescription {
                pixel_size: [64, 64]
            },
            64 * 64 * 4
        ),
        Err(WebImageImportError::Request(_))
    ));
    fixture.frame();
    let second = fixture.import().unwrap();
    fixture.frame();
    assert_eq!(fixture.image(), Some(second));
    fixture.renderer.set_asset_context_active(false);
    fixture.renderer.unload_host(&mut fixture.host).unwrap();
    fixture.renderer.set_asset_context_active(true);
    let publication = fixture
        .host
        .latest_publication(fixture.surface.output.world().id())
        .unwrap();
    assert!(
        fixture
            .host
            .publication_web_image(publication, second)
            .is_none()
    );
    fixture.frame();
    let third = fixture.import().unwrap();
    assert_ne!(second, third);
    fixture.frame();
    assert_eq!(fixture.image(), Some(third));
}

#[test]
fn capture_permits_charge_before_capture_and_survive_context_loss() {
    let mut fixture = Fixture::new();
    let bytes = 64 * 64 * 4;
    fixture.renderer.set_web_image_limits(4, 4 * bytes);
    let producer = fixture
        .renderer
        .reserve_web_image_staging(2, 2 * bytes)
        .unwrap();
    let request = fixture.request();
    let ticket = fixture
        .renderer
        .reserve_web_image(
            &mut fixture.host,
            request,
            WebContentImageDescription {
                pixel_size: [64, 64],
            },
            bytes,
        )
        .unwrap();
    assert_eq!(ticket.pixel_size(), [64, 64]);
    assert_eq!(fixture.renderer.web_image_usage().bytes, 4 * bytes);
    assert_eq!(fixture.renderer.web_image_usage().staging_images, 3);
    assert!(matches!(
        fixture.renderer.reserve_web_image_staging(1, bytes),
        Err(WebImageImportError::Capacity)
    ));
    fixture.renderer.set_asset_context_active(false);
    fixture
        .renderer
        .refresh_web_images(&mut fixture.host)
        .unwrap();
    // Lost GPU destination storage is invalid, but producer resources remain
    // charged until their actual owner closes them and drops the permits.
    assert_eq!(fixture.renderer.web_image_usage().bytes, 3 * bytes);
    let mut called = false;
    assert!(
        fixture
            .renderer
            .import_reserved_web_image(&mut fixture.host, ticket, |_, _| {
                called = true;
                Ok(())
            })
            .is_err()
    );
    assert!(!called);
    assert_eq!(fixture.renderer.web_image_usage().bytes, 2 * bytes);
    drop(producer);
    assert_eq!(fixture.renderer.web_image_usage().bytes, 0);
}

#[test]
fn abandoned_capture_cancels_reservation_and_fences_private_storage() {
    let mut fixture = Fixture::new();
    let bytes = 64 * 64 * 4;
    let request = fixture.request();
    let ticket = fixture
        .renderer
        .reserve_web_image(
            &mut fixture.host,
            request,
            WebContentImageDescription {
                pixel_size: [64, 64],
            },
            bytes,
        )
        .unwrap();
    assert_eq!(fixture.renderer.web_image_usage().bytes, 2 * bytes);
    drop(ticket);
    fixture
        .renderer
        .refresh_web_images(&mut fixture.host)
        .unwrap();
    assert_eq!(fixture.renderer.web_image_usage().bytes, bytes);
    assert_eq!(fixture.renderer.web_image_usage().retiring_images, 1);
    fixture.import().unwrap();
    fixture.frame();
    assert!(fixture.image().is_some());
}

#[test]
fn admission_binds_one_host_before_any_selected_prepare() {
    let mut first = Fixture::new();
    let mut second = Fixture::new();
    // A fresh renderer has no program demand or selected output yet.
    first.renderer = RenderService::new(TestDevice(first.state.clone())).unwrap();
    let image = first.import().unwrap();
    let usage = first.renderer.web_image_usage();
    assert!(matches!(
        first.renderer.refresh_web_images(&mut second.host),
        Err(RenderError::HostCatalogMismatch)
    ));
    assert!(matches!(
        first.renderer.unload_host(&mut second.host),
        Err(RenderError::HostCatalogMismatch)
    ));
    assert!(matches!(
        first.renderer.prepare(&mut second.host, None),
        Err(RenderError::HostCatalogMismatch)
    ));
    let request = second.request();
    assert!(matches!(
        first.renderer.reserve_web_image(
            &mut second.host,
            request,
            WebContentImageDescription {
                pixel_size: [64, 64]
            },
            64 * 64 * 4
        ),
        Err(WebImageImportError::Device(
            RenderError::HostCatalogMismatch
        ))
    ));
    assert_eq!(first.renderer.web_image_usage(), usage);
    assert!(first.host.web_content().image_description(image).is_some());
    let first_request = first.request();
    let ticket = first
        .renderer
        .reserve_web_image(
            &mut first.host,
            first_request,
            WebContentImageDescription {
                pixel_size: [64, 64],
            },
            64 * 64 * 4,
        )
        .unwrap();
    assert!(matches!(
        first
            .renderer
            .import_reserved_web_image(&mut second.host, ticket, |_, _| panic!(
                "foreign admission callback"
            )),
        Err(WebImageImportError::Device(
            RenderError::HostCatalogMismatch
        ))
    ));
    assert_eq!(first.renderer.web_image_usage().retiring_images, 0);
    // Explicit release revokes old image authority before allowing another Host.
    first.renderer.prepare(&mut first.host, None).unwrap();
    assert!(first.host.web_content().image_description(image).is_none());
    let ticket = first
        .renderer
        .reserve_web_image(
            &mut second.host,
            request,
            WebContentImageDescription {
                pixel_size: [64, 64],
            },
            64 * 64 * 4,
        )
        .unwrap();
    first
        .renderer
        .import_reserved_web_image(&mut second.host, ticket, |_, _| Ok(()))
        .unwrap();
}

#[test]
fn cached_repaints_obey_refresh_policy_but_revocation_is_immediate() {
    let mut fixture = Fixture::new();
    fixture.state.cache_limit.set(4096);
    support::canvas::apply(
        &mut fixture.host,
        fixture.root,
        vec![ipp_core::Command::insert_value(
            ipp_core::EntityRef::Handle(fixture.surface.anchor),
            ComponentValue::SurfaceCache(ipp_core::SurfaceCache {
                direct_distance: 0.0,
                resolution_scale: 1.0,
                allocation_priority: 1.0,
                max_refresh_hz: 1.0,
                max_texture_size: 0,
                force_cache: true,
            }),
        )],
    );
    fixture.import().unwrap();
    fixture.frame();
    let drawn = fixture
        .renderer
        .draw_stats(&fixture.host, fixture.root, 100, 100)
        .unwrap();
    assert_eq!(drawn.surface_cache_repaints, 1);
    for _ in 0..3 {
        fixture.import().unwrap();
        fixture.frame();
        let drawn = fixture
            .renderer
            .draw_stats(&fixture.host, fixture.root, 100, 100)
            .unwrap();
        assert_eq!(
            drawn.surface_cache_repaints, 0,
            "new pixels must obey the cache interval"
        );
        assert_eq!(drawn.surface_cache_reuses, 1);
    }
    fixture.host.frame(1.0).unwrap();
    fixture.frame();
    assert_eq!(
        fixture
            .renderer
            .draw_stats(&fixture.host, fixture.root, 100, 100)
            .unwrap()
            .surface_cache_repaints,
        1
    );
    // Make the painted version differ from the most recently planned one. The
    // cache must retain that old image's validity, not only the latest candidate.
    fixture.import().unwrap();
    fixture.frame();
    assert_eq!(
        fixture
            .renderer
            .draw_stats(&fixture.host, fixture.root, 100, 100)
            .unwrap()
            .surface_cache_repaints,
        0
    );
    fixture.host.web_content_mut().register_source("panel");
    // No World evaluation: an already baked revoked source cannot remain visible.
    let drawn = fixture
        .renderer
        .draw_stats(&fixture.host, fixture.root, 100, 100)
        .unwrap();
    assert_eq!(drawn.surface_cache_reuses, 0);
    assert!(drawn.failed_draw_calls > 0);
}

#[test]
fn live_detach_finishes_and_deletes_without_losing_staging_pressure() {
    let mut fixture = Fixture::new();
    let bytes = 64 * 64 * 4;
    fixture.import().unwrap();
    fixture.frame();
    fixture.import().unwrap();
    fixture.frame();
    assert_eq!(fixture.renderer.web_image_usage().retiring_images, 1);
    let producer = fixture
        .renderer
        .reserve_web_image_staging(2, 2 * bytes)
        .unwrap();
    let request = fixture.request();
    let capture = fixture
        .renderer
        .reserve_web_image(
            &mut fixture.host,
            request,
            WebContentImageDescription {
                pixel_size: [64, 64],
            },
            bytes,
        )
        .unwrap();
    fixture.renderer.unload_host(&mut fixture.host).unwrap();
    assert_eq!(fixture.state.completion_finishes.get(), 1);
    assert_eq!(fixture.state.textures_deleted.get(), 3);
    assert!(fixture.state.completion_fences.borrow().is_empty());
    assert_eq!(fixture.renderer.web_image_usage().bytes, 3 * bytes);
    fixture.renderer.set_asset_context_active(false);
    fixture.renderer.set_asset_context_active(true);
    fixture
        .renderer
        .refresh_web_images(&mut fixture.host)
        .unwrap();
    assert_eq!(fixture.renderer.web_image_usage().bytes, 3 * bytes);
    drop(capture);
    drop(producer);
    assert_eq!(fixture.renderer.web_image_usage().bytes, 0);
    fixture.frame();
    fixture.import().unwrap();
}
