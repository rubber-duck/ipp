//! Overlay placement through real headless Host frames: out of flow, placed
//! against the parent's evaluated box or the canvas on each side and
//! alignment, flipped, shifted and limited to the room available, following
//! its anchor in the same frame, stacked when nested, and costing nothing
//! while closed.

mod support;

use ipp_core::components::{GuiBehavior, GuiButton, GuiLayout, GuiOverlay, GuiScrollView};
use ipp_core::systems::canvas::{
    CanvasBox, CanvasPaintEntry, CanvasPublication, CanvasStyle, CanvasSystem,
};
use ipp_core::systems::gui::layout::GuiEntityLayoutDiagnostic;
use ipp_core::systems::gui::local::GuiLocalAction;
use ipp_core::systems::gui::presentation::{GuiCanvasPublication, GuiOverlayObservation};
use ipp_core::*;
use std::mem::offset_of;
use support::gui_panel::*;

const EXTENT: [f32; 2] = [300.0, 200.0];

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

/// A top-left aligned layout of `size`.
fn placed(size: [f32; 2]) -> GuiLayout {
    GuiLayout {
        width: size[0],
        height: size[1],
        align_x: -1.0,
        align_y: -1.0,
        ..Default::default()
    }
}

fn at(position: [f32; 2]) -> CanvasStyle {
    CanvasStyle {
        x: position[0],
        y: position[1],
        ..Default::default()
    }
}

/// A button of `size` translated to `position` in its parent.
fn trigger(panel: &mut GuiPanel, parent: EntityId, position: [f32; 2], size: [f32; 2]) -> EntityId {
    panel.create(
        Some(parent),
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(placed(size)),
            ComponentValue::CanvasStyle(at(position)),
        ],
    )
}

fn overlay(side: u32, align: u32) -> GuiOverlay {
    GuiOverlay {
        side,
        align,
        ..Default::default()
    }
}

/// A raised overlay button of `size` under `parent`, offset by `offset`.
fn popup(
    panel: &mut GuiPanel,
    parent: Option<EntityId>,
    placement: GuiOverlay,
    size: [f32; 2],
    offset: [f32; 2],
) -> EntityId {
    panel.create(
        parent,
        vec![
            ComponentValue::GuiOverlay(placement),
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(placed(size)),
            ComponentValue::CanvasStyle(CanvasStyle {
                layer: 1,
                ..at(offset)
            }),
        ],
    )
}

fn set_overlay(panel: &mut GuiPanel, entity: EntityId, placement: GuiOverlay) {
    let write = |offset: usize, value: u32| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_OVERLAY,
        field: FieldWrite {
            offset: offset as u32,
            value: FieldValue::U32(value),
        },
    };
    let outcome = panel.apply(vec![
        write(offset_of!(GuiOverlay, side), placement.side),
        write(offset_of!(GuiOverlay, align), placement.align),
    ]);
    assert!(outcome.result.is_ok(), "{outcome:?}");
}

fn set_style(panel: &mut GuiPanel, entity: EntityId, offset: usize, value: f32) {
    let outcome = panel.set(
        entity,
        ComponentValue::CANVAS_STYLE,
        offset,
        FieldValue::F32(value),
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
}

fn set_visible(panel: &mut GuiPanel, entity: EntityId, visible: bool) {
    let outcome = panel.set(
        entity,
        ComponentValue::GUI_BEHAVIOR,
        offset_of!(GuiBehavior, visible),
        FieldValue::Bool(visible),
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
}

/// The open overlays of the latest publication, topmost last.
fn overlays(panel: &GuiPanel) -> Vec<GuiOverlayObservation> {
    let publication = panel
        .host
        .publication(panel.host.latest_publication(panel.world).unwrap())
        .unwrap();
    let gui = publication
        .chunk(CanvasSystem::ID)
        .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
        .unwrap();
    gui.views
        .values()
        .flat_map(|view| view.overlays.iter().cloned())
        .collect()
}

fn diagnostics(panel: &mut GuiPanel) -> Vec<GuiEntityLayoutDiagnostic> {
    panel
        .host
        .world_mut(panel.world)
        .unwrap()
        .gui_entity_layout_diagnostics()
        .unwrap()
        .to_vec()
}

/// Whether any paint or hit of the publication comes from `entity`.
fn appears(view: &CanvasPublication, entity: EntityId) -> bool {
    view.hits.iter().any(|hit| hit.target.entity == entity)
        || view.entries.iter().any(|entry| match entry.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } => primitive.style().identity.target.entity == entity,
            CanvasPaintEntry::Attachment(slot) => slot.anchor == entity,
        })
}

#[test]
fn an_overlay_takes_no_space_in_its_parents_flow() {
    let build = |with_overlay: bool| {
        let mut panel = panel();
        let root = panel.root_entity;
        let view = panel.create(
            Some(root),
            vec![
                ComponentValue::GuiScrollView(GuiScrollView {
                    axis: 1,
                    ..Default::default()
                }),
                ComponentValue::GuiLayout(placed([120.0, 150.0])),
            ],
        );
        let column = panel.node(
            view,
            GuiLayout {
                kind: 2,
                width: 120.0,
                ..Default::default()
            },
        );
        let first = panel.button(column, placed([100.0, 30.0]));
        let hanging = with_overlay.then(|| {
            popup(
                &mut panel,
                Some(column),
                overlay(0, 0),
                [80.0, 90.0],
                [0.0, 0.0],
            )
        });
        let second = panel.button(column, placed([100.0, 30.0]));
        // A single-child container keeps its one child beside an overlay.
        let padded = panel.node(
            root,
            GuiLayout {
                kind: 4,
                ..placed([60.0, 60.0])
            },
        );
        let only = panel.button(padded, placed([20.0, 20.0]));
        if with_overlay {
            popup(
                &mut panel,
                Some(padded),
                overlay(0, 0),
                [10.0, 10.0],
                [0.0, 0.0],
            );
        }
        panel.frame();
        let view_fields = panel.scroll(view);
        let output = panel.output();
        (
            view_fields.content,
            bounds(&output, first),
            bounds(&output, second),
            bounds(&output, only),
            hanging.map(|hanging| bounds(&output, hanging)),
            diagnostics(&mut panel),
        )
    };
    let (content, first, second, only, _, diagnostics_without) = build(false);
    let (with_content, with_first, with_second, with_only, popup, with_diagnostics) = build(true);

    // The column measures only its two buttons, and the siblings keep their
    // places; the overlay hangs below the column's top edge instead.
    assert_eq!(content, [120.0, 60.0]);
    assert_eq!(with_content, content);
    assert_eq!((with_first, with_second, with_only), (first, second, only));
    assert_eq!(popup, Some(rect(0.0, 60.0, 80.0, 90.0)));
    assert!(diagnostics_without.is_empty());
    assert!(with_diagnostics.is_empty(), "{with_diagnostics:?}");
}

#[test]
fn overlays_take_each_side_and_alignment_of_their_parent() {
    let mut panel = panel();
    let root = panel.root_entity;
    let anchor = trigger(&mut panel, root, [100.0, 80.0], [60.0, 20.0]);
    let list = popup(
        &mut panel,
        Some(anchor),
        overlay(0, 0),
        [40.0, 30.0],
        [0.0, 0.0],
    );
    panel.frame();
    assert_eq!(bounds(&panel.output(), anchor), [100.0, 80.0, 160.0, 100.0]);

    let cases = [
        (overlay(0, 0), [100.0, 100.0, 140.0, 130.0]),
        (overlay(0, 1), [110.0, 100.0, 150.0, 130.0]),
        (overlay(0, 2), [120.0, 100.0, 160.0, 130.0]),
        // Stretch matches the trigger's width over the authored one.
        (overlay(0, 3), [100.0, 100.0, 160.0, 130.0]),
        (overlay(1, 0), [100.0, 50.0, 140.0, 80.0]),
        (overlay(1, 3), [100.0, 50.0, 160.0, 80.0]),
        (overlay(2, 0), [160.0, 80.0, 200.0, 110.0]),
        (overlay(2, 1), [160.0, 75.0, 200.0, 105.0]),
        (overlay(3, 2), [60.0, 70.0, 100.0, 100.0]),
        (overlay(3, 3), [60.0, 80.0, 100.0, 100.0]),
        // Centred over the box, aligned horizontally.
        (overlay(4, 1), [110.0, 75.0, 150.0, 105.0]),
        (overlay(4, 0), [100.0, 75.0, 140.0, 105.0]),
    ];
    for (placement, expected) in cases {
        set_overlay(&mut panel, list, placement);
        assert_eq!(bounds(&panel.output(), list), expected, "{placement:?}");
    }

    // The overlay's own translation offsets it: a gap below the trigger.
    set_overlay(&mut panel, list, overlay(0, 0));
    set_style(&mut panel, list, offset_of!(CanvasStyle, y), 4.0);
    set_style(&mut panel, list, offset_of!(CanvasStyle, x), -6.0);
    assert_eq!(bounds(&panel.output(), list), [94.0, 104.0, 134.0, 134.0]);
}

#[test]
fn an_overlay_flips_to_the_side_with_more_room_and_is_limited_to_it() {
    let mut panel = panel();
    let root = panel.root_entity;
    let anchor = trigger(&mut panel, root, [100.0, 160.0], [60.0, 20.0]);
    let list = popup(
        &mut panel,
        Some(anchor),
        overlay(0, 0),
        [40.0, 30.0],
        [0.0, 4.0],
    );
    panel.frame();
    // Below has 16 units after the 4-unit gap; above has 156, and the gap
    // stays between the trigger and the flipped list.
    assert_eq!(bounds(&panel.output(), list), [100.0, 126.0, 140.0, 156.0]);

    // A trigger at the top opens a list that wants to go above it downwards,
    // keeping the gap authored upwards.
    set_style(&mut panel, anchor, offset_of!(CanvasStyle, y), 10.0);
    set_overlay(&mut panel, list, overlay(1, 0));
    set_style(&mut panel, list, offset_of!(CanvasStyle, y), -4.0);
    assert_eq!(bounds(&panel.output(), list), [100.0, 34.0, 140.0, 64.0]);

    // Too tall for either side: it takes the side with more room, limited to it.
    let tall = popup(
        &mut panel,
        Some(anchor),
        overlay(0, 0),
        [40.0, 300.0],
        [0.0, 0.0],
    );
    panel.frame();
    assert_eq!(bounds(&panel.output(), tall), [100.0, 30.0, 140.0, 200.0]);
    set_style(&mut panel, anchor, offset_of!(CanvasStyle, y), 120.0);
    assert_eq!(bounds(&panel.output(), tall), [100.0, 0.0, 140.0, 120.0]);
    // Without more room opposite, it stays on its side, limited to it.
    set_style(&mut panel, anchor, offset_of!(CanvasStyle, y), 80.0);
    assert_eq!(bounds(&panel.output(), tall), [100.0, 100.0, 140.0, 200.0]);
}

#[test]
fn an_overlay_shifts_across_its_side_to_stay_inside_the_canvas() {
    let mut panel = panel();
    let root = panel.root_entity;
    let right = trigger(&mut panel, root, [270.0, 50.0], [30.0, 20.0]);
    let below_right = popup(
        &mut panel,
        Some(right),
        overlay(0, 0),
        [40.0, 30.0],
        [0.0, 0.0],
    );
    let left = trigger(&mut panel, root, [0.0, 50.0], [30.0, 20.0]);
    let below_left = popup(
        &mut panel,
        Some(left),
        overlay(0, 2),
        [40.0, 30.0],
        [0.0, 0.0],
    );
    let top = trigger(&mut panel, root, [100.0, 0.0], [30.0, 20.0]);
    let beside_top = popup(
        &mut panel,
        Some(top),
        overlay(2, 2),
        [40.0, 30.0],
        [0.0, 0.0],
    );
    let bottom = trigger(&mut panel, root, [100.0, 190.0], [30.0, 10.0]);
    let beside_bottom = popup(
        &mut panel,
        Some(bottom),
        overlay(3, 0),
        [40.0, 30.0],
        [0.0, 0.0],
    );
    panel.frame();
    let output = panel.output();
    assert_eq!(bounds(&output, below_right), [260.0, 70.0, 300.0, 100.0]);
    assert_eq!(bounds(&output, below_left), [0.0, 70.0, 40.0, 100.0]);
    assert_eq!(bounds(&output, beside_top), [130.0, 0.0, 170.0, 30.0]);
    assert_eq!(bounds(&output, beside_bottom), [60.0, 170.0, 100.0, 200.0]);
}

#[test]
fn content_beyond_the_room_scrolls_inside_the_limited_overlay() {
    let mut panel = panel();
    let root = panel.root_entity;
    let anchor = trigger(&mut panel, root, [20.0, 100.0], [80.0, 20.0]);
    let list = panel.create(
        Some(anchor),
        vec![
            ComponentValue::GuiOverlay(overlay(0, 3)),
            ComponentValue::GuiLayout(GuiLayout {
                kind: 2,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                layer: 1,
                ..Default::default()
            }),
        ],
    );
    let view = panel.create(
        Some(list),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView {
                axis: 1,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(GuiLayout {
                max_height: 150.0,
                ..Default::default()
            }),
        ],
    );
    let rows = panel.node(
        view,
        GuiLayout {
            kind: 2,
            ..Default::default()
        },
    );
    for _ in 0..10 {
        panel.button(
            rows,
            GuiLayout {
                kind: 3,
                height: 30.0,
                ..Default::default()
            },
        );
    }
    panel.frame();
    panel.frame();
    // The 150-unit list fits neither the 80 units below nor the 100 above;
    // it flips above, is limited to 100 and scrolls 200 of its 300.
    assert_eq!(bounds(&panel.output(), view), [20.0, 0.0, 100.0, 100.0]);
    let fields = panel.scroll(view);
    assert_eq!(fields.viewport, [80.0, 100.0]);
    assert_eq!(fields.content, [80.0, 300.0]);
    assert_eq!(fields.capacity, [0.0, 200.0]);
    assert_eq!(panel.layout(list).size, [80.0, 100.0]);
}

#[test]
fn an_overlay_follows_its_anchor_through_scrolling_and_translation_in_the_same_frame() {
    let mut panel = panel();
    let root = panel.root_entity;
    let group = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                kind: 3,
                ..placed([150.0, 100.0])
            }),
            ComponentValue::CanvasStyle(at([0.0, 0.0])),
        ],
    );
    let view = panel.create(
        Some(group),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView {
                axis: 1,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(placed([150.0, 100.0])),
        ],
    );
    let column = panel.node(
        view,
        GuiLayout {
            kind: 3,
            ..placed([150.0, 400.0])
        },
    );
    let anchor = trigger(&mut panel, column, [0.0, 40.0], [60.0, 20.0]);
    let list = popup(
        &mut panel,
        Some(anchor),
        overlay(0, 0),
        [50.0, 30.0],
        [0.0, 0.0],
    );
    panel.frame();
    panel.frame();
    let output = panel.output();
    assert_eq!(bounds(&output, anchor), [0.0, 40.0, 60.0, 60.0]);
    assert_eq!(bounds(&output, list), [0.0, 60.0, 50.0, 90.0]);
    // Raised, the list escapes the 100-unit viewport's clip.
    assert_eq!(control_hit(&output, list).clip, [0.0, 0.0, 300.0, 200.0]);

    // Scrolling moves the list with its trigger in the same publication.
    panel.act(view, GuiLocalAction::ScrollTo([0.0, 30.0]));
    panel.frame();
    let output = panel.output();
    assert_eq!(bounds(&output, anchor), [0.0, 10.0, 60.0, 30.0]);
    assert_eq!(bounds(&output, list), [0.0, 30.0, 50.0, 60.0]);

    // Translating an ancestor near the bottom edge flips the list above its
    // trigger in the frame that applies the write.
    set_style(&mut panel, group, offset_of!(CanvasStyle, y), 165.0);
    let output = panel.output();
    assert_eq!(bounds(&output, anchor), [0.0, 175.0, 60.0, 195.0]);
    assert_eq!(bounds(&output, list), [0.0, 145.0, 50.0, 175.0]);

    // An unchanged frame re-reads the inputs without laying out again.
    panel.frame();
    assert_eq!(panel.work().reflows, 0);
}

#[test]
fn a_top_level_overlay_is_placed_inside_the_canvas() {
    let mut panel = panel();
    // A toast stack against the bottom-right corner, 10 units in.
    let toast = popup(
        &mut panel,
        None,
        overlay(0, 2),
        [80.0, 40.0],
        [-10.0, -10.0],
    );
    // A dialog centred on the canvas.
    let dialog = popup(&mut panel, None, overlay(4, 1), [100.0, 60.0], [0.0, 0.0]);
    // A bar across the top.
    let bar = popup(&mut panel, None, overlay(1, 3), [10.0, 20.0], [0.0, 0.0]);
    // A context menu at a canvas point near the bottom-right corner shifts inside.
    let menu = popup(
        &mut panel,
        None,
        overlay(1, 0),
        [60.0, 50.0],
        [280.0, 190.0],
    );
    panel.frame();
    let output = panel.output();
    assert_eq!(bounds(&output, toast), [210.0, 150.0, 290.0, 190.0]);
    assert_eq!(bounds(&output, dialog), [100.0, 70.0, 200.0, 130.0]);
    assert_eq!(bounds(&output, bar), [0.0, 0.0, 300.0, 20.0]);
    assert_eq!(bounds(&output, menu), [240.0, 150.0, 300.0, 200.0]);
    let records = overlays(&panel);
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|record| record.parent.is_none()));
}

#[test]
fn an_overlay_fits_its_content_with_its_padding_round_it() {
    let mut panel = panel();
    let root = panel.root_entity;
    let anchor = trigger(&mut panel, root, [20.0, 20.0], [60.0, 20.0]);
    // A popover's column: a fixed width, no height, padded on every side.
    let column = panel.create(
        Some(anchor),
        vec![
            ComponentValue::GuiOverlay(overlay(0, 0)),
            ComponentValue::GuiLayout(GuiLayout {
                kind: 2,
                width: 100.0,
                padding_top: 4.0,
                padding_right: 6.0,
                padding_bottom: 8.0,
                padding_left: 10.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                layer: 1,
                ..Default::default()
            }),
        ],
    );
    let rows: Vec<_> = (0..2)
        .map(|_| {
            panel.button(
                column,
                GuiLayout {
                    kind: 3,
                    height: 30.0,
                    ..Default::default()
                },
            )
        })
        .collect();
    // A content-fitted stack in it keeps its padding too.
    let stack = panel.node(
        column,
        GuiLayout {
            kind: 3,
            padding_top: 5.0,
            padding_bottom: 5.0,
            ..Default::default()
        },
    );
    panel.button(
        stack,
        GuiLayout {
            height: 10.0,
            ..Default::default()
        },
    );
    panel.frame();
    // Its height is the rows' 60, the stack's 20 and its own 12 of padding.
    assert_eq!(panel.layout(column).size, [100.0, 92.0]);
    assert_eq!(panel.layout(stack).size, [84.0, 20.0]);
    // Placed below the anchor at (20, 40), its rows inside the padding.
    let output = panel.output();
    assert_eq!(bounds(&output, rows[0]), [30.0, 44.0, 114.0, 74.0]);
    assert_eq!(bounds(&output, rows[1]), [30.0, 74.0, 114.0, 104.0]);
}

#[test]
fn an_overlay_of_a_parent_without_a_box_opens_at_its_point() {
    let mut panel = panel();
    // A context menu's anchor: a top-level entity translated to a canvas
    // point, with no layout of its own.
    let point = panel.create(None, vec![ComponentValue::CanvasStyle(at([40.0, 50.0]))]);
    let menu = popup(
        &mut panel,
        Some(point),
        overlay(0, 0),
        [60.0, 50.0],
        [0.0, 0.0],
    );
    panel.frame();
    assert_eq!(bounds(&panel.output(), menu), [40.0, 50.0, 100.0, 100.0]);

    // Near the bottom-right corner it flips above the point and shifts left.
    set_style(&mut panel, point, offset_of!(CanvasStyle, x), 250.0);
    set_style(&mut panel, point, offset_of!(CanvasStyle, y), 180.0);
    panel.frame();
    assert_eq!(bounds(&panel.output(), menu), [240.0, 130.0, 300.0, 180.0]);
}

#[test]
fn an_overlay_opened_from_an_overlay_stacks_above_it() {
    let mut panel = panel();
    let root = panel.root_entity;
    let anchor = trigger(&mut panel, root, [20.0, 20.0], [60.0, 20.0]);
    let menu = panel.create(
        Some(anchor),
        vec![
            ComponentValue::GuiOverlay(overlay(0, 0)),
            ComponentValue::GuiLayout(GuiLayout {
                kind: 2,
                width: 120.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                layer: 1,
                ..Default::default()
            }),
            ComponentValue::CanvasBox(CanvasBox::default()),
        ],
    );
    let first = panel.button(menu, placed([120.0, 30.0]));
    let item = panel.button(menu, placed([120.0, 30.0]));
    let submenu = popup(
        &mut panel,
        Some(item),
        overlay(2, 0),
        [80.0, 60.0],
        [-40.0, 0.0],
    );
    // A later top-level sibling, lower in the tree but still on the base layer.
    let later = trigger(&mut panel, root, [0.0, 0.0], [300.0, 200.0]);
    panel.frame();
    let output = panel.output();
    assert_eq!(bounds(&output, item), [20.0, 70.0, 140.0, 100.0]);
    // Beside its item, overlapping the menu by its translation.
    assert_eq!(bounds(&output, submenu), [100.0, 70.0, 180.0, 130.0]);
    assert_eq!(control_hit(&output, first).layer, 1);
    assert_eq!(control_hit(&output, submenu).layer, 2);
    assert_eq!(control_hit(&output, later).layer, 0);
    // The submenu takes the press where it covers the menu; the menu takes
    // presses on the full-canvas button beneath it.
    assert_eq!(hit_at(&output, [120.0, 80.0]), Some(submenu));
    assert_eq!(hit_at(&output, [40.0, 50.0]), Some(first));
    assert_eq!(hit_at(&output, [250.0, 20.0]), Some(later));

    let records = overlays(&panel);
    assert_eq!(
        records
            .iter()
            .map(|record| (record.target.entity, record.parent, record.layer))
            .collect::<Vec<_>>(),
        [(menu, Some(anchor), 1), (submenu, Some(item), 2)]
    );
    assert_eq!(records[0].target.component, ComponentValue::GUI_OVERLAY);
    assert_eq!(records[1].bounds, [100.0, 70.0, 180.0, 130.0]);
    assert!(records[0].contains(&control_hit(&output, submenu).ancestry));
    assert!(!records[1].contains(&control_hit(&output, first).ancestry));
}

#[test]
fn a_closed_overlay_costs_nothing_and_opens_in_the_frame_that_shows_it() {
    let build = |with_overlay: bool| {
        let mut panel = panel();
        let root = panel.root_entity;
        let anchor = trigger(&mut panel, root, [20.0, 20.0], [60.0, 20.0]);
        let later = trigger(&mut panel, root, [20.0, 60.0], [60.0, 20.0]);
        let list = with_overlay.then(|| {
            let list = popup(
                &mut panel,
                Some(anchor),
                overlay(0, 0),
                [60.0, 90.0],
                [0.0, 0.0],
            );
            let row = panel.create(
                Some(list),
                vec![
                    ComponentValue::GuiButton(GuiButton::default()),
                    ComponentValue::GuiLayout(placed([60.0, 30.0])),
                    ComponentValue::CanvasBox(CanvasBox::default()),
                ],
            );
            set_visible(&mut panel, list, false);
            (list, row)
        });
        panel.frame();
        (panel, later, list)
    };
    let (bare, later, _) = build(false);
    let (mut panel, _, list) = build(true);
    let (list, row) = list.unwrap();

    // Closed, the overlay adds no paint, hit, layer, control or record.
    let closed = panel.output();
    let flat = bare.output();
    assert!(!appears(&closed, list) && !appears(&closed, row));
    assert_eq!(
        closed
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0].map(f64::from)
    );
    assert_eq!(closed.hits, flat.hits);
    assert_eq!(closed.entries.len(), flat.entries.len());
    assert!(overlays(&panel).is_empty());
    assert_eq!(hit_at(&closed, [30.0, 65.0]), Some(later));
    assert!(
        control_record(&panel.host, panel.world, row).is_none(),
        "a closed overlay's controls are not published"
    );

    // Opening it lays it out, raises it and publishes it in the same frame.
    set_visible(&mut panel, list, true);
    let open = panel.output();
    assert_eq!(
        open.layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0, 1].map(f64::from)
    );
    assert_eq!(bounds(&open, list), [20.0, 40.0, 80.0, 130.0]);
    assert_eq!(hit_at(&open, [30.0, 65.0]), Some(row));
    assert_eq!(overlays(&panel).len(), 1);

    set_visible(&mut panel, list, false);
    let closed = panel.output();
    assert_eq!(
        closed
            .layers
            .iter()
            .map(|plane| plane.offset)
            .collect::<Vec<_>>(),
        [0].map(f64::from)
    );
    assert_eq!(closed.hits, flat.hits);
    assert!(overlays(&panel).is_empty());
}

#[test]
fn overlays_raise_without_a_style_offset_and_validate_mode_and_band() {
    let mut panel = panel();
    let root = panel.root_entity;
    let anchor = trigger(&mut panel, root, [20.0, 20.0], [60.0, 20.0]);
    let list = popup(
        &mut panel,
        Some(anchor),
        overlay(0, 0),
        [40.0, 30.0],
        [0.0, 0.0],
    );
    let outcome = panel.set(
        list,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, layer),
        FieldValue::U32(0),
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
    assert!(appears(&panel.output(), list));
    assert!(diagnostics(&mut panel).is_empty());
    for (band, accepted) in [(0, false), (1, true), (2, true), (3, true), (4, false)] {
        let outcome = panel.set(
            list,
            ComponentValue::GUI_OVERLAY,
            offset_of!(GuiOverlay, band),
            FieldValue::U32(band),
        );
        assert_eq!(outcome.result.is_ok(), accepted, "{band}: {outcome:?}");
    }

    // The four modes are manual, light, modal and hint; others are refused.
    for (mode, accepted) in [(1, true), (3, true), (4, false)] {
        let outcome = panel.set(
            list,
            ComponentValue::GUI_OVERLAY,
            offset_of!(GuiOverlay, mode),
            FieldValue::U32(mode),
        );
        assert_eq!(outcome.result.is_ok(), accepted, "{mode}: {outcome:?}");
    }
}

#[test]
fn a_canvas_without_overlays_publishes_no_overlay_records() {
    let mut panel = panel();
    let root = panel.root_entity;
    trigger(&mut panel, root, [0.0, 0.0], [10.0, 10.0]);
    panel.frame();
    assert!(overlays(&panel).is_empty());
    assert!(diagnostics(&mut panel).is_empty());
    panel.frame();
    assert_eq!(panel.work().reflows, 0);
}

#[test]
fn semantic_bands_order_complete_scopes_above_arbitrary_content_levels() {
    let mut panel = panel();
    let root = panel.root_entity;
    let first = popup(
        &mut panel,
        Some(root),
        GuiOverlay {
            band: GuiOverlay::BAND_DIALOG,
            ..overlay(4, 0)
        },
        [100.0, 100.0],
        [0.0, 0.0],
    );
    let raised = trigger(&mut panel, first, [0.0, 0.0], [30.0, 30.0]);
    panel.set(
        raised,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, layer),
        FieldValue::U32(u32::MAX),
    );
    let nested = popup(
        &mut panel,
        Some(first),
        overlay(4, 0),
        [40.0, 40.0],
        [0.0, 0.0],
    );
    let nested_high = trigger(&mut panel, nested, [0.0, 0.0], [20.0, 20.0]);
    panel.set(
        nested_high,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, layer),
        FieldValue::U32(u32::MAX),
    );
    let later = popup(
        &mut panel,
        Some(root),
        GuiOverlay {
            band: GuiOverlay::BAND_DIALOG,
            ..overlay(4, 0)
        },
        [100.0, 100.0],
        [0.0, 0.0],
    );
    let notification = popup(
        &mut panel,
        Some(first),
        GuiOverlay {
            band: GuiOverlay::BAND_NOTIFICATION,
            ..overlay(4, 0)
        },
        [40.0, 40.0],
        [0.0, 0.0],
    );
    let content = trigger(&mut panel, root, [0.0, 0.0], [100.0, 100.0]);
    panel.set(
        content,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, layer),
        FieldValue::U32(u32::MAX),
    );
    let popup_scope = popup(
        &mut panel,
        Some(root),
        overlay(4, 0),
        [30.0, 30.0],
        [0.0, 0.0],
    );
    panel.frame();
    let output = panel.output();
    let ordered = [
        content,
        popup_scope,
        first,
        raised,
        nested,
        nested_high,
        later,
        notification,
    ]
    .map(|entity| control_hit(&output, entity).layer);
    assert!(
        ordered.windows(2).all(|pair| pair[0] < pair[1]),
        "{ordered:?}"
    );
    assert_eq!(ordered, [1, 2, 3, 4, 5, 6, 7, 8]);
    // All skin decorations stay with their component root.
    for entry in output.entries.iter() {
        if let CanvasPaintEntry::Primitive {
            primitive,
            ..
        } = entry.as_ref()
        {
            let style = primitive.style();
            if style.identity.target.entity == raised {
                assert_eq!(style.layer, 4);
            }
        }
    }
    // Band edits rebuild grouping, including descendant inheritance.
    panel.set(
        later,
        ComponentValue::GUI_OVERLAY,
        offset_of!(GuiOverlay, band),
        FieldValue::U32(GuiOverlay::BAND_POPUP),
    );
    let changed = panel.output();
    assert!(control_hit(&changed, later).layer < control_hit(&changed, first).layer);
    assert!(control_hit(&changed, notification).layer > control_hit(&changed, nested_high).layer);
}

#[test]
fn overlay_clip_escape_is_explicit_and_keeps_its_own_descendant_clips() {
    let mut panel = panel();
    let root = panel.root_entity;
    let parent = trigger(&mut panel, root, [10.0, 10.0], [60.0, 20.0]);
    panel.set(
        parent,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, clipped),
        FieldValue::Bool(true),
    );
    panel.set(
        parent,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, clip_max_x),
        FieldValue::F32(20.0),
    );
    panel.set(
        parent,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, clip_max_y),
        FieldValue::F32(20.0),
    );
    let overlay = popup(
        &mut panel,
        Some(parent),
        overlay(0, 0),
        [100.0, 50.0],
        [0.0, 0.0],
    );
    panel.set(
        overlay,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, layer),
        FieldValue::U32(0),
    );
    panel.frame();
    let output = panel.output();
    assert_eq!(control_hit(&output, parent).clip, [10.0, 10.0, 30.0, 30.0]);
    assert_eq!(control_hit(&output, overlay).clip, [0.0, 0.0, 300.0, 200.0]);
    assert_eq!(hit_at(&output, [80.0, 40.0]), Some(overlay));
    panel.set(
        overlay,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, clipped),
        FieldValue::Bool(true),
    );
    panel.set(
        overlay,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, clip_max_x),
        FieldValue::F32(40.0),
    );
    panel.set(
        overlay,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, clip_max_y),
        FieldValue::F32(20.0),
    );
    let child = trigger(&mut panel, overlay, [0.0, 0.0], [100.0, 50.0]);
    panel.frame();
    let output = panel.output();
    assert_eq!(control_hit(&output, child).clip, [10.0, 30.0, 50.0, 50.0]);
}
