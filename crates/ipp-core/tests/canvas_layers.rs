//! Relative layers through headless Host frames: sums across uneven depths,
//! compact ranks, repacking, retained ancestor clips and the all-zero fast path.

mod support;

use ipp_core::components::{GuiButton, GuiLayout, GuiScrollView};
use ipp_core::systems::canvas::{
    CanvasBox, CanvasLayerTransition, CanvasPaintEntry, CanvasPublication, CanvasStyle,
};
use ipp_core::systems::gui::local::GuiLocalAction;
use ipp_core::*;
use std::mem::offset_of;
use support::gui_panel::*;

const EXTENT: [f32; 2] = [300.0, 200.0];

/// The initial canvas clip.
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

/// An eligible button of `size` at `position` with relative offset `layer`, painting a
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
        view.hits
            .windows(2)
            .all(|pair| (pair[0].priority, pair[0].paint_order)
                <= (pair[1].priority, pair[1].paint_order)),
        "{:?}",
        view.hits
            .iter()
            .map(|hit| (hit.priority, hit.paint_order))
            .collect::<Vec<_>>()
    );
    assert!(
        view.entries
            .windows(2)
            .all(|pair| pair[0].layer() <= pair[1].layer())
    );
    assert!(view.layers.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert!(
        view.entries
            .iter()
            .all(|entry| view.layer_offset(entry.layer()).is_some())
    );
}

/// The scene of the ordering tests, with `raised` 1:
///
/// ```text
/// root
///  ├ a      button (0, 0) 100 x 100
///  ├ b      group (50, 50), layer 1
///  │  ├ c   button (0, 0) 100 x 100          level 1, inherited
///  │  └ e   button (10, 10) 20 x 20, layer 1 level 2, parent + 1
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
    assert_eq!(
        view.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1, 2].map(f64::from)
    );
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
fn large_offsets_publish_compact_ranks_and_removing_a_group_repacks() {
    let Scene {
        mut panel,
        b,
        c,
        e,
        ..
    } = scene(1);
    set_layer(&mut panel, e, 5);
    let view = panel.output();
    assert_eq!(
        view.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1, 2].map(f64::from)
    );
    assert_eq!(box_style(&view, e).layer, 2);
    assert_eq!(control_hit(&view, e).layer, 2);

    // Lowering b leaves two occupied groups; the later rank repacks.
    set_layer(&mut panel, b, 0);
    let view = panel.output();
    assert_eq!(
        view.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1].map(f64::from)
    );
    assert_eq!(box_style(&view, c).layer, 0);
    assert_eq!(box_style(&view, e).layer, 1);
    assert_ordered(&view);
}

#[test]
fn equal_relative_sums_share_a_group_across_uneven_depths() {
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
    let deep = button(&mut panel, inner, [20.0, 20.0], [40.0, 40.0], 1);
    // Under resolved level 2, zero inherits; offsets 1 and 2 make levels 3 and 4.
    let same = button(&mut panel, deep, [0.0, 0.0], [10.0, 10.0], 0);
    let lower = button(&mut panel, deep, [10.0, 0.0], [10.0, 10.0], 1);
    let equal = button(&mut panel, deep, [20.0, 0.0], [10.0, 10.0], 2);
    // A later top-level button on the base.
    let later = button(&mut panel, root, [0.0, 0.0], [300.0, 200.0], 0);
    panel.frame();
    let view = panel.output();

    assert_eq!(
        view.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4].map(f64::from)
    );
    assert_eq!(
        [top, deep, same, lower, equal, later].map(|entity| box_style(&view, entity).layer),
        [2, 2, 2, 3, 4, 0]
    );
    assert_eq!(control_hit(&view, deep).layer, 2);
    assert_ordered(&view);

    // Resolved level 2 holds both declarations in tree order, above level 1
    // and the later base button, and below the level-3/4 children.
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
fn adding_and_removing_an_occupied_group_repacks_later_ranks() {
    let mut panel = panel();
    let root = panel.root_entity;
    let anchor = button(&mut panel, root, [0.0, 0.0], [100.0, 40.0], 0);
    // A toast-like entity on plane 3 and a tooltip-like one on the base,
    // under the anchor, until it is raised to plane 1.
    let toast = button(&mut panel, root, [150.0, 120.0], [100.0, 40.0], 3);
    let tooltip = button(&mut panel, anchor, [0.0, 40.0], [60.0, 20.0], 0);
    panel.frame();
    let shown = panel.output();
    assert_eq!(
        shown
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1].map(f64::from)
    );
    assert_eq!(box_style(&shown, toast).layer, 1);
    let toast_entry = |view: &CanvasPublication| view.entries[box_position(view, toast)].clone();

    set_layer(&mut panel, tooltip, 1);
    let open = panel.output();
    assert_eq!(
        open.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1, 2].map(f64::from)
    );
    assert_eq!(box_style(&open, tooltip).layer, 1);
    assert_eq!(box_style(&open, toast).layer, 2);
    // Repacking changes the physical rank stored in the retained primitive.
    assert!(!std::sync::Arc::ptr_eq(
        &toast_entry(&shown),
        &toast_entry(&open)
    ));
    assert_ordered(&open);

    set_layer(&mut panel, tooltip, 0);
    let closed = panel.output();
    assert_eq!(
        closed
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1].map(f64::from)
    );
    assert_eq!(box_style(&closed, toast).layer, 1);
    assert_eq!(
        primitive(&toast_entry(&shown)),
        primitive(&toast_entry(&closed))
    );
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
    assert_eq!(
        flat.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0].map(f64::from)
    );
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
    assert_eq!(
        panel
            .output()
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1].map(f64::from)
    );
    set_layer(&mut panel, b, 0);
    let restored = panel.output();
    assert_eq!(
        restored
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0].map(f64::from)
    );
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
fn ordinary_raised_content_retains_ancestor_clips() {
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
    // Raising ordinary content preserves the ancestor clip.
    assert_eq!(control_hit(&view, high).clip, [0.0, 0.0, 40.0, 40.0]);
    assert_eq!(box_style(&view, high).clip, [0.0, 0.0, 40.0, 40.0]);
    assert_eq!(hit_at(&view, [75.0, 75.0]), None);
    // The raised root's own clip intersects the ancestor clip.
    assert_eq!(control_hit(&view, inner).clip, [10.0, 10.0, 40.0, 40.0]);
    assert_eq!(control_hit(&view, inner).layer, 1);
    assert_eq!(box_style(&view, inner).clip, [10.0, 10.0, 40.0, 40.0]);
}

#[test]
fn raised_content_in_a_scroll_view_moves_with_its_offset_and_stays_clipped() {
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
    // Raised ordinary content stays clipped to the same scroll viewport.
    assert_eq!(control_hit(&view, row).clip, [0.0, 0.0, 100.0, 50.0]);
    let style = box_style(&view, popup);
    assert_eq!(style.clip, [0.0, 0.0, 100.0, 50.0]);
    assert_eq!(style.layer, 1);
    assert_eq!(style.position, [0.0, 30.0]);

    // Scrolling still moves the raised popup with its row.
    panel.act(view_entity, GuiLocalAction::ScrollTo([0.0, 10.0]));
    panel.frame();
    panel.frame();
    let style = box_style(&panel.output(), popup);
    assert_eq!(style.position, [0.0, 20.0]);
    assert_eq!(style.clip, [0.0, 0.0, 100.0, 50.0]);
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
    assert_eq!(
        panel
            .output()
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1, 2].map(f64::from)
    );

    // Removing the top layer's only entity leaves two planes.
    let outcome = panel.apply(vec![Command::Delete {
        entity: EntityRef::Handle(e),
    }]);
    assert!(outcome.result.is_ok(), "{outcome:?}");
    let view = panel.output();
    assert_eq!(
        view.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1].map(f64::from)
    );
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
    assert_eq!(
        view.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1].map(f64::from)
    );
    assert_eq!(box_style(&view, c).layer, 0);
    assert_eq!(control_hit(&view, c).layer, 0);
    let order = [a, d, c].map(|entity| box_position(&view, entity));
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    // At its own origin now, c is the latest base target there.
    assert_eq!(hit_at(&view, [10.0, 10.0]), Some(c));
    assert_ordered(&view);
}

#[test]
fn relative_sums_beyond_u32_do_not_saturate_or_overflow() {
    let mut panel = panel();
    let root = panel.root_entity;
    let parent = group(&mut panel, root, [100.0, 100.0], at([0.0, 0.0], u32::MAX));
    let child = button(&mut panel, parent, [0.0, 0.0], [20.0, 20.0], u32::MAX);
    let later = button(&mut panel, root, [0.0, 0.0], [20.0, 20.0], u32::MAX);
    panel.frame();
    let view = panel.output();
    assert_eq!(
        view.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1, 2].map(f64::from)
    );
    assert_eq!(box_style(&view, child).layer, 2);
    assert_eq!(box_style(&view, later).layer, 1);
    assert_eq!(hit_at(&view, [5.0, 5.0]), Some(child));
}

#[test]
fn empty_structural_roots_occupy_groups_and_zero_wrappers_do_not() {
    let mut panel = panel();
    let root = panel.root_entity;
    let empty = group(&mut panel, root, [100.0, 100.0], at([0.0, 0.0], 10));
    let wrapper = group(&mut panel, root, [100.0, 100.0], CanvasStyle::default());
    let high = button(&mut panel, wrapper, [0.0, 0.0], [20.0, 20.0], 20);
    panel.frame();
    assert_eq!(
        panel
            .output()
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1, 2].map(f64::from)
    );
    assert_eq!(box_style(&panel.output(), high).layer, 2);
    set_layer(&mut panel, empty, 0);
    assert_eq!(
        panel
            .output()
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1].map(f64::from)
    );
    assert_eq!(box_style(&panel.output(), high).layer, 1);
}

fn transition(panel: &mut GuiPanel, entity: EntityId, previous_layer: u32, progress: f32) {
    let outcome = panel.apply(vec![Command::insert_value(
        EntityRef::Handle(entity),
        ComponentValue::CanvasLayerTransition(CanvasLayerTransition {
            previous_layer,
            progress,
        }),
    )]);
    assert!(outcome.result.is_ok(), "{outcome:?}");
}

fn plane_offset(view: &CanvasPublication, entity: EntityId) -> f64 {
    view.layer_offset(box_style(view, entity).layer).unwrap()
}

#[test]
fn transition_planes_move_independently_of_destination_painter_and_hit_order() {
    let mut panel = panel();
    let root = panel.root_entity;
    let moving = button(&mut panel, root, [0.0; 2], [30.0; 2], 10);
    let other = button(&mut panel, root, [0.0; 2], [30.0; 2], 20);
    transition(&mut panel, moving, 30, 0.25);
    let before = panel.output();
    assert_eq!(plane_offset(&before, moving), 2.5);
    assert_eq!(plane_offset(&before, other), 2.0);
    assert!(box_style(&before, moving).layer > box_style(&before, other).layer);
    assert!(control_hit(&before, moving).priority < control_hit(&before, other).priority);
    assert!(box_position(&before, moving) < box_position(&before, other));
    // A root Canvas ignores physical separation and selects logical destination order.
    assert_eq!(hit_at(&before, [5.0; 2]), Some(other));
    let outcome = panel.set(
        moving,
        ComponentValue::CANVAS_LAYER_TRANSITION,
        offset_of!(CanvasLayerTransition, progress),
        FieldValue::F32(0.375),
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
    let after = panel.output();
    assert_eq!(plane_offset(&after, moving), 2.25);
    assert_eq!(before.paint_revision, after.paint_revision);
    assert!(after.input_revision > before.input_revision);
    assert!(
        before
            .entries
            .iter()
            .zip(after.entries.iter())
            .all(|(a, b)| std::sync::Arc::ptr_eq(a, b))
    );
}

#[test]
fn nested_transition_roots_add_independent_motion_and_reserve_overlay_scope_extent() {
    use ipp_core::components::GuiOverlay;
    let mut panel = panel();
    let root = panel.root_entity;
    let owner = button(&mut panel, root, [0.0; 2], [30.0; 2], 10);
    let decoration = button(&mut panel, owner, [0.0; 2], [10.0; 2], 0);
    let nested = button(&mut panel, owner, [0.0; 2], [10.0; 2], 10);
    let overlay = button(&mut panel, root, [0.0; 2], [30.0; 2], 0);
    assert!(
        panel
            .apply(vec![Command::insert_value(
                EntityRef::Handle(overlay),
                ComponentValue::GuiOverlay(GuiOverlay::default())
            )])
            .result
            .is_ok()
    );
    transition(&mut panel, owner, 30, 0.25);
    transition(&mut panel, nested, 0, 0.5);
    let view = panel.output();
    assert_eq!(plane_offset(&view, owner), 2.5);
    assert_eq!(plane_offset(&view, decoration), 2.5);
    assert_eq!(
        box_style(&view, owner).layer,
        box_style(&view, decoration).layer
    );
    assert_eq!(plane_offset(&view, nested), 3.0);
    // Parent range [1,3], child delta [0,1]: complete scope reserves [0,4].
    assert_eq!(plane_offset(&view, overlay), 5.0);
    assert!(control_hit(&view, overlay).priority > control_hit(&view, nested).priority);
    assert!(
        panel
            .set(
                owner,
                ComponentValue::CANVAS_LAYER_TRANSITION,
                offset_of!(CanvasLayerTransition, progress),
                FieldValue::F32(1.0)
            )
            .result
            .is_ok()
    );
    let completed = panel.output();
    assert_eq!(plane_offset(&completed, owner), 1.0);
    assert_eq!(plane_offset(&completed, nested), 1.5);
    assert_eq!(plane_offset(&completed, overlay), 5.0);
    assert!(
        panel
            .apply(vec![Command::RemoveComponent {
                entity: EntityRef::Handle(owner),
                component: ComponentValue::CANVAS_LAYER_TRANSITION
            }])
            .result
            .is_ok()
    );
    let removed = panel.output();
    assert_eq!(plane_offset(&removed, owner), 1.0);
    assert_eq!(plane_offset(&removed, nested), 1.5);
    assert_eq!(plane_offset(&removed, overlay), 3.0);
}

#[test]
fn removing_an_unfinished_previous_endpoint_snaps_to_destination_without_history() {
    let mut panel = panel();
    let root = panel.root_entity;
    let moving = button(&mut panel, root, [0.0; 2], [30.0; 2], 10);
    button(&mut panel, root, [0.0; 2], [30.0; 2], 20);
    transition(&mut panel, moving, 30, 0.25);
    assert_eq!(plane_offset(&panel.output(), moving), 2.5);
    assert!(
        panel
            .apply(vec![Command::RemoveComponent {
                entity: EntityRef::Handle(moving),
                component: ComponentValue::CANVAS_LAYER_TRANSITION
            }])
            .result
            .is_ok()
    );
    assert_eq!(plane_offset(&panel.output(), moving), 1.0);
    transition(&mut panel, moving, 30, 0.0);
    assert_eq!(plane_offset(&panel.output(), moving), 3.0);
}

#[test]
fn progress_bounds_are_validated_on_insert_and_field_writes() {
    let mut panel = panel();
    let root = panel.root_entity;
    let moving = button(&mut panel, root, [0.0; 2], [30.0; 2], 10);
    for progress in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        let outcome = panel.apply(vec![Command::insert_value(
            EntityRef::Handle(moving),
            ComponentValue::CanvasLayerTransition(CanvasLayerTransition {
                previous_layer: 20,
                progress,
            }),
        )]);
        assert!(outcome.result.is_err(), "{progress}: {outcome:?}");
    }
    transition(&mut panel, moving, 20, 0.5);
    for progress in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        assert!(
            panel
                .set(
                    moving,
                    ComponentValue::CANVAS_LAYER_TRANSITION,
                    offset_of!(CanvasLayerTransition, progress),
                    FieldValue::F32(progress)
                )
                .result
                .is_err()
        );
    }
    for progress in [0.0, 0.5, 1.0] {
        assert!(
            panel
                .set(
                    moving,
                    ComponentValue::CANVAS_LAYER_TRANSITION,
                    offset_of!(CanvasLayerTransition, progress),
                    FieldValue::F32(progress)
                )
                .result
                .is_ok()
        );
    }
}

#[test]
fn ordinary_animation_driver_updates_progress_and_invalidates_on_endpoint_component_removal() {
    use ipp_core::services::asset_management::{AssetUpload, AssetUploadIdentity};
    use ipp_core::systems::animation::*;
    use support::WorldTestDriver;
    let mut panel = panel();
    let root = panel.root_entity;
    let moving = button(&mut panel, root, [0.0; 2], [30.0; 2], 10);
    button(&mut panel, root, [0.0; 2], [30.0; 2], 20);
    transition(&mut panel, moving, 30, 0.0);
    let property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::CANVAS_LAYER_TRANSITION,
        offsets: vec![offset_of!(CanvasLayerTransition, progress) as u32],
    });
    let clip = AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: property.clone(),
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(ipp_core::components::schema::FieldValue::F32(
                        0.0,
                    )),
                    interpolation: AnimationInterpolation::Linear,
                },
                AnimationKeyframe {
                    time: 1.0,
                    value: AnimationValue::Field(ipp_core::components::schema::FieldValue::F32(
                        1.0,
                    )),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        }],
    )
    .unwrap();
    let controller = {
        let mut world = panel.host.world_mut(panel.world).unwrap();
        world
            .enqueue_asset(AssetUpload {
                id: 1,
                key: AssetUploadIdentity {
                    kind: ANIMATION_TYPE,
                    asset: 901,
                    variant: 0,
                },
                bytes: clip.encode(),
            })
            .unwrap();
        assert!(world.await_upload_for_test().assets[0].result.is_ok());
        let controller = world
            .create_animation_controller(AnimationControllerDescription {
                drivers: vec![AnimationDriverDescription {
                    source: "asset://10/901".into(),
                    variant: 0,
                    track: 0,
                    target: moving,
                    property,
                    entity_bindings: vec![],
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                }],
                ..Default::default()
            })
            .unwrap();
        world
            .control_animation_controller(controller, AnimationPlaybackControl::Play)
            .unwrap();
        world
            .control_animation_controller(controller, AnimationPlaybackControl::Seek(0.25))
            .unwrap();
        controller
    };
    panel.frame();
    assert_eq!(plane_offset(&panel.output(), moving), 2.5);
    assert!(
        panel
            .apply(vec![Command::RemoveComponent {
                entity: EntityRef::Handle(moving),
                component: ComponentValue::CANVAS_LAYER_TRANSITION
            }])
            .result
            .is_ok()
    );
    assert_eq!(plane_offset(&panel.output(), moving), 1.0);
    transition(&mut panel, moving, 30, 0.0);
    panel
        .host
        .world_mut(panel.world)
        .unwrap()
        .control_animation_controller(controller, AnimationPlaybackControl::Seek(0.75))
        .unwrap();
    panel.frame();
    // The prepared driver pins the old component incarnation; reinsertion cannot revive it.
    assert_eq!(plane_offset(&panel.output(), moving), 3.0);
}

#[test]
fn inherited_decorations_share_exact_planes_at_nonbinary_and_tiny_progress() {
    for progress in [0.1_f32, 0.3, 1e-10] {
        let mut panel = panel();
        let root = panel.root_entity;
        let owner = button(&mut panel, root, [0.0; 2], [30.0; 2], 1);
        let decoration = button(&mut panel, owner, [0.0; 2], [10.0; 2], 0);
        let nested = button(&mut panel, owner, [0.0; 2], [10.0; 2], 1);
        let nested_decoration = button(&mut panel, nested, [0.0; 2], [5.0; 2], 0);
        transition(&mut panel, owner, 0, progress);
        transition(&mut panel, nested, 0, progress);
        // An explicit completed transition to relative zero also coincides
        // exactly with the owner, even when its previous delta was nonzero.
        transition(&mut panel, decoration, 1, 1.0);
        let view = panel.output();
        assert_eq!(plane_offset(&view, owner), f64::from(progress));
        assert_eq!(plane_offset(&view, nested), 2.0 * f64::from(progress));
        assert_eq!(
            box_style(&view, owner).layer,
            box_style(&view, decoration).layer,
            "{progress}"
        );
        assert_eq!(
            box_style(&view, nested).layer,
            box_style(&view, nested_decoration).layer,
            "{progress}"
        );
        assert_eq!(plane_offset(&view, owner), plane_offset(&view, decoration));
        assert_eq!(
            plane_offset(&view, nested),
            plane_offset(&view, nested_decoration)
        );
    }
}

#[test]
fn additive_nested_ranges_keep_a_new_overlay_scope_above_all_previous_endpoints() {
    let mut panel = panel();
    let root = panel.root_entity;
    let owner = button(&mut panel, root, [0.0; 2], [30.0; 2], 1);
    let child = button(&mut panel, owner, [0.0; 2], [10.0; 2], 1);
    let overlay = button(&mut panel, child, [0.0; 2], [10.0; 2], 0);
    assert!(
        panel
            .apply(vec![Command::insert_value(
                EntityRef::Handle(overlay),
                ComponentValue::GuiOverlay(ipp_core::components::GuiOverlay::default())
            )])
            .result
            .is_ok()
    );
    transition(&mut panel, owner, 100, 0.0);
    transition(&mut panel, child, 100, 0.0);
    let view = panel.output();
    // Local endpoint keys 0,1,2,100,101 resolve to ranks 0..4.
    // Parent source3 + child's independent delta(4-1) =6; overlay resets to7.
    assert_eq!(plane_offset(&view, owner), 3.0);
    assert_eq!(plane_offset(&view, child), 6.0);
    assert_eq!(plane_offset(&view, overlay), 7.0);
    assert!(
        panel
            .set(
                owner,
                ComponentValue::CANVAS_LAYER_TRANSITION,
                offset_of!(CanvasLayerTransition, progress),
                FieldValue::F32(0.5)
            )
            .result
            .is_ok()
    );
    assert!(
        panel
            .set(
                child,
                ComponentValue::CANVAS_LAYER_TRANSITION,
                offset_of!(CanvasLayerTransition, progress),
                FieldValue::F32(0.5)
            )
            .result
            .is_ok()
    );
    let view = panel.output();
    assert_eq!(plane_offset(&view, owner), 2.0);
    assert_eq!(plane_offset(&view, child), 4.0);
    assert_eq!(plane_offset(&view, overlay), 7.0);
}
