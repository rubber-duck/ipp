//! Tests for retained GUI layout caching and remeasure avoidance.

use super::super::super::test_support::test_font;
use super::test_support::*;
use super::*;
use crate::DynamicValue;
use crate::EntityId;
use crate::services::asset_management::font::FontAsset;
use crate::services::asset_management::{AssetKey, AssetSource};
use crate::systems::surface::SurfaceRenderResource;
use std::collections::BTreeSet;

#[test]
fn unchanged_frames_do_no_remeasure_work() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    tree.add(
        Some(root),
        GuiNodeContent::Text("Ae".to_owned()),
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();
    assert_eq!(first.reflow_count, 1);
    assert_eq!(first.remeasure_count, 1);

    let second = cache
        .evaluate(entity(), &request(&root_tree, 2), &resolver)
        .clone();
    assert_eq!(second.evaluation_tick, 2);
    assert_eq!(second.layout_revision, first.layout_revision);
    assert_eq!(second.paint_revision, first.paint_revision);
    assert_eq!(second.reflow_count, 1);
    assert_eq!(second.remeasure_count, 1);
    assert_eq!(second.nodes, first.nodes);
}

#[test]
fn paint_only_edits_skip_remeasure() {
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
    let mut root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();

    // Recolour the leaf: paint revision advances, layout does not reflow
    // and text is never remeasured.
    let mut recoloured = root_tree.style(leaf).unwrap();
    recoloured.background_color = Some([1.0, 0.0, 0.0, 1.0]);
    root_tree.install_node_style(leaf, &recoloured).unwrap();

    let second = cache
        .evaluate(entity(), &request(&root_tree, 2), &resolver)
        .clone();
    assert_eq!(second.layout_revision, first.layout_revision);
    assert_eq!(second.paint_revision, first.paint_revision + 1);
    assert_eq!(second.reflow_count, 1);
    assert_eq!(second.remeasure_count, 1);
    assert_rect(
        second.nodes[first.nodes.len() - 1].rect,
        first.nodes[first.nodes.len() - 1].rect,
    );
    assert_eq!(
        node_by_id(&second, leaf).background,
        Some([1.0, 0.0, 0.0, 1.0])
    );

    // Paint now carries a background box for the recoloured leaf.
    let boxes = second
        .surface_primitives()
        .iter()
        .filter(|primitive| {
            matches!(
                primitive,
                crate::systems::surface::SurfaceRenderPrimitive::Box { .. }
            )
        })
        .count();
    assert_eq!(boxes, 1);
    let _ = root;
}

#[test]
fn skin_part_edits_advance_paint_without_reflow_or_remeasure() {
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
    let mut root_tree = tree.build();
    let part = GuiRoot::part_property_name(leaf, "background", "color").unwrap();
    root_tree
        .properties
        .set(&part, DynamicValue::Vec4([0.0, 0.0, 1.0, 1.0]))
        .unwrap();

    let mut cache = GuiLayoutCache::default();
    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();

    // A reskin touches only named part lanes: paint revision advances
    // with no reflow and no remeasure, so render preparation re-skins
    // without any unrelated trigger.
    root_tree
        .properties
        .set(&part, DynamicValue::Vec4([1.0, 0.0, 0.0, 1.0]))
        .unwrap();

    let second = cache
        .evaluate(entity(), &request(&root_tree, 2), &resolver)
        .clone();
    assert_eq!(second.layout_revision, first.layout_revision);
    assert_eq!(second.paint_revision, first.paint_revision + 1);
    assert_eq!(second.reflow_count, 1);
    assert_eq!(second.remeasure_count, 1);
    let _ = root;
}

#[test]
fn material_part_edits_advance_paint_without_reflow_or_remeasure() {
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
    let mut root_tree = tree.build();
    let glow_intensity = GuiRoot::part_property_name(leaf, "background", "glow_intensity").unwrap();
    root_tree
        .properties
        .set(&glow_intensity, DynamicValue::F32(1.0))
        .unwrap();

    let mut cache = GuiLayoutCache::default();
    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();

    // Change material lane: glow_intensity advances paint revision without reflow.
    root_tree
        .properties
        .set(&glow_intensity, DynamicValue::F32(2.5))
        .unwrap();

    let second = cache
        .evaluate(entity(), &request(&root_tree, 2), &resolver)
        .clone();
    assert_eq!(second.layout_revision, first.layout_revision);
    assert_eq!(second.paint_revision, first.paint_revision + 1);
    assert_eq!(second.reflow_count, 1);
    assert_eq!(second.remeasure_count, 1);

    // Change gradient color lane advances paint revision without reflow.
    let grad_color = GuiRoot::part_property_name(leaf, "background", "gradient_color0").unwrap();
    root_tree
        .properties
        .set(&grad_color, DynamicValue::Vec4([0.2, 0.4, 0.6, 1.0]))
        .unwrap();

    let third = cache
        .evaluate(entity(), &request(&root_tree, 3), &resolver)
        .clone();
    assert_eq!(third.layout_revision, first.layout_revision);
    assert_eq!(third.paint_revision, second.paint_revision + 1);
    assert_eq!(third.reflow_count, 1);
    assert_eq!(third.remeasure_count, 1);
    let _ = root;
}

#[test]
fn insert_and_reorder_invalidate_dependents() {
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
    let mut root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let view = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();
    assert_eq!(view.reflow_count, 1);

    // Insert a sibling: dependent output rebuilds.
    let id = GuiNodeId(root_tree.nodes().next_node_id());
    root_tree
        .nodes_mut()
        .insert_node(
            id,
            Some(root),
            0,
            GuiNodeContent::Container(GuiContainerKind::SizedBox),
        )
        .unwrap();
    root_tree.install_node_style(id, &sized(1.0, 1.0)).unwrap();
    let view = cache
        .evaluate(entity(), &request(&root_tree, 2), &resolver)
        .clone();
    assert_eq!(view.reflow_count, 2);
    // The inserted node leads, pushing the older sibling down.
    assert_rect(node_by_id(&view, id).rect, [0.0, 0.0, 1.0, 1.0]);
    assert_rect(node_by_id(&view, first).rect, [0.0, 1.0, 1.0, 1.0]);
}

#[test]
fn incarnation_change_rebuilds_retained_text() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        text_style(),
    );
    tree.add(
        Some(root),
        GuiNodeContent::Text("A".to_owned()),
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();
    assert_eq!(first.remeasure_count, 1);

    // Same entity, new incarnation: measurements rebuild before reuse.
    let replaces = GuiLayoutRequest {
        root_incarnation: 8,
        evaluation_tick: 2,
        ..request(&root_tree, 2)
    };
    let second = cache.evaluate(entity(), &replaces, &resolver).clone();
    assert_eq!(second.root_incarnation, 8);
    assert_eq!(second.remeasure_count, 2);
    assert_eq!(second.reflow_count, 2);
}

#[test]
fn identical_valid_root_recovers_after_invalid_input() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let mut tree = TreeBuilder::new();
    let control = tree.add(
        None,
        GuiNodeContent::Checkbox {
            checked: false,
        },
        GuiNodeStyle {
            width: Some(1.0),
            height: Some(1.0),
            background_color: Some([0.2, 0.3, 0.4, 1.0]),
            ..Default::default()
        },
    );
    let root_tree = tree.build();
    let mut cache = GuiLayoutCache::default();

    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();
    assert!(first.available);
    assert_eq!(first.hit_test([0.5, 0.5]).unwrap().node, control);
    assert!(!first.surface_primitives().is_empty());

    let invalid = GuiLayoutRequest {
        surface_size: [0.0, 2.0],
        evaluation_tick: 2,
        ..request(&root_tree, 2)
    };
    let unavailable = cache.evaluate(entity(), &invalid, &resolver).clone();
    assert!(!unavailable.available);
    assert_eq!(unavailable.root_bounds, [0.0; 4]);
    assert!(unavailable.hit_test([0.5, 0.5]).is_none());
    assert!(unavailable.surface_primitives().is_empty());
    assert!(unavailable.paint_revision > first.paint_revision);

    let recovered = cache
        .evaluate(entity(), &request(&root_tree, 3), &resolver)
        .clone();
    assert!(recovered.available);
    assert_eq!(recovered.root_bounds, first.root_bounds);
    assert_eq!(recovered.hit_test([0.5, 0.5]).unwrap().node, control);
    assert!(!recovered.surface_primitives().is_empty());
    assert!(!recovered.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        GuiLayoutDiagnostic::InvalidConstraints {
            node: None,
            detail: GuiConstraintError::InvalidRoot,
        }
    )));
    assert_eq!(recovered.reflow_count, first.reflow_count + 1);
}

#[test]
fn cache_drop_and_retain_cover_lifecycle() {
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
    cache.evaluate(entity(), &request(&root_tree, 1), &resolver);
    assert_eq!(cache.len(), 1);
    assert!(cache.paint_revision(entity()).is_some());

    let other = EntityId::from_bits(2);
    let mut live = BTreeSet::new();
    live.insert(other);
    cache.retain_entities(&live);
    assert!(cache.is_empty());
    assert!(cache.view(entity()).is_none());
    assert!(!cache.remove_entity(entity()));
}

#[test]
fn visual_only_edits_rebuild_without_reflow_or_remeasure() {
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let build = |offset: [f32; 2], scale: [f32; 2]| {
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
        tree.set_visual(middle, offset, scale);
        let leaf = tree.add(
            Some(middle),
            GuiNodeContent::Text("A".to_owned()),
            text_style(),
        );
        tree.set_visual(leaf, [0.0, 0.0], [1.0, 0.5]);
        (tree.build(), middle, leaf)
    };

    let mut cache = GuiLayoutCache::default();
    let (root_tree, _, leaf) = build([0.0, 0.0], [2.0, 1.0]);
    let first = cache
        .evaluate(entity(), &request(&root_tree, 1), &resolver)
        .clone();
    assert_rect(node_by_id(&first, leaf).rect, [0.0, 0.0, 0.12, 0.06]);
    assert_eq!(first.remeasure_count, 1);

    // A visual-only edit: new offset and scale on the middle node.
    let (root_tree, _, leaf) = build([0.5, 0.25], [0.5, 4.0]);
    let second = cache
        .evaluate(entity(), &request(&root_tree, 2), &resolver)
        .clone();
    assert_eq!(second.layout_revision, first.layout_revision);
    assert_eq!(second.reflow_count, first.reflow_count);
    assert_eq!(second.remeasure_count, first.remeasure_count);
    assert_eq!(second.paint_revision, first.paint_revision + 1);
    // Accumulated [0.5, 2.0] over the leaf: origin follows the scaled
    // placement and extents follow the chained scales.
    assert_rect(node_by_id(&second, leaf).rect, [0.5, 0.25, 0.03, 0.24]);
    assert_eq!(view_hit(&second, [0.51, 0.3]), leaf);
}

/// Generation-reporting resolver for readiness recovery: pending and ready
/// share the authored reference while the epoch bump invalidates retained
/// output, mirroring the production slot-generation input.
struct EpochResolver<'a> {
    font: Option<&'a FontAsset>,
    pending: bool,
    epoch: u64,
}

impl GuiResourceResolver for EpochResolver<'_> {
    fn text_font(&self, source: &AssetSource) -> GuiFontResolution<'_> {
        if source.uri != "test-font" {
            return GuiFontResolution::Missing;
        }
        match self.font {
            Some(font) => GuiFontResolution::Ready {
                key: AssetKey {
                    slot: 3,
                    generation: 1,
                },
                font,
            },
            None if self.pending => GuiFontResolution::Pending {
                key: AssetKey {
                    slot: 3,
                    generation: 1,
                },
            },
            None => GuiFontResolution::Missing,
        }
    }

    fn surface_resource(&self, _source: &AssetSource) -> Option<SurfaceRenderResource> {
        None
    }

    fn resource_generation(&self, source: &AssetSource) -> Option<u64> {
        (source.uri == "test-font").then_some(self.epoch)
    }
}

#[test]
fn pending_font_recovers_without_unrelated_edits() {
    let font = test_font();
    let mut tree = TreeBuilder::new();
    let root = tree.add(
        None,
        GuiNodeContent::Container(GuiContainerKind::Column),
        GuiNodeStyle::default(),
    );
    let leaf = tree.add(
        Some(root),
        GuiNodeContent::Text("AVa".to_owned()),
        text_style(),
    );
    let root_tree = tree.build();

    let mut cache = GuiLayoutCache::default();
    // Pending on first evaluation: unavailable with a pending diagnostic.
    let pending = EpochResolver {
        font: None,
        pending: true,
        epoch: 1,
    };
    let view = cache.evaluate(entity(), &request(&root_tree, 1), &pending);
    assert!(!node_by_id(view, leaf).available);
    assert!(
        view.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            GuiLayoutDiagnostic::PendingText {
                node,
            } if *node == leaf
        )),
        "diagnostics: {:?}",
        view.diagnostics
    );
    let reflows = view.reflow_count;

    // The same reference readies with a bumped generation and no GUI edit:
    // retained output rebuilds and only the affected branch remeasures.
    let ready = EpochResolver {
        font: Some(&font),
        pending: false,
        epoch: 2,
    };
    let view = cache.evaluate(entity(), &request(&root_tree, 2), &ready);
    assert!(node_by_id(view, leaf).available);
    assert!(
        !view
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic, GuiLayoutDiagnostic::PendingText { .. })),
        "diagnostics: {:?}",
        view.diagnostics
    );
    assert_eq!(view.reflow_count, reflows + 1);
    assert_eq!(view.remeasure_count, 1);

    // A stable generation with no edits does no further work.
    let view = cache.evaluate(entity(), &request(&root_tree, 3), &ready);
    assert_eq!(view.reflow_count, reflows + 1);
    assert_eq!(view.remeasure_count, 1);
    assert!(node_by_id(view, leaf).available);
}
