//! Scroll bar geometry, visibility and skinned paint over evaluated records.

use super::*;
use crate::DynamicValue;
use crate::EntityId;
use crate::services::asset_management::AssetSource;
use crate::systems::gui::test_support::author_part;
use crate::systems::gui::{GuiEvaluatedContent, GuiNodeId, GuiScrollBarCursor};
use crate::systems::gui::{GuiFontResolution, GuiResourceResolver};
use crate::systems::surface::SurfaceRenderResource;

/// Resolver without fonts or resources: bars need neither.
struct NoResources;

impl GuiResourceResolver for NoResources {
    fn text_font(&self, _source: &AssetSource) -> GuiFontResolution<'_> {
        GuiFontResolution::Missing
    }

    fn surface_resource(&self, _source: &AssetSource) -> Option<SurfaceRenderResource> {
        None
    }
}

/// ScrollView record with a `viewport` at the origin over `extents`.
fn scroll_view(node: u32, depth: u32, viewport: [f32; 2], extents: [f32; 2]) -> GuiEvaluatedNode {
    GuiEvaluatedNode {
        node: GuiNodeId(node),
        depth,
        rect: [0.0, 0.0, viewport[0], viewport[1]],
        clip: Some([0.0, 0.0, 10.0, 10.0]),
        content: GuiEvaluatedContent::Container,
        enabled: true,
        visible: true,
        available: true,
        paint_suppressed: false,
        visual_offset: [0.0, 0.0],
        visual_scale: [1.0, 1.0],
        acc_scale: [1.0, 1.0],
        content_extents: Some(extents),
        viewport: Some([0.0, 0.0, viewport[0], viewport[1]]),
        content_origin: [0.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
        background: None,
        opacity: 1.0,
    }
}

/// Plain child record inside the ScrollView.
fn content(node: u32, depth: u32) -> GuiEvaluatedNode {
    GuiEvaluatedNode {
        content_extents: None,
        viewport: None,
        background: Some([1.0, 0.0, 0.0, 1.0]),
        ..scroll_view(node, depth, [10.0, 10.0], [0.0, 0.0])
    }
}

fn view(nodes: Vec<GuiEvaluatedNode>) -> GuiEvaluatedView {
    GuiEvaluatedView {
        entity: EntityId::from_bits(0x51),
        root_incarnation: 1,
        layout_revision: 0,
        paint_revision: 0,
        evaluation_tick: 0,
        root_bounds: [0.0, 0.0, 10.0, 10.0],
        units_per_metre: 1.0,
        nodes,
        diagnostics: Vec::new(),
        remeasure_count: 0,
        reflow_count: 0,
        available: true,
    }
}

/// Bars of one lone ScrollView record at a committed offset.
fn scroll_bars(node: &GuiEvaluatedNode, offset: [f32; 2]) -> [Option<GuiScrollBar>; 2] {
    scroll_bars_in_view(&view(vec![node.clone()]), &GuiRoot::default(), 0, |_| {
        offset
    })
}

fn assert_rect(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual:?} != {expected:?}"
        );
    }
}

#[test]
fn vertical_thumb_follows_viewport_ratio_and_offset() {
    let node = scroll_view(1, 0, [10.0, 6.0], [10.0, 10.0]);
    assert_eq!(scroll_capacity(&node), [0.0, 4.0]);
    assert_eq!(scroll_bar_overflow(&node), [false, true]);

    // Thickness is 5% of the 6-unit side; the thumb shows 60% of the track.
    let [horizontal, vertical] = scroll_bars(&node, [0.0, 0.0]);
    assert!(horizontal.is_none());
    let vertical = vertical.unwrap();
    assert_rect(vertical.track, [9.7, 0.0, 0.3, 6.0]);
    assert_rect(vertical.thumb, [9.7, 0.0, 0.3, 3.6]);
    assert_eq!((vertical.capacity, vertical.page), (4.0, 6.0));

    // The thumb travels the remaining 2.4 units in proportion to the offset.
    let middle = scroll_bars(&node, [0.0, 2.0])[1].unwrap();
    assert_rect(middle.thumb, [9.7, 1.2, 0.3, 3.6]);
    let end = scroll_bars(&node, [0.0, 4.0])[1].unwrap();
    assert_rect(end.thumb, [9.7, 2.4, 0.3, 3.6]);

    // Dragging the thumb start maps back to offsets, clamped to the track.
    assert!((middle.offset_for_thumb(1.2) - 2.0).abs() < 1e-5);
    assert_eq!(middle.offset_for_thumb(-5.0), 0.0);
    assert_eq!(middle.offset_for_thumb(50.0), 4.0);

    // Shifting moves both rectangles with the ScrollView's outer scroll.
    let shifted = middle.shifted([0.0, -2.0]);
    assert_rect(shifted.track, [9.7, -2.0, 0.3, 6.0]);
    assert_rect(shifted.thumb, [9.7, -0.8, 0.3, 3.6]);
}

#[test]
fn both_axes_share_the_corner_and_thumbs_keep_a_minimum_length() {
    let node = scroll_view(1, 0, [10.0, 6.0], [20.0, 1000.0]);
    let [horizontal, vertical] = scroll_bars(&node, [0.0, 0.0]);
    let (horizontal, vertical) = (horizontal.unwrap(), vertical.unwrap());
    assert_rect(horizontal.track, [0.0, 5.7, 9.7, 0.3]);
    assert_rect(vertical.track, [9.7, 0.0, 0.3, 5.7]);
    // Half the width is visible; the tall content hits the two-thickness
    // minimum.
    assert_rect(horizontal.thumb, [0.0, 5.7, 4.85, 0.3]);
    assert_rect(vertical.thumb, [9.7, 0.0, 0.3, 0.6]);
}

#[test]
fn content_that_fits_hides_bars_unless_the_theme_styles_the_disabled_track() {
    let node = scroll_view(1, 0, [10.0, 6.0], [10.0, 5.0]);
    assert_eq!(scroll_bar_overflow(&node), [false, false]);
    let mut root = GuiRoot::default();
    author_part(
        &mut root,
        1,
        "scrollTrackX",
        "opacity",
        DynamicValue::F32(1.0),
    );
    // A base-part style alone does not ask for always-visible bars.
    assert!(!scroll_bar_shown(&root, &node, 1));
    author_part(
        &mut root,
        1,
        "scrollTrackY_disabled",
        "color",
        DynamicValue::Vec4([0.5, 0.5, 0.5, 1.0]),
    );
    assert!(scroll_bar_shown(&root, &node, 1));
    assert!(!scroll_bar_shown(&root, &node, 0));
    // Always-visible bars never take input.
    assert!(!scroll_bar_enabled(&node, 1));
}

#[test]
fn skinned_bars_paint_above_the_subtree_with_hover_and_pressed_states() {
    let mut root = GuiRoot::default();
    author_part(
        &mut root,
        1,
        "scrollThumbY",
        "color",
        DynamicValue::Vec4([0.0, 0.0, 1.0, 1.0]),
    );
    author_part(
        &mut root,
        1,
        "scrollThumbY_hovered",
        "color",
        DynamicValue::Vec4([0.0, 1.0, 0.0, 1.0]),
    );
    author_part(
        &mut root,
        1,
        "scrollTrackY_pressed",
        "color",
        DynamicValue::Vec4([1.0, 1.0, 0.0, 1.0]),
    );
    let view = view(vec![
        scroll_view(1, 0, [10.0, 6.0], [10.0, 10.0]),
        content(2, 1),
        content(3, 0),
    ]);
    let resolver = NoResources;
    let target = GuiInputTarget {
        entity: view.entity,
        root_incarnation: 1,
        node: GuiNodeId(1),
    };
    let paint = |cursors: &GuiSkinCursors| {
        crate::systems::gui::skinned_primitives_for_view(&view, &root, cursors, &resolver)
            .into_iter()
            .filter_map(|primitive| match primitive {
                SurfaceRenderPrimitive::Box {
                    style,
                    size,
                    fill,
                    ..
                } => match style.identity {
                    SurfacePrimitiveIdentity::Gui(id) => {
                        Some((id.node.0, id.part, style.position, size, fill))
                    }
                    SurfacePrimitiveIdentity::Authored(_) => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    // Bars follow the ScrollView's content (node 2) and precede the later
    // sibling (node 3); only the overflowing vertical axis paints.
    let mut cursors = GuiSkinCursors::default();
    cursors.scroll_bars.insert(
        target,
        GuiScrollBarCursor {
            offset: [0.0, 4.0],
            ..GuiScrollBarCursor::default()
        },
    );
    let idle = paint(&cursors);
    let order: Vec<_> = idle.iter().map(|(node, part, ..)| (*node, *part)).collect();
    assert_eq!(
        order,
        vec![
            (2, GuiPrimitivePart::Background),
            (1, GuiPrimitivePart::ScrollTrackY),
            (1, GuiPrimitivePart::ScrollThumbY),
            (3, GuiPrimitivePart::Background),
        ]
    );
    let thumb = &idle[2];
    assert!((thumb.2[1] - 2.4).abs() < 1e-5, "thumb at {:?}", thumb.2);
    assert_eq!(thumb.4, GuiShapeFill::Solid([0.0, 0.0, 1.0, 1.0]));
    // The unthemed track takes the translucent default over the node colour.
    assert_eq!(idle[1].4, GuiShapeFill::Solid([1.0, 1.0, 1.0, 0.25]));

    // Hovering the thumb and pressing the track resolve their own states.
    cursors.scroll_bars.insert(
        target,
        GuiScrollBarCursor {
            offset: [0.0, 4.0],
            hovered: Some(GuiPrimitivePart::ScrollThumbY),
            pressed: Some(GuiPrimitivePart::ScrollTrackY),
        },
    );
    let active = paint(&cursors);
    assert_eq!(active[1].4, GuiShapeFill::Solid([1.0, 1.0, 0.0, 1.0]));
    assert_eq!(active[2].4, GuiShapeFill::Solid([0.0, 1.0, 0.0, 1.0]));
}

/// ScrollView record whose viewport spans `[min_x, min_y, max_x, max_y]`.
fn scroll_view_at(
    node: u32,
    depth: u32,
    viewport: [f32; 4],
    extents: [f32; 2],
) -> GuiEvaluatedNode {
    GuiEvaluatedNode {
        rect: [
            viewport[0],
            viewport[1],
            viewport[2] - viewport[0],
            viewport[3] - viewport[1],
        ],
        viewport: Some(viewport),
        ..scroll_view(node, depth, [1.0, 1.0], extents)
    }
}

/// Bars of record `index` in `records` with per-node committed offsets.
fn nested_bars(
    records: Vec<GuiEvaluatedNode>,
    index: usize,
    offsets: &[(u32, [f32; 2])],
) -> [Option<GuiScrollBar>; 2] {
    let offset_of = |node: GuiNodeId| {
        offsets
            .iter()
            .find(|(id, _)| *id == node.0)
            .map_or([0.0, 0.0], |(_, offset)| *offset)
    };
    scroll_bars_in_view(&view(records), &GuiRoot::default(), index, offset_of)
}

/// An outer 4x3 ScrollView over `outer` content holding, through a plain
/// column, an inner ScrollView at `inner_viewport` over 4x3 content.
fn nested(outer: [f32; 2], inner_viewport: [f32; 4]) -> Vec<GuiEvaluatedNode> {
    vec![
        scroll_view_at(1, 0, [0.0, 0.0, 4.0, 3.0], outer),
        content(2, 1),
        scroll_view_at(3, 2, inner_viewport, [4.0, 3.0]),
        content(4, 3),
    ]
}

#[test]
fn nested_vertical_bars_sharing_an_edge_sit_side_by_side() {
    let records = nested([4.0, 5.0], [0.0, 0.0, 4.0, 2.0]);
    let outer = nested_bars(records.clone(), 0, &[])[1].unwrap();
    assert_rect(outer.track, [3.85, 0.0, 0.15, 3.0]);

    // The inner 0.1-thick track ends at the outer track's inner edge
    // instead of lying under it, at any vertical outer offset.
    for offset in [[0.0, 0.0], [0.0, 1.0]] {
        let [horizontal, vertical] = nested_bars(records.clone(), 2, &[(1, offset)]);
        assert!(horizontal.is_none());
        let vertical = vertical.unwrap();
        assert_rect(vertical.track, [3.75, 0.0, 0.1, 2.0]);
        assert_eq!(vertical.thumb[0], 3.75);
    }

    // Records without an enclosing ScrollView keep their own geometry.
    let alone = nested_bars(records[2..].to_vec(), 0, &[])[1].unwrap();
    assert_rect(alone.track, [3.9, 0.0, 0.1, 2.0]);
}

#[test]
fn nested_bars_that_do_not_meet_keep_their_own_edges() {
    // A narrower inner viewport, which also overflows horizontally, leaves
    // both inner tracks clear of the outer one around their own corner.
    let records = nested([4.0, 5.0], [0.0, 0.0, 3.0, 2.0]);
    let [horizontal, vertical] = nested_bars(records, 2, &[]);
    assert_rect(vertical.unwrap().track, [2.9, 0.0, 0.1, 1.9]);
    assert_rect(horizontal.unwrap().track, [0.0, 1.9, 2.9, 0.1]);

    // A horizontal outer offset moves the inner viewport left of the outer
    // track, so the inner track stays at its viewport edge; unscrolled, it
    // moves aside.
    let records = nested([5.0, 5.0], [0.0, 0.0, 4.0, 2.0]);
    let scrolled = nested_bars(records.clone(), 2, &[(1, [1.0, 0.0])])[1].unwrap();
    assert_rect(scrolled.track, [3.9, 0.0, 0.1, 2.0]);
    let unscrolled = nested_bars(records, 2, &[])[1].unwrap();
    assert_rect(unscrolled.track, [3.75, 0.0, 0.1, 2.0]);
}

#[test]
fn nested_tracks_end_before_a_crossing_outer_track() {
    // The outer view scrolls only horizontally: its 0.15-thick track runs
    // along the bottom at y 2.85..3, where the inner vertical track ends.
    let records = nested([6.0, 3.0], [0.0, 1.0, 4.0, 3.0]);
    let outer = nested_bars(records.clone(), 0, &[]);
    assert!(outer[1].is_none());
    assert_rect(outer[0].unwrap().track, [0.0, 2.85, 4.0, 0.15]);
    let inner = nested_bars(records, 2, &[])[1].unwrap();
    assert_rect(inner.track, [3.9, 1.0, 0.1, 1.85]);
}
