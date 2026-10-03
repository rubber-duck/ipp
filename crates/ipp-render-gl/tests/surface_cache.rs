//! Canvas output caching through real Worlds and a failure-injecting render device.
//! Ordinary parent SurfaceCache components opt in; completed attachment policy and
//! child Canvas paint/resource/interaction publications drive the retained images.
//! The maintained GLES publication scenarios own real image evidence.

mod support;

use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, SurfaceCache, WorldContext, WorldId,
};
use ipp_core::{Surface, components::Transform};
use ipp_render_gl::{RenderError, RenderService, SurfaceCacheDiagnostic, SurfaceCachePresentation};
use support::canvas::{self, CanvasSurface};
use support::selection::{ATTACHMENTS, CAMERA, RENDER, SURFACE, select};
use support::selection::{CONSTRAINTS, GEOMETRY};
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
                Some(policy) => Command::insert_value(
                    EntityRef::Handle(entity),
                    ComponentValue::SurfaceCache(policy),
                ),
                None => Command::RemoveComponent {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::SURFACE_CACHE,
                },
            }],
        })
        .unwrap();
    update(world).unwrap();
}

fn cache_policy(
    host: &mut ipp_core::HostRuntime,
    surface: CanvasSurface,
    policy: Option<SurfaceCache>,
) {
    set_policy(
        &mut host.world_mut(surface.parent).unwrap(),
        surface.anchor,
        policy,
    );
}

fn recolor(host: &mut ipp_core::HostRuntime, surface: CanvasSurface, step: u32) {
    let shade = (step % 97) as f32 / 97.0;
    surface.set_style(
        host,
        ipp_core::components::CanvasStyle {
            x: 0.5,
            y: 0.5,
            red: shade,
            green: 1.0 - shade,
            blue: 0.5,
            ..Default::default()
        },
    );
}

fn move_surface(host: &mut ipp_core::HostRuntime, surface: CanvasSurface, z: f32) {
    place(
        &mut host.world_mut(surface.parent).unwrap(),
        surface.anchor,
        z,
    );
}

fn canvas_revisions(host: &ipp_core::HostRuntime, surface: CanvasSurface) -> (u64, u64) {
    let publication = surface.publication(host);
    (publication.paint_revision, publication.resource_revision)
}

/// Advance the World by `dt` seconds, then render its prepared Surfaces.
fn try_frame(
    renderer: &mut RenderService<TestDevice>,
    host: &mut ipp_core::HostRuntime,
    world: WorldId,
    dt: f64,
) -> Result<FrameStats, RenderError> {
    host.frame(dt).unwrap();
    renderer.prepare(
        host,
        host.root_output(world)
            .map(|(output, _, publication)| (output, publication)),
    )?;
    host.progress_assets();
    renderer.draw_stats(host, world, VIEWPORT, VIEWPORT)
}

fn frame(
    renderer: &mut RenderService<TestDevice>,
    host: &mut ipp_core::HostRuntime,
    world: WorldId,
    dt: f64,
) -> FrameStats {
    try_frame(renderer, host, world, dt).unwrap()
}

fn diagnostics(
    renderer: &RenderService<TestDevice>,
    world: WorldId,
) -> Vec<SurfaceCacheDiagnostic> {
    let mut out = Vec::new();
    renderer.surface_cache_diagnostics(world, &mut out);
    out
}

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
fn work(stats: &FrameStats) -> [u32; 5] {
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
    let text = state.glyph_batch_draws.get();
    state.surface_path_draws.get() + state.analytic_glyph_draws.get() + text
}

fn take_events(state: &DeviceState) -> String {
    std::mem::take(&mut *state.surface_events.borrow_mut())
}

/// Main-pass retained Canvas glyph atlas batch, independent of GUI controls.
const TEXT: char = 'T';

/// A text Surface in front of the camera with caching enabled on the device.
fn scene(
    host: &mut ipp_core::HostRuntime,
) -> (
    RenderService<TestDevice>,
    std::rc::Rc<DeviceState>,
    WorldId,
    CanvasSurface,
) {
    let (renderer, state, world_id, entity) = text_run_scene(host, surface_font(), &[0]);
    state.cache_limit.set(4096);
    (renderer, state, world_id, entity)
}

fn add_text_surface(host: &mut ipp_core::HostRuntime, parent: WorldId, z: f32) -> CanvasSurface {
    let surface = CanvasSurface::new(host, parent, z, canvas::glyph_run(&[0], [0.5, 0.5]));
    resolve_text(host, parent, &surface_font());
    assert_eq!(surface.publication(host).entries.len(), 1);
    surface
}

#[test]
fn absent_policies_make_no_cache_calls() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);

    for _ in 0..3 {
        let stats = render_frame(&mut renderer, &mut host, world_id, VIEWPORT, VIEWPORT).unwrap();
        assert_eq!(work(&stats), [0; 5]);
        assert_eq!(
            (
                stats.surface_cache_entries,
                stats.surface_cache_resident_bytes
            ),
            (0, 0)
        );
        let stats = frame(&mut renderer, &mut host, world_id, 0.1);
        assert_eq!(work(&stats), [0; 5]);
    }

    // Opting in and back out again leaves no cache state behind.
    cache_policy(&mut host, entity, Some(ALWAYS));
    cache_policy(&mut host, entity, None);
    let stats = frame(&mut renderer, &mut host, world_id, 0.1);
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

fn observed_frame(
    renderer: &mut RenderService<TestDevice>,
    host: &mut ipp_core::HostRuntime,
    world: WorldId,
    child: ipp_core::OutputRef,
    dt: f64,
) -> [ipp_core::OutputPublicationObservation; 2] {
    host.frame(dt).unwrap();
    let (output, viewport, publication) = host.root_output(world).unwrap();
    renderer.prepare(host, Some((output, publication))).unwrap();
    host.progress_assets();
    let mut observations = [output, child].map(|output| ipp_core::OutputPublicationObservation {
        output,
        publication: None,
    });
    let summary = renderer
        .draw_observed(
            host,
            output,
            publication,
            viewport,
            host.publication(publication).unwrap().time,
            &mut observations,
        )
        .unwrap();
    assert!(!summary.invalid_camera);
    assert_eq!(summary.failed_draw_calls, 0);
    observations
}

#[test]
fn included_cache_witness_reuses_equivalent_content_but_never_relabels_stale_pixels() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, _, world, surface) = scene(&mut host);
    cache_policy(&mut host, surface, Some(ALWAYS));
    frame(&mut renderer, &mut host, world, 0.01);
    let painted = record(&renderer, world, surface.anchor).repaints;
    let first = observed_frame(&mut renderer, &mut host, world, surface.output, 0.001);
    assert!(first.iter().all(|source| source.publication.is_some()));
    let second = observed_frame(&mut renderer, &mut host, world, surface.output, 0.001);
    assert!(second.iter().all(|source| source.publication.is_some()));
    assert_ne!(first[1].publication, second[1].publication);
    let earlier = host
        .publication(second[1].publication.unwrap())
        .unwrap()
        .tick;
    assert!(earlier > 0);
    assert_eq!(record(&renderer, world, surface.anchor).repaints, painted);

    recolor(&mut host, surface, 42);
    let stale = observed_frame(&mut renderer, &mut host, world, surface.output, 0.001);
    assert_eq!(stale.map(|source| source.publication), [None, None]);
    assert_eq!(record(&renderer, world, surface.anchor).repaints, painted);
    let fresh = observed_frame(&mut renderer, &mut host, world, surface.output, 0.2);
    assert!(fresh.iter().all(|source| source.publication.is_some()));
    assert!(
        host.publication(fresh[1].publication.unwrap())
            .unwrap()
            .tick
            > earlier
    );
    assert_eq!(
        record(&renderer, world, surface.anchor).repaints,
        painted + 1
    );
}

#[test]
fn cache_preparation_of_an_uncomposited_output_is_not_inclusion() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, _, world, surface) = scene(&mut host);
    cache_policy(&mut host, surface, Some(ALWAYS));
    assert!(
        observed_frame(&mut renderer, &mut host, world, surface.output, 0.01)[1]
            .publication
            .is_some()
    );
    surface.set_style(
        &mut host,
        ipp_core::components::CanvasStyle {
            opacity: 0.0,
            ..Default::default()
        },
    );
    move_surface(&mut host, surface, 1000.0);
    let hidden = observed_frame(&mut renderer, &mut host, world, surface.output, 0.2);
    assert!(hidden[0].publication.is_some());
    assert!(hidden[1].publication.is_none());
}

#[test]
fn stale_cached_sibling_does_not_hide_a_current_child_witness() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, _, world, stale) = scene(&mut host);
    let healthy = add_text_surface(&mut host, world, -4.0);
    cache_policy(&mut host, stale, Some(ALWAYS));
    cache_policy(&mut host, healthy, Some(ALWAYS));
    frame(&mut renderer, &mut host, world, 0.01);
    recolor(&mut host, stale, 42);
    let observations = observed_frame(&mut renderer, &mut host, world, healthy.output, 0.001);
    assert!(observations[0].publication.is_none());
    assert!(observations[1].publication.is_some());
}

#[test]
fn repaint_containing_a_stale_nested_image_cannot_claim_current_provenance() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, _, world, outer) = scene(&mut host);
    let inner = CanvasSurface::new(
        &mut host,
        outer.output.world().id(),
        0.0,
        vec![ComponentValue::CanvasBox(Default::default())],
    );
    cache_policy(&mut host, outer, Some(ALWAYS));
    cache_policy(
        &mut host,
        inner,
        Some(SurfaceCache {
            max_refresh_hz: 1.0,
            ..ALWAYS
        }),
    );
    let initial = observed_frame(&mut renderer, &mut host, world, inner.output, 0.01);
    assert!(initial.iter().all(|source| source.publication.is_some()));
    let inner_repaints = record(&renderer, inner.parent, inner.anchor).repaints;
    let outer_repaints = record(&renderer, outer.parent, outer.anchor).repaints;
    recolor(&mut host, inner, 42);
    recolor(&mut host, outer, 43);
    let stale = observed_frame(&mut renderer, &mut host, world, inner.output, 0.11);
    assert_eq!(stale.map(|source| source.publication), [None, None]);
    assert_eq!(
        record(&renderer, inner.parent, inner.anchor).repaints,
        inner_repaints
    );
    assert_eq!(
        record(&renderer, outer.parent, outer.anchor).repaints,
        outer_repaints + 1
    );
    observed_frame(&mut renderer, &mut host, world, inner.output, 0.95);
    let refreshed = observed_frame(&mut renderer, &mut host, world, inner.output, 0.11);
    assert!(refreshed.iter().all(|source| source.publication.is_some()));
}

#[test]
fn warm_frames_composite_the_image_without_surface_work() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    cache_policy(&mut host, entity, Some(ALWAYS));

    let cold = frame(&mut renderer, &mut host, world_id, 0.1);
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
    assert_eq!(cold.glyph_populates, 1);

    let draws = surface_draws(&state);
    for _ in 0..3 {
        let warm = frame(&mut renderer, &mut host, world_id, 0.1);
        assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
        assert_eq!((warm.draw_calls, warm.triangles), (1, 2));
        assert_eq!(warm.uploaded_bytes, 0);
        assert_eq!(take_events(&state), "FC");
    }

    assert_eq!(surface_draws(&state), draws);
    assert_eq!(state.cache_begins.get(), 1);
    assert_eq!(state.cache_composites.get(), 4);
    assert!(!state.cache_target_bound.get());
    assert!(!state.atlas_target_bound.get());

    let [diagnostic] = diagnostics(&renderer, world_id)[..] else {
        panic!("one entry");
    };
    assert_eq!(diagnostic.entity, entity.anchor);
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
    cache_policy(&mut host, entity, Some(BANDED));

    // Five metres: band 2 at 32 texels per metre.
    frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(diagnostics(&renderer, world_id)[0].size, [32, 32]);
    let revisions_before = canvas_revisions(&host, entity);

    // Moving inside the band, and just past its boundary, only composites.
    for z in [-1.0, 0.5, 1.2, 0.8] {
        move_surface(&mut host, entity, z);
        let stats = frame(&mut renderer, &mut host, world_id, 0.1);
        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "z {z}: {stats:?}");
    }
    // Placement never advances the prepared revisions.
    assert_eq!(canvas_revisions(&host, entity), revisions_before);

    // Three metres crosses the hysteresis margin into band 1.
    move_surface(&mut host, entity, 2.0);
    let nearer = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&nearer), [1, 0, 0, 0, 1], "{nearer:?}");
    assert_eq!(diagnostics(&renderer, world_id)[0].size, [64, 64]);
    assert_eq!(nearer.surface_cache_resident_bytes, 4 * 64 * 64);
    assert_eq!(
        (state.cache_creates.get(), state.cache_resizes.get()),
        (1, 1)
    );

    // One and a half metres is inside the direct distance.
    move_surface(&mut host, entity, 3.5);
    let near = frame(&mut renderer, &mut host, world_id, 0.1);
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
    cache_policy(&mut host, entity, Some(ALWAYS));

    frame(&mut renderer, &mut host, world_id, 0.1);

    // A frozen World clock never refreshes the displayed image.
    for step in 2..6 {
        let (paint, _) = canvas_revisions(&host, entity);
        recolor(&mut host, entity, step);
        let stats = frame(&mut renderer, &mut host, world_id, 0.0);
        assert!(canvas_revisions(&host, entity).0 > paint, "edit {step}");
        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "{stats:?}");
    }

    // Edits within 0.1 s of World time coalesce into one repaint of the latest.
    recolor(&mut host, entity, 6);
    assert_eq!(
        frame(&mut renderer, &mut host, world_id, 0.05).surface_cache_repaints,
        0
    );
    recolor(&mut host, entity, 7);
    assert_eq!(
        frame(&mut renderer, &mut host, world_id, 0.05).surface_cache_repaints,
        1
    );
    assert_eq!(
        frame(&mut renderer, &mut host, world_id, 0.5).surface_cache_reuses,
        1
    );

    // Continuous animation reaches every refresh opportunity.
    let mut repaints = 0;
    for step in 0..60 {
        recolor(&mut host, entity, 100 + step);
        repaints += frame(&mut renderer, &mut host, world_id, 1.0 / 60.0).surface_cache_repaints;
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
    cache_policy(&mut host, entity, Some(ALWAYS));

    frame(&mut renderer, &mut host, world_id, 0.1);
    let (_, resource) = canvas_revisions(&host, entity);

    // A pending font removes the glyph run's identity from the item: the
    // frozen clock proves the repaint bypasses the refresh interval.
    let mut values = canvas::glyph_run(&[0], [0.5, 0.5]);
    let ComponentValue::CanvasGlyphRun(run) = &mut values[0] else {
        unreachable!()
    };
    run.source = "fixture:///pending-font.ippf".into();
    canvas::apply(
        &mut host,
        entity.output.world().id(),
        vec![Command::insert_value(
            EntityRef::Handle(entity.content),
            values.remove(0),
        )],
    );
    let stats = frame(&mut renderer, &mut host, world_id, 0.0);
    assert!(canvas_revisions(&host, entity).1 > resource);
    assert_eq!(work(&stats), [1, 0, 0, 0, 0], "{stats:?}");
}

const GUI_SESSION: u64 = 7;

/// The exact checkbox identity a Host names in GUI actions.
fn checkbox_target(
    host: &mut ipp_core::HostRuntime,
    surface: CanvasSurface,
) -> ipp_core::systems::gui::local::GuiEntityTarget {
    let world = surface.output.world();
    let incarnation = host
        .world_mut(world.id())
        .unwrap()
        .component_incarnation(surface.content, ComponentValue::GUI_CHECKBOX)
        .unwrap();
    ipp_core::systems::gui::local::GuiEntityTarget {
        world,
        entity: surface.content,
        component: ComponentValue::GUI_CHECKBOX,
        incarnation,
    }
}

/// Queue one `GuiAction` command on the panel's checkbox; the next frame applies it.
fn checkbox_action(
    host: &mut ipp_core::HostRuntime,
    surface: CanvasSurface,
    id: u64,
    action: ipp_core::systems::gui::local::GuiLocalAction,
) {
    let target = checkbox_target(host, surface);
    host.world_mut(surface.output.world().id())
        .unwrap()
        .enqueue(Batch {
            id,
            operations: vec![Command::GuiAction {
                target: ipp_core::GuiActionTarget {
                    entity: EntityRef::Handle(target.entity),
                    component: target.component,
                    incarnation: target.incarnation,
                },
                action,
            }],
        })
        .unwrap();
}

/// Logical focus on the surface's control, through the `GuiFocus` System query.
fn control_focused(host: &mut ipp_core::HostRuntime, surface: CanvasSurface) -> bool {
    host.world_mut(surface.output.world().id())
        .unwrap()
        .gui_focus_page(0, surface.content.to_bits(), 1)
        .iter()
        .any(|record| record.target.entity == surface.content)
}

/// Pointer hover on the surface's control, through the `GuiPointers` System query.
fn control_hovered(host: &mut ipp_core::HostRuntime, surface: CanvasSurface) -> bool {
    host.world_mut(surface.output.world().id())
        .unwrap()
        .gui_pointer_page(0, surface.content.to_bits(), 256)
        .iter()
        .any(|record| record.state.hovered)
}

#[test]
fn focus_takes_interaction_priority_until_the_control_is_disabled() {
    use ipp_core::components::{GuiBehavior, GuiCheckbox, GuiLayout};
    use ipp_core::systems::gui::local::GuiLocalAction;

    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, _state, world_id, _) = scene(&mut host);
    let panel = CanvasSurface::new(
        &mut host,
        world_id,
        -1.0,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 1.0,
                height: 1.0,
                ..Default::default()
            }),
        ],
    );
    cache_policy(&mut host, panel, Some(ALWAYS));
    let cold = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1]);
    checkbox_action(&mut host, panel, 1, GuiLocalAction::Focus(0));
    let focused = frame(&mut renderer, &mut host, world_id, 0.01);
    assert_eq!(work(&focused), [0, 0, 1, 0, 0]);
    assert!(panel.publication(&host).interaction.focused);

    host.world_mut(panel.output.world().id())
        .unwrap()
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::insert_value(
                EntityRef::Handle(panel.content),
                ComponentValue::GuiBehavior(GuiBehavior {
                    enabled: false,
                    ..Default::default()
                }),
            )],
        })
        .unwrap();
    let revalidated = frame(&mut renderer, &mut host, world_id, 0.01);
    assert_eq!(revalidated.surface_cache_direct, 0);
    assert!(!panel.publication(&host).interaction.focused);
    assert!(!control_focused(&mut host, panel));
}

#[test]
fn faulted_focused_canvas_cannot_retain_interaction_cache_priority() {
    faulted_focus(FaultLocation::Canvas);
}

#[test]
fn faulted_containing_camera_cannot_retain_descendant_interaction_cache_priority() {
    faulted_focus(FaultLocation::RootCamera);
}

#[test]
fn faulted_spatial_ancestor_preserves_only_healthy_sibling_interaction_priority() {
    faulted_focus(FaultLocation::SpatialAncestor);
}

#[test]
fn faulted_nested_camera_preserves_only_healthy_sibling_interaction_priority() {
    faulted_focus(FaultLocation::NestedCamera);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FaultLocation {
    Canvas,
    RootCamera,
    SpatialAncestor,
    NestedCamera,
}

fn faulted_focus(location: FaultLocation) {
    use ipp_core::ErrorReason;
    use ipp_core::components::{GuiCheckbox, GuiLayout, Scalar};
    use ipp_core::systems::gui::local::GuiLocalAction;
    use ipp_core::systems::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct Factory(Arc<AtomicBool>);

    struct CleanupFault(Arc<AtomicBool>, usize);

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            SystemId("fixture.focused-canvas-commit-fault")
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(CleanupFault(self.0.clone(), 0)))
        }
    }

    impl System for CleanupFault {
        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

        fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
            if !self.0.load(Ordering::Relaxed) {
                return;
            }
            let targets: Vec<_> = context
                .changed_components()
                .filter(|(_, component)| *component == ComponentValue::SCALAR)
                .map(|(entity, _)| entity)
                .collect();
            for entity in targets {
                self.1 += 1;
                context.restore_evaluated_component(
                    entity,
                    ComponentValue::Scalar(Scalar {
                        value: self.1 as f32 + 100.0,
                    }),
                );
            }
        }
    }

    let enabled = Arc::new(AtomicBool::new(false));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(enabled.clone())));
    let mut host = ipp_core::HostRuntime::with_system_factories(factories).unwrap();
    // The scene World also holds the faulting Scalar oscillator.
    let systems = [scene_systems(&host), CONSTRAINTS.to_vec()].concat();
    let (mut renderer, state, parent, _) =
        text_run_scene_with(&mut host, surface_font(), &[0], &systems);
    state.cache_limit.set(4096);
    let (panel_parent, ancestor) = match location {
        FaultLocation::Canvas | FaultLocation::RootCamera => (parent, parent),
        FaultLocation::SpatialAncestor => {
            let middle = host
                .create_world(
                    Default::default(),
                    &[
                        select(&[ATTACHMENTS, CONSTRAINTS]),
                        vec![SystemId("fixture.focused-canvas-commit-fault")],
                    ]
                    .concat(),
                )
                .unwrap();
            let owner = host
                .create_world(
                    Default::default(),
                    &select(&[ATTACHMENTS, GEOMETRY, SURFACE]),
                )
                .unwrap();
            for (parent, child) in [(parent, middle), (middle, owner)] {
                let child = host.world_ref(child).unwrap();
                create(
                    &mut host.world_mut(parent).unwrap(),
                    vec![ComponentValue::WorldAttachment(
                        ipp_core::WorldAttachment::spatial(child),
                    )],
                );
            }
            (owner, middle)
        }
        FaultLocation::NestedCamera => {
            let camera_world = host
                .create_world(
                    Default::default(),
                    &[
                        select(&[ATTACHMENTS, CAMERA, CONSTRAINTS, RENDER, SURFACE]),
                        vec![SystemId("fixture.focused-canvas-commit-fault")],
                    ]
                    .concat(),
                )
                .unwrap();
            let camera = create(
                &mut host.world_mut(camera_world).unwrap(),
                vec![ComponentValue::Camera(
                    ipp_core::components::Camera::default(),
                )],
            );
            let output = host
                .bind_output(
                    host.world_ref(camera_world).unwrap(),
                    camera,
                    ipp_core::OutputKind::Camera,
                )
                .unwrap();
            create(
                &mut host.world_mut(parent).unwrap(),
                vec![
                    ComponentValue::Surface(Surface::default()),
                    ComponentValue::Transform(Transform {
                        z: -1.0,
                        ..Default::default()
                    }),
                    ComponentValue::WorldAttachment(ipp_core::WorldAttachment::surface(output)),
                ],
            );
            (camera_world, camera_world)
        }
    };
    let control = || {
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 1.0,
                height: 1.0,
                ..Default::default()
            }),
        ]
    };
    // A faulting Canvas holds the Scalar oscillator in its own panel World.
    let panel_systems = if location == FaultLocation::Canvas {
        [
            support::canvas::panel_systems(),
            CONSTRAINTS.to_vec(),
            vec![SystemId("fixture.focused-canvas-commit-fault")],
        ]
        .concat()
    } else {
        support::canvas::panel_systems()
    };
    let panel = CanvasSurface::new_in(&mut host, panel_parent, -1.0, control(), &panel_systems);
    cache_policy(&mut host, panel, Some(ALWAYS));
    let sibling = matches!(
        location,
        FaultLocation::SpatialAncestor | FaultLocation::NestedCamera
    )
    .then(|| {
        let panel = CanvasSurface::new(&mut host, parent, -1.0, control());
        cache_policy(&mut host, panel, Some(ALWAYS));
        panel
    });
    let fault_world = if location == FaultLocation::Canvas {
        panel.output.world().id()
    } else {
        ancestor
    };
    let oscillator = create(
        &mut host.world_mut(fault_world).unwrap(),
        vec![ComponentValue::Scalar(Scalar::default())],
    );
    assert_eq!(
        frame(&mut renderer, &mut host, parent, 0.1).surface_cache_repaints,
        1 + u32::from(sibling.is_some())
    );
    for focused in std::iter::once(panel).chain(sibling) {
        checkbox_action(&mut host, focused, 1, GuiLocalAction::Focus(0));
    }
    assert_eq!(
        frame(&mut renderer, &mut host, parent, 0.01).surface_cache_direct,
        1 + u32::from(sibling.is_some())
    );
    assert!(panel.publication(&host).interaction.focused);
    let publication = host.latest_publication(panel.output.world().id()).unwrap();

    enabled.store(true, Ordering::Relaxed);
    host.world_mut(fault_world)
        .unwrap()
        .enqueue(Batch {
            id: 90,
            operations: vec![Command::insert_value(
                EntityRef::Handle(oscillator),
                ComponentValue::Scalar(Scalar {
                    value: 1.0,
                }),
            )],
        })
        .unwrap();
    let report = host.frame(0.1).unwrap();
    let outcome = &report.worlds[&fault_world].as_ref().unwrap().outcomes[0];
    assert_eq!(
        outcome.result.as_ref().unwrap_err().reason,
        ErrorReason::NonConvergentCommit
    );
    assert!(!report.evaluation_order.contains(&fault_world));
    assert_eq!(
        report.evaluation_order.contains(&parent),
        fault_world != parent
    );
    assert_eq!(
        host.world_fault(host.world_ref(fault_world).unwrap()),
        Ok(Some(ErrorReason::NonConvergentCommit))
    );
    if location == FaultLocation::Canvas {
        assert_eq!(
            host.latest_publication(panel.output.world().id()),
            Some(publication)
        );
    } else {
        assert_eq!(host.world_fault(panel.output.world()), Ok(None));
    }
    assert!(host.output(publication, panel.output).is_some());
    assert!(panel.publication(&host).interaction.focused);

    let (selection, _, source) = host.root_output(parent).unwrap();
    renderer
        .prepare(&mut host, Some((selection, source)))
        .unwrap();
    let faulted = renderer
        .draw_stats(&host, parent, VIEWPORT, VIEWPORT)
        .unwrap();
    assert_eq!(
        faulted.surface_cache_direct,
        u32::from(sibling.is_some()),
        "{faulted:?}"
    );
    assert_eq!(
        faulted.surface_cache_repaints + faulted.surface_cache_reuses,
        1
    );
    assert!(matches!(
        record(&renderer, panel.parent, panel.anchor).presentation,
        SurfaceCachePresentation::Reused | SurfaceCachePresentation::Repainted
    ));
    if let Some(sibling) = sibling {
        assert_eq!(host.world_fault(sibling.output.world()), Ok(None));
        assert_eq!(
            record(&renderer, sibling.parent, sibling.anchor).presentation,
            SurfaceCachePresentation::Interaction
        );
    }
}

#[test]
fn interaction_presents_directly_and_returns_only_to_current_images() {
    use ipp_core::components::{GuiCheckbox, GuiLayout};
    use ipp_core::services::gui_input::{
        GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputService, GuiPointerLease,
    };
    use ipp_core::systems::gui::GuiPrimitivePart;
    use ipp_core::systems::gui::local::{
        GuiInteractionUpdate, GuiLocalAction, GuiLocalCommand, GuiLocalEffect,
    };
    use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiSkin};
    use ipp_core::systems::gui::{GuiPartId, GuiSystem};
    use std::{cell::RefCell, rc::Rc};

    struct Permit(Rc<RefCell<Vec<GuiDeliveryTerminal>>>);

    impl GuiDeliveryPermit for Permit {
        fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
            Ok(())
        }

        fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
            self.0.borrow_mut().push(terminal);
        }
    }

    // Override rows win in every state: a plain box without the default
    // look's border or glow, so hover leaves the paint unchanged.
    let appearance = |color| {
        let mut parts = ipp_core::components::rows::Rows::new();
        parts
            .push(GuiPaintPart {
                color: Some(color),
                border_width: Some(0.0),
                border_color: Some([0.0; 4]),
                glow_intensity: Some(0.0),
                ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background)).unwrap()
            })
            .unwrap();
        ComponentValue::GuiSkin(GuiSkin {
            parts,
            ..Default::default()
        })
    };

    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, _) = scene(&mut host);
    let panel = CanvasSurface::new(
        &mut host,
        world_id,
        -1.0,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 1.0,
                height: 1.0,
                ..Default::default()
            }),
            appearance([0.2, 0.3, 0.4, 1.0]),
        ],
    );
    cache_policy(&mut host, panel, Some(ALWAYS));
    let service = GuiInputService::default();
    let session = service.open_session().unwrap();
    let terminals = Rc::new(RefCell::new(Vec::new()));
    let action = |host: &mut ipp_core::HostRuntime, id, action| {
        checkbox_action(host, panel, id, action);
    };

    let cold = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(
        frame(&mut renderer, &mut host, world_id, 0.1).surface_cache_reuses,
        1
    );
    take_events(&state);

    action(&mut host, 1, GuiLocalAction::Focus(0));
    let focused = frame(&mut renderer, &mut host, world_id, 0.01);
    assert_eq!(work(&focused), [0, 0, 1, 0, 0], "{focused:?}");
    assert!(!take_events(&state).contains('C'));
    assert!(control_focused(&mut host, panel));
    let diagnostic = record(&renderer, world_id, panel.anchor);
    assert_eq!(
        diagnostic.presentation,
        SurfaceCachePresentation::Interaction
    );
    assert_eq!(diagnostic.presentation.code(), 1);
    assert_eq!(focused.surface_cache_entries, 1);

    host.world_mut(panel.output.world().id())
        .unwrap()
        .enqueue(Batch {
            id: 100,
            operations: vec![Command::insert_value(
                EntityRef::Handle(panel.content),
                appearance([0.9, 0.1, 0.1, 1.0]),
            )],
        })
        .unwrap();
    frame(&mut renderer, &mut host, world_id, 0.01);
    action(&mut host, 2, GuiLocalAction::Blur);
    let released = frame(&mut renderer, &mut host, world_id, 0.01);
    assert_eq!(work(&released), [1, 0, 0, 0, 0], "{released:?}");
    assert!(!control_focused(&mut host, panel));
    assert_eq!(
        frame(&mut renderer, &mut host, world_id, 0.01).surface_cache_reuses,
        1
    );

    let context = service
        .bind_context(&host, &session, host.world_ref(world_id).unwrap())
        .unwrap()
        .context;
    let publication = host
        .publication(host.latest_publication(world_id).unwrap())
        .unwrap();
    let path = [publication
        .attachments
        .iter()
        .find(|edge| edge.anchor == panel.anchor)
        .unwrap()
        .token
        .clone()];
    let feedback = |host: &mut ipp_core::HostRuntime,
                    request_id,
                    lease: &mut Option<GuiPointerLease>,
                    update| {
        let target = checkbox_target(host, panel);
        let ticket = service
            .reserve_routed(
                host,
                &context,
                target,
                request_id,
                &path,
                Box::new(Permit(terminals.clone())),
            )
            .unwrap();
        let pointer = lease
            .get_or_insert_with(|| service.pointer_lease(&ticket, 1).unwrap())
            .clone();
        let command = GuiLocalCommand::interaction(ticket, pointer, update).unwrap();
        host.world_mut(panel.output.world().id())
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, GUI_SESSION, command)
            .unwrap();
    };
    let paint = canvas_revisions(&host, panel).0;
    let mut lease = None;
    feedback(&mut host, 3, &mut lease, GuiInteractionUpdate::Hover(true));
    let hovered = frame(&mut renderer, &mut host, world_id, 0.01);
    assert!(control_hovered(&mut host, panel));
    assert_eq!(work(&hovered), [0, 0, 1, 0, 0], "{hovered:?}");
    feedback(&mut host, 4, &mut lease, GuiInteractionUpdate::Hover(false));
    let returned = frame(&mut renderer, &mut host, world_id, 0.01);
    assert!(!control_hovered(&mut host, panel));
    assert_eq!(canvas_revisions(&host, panel).0, paint);
    assert_eq!(work(&returned), [0, 1, 0, 0, 0], "{returned:?}");
    feedback(&mut host, 5, &mut lease, GuiInteractionUpdate::Cancel);
    frame(&mut renderer, &mut host, world_id, 0.01);
    assert_eq!(service.pending_count(), 0);
    assert_eq!(terminals.borrow().len(), 3);
    assert!(
        terminals
            .borrow()
            .iter()
            .all(|terminal| matches!(terminal, GuiDeliveryTerminal::Applied(_)))
    );
    assert!(!lease.as_ref().unwrap().is_live());
    drop(lease.take());
    service.release_context(&context);
    service.close_session(&session);
}

/// Retained box batches of a directly presented panel whose paint revision is
/// published rebuild exactly when its paint changes, although unchanged frames
/// reuse their hashes instead of hashing every box.
#[test]
fn published_paint_revisions_rebuild_retained_boxes_exactly_when_paint_changes() {
    use ipp_core::components::{GuiCheckbox, GuiLayout};
    use ipp_core::systems::gui::GuiPartId;
    use ipp_core::systems::gui::GuiPrimitivePart;
    use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiSkin};

    let appearance = |color| {
        let mut parts = ipp_core::components::rows::Rows::new();
        parts
            .push(GuiPaintPart {
                color: Some(color),
                ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background)).unwrap()
            })
            .unwrap();
        ComponentValue::GuiSkin(GuiSkin {
            parts,
            ..Default::default()
        })
    };
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, _state, world_id, _) = scene(&mut host);
    let panel = CanvasSurface::new(
        &mut host,
        world_id,
        -1.0,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 1.0,
                height: 1.0,
                ..Default::default()
            }),
            appearance([0.2, 0.3, 0.4, 1.0]),
        ],
    );
    cache_policy(
        &mut host,
        panel,
        Some(SurfaceCache {
            direct_distance: 1000.0,
            ..ALWAYS
        }),
    );

    let cold = frame(&mut renderer, &mut host, world_id, 0.01);
    assert!(cold.gui_rebuilds > 0, "{cold:?}");
    assert_eq!(work(&cold), [0, 0, 1, 0, 0]);
    let paint = canvas_revisions(&host, panel).0;
    assert_ne!(paint, 0);
    for _ in 0..3 {
        let warm = frame(&mut renderer, &mut host, world_id, 0.01);
        assert_eq!((warm.gui_rebuilds, warm.uploaded_bytes), (0, 0), "{warm:?}");
        assert!(warm.gui_batches > 0);
    }
    assert_eq!(canvas_revisions(&host, panel).0, paint);

    canvas::apply(
        &mut host,
        panel.output.world().id(),
        vec![Command::insert_value(
            EntityRef::Handle(panel.content),
            appearance([0.9, 0.1, 0.1, 1.0]),
        )],
    );
    let edited = frame(&mut renderer, &mut host, world_id, 0.01);
    assert!(canvas_revisions(&host, panel).0 > paint);
    assert!(edited.gui_rebuilds > 0, "{edited:?}");
    assert!(edited.uploaded_bytes > 0);

    let settled = frame(&mut renderer, &mut host, world_id, 0.01);
    assert_eq!((settled.gui_rebuilds, settled.uploaded_bytes), (0, 0));
}

/// A cached Surface whose paint changes every frame at its refresh cap draws
/// directly after a streak of repaints, and returns to a current image once its
/// paint holds.
#[test]
fn surfaces_repainting_every_frame_present_directly_until_their_paint_settles() {
    use ipp_render_gl::{SURFACE_CACHE_ANIMATED_FRAMES, SURFACE_CACHE_SETTLE_FRAMES};

    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    // A cap above the 60 Hz frame rate: every changed frame is due.
    cache_policy(
        &mut host,
        entity,
        Some(SurfaceCache {
            max_refresh_hz: 120.0,
            ..ALWAYS
        }),
    );
    let dt = 1.0 / 60.0;
    frame(&mut renderer, &mut host, world_id, dt);
    take_events(&state);

    let mut step = 0;
    for _ in 0..SURFACE_CACHE_ANIMATED_FRAMES {
        step += 1;
        recolor(&mut host, entity, step);
        let repainted = frame(&mut renderer, &mut host, world_id, dt);
        assert_eq!(work(&repainted), [1, 0, 0, 0, 0], "{repainted:?}");
    }
    take_events(&state);

    // The next due repaint draws directly instead: no cache target, no composite.
    for _ in 0..3 {
        step += 1;
        recolor(&mut host, entity, step);
        let animated = frame(&mut renderer, &mut host, world_id, dt);
        assert_eq!(work(&animated), [0, 0, 1, 0, 0], "{animated:?}");
        assert_eq!(animated.surface_cache_animated, 1);
    }
    let events = take_events(&state);
    assert!(!events.contains('B') && !events.contains('C'), "{events}");
    let diagnostic = record(&renderer, world_id, entity.anchor);
    assert_eq!(diagnostic.presentation, SurfaceCachePresentation::Animated);
    assert_eq!(diagnostic.presentation.code(), 7);
    // The image stays resident for the return.
    assert_eq!(
        diagnostic.resident_bytes,
        4 * diagnostic.size[0] * diagnostic.size[1]
    );

    // Once the paint holds, the stale image repaints and is reused.
    for _ in 1..SURFACE_CACHE_SETTLE_FRAMES {
        assert_eq!(
            frame(&mut renderer, &mut host, world_id, dt).surface_cache_animated,
            1
        );
    }
    let returned = frame(&mut renderer, &mut host, world_id, dt);
    assert_eq!(work(&returned), [1, 0, 0, 0, 0], "{returned:?}");
    assert_eq!(returned.surface_cache_animated, 0);
    let warm = frame(&mut renderer, &mut host, world_id, dt);
    assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
}

#[test]
fn culled_surfaces_skip_repaints_and_refresh_stale_content_on_return() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    cache_policy(&mut host, entity, Some(ALWAYS));

    frame(&mut renderer, &mut host, world_id, 0.1);
    let begins = state.cache_begins.get();
    let resident = state.cache_targets_live.get();
    move_surface(&mut host, entity, 100.0);
    let unchanged_culled = frame(&mut renderer, &mut host, world_id, 0.5);
    assert_eq!(work(&unchanged_culled), [0; 5]);
    assert_eq!(
        (
            unchanged_culled.uploaded_bytes,
            unchanged_culled.glyph_page_retirements
        ),
        (0, 0)
    );
    assert_eq!(state.cache_targets_live.get(), resident);
    move_surface(&mut host, entity, 0.0);
    let unchanged_return = frame(&mut renderer, &mut host, world_id, 0.0);
    assert_eq!(work(&unchanged_return), [0, 1, 0, 0, 0]);
    assert_eq!(
        (
            unchanged_return.uploaded_bytes,
            unchanged_return.gui_rebuilds,
            unchanged_return.glyph_populates
        ),
        (0, 0, 0)
    );

    // Behind the camera, edits and elapsed time cause no cache work.
    move_surface(&mut host, entity, 10.0);
    for step in 2..5 {
        recolor(&mut host, entity, step);
        let culled = frame(&mut renderer, &mut host, world_id, 0.5);
        assert_eq!(work(&culled), [0; 5], "{culled:?}");
        assert_eq!(culled.draw_calls, 0);
    }
    assert_eq!(state.cache_begins.get(), begins);
    assert_eq!(
        presentation(&renderer, world_id),
        SurfaceCachePresentation::Culled
    );
    assert_eq!(presentation(&renderer, world_id).code(), 4);

    move_surface(&mut host, entity, 0.0);
    let visible = frame(&mut renderer, &mut host, world_id, 0.0);
    assert_eq!(work(&visible), [1, 0, 0, 0, 0], "{visible:?}");
}

#[test]
fn removal_policy_removal_and_forgetting_the_world_release_images() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    cache_policy(&mut host, entity, Some(ALWAYS));

    frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(state.cache_targets_live.get(), 1);

    // Removing the policy releases the image after the frame.
    cache_policy(&mut host, entity, None);
    let direct = frame(&mut renderer, &mut host, world_id, 0.1);
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
    cache_policy(&mut host, entity, Some(ALWAYS));
    frame(&mut renderer, &mut host, world_id, 0.1);
    canvas::apply(
        &mut host,
        world_id,
        vec![Command::Delete {
            entity: EntityRef::Handle(entity.anchor),
        }],
    );
    let removed = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(removed.surface_cache_entries, 0);
    assert_eq!(state.cache_targets_live.get(), 0);

    let recreated = add_text_surface(&mut host, world_id, 0.0);
    assert_ne!(recreated, entity);
    cache_policy(&mut host, recreated, Some(ALWAYS));
    let fresh = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&fresh), [1, 0, 0, 0, 1], "{fresh:?}");
    assert_eq!(diagnostics(&renderer, world_id)[0].repaints, 1);

    renderer.forget_world(world_id);
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(diagnostics(&renderer, world_id).is_empty());
}

#[test]
fn allocation_and_repaint_failures_fall_back_directly_and_recover() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    cache_policy(&mut host, entity, Some(ALWAYS));

    *state.fail_cache_create.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let failed = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&failed), [0, 0, 1, 1, 0], "{failed:?}");
    assert_eq!(take_events(&state), format!("F{TEXT}"));
    assert_eq!(
        presentation(&renderer, world_id),
        SurfaceCachePresentation::Fallback
    );
    assert_eq!(presentation(&renderer, world_id).code(), 2);

    // Direct presentation continues through the back-off without retrying.
    *state.fail_cache_create.borrow_mut() = None;
    frame(&mut renderer, &mut host, world_id, 0.5);
    assert_eq!(state.cache_creates.get(), 1);
    let recovered = frame(&mut renderer, &mut host, world_id, 0.5);
    assert_eq!(work(&recovered), [1, 0, 0, 0, 1], "{recovered:?}");

    // A failed begin releases the image and presents directly.
    *state.fail_cache_begin.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    recolor(&mut host, entity, 1);
    let begin = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&begin), [0, 0, 1, 1, 0], "{begin:?}");
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(!state.cache_target_bound.get());
    *state.fail_cache_begin.borrow_mut() = None;

    // A failed end marks the image unusable the same way.
    *state.fail_cache_end.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let end = frame(&mut renderer, &mut host, world_id, 1.0);
    assert_eq!(work(&end), [0, 0, 1, 1, 1], "{end:?}");
    assert_eq!(state.cache_targets_live.get(), 0);
    *state.fail_cache_end.borrow_mut() = None;

    // A failed composite draws the Surface directly in its slot.
    frame(&mut renderer, &mut host, world_id, 1.0);
    take_events(&state);
    *state.fail_cache_composite.borrow_mut() = Some(RenderError::RenderDevice("injected".into()));
    let composite = frame(&mut renderer, &mut host, world_id, 0.0);
    assert_eq!(work(&composite), [0, 0, 1, 1, 0], "{composite:?}");
    assert_eq!(take_events(&state), format!("FC{TEXT}"));
    *state.fail_cache_composite.borrow_mut() = None;
    let later = frame(&mut renderer, &mut host, world_id, 1.0);
    assert_eq!(work(&later), [1, 0, 0, 0, 1], "{later:?}");
}

#[test]
fn a_repaint_without_gui_storage_presents_directly_and_recovers() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    cache_policy(&mut host, entity, Some(ALWAYS));

    // The repaint cannot write the Surface's retained storage: the image would lack
    // that work, so it is released and the Surface draws its text analytically.
    state
        .fail_gui_batch_write
        .replace(Some(RenderError::RenderDevice("out of memory".into())));
    let failed = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&failed), [0, 0, 1, 1, 1], "{failed:?}");
    assert_eq!(take_events(&state), "BGEFG");
    // One for the abandoned repaint and one for the direct draw.
    assert_eq!(failed.failed_draw_calls, 2);
    assert_eq!(state.cache_targets_live.get(), 0);
    assert_eq!(
        presentation(&renderer, world_id),
        SurfaceCachePresentation::Fallback
    );

    // Once the device recovers, the storage returns and the cache retry repaints the
    // image from atlas text.
    state.fail_gui_batch_write.replace(None);
    let (recovered, events) = (0..16)
        .map(|_| {
            let stats = frame(&mut renderer, &mut host, world_id, 0.2);
            (stats, take_events(&state))
        })
        .find(|(stats, _)| stats.surface_cache_repaints == 1)
        .expect("the image is repainted after the retry interval");
    assert_eq!(events, "BTEFC", "{recovered:?}");
    assert_eq!(recovered.failed_draw_calls, 0);
}

#[test]
fn context_loss_fails_the_frame_and_recovery_repaints() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = scene(&mut host);
    {
        cache_policy(&mut host, entity, Some(ALWAYS));
        frame(&mut renderer, &mut host, world_id, 0.1);
        *state.fail_cache_begin.borrow_mut() = Some(RenderError::ContextLost);
        recolor(&mut host, entity, 1);
        let lost = try_frame(&mut renderer, &mut host, world_id, 0.1);
        assert_eq!(lost, Err(RenderError::ContextLost));
        assert!(!state.cache_target_bound.get());
        assert!(!state.atlas_target_bound.get());
    }

    *state.fail_cache_begin.borrow_mut() = None;
    recover_context(&mut renderer, &mut host, world_id, &surface_font());
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(diagnostics(&renderer, world_id).is_empty());

    let recovered = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&recovered), [1, 0, 0, 0, 1], "{recovered:?}");
    assert_eq!(state.cache_targets_live.get(), 1);

    // Context loss while allocating fails the frame without leaking state.
    recover_context(&mut renderer, &mut host, world_id, &surface_font());
    *state.fail_cache_create.borrow_mut() = Some(RenderError::ContextLost);
    let lost = try_frame(&mut renderer, &mut host, world_id, 0.1);
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
    CanvasSurface,
) {
    host.io_mut().register_stream("fixture://").unwrap();
    let (world, renderer, state) = setup(host);
    state.cache_limit.set(4096);
    let world_id = world.id();
    drop(world);
    let entity = CanvasSurface::new(
        host,
        world_id,
        0.0,
        vec![ComponentValue::CanvasDrawing(
            ipp_core::components::CanvasDrawing {
                source: "fixture:///drawing.ippd".into(),
                ..Default::default()
            },
        )],
    );
    cache_policy(host, entity, Some(ALWAYS));
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

fn drawing_primitives(host: &ipp_core::HostRuntime, entity: CanvasSurface) -> usize {
    host.latest_publication(entity.output.world().id())
        .and_then(|publication| host.output(publication, entity.output))
        .and_then(|output| output.data::<ipp_core::systems::canvas::CanvasPublication>())
        .map_or(0, |publication| publication.entries.len())
}

#[test]
fn an_image_missing_gpu_data_after_device_replacement_repaints_once_it_is_resident() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = drawing_scene(&mut host);
    for _ in 0..16 {
        deliver_drawing(&mut host);
        host.frame(0.0).unwrap();
        if drawing_primitives(&host, entity) == 1 {
            break;
        }
    }
    assert_eq!(drawing_primitives(&host, entity), 1);

    let cold = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(
        state.surface_path_draws.get(),
        1,
        "the repaint drew the drawing"
    );
    let before = canvas_revisions(&host, entity);

    // Replacing the device releases every image and the drawing's GPU data;
    // its CPU data and identity remain, so the prepared item is unchanged.
    renderer
        .replace_device(&mut host, TestDevice(state.clone()))
        .unwrap();
    assert_eq!(state.cache_targets_live.get(), 0);

    // The first frames repaint without the drawing, which is not resident
    // until the Host delivers its bytes again. The image matches direct
    // presentation, so it is reused past many refresh intervals.
    let draws = state.surface_path_draws.get();
    let recovered = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&recovered), [1, 0, 0, 0, 1], "{recovered:?}");
    assert_eq!(
        state.surface_path_draws.get(),
        draws,
        "drawing not resident yet"
    );
    for _ in 0..3 {
        let waiting = frame(&mut renderer, &mut host, world_id, 0.5);
        assert_eq!(
            work(&waiting),
            [0, 1, 0, 0, 0],
            "{waiting:?}; before={before:?}, now={:?}, primitives={}",
            canvas_revisions(&host, entity),
            drawing_primitives(&host, entity)
        );
    }
    assert_eq!(canvas_revisions(&host, entity), before);

    // Once the drawing is resident again, the next frame repaints even with a
    // frozen clock, and the complete image is then reused.
    assert!(
        deliver_drawing(&mut host) > 0,
        "the Host reloads the drawing"
    );
    let arrived = frame(&mut renderer, &mut host, world_id, 0.0);
    assert_eq!(work(&arrived), [1, 0, 0, 0, 0], "{arrived:?}");
    assert_eq!(
        state.surface_path_draws.get(),
        draws + 1,
        "the repaint drew it"
    );
    assert_eq!(canvas_revisions(&host, entity), before);
    let warm = frame(&mut renderer, &mut host, world_id, 0.5);
    assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
    assert_eq!(state.surface_path_draws.get(), draws + 1);
}

#[test]
fn a_drawing_still_loading_at_the_first_repaint_repaints_on_arrival() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) = drawing_scene(&mut host);
    assert_eq!(drawing_primitives(&host, entity), 0);

    // Never resident: the first image is painted without the drawing.
    let cold = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(state.surface_path_draws.get(), 0);
    let waiting = frame(&mut renderer, &mut host, world_id, 0.5);
    assert_eq!(work(&waiting), [0, 1, 0, 0, 0], "{waiting:?}");

    // Arrival repaints on the frame that sees it, with a frozen clock.
    let mut arrived = None;
    for _ in 0..8 {
        deliver_drawing(&mut host);
        let stats = frame(&mut renderer, &mut host, world_id, 0.0);
        if state.surface_path_draws.get() > 0 {
            arrived = Some(stats);
            break;
        }
        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "{stats:?}");
    }
    let arrived = arrived.expect("the drawing arrived");
    assert_eq!(drawing_primitives(&host, entity), 1);
    assert_eq!(work(&arrived), [1, 0, 0, 0, 0], "{arrived:?}");
    assert_eq!(state.surface_path_draws.get(), 1);
    let warm = frame(&mut renderer, &mut host, world_id, 0.5);
    assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
}

#[test]
fn budget_pressure_evicts_idle_images_then_falls_back() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, first) = scene(&mut host);
    let second = add_text_surface(&mut host, world_id, -1.0);
    cache_policy(&mut host, first, Some(ALWAYS));
    cache_policy(&mut host, second, Some(ALWAYS));
    renderer.set_surface_cache_budget(4 * 64 * 64);

    // Only one image fits: entity order admits the first, the second falls back.
    let both = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&both), [1, 0, 1, 1, 1], "{both:?}");
    assert_eq!(both.surface_cache_resident_bytes, 4 * 64 * 64);

    // With the first culled, its idle image makes room for the second.
    move_surface(&mut host, first, 10.0);
    let swapped = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&swapped), [1, 0, 0, 0, 1], "{swapped:?}");
    assert_eq!(state.cache_targets_live.get(), 1);
    let records = diagnostics(&renderer, world_id);
    let size = |entity| records.iter().find(|r| r.entity == entity).unwrap().size;
    assert_eq!(
        (size(first.anchor), size(second.anchor)),
        ([0, 0], [64, 64])
    );
}

#[test]
fn worlds_share_the_context_budget() {
    let mut host = ipp_core::HostRuntime::new();
    host.io_mut().register_stream("fixture://").unwrap();
    let (world, mut renderer, state) = setup(&mut host);
    let world_id = world.id();
    drop(world);
    state.cache_limit.set(4096);
    let first_owner = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SURFACE]))
        .unwrap();
    let second_owner = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SURFACE]))
        .unwrap();
    for owner in [first_owner, second_owner] {
        let child = host.world_ref(owner).unwrap();
        create(
            &mut host.world_mut(world_id).unwrap(),
            vec![ComponentValue::WorldAttachment(
                ipp_core::WorldAttachment::spatial(child),
            )],
        );
    }
    let first = add_text_surface(&mut host, first_owner, 0.0);
    let second = add_text_surface(&mut host, second_owner, 10.0);
    assert_ne!(first.parent, second.parent);
    assert_ne!(first.output.world(), second.output.world());
    cache_policy(&mut host, first, Some(ALWAYS));
    cache_policy(&mut host, second, Some(ALWAYS));
    renderer.set_surface_cache_budget(4 * 64 * 64);

    frame(&mut renderer, &mut host, world_id, 0.1);
    move_surface(&mut host, first, 10.0);
    move_surface(&mut host, second, 0.0);
    let stats = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&stats), [1, 0, 0, 0, 1], "{stats:?}");
    assert_eq!(
        (
            stats.surface_cache_entries,
            stats.surface_cache_resident_bytes
        ),
        (1, 4 * 64 * 64)
    );
    assert_eq!(record(&renderer, first_owner, first.anchor).size, [0, 0]);
    assert_eq!(
        record(&renderer, second_owner, second.anchor).size,
        [64, 64]
    );

    renderer.set_surface_cache_budget(ipp_render_gl::SURFACE_CACHE_BUDGET_BYTES);
    move_surface(&mut host, first, -1.0);
    frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(state.cache_targets_live.get(), 2);
    assert!(host.destroy_world(first_owner));
    renderer.forget_world(first_owner);
    assert_eq!(state.cache_targets_live.get(), 1);
    assert!(host.world_ref(first.output.world().id()).is_some());
    let survivor = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&survivor), [0, 1, 0, 0, 0]);
    assert!(diagnostics(&renderer, first_owner).is_empty());
    assert_eq!(
        record(&renderer, second_owner, second.anchor).size,
        [64, 64]
    );
    assert!(host.destroy_world(second_owner));
    renderer.forget_world(second_owner);
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(host.world_ref(second.output.world().id()).is_some());
}

#[test]
fn mixed_cached_and_direct_surfaces_keep_painter_order() {
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, near) = scene(&mut host);
    let far = add_text_surface(&mut host, world_id, -3.0);
    cache_policy(&mut host, far, Some(ALWAYS));

    take_events(&state);
    frame(&mut renderer, &mut host, world_id, 0.1);
    // The far image repaints before the frame; the main pass composites it
    // behind the nearer direct Surface.
    assert_eq!(take_events(&state), format!("B{TEXT}EFC{TEXT}"));
    let warm = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(take_events(&state), format!("FC{TEXT}"));
    assert_eq!(work(&warm), [0, 1, 0, 0, 0]);

    // Swapping depths swaps the slots.
    move_surface(&mut host, near, -6.0);
    move_surface(&mut host, far, 0.0);
    frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(take_events(&state), format!("F{TEXT}C"));
}

#[test]
fn text_drawn_analytically_under_the_population_bound_refines_at_the_refresh_cap() {
    let budget = ipp_render_gl::GLYPH_MIN_POPULATES_PER_FRAME as u32;
    let ids: Vec<u32> = (0..budget + 8).collect();
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, entity) =
        text_run_scene(&mut host, glyph_font(budget + 8, 1000, 1.0), &ids);
    // Only the per-frame floor populates, so the first repaint defers glyphs.
    renderer.set_glyph_population_budget_ms(0.0);
    state.cache_limit.set(4096);
    cache_policy(&mut host, entity, Some(ALWAYS));

    // The first repaint populates up to the bound and draws the run analytically.
    let cold = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&cold), [1, 0, 0, 0, 1], "{cold:?}");
    assert_eq!(cold.glyph_populates, budget);
    assert_eq!(state.analytic_glyph_draws.get(), 1);

    // The reused image keeps queueing its missing entries, which the next frame
    // populates without repainting.
    let populated = frame(&mut renderer, &mut host, world_id, 0.0);
    assert_eq!(work(&populated), [0, 1, 0, 0, 0], "{populated:?}");
    assert_eq!(populated.glyph_populates, 8);

    // With a frozen clock the refresh interval has not elapsed, so the image stays.
    let frozen = frame(&mut renderer, &mut host, world_id, 0.0);
    assert_eq!(work(&frozen), [0, 1, 0, 0, 0], "{frozen:?}");

    // At the refresh interval the run samples the atlas as direct presentation would.
    let refined = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&refined), [1, 0, 0, 0, 0], "{refined:?}");
    assert_eq!(state.analytic_glyph_draws.get(), 1);
    assert_eq!(state.glyph_batch_draws.get(), 1);

    let warm = frame(&mut renderer, &mut host, world_id, 0.1);
    assert_eq!(work(&warm), [0, 1, 0, 0, 0], "{warm:?}");
    assert_eq!((warm.glyph_misses, warm.glyph_populates), (0, 0));
}

#[test]
fn a_saturated_population_queue_refines_cached_text_only_at_the_refresh_cap() {
    const ROWS: u32 = 4;
    const PER_ROW: u32 = 40;
    const DT: f64 = 1.0 / 60.0;
    let busy_glyphs = ROWS * PER_ROW;
    let budget = ipp_render_gl::GLYPH_MIN_POPULATES_PER_FRAME as u32;
    let mut host = ipp_core::HostRuntime::new();
    let (mut renderer, state, world_id, busy) =
        text_run_scene(&mut host, glyph_font(busy_glyphs + 8, 1000, 1.0), &[0]);
    renderer.set_glyph_population_budget_ms(0.0);
    state.cache_limit.set(4096);

    // A direct Surface inside the first Camera target fills the frame's population
    // queue before the later cached Canvas target can populate its own glyphs.
    canvas::apply(
        &mut host,
        busy.output.world().id(),
        vec![Command::Delete {
            entity: EntityRef::Handle(busy.content),
        }],
    );
    for row in 0..ROWS {
        canvas::add_content(
            &mut host,
            busy.output,
            canvas::glyph_run(
                &(row * PER_ROW..(row + 1) * PER_ROW).collect::<Vec<_>>(),
                [0.1, 0.1 + 0.2 * row as f32],
            ),
        );
    }
    canvas::apply(
        &mut host,
        world_id,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(busy.anchor),
            component: ComponentValue::WORLD_ATTACHMENT,
        }],
    );
    host.frame(0.0).unwrap();
    let camera_world = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, CAMERA, RENDER, SURFACE]),
        )
        .unwrap();
    let camera = create(
        &mut host.world_mut(camera_world).unwrap(),
        vec![
            ComponentValue::Camera(Default::default()),
            ComponentValue::Transform(ipp_core::components::Transform {
                z: 5.0,
                ..Default::default()
            }),
        ],
    );
    let camera_output = host
        .bind_output(
            host.world_ref(camera_world).unwrap(),
            camera,
            ipp_core::OutputKind::Camera,
        )
        .unwrap();
    create(
        &mut host.world_mut(camera_world).unwrap(),
        vec![
            ComponentValue::Surface(Default::default()),
            ComponentValue::WorldAttachment(ipp_core::WorldAttachment::surface(busy.output)),
        ],
    );
    canvas::apply(
        &mut host,
        world_id,
        vec![Command::insert_value(
            EntityRef::Handle(busy.anchor),
            ComponentValue::WorldAttachment(ipp_core::WorldAttachment::surface(camera_output)),
        )],
    );
    let cached = CanvasSurface::new(
        &mut host,
        world_id,
        1.0,
        canvas::glyph_run(
            &(busy_glyphs..busy_glyphs + 8).collect::<Vec<_>>(),
            [0.1, 0.1],
        ),
    );
    cache_policy(&mut host, cached, Some(ALWAYS));
    assert!(
        busy.anchor < cached.anchor,
        "the direct Camera output prepares its demand first"
    );

    // The first repaint draws the cached text analytically behind a full queue.
    let cold = frame(&mut renderer, &mut host, world_id, DT);
    assert_eq!(work(&cold)[0], 1, "{cold:?}");
    assert_eq!(cold.glyph_populates, budget);
    let events = take_events(&state);
    assert_eq!(events, "GGGGEBGEFCC", "{cold:?}");

    // While the queue stays saturated the image is reused, whatever other Surfaces
    // populate, until its own glyphs are populated and its refresh interval ends.
    let mut frames = 1;
    let refined = loop {
        let stats = frame(&mut renderer, &mut host, world_id, DT);
        frames += 1;
        assert!(stats.glyph_populates <= budget, "{stats:?}");
        let events = take_events(&state);
        if stats.surface_cache_repaints == 1 {
            break events;
        }

        assert_eq!(work(&stats), [0, 1, 0, 0, 0], "frame {frames}: {stats:?}");
        assert!(frames < 30, "cached text never refined");
    };
    assert_eq!(
        frames,
        busy_glyphs.div_ceil(budget) + 2,
        "one frame after the queue reaches its glyphs, at the 10 Hz cap"
    );
    assert_eq!(refined, "TEBTEFCC");
}
