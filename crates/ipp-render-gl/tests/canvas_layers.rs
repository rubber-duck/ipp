//! Layered Canvas presentation through real Worlds and the recording render
//! device: projected cutouts share one image, retain GUI geometry, preserve
//! physical layer depths and front/back ordering, and expand culling bounds.
//! The maintained GLES and WebGL scenarios own the image evidence.

mod support;

use ipp_core::components::{
    CanvasLayerTransition, CanvasStyle, FlatSurface, GuiBehavior, GuiLayout, GuiOverlay, Transform,
};
use ipp_core::systems::canvas::CanvasBox;
use ipp_core::{
    Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, FieldValue, FieldWrite,
    HostRuntime, SurfaceCache, WorldId,
};
use ipp_render_gl::{RenderService, SurfaceCachePresentation};
use std::rc::Rc;
use support::canvas::{self, CanvasSurface};
use support::*;

/// A half-unit box at `position` of the default one-unit canvas, on plane `layer`.
fn plain_box(position: [f32; 2], layer: u32) -> Vec<ComponentValue> {
    vec![
        ComponentValue::CanvasStyle(CanvasStyle {
            x: position[0],
            y: position[1],
            layer,
            ..Default::default()
        }),
        ComponentValue::CanvasBox(CanvasBox {
            width: 0.5,
            height: 0.5,
            ..Default::default()
        }),
    ]
}

/// The camera at z 5 looks at a 1 x 1 m Surface at the origin holding a base
/// box and a box on plane `layer` above it.
fn scene_on(
    layer: u32,
) -> (
    HostRuntime,
    RenderService<TestDevice>,
    Rc<DeviceState>,
    WorldId,
    CanvasSurface,
) {
    let mut host = support::task_scheduler::host();
    let (world, renderer, state) = setup(&mut host);
    state.cache_limit.set(4096);
    let world_id = world.id();
    drop(world);
    let surface = CanvasSurface::new(&mut host, world_id, 0.0, plain_box([0.0, 0.0], 0));
    canvas::add_content(&mut host, surface.output, plain_box([0.25, 0.25], layer));
    (host, renderer, state, world_id, surface)
}

fn scene() -> (
    HostRuntime,
    RenderService<TestDevice>,
    Rc<DeviceState>,
    WorldId,
    CanvasSurface,
) {
    scene_on(1)
}

fn edit(host: &mut HostRuntime, surface: CanvasSurface, values: Vec<ComponentValue>) {
    canvas::apply(
        host,
        surface.parent,
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Handle(surface.anchor), value))
            .collect(),
    );
}

fn spacing(host: &mut HostRuntime, surface: CanvasSurface, layer_spacing: f32) {
    edit(
        host,
        surface,
        vec![ComponentValue::FlatSurface(FlatSurface {
            layer_spacing,
            ..Default::default()
        })],
    );
}

/// A submitted plane's center and normal in homogeneous clip coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    center: [f32; 4],
    normal: [f32; 4],
}

fn placement(mvp: [f32; 16], center: [f32; 3]) -> Placement {
    Placement {
        center: std::array::from_fn(|row| {
            mvp[12 + row]
                + (0..3)
                    .map(|axis| center[axis] * mvp[axis * 4 + row])
                    .sum::<f32>()
        }),
        normal: std::array::from_fn(|row| mvp[8 + row]),
    }
}

/// Observe actual mesh vertices for image draws, or the unit Canvas center
/// for direct GUI draws. Ignore the rasterization pass when an image is shown.
fn draws(
    renderer: &mut RenderService<TestDevice>,
    state: &DeviceState,
    host: &mut HostRuntime,
    world: WorldId,
) -> Vec<Placement> {
    state.gui_draw_mvps.borrow_mut().clear();
    state.surface_mesh_draws.borrow_mut().clear();
    render_frame(renderer, host, world, 100, 100).unwrap();
    let meshes = state.surface_mesh_draws.borrow();
    if meshes.is_empty() {
        state
            .gui_draw_mvps
            .take()
            .into_iter()
            .map(|mvp| placement(mvp, [0.5, 0.5, 0.0]))
            .collect()
    } else {
        meshes
            .iter()
            .map(|draw| placement(draw.mvp, draw.center))
            .collect()
    }
}

fn offset(mut plane: Placement, offset: f32) -> Placement {
    for row in 0..4 {
        plane.center[row] += offset * plane.normal[row];
    }
    plane
}

fn near(left: Placement, right: Placement) -> bool {
    left.center
        .iter()
        .chain(&left.normal)
        .zip(right.center.iter().chain(&right.normal))
        .all(|(left, right)| (left - right).abs() <= 1e-4 * (1.0 + right.abs()))
}

fn planes(host: &HostRuntime, surface: CanvasSurface) -> Vec<(u32, f64)> {
    surface
        .publication(host)
        .layers
        .iter()
        .map(|plane| (plane.id, plane.offset))
        .collect()
}

#[test]
fn fractional_transition_moves_retained_draws_and_expands_culling_bounds() {
    let (mut host, mut renderer, state, world, surface) = scene();
    let mut values = plain_box([0.5, 0.5], 2);
    values.push(ComponentValue::CanvasLayerTransition(
        CanvasLayerTransition {
            previous_layer: 4,
            progress: 0.125,
        },
    ));
    let moving = canvas::add_content(&mut host, surface.output, values);
    let flat = draws(&mut renderer, &state, &mut host, world);
    spacing(&mut host, surface, 0.25);
    let writes = state.gui_batch_writes.get();
    let from = draws(&mut renderer, &state, &mut host, world);
    assert_eq!(planes(&host, surface), [(0, 0.0), (1, 1.0), (2, 2.875)]);
    assert!(near(from[2], offset(flat[0], 2.875 * 0.25)));
    let revision = surface.publication(&host).paint_revision;
    panel_apply(
        &mut host,
        surface,
        vec![Command::SetField {
            entity: EntityRef::Handle(moving),
            component: ComponentValue::CANVAS_LAYER_TRANSITION,
            field: FieldWrite {
                offset: std::mem::offset_of!(CanvasLayerTransition, progress) as u32,
                value: FieldValue::F32(0.25),
            },
        }],
    );
    let moved = draws(&mut renderer, &state, &mut host, world);
    assert_eq!(surface.publication(&host).paint_revision, revision);
    assert_eq!(state.gui_batch_writes.get(), writes);
    assert!(near(moved[2], offset(flat[0], 2.75 * 0.25)));

    // Coordinate2.75 crosses the far plane while opaque ID2 would remain culled.
    edit(
        &mut host,
        surface,
        vec![ComponentValue::Transform(Transform {
            z: -95.6,
            ..Default::default()
        })],
    );
    assert_eq!(draws(&mut renderer, &state, &mut host, world).len(), 3);
}

#[test]
fn layer_planes_move_without_rewriting_retained_gui_geometry() {
    let (mut host, mut renderer, state, world, surface) = scene();

    // Without spacing both layers draw on the Surface plane, base first.
    let flat = draws(&mut renderer, &state, &mut host, world);
    assert_eq!(planes(&host, surface), [(0, 0.0), (1, 1.0)]);
    assert_eq!(flat.len(), 2, "one retained range per layer");
    assert_eq!(flat[0], flat[1]);

    // Spacing moves the raised cutout along the normal; GUI records stay retained
    // and the camera in front still sees the base drawn first.
    let writes = state.gui_batch_writes.get();
    spacing(&mut host, surface, 0.5);
    let exploded = draws(&mut renderer, &state, &mut host, world);
    assert_eq!(state.gui_batch_writes.get(), writes);
    assert_eq!(exploded.len(), 2);
    assert!(near(exploded[0], flat[0]));
    assert!(near(exploded[1], offset(flat[0], 0.5)));
    assert!(!near(exploded[1], exploded[0]));

    // Seen from behind, the raised plane is farthest and draws first.
    edit(
        &mut host,
        surface,
        vec![ComponentValue::Transform(Transform {
            qy: 1.0,
            qw: 0.0,
            ..Default::default()
        })],
    );
    let behind = draws(&mut renderer, &state, &mut host, world);
    assert_eq!(behind.len(), 2);
    assert!(near(behind[0], offset(behind[1], 0.5)));
    assert_eq!(state.gui_batch_writes.get(), writes);
}

#[test]
fn bounds_reach_the_deepest_plane() {
    let (mut host, mut renderer, state, world, surface) = scene();
    // Just beyond the 100 m far plane, the Surface itself is culled.
    edit(
        &mut host,
        surface,
        vec![ComponentValue::Transform(Transform {
            z: -95.5,
            ..Default::default()
        })],
    );
    assert!(draws(&mut renderer, &state, &mut host, world).is_empty());

    // One metre toward the camera, the raised plane is inside the view.
    spacing(&mut host, surface, 1.0);
    let draws = draws(&mut renderer, &state, &mut host, world);
    assert_eq!(draws.len(), 2);
}

#[test]
fn large_authored_offsets_do_not_expand_the_compact_rank_bounds() {
    // A large authored offset occupies rank1: its physical depth is one
    // spacing, so unoccupied priorities cannot expand the shell bounds.
    let (mut host, mut renderer, state, world, surface) = scene_on(3);
    edit(
        &mut host,
        surface,
        vec![ComponentValue::Transform(Transform {
            z: -95.5,
            ..Default::default()
        })],
    );
    assert!(draws(&mut renderer, &state, &mut host, world).is_empty());
    spacing(&mut host, surface, 1.0 / 3.0);
    let draws = draws(&mut renderer, &state, &mut host, world);
    assert!(draws.is_empty());
}

/// An open overlay of `size` on plane `layer`, painting a box over its bounds.
fn overlay(size: f32, layer: u32) -> Vec<ComponentValue> {
    vec![
        ComponentValue::GuiOverlay(GuiOverlay::default()),
        ComponentValue::GuiLayout(GuiLayout {
            width: size,
            height: size,
            ..Default::default()
        }),
        ComponentValue::CanvasStyle(CanvasStyle {
            layer,
            ..Default::default()
        }),
        ComponentValue::CanvasBox(CanvasBox {
            width: size,
            height: size,
            ..Default::default()
        }),
    ]
}

fn panel_apply(host: &mut HostRuntime, surface: CanvasSurface, operations: Vec<Command>) {
    canvas::apply(host, surface.output.world().id(), operations);
}

fn set_open(host: &mut HostRuntime, surface: CanvasSurface, entity: EntityId, open: bool) {
    panel_apply(
        host,
        surface,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_BEHAVIOR,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiBehavior, visible) as u32,
                value: FieldValue::Bool(open),
            },
        }],
    );
}

#[test]
fn occupied_ranks_repack_when_an_overlay_opens_and_closes() {
    let mut host = support::task_scheduler::host();
    let (world, mut renderer, state) = setup(&mut host);
    state.cache_limit.set(4096);
    let world_id = world.id();
    drop(world);
    let surface = CanvasSurface::new(&mut host, world_id, 0.0, plain_box([0.0, 0.0], 0));
    // A later sibling overlay scope and an initially closed earlier scope
    // occupy compact ranks only while visible.
    canvas::add_content(&mut host, surface.output, overlay(0.3, 3));
    let tooltip = canvas::add_content(&mut host, surface.output, overlay(0.2, 1));
    panel_apply(
        &mut host,
        surface,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(tooltip),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(surface.content)),
                before: None,
            },
        }],
    );
    set_open(&mut host, surface, tooltip, false);
    spacing(&mut host, surface, 0.5);
    let shown = draws(&mut renderer, &state, &mut host, world_id);
    assert_eq!(planes(&host, surface), [(0, 0.0), (1, 1.0)]);
    assert_eq!(shown.len(), 2);
    let base = shown[0];
    // Only one overlay group is occupied above the base.
    let toast = offset(base, 0.5);
    assert!(near(shown[1], toast));

    // Opening the earlier scope inserts a rank and moves the later scope.
    set_open(&mut host, surface, tooltip, true);
    let open = draws(&mut renderer, &state, &mut host, world_id);
    assert_eq!(planes(&host, surface), [(0, 0.0), (1, 1.0), (2, 2.0)]);
    assert_eq!(open.len(), 3);
    assert!(near(open[0], base));
    assert!(near(open[1], offset(base, 0.5)));
    assert!(near(open[2], offset(base, 1.0)));

    // Closing the earlier scope repacks the later scope to rank1.
    set_open(&mut host, surface, tooltip, false);
    let closed = draws(&mut renderer, &state, &mut host, world_id);
    assert_eq!(planes(&host, surface), [(0, 0.0), (1, 1.0)]);
    assert_eq!(closed.len(), 2);
    assert!(near(closed[0], base));
    assert!(near(closed[1], toast));

    // Seen from behind, the toast's plane is farthest and draws first, at
    // its own depth from the base.
    edit(
        &mut host,
        surface,
        vec![ComponentValue::Transform(Transform {
            qy: 1.0,
            qw: 0.0,
            ..Default::default()
        })],
    );
    let behind = draws(&mut renderer, &state, &mut host, world_id);
    assert_eq!(behind.len(), 2);
    assert!(near(behind[0], offset(behind[1], 0.5)));
}

#[test]
fn a_cached_surface_that_separates_layers_reuses_one_image_with_cutouts() {
    let (mut host, mut renderer, state, world, surface) = scene();
    state.cache_limit.set(4096);
    edit(
        &mut host,
        surface,
        vec![ComponentValue::SurfaceCache(SurfaceCache {
            direct_distance: 0.0,
            resolution_scale: 1.0,
            allocation_priority: 1.0,
            max_refresh_hz: 10.0,
            max_texture_size: 0,
            force_cache: false,
        })],
    );
    let presentation = |renderer: &RenderService<TestDevice>| {
        let mut records = Vec::new();
        renderer.surface_cache_diagnostics(world, &mut records);
        records[0].presentation
    };
    draws(&mut renderer, &state, &mut host, world);
    assert!(!presentation(&renderer).is_direct());

    spacing(&mut host, surface, 0.5);
    let exploded = draws(&mut renderer, &state, &mut host, world);
    assert_eq!(presentation(&renderer), SurfaceCachePresentation::Repainted);
    let image = {
        let submissions = state.surface_mesh_draws.borrow();
        assert_eq!(submissions.len(), 2);
        assert_eq!(submissions[0].target, submissions[1].target);
        assert_eq!((submissions[0].cutout, submissions[1].cutout), (1, 2));
        submissions[0].target
    };
    assert_eq!(exploded.len(), 2);
    assert!(!near(exploded[0], exploded[1]));

    let repaints = state.cache_begins.get();
    let reused = draws(&mut renderer, &state, &mut host, world);
    assert_eq!(presentation(&renderer), SurfaceCachePresentation::Reused);
    assert_eq!(state.cache_begins.get(), repaints);
    assert_eq!(reused, exploded);
    assert!(
        state
            .surface_mesh_draws
            .borrow()
            .iter()
            .all(|draw| draw.target == image)
    );

    // Back on one plane, the flat image is exact again.
    spacing(&mut host, surface, 0.0);
    draws(&mut renderer, &state, &mut host, world);
    assert!(!presentation(&renderer).is_direct());
}

#[test]
fn a_single_occupied_priority_is_rank_zero_and_can_cache() {
    // Every entity at priority2: the only occupied group is physical rank0
    // at every spacing, so one flat image remains an exact presentation.
    let mut host = support::task_scheduler::host();
    let (world, mut renderer, state) = setup(&mut host);
    state.cache_limit.set(4096);
    let world_id = world.id();
    drop(world);
    let surface = CanvasSurface::new(&mut host, world_id, 0.0, plain_box([0.0, 0.0], 2));
    state.cache_limit.set(4096);
    edit(
        &mut host,
        surface,
        vec![ComponentValue::SurfaceCache(SurfaceCache {
            direct_distance: 0.0,
            resolution_scale: 1.0,
            allocation_priority: 1.0,
            max_refresh_hz: 10.0,
            max_texture_size: 0,
            force_cache: false,
        })],
    );
    let presentation = |renderer: &RenderService<TestDevice>| {
        let mut records = Vec::new();
        renderer.surface_cache_diagnostics(world_id, &mut records);
        records[0].presentation
    };
    draws(&mut renderer, &state, &mut host, world_id);
    assert_eq!(planes(&host, surface), [(0, 0.0)]);
    assert!(!presentation(&renderer).is_direct());

    spacing(&mut host, surface, 0.5);
    let exploded = draws(&mut renderer, &state, &mut host, world_id);
    assert!(!presentation(&renderer).is_direct());
    assert!(exploded.is_empty());

    spacing(&mut host, surface, 0.0);
    draws(&mut renderer, &state, &mut host, world_id);
    assert!(!presentation(&renderer).is_direct());
}
