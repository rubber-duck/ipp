//! Aligned containers move complete subtrees.
//!
//! Align, Stack and Row/Column cross alignment place a child only after its
//! subtree has measured. These tests pin that descendants, content origins,
//! nested ScrollView clips and hit targets follow the aligned child, under
//! scaled and reflected placement, without remeasuring text. Expected
//! rectangles are hand-computed from the documented layout rules.

use super::super::super::test_support::test_font;
use super::test_support::*;
use super::*;
use crate::systems::surface::SurfaceRenderPrimitive;

fn container(kind: GuiContainerKind) -> GuiNodeContent {
    GuiNodeContent::Container(kind)
}

fn aligned(w: f32, h: f32, align_x: f32, align_y: f32) -> GuiNodeStyle {
    GuiNodeStyle {
        align_x: Some(align_x),
        align_y: Some(align_y),
        ..sized(w, h)
    }
}

#[test]
fn align_moves_nested_containers_text_and_hit_targets() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(None, container(GuiContainerKind::Align), sized(4.0, 2.0));
    let column = tree.add(
        Some(root),
        container(GuiContainerKind::Column),
        sized(2.0, 1.0),
    );
    let block = tree.add(
        Some(column),
        container(GuiContainerKind::SizedBox),
        sized(1.0, 0.5),
    );
    let text = tree.add(
        Some(column),
        GuiNodeContent::Text("A".to_owned()),
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // The 2 x 1 column centres in 4 x 2 at (1, 0.5); its children keep
    // their column-local slots relative to that origin.
    assert_rect(node_by_id(view, column).rect, [1.0, 0.5, 2.0, 1.0]);
    assert_rect(node_by_id(view, block).rect, [1.0, 0.5, 1.0, 0.5]);
    assert_rect(node_by_id(view, text).rect, [1.0, 1.0, ADV_A, LINE]);
    assert_eq!(node_by_id(view, text).content_origin, [1.0, 1.0]);

    assert_eq!(view_hit(view, [1.5, 0.75]), block);
    assert_eq!(view_hit(view, [1.03, 1.05]), text);
    assert_eq!(view_hit(view, [1.5, 1.4]), column);
    assert_eq!(view_hit(view, [0.5, 0.25]), root);

    // Glyph paint anchors at the moved content origin.
    let glyphs = view
        .surface_primitives()
        .into_iter()
        .find_map(|primitive| match primitive {
            SurfaceRenderPrimitive::Glyphs {
                style,
                ..
            } => Some(style.position),
            _ => None,
        })
        .unwrap();
    assert_eq!(glyphs, [1.0, 1.0]);
}

#[test]
fn stack_centre_and_end_alignment_move_nested_children() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(None, container(GuiContainerKind::Stack), sized(4.0, 2.0));
    let centred = tree.add(
        Some(root),
        container(GuiContainerKind::Row),
        aligned(2.0, 0.5, 0.0, 1.0),
    );
    let first = tree.add(
        Some(centred),
        container(GuiContainerKind::SizedBox),
        sized(0.5, 0.5),
    );
    let second = tree.add(
        Some(centred),
        container(GuiContainerKind::SizedBox),
        sized(1.0, 0.25),
    );
    let ended = tree.add(
        Some(root),
        container(GuiContainerKind::Stack),
        GuiNodeStyle {
            margin: Some([0.0, 0.25, 0.25, 0.0]),
            ..aligned(1.0, 1.0, 1.0, 1.0)
        },
    );
    let inner = tree.add(
        Some(ended),
        container(GuiContainerKind::Align),
        GuiNodeStyle::default(),
    );
    let dot = tree.add(
        Some(inner),
        container(GuiContainerKind::SizedBox),
        sized(0.5, 0.5),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // Centre x and end y: ((4 - 2) / 2, 2 - 0.5).
    assert_rect(node_by_id(view, centred).rect, [1.0, 1.5, 2.0, 0.5]);
    assert_rect(node_by_id(view, first).rect, [1.0, 1.5, 0.5, 0.5]);
    assert_rect(node_by_id(view, second).rect, [1.5, 1.5, 1.0, 0.25]);

    // The 1.25 x 1.25 margin box ends at (2.75, 0.75); the nested Align then
    // centres its dot by (0.25, 0.25), composing both deferred moves.
    assert_rect(node_by_id(view, ended).rect, [2.75, 0.75, 1.0, 1.0]);
    assert_rect(node_by_id(view, inner).rect, [2.75, 0.75, 1.0, 1.0]);
    assert_rect(node_by_id(view, dot).rect, [3.0, 1.0, 0.5, 0.5]);

    assert_eq!(view_hit(view, [1.2, 1.7]), first);
    assert_eq!(view_hit(view, [2.0, 1.6]), second);
    assert_eq!(view_hit(view, [3.2, 1.2]), dot);
    assert_eq!(view_hit(view, [2.8, 0.8]), inner);
    assert_eq!(view_hit(view, [0.5, 0.5]), root);
}

#[test]
fn cross_alignment_moves_nested_row_and_column_children() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);

    // Row: the tallest child sets the 1.5 cross extent, so the centred
    // column moves down by (1.5 - 0.5) / 2 with its leaf.
    let mut tree = TreeBuilder::new();
    let row = tree.add(None, container(GuiContainerKind::Row), sized(4.0, 2.0));
    tree.add(
        Some(row),
        container(GuiContainerKind::SizedBox),
        sized(0.5, 1.5),
    );
    let centred = tree.add(
        Some(row),
        container(GuiContainerKind::Column),
        aligned(1.0, 0.5, -1.0, 0.0),
    );
    let leaf = tree.add(
        Some(centred),
        container(GuiContainerKind::SizedBox),
        sized(0.5, 0.25),
    );
    let root_tree = tree.build();
    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, centred).rect, [0.5, 0.5, 1.0, 0.5]);
    assert_rect(node_by_id(view, leaf).rect, [0.5, 0.5, 0.5, 0.25]);
    assert_eq!(view_hit(view, [0.75, 0.6]), leaf);

    // Column: the widest child sets the 2.0 cross extent, so the end-aligned
    // row moves right by 2 - 1 with its leaf.
    let mut tree = TreeBuilder::new();
    let column = tree.add(None, container(GuiContainerKind::Column), sized(4.0, 2.0));
    tree.add(
        Some(column),
        container(GuiContainerKind::SizedBox),
        sized(2.0, 0.5),
    );
    let ended = tree.add(
        Some(column),
        container(GuiContainerKind::Row),
        aligned(1.0, 0.5, 1.0, -1.0),
    );
    let leaf = tree.add(
        Some(ended),
        container(GuiContainerKind::SizedBox),
        sized(0.25, 0.25),
    );
    let root_tree = tree.build();
    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    assert_rect(node_by_id(view, ended).rect, [1.0, 0.5, 1.0, 0.5]);
    assert_rect(node_by_id(view, leaf).rect, [1.0, 0.5, 0.25, 0.25]);
    assert_eq!(view_hit(view, [1.1, 0.6]), leaf);
    assert_eq!(view_hit(view, [1.5, 0.9]), ended);
}

#[test]
fn scaled_and_reflected_parents_move_aligned_subtrees() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(None, container(GuiContainerKind::Stack), sized(4.0, 2.0));
    let scaled = tree.add(
        Some(root),
        container(GuiContainerKind::Align),
        sized(1.0, 1.0),
    );
    tree.set_visual(scaled, [0.0, 0.0], [2.0, 0.5]);
    let column = tree.add(
        Some(scaled),
        container(GuiContainerKind::Column),
        sized(0.5, 0.5),
    );
    let leaf = tree.add(
        Some(column),
        container(GuiContainerKind::SizedBox),
        sized(0.25, 0.25),
    );
    let mirrored = tree.add(
        Some(root),
        container(GuiContainerKind::Align),
        sized(1.0, 1.0),
    );
    tree.set_visual(mirrored, [3.0, 1.0], [-1.0, 1.0]);
    let mirrored_leaf = tree.add(
        Some(mirrored),
        container(GuiContainerKind::SizedBox),
        sized(0.5, 0.25),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // Local centring offset (0.25, 0.25) scales by (2, 0.5) to (0.5, 0.125).
    assert_rect(node_by_id(view, scaled).rect, [0.0, 0.0, 2.0, 0.5]);
    assert_rect(node_by_id(view, column).rect, [0.5, 0.125, 1.0, 0.25]);
    assert_rect(node_by_id(view, leaf).rect, [0.5, 0.125, 0.5, 0.125]);
    assert_eq!(view_hit(view, [0.7, 0.2]), leaf);
    assert_eq!(view_hit(view, [0.3, 0.1]), scaled);

    // The mirrored box spans x 2..3 from its origin at 3. Local centring
    // (0.25, 0.375) maps through scale (-1, 1) to (-0.25, 0.375), leaving
    // the 0.5 x 0.25 leaf centred in the mirrored box.
    assert_rect(node_by_id(view, mirrored).rect, [2.0, 1.0, 1.0, 1.0]);
    assert_rect(
        node_by_id(view, mirrored_leaf).rect,
        [2.25, 1.375, 0.5, 0.25],
    );
    assert_eq!(view_hit(view, [2.5, 1.5]), mirrored_leaf);
    assert_eq!(view_hit(view, [2.1, 1.1]), mirrored);
}

/// Stack > ScrollView (2 x 1) > Align (2 x `align_height`, centred) >
/// ScrollView (1 x 1) > filled content (1 x 2). Returns the tree with the
/// ids of the root, the Align, the inner ScrollView and its content.
fn nested_scroll_tree(align_height: f32) -> (GuiRoot, [GuiNodeId; 4]) {
    let mut tree = TreeBuilder::new();
    let root = tree.add(None, container(GuiContainerKind::Stack), sized(4.0, 2.0));
    let outer = tree.add(
        Some(root),
        container(GuiContainerKind::ScrollView),
        sized(2.0, 1.0),
    );
    let align = tree.add(
        Some(outer),
        container(GuiContainerKind::Align),
        sized(2.0, align_height),
    );
    let inner = tree.add(
        Some(align),
        container(GuiContainerKind::ScrollView),
        sized(1.0, 1.0),
    );
    let content = tree.add(
        Some(inner),
        container(GuiContainerKind::SizedBox),
        GuiNodeStyle {
            background_color: Some([1.0, 0.0, 0.0, 1.0]),
            ..sized(1.0, 2.0)
        },
    );
    (tree.build(), [root, align, inner, content])
}

#[test]
fn aligned_scroll_view_intersects_its_moved_viewport_with_fixed_ancestor_clips() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let (root_tree, [root, align, inner, content]) = nested_scroll_tree(1.5);

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // Centring moves the inner viewport by (0.5, 0.25) to 0.5..1.5 x
    // 0.25..1.25; the outer viewport stays at 0..2 x 0..1. Translating an
    // already-intersected clip would wrongly extend below y = 1.
    assert_eq!(node_by_id(view, align).clip, Some([0.0, 0.0, 2.0, 1.0]));
    assert_rect(node_by_id(view, inner).rect, [0.5, 0.25, 1.0, 1.0]);
    assert_eq!(node_by_id(view, inner).clip, Some([0.0, 0.0, 2.0, 1.0]));
    let record = node_by_id(view, content);
    assert_rect(record.rect, [0.5, 0.25, 1.0, 2.0]);
    assert_eq!(record.clip, Some([0.5, 0.25, 1.5, 1.0]));
    assert!(!record.paint_suppressed);

    assert_eq!(view_hit(view, [1.0, 0.5]), content);
    assert_eq!(view_hit(view, [0.25, 0.5]), align);
    assert_eq!(view_hit(view, [1.0, 1.1]), root);

    // The content's background paints under the same resolved clip.
    let clips: Vec<_> = view
        .surface_primitives()
        .into_iter()
        .filter_map(|primitive| match primitive {
            SurfaceRenderPrimitive::Box {
                style,
                ..
            } => Some(style.clip),
            _ => None,
        })
        .collect();
    assert_eq!(clips, vec![Some([0.5, 0.25, 1.5, 1.0])]);
}

#[test]
fn disjoint_nested_viewport_suppresses_paint_and_hits() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let (root_tree, [root, _align, inner, content]) = nested_scroll_tree(3.0);

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // Centring in 2 x 3 moves the inner viewport to y 1..2, entirely below
    // the outer 0..1 viewport. Its content keeps an explicit empty clip
    // instead of escaping unclipped.
    assert_rect(node_by_id(view, inner).rect, [0.5, 1.0, 1.0, 1.0]);
    let record = node_by_id(view, content);
    assert_rect(record.rect, [0.5, 1.0, 1.0, 2.0]);
    let clip = record.clip.unwrap();
    assert!(crate::systems::surface::surface_clip_is_empty(clip));
    assert!(record.paint_suppressed);
    assert_eq!(view_hit(view, [1.0, 1.5]), root);
    assert!(view.surface_primitives().is_empty());
}

#[test]
fn visual_moves_of_aligned_text_rebuild_geometry_without_remeasuring() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(None, container(GuiContainerKind::Stack), sized(4.0, 2.0));
    let panel = tree.add(
        Some(root),
        container(GuiContainerKind::Row),
        aligned(2.0, 1.0, 1.0, 0.0),
    );
    let text = tree.add(
        Some(panel),
        GuiNodeContent::Text("A".to_owned()),
        text_style(),
    );
    let mut root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &resolver);

    // End x and centred y within 4 x 2: (2, 0.5).
    assert_rect(node_by_id(view, text).rect, [2.0, 0.5, ADV_A, LINE]);
    assert_eq!(view.remeasure_count, 1);
    assert_eq!(view.reflow_count, 1);

    let position = GuiRoot::property_name(panel, "position").unwrap();
    root_tree
        .properties
        .set(&position, crate::DynamicValue::Vec2([0.5, 0.0]))
        .unwrap();
    let view = cache.evaluate(entity(), &request(&root_tree, 2), &resolver);

    assert_rect(node_by_id(view, panel).rect, [2.5, 0.5, 2.0, 1.0]);
    assert_rect(node_by_id(view, text).rect, [2.5, 0.5, ADV_A, LINE]);
    assert_eq!(node_by_id(view, text).content_origin, [2.5, 0.5]);
    assert_eq!(view.remeasure_count, 1);
    assert_eq!(view.reflow_count, 1);
}
