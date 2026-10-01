//! Ordinary ScrollView and VirtualList geometry through real headless Host frames:
//! scroll bars against enclosing tracks, painter order, VirtualList extents and
//! clips, scrolled paint of every leaf kind, and layout-sourced scroll changes.
#![cfg(feature = "gui")]

mod support;

use ipp_core::components::rows::Rows;
use ipp_core::components::{GuiLayout, GuiScrollView, GuiVirtualItem, GuiVirtualList};
use ipp_core::services::asset_management::{AssetSource, font::FONT_TYPE};
use ipp_core::systems::canvas::{
    CanvasAxis, CanvasBox, CanvasDrawing, CanvasGlyphRow, CanvasGlyphRun, CanvasHitKind,
    CanvasPaintEntry, CanvasPart, CanvasPublication, CanvasStyle, CanvasText,
};
use ipp_core::systems::gui::local::GuiLocalAction;
use ipp_core::*;
use std::mem::offset_of;
use std::sync::Arc;
use support::gui_panel::*;

fn sized(width: f32, height: f32) -> GuiLayout {
    GuiLayout {
        width,
        height,
        ..Default::default()
    }
}

fn container(kind: u32) -> GuiLayout {
    GuiLayout {
        kind,
        ..Default::default()
    }
}

fn panel(width: f32, height: f32, root: GuiLayout) -> GuiPanel {
    GuiPanel::with_canvas(
        CanvasState {
            extent: [width, height],
            units_per_metre: 100.0,
        },
        Some(root),
    )
}

fn scroll_view(panel: &mut GuiPanel, parent: EntityId, axis: u32, layout: GuiLayout) -> EntityId {
    panel.create(
        Some(parent),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView {
                axis,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(layout),
        ],
    )
}

fn scroll_hit(view: &CanvasPublication, entity: EntityId, kind: CanvasHitKind) -> Option<[f32; 4]> {
    view.hits
        .iter()
        .find(|hit| hit.target.entity == entity && hit.kind == kind)
        .map(|hit| hit.bounds)
}

fn track(view: &CanvasPublication, entity: EntityId, axis: CanvasAxis) -> [f32; 4] {
    scroll_hit(
        view,
        entity,
        CanvasHitKind::ScrollTrack {
            axis,
        },
    )
    .unwrap_or_else(|| panic!("no {axis:?} track for {entity:?}"))
}

fn position(panel: &mut GuiPanel, entity: EntityId) -> ScrollFields {
    panel.scroll(entity)
}

fn scroll_to(panel: &mut GuiPanel, entity: EntityId, offset: [f32; 2]) {
    panel.act(entity, GuiLocalAction::ScrollTo(offset));
    panel.frame();
    panel.frame();
    assert_eq!(position(panel, entity).offset, offset);
}

/// Outer 400 x 300 ScrollView over an explicit `content` column that holds, below
/// `spacer`, an inner ScrollView of `inner` size over 400 x 300 content.
fn nested(
    outer_axis: u32,
    content: [f32; 2],
    spacer: f32,
    inner_axis: u32,
    inner: [f32; 2],
) -> (GuiPanel, EntityId, EntityId) {
    let mut panel = panel(400.0, 300.0, container(3));
    let root = panel.root_entity;
    let outer = scroll_view(&mut panel, root, outer_axis, sized(400.0, 300.0));
    let column = panel.node(
        outer,
        GuiLayout {
            kind: 2,
            ..sized(content[0], content[1])
        },
    );
    panel.node(column, sized(400.0, spacer));
    let inner_view = scroll_view(&mut panel, column, inner_axis, sized(inner[0], inner[1]));
    panel.button(inner_view, sized(400.0, 300.0));
    panel.frame();
    panel.frame();
    (panel, outer, inner_view)
}

#[test]
fn nested_vertical_bars_sharing_an_edge_sit_side_by_side_at_any_outer_offset() {
    let (mut panel, outer, inner) = nested(1, [400.0, 500.0], 0.0, 1, [400.0, 200.0]);
    let view = panel.output();
    // Thickness is 5% of each viewport's shorter side.
    assert_near(
        &track(&view, outer, CanvasAxis::Vertical),
        &rect(385.0, 0.0, 15.0, 300.0),
    );
    // The inner 10-thick track ends at the outer track's inner edge instead of
    // lying under it.
    assert_near(
        &track(&view, inner, CanvasAxis::Vertical),
        &rect(375.0, 0.0, 10.0, 200.0),
    );
    assert!(
        scroll_hit(
            &view,
            inner,
            CanvasHitKind::ScrollTrack {
                axis: CanvasAxis::Horizontal
            }
        )
        .is_none()
    );

    // A vertical outer scroll moves the inner viewport, never the outer bars.
    scroll_to(&mut panel, outer, [0.0, 100.0]);
    let scrolled = panel.output();
    assert_near(
        &track(&scrolled, inner, CanvasAxis::Vertical),
        &rect(375.0, -100.0, 10.0, 200.0),
    );
    assert_near(
        &track(&scrolled, outer, CanvasAxis::Vertical),
        &rect(385.0, 0.0, 15.0, 300.0),
    );
}

#[test]
fn nested_bars_that_do_not_meet_keep_their_own_edges() {
    // A narrower inner viewport that also overflows horizontally leaves both
    // inner tracks clear of the outer one around their own corner.
    let (panel, _, inner) = nested(1, [400.0, 500.0], 0.0, 2, [300.0, 200.0]);
    let view = panel.output();
    assert_near(
        &track(&view, inner, CanvasAxis::Vertical),
        &rect(290.0, 0.0, 10.0, 190.0),
    );
    assert_near(
        &track(&view, inner, CanvasAxis::Horizontal),
        &rect(0.0, 190.0, 290.0, 10.0),
    );

    // A horizontal outer offset moves the inner viewport left of the outer track,
    // so the inner track stays at its viewport edge; unscrolled, it moves aside.
    let (mut panel, outer, inner) = nested(2, [500.0, 500.0], 0.0, 1, [400.0, 200.0]);
    assert_near(
        &track(&panel.output(), inner, CanvasAxis::Vertical),
        &rect(375.0, 0.0, 10.0, 200.0),
    );
    scroll_to(&mut panel, outer, [100.0, 0.0]);
    assert_near(
        &track(&panel.output(), inner, CanvasAxis::Vertical),
        &rect(290.0, 0.0, 10.0, 200.0),
    );
}

#[test]
fn nested_tracks_end_before_a_crossing_outer_track() {
    // The outer view scrolls only horizontally: its 15-thick track runs along
    // the bottom at y 285..300, where the inner vertical track ends.
    let (panel, outer, inner) = nested(0, [600.0, 300.0], 100.0, 1, [400.0, 200.0]);
    let view = panel.output();
    assert!(
        scroll_hit(
            &view,
            outer,
            CanvasHitKind::ScrollTrack {
                axis: CanvasAxis::Vertical
            }
        )
        .is_none()
    );
    assert_near(
        &track(&view, outer, CanvasAxis::Horizontal),
        &rect(0.0, 285.0, 400.0, 15.0),
    );
    assert_near(
        &track(&view, inner, CanvasAxis::Vertical),
        &rect(390.0, 100.0, 10.0, 185.0),
    );
}

#[test]
fn scroll_bars_paint_above_the_scrolled_subtree_and_below_later_siblings() {
    let mut panel = panel(400.0, 300.0, container(2));
    let root = panel.root_entity;
    let scroll = scroll_view(&mut panel, root, 1, sized(400.0, 200.0));
    let content = panel.button(scroll, sized(400.0, 300.0));
    let later = panel.button(root, sized(400.0, 50.0));
    panel.frame();
    scroll_to(&mut panel, scroll, [0.0, 100.0]);
    let view = panel.output();

    let index = |entity: EntityId, part: CanvasPart| {
        view.entries
            .iter()
            .position(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    primitive,
                    ..
                } => {
                    primitive.style().identity.target.entity == entity
                        && primitive.style().identity.part == part
                }
                CanvasPaintEntry::Attachment(_) => false,
            })
            .unwrap_or_else(|| panic!("{entity:?} {part:?} not painted"))
    };
    let (track, thumb) = (
        index(scroll, CanvasPart::ScrollTrackY),
        index(scroll, CanvasPart::ScrollThumbY),
    );
    let content_parts: Vec<_> = view
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| primitive(entry).style().identity.target.entity == content)
        .map(|(position, _)| position)
        .collect();
    assert!(!content_parts.is_empty());
    assert!(content_parts.iter().all(|&position| position < track));
    assert!(track < thumb);
    assert!(thumb < index(later, CanvasPart::Background));
    assert!(index(scroll, CanvasPart::Background) < content_parts[0]);

    // The thumb shows two thirds of the 200-long track and sits at the end of
    // its travel for the full 100 offset.
    let thumb_hit = scroll_hit(
        &view,
        scroll,
        CanvasHitKind::ScrollThumb {
            axis: CanvasAxis::Vertical,
        },
    )
    .unwrap();
    assert_near(&thumb_hit, &[390.0, 200.0 / 3.0, 400.0, 200.0]);
    let bar_order = view
        .hits
        .iter()
        .find(|hit| {
            hit.target.entity == scroll
                && hit.kind
                    == CanvasHitKind::ScrollThumb {
                        axis: CanvasAxis::Vertical,
                    }
        })
        .unwrap()
        .paint_order;
    assert!(bar_order > control_hit(&view, content).paint_order);
    assert!(bar_order < control_hit(&view, later).paint_order);
    assert_eq!(hit_at(&view, [395.0, 150.0]), Some(scroll));
    assert_eq!(hit_at(&view, [200.0, 150.0]), Some(content));
}

#[test]
fn virtual_list_extent_follows_its_count_with_a_minimum_thumb_and_clipped_realized_items() {
    const COUNT: u32 = 100_000;
    let mut panel = panel(1000.0, 600.0, container(3));
    let root = panel.root_entity;
    let list = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiVirtualList(GuiVirtualList {
                item_count: COUNT,
                item_extent: 100.0,
                overscan: 2,
                axis: 1,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(sized(1000.0, 600.0)),
        ],
    );
    panel.frame();
    panel.frame();
    let geometry = panel.scroll(list);
    assert_eq!(geometry.content, [1000.0, COUNT as f32 * 100.0]);
    assert_eq!((geometry.first, geometry.last), (0, 8));

    // The 1000 x 600 viewport shows a vertical bar 30 wide at x 970..1000; over
    // 100000 items its thumb keeps the two-thickness minimum of 60.
    let view = panel.output();
    assert_near(
        &track(&view, list, CanvasAxis::Vertical),
        &rect(970.0, 0.0, 30.0, 600.0),
    );
    assert_near(
        &scroll_hit(
            &view,
            list,
            CanvasHitKind::ScrollThumb {
                axis: CanvasAxis::Vertical,
            },
        )
        .unwrap(),
        &rect(970.0, 0.0, 30.0, 60.0),
    );

    // Declaring items twice the estimate, two of them above the viewport,
    // keeps item 10 where it was: the offset follows by +200.
    panel.act(
        list,
        GuiLocalAction::ScrollToIndex {
            index: 10,
            offset: 50.0,
        },
    );
    panel.frame();
    assert_eq!(position(&mut panel, list).offset, [0.0, 1050.0]);
    let items: Vec<_> = (8..19)
        .map(|index| {
            panel.create(
                Some(list),
                vec![
                    ComponentValue::GuiButton(Default::default()),
                    ComponentValue::GuiVirtualItem(GuiVirtualItem {
                        index,
                    }),
                    ComponentValue::GuiLayout(sized(1000.0, 200.0)),
                ],
            )
        })
        .collect();
    for _ in 0..3 {
        panel.frame();
    }
    let settled = position(&mut panel, list);
    assert_eq!(settled.offset, [0.0, 1250.0]);
    assert_eq!((settled.anchor_index, settled.anchor_offset), (10, 50.0));
    assert_eq!(
        panel.scroll(list).content,
        [1000.0, COUNT as f32 * 100.0 + 1100.0]
    );
    let view = panel.output();
    assert_near(&bounds(&view, items[2]), &rect(0.0, -50.0, 1000.0, 200.0));

    // Declared children outside the viewport stay clipped by it.
    for &item in &items {
        assert_near(
            &control_hit(&view, item).clip,
            &rect(0.0, 0.0, 1000.0, 600.0),
        );
    }
    let above = bounds(&view, items[0]);
    assert!(above[3] <= 0.0, "{above:?}");
    assert_eq!(hit_at(&view, [500.0, 10.0]), Some(items[2]));
    assert_eq!(hit_at(&view, [500.0, -100.0]), None);
}

const DRAWING_SOURCE: &str = "gui-scroll-drawing:///icon.ippd";

fn drawing_bytes() -> Vec<u8> {
    let mut bytes = b"IPPD".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    for value in [10.0_f32, 20.0, 42.0, 44.0, 10.0, 20.0, 42.0, 44.0, 0.05] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes
}

fn content_entry(view: &CanvasPublication, entity: EntityId) -> Arc<CanvasPaintEntry> {
    view.entries
        .iter()
        .find(|entry| primitive(entry).style().identity.target.entity == entity)
        .unwrap_or_else(|| panic!("{entity:?} is not painted"))
        .clone()
}

/// Scrolling translates every leaf kind under its unchanged viewport clip and leaves
/// paint outside the scrolled subtree untouched. Bitmaps share the leaf placement of
/// the kinds below; a headless Host without a renderer never readies texture data, so
/// their paint is covered by the renderer smoke scenarios instead. The legacy lane moved scrolled paint
/// without any layout pass; the ordinary lane settles each committed offset through
/// layout, so a scroll re-evaluates its Canvas scope once without text measurement.
#[test]
fn scrolling_moves_every_leaf_kind_under_fixed_viewport_clips() {
    let mut panel = panel(1000.0, 1000.0, container(2));
    panel
        .host
        .register_stream_resource_provider("gui-scroll-drawing")
        .unwrap();
    let world = panel.world;
    let font = AssetSource {
        kind: FONT_TYPE,
        uri: std::sync::Arc::<str>::from(format!("producer://{}/{}/41", world.0, FONT_TYPE.0)),
        variant: 0,
    };
    panel
        .host
        .asset_resources_mut()
        .register_client_source(world, font.clone(), support::canvas_font_bytes())
        .unwrap();
    let root = panel.root_entity;
    let outer = scroll_view(&mut panel, root, 1, sized(1000.0, 600.0));
    let outer_content = panel.node(outer, container(2));
    let inner = scroll_view(&mut panel, outer_content, 1, sized(1000.0, 400.0));
    let inner_content = panel.node(inner, container(2));
    let leaf = |value: ComponentValue, height: f32| {
        vec![
            value,
            ComponentValue::GuiLayout(sized(1000.0, height)),
            ComponentValue::CanvasStyle(CanvasStyle::default()),
        ]
    };
    let mut glyphs = Rows::new();
    glyphs
        .push(CanvasGlyphRow {
            glyph_id: 1,
            position: [0.0, 8.0],
            color: None,
        })
        .unwrap();
    let leaves = [
        leaf(
            ComponentValue::CanvasBox(CanvasBox {
                width: 1000.0,
                height: 400.0,
                ..Default::default()
            }),
            400.0,
        ),
        leaf(
            ComponentValue::CanvasText(CanvasText {
                text: "AA".into(),
                source: font.uri.clone(),
                variant: 0,
                font_size: 10.0,
            }),
            80.0,
        ),
        leaf(
            ComponentValue::CanvasGlyphRun(CanvasGlyphRun {
                source: font.uri.clone(),
                variant: 0,
                font_size: 10.0,
                glyphs,
            }),
            80.0,
        ),
        leaf(
            ComponentValue::CanvasDrawing(CanvasDrawing {
                source: DRAWING_SOURCE.into(),
                variant: 0,
            }),
            80.0,
        ),
    ]
    .map(|values| panel.create(Some(inner_content), values));
    let outer_sibling = panel.create(
        Some(outer_content),
        leaf(
            ComponentValue::CanvasBox(CanvasBox {
                width: 1000.0,
                height: 600.0,
                ..Default::default()
            }),
            600.0,
        ),
    );
    let fixed = panel.create(
        Some(root),
        leaf(ComponentValue::CanvasBox(CanvasBox::default()), 100.0),
    );
    for _ in 0..16 {
        for request in panel.host.take_resource_requests() {
            panel
                .host
                .complete_resource(request.id, Ok(drawing_bytes()))
                .unwrap();
        }
        panel.frame();
        let view = panel.output();
        if leaves.iter().all(|&leaf| {
            view.entries
                .iter()
                .any(|entry| primitive(entry).style().identity.target.entity == leaf)
        }) {
            break;
        }
    }
    let before = panel.output();
    // Inner content spans 400 + 3 x 80, so the inner view can scroll by 240.
    assert_eq!(panel.scroll(inner).capacity, [0.0, 240.0]);

    panel.act(inner, GuiLocalAction::ScrollTo([0.0, 240.0]));
    panel.frame();
    assert_eq!(position(&mut panel, inner).offset, [0.0, 240.0]);
    #[cfg(feature = "diagnostics")]
    {
        let work = panel.work();
        assert_eq!((work.reflows, work.text_measurements), (1, 0));
    }
    let after = panel.output();
    for &leaf in &leaves {
        let (old, new) = (content_entry(&before, leaf), content_entry(&after, leaf));
        let (old, new) = (primitive(&old).style(), primitive(&new).style());
        assert_eq!(new.position, [old.position[0], old.position[1] - 240.0]);
        assert_eq!(new.clip, old.clip);
        assert_near(&new.clip, &rect(0.0, 0.0, 1000.0, 400.0));
    }
    for untouched in [outer_sibling, fixed] {
        assert!(Arc::ptr_eq(
            &content_entry(&before, untouched),
            &content_entry(&after, untouched)
        ));
    }
    #[cfg(feature = "diagnostics")]
    {
        panel.frame();
        assert_eq!(panel.work().reflows, 0);
    }
}

/// Layout writes a ScrollView's geometry fields whenever it settles the view to new
/// geometry. Geometry-only changes therefore change the fields once per changed
/// frame, with or without an offset change, and the pass that writes them leaves
/// nothing for an extra layout pass.
#[test]
fn layout_sourced_scroll_changes_write_the_fields_once_per_geometry_change() {
    let mut panel = panel(400.0, 300.0, container(3));
    let root = panel.root_entity;
    let scroll = scroll_view(&mut panel, root, 1, sized(400.0, 200.0));
    let content = panel.button(scroll, sized(400.0, 400.0));
    for _ in 0..3 {
        panel.frame();
    }
    let mut fields = panel.scroll(scroll);
    let mut emissions = |panel: &mut GuiPanel, frames: usize| {
        let mut counts = Vec::new();
        #[allow(unused_mut)]
        let mut reflows: Vec<u64> = Vec::new();
        for _ in 0..frames {
            panel.frame();
            let current = panel.scroll(scroll);
            counts.push(u32::from(current != fields));
            fields = current;
            #[cfg(feature = "diagnostics")]
            reflows.push(panel.work().reflows);
        }
        (counts, reflows)
    };
    // The next measured frame applies the queued write and settles it.
    let set_height = |panel: &mut GuiPanel, entity: EntityId, height: f32| {
        panel.queue_set(
            entity,
            ComponentValue::GUI_LAYOUT,
            offset_of!(GuiLayout, height),
            FieldValue::F32(height),
        );
    };

    // Settled frames write nothing.
    assert_eq!(emissions(&mut panel, 3).0, vec![0, 0, 0]);

    // One content change writes once, in the frame whose single layout pass
    // settled it.
    set_height(&mut panel, content, 500.0);
    let (counts, _reflows) = emissions(&mut panel, 4);
    assert_eq!(counts, vec![1, 0, 0, 0]);
    #[cfg(feature = "diagnostics")]
    assert_eq!(_reflows, vec![1, 0, 0, 0]);
    assert_eq!(position(&mut panel, scroll).offset, [0.0, 0.0]);

    // Geometry that changes every frame writes on every one of those frames.
    let mut counts = Vec::new();
    for step in 1..=4 {
        set_height(&mut panel, content, 500.0 + 10.0 * step as f32);
        counts.extend(emissions(&mut panel, 1).0);
    }
    assert_eq!(counts, vec![1, 1, 1, 1]);
    assert_eq!(emissions(&mut panel, 2).0, vec![0, 0]);
    assert_eq!(position(&mut panel, scroll).offset, [0.0, 0.0]);

    // A viewport-only change writes once as well.
    set_height(&mut panel, scroll, 150.0);
    assert_eq!(emissions(&mut panel, 3).0, vec![1, 0, 0]);
    assert_eq!(position(&mut panel, scroll).offset, [0.0, 0.0]);

    // Shrinking content under a scrolled offset clamps it in the same single
    // write as the new geometry.
    panel.act(scroll, GuiLocalAction::ScrollTo([0.0, 300.0]));
    assert_eq!(emissions(&mut panel, 3).0.iter().sum::<u32>(), 1);
    assert_eq!(position(&mut panel, scroll).offset, [0.0, 300.0]);
    set_height(&mut panel, content, 250.0);
    assert_eq!(emissions(&mut panel, 3).0, vec![1, 0, 0]);
    assert_eq!(position(&mut panel, scroll).offset, [0.0, 100.0]);
}

#[test]
fn scroll_configuration_is_validated_at_its_bounds() {
    let mut panel = panel(400.0, 300.0, container(3));
    let root = panel.root_entity;
    let list = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiVirtualList(GuiVirtualList {
                item_count: 10,
                item_extent: 10.0,
                overscan: 0,
                axis: 1,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(sized(400.0, 300.0)),
        ],
    );
    let scroll = scroll_view(&mut panel, root, 1, sized(400.0, 300.0));
    let insert = |entity: EntityId, value: ComponentValue| {
        vec![Command::insert_value(EntityRef::Handle(entity), value)]
    };
    let list_with = |item_count: u32, item_extent: f32, axis: u32| {
        ComponentValue::GuiVirtualList(GuiVirtualList {
            item_count,
            item_extent,
            overscan: 2,
            axis,
            ..Default::default()
        })
    };

    for value in [
        list_with((1 << 24) + 1, 1.0, 1),
        list_with(10, 0.0, 1),
        list_with(10, -1.0, 1),
        list_with(10, f32::NAN, 1),
        list_with(10, f32::INFINITY, 1),
        list_with(10, 1.0, 2),
        list_with(1 << 24, f32::MAX, 1),
    ] {
        let outcome = panel.apply(insert(list, value.clone()));
        assert!(outcome.result.is_err(), "{value:?} accepted");
    }
    let outcome = panel.apply(insert(
        scroll,
        ComponentValue::GuiScrollView(GuiScrollView {
            axis: 3,
            ..Default::default()
        }),
    ));
    assert!(outcome.result.is_err());
    panel.frame();
    assert_eq!(panel.scroll(list).item_count, Some(10));

    for value in [list_with(1 << 24, 1.0, 0), list_with(0, 1.0, 1)] {
        let outcome = panel.apply(insert(list, value.clone()));
        assert!(outcome.result.is_ok(), "{value:?} rejected");
    }
    panel.frame();
    let geometry = panel.scroll(list);
    assert_eq!(geometry.item_count, Some(0));
    assert_eq!(geometry.capacity, [0.0, 0.0]);
}
