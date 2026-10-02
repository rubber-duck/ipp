//! Canvas layers through real headless Host frames: plane ids resolved from
//! any depth, painter and hit order by plane then tree order, planes that keep
//! their ids as others come and go, clip scopes, inherited style, the
//! unchanged publication of a canvas without layers, and tree edits.

mod support;

use ipp_core::components::{GuiButton, GuiLayout, GuiScrollView};
use ipp_core::systems::canvas::{CanvasBox, CanvasPaintEntry, CanvasPublication, CanvasStyle};
use ipp_core::systems::gui::local::GuiLocalAction;
use ipp_core::*;
use std::mem::offset_of;
use support::gui_panel::*;

const EXTENT: [f32; 2] = [300.0, 200.0];

/// The canvas clip a raised entity's scope starts from.
const CANVAS: [f32; 4] = [0.0, 0.0, EXTENT[0], EXTENT[1]];

fn panel() -> GuiPanel {
    GuiPanel::with_canvas(
        CanvasState {
            extent: EXTENT,
            units_per_metre: 100.0,
        },
        Some(GuiLayout {
            kind: 3,
            ..Default::default()
        }),
    )
}

/// A top-left aligned layout of `size`, positioned by its CanvasStyle.
fn placed(size: [f32; 2]) -> GuiLayout {
    GuiLayout {
        width: size[0],
        height: size[1],
        align_x: -1.0,
        align_y: -1.0,
        ..Default::default()
    }
}

fn at(position: [f32; 2], layer: u32) -> CanvasStyle {
    CanvasStyle {
        x: position[0],
        y: position[1],
        layer,
        ..Default::default()
    }
}

/// An eligible button of `size` at `position` on plane `layer`, painting a
/// plain box over its bounds.
fn button(
    panel: &mut GuiPanel,
    parent: EntityId,
    position: [f32; 2],
    size: [f32; 2],
    layer: u32,
) -> EntityId {
    panel.create(
        Some(parent),
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(placed(size)),
            ComponentValue::CanvasStyle(at(position, layer)),
            ComponentValue::CanvasBox(CanvasBox {
                width: size[0],
                height: size[1],
                ..Default::default()
            }),
        ],
    )
}

/// A layout group of `size` styled by `style`.
fn group(panel: &mut GuiPanel, parent: EntityId, size: [f32; 2], style: CanvasStyle) -> EntityId {
    panel.create(
        Some(parent),
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                kind: 3,
                ..placed(size)
            }),
            ComponentValue::CanvasStyle(style),
        ],
    )
}

fn set_layer(panel: &mut GuiPanel, entity: EntityId, layer: u32) {
    let outcome = panel.set(
        entity,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, layer),
        FieldValue::U32(layer),
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
}

/// Painter position of an entity's plain box.
fn box_position(view: &CanvasPublication, entity: EntityId) -> usize {
    view.entries
        .iter()
        .position(|entry| {
            matches!(entry.as_ref(), CanvasPaintEntry::Primitive { primitive, .. }
                if primitive.style().identity.target.entity == entity
                    && primitive.style().identity.target.component == ComponentValue::CANVAS_BOX)
        })
        .unwrap_or_else(|| panic!("no box for {entity:?}"))
}

fn box_style(
    view: &CanvasPublication,
    entity: EntityId,
) -> ipp_core::systems::canvas::CanvasPrimitiveStyle {
    *primitive(&view.entries[box_position(view, entity)]).style()
}

/// Every hit is listed by layer, then tree-order ordinal, and every entry by
/// layer, each one of the ascending layers in use.
fn assert_ordered(view: &CanvasPublication) {
    assert!(
        view.hits.windows(2).all(
            |pair| (pair[0].layer, pair[0].paint_order) <= (pair[1].layer, pair[1].paint_order)
        ),
        "{:?}",
        view.hits
            .iter()
            .map(|hit| (hit.layer, hit.paint_order))
            .collect::<Vec<_>>()
    );
    assert!(
        view.entries
            .windows(2)
            .all(|pair| pair[0].layer() <= pair[1].layer())
    );
    assert!(view.layers.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        view.entries
            .iter()
            .all(|entry| view.layers.contains(&entry.layer()))
    );
}

/// The scene of the ordering tests, with `raised` 1:
///
/// ```text
/// root
///  ├ a      button (0, 0) 100 x 100
///  ├ b      group (50, 50), layer 1
///  │  ├ c   button (0, 0) 100 x 100          plane 1, its parent's
///  │  └ e   button (10, 10) 20 x 20, layer 1 plane 2, above its parent's
///  └ d      button (60, 60) 100 x 100
/// ```
struct Scene {
    panel: GuiPanel,
    a: EntityId,
    b: EntityId,
    c: EntityId,
    d: EntityId,
    e: EntityId,
}

fn scene(raised: u32) -> Scene {
    let mut panel = panel();
    let root = panel.root_entity;
    let a = button(&mut panel, root, [0.0, 0.0], [100.0, 100.0], 0);
    let b = group(&mut panel, root, [150.0, 150.0], at([50.0, 50.0], raised));
    let c = button(&mut panel, b, [0.0, 0.0], [100.0, 100.0], 0);
    let e = button(&mut panel, b, [10.0, 10.0], [20.0, 20.0], raised);
    let d = button(&mut panel, root, [60.0, 60.0], [100.0, 100.0], 0);
    panel.frame();
    Scene {
        panel,
        a,
        b,
        c,
        d,
        e,
    }
}

#[test]
fn paint_follows_layer_then_tree_order_and_hits_reverse_it() {
    let Scene {
        panel,
        a,
        c,
        d,
        e,
        ..
    } = scene(1);
    let view = panel.output();
    assert_eq!(*view.layers, [0, 1, 2]);
    assert_ordered(&view);

    // Tree order is a, c, e, d; layers 0, 1, 2, 0 paint a and d below c and e.
    let order = [a, d, c, e].map(|entity| box_position(&view, entity));
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    assert_eq!(
        [a, c, e, d].map(|entity| box_style(&view, entity).layer),
        [0, 1, 2, 0]
    );
    assert_eq!(
        [a, c, e, d].map(|entity| control_hit(&view, entity).layer),
        [0, 1, 2, 0]
    );

    // Keyboard traversal keeps tree order: d's ordinal follows the raised c.
    assert!(control_hit(&view, c).paint_order < control_hit(&view, d).paint_order);

    // c rises over d although d comes later in the tree; e over both.
    assert_eq!(hit_at(&view, [90.0, 90.0]), Some(c));
    assert_eq!(hit_at(&view, [65.0, 65.0]), Some(e));
    assert_eq!(hit_at(&view, [155.0, 155.0]), Some(d));
    assert_eq!(hit_at(&view, [10.0, 10.0]), Some(a));
}

#[test]
fn layers_are_plane_ids_that_keep_their_ids_and_leave_gaps() {
    let Scene {
        mut panel,
        b,
        c,
        e,
        ..
    } = scene(1);
    set_layer(&mut panel, e, 5);
    let view = panel.output();
    assert_eq!(*view.layers, [0, 1, 5]);
    assert_eq!(box_style(&view, e).layer, 5);
    assert_eq!(control_hit(&view, e).layer, 5);

    // Lowering b leaves planes 0 and 5, with c back on the base and e still
    // on plane 5 although no plane between them is in use.
    set_layer(&mut panel, b, 0);
    let view = panel.output();
    assert_eq!(*view.layers, [0, 5]);
    assert_eq!(box_style(&view, c).layer, 0);
    assert_eq!(box_style(&view, e).layer, 5);
    assert_ordered(&view);
}

#[test]
fn the_same_id_from_any_depth_shares_a_plane_and_a_nested_raise_rises_above_its_parent() {
    let mut panel = panel();
    let root = panel.root_entity;
    // A top-level button on plane 2, declared first.
    let top = button(&mut panel, root, [0.0, 0.0], [40.0, 40.0], 2);
    // A group on plane 1 holding, two levels down, a button on plane 2.
    let group_one = group(&mut panel, root, [200.0, 200.0], at([0.0, 0.0], 1));
    let inner = group(
        &mut panel,
        group_one,
        [200.0, 200.0],
        CanvasStyle::default(),
    );
    let deep = button(&mut panel, inner, [20.0, 20.0], [40.0, 40.0], 2);
    // Under the plane-2 button: zero keeps its plane, and ids 1 and 2, not
    // above it, both rise to plane 3.
    let same = button(&mut panel, deep, [0.0, 0.0], [10.0, 10.0], 0);
    let lower = button(&mut panel, deep, [10.0, 0.0], [10.0, 10.0], 1);
    let equal = button(&mut panel, deep, [20.0, 0.0], [10.0, 10.0], 2);
    // A later top-level button on the base.
    let later = button(&mut panel, root, [0.0, 0.0], [300.0, 200.0], 0);
    panel.frame();
    let view = panel.output();

    assert_eq!(*view.layers, [0, 1, 2, 3]);
    assert_eq!(
        [top, deep, same, lower, equal, later].map(|entity| box_style(&view, entity).layer),
        [2, 2, 2, 3, 3, 0]
    );
    assert_eq!(control_hit(&view, deep).layer, 2);
    assert_ordered(&view);

    // Plane 2 holds both declarations in tree order, above plane 1 and the
    // later base button, and below the plane-3 children.
    let order = [later, top, deep, same, lower, equal].map(|entity| box_position(&view, entity));
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    // The deep plane-2 button, later in the tree than the top-level one,
    // takes the hit where they overlap; its raised child over it.
    assert_eq!(hit_at(&view, [25.0, 35.0]), Some(deep));
    assert_eq!(hit_at(&view, [32.0, 22.0]), Some(lower));
    assert_eq!(hit_at(&view, [5.0, 5.0]), Some(top));
    assert_eq!(hit_at(&view, [250.0, 150.0]), Some(later));
}

#[test]
fn a_plane_keeps_its_id_and_paint_while_another_comes_and_goes() {
    let mut panel = panel();
    let root = panel.root_entity;
    let anchor = button(&mut panel, root, [0.0, 0.0], [100.0, 40.0], 0);
    // A toast-like entity on plane 3 and a tooltip-like one on the base,
    // under the anchor, until it is raised to plane 1.
    let toast = button(&mut panel, root, [150.0, 120.0], [100.0, 40.0], 3);
    let tooltip = button(&mut panel, anchor, [0.0, 40.0], [60.0, 20.0], 0);
    panel.frame();
    let shown = panel.output();
    assert_eq!(*shown.layers, [0, 3]);
    assert_eq!(box_style(&shown, toast).layer, 3);
    let toast_entry = |view: &CanvasPublication| view.entries[box_position(view, toast)].clone();

    set_layer(&mut panel, tooltip, 1);
    let open = panel.output();
    assert_eq!(*open.layers, [0, 1, 3]);
    assert_eq!(box_style(&open, tooltip).layer, 1);
    assert_eq!(box_style(&open, toast).layer, 3);
    // The toast's retained primitive is unchanged by the plane opening below it.
    assert!(std::sync::Arc::ptr_eq(
        &toast_entry(&shown),
        &toast_entry(&open)
    ));
    assert_ordered(&open);

    set_layer(&mut panel, tooltip, 0);
    let closed = panel.output();
    assert_eq!(*closed.layers, [0, 3]);
    assert!(std::sync::Arc::ptr_eq(
        &toast_entry(&shown),
        &toast_entry(&closed)
    ));
}

#[test]
fn a_canvas_without_layers_publishes_tree_order_unchanged() {
    let Scene {
        mut panel,
        a,
        b,
        c,
        d,
        e,
    } = scene(0);
    let flat = panel.output();
    assert_eq!(*flat.layers, [0]);
    assert!(flat.entries.iter().all(|entry| entry.layer() == 0));
    assert!(flat.hits.iter().all(|hit| hit.layer == 0));
    // Tree order is painter order, and each hit's ordinal is its paint's position.
    let order = [a, c, e, d].map(|entity| box_position(&flat, entity));
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    for entity in [a, c, e, d] {
        let hit = control_hit(&flat, entity);
        assert!(hit.paint_order as usize <= box_position(&flat, entity));
    }

    // Raising and lowering again restores the identical paint and hits; only
    // the revisions of the re-evaluated primitives advance.
    set_layer(&mut panel, b, 2);
    assert_eq!(*panel.output().layers, [0, 2]);
    set_layer(&mut panel, b, 0);
    let restored = panel.output();
    assert_eq!(*restored.layers, [0]);
    assert_eq!(restored.hits, flat.hits);
    let primitives = |view: &CanvasPublication| {
        view.entries
            .iter()
            .map(|entry| primitive(entry).clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(primitives(&restored), primitives(&flat));
}

#[test]
fn raised_content_starts_a_clip_scope_at_the_canvas_extent() {
    let mut panel = panel();
    let root = panel.root_entity;
    let clip = group(
        &mut panel,
        root,
        [100.0, 100.0],
        CanvasStyle {
            clipped: true,
            clip_max_x: 40.0,
            clip_max_y: 40.0,
            ..Default::default()
        },
    );
    let low = button(&mut panel, clip, [0.0, 0.0], [80.0, 80.0], 0);
    let high = button(&mut panel, clip, [0.0, 0.0], [80.0, 80.0], 1);
    let scoped = panel.create(
        Some(clip),
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                kind: 3,
                ..placed([80.0, 80.0])
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 10.0,
                y: 10.0,
                layer: 1,
                clipped: true,
                clip_max_x: 60.0,
                clip_max_y: 60.0,
                ..Default::default()
            }),
        ],
    );
    let inner = button(&mut panel, scoped, [0.0, 0.0], [80.0, 80.0], 0);
    panel.frame();
    let view = panel.output();

    assert_eq!(control_hit(&view, low).clip, [0.0, 0.0, 40.0, 40.0]);
    assert_eq!(box_style(&view, low).clip, [0.0, 0.0, 40.0, 40.0]);
    // The raised button escapes its lower-layer ancestor's clip.
    assert_eq!(control_hit(&view, high).clip, CANVAS);
    assert_eq!(box_style(&view, high).clip, CANVAS);
    assert_eq!(hit_at(&view, [75.0, 75.0]), Some(high));
    // A raised scope's own clip and its descendants' clips still apply.
    assert_eq!(control_hit(&view, inner).clip, [10.0, 10.0, 70.0, 70.0]);
    assert_eq!(control_hit(&view, inner).layer, 1);
    assert_eq!(box_style(&view, inner).clip, [10.0, 10.0, 70.0, 70.0]);
}

#[test]
fn raised_content_in_a_scroll_view_follows_its_offset_outside_its_viewport() {
    let mut panel = panel();
    let root = panel.root_entity;
    let view_entity = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView {
                axis: 1,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(placed([100.0, 50.0])),
        ],
    );
    let column = panel.create(
        Some(view_entity),
        vec![ComponentValue::GuiLayout(GuiLayout {
            kind: 2,
            ..placed([100.0, 200.0])
        })],
    );
    let row = button(&mut panel, column, [0.0, 0.0], [100.0, 30.0], 0);
    let popup = panel.create(
        Some(row),
        vec![
            ComponentValue::CanvasStyle(at([0.0, 30.0], 1)),
            ComponentValue::CanvasBox(CanvasBox {
                width: 100.0,
                height: 60.0,
                ..Default::default()
            }),
        ],
    );
    panel.frame();
    panel.frame();
    let view = panel.output();
    // The row stays inside the 50-unit viewport; the raised popup paints below it.
    assert_eq!(control_hit(&view, row).clip, [0.0, 0.0, 100.0, 50.0]);
    let style = box_style(&view, popup);
    assert_eq!(style.clip, CANVAS);
    assert_eq!(style.layer, 1);
    assert_eq!(style.position, [0.0, 30.0]);

    // Scrolling still moves the raised popup with its row.
    panel.act(view_entity, GuiLocalAction::ScrollTo([0.0, 10.0]));
    panel.frame();
    panel.frame();
    let style = box_style(&panel.output(), popup);
    assert_eq!(style.position, [0.0, 20.0]);
    assert_eq!(style.clip, CANVAS);
}

#[test]
fn raised_entities_inherit_translation_scale_tint_and_opacity() {
    let mut panel = panel();
    let root = panel.root_entity;
    let parent = group(
        &mut panel,
        root,
        [100.0, 100.0],
        CanvasStyle {
            x: 10.0,
            y: 20.0,
            scale_x: 2.0,
            scale_y: 2.0,
            red: 0.5,
            opacity: 0.5,
            ..Default::default()
        },
    );
    let child = panel.create(
        Some(parent),
        vec![
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 5.0,
                y: 5.0,
                green: 0.5,
                layer: 1,
                ..Default::default()
            }),
            ComponentValue::CanvasBox(CanvasBox {
                width: 10.0,
                height: 10.0,
                ..Default::default()
            }),
        ],
    );
    panel.frame();
    let style = box_style(&panel.output(), child);
    assert_eq!(style.position, [20.0, 30.0]);
    assert_eq!(style.scale, [2.0, 2.0]);
    assert_eq!(style.color, [0.5, 0.5, 1.0, 1.0]);
    assert_eq!(style.opacity, 0.5);
    assert_eq!(style.layer, 1);
    assert_eq!(style.clip, CANVAS);
}

#[test]
fn removal_and_reparenting_resolve_layers_again() {
    let Scene {
        mut panel,
        a,
        c,
        d,
        e,
        ..
    } = scene(1);
    assert_eq!(*panel.output().layers, [0, 1, 2]);

    // Removing the top layer's only entity leaves two planes.
    let outcome = panel.apply(vec![Command::Delete {
        entity: EntityRef::Handle(e),
    }]);
    assert!(outcome.result.is_ok(), "{outcome:?}");
    let view = panel.output();
    assert_eq!(*view.layers, [0, 1]);
    assert_eq!(box_style(&view, c).layer, 1);
    assert_eq!(hit_at(&view, [70.0, 70.0]), Some(c));
    assert_ordered(&view);

    // Moving c out of the raised group returns it to the base, after d.
    let outcome = panel.apply(vec![Command::PlaceEntity {
        entity: EntityRef::Handle(c),
        placement: EntityPlacementRef {
            parent: Some(EntityRef::Handle(panel.root_entity)),
            before: None,
        },
    }]);
    assert!(outcome.result.is_ok(), "{outcome:?}");
    let view = panel.output();
    assert_eq!(*view.layers, [0, 1]);
    assert_eq!(box_style(&view, c).layer, 0);
    assert_eq!(control_hit(&view, c).layer, 0);
    let order = [a, d, c].map(|entity| box_position(&view, entity));
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    // At its own origin now, c is the latest base target there.
    assert_eq!(hit_at(&view, [10.0, 10.0]), Some(c));
    assert_ordered(&view);
}
