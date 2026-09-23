//! Whole-Surface cache presentation through a real Host, World and RenderService
//! with the failure-injecting test device. GL harnesses own image evidence.
//!
//! Core does not yet populate the prepared cache fields, so each frame renders
//! the World's evaluated Surface items with explicit policy, revision and
//! interaction values through the harness entry.

#![cfg(feature = "surfaces")]

mod support;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, SurfaceCachePolicy, SurfaceRenderItem,
    WorldContext, WorldId,
    components::{Camera, Transform},
};
use ipp_render_gl::{
    RenderError, RenderService, RenderStats, SurfaceCacheDiagnostic, SurfaceCachePresentation,
};
use support::*;

const VIEWPORT: u32 = 100;

/// Cached at every distance: band 1, 64 texels per metre, 10 Hz.
fn always() -> SurfaceCachePolicy {
    SurfaceCachePolicy {
        direct_distance: 0.0,
        texels_per_metre: 64.0,
        max_refresh_hz: 10.0,
    }
}

/// Direct below 2 m; band 1 covers [2, 4) m and band 2 [4, 8) m.
fn banded() -> SurfaceCachePolicy {
    SurfaceCachePolicy {
        direct_distance: 2.0,
        ..always()
    }
}

/// Prepared cache inputs of one Surface for a frame.
#[derive(Clone, Copy)]
struct Inputs {
    cache: Option<SurfaceCachePolicy>,
    paint: u64,
    resource: u64,
    interaction: bool,
}

impl Inputs {
    fn cached(policy: SurfaceCachePolicy) -> Self {
        Self {
            cache: Some(policy),
            paint: 1,
            resource: 1,
            interaction: false,
        }
    }

    fn direct() -> Self {
        Self {
            cache: None,
            ..Self::cached(always())
        }
    }

    fn apply(self, item: &mut SurfaceRenderItem) {
        item.cache = self.cache;
        item.paint_revision = self.paint;
        item.resource_revision = self.resource;
        #[cfg(feature = "gui")]
        {
            item.interaction = self.interaction;
        }
        #[cfg(not(feature = "gui"))]
        assert!(!self.interaction, "interaction needs the gui capability");
    }
}

/// Advance the World by `dt` seconds, then render its Surfaces with `inputs`
/// selected per entity.
fn frame_with(
    renderer: &mut RenderService<TestDevice>,
    world: &mut WorldContext<'_>,
    dt: f64,
    inputs: impl Fn(EntityId) -> Inputs,
) -> Result<RenderStats, RenderError> {
    renderer.begin_frame();
    advance(world, dt).unwrap();
    let items: Vec<_> = world
        .surface_render_items()
        .iter()
        .cloned()
        .map(|mut item| {
            inputs(item.entity).apply(&mut item);
            item
        })
        .collect();
    renderer.render_with_surface_items(world, VIEWPORT, VIEWPORT, &items)
}

fn frame(
    renderer: &mut RenderService<TestDevice>,
    world: &mut WorldContext<'_>,
    dt: f64,
    inputs: Inputs,
) -> RenderStats {
    frame_with(renderer, world, dt, |_| inputs).unwrap()
}

fn diagnostics(
    renderer: &RenderService<TestDevice>,
    world: WorldId,
) -> Vec<SurfaceCacheDiagnostic> {
    let mut out = Vec::new();
    renderer.surface_cache_diagnostics(world, &mut out);
    out
}

fn presentation(renderer: &RenderService<TestDevice>, world: WorldId) -> SurfaceCachePresentation {
    diagnostics(renderer, world)[0].presentation
}

/// Cache counters of one frame: repaints, reuses, direct, fallbacks, allocations.
fn work(stats: &RenderStats) -> [u32; 5] {
    [
        stats.surface_cache_repaints,
        stats.surface_cache_reuses,
        stats.surface_cache_direct,
        stats.surface_cache_fallbacks,
        stats.surface_cache_allocations,
    ]
}

/// Draws of Surface primitives outside cache composites.
fn surface_draws(state: &DeviceState) -> u32 {
    #[cfg(feature = "gui")]
    let text = state.glyph_batch_draws.get();
    #[cfg(not(feature = "gui"))]
    let text = 0;
    state.surface_path_draws.get() + state.analytic_glyph_draws.get() + text
}

fn take_events(state: &DeviceState) -> String {
    std::mem::take(&mut *state.surface_events.borrow_mut())
}

/// Main-pass text draw: atlas batches with gui, analytic glyphs without.
const TEXT: char = if cfg!(feature = "gui") {
    'T'
} else {
    'G'
};

/// A text Surface in front of the camera with caching enabled on the device.
fn scene(
    host: &mut ipp_core::HostRuntime,
) -> (
    RenderService<TestDevice>,
    std::rc::Rc<DeviceState>,
    WorldId,
    EntityId,
) {
    let (renderer, state, world_id, entity) = text_run_scene(host, surface_font(), &[0]);
    state.cache_limit.set(4096);
    (renderer, state, world_id, entity)
}

/// The scene's one-glyph text Surface.
fn text_surface() -> ipp_core::Surface {
    use ipp_core::services::asset_management::{AssetSource, font::FONT_TYPE};
    use ipp_core::{PositionedGlyph, Surface, SurfaceItemContent, SurfaceItemStyle};

    let mut surface = Surface::default();
    surface
        .insert_item(
            0,
            SurfaceItemContent::GlyphRun(vec![PositionedGlyph {
                glyph_id: 0,
                position: [0.0, 0.0],
                color: None,
            }]),
            SurfaceItemStyle {
                position: [0.5, 0.5],
                font_size: 1.0,
                asset: Some(AssetSource {
                    kind: FONT_TYPE,
                    uri: "fixture:///font.ippf".into(),
                    variant: 0,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    surface
}

/// Another text Surface sharing the scene font, at `z`.
fn add_text_surface(world: &mut WorldContext<'_>, z: f32) -> EntityId {
    let surface = text_surface();
    let entity = create(
        world,
        vec![
            ComponentValue::Transform(Transform {
                z,
                ..Transform::default()
            }),
            ComponentValue::Surface(surface),
            ComponentValue::BoundingGeometry(Default::default()),
        ],
    );
    for _ in 0..8 {
        update(world).unwrap();
        if world
            .surface_render_items()
            .iter()
            .any(|item| item.entity == entity && !item.primitives.is_empty())
        {
            return entity;
        }
    }
    panic!("added Surface did not prepare");
}

#[test]
fn default_and_absent_policies_make_no_cache_calls() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    for _ in 0..3 {
        let stats = render_frame(&mut renderer, &mut world, VIEWPORT, VIEWPORT).unwrap();
        assert_eq!(work(&stats), [0; 5]);
        assert_eq!(
            (
                stats.surface_cache_entries,
                stats.surface_cache_resident_bytes
            ),
            (0, 0)
        );
        let stats = frame(&mut renderer, &mut world, 0.1, Inputs::direct());
        assert_eq!(work(&stats), [0; 5]);
    }

    assert_eq!(
        (
            state.cache_creates.get(),
            state.cache_begins.get(),
            state.cache_composites.get()
        ),
        (0, 0, 0)
    );
    assert!(surface_draws(&state) > 0);
    assert!(diagnostics(&renderer, world_id).is_empty());
}

#[test]
fn warm_frames_composite_the_image_without_surface_work() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    let cold = frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(
        (
            cold.surface_cache_entries,
            cold.surface_cache_resident_bytes
        ),
        (1, 4 * 64 * 64)
    );
    // Atlas population precedes the repaint, which precedes the main pass.
    assert_eq!(take_events(&state), format!("B{TEXT}EFC"));
    #[cfg(feature = "gui")]
    assert_eq!(cold.glyph_populates, 1);

    let draws = surface_draws(&state);
    for _ in 0..3 {
        let warm = frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
        assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
        assert_eq!((warm.draw_calls, warm.triangles), (1, 2));
        assert_eq!(warm.uploaded_bytes, 0);
        assert_eq!(take_events(&state), "FC");
    }

    assert_eq!(surface_draws(&state), draws);
    assert_eq!(state.cache_begins.get(), 1);
    assert_eq!(state.cache_composites.get(), 4);
    assert!(!state.cache_target_bound.get());
    #[cfg(feature = "gui")]
    assert!(!state.atlas_target_bound.get());

    let [diagnostic] = diagnostics(&renderer, world_id)[..] else {
        panic!("one entry");
    };
    assert_eq!(diagnostic.entity, entity);
    assert_eq!(diagnostic.presentation, SurfaceCachePresentation::Reused);
    assert_eq!(diagnostic.presentation.code(), 5);
    assert_eq!((diagnostic.band, diagnostic.size), (1, [64, 64]));
    assert_eq!((diagnostic.repaints, diagnostic.reuses), (1, 3));
    assert!((diagnostic.painted_at - 0.1).abs() < 1e-9);
    assert_eq!(diagnostic.resident_bytes, 4 * 64 * 64);
}

#[test]
fn placement_within_a_band_composites_and_crossing_a_band_resizes() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    // Five metres: band 2 at 32 texels per metre.
    frame(&mut renderer, &mut world, 0.1, Inputs::cached(banded()));
    assert_eq!(diagnostics(&renderer, world_id)[0].size, [32, 32]);

    // Moving inside the band, and just past its boundary, only composites.
    for z in [-1.0, 0.5, 1.2, 0.8] {
        place(&mut world, entity, z);
        let stats = frame(&mut renderer, &mut world, 0.1, Inputs::cached(banded()));
        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "z {z}: {stats:?}");
    }

    // Three metres crosses the hysteresis margin into band 1.
    place(&mut world, entity, 2.0);
    let nearer = frame(&mut renderer, &mut world, 0.1, Inputs::cached(banded()));
    assert_eq!(work(&nearer), [1, 0, 0, 0, 1], "{nearer:?}");
    assert_eq!(diagnostics(&renderer, world_id)[0].size, [64, 64]);
    assert_eq!(nearer.surface_cache_resident_bytes, 4 * 64 * 64);
    assert_eq!(
        (state.cache_creates.get(), state.cache_resizes.get()),
        (1, 1)
    );

    // One and a half metres is inside the direct distance.
    place(&mut world, entity, 3.5);
    let near = frame(&mut renderer, &mut world, 0.1, Inputs::cached(banded()));
    assert_eq!(work(&near), [0, 0, 1, 0, 0], "{near:?}");
    assert_eq!(
        presentation(&renderer, world_id),
        SurfaceCachePresentation::Near
    );
    assert_eq!(take_events(&state).chars().last(), Some(TEXT));
}

#[test]
fn paint_edits_coalesce_to_the_refresh_interval_of_world_time() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, _state, world_id, _) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let mut inputs = Inputs::cached(always());

    frame(&mut renderer, &mut world, 0.1, inputs);

    // A frozen World clock never refreshes the displayed image.
    for paint in 2..6 {
        inputs.paint = paint;
        let stats = frame(&mut renderer, &mut world, 0.0, inputs);
        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "{stats:?}");
    }

    // Edits within 0.1 s of World time coalesce into one repaint of the latest.
    inputs.paint = 6;
    assert_eq!(
        frame(&mut renderer, &mut world, 0.05, inputs).surface_cache_repaints,
        0
    );
    inputs.paint = 7;
    assert_eq!(
        frame(&mut renderer, &mut world, 0.05, inputs).surface_cache_repaints,
        1
    );
    assert_eq!(
        frame(&mut renderer, &mut world, 0.5, inputs).surface_cache_reuses,
        1
    );

    // Continuous animation reaches every refresh opportunity.
    let mut repaints = 0;
    for step in 0..60 {
        inputs.paint = 100 + step;
        repaints += frame(&mut renderer, &mut world, 1.0 / 60.0, inputs).surface_cache_repaints;
    }
    assert!(
        (9..=11).contains(&repaints),
        "{repaints} repaints in 1 s at 10 Hz"
    );
    assert_eq!(diagnostics(&renderer, world_id)[0].repaints, 2 + repaints);
}

#[test]
fn resource_revisions_repaint_immediately() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, _state, world_id, _) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let mut inputs = Inputs::cached(always());

    frame(&mut renderer, &mut world, 0.1, inputs);
    inputs.resource = 2;
    let stats = frame(&mut renderer, &mut world, 0.0, inputs);
    assert_eq!(work(&stats), [1, 0, 0, 0, 0], "{stats:?}");
}

#[cfg(feature = "gui")]
#[test]
fn interaction_presents_directly_and_returns_only_to_current_images() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let mut inputs = Inputs::cached(always());

    frame(&mut renderer, &mut world, 0.1, inputs);
    take_events(&state);

    // Interaction switches to direct presentation in the same frame.
    inputs.interaction = true;
    let focused = frame(&mut renderer, &mut world, 0.01, inputs);
    assert_eq!(work(&focused), [0, 0, 1, 0, 0], "{focused:?}");
    assert_eq!(take_events(&state), format!("F{TEXT}"));
    let [diagnostic] = diagnostics(&renderer, world_id)[..] else {
        panic!("one entry");
    };
    assert_eq!(
        diagnostic.presentation,
        SurfaceCachePresentation::Interaction
    );
    assert_eq!(diagnostic.presentation.code(), 1);
    // The image stays resident for the return.
    assert_eq!(focused.surface_cache_entries, 1);

    // Content edited while direct repaints before the image shows again.
    inputs.paint = 2;
    frame(&mut renderer, &mut world, 0.01, inputs);
    inputs.interaction = false;
    let released = frame(&mut renderer, &mut world, 0.01, inputs);
    assert_eq!(work(&released), [1, 0, 0, 0, 0], "{released:?}");

    // Unchanged content returns to its image without repainting.
    inputs.interaction = true;
    frame(&mut renderer, &mut world, 0.01, inputs);
    inputs.interaction = false;
    let returned = frame(&mut renderer, &mut world, 0.01, inputs);
    assert_eq!(work(&returned), [0, 1, 0, 0, 0], "{returned:?}");
}

#[test]
fn culled_surfaces_skip_repaints_and_refresh_stale_content_on_return() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let mut inputs = Inputs::cached(always());

    frame(&mut renderer, &mut world, 0.1, inputs);
    let begins = state.cache_begins.get();

    // Behind the camera, edits and elapsed time cause no cache work.
    place(&mut world, entity, 10.0);
    for paint in 2..5 {
        inputs.paint = paint;
        let culled = frame(&mut renderer, &mut world, 0.5, inputs);
        assert_eq!(work(&culled), [0; 5], "{culled:?}");
        assert_eq!(culled.draw_calls, 0);
    }
    assert_eq!(state.cache_begins.get(), begins);
    assert_eq!(
        presentation(&renderer, world_id),
        SurfaceCachePresentation::Culled
    );
    assert_eq!(presentation(&renderer, world_id).code(), 4);

    place(&mut world, entity, 0.0);
    let visible = frame(&mut renderer, &mut world, 0.0, inputs);
    assert_eq!(work(&visible), [1, 0, 0, 0, 0], "{visible:?}");
}

#[test]
fn removal_policy_removal_and_forgetting_the_world_release_images() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();

    frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    assert_eq!(state.cache_targets_live.get(), 1);

    // Removing the policy releases the image after the frame.
    let direct = frame(&mut renderer, &mut world, 0.1, Inputs::direct());
    assert_eq!(
        (
            direct.surface_cache_entries,
            direct.surface_cache_resident_bytes
        ),
        (0, 0)
    );
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(diagnostics(&renderer, world_id).is_empty());

    // A destroyed and recreated Surface never meets its old entry.
    frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(entity),
            }],
        })
        .unwrap();
    let removed = frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    assert_eq!(removed.surface_cache_entries, 0);
    assert_eq!(state.cache_targets_live.get(), 0);

    let recreated = add_text_surface(&mut world, 0.0);
    assert_ne!(recreated, entity);
    let fresh = frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    assert_eq!(work(&fresh), [1, 0, 0, 0, 1], "{fresh:?}");
    assert_eq!(diagnostics(&renderer, world_id)[0].repaints, 1);

    drop(world);
    renderer.forget_world(world_id);
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(diagnostics(&renderer, world_id).is_empty());
}

#[test]
fn allocation_and_repaint_failures_fall_back_directly_and_recover() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let inputs = Inputs::cached(always());

    *state.fail_cache_create.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let failed = frame(&mut renderer, &mut world, 0.1, inputs);
    assert_eq!(work(&failed), [0, 0, 1, 1, 0], "{failed:?}");
    assert_eq!(take_events(&state), format!("F{TEXT}"));
    assert_eq!(
        presentation(&renderer, world_id),
        SurfaceCachePresentation::Fallback
    );
    assert_eq!(presentation(&renderer, world_id).code(), 2);

    // Direct presentation continues through the back-off without retrying.
    *state.fail_cache_create.borrow_mut() = None;
    frame(&mut renderer, &mut world, 0.5, inputs);
    assert_eq!(state.cache_creates.get(), 1);
    let recovered = frame(&mut renderer, &mut world, 0.5, inputs);
    assert_eq!(work(&recovered), [1, 0, 0, 0, 1], "{recovered:?}");

    // A failed begin releases the image and presents directly.
    *state.fail_cache_begin.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let begin = frame(
        &mut renderer,
        &mut world,
        0.0,
        Inputs {
            resource: 2,
            ..inputs
        },
    );
    assert_eq!(work(&begin), [0, 0, 1, 1, 0], "{begin:?}");
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(!state.cache_target_bound.get());
    *state.fail_cache_begin.borrow_mut() = None;

    // A failed end marks the image unusable the same way.
    *state.fail_cache_end.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let end = frame(&mut renderer, &mut world, 1.0, inputs);
    assert_eq!(work(&end), [0, 0, 1, 1, 1], "{end:?}");
    assert_eq!(state.cache_targets_live.get(), 0);
    *state.fail_cache_end.borrow_mut() = None;

    // A failed composite draws the Surface directly in its slot.
    frame(&mut renderer, &mut world, 1.0, inputs);
    take_events(&state);
    *state.fail_cache_composite.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let composite = frame(&mut renderer, &mut world, 0.0, inputs);
    assert_eq!(work(&composite), [0, 0, 1, 1, 0], "{composite:?}");
    assert_eq!(take_events(&state), format!("FC{TEXT}"));
    *state.fail_cache_composite.borrow_mut() = None;
    let later = frame(&mut renderer, &mut world, 1.0, inputs);
    assert_eq!(work(&later), [1, 0, 0, 0, 1], "{later:?}");
}

#[test]
fn context_loss_fails_the_frame_and_recovery_repaints() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = scene(&mut host);
    let inputs = Inputs::cached(always());
    {
        let mut world = host.world_mut(world_id).unwrap();
        frame(&mut renderer, &mut world, 0.1, inputs);
        *state.fail_cache_begin.borrow_mut() = Some(RenderError::ContextLost);
        let lost = frame_with(&mut renderer, &mut world, 0.0, |_| Inputs {
            resource: 2,
            ..inputs
        });
        assert_eq!(lost, Err(RenderError::ContextLost));
        assert!(!state.cache_target_bound.get());
        #[cfg(feature = "gui")]
        assert!(!state.atlas_target_bound.get());
    }

    *state.fail_cache_begin.borrow_mut() = None;
    recover_context(&mut renderer, &mut host, world_id, &surface_font());
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(diagnostics(&renderer, world_id).is_empty());

    let mut world = host.world_mut(world_id).unwrap();
    let recovered = frame(&mut renderer, &mut world, 0.1, inputs);
    assert_eq!(work(&recovered), [1, 0, 0, 0, 1], "{recovered:?}");
    assert_eq!(state.cache_targets_live.get(), 1);

    // Context loss while allocating fails the frame without leaking state.
    drop(world);
    recover_context(&mut renderer, &mut host, world_id, &surface_font());
    let mut world = host.world_mut(world_id).unwrap();
    *state.fail_cache_create.borrow_mut() = Some(RenderError::ContextLost);
    let lost = frame_with(&mut renderer, &mut world, 0.1, |_| inputs);
    assert_eq!(lost, Err(RenderError::ContextLost));
    *state.fail_cache_create.borrow_mut() = None;
}

#[test]
fn budget_pressure_evicts_idle_images_then_falls_back() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, first) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let second = add_text_surface(&mut world, -1.0);
    renderer.set_surface_cache_budget(4 * 64 * 64);

    // Only one image fits: entity order admits the first, the second falls back.
    let both = frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    assert_eq!(work(&both), [1, 0, 1, 1, 1], "{both:?}");
    assert_eq!(both.surface_cache_resident_bytes, 4 * 64 * 64);

    // With the first culled, its idle image makes room for the second.
    place(&mut world, first, 10.0);
    let swapped = frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    assert_eq!(work(&swapped), [1, 0, 0, 0, 1], "{swapped:?}");
    assert_eq!(state.cache_targets_live.get(), 1);
    let records = diagnostics(&renderer, world_id);
    let size = |entity| records.iter().find(|r| r.entity == entity).unwrap().size;
    assert_eq!((size(first), size(second)), ([0, 0], [64, 64]));
}

#[test]
fn worlds_share_the_context_budget() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, first, _) = scene(&mut host);
    renderer.set_surface_cache_budget(4 * 64 * 64);

    let second = host.create_world(Default::default()).unwrap();
    {
        let mut world = host.world_mut(second).unwrap();
        let camera = create(
            &mut world,
            vec![
                ComponentValue::Camera(Camera::default()),
                ComponentValue::Transform(Transform {
                    z: 5.0,
                    ..Transform::default()
                }),
            ],
        );
        world.enqueue_camera_activate(camera).unwrap();
        update(&mut world).unwrap();
    }
    let surface = text_surface();
    {
        let mut world = host.world_mut(second).unwrap();
        create(
            &mut world,
            vec![
                ComponentValue::Transform(Transform::default()),
                ComponentValue::Surface(surface),
                ComponentValue::BoundingGeometry(Default::default()),
            ],
        );
    }
    for _ in 0..8 {
        host.progress_assets();
        let mut world = host.world_mut(second).unwrap();
        update(&mut world).unwrap();
    }

    let mut world = host.world_mut(first).unwrap();
    frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    drop(world);

    // The second World's image evicts the first World's image, which its
    // frame did not present.
    let mut world = host.world_mut(second).unwrap();
    let stats = frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    assert_eq!(work(&stats), [1, 0, 0, 0, 1], "{stats:?}");
    assert_eq!(
        (
            stats.surface_cache_entries,
            stats.surface_cache_resident_bytes
        ),
        (1, 4 * 64 * 64)
    );
    drop(world);
    assert_eq!(diagnostics(&renderer, first)[0].size, [0, 0]);

    // Forgetting one World leaves the other's image.
    renderer.set_surface_cache_budget(ipp_render_gl::DEFAULT_SURFACE_CACHE_BUDGET_BYTES);
    let mut world = host.world_mut(first).unwrap();
    frame(&mut renderer, &mut world, 0.1, Inputs::cached(always()));
    drop(world);
    assert_eq!(state.cache_targets_live.get(), 2);
    renderer.forget_world(first);
    assert_eq!(state.cache_targets_live.get(), 1);
    assert_eq!(diagnostics(&renderer, second).len(), 1);
}

#[test]
fn mixed_cached_and_direct_surfaces_keep_painter_order() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, near) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let far = add_text_surface(&mut world, -3.0);
    let inputs = |entity| {
        if entity == far {
            Inputs::cached(always())
        } else {
            Inputs::direct()
        }
    };

    take_events(&state);
    frame_with(&mut renderer, &mut world, 0.1, inputs).unwrap();
    // The far image repaints before the frame; the main pass composites it
    // behind the nearer direct Surface.
    assert_eq!(take_events(&state), format!("B{TEXT}EFC{TEXT}"));
    let warm = frame_with(&mut renderer, &mut world, 0.1, inputs).unwrap();
    assert_eq!(take_events(&state), format!("FC{TEXT}"));
    assert_eq!(work(&warm), [0, 1, 0, 0, 0]);

    // Swapping depths swaps the slots.
    place(&mut world, near, -6.0);
    place(&mut world, far, 0.0);
    frame_with(&mut renderer, &mut world, 0.1, inputs).unwrap();
    assert_eq!(take_events(&state), format!("F{TEXT}C"));
}
