//! Focused tests for GUI constraint evaluation.

use super::super::super::test_support::{font_source, test_font};
use super::test_support::*;
use super::*;
use crate::EntityId;
use crate::services::asset_management::AssetKey;
use crate::systems::gui::GuiNodePatch;
use std::collections::BTreeMap;

#[test]
fn column_stacks_children_vertically() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let first = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 1.0),
    );
    let second = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(2.0, 0.5),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // Root fills the 4x2 logical Surface; children stack from the top.
    assert_rect(node_by_id(view, root).rect, [0.0, 0.0, 4.0, 2.0]);
    assert_rect(node_by_id(view, first).rect, [0.0, 0.0, 1.0, 1.0]);
    assert_rect(node_by_id(view, second).rect, [0.0, 1.0, 2.0, 0.5]);
    assert!(view.diagnostics.is_empty());
}

#[test]
fn row_flex_shares_leftover_space() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Row),
        text_style(),
    );
    let fixed = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 0.5),
    );
    let flex_one = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            flex: Some(1.0),
            ..sized(0.0, 0.5)
        },
    );
    let flex_three = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            flex: Some(3.0),
            ..sized(0.0, 0.5)
        },
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // Leftover 3.0 splits 1:3 into 0.75 and 2.25; heights fill the row max.
    assert_rect(node_by_id(view, fixed).rect, [0.0, 0.0, 1.0, 0.5]);
    assert_rect(node_by_id(view, flex_one).rect, [1.0, 0.0, 0.75, 0.5]);
    assert_rect(node_by_id(view, flex_three).rect, [1.75, 0.0, 2.25, 0.5]);
    assert_rect(node_by_id(view, root).rect, [0.0, 0.0, 4.0, 2.0]);
}

#[test]
fn unbounded_flex_diagnoses_instead_of_solving() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::ScrollView),
        GuiNodeStyle {
            width: Some(2.0),
            height: Some(1.0),
            ..Default::default()
        },
    );
    let column = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::Column),
        GuiNodeStyle::default(),
    );
    let flexed = tree.add(
        Some(column),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            flex: Some(1.0),
            ..Default::default()
        },
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // The scroll height is unbounded, so flex keeps zero main-axis extent;
    // the empty box also fits zero cross extent.
    assert_rect(node_by_id(view, root).rect, [0.0, 0.0, 2.0, 1.0]);
    assert_rect(node_by_id(view, flexed).rect, [0.0, 0.0, 0.0, 0.0]);
    assert!(
        view.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            GuiLayoutDiagnostic::UnboundedFlex {
                node,
            } if *node == flexed
        )),
        "diagnostics: {:?}",
        view.diagnostics
    );
    assert_eq!(node_by_id(view, root).content_extents, Some([2.0, 0.0]));
}

#[test]
fn min_max_clamps_explicit_size() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            width: Some(5.0),
            height: Some(5.0),
            max_width: Some(2.0),
            max_height: Some(1.0),
            ..Default::default()
        },
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, root).rect, [0.0, 0.0, 2.0, 1.0]);
}

#[test]
fn padding_insets_the_child() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Padding),
        GuiNodeStyle {
            padding: Some([0.5, 0.0, 0.0, 0.25]),
            ..Default::default()
        },
    );
    let child = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 1.0),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, child).rect, [0.25, 0.5, 1.0, 1.0]);
    // The padding container fills its bounded constraints.
    assert_rect(node_by_id(view, root).rect, [0.0, 0.0, 4.0, 2.0]);
}

#[test]
fn align_defaults_to_center() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Align),
        sized(4.0, 2.0),
    );
    let child = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 1.0),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, child).rect, [1.5, 0.5, 1.0, 1.0]);
}

#[test]
fn stack_overlays_with_reverse_painter_hits() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Stack),
        sized(4.0, 2.0),
    );
    let _first = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 1.0),
    );
    let second = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(2.0, 0.5),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // Later siblings paint above earlier ones and win hit ties. The root
    // container fills the panel, so points outside every child still hit it.
    let hit = view.hit_test([0.1, 0.1]).unwrap();
    assert_eq!(hit.node, second);
    assert_eq!(view.hit_test([3.0, 1.5]).unwrap().node, root);
}

#[test]
fn stack_margins_place_descendants_and_preserve_clipped_hit_bounds() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Stack),
        sized(4.0, 2.0),
    );
    let offset = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::Stack),
        GuiNodeStyle {
            margin: Some([0.3, -1.2, -0.3, 1.2]),
            ..sized(1.0, 0.4)
        },
    );
    let child = tree.add(
        Some(offset),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 0.4),
    );
    let clipped = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            margin: Some([0.0, 0.25, 0.0, -0.25]),
            ..sized(0.5, 0.5)
        },
    );
    let inset = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            margin: Some([1.0, 0.5, 0.5, 0.5]),
            enabled: false,
            ..sized(4.0, 2.0)
        },
    );
    let root_tree = tree.build();
    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, offset).rect, [1.2, 0.3, 1.0, 0.4]);
    assert_rect(node_by_id(view, child).rect, [1.2, 0.3, 1.0, 0.4]);
    assert_rect(node_by_id(view, clipped).rect, [-0.25, 0.0, 0.5, 0.5]);
    assert_rect(node_by_id(view, inset).rect, [0.5, 1.0, 3.0, 0.5]);
    assert_eq!(view.hit_test([1.5, 0.5]).unwrap().node, child);
    assert_eq!(view.hit_test([0.1, 0.1]).unwrap().node, clipped);
    assert!(view.hit_test([-0.1, 0.1]).is_none());
}

#[test]
fn stack_aligns_centre_and_end_children_within_their_margin_boxes() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Stack),
        sized(4.0, 2.0),
    );
    let centred = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            margin: Some([0.2, 0.3, 0.1, 0.5]),
            align_x: Some(0.0),
            align_y: Some(1.0),
            ..sized(1.0, 0.4)
        },
    );
    let overhanging = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            margin: Some([0.0, -0.2, 0.0, 0.0]),
            align_x: Some(1.0),
            ..sized(1.0, 0.4)
        },
    );
    let root_tree = tree.build();
    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // The 1.8 x 0.7 margin box centres horizontally and ends vertically:
    // x = 0.5 + (4.0 - 1.8) / 2, y = 0.2 + (2.0 - 0.7).
    assert_rect(node_by_id(view, centred).rect, [1.6, 1.5, 1.0, 0.4]);

    // A negative end margin shrinks the margin box to 0.8, so end alignment
    // places the node 0.2 past the Stack's right edge.
    assert_rect(node_by_id(view, overhanging).rect, [3.2, 0.0, 1.0, 0.4]);
    assert_rect(node_by_id(view, root).rect, [0.0, 0.0, 4.0, 2.0]);
}

#[test]
fn text_leaf_measures_and_paints_glyphs() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let leaf = tree.add(
        Some(root),
        GuiNodeContent::Text("A".to_owned()),
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // 0.6 em wide, 1.2 em tall at 0.1 m/em.
    assert_rect(node_by_id(view, leaf).rect, [0.0, 0.0, ADV_A, LINE]);
    assert_eq!(view.remeasure_count, 1);

    let primitives = view.surface_primitives();
    assert_eq!(primitives.len(), 1);
    match &primitives[0] {
        crate::systems::surface::SurfaceRenderPrimitive::Glyphs {
            glyphs,
            font_size,
            ..
        } => {
            assert_eq!(glyphs.len(), 1);
            assert_eq!(glyphs[0].glyph_id, 1);
            assert_eq!(glyphs[0].position[0], 0.0);
            assert!((glyphs[0].position[1] - 0.08).abs() < 1e-5);
            assert_eq!(*font_size, FONT_SIZE);
        }
        other => panic!("expected glyphs, got {other:?}"),
    }
}

#[test]
fn text_wraps_under_finite_width() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let leaf = tree.add(
        Some(root),
        GuiNodeContent::Text("A A A".to_owned()),
        GuiNodeStyle {
            width: Some(2.0 * ADV_A),
            font_size: FONT_SIZE,
            asset: Some(font_source()),
            ..Default::default()
        },
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // "A A" spans 0.15 logical units, wider than the 0.12 constraint, so
    // each word wraps onto its own of three lines. The explicit lane keeps
    // the node box at the full constraint width.
    let record = node_by_id(view, leaf);
    match &record.content {
        GuiEvaluatedContent::Text {
            layout,
            ..
        } => assert_eq!(layout.lines.len(), 3),
        other => panic!("expected text, got {other:?}"),
    }
    assert_rect(record.rect, [0.0, 0.0, 2.0 * ADV_A, 3.0 * LINE]);
}

#[test]
fn pending_font_marks_leaf_unavailable() {
    let font = test_font();
    let _ = font;
    let resolver = TestResolver {
        font: None,
        pending_font: true,
        resources: BTreeMap::new(),
    };
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let leaf = tree.add(
        Some(root),
        GuiNodeContent::Text("A".to_owned()),
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    let record = node_by_id(view, leaf);
    assert!(!record.available);
    assert!(!record.visible);
    assert_rect(record.rect, [0.0, 0.0, 0.0, 0.0]);
    assert!(view.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        GuiLayoutDiagnostic::PendingText {
            node,
        } if *node == leaf
    )));
    assert_eq!(view.remeasure_count, 0);
    assert!(view.surface_primitives().is_empty());
    let _ = root;
}

#[test]
fn visual_transform_moves_paint_and_hit_together() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Stack),
        sized(4.0, 2.0),
    );
    let moved = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 1.0),
    );
    tree.set_visual(moved, [2.0, 1.0], [2.0, 2.0]);
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, moved).rect, [2.0, 1.0, 2.0, 2.0]);
    assert_eq!(view.hit_test([2.5, 1.5]).unwrap().node, moved);
    // The filling root still owns points the moved child vacated.
    assert_eq!(view.hit_test([0.5, 0.5]).unwrap().node, root);

    // Visual edits rebuild rectangles without reflowing or remeasuring text.
    assert_eq!(view.remeasure_count, 0);
}

#[test]
fn singular_scale_suppresses_its_subtree() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Stack),
        sized(4.0, 2.0),
    );
    let flat = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 1.0),
    );
    tree.set_visual(flat, [0.0, 0.0], [0.0, 1.0]);
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    let record = node_by_id(view, flat);
    assert!(!record.available);
    assert_rect(record.rect, [0.0, 0.0, 0.0, 0.0]);
    assert!(view.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        GuiLayoutDiagnostic::SingularTransform {
            node,
        } if *node == flat
    )));
    assert!(view.hit_test([0.1, 0.1]).is_none());
}

#[test]
fn scroll_view_clips_content_and_reports_extents() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::ScrollView),
        sized(2.0, 1.0),
    );
    let column = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::Column),
        GuiNodeStyle::default(),
    );
    for _ in 0..3 {
        tree.add(
            Some(column),
            GuiNodeContent::Container(GuiContainerKind::SizedBox),
            sized(2.0, 1.0),
        );
    }
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    let viewport = node_by_id(view, root);
    assert_rect(viewport.rect, [0.0, 0.0, 2.0, 1.0]);
    assert_eq!(viewport.content_extents, Some([2.0, 3.0]));

    // Content below the viewport keeps its rectangles but carries the
    // viewport clip, so paint and hit testing agree on visibility.
    let below = &view.nodes[view.nodes.len() - 1];
    assert_rect(below.rect, [0.0, 2.0, 2.0, 1.0]);
    assert_eq!(below.clip, Some([0.0, 0.0, 2.0, 1.0]));
    assert!(view.hit_test([1.0, 2.5]).is_none());
    assert!(view.hit_test([1.0, 0.5]).is_some());
}

#[test]
fn units_per_metre_scales_root_and_metre_lanes() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    // Explicit lanes are local metres; at 100 units per metre a 0.02 m box
    // spans 2 logical units on a 400x200 logical root.
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let child = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(0.02, 0.01),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let req = GuiLayoutRequest {
        units_per_metre: 100.0,
        ..request(&root_tree, 1)
    };
    let view = cache.evaluate(entity(), &req, &resolver);

    assert_eq!(view.root_bounds, [0.0, 0.0, 400.0, 200.0]);
    assert_rect(node_by_id(view, child).rect, [0.0, 0.0, 2.0, 1.0]);
}

#[test]
fn invalid_root_input_is_unavailable() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let req = GuiLayoutRequest {
        surface_size: [0.0, 2.0],
        ..request(&root_tree, 1)
    };
    let view = cache.evaluate(entity(), &req, &resolver);

    assert!(!view.available);
    assert!(view.hit_test([0.5, 0.5]).is_none());
    assert!(view.surface_primitives().is_empty());
    assert!(view.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        GuiLayoutDiagnostic::InvalidConstraints {
            node: None,
            detail: GuiConstraintError::InvalidRoot,
        }
    )));
}

#[test]
fn panel_resolution_compares_blocker_distances() {
    let hit = GuiHit {
        node: GuiNodeId(2),
        lifetime: 1,
        position: [1.0, 1.0],
    };
    let blocker = |distance: f32, bits: u64| GuiBlockerHit {
        distance,
        entity: EntityId::from_bits(bits),
    };

    // No blockers: the panel stands.
    assert_eq!(
        resolve_panel_hit(Some(5.0), Some(hit), &[]),
        GuiPanelResolution::Panel(hit)
    );
    // A nearer explicit blocker wins.
    assert_eq!(
        resolve_panel_hit(Some(5.0), Some(hit), &[blocker(3.0, 9)]),
        GuiPanelResolution::Blocked {
            entity: EntityId::from_bits(9),
        }
    );
    // A farther blocker does not block.
    assert_eq!(
        resolve_panel_hit(Some(5.0), Some(hit), &[blocker(7.0, 9)]),
        GuiPanelResolution::Panel(hit)
    );
    // A coincident blocker takes precedence over the panel.
    assert_eq!(
        resolve_panel_hit(Some(5.0), Some(hit), &[blocker(5.0, 9)]),
        GuiPanelResolution::Blocked {
            entity: EntityId::from_bits(9),
        }
    );
    // Blocker ties break on stable entity identity.
    assert_eq!(
        resolve_panel_hit(Some(5.0), Some(hit), &[blocker(3.0, 11), blocker(3.0, 9)]),
        GuiPanelResolution::Blocked {
            entity: EntityId::from_bits(9),
        }
    );
    // Missing or invalid panel input never fabricates a hit.
    assert_eq!(
        resolve_panel_hit(None, Some(hit), &[]),
        GuiPanelResolution::Miss
    );
    assert_eq!(
        resolve_panel_hit(Some(f32::NAN), Some(hit), &[]),
        GuiPanelResolution::Miss
    );
    assert_eq!(
        resolve_panel_hit(Some(5.0), None, &[]),
        GuiPanelResolution::Miss
    );
}

#[test]
fn content_point_converts_through_units() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Stack),
        sized(0.02, 0.02),
    );
    let child = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(0.01, 0.01),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let req = GuiLayoutRequest {
        units_per_metre: 100.0,
        ..request(&root_tree, 1)
    };
    // At 100 units per metre the 0.01 m box spans 1 logical unit, so the
    // content-metre point [0.005, 0.005] lands inside it while
    // [0.015, 0.015] lands on the filling root.
    let view = cache.evaluate(entity(), &req, &resolver);
    assert_rect(node_by_id(view, child).rect, [0.0, 0.0, 1.0, 1.0]);
    let hit = view.hit_test_content([0.005, 0.005]).unwrap();
    assert_eq!(hit.node, child);
    assert_eq!(view.hit_test_content([0.015, 0.015]).unwrap().node, root);
}

#[test]
fn drawing_leaf_needs_its_resource() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font).with_resource(
        "test-draw",
        AssetKey {
            slot: 5,
            generation: 1,
        },
    );
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let leaf = tree.add(
        Some(root),
        GuiNodeContent::Drawing,
        GuiNodeStyle {
            width: Some(1.0),
            height: Some(0.5),
            asset: Some(draw_source()),
            ..Default::default()
        },
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, leaf).rect, [0.0, 0.0, 1.0, 0.5]);
    let drawings = view
        .surface_primitives()
        .iter()
        .filter(|primitive| {
            matches!(
                primitive,
                crate::systems::surface::SurfaceRenderPrimitive::Drawing { .. }
            )
        })
        .count();
    assert_eq!(drawings, 1);

    // Without the resource the leaf keeps its size but paints nothing.
    let bare = TestResolver::with_font(&font);
    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &bare);
    assert_rect(node_by_id(view, leaf).rect, [0.0, 0.0, 1.0, 0.5]);
    assert!(view.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        GuiLayoutDiagnostic::MissingResource {
            node,
            kind: "drawing",
        } if *node == leaf
    )));
    assert!(view.surface_primitives().is_empty());
}

#[test]
fn checkbox_and_slider_carry_intrinsic_sizes() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let check = tree.add(
        Some(root),
        GuiNodeContent::Checkbox {
            checked: false,
        },
        text_style(),
    );
    let slider = tree.add(
        Some(root),
        GuiNodeContent::Slider {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            step: 0.0,
        },
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // 1.4 em box and 8 x 1.4 em bar at 0.1 m/em.
    assert_rect(node_by_id(view, check).rect, [0.0, 0.0, 1.4 * EM, 1.4 * EM]);
    assert_rect(
        node_by_id(view, slider).rect,
        [0.0, 1.4 * EM, 8.0 * EM, 1.4 * EM],
    );
}

#[test]
fn text_input_measures_placeholder_when_empty() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let input = tree.add(
        Some(root),
        GuiNodeContent::TextInput {
            text: String::new(),
            placeholder: "Ae".to_owned(),
        },
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // "Ae" spans 0.6 + 0.55 em at 0.1 m/em; single line keeps one line.
    // The builder seeds the initial commit, so the empty effective value
    // measures the placeholder at revision 1.
    let record = node_by_id(view, input);
    assert_rect(record.rect, [0.0, 0.0, 0.115, LINE]);
    match &record.content {
        GuiEvaluatedContent::TextInput {
            text,
            revision,
            ..
        } => {
            assert_eq!(text, "Ae");
            assert_eq!(*revision, 1);
        }
        other => panic!("expected text input, got {other:?}"),
    }
}

/// Commit one control value through the authoritative revision gate, mirroring
/// a routed input commit at the mutation boundary. Returns the new revision.
fn commit_value(root: &mut GuiRoot, id: GuiNodeId, expected: u32, value: GuiControlValue) -> u32 {
    let content = root
        .nodes()
        .node(id)
        .map(|node| node.content.clone())
        .unwrap();
    root.controls_mut()
        .set(id, &content, expected, value)
        .unwrap()
}

#[test]
fn committed_text_wins_over_authored_content() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let input = tree.add(
        Some(root),
        GuiNodeContent::TextInput {
            text: "a".to_owned(),
            placeholder: "e".to_owned(),
        },
        text_style(),
    );
    let mut root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();
    // "a" spans 0.55 em at 0.1 m/em (`a` maps to glyph 3 advancing 550).
    let record = node_by_id(&first, input);
    assert_rect(record.rect, [0.0, 0.0, 0.055, LINE]);
    match &record.content {
        GuiEvaluatedContent::TextInput {
            text,
            revision,
            ..
        } => {
            assert_eq!(text, "a");
            assert_eq!(*revision, 1);
        }
        other => panic!("expected text input, got {other:?}"),
    }

    // A routed commit at revision 1 replaces the authored initial: the
    // retained view must reflow and measure the effective text.
    assert_eq!(
        commit_value(&mut root_tree, input, 1, GuiControlValue::Text("ae".into())),
        2
    );
    let second = cache
        .evaluate(entity(), &request(&root_tree, 2), &resolver)
        .clone();
    assert_eq!(second.layout_revision, first.layout_revision + 1);
    assert_eq!(second.reflow_count, first.reflow_count + 1);
    assert_eq!(second.remeasure_count, first.remeasure_count + 1);
    // "ae" spans 0.55 + 0.55 em at 0.1 m/em.
    let record = node_by_id(&second, input);
    assert_rect(record.rect, [0.0, 0.0, 0.11, LINE]);
    match &record.content {
        GuiEvaluatedContent::TextInput {
            text,
            revision,
            ..
        } => {
            assert_eq!(text, "ae");
            assert_eq!(*revision, 2);
        }
        other => panic!("expected text input, got {other:?}"),
    }

    // Unchanged frames reuse retained output without further work.
    let third = cache
        .evaluate(entity(), &request(&root_tree, 3), &resolver)
        .clone();
    assert_eq!(third.layout_revision, second.layout_revision);
    assert_eq!(third.reflow_count, second.reflow_count);
    assert_eq!(third.remeasure_count, second.remeasure_count);
}

#[test]
fn checkbox_and_slider_report_effective_values() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let check = tree.add(
        Some(root),
        GuiNodeContent::Checkbox {
            checked: false,
        },
        text_style(),
    );
    let slider = tree.add(
        Some(root),
        GuiNodeContent::Slider {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            step: 0.0,
        },
        text_style(),
    );
    let mut root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();
    match &node_by_id(&first, check).content {
        GuiEvaluatedContent::Checkbox {
            checked,
            revision,
        } => {
            assert!(!checked);
            assert_eq!(*revision, 1);
        }
        other => panic!("expected checkbox, got {other:?}"),
    }
    match &node_by_id(&first, slider).content {
        GuiEvaluatedContent::Slider {
            value,
            min,
            max,
            step,
            revision,
        } => {
            assert_eq!(
                (*value, *min, *max, *step, *revision),
                (0.5, 0.0, 1.0, 0.0, 1)
            );
        }
        other => panic!("expected slider, got {other:?}"),
    }

    // Committed revisions win; bounds and step stay authored lanes.
    commit_value(&mut root_tree, check, 1, GuiControlValue::Bool(true));
    commit_value(&mut root_tree, slider, 1, GuiControlValue::Scalar(0.75));
    let second = cache
        .evaluate(entity(), &request(&root_tree, 2), &resolver)
        .clone();
    assert_eq!(second.layout_revision, first.layout_revision);
    assert_eq!(second.reflow_count, first.reflow_count);
    assert_eq!(second.remeasure_count, first.remeasure_count);
    assert_eq!(second.paint_revision, first.paint_revision + 1);
    match &node_by_id(&second, check).content {
        GuiEvaluatedContent::Checkbox {
            checked,
            revision,
        } => {
            assert!(checked);
            assert_eq!(*revision, 2);
        }
        other => panic!("expected checkbox, got {other:?}"),
    }
    match &node_by_id(&second, slider).content {
        GuiEvaluatedContent::Slider {
            value,
            min,
            max,
            step,
            revision,
        } => {
            assert_eq!(
                (*value, *min, *max, *step, *revision),
                (0.75, 0.0, 1.0, 0.0, 2)
            );
        }
        other => panic!("expected slider, got {other:?}"),
    }
    // Value-only commits refresh payload and paint without reflow.
    assert_rect(
        node_by_id(&second, check).rect,
        [0.0, 0.0, 1.4 * EM, 1.4 * EM],
    );
    assert_rect(
        node_by_id(&second, slider).rect,
        [0.0, 1.4 * EM, 8.0 * EM, 1.4 * EM],
    );

    // Repeated slider commits remain proportional: payload and paint advance,
    // while the retained tree performs no additional layout or measurement.
    commit_value(&mut root_tree, slider, 2, GuiControlValue::Scalar(0.25));
    let third = cache
        .evaluate(entity(), &request(&root_tree, 3), &resolver)
        .clone();
    assert_eq!(third.layout_revision, second.layout_revision);
    assert_eq!(third.reflow_count, second.reflow_count);
    assert_eq!(third.remeasure_count, second.remeasure_count);
    assert_eq!(third.paint_revision, second.paint_revision + 1);
    assert!(matches!(
        &node_by_id(&third, slider).content,
        GuiEvaluatedContent::Slider {
            value,
            revision: 3,
            ..
        } if *value == 0.25
    ));
}

#[test]
fn missing_committed_state_falls_back_to_authored() {
    // A hand-rolled root without control entries is never valid production
    // state, but the evaluator stays total: authored values apply at
    // revision zero instead of panicking.
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut root_tree = GuiRoot::default();
    root_tree
        .nodes_mut()
        .insert_node(
            GuiNodeId(1),
            None,
            0,
            GuiNodeContent::Container(GuiContainerKind::Column),
        )
        .unwrap();
    root_tree
        .nodes_mut()
        .insert_node(
            GuiNodeId(2),
            Some(GuiNodeId(1)),
            0,
            GuiNodeContent::Checkbox {
                checked: true,
            },
        )
        .unwrap();
    root_tree
        .nodes_mut()
        .insert_node(
            GuiNodeId(3),
            Some(GuiNodeId(1)),
            1,
            GuiNodeContent::TextInput {
                text: "a".to_owned(),
                placeholder: "e".to_owned(),
            },
        )
        .unwrap();
    for id in [GuiNodeId(1), GuiNodeId(2), GuiNodeId(3)] {
        root_tree.install_node_style(id, &text_style()).unwrap();
    }

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);
    match &node_by_id(view, GuiNodeId(2)).content {
        GuiEvaluatedContent::Checkbox {
            checked,
            revision,
        } => {
            assert!(checked);
            assert_eq!(*revision, 0);
        }
        other => panic!("expected checkbox, got {other:?}"),
    }
    match &node_by_id(view, GuiNodeId(3)).content {
        GuiEvaluatedContent::TextInput {
            text,
            revision,
            ..
        } => {
            assert_eq!(text, "a");
            assert_eq!(*revision, 0);
        }
        other => panic!("expected text input, got {other:?}"),
    }
}

#[test]
fn nested_nonuniform_scale_applies_to_child_extents() {
    // Root Column fills 4x2. Child A is a 2x2 box scaled [2.0, 0.5]; its
    // 1x1 leaf B inherits the chain, so B spans [0, 0, 2, 0.5] in final
    // units. The old own-scale-only rectangle read [0, 0, 1, 1].
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let scaled = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(2.0, 2.0),
    );
    tree.set_visual(scaled, [0.0, 0.0], [2.0, 0.5]);
    let leaf = tree.add(
        Some(scaled),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 1.0),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, scaled).rect, [0.0, 0.0, 4.0, 1.0]);
    assert_rect(node_by_id(view, leaf).rect, [0.0, 0.0, 2.0, 0.5]);
    assert_eq!(node_by_id(view, leaf).acc_scale, [2.0, 0.5]);
    // Hit testing shares the scaled rectangle: stretched X hits where the
    // unscaled box ended, while shrunk Y falls through to the parent where
    // the unscaled box still covered.
    assert_eq!(view.hit_test([1.5, 0.25]).unwrap().node, leaf);
    assert_eq!(view.hit_test([0.5, 0.75]).unwrap().node, scaled);
}

#[test]
fn asymmetric_text_child_paints_and_hits_scaled() {
    // Text "A" measures 0.06 x 0.12 logical. Nested scales [2.0, 1.0] then
    // [1.0, 0.5] accumulate to [2.0, 0.5], so the final rect reads
    // [0, 0, 0.12, 0.06] with unscaled glyph payload and scaled style.
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let middle = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 1.0),
    );
    tree.set_visual(middle, [0.0, 0.0], [2.0, 1.0]);
    let leaf = tree.add(
        Some(middle),
        GuiNodeContent::Text("A".to_owned()),
        text_style(),
    );
    tree.set_visual(leaf, [0.0, 0.0], [1.0, 0.5]);
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, leaf).rect, [0.0, 0.0, 0.12, 0.06]);
    assert_eq!(node_by_id(view, leaf).acc_scale, [2.0, 0.5]);
    assert_eq!(view.hit_test([0.1, 0.03]).unwrap().node, leaf);
    assert_eq!(view.hit_test([0.03, 0.1]).unwrap().node, middle);

    let primitives = view.surface_primitives();
    assert_eq!(primitives.len(), 1);
    match &primitives[0] {
        crate::systems::surface::SurfaceRenderPrimitive::Glyphs {
            style,
            glyphs,
            font_size,
            font: _,
        } => {
            assert_eq!(glyphs.len(), 1);
            assert_eq!(glyphs[0].glyph_id, 1);
            assert_eq!(glyphs[0].position[0], 0.0);
            assert!((glyphs[0].position[1] - 0.08).abs() < 1e-5);
            assert_eq!(*font_size, FONT_SIZE);
            assert_eq!(style.scale, [2.0, 0.5]);
        }
        other => panic!("expected glyphs, got {other:?}"),
    }
}

#[test]
fn scaled_drawing_leaf_carries_accumulated_scale() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font).with_resource(
        "test-draw",
        AssetKey {
            slot: 5,
            generation: 1,
        },
    );
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    let scaled = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(1.0, 0.5),
    );
    tree.set_visual(scaled, [0.0, 0.0], [3.0, 2.0]);
    let leaf = tree.add(
        Some(scaled),
        GuiNodeContent::Drawing,
        GuiNodeStyle {
            width: Some(1.0),
            height: Some(0.5),
            asset: Some(draw_source()),
            ..Default::default()
        },
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, leaf).rect, [0.0, 0.0, 3.0, 1.0]);
    let primitives = view.surface_primitives();
    assert_eq!(primitives.len(), 1);
    match &primitives[0] {
        crate::systems::surface::SurfaceRenderPrimitive::Drawing {
            style,
            ..
        } => {
            assert_eq!(style.scale, [3.0, 2.0]);
            assert_eq!(style.position, [0.0, 0.0]);
        }
        other => panic!("expected drawing, got {other:?}"),
    }
}

#[test]
fn scaled_scroll_viewport_clips_in_final_coordinates() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::ScrollView),
        sized(2.0, 1.0),
    );
    tree.set_visual(root, [0.0, 0.0], [2.0, 0.5]);
    let column = tree.add(
        Some(root),
        GuiNodeContent::Container(GuiContainerKind::Column),
        GuiNodeStyle::default(),
    );
    let child = tree.add(
        Some(column),
        GuiNodeContent::Container(GuiContainerKind::SizedBox),
        sized(2.0, 1.0),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, root).rect, [0.0, 0.0, 4.0, 0.5]);
    let record = node_by_id(view, child);
    assert_rect(record.rect, [0.0, 0.0, 4.0, 0.5]);
    assert_eq!(record.clip, Some([0.0, 0.0, 4.0, 0.5]));
}

#[test]
fn disabled_nodes_skip_hit_testing() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        sized(4.0, 2.0),
    );
    let control = tree.add(
        Some(root),
        GuiNodeContent::Checkbox {
            checked: false,
        },
        GuiNodeStyle {
            enabled: false,
            ..sized(1.0, 1.0)
        },
    );
    let mut root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);
    let node = node_by_id(view, control);
    assert!(!node.enabled);
    // The disabled control keeps its rectangle but never activates: the
    // retained hit test passes through to the enabled container.
    let centre = [
        node.rect[0] + node.rect[2] / 2.0,
        node.rect[1] + node.rect[3] / 2.0,
    ];
    assert_eq!(view.hit_test(centre).map(|hit| hit.node), Some(root));
    assert_eq!(view.reflow_count, 1);

    // Re-enabling through the authoring lane refreshes hit eligibility without reflow.
    root_tree
        .apply_patch(
            control,
            &GuiNodePatch {
                enabled: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
    let view = cache.evaluate(entity(), &request(&root_tree, 2), &resolver);
    let node = node_by_id(view, control);
    assert!(node.enabled);
    let centre = [
        node.rect[0] + node.rect[2] / 2.0,
        node.rect[1] + node.rect[3] / 2.0,
    ];
    assert_eq!(view.hit_test(centre).map(|hit| hit.node), Some(control));
    assert_eq!(view.reflow_count, 1);
}
