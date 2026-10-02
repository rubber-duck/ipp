//! A control focus moves to scrolls into view in the frame that moves focus:
//! every ScrollView and VirtualList containing it, innermost first, by the
//! least distance that shows its box, through real headless Host frames.
//! Expected offsets are computed from the authored geometry.

mod support;

use ipp_core::components::{
    CanvasStyle, GuiButton, GuiLayout, GuiScrollView, GuiVirtualItem, GuiVirtualList,
};
use ipp_core::systems::gui::local::GuiLocalAction;
use ipp_core::*;
use support::gui_panel::*;

/// Column layout kind.
const COLUMN: u32 = 2;

fn sized(width: f32, height: f32) -> GuiLayout {
    GuiLayout {
        width,
        height,
        ..Default::default()
    }
}

fn column(width: f32, height: f32) -> GuiLayout {
    GuiLayout {
        kind: COLUMN,
        ..sized(width, height)
    }
}

/// A 200 x 100 panel whose root is a column.
fn panel() -> GuiPanel {
    GuiPanel::with_canvas(
        CanvasState {
            extent: [200.0, 100.0],
            units_per_metre: 100.0,
        },
        Some(column(200.0, 100.0)),
    )
}

/// A vertical ScrollView of `size` under `parent` over a column of `rows`
/// buttons, each 100 wide and `row` tall; returns the view and the buttons.
fn scrolled_rows(
    panel: &mut GuiPanel,
    parent: EntityId,
    size: [f32; 2],
    rows: usize,
    row: f32,
) -> (EntityId, Vec<EntityId>) {
    let view = panel.create(
        Some(parent),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiLayout(sized(size[0], size[1])),
        ],
    );
    let content = panel.node(view, column(100.0, rows as f32 * row));
    let buttons = (0..rows)
        .map(|_| panel.button(content, sized(100.0, row)))
        .collect();
    (view, buttons)
}

/// Focus `entity` with a `GuiAction`, apply it in one frame and return the
/// offset of `view` that frame wrote.
fn focus(panel: &mut GuiPanel, entity: EntityId, view: EntityId) -> [f32; 2] {
    panel.act(entity, GuiLocalAction::Focus(0));
    panel.frame();
    assert!(panel.snapshot(entity).focused);
    panel.scroll(view).offset
}

/// The control's published hit bounds, `[min_x, min_y, max_x, max_y]`.
fn shown(panel: &GuiPanel, entity: EntityId) -> [f32; 4] {
    bounds(&panel.output(), entity)
}

#[test]
fn focus_scrolls_a_control_below_above_or_partly_outside_by_the_least_distance() {
    let mut panel = panel();
    let root = panel.root_entity;
    // A 50-tall viewport over ten 20-tall rows: content 200, capacity 150.
    let (view, rows) = scrolled_rows(&mut panel, root, [100.0, 50.0], 10, 20.0);
    panel.frame();
    assert_eq!(panel.scroll(view).capacity, [0.0, 150.0]);

    // Row 4 (80..100) lies below the viewport (0..50): its bottom edge
    // meets the viewport's, in the same frame as the focus.
    assert_eq!(focus(&mut panel, rows[4], view), [0.0, 50.0]);
    assert_near(&shown(&panel, rows[4]), &rect(0.0, 30.0, 100.0, 20.0));

    // Row 1 (20..40) lies above it (50..100): its top edge meets the top.
    assert_eq!(focus(&mut panel, rows[1], view), [0.0, 20.0]);
    assert_near(&shown(&panel, rows[1]), &rect(0.0, 0.0, 100.0, 20.0));

    // Row 3 (60..80) is cut by the bottom edge (20..70): it moves 10.
    assert_eq!(focus(&mut panel, rows[3], view), [0.0, 30.0]);

    // Row 2 (40..60) is already shown (30..80): nothing moves.
    assert_eq!(focus(&mut panel, rows[2], view), [0.0, 30.0]);
}

#[test]
fn reveal_happens_once_per_focus_move_and_never_overrides_later_scrolling() {
    let mut panel = panel();
    let root = panel.root_entity;
    let (view, rows) = scrolled_rows(&mut panel, root, [100.0, 50.0], 10, 20.0);
    panel.frame();
    assert_eq!(focus(&mut panel, rows[9], view), [0.0, 150.0]);

    // Scrolling away from the focused control keeps the scrolled position,
    // and focusing the control it already holds reveals nothing.
    panel.act(view, GuiLocalAction::ScrollTo([0.0, 0.0]));
    panel.frame();
    panel.frame();
    assert_eq!(panel.scroll(view).offset, [0.0, 0.0]);
    assert_eq!(focus(&mut panel, rows[9], view), [0.0, 0.0]);

    // Blurring and focusing it again is a new focus move.
    panel.act(rows[9], GuiLocalAction::Blur);
    panel.frame();
    assert_eq!(focus(&mut panel, rows[9], view), [0.0, 150.0]);
}

#[test]
fn nested_scroll_views_reveal_innermost_first_and_the_outer_one_shows_what_the_inner_one_shows() {
    let mut panel = panel();
    let root = panel.root_entity;
    // An outer 100 x 50 view over a column holding a 60-tall spacer and an
    // inner 100 x 40 view over five 20-tall rows.
    let outer = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiLayout(sized(100.0, 50.0)),
        ],
    );
    let content = panel.node(outer, column(100.0, 100.0));
    panel.node(content, sized(100.0, 60.0));
    let (inner, rows) = scrolled_rows(&mut panel, content, [100.0, 40.0], 5, 20.0);
    panel.frame();

    // Row 4 sits at 80..100 in the inner content: the inner view moves 60,
    // which shows it at 20..40 of its viewport, 80..100 in the outer
    // content; the outer view then moves 50.
    panel.act(rows[4], GuiLocalAction::Focus(0));
    panel.frame();
    assert_eq!(panel.scroll(inner).offset, [0.0, 60.0]);
    assert_eq!(panel.scroll(outer).offset, [0.0, 50.0]);
    assert_near(&shown(&panel, rows[4]), &rect(0.0, 30.0, 100.0, 20.0));

    // Row 0 sits above the inner viewport: the inner view moves back to 0,
    // which shows it at 60..80 in the outer content, inside the outer
    // viewport (50..100), so the outer view stays.
    panel.act(rows[0], GuiLocalAction::Focus(0));
    panel.frame();
    assert_eq!(panel.scroll(inner).offset, [0.0, 0.0]);
    assert_eq!(panel.scroll(outer).offset, [0.0, 50.0]);
    assert_near(&shown(&panel, rows[0]), &rect(0.0, 10.0, 100.0, 20.0));
}

#[test]
fn the_box_follows_visual_translation_and_scale_between_the_control_and_its_view() {
    let mut panel = panel();
    let root = panel.root_entity;
    let (view, rows) = scrolled_rows(&mut panel, root, [100.0, 50.0], 10, 20.0);
    // Row 1 (20..40) draws 30 lower at twice its height: 50..90.
    panel.apply(vec![Command::insert_value(
        EntityRef::Handle(rows[1]),
        ComponentValue::CanvasStyle(CanvasStyle {
            y: 30.0,
            scale_y: 2.0,
            ..Default::default()
        }),
    )]);
    panel.frame();
    assert_eq!(focus(&mut panel, rows[1], view), [0.0, 40.0]);
}

/// A vertical VirtualList of 100 ten-tall items in a 100 x 50 viewport with
/// one button item declared at each of `indices`.
fn list(panel: &mut GuiPanel, indices: &[u32]) -> (EntityId, Vec<EntityId>) {
    let root = panel.root_entity;
    let list = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiVirtualList(GuiVirtualList {
                item_count: 100,
                item_extent: 10.0,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(sized(100.0, 50.0)),
        ],
    );
    let items = indices
        .iter()
        .map(|&index| {
            panel.create(
                Some(list),
                vec![
                    ComponentValue::GuiButton(GuiButton::default()),
                    ComponentValue::GuiVirtualItem(GuiVirtualItem {
                        index,
                    }),
                    ComponentValue::GuiLayout(sized(100.0, 10.0)),
                ],
            )
        })
        .collect();
    panel.frame();
    (list, items)
}

#[test]
fn a_virtual_list_reveals_its_item_and_anchors_at_the_revealed_offset() {
    let mut panel = panel();
    let (list, items) = list(&mut panel, &[0, 1, 40]);

    // Item 40 spans 400..410: the list moves 360 and anchors item 36.
    assert_eq!(focus(&mut panel, items[2], list), [0.0, 360.0]);
    let fields = panel.scroll(list);
    assert_eq!((fields.anchor_index, fields.anchor_offset), (36, 0.0));
    assert_eq!((fields.first, fields.last), (36, 41));
    assert_near(&shown(&panel, items[2]), &rect(0.0, 40.0, 100.0, 10.0));

    // The anchored position holds on later frames.
    panel.frame();
    assert_eq!(panel.scroll(list).offset, [0.0, 360.0]);

    // Item 1 lies above: the list moves back to show it at the top.
    assert_eq!(focus(&mut panel, items[1], list), [0.0, 10.0]);
    assert_eq!(panel.scroll(list).anchor_index, 1);
}

#[test]
fn an_item_the_list_does_not_lay_out_is_revealed_through_its_index() {
    let mut panel = panel();
    // The second item repeating index 70 is not laid out, so its button has
    // no box; its index places it where scroll-to-index would.
    let (list, items) = list(&mut panel, &[70, 70]);
    assert!(!panel.layout(items[1]).available);
    assert_eq!(focus(&mut panel, items[1], list), [0.0, 660.0]);
    assert_eq!(panel.scroll(list).anchor_index, 66);
}
