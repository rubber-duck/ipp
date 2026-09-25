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
        virtual_lists: Default::default(),
        diagnostics: Vec::new(),
        remeasure_count: 0,
        reflow_count: 0,
        available: true,
    }
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
    let [horizontal, vertical] = scroll_bars(&node, [0.0, 0.0], [false, true]);
    assert!(horizontal.is_none());
    let vertical = vertical.unwrap();
    assert_rect(vertical.track, [9.7, 0.0, 0.3, 6.0]);
    assert_rect(vertical.thumb, [9.7, 0.0, 0.3, 3.6]);
    assert_eq!((vertical.capacity, vertical.page), (4.0, 6.0));

    // The thumb travels the remaining 2.4 units in proportion to the offset.
    let middle = scroll_bars(&node, [0.0, 2.0], [false, true])[1].unwrap();
    assert_rect(middle.thumb, [9.7, 1.2, 0.3, 3.6]);
    let end = scroll_bars(&node, [0.0, 4.0], [false, true])[1].unwrap();
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
    let [horizontal, vertical] = scroll_bars(&node, [0.0, 0.0], [true, true]);
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
