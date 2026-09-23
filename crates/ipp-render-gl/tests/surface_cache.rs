//! Whole-Surface cache presentation through a real Host, World and RenderService
//! with the failure-injecting test device. Surfaces opt in through their
//! `SurfaceCache` component, and RenderSystem publishes the policy, paint and
//! resource revisions and interaction priority the renderer consumes. GL
//! harnesses own image evidence.

#![cfg(feature = "surfaces")]

mod support;

use ipp_core::services::asset_management::{AssetSource, drawing::DRAWING_TYPE};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, Surface, SurfaceCache, SurfaceCommand,
    SurfaceItemContent, SurfaceItemId, SurfaceItemPatch, SurfaceItemStyle, WorldContext, WorldId,
    components::{Camera, Transform},
};
use ipp_render_gl::{
    RenderError, RenderService, RenderStats, SurfaceCacheDiagnostic, SurfaceCachePresentation,
};
use support::*;

const VIEWPORT: u32 = 100;

/// Cached at every distance: band 1, 64 texels per metre, 10 Hz.
const ALWAYS: SurfaceCache = SurfaceCache {
    direct_distance: 0.0,
    texels_per_metre: 64.0,
    max_refresh_hz: 10.0,
};

/// Direct below 2 m; band 1 covers [2, 4) m and band 2 [4, 8) m.
const BANDED: SurfaceCache = SurfaceCache {
    direct_distance: 2.0,
    ..ALWAYS
};

/// Author, replace or with `None` remove a Surface's cache policy.
fn set_policy(world: &mut WorldContext<'_>, entity: EntityId, policy: Option<SurfaceCache>) {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![match policy {
                Some(policy) => Command::InsertComponentValue {
                    entity: EntityRef::Handle(entity),
                    value: ComponentValue::SurfaceCache(policy),
                },
                None => Command::RemoveComponent {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::SURFACE_CACHE,
                },
            }],
        })
        .unwrap();
    update(world).unwrap();
}

/// Queue an edit of the first item; the next frame applies it and advances
/// the paint revision.
fn edit(world: &mut WorldContext<'_>, entity: EntityId, patch: SurfaceItemPatch) {
    world
        .enqueue_surface_command(
            1,
            SurfaceCommand::Update {
                entity,
                id: SurfaceItemId(1),
                patch,
            },
        )
        .unwrap();
}

/// Queue a paint-only edit: a distinct colour for each `step`.
fn recolor(world: &mut WorldContext<'_>, entity: EntityId, step: u32) {
    let shade = (step % 97) as f32 / 97.0;
    edit(
        world,
        entity,
        SurfaceItemPatch {
            color: Some([shade, 1.0 - shade, 0.5, 1.0]),
            ..Default::default()
        },
    );
}

/// Prepared cache revisions of one Surface: paint, then resource.
fn revisions(world: &WorldContext<'_>, entity: EntityId) -> (u64, u64) {
    let item = world
        .surface_render_items()
        .iter()
        .find(|item| item.entity == entity)
        .expect("prepared Surface");
    (item.paint_revision, item.resource_revision)
}

/// Advance the World by `dt` seconds, then render its prepared Surfaces.
fn try_frame(
    renderer: &mut RenderService<TestDevice>,
    world: &mut WorldContext<'_>,
    dt: f64,
) -> Result<RenderStats, RenderError> {
    renderer.begin_frame();
    advance(world, dt).unwrap();
    renderer.render(world, VIEWPORT, VIEWPORT)
}

fn frame(
    renderer: &mut RenderService<TestDevice>,
    world: &mut WorldContext<'_>,
    dt: f64,
) -> RenderStats {
    try_frame(renderer, world, dt).unwrap()
}

fn diagnostics(
    renderer: &RenderService<TestDevice>,
    world: WorldId,
) -> Vec<SurfaceCacheDiagnostic> {
    let mut out = Vec::new();
    renderer.surface_cache_diagnostics(world, &mut out);
    out
}

#[cfg(feature = "gui")]
fn record(
    renderer: &RenderService<TestDevice>,
    world: WorldId,
    entity: EntityId,
) -> SurfaceCacheDiagnostic {
    diagnostics(renderer, world)
        .into_iter()
        .find(|record| record.entity == entity)
        .expect("cache record")
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
fn text_surface() -> Surface {
    use ipp_core::PositionedGlyph;
    use ipp_core::services::asset_management::font::FONT_TYPE;

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
fn absent_policies_make_no_cache_calls() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
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
        let stats = frame(&mut renderer, &mut world, 0.1);
        assert_eq!(work(&stats), [0; 5]);
    }

    // Opting in and back out again leaves no cache state behind.
    set_policy(&mut world, entity, Some(ALWAYS));
    set_policy(&mut world, entity, None);
    let stats = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&stats), [0; 5]);

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
    set_policy(&mut world, entity, Some(ALWAYS));

    let cold = frame(&mut renderer, &mut world, 0.1);
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
        let warm = frame(&mut renderer, &mut world, 0.1);
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
    set_policy(&mut world, entity, Some(BANDED));

    // Five metres: band 2 at 32 texels per metre.
    frame(&mut renderer, &mut world, 0.1);
    assert_eq!(diagnostics(&renderer, world_id)[0].size, [32, 32]);
    let revisions_before = revisions(&world, entity);

    // Moving inside the band, and just past its boundary, only composites.
    for z in [-1.0, 0.5, 1.2, 0.8] {
        place(&mut world, entity, z);
        let stats = frame(&mut renderer, &mut world, 0.1);
        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "z {z}: {stats:?}");
    }
    // Placement never advances the prepared revisions.
    assert_eq!(revisions(&world, entity), revisions_before);

    // Three metres crosses the hysteresis margin into band 1.
    place(&mut world, entity, 2.0);
    let nearer = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&nearer), [1, 0, 0, 0, 1], "{nearer:?}");
    assert_eq!(diagnostics(&renderer, world_id)[0].size, [64, 64]);
    assert_eq!(nearer.surface_cache_resident_bytes, 4 * 64 * 64);
    assert_eq!(
        (state.cache_creates.get(), state.cache_resizes.get()),
        (1, 1)
    );

    // One and a half metres is inside the direct distance.
    place(&mut world, entity, 3.5);
    let near = frame(&mut renderer, &mut world, 0.1);
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
    let (mut renderer, _state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    set_policy(&mut world, entity, Some(ALWAYS));

    frame(&mut renderer, &mut world, 0.1);

    // A frozen World clock never refreshes the displayed image.
    for step in 2..6 {
        let (paint, _) = revisions(&world, entity);
        recolor(&mut world, entity, step);
        let stats = frame(&mut renderer, &mut world, 0.0);
        assert!(revisions(&world, entity).0 > paint, "edit {step}");
        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "{stats:?}");
    }

    // Edits within 0.1 s of World time coalesce into one repaint of the latest.
    recolor(&mut world, entity, 6);
    assert_eq!(
        frame(&mut renderer, &mut world, 0.05).surface_cache_repaints,
        0
    );
    recolor(&mut world, entity, 7);
    assert_eq!(
        frame(&mut renderer, &mut world, 0.05).surface_cache_repaints,
        1
    );
    assert_eq!(
        frame(&mut renderer, &mut world, 0.5).surface_cache_reuses,
        1
    );

    // Continuous animation reaches every refresh opportunity.
    let mut repaints = 0;
    for step in 0..60 {
        recolor(&mut world, entity, 100 + step);
        repaints += frame(&mut renderer, &mut world, 1.0 / 60.0).surface_cache_repaints;
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
    let (mut renderer, _state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    set_policy(&mut world, entity, Some(ALWAYS));

    frame(&mut renderer, &mut world, 0.1);
    let (_, resource) = revisions(&world, entity);

    // A pending font removes the glyph run's identity from the item: the
    // frozen clock proves the repaint bypasses the refresh interval.
    edit(
        &mut world,
        entity,
        SurfaceItemPatch {
            asset: Some(Some(AssetSource {
                kind: ipp_core::services::asset_management::font::FONT_TYPE,
                uri: "fixture:///pending-font.ippf".into(),
                variant: 0,
            })),
            ..Default::default()
        },
    );
    let stats = frame(&mut renderer, &mut world, 0.0);
    assert!(revisions(&world, entity).1 > resource);
    assert_eq!(work(&stats), [1, 0, 0, 0, 0], "{stats:?}");
}

/// An opted-in GuiRoot panel at `z` holding a column filled by one checkbox
/// (node 2), and its root incarnation.
#[cfg(feature = "gui")]
fn gui_panel(world: &mut WorldContext<'_>, z: f32) -> (EntityId, u64) {
    use ipp_core::{GuiCommand, GuiContainerKind, GuiNodeContent, GuiNodeId, GuiNodeStyle};

    let panel = create(
        world,
        vec![
            ComponentValue::Transform(Transform {
                z,
                ..Transform::default()
            }),
            ComponentValue::Surface(Surface::default()),
            ComponentValue::GuiRoot(Default::default()),
            ComponentValue::BoundingGeometry(Default::default()),
            ComponentValue::SurfaceCache(ALWAYS),
        ],
    );
    let incarnation = world
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    let style = |width, height| GuiNodeStyle {
        width: Some(width),
        height: Some(height),
        background_color: Some([0.2, 0.3, 0.4, 1.0]),
        ..Default::default()
    };
    for (id, parent, content, style) in [
        (
            1,
            None,
            GuiNodeContent::Container(GuiContainerKind::Column),
            style(1.0, 1.0),
        ),
        (
            2,
            Some(GuiNodeId(1)),
            GuiNodeContent::Checkbox {
                checked: false,
            },
            style(1.0, 1.0),
        ),
    ] {
        world
            .enqueue_gui_command(
                GUI_SESSION,
                GuiCommand::InsertNode {
                    entity: panel,
                    root_incarnation: incarnation,
                    id: GuiNodeId(id),
                    parent,
                    index: 0,
                    content,
                    style,
                },
            )
            .unwrap();
    }
    update(world).unwrap();
    update(world).unwrap();
    (panel, incarnation)
}

#[cfg(feature = "gui")]
const GUI_SESSION: u64 = 7;

#[cfg(feature = "gui")]
#[test]
fn interaction_presents_directly_and_returns_only_to_current_images() {
    use ipp_core::{GuiCommand, GuiInputCommand, GuiNodeHandle, GuiNodeId, GuiNodePatch};

    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let (panel, incarnation) = gui_panel(&mut world, -1.0);
    let checkbox = GuiNodeHandle::new(GUI_SESSION, panel, incarnation, GuiNodeId(2), 1);
    let input = |world: &mut WorldContext<'_>, command| {
        world
            .enqueue_gui_input_command(GUI_SESSION, command)
            .unwrap();
    };

    let cold = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(
        frame(&mut renderer, &mut world, 0.1).surface_cache_reuses,
        1
    );
    take_events(&state);

    // Keyboard focus switches to direct presentation in the same frame.
    input(
        &mut world,
        GuiInputCommand::Focus {
            handle: checkbox,
        },
    );
    let focused = frame(&mut renderer, &mut world, 0.01);
    assert_eq!(work(&focused), [0, 0, 1, 0, 0], "{focused:?}");
    assert!(!take_events(&state).contains('C'));
    let diagnostic = record(&renderer, world_id, panel);
    assert_eq!(
        diagnostic.presentation,
        SurfaceCachePresentation::Interaction
    );
    assert_eq!(diagnostic.presentation.code(), 1);
    // The image stays resident for the return.
    assert_eq!(focused.surface_cache_entries, 1);

    // Content edited while direct repaints before the image shows again.
    world
        .enqueue_gui_command(
            GUI_SESSION,
            GuiCommand::UpdateNode {
                handle: checkbox,
                patch: GuiNodePatch {
                    background_color: Some(Some([0.9, 0.1, 0.1, 1.0])),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    frame(&mut renderer, &mut world, 0.01);
    input(&mut world, GuiInputCommand::Blur);
    let released = frame(&mut renderer, &mut world, 0.01);
    assert_eq!(work(&released), [1, 0, 0, 0, 0], "{released:?}");
    assert_eq!(
        frame(&mut renderer, &mut world, 0.01).surface_cache_reuses,
        1
    );

    // Hovering the panel outside the checkbox changes no paint: unchanged
    // content returns to its image without repainting.
    // The checkbox fills the panel, which the camera centres in the viewport.
    let paint = revisions(&world, panel).0;
    input(
        &mut world,
        GuiInputCommand::PointerMove {
            pointer: 1,
            panel: None,
            position: [0.5, 0.5],
            blockers: Vec::new(),
            panel_distance: None,
        },
    );
    // Pointer routing completes at the following update boundary.
    update(&mut world).unwrap();
    assert!(world.gui_input_hover(1).is_some());
    let hovered = frame(&mut renderer, &mut world, 0.01);
    assert_eq!(work(&hovered), [0, 0, 1, 0, 0], "{hovered:?}");
    input(
        &mut world,
        GuiInputCommand::PointerMove {
            pointer: 1,
            panel: None,
            position: [0.02, 0.02],
            blockers: Vec::new(),
            panel_distance: None,
        },
    );
    update(&mut world).unwrap();
    assert!(world.gui_input_hover(1).is_none());
    let returned = frame(&mut renderer, &mut world, 0.01);
    assert_eq!(revisions(&world, panel).0, paint);
    assert_eq!(work(&returned), [0, 1, 0, 0, 0], "{returned:?}");
}

#[test]
fn culled_surfaces_skip_repaints_and_refresh_stale_content_on_return() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    set_policy(&mut world, entity, Some(ALWAYS));

    frame(&mut renderer, &mut world, 0.1);
    let begins = state.cache_begins.get();

    // Behind the camera, edits and elapsed time cause no cache work.
    place(&mut world, entity, 10.0);
    for step in 2..5 {
        recolor(&mut world, entity, step);
        let culled = frame(&mut renderer, &mut world, 0.5);
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
    let visible = frame(&mut renderer, &mut world, 0.0);
    assert_eq!(work(&visible), [1, 0, 0, 0, 0], "{visible:?}");
}

#[test]
fn removal_policy_removal_and_forgetting_the_world_release_images() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    set_policy(&mut world, entity, Some(ALWAYS));

    frame(&mut renderer, &mut world, 0.1);
    assert_eq!(state.cache_targets_live.get(), 1);

    // Removing the policy releases the image after the frame.
    set_policy(&mut world, entity, None);
    let direct = frame(&mut renderer, &mut world, 0.1);
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
    set_policy(&mut world, entity, Some(ALWAYS));
    frame(&mut renderer, &mut world, 0.1);
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(entity),
            }],
        })
        .unwrap();
    let removed = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(removed.surface_cache_entries, 0);
    assert_eq!(state.cache_targets_live.get(), 0);

    let recreated = add_text_surface(&mut world, 0.0);
    assert_ne!(recreated, entity);
    set_policy(&mut world, recreated, Some(ALWAYS));
    let fresh = frame(&mut renderer, &mut world, 0.1);
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
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    set_policy(&mut world, entity, Some(ALWAYS));

    *state.fail_cache_create.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let failed = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&failed), [0, 0, 1, 1, 0], "{failed:?}");
    assert_eq!(take_events(&state), format!("F{TEXT}"));
    assert_eq!(
        presentation(&renderer, world_id),
        SurfaceCachePresentation::Fallback
    );
    assert_eq!(presentation(&renderer, world_id).code(), 2);

    // Direct presentation continues through the back-off without retrying.
    *state.fail_cache_create.borrow_mut() = None;
    frame(&mut renderer, &mut world, 0.5);
    assert_eq!(state.cache_creates.get(), 1);
    let recovered = frame(&mut renderer, &mut world, 0.5);
    assert_eq!(work(&recovered), [1, 0, 0, 0, 1], "{recovered:?}");

    // A failed begin releases the image and presents directly.
    *state.fail_cache_begin.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    recolor(&mut world, entity, 1);
    let begin = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&begin), [0, 0, 1, 1, 0], "{begin:?}");
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(!state.cache_target_bound.get());
    *state.fail_cache_begin.borrow_mut() = None;

    // A failed end marks the image unusable the same way.
    *state.fail_cache_end.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let end = frame(&mut renderer, &mut world, 1.0);
    assert_eq!(work(&end), [0, 0, 1, 1, 1], "{end:?}");
    assert_eq!(state.cache_targets_live.get(), 0);
    *state.fail_cache_end.borrow_mut() = None;

    // A failed composite draws the Surface directly in its slot.
    frame(&mut renderer, &mut world, 1.0);
    take_events(&state);
    *state.fail_cache_composite.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let composite = frame(&mut renderer, &mut world, 0.0);
    assert_eq!(work(&composite), [0, 0, 1, 1, 0], "{composite:?}");
    assert_eq!(take_events(&state), format!("FC{TEXT}"));
    *state.fail_cache_composite.borrow_mut() = None;
    let later = frame(&mut renderer, &mut world, 1.0);
    assert_eq!(work(&later), [1, 0, 0, 0, 1], "{later:?}");
}

#[test]
fn context_loss_fails_the_frame_and_recovery_repaints() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    {
        let mut world = host.world_mut(world_id).unwrap();
        set_policy(&mut world, entity, Some(ALWAYS));
        frame(&mut renderer, &mut world, 0.1);
        *state.fail_cache_begin.borrow_mut() = Some(RenderError::ContextLost);
        recolor(&mut world, entity, 1);
        let lost = try_frame(&mut renderer, &mut world, 0.1);
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
    let recovered = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&recovered), [1, 0, 0, 0, 1], "{recovered:?}");
    assert_eq!(state.cache_targets_live.get(), 1);

    // Context loss while allocating fails the frame without leaking state.
    drop(world);
    recover_context(&mut renderer, &mut host, world_id, &surface_font());
    let mut world = host.world_mut(world_id).unwrap();
    *state.fail_cache_create.borrow_mut() = Some(RenderError::ContextLost);
    let lost = try_frame(&mut renderer, &mut world, 0.1);
    assert_eq!(lost, Err(RenderError::ContextLost));
    *state.fail_cache_create.borrow_mut() = None;
}

/// An opted-in Surface holding one drawing, with the drawing loaded.
fn drawing_scene(
    host: &mut ipp_core::HostRuntime,
) -> (
    RenderService<TestDevice>,
    std::rc::Rc<DeviceState>,
    WorldId,
    EntityId,
) {
    host.data_sources_mut()
        .register_stream("fixture://")
        .unwrap();
    let (mut world, renderer, state) = setup(host);
    state.cache_limit.set(4096);
    let world_id = world.id();
    let mut surface = Surface::default();
    surface
        .insert_item(
            0,
            SurfaceItemContent::Drawing,
            SurfaceItemStyle {
                asset: Some(AssetSource {
                    kind: DRAWING_TYPE,
                    uri: "fixture:///drawing.ippd".into(),
                    variant: 0,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    let entity = create(
        &mut world,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Surface(surface),
            ComponentValue::BoundingGeometry(Default::default()),
            ComponentValue::SurfaceCache(ALWAYS),
        ],
    );
    drop(world);
    (renderer, state, world_id, entity)
}

/// Deliver pending resource requests with the drawing fixture.
fn deliver_drawing(host: &mut ipp_core::HostRuntime) -> usize {
    host.progress_assets();
    let requests = host.take_resource_requests();
    for request in &requests {
        host.complete_resource(request.id, Ok(surface_drawing()))
            .unwrap();
    }
    requests.len()
}

fn drawing_primitives(host: &mut ipp_core::HostRuntime, world: WorldId, entity: EntityId) -> usize {
    host.world_mut(world)
        .unwrap()
        .surface_render_items()
        .iter()
        .find(|item| item.entity == entity)
        .map_or(0, |item| item.primitives.len())
}

#[test]
fn an_image_missing_gpu_data_after_device_replacement_repaints_once_it_is_resident() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = drawing_scene(&mut host);
    for _ in 0..16 {
        deliver_drawing(&mut host);
        update(&mut host.world_mut(world_id).unwrap()).unwrap();
        if drawing_primitives(&mut host, world_id, entity) == 1 {
            break;
        }
    }
    assert_eq!(drawing_primitives(&mut host, world_id, entity), 1);

    let mut world = host.world_mut(world_id).unwrap();
    let cold = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(
        state.surface_path_draws.get(),
        1,
        "the repaint drew the drawing"
    );
    let before = revisions(&world, entity);
    drop(world);

    // Replacing the device releases every image and the drawing's GPU data;
    // its CPU data and identity remain, so the prepared item is unchanged.
    renderer
        .replace_device(&mut host, TestDevice(state.clone()))
        .unwrap();
    assert_eq!(state.cache_targets_live.get(), 0);

    // The first frames repaint without the drawing, which is not resident
    // until the Host delivers its bytes again. The image matches direct
    // presentation, so it is reused past many refresh intervals.
    let mut world = host.world_mut(world_id).unwrap();
    let draws = state.surface_path_draws.get();
    let recovered = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&recovered), [1, 0, 0, 0, 1], "{recovered:?}");
    assert_eq!(
        state.surface_path_draws.get(),
        draws,
        "drawing not resident yet"
    );
    for _ in 0..3 {
        let waiting = frame(&mut renderer, &mut world, 0.5);
        assert_eq!(work(&waiting), [0, 1, 0, 0, 0], "{waiting:?}");
    }
    assert_eq!(revisions(&world, entity), before);
    drop(world);

    // Once the drawing is resident again, the next frame repaints even with a
    // frozen clock, and the complete image is then reused.
    assert!(
        deliver_drawing(&mut host) > 0,
        "the Host reloads the drawing"
    );
    let mut world = host.world_mut(world_id).unwrap();
    let arrived = frame(&mut renderer, &mut world, 0.0);
    assert_eq!(work(&arrived), [1, 0, 0, 0, 0], "{arrived:?}");
    assert_eq!(
        state.surface_path_draws.get(),
        draws + 1,
        "the repaint drew it"
    );
    assert_eq!(revisions(&world, entity), before);
    let warm = frame(&mut renderer, &mut world, 0.5);
    assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
    assert_eq!(state.surface_path_draws.get(), draws + 1);
}

#[test]
fn a_drawing_still_loading_at_the_first_repaint_repaints_on_arrival() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = drawing_scene(&mut host);
    assert_eq!(drawing_primitives(&mut host, world_id, entity), 0);

    // Never resident: the first image is painted without the drawing.
    let mut world = host.world_mut(world_id).unwrap();
    let cold = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(state.surface_path_draws.get(), 0);
    let waiting = frame(&mut renderer, &mut world, 0.5);
    assert_eq!(work(&waiting), [0, 1, 0, 0, 0], "{waiting:?}");
    drop(world);

    // Arrival repaints on the frame that sees it, with a frozen clock.
    let mut arrived = None;
    for _ in 0..8 {
        deliver_drawing(&mut host);
        let mut world = host.world_mut(world_id).unwrap();
        let stats = frame(&mut renderer, &mut world, 0.0);
        if state.surface_path_draws.get() > 0 {
            arrived = Some(stats);
            break;
        }
        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "{stats:?}");
    }
    let arrived = arrived.expect("the drawing arrived");
    assert_eq!(drawing_primitives(&mut host, world_id, entity), 1);
    assert_eq!(work(&arrived), [1, 0, 0, 0, 0], "{arrived:?}");
    assert_eq!(state.surface_path_draws.get(), 1);
    let mut world = host.world_mut(world_id).unwrap();
    let warm = frame(&mut renderer, &mut world, 0.5);
    assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
}

#[test]
fn budget_pressure_evicts_idle_images_then_falls_back() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, first) = scene(&mut host);
    let mut world = host.world_mut(world_id).unwrap();
    let second = add_text_surface(&mut world, -1.0);
    set_policy(&mut world, first, Some(ALWAYS));
    set_policy(&mut world, second, Some(ALWAYS));
    renderer.set_surface_cache_budget(4 * 64 * 64);

    // Only one image fits: entity order admits the first, the second falls back.
    let both = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&both), [1, 0, 1, 1, 1], "{both:?}");
    assert_eq!(both.surface_cache_resident_bytes, 4 * 64 * 64);

    // With the first culled, its idle image makes room for the second.
    place(&mut world, first, 10.0);
    let swapped = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&swapped), [1, 0, 0, 0, 1], "{swapped:?}");
    assert_eq!(state.cache_targets_live.get(), 1);
    let records = diagnostics(&renderer, world_id);
    let size = |entity| records.iter().find(|r| r.entity == entity).unwrap().size;
    assert_eq!((size(first), size(second)), ([0, 0], [64, 64]));
}

#[test]
fn worlds_share_the_context_budget() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, first, entity) = scene(&mut host);
    set_policy(&mut host.world_mut(first).unwrap(), entity, Some(ALWAYS));
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
                ComponentValue::SurfaceCache(ALWAYS),
            ],
        );
    }
    for _ in 0..8 {
        host.progress_assets();
        let mut world = host.world_mut(second).unwrap();
        update(&mut world).unwrap();
    }

    let mut world = host.world_mut(first).unwrap();
    frame(&mut renderer, &mut world, 0.1);
    drop(world);

    // The second World's image evicts the first World's image, which its
    // frame did not present.
    let mut world = host.world_mut(second).unwrap();
    let stats = frame(&mut renderer, &mut world, 0.1);
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
    frame(&mut renderer, &mut world, 0.1);
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
    set_policy(&mut world, far, Some(ALWAYS));

    take_events(&state);
    frame(&mut renderer, &mut world, 0.1);
    // The far image repaints before the frame; the main pass composites it
    // behind the nearer direct Surface.
    assert_eq!(take_events(&state), format!("B{TEXT}EFC{TEXT}"));
    let warm = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(take_events(&state), format!("FC{TEXT}"));
    assert_eq!(work(&warm), [0, 1, 0, 0, 0]);

    // Swapping depths swaps the slots.
    place(&mut world, near, -6.0);
    place(&mut world, far, 0.0);
    frame(&mut renderer, &mut world, 0.1);
    assert_eq!(take_events(&state), format!("F{TEXT}C"));
}

#[cfg(feature = "gui")]
#[test]
fn text_drawn_analytically_under_the_population_bound_is_refined_next_frame() {
    let budget = ipp_render_gl::glyph_atlas::MIN_POPULATES_PER_FRAME as u32;
    let ids: Vec<u32> = (0..budget + 8).collect();
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) =
        text_run_scene(&mut host, glyph_font(budget + 8, 1000, 1.0), &ids);
    // Only the per-frame floor populates, so the first repaint defers glyphs.
    renderer.set_glyph_population_budget_ms(0.0);
    state.cache_limit.set(4096);
    let mut world = host.world_mut(world_id).unwrap();
    set_policy(&mut world, entity, Some(ALWAYS));

    // The first repaint populates up to the bound and draws the run analytically.
    let cold = frame(&mut renderer, &mut world, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(cold.glyph_populates, budget);
    assert_eq!(state.analytic_glyph_draws.get(), 1);

    // The next frame repaints with a frozen clock, populating the rest, and the
    // run now samples the atlas as direct presentation would.
    let refined = frame(&mut renderer, &mut world, 0.0);
    assert_eq!(work(&refined), [1, 0, 0, 0, 0], "{refined:?}");
    assert_eq!(refined.glyph_populates, 8);
    assert_eq!(state.analytic_glyph_draws.get(), 1);
    assert_eq!(state.glyph_batch_draws.get(), 1);

    let warm = frame(&mut renderer, &mut world, 0.0);
    assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
    assert_eq!((warm.glyph_misses, warm.glyph_populates), (0, 0));
}
