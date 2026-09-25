//! Near-linear growth of GUI tree work with node count (ipp-9nx.69).
//!
//! Flat and deep roots of 1k, 4k, 8k and 16k nodes measure the whole-root
//! insertion check, deriving child order, a full layout, a paint-only
//! refresh, and removing the root or half the tree through the retiring row
//! write plus the derived-order update. Flat roots hold sized Column leaves,
//! half of them under one Column; deep roots are one chain of Columns, which
//! layout evaluates to [`MAX_LAYOUT_DEPTH`](super::MAX_LAYOUT_DEPTH).
//!
//! Each step asserts growth from 1k to 16k nodes stays within three times
//! linear (a quadratic step grows 256 times), and prints the minimum of three
//! runs (`cargo test -p ipp-core --features gui --lib tree_scaling --
//! --nocapture`).

use super::*;
use crate::ComponentValue;
use crate::components::registry;
use crate::services::asset_management::AssetSource;
use crate::systems::gui::{
    DEFAULT_UNITS_PER_METRE, GuiContainerKind, GuiFontResolution, GuiLayoutCache, GuiLayoutRequest,
    GuiNodeStyle, GuiNodeTreeProperty, GuiResourceResolver, GuiTreeIndex,
};
use std::time::{Duration, Instant};

const SIZES: [u32; 4] = [1_000, 4_000, 8_000, 16_000];

struct NoResources;

impl GuiResourceResolver for NoResources {
    fn text_font(&self, _source: &AssetSource) -> GuiFontResolution<'_> {
        GuiFontResolution::Missing
    }

    fn surface_resource(
        &self,
        _source: &AssetSource,
    ) -> Option<crate::systems::surface::SurfaceRenderResource> {
        None
    }
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    Flat,
    Deep,
}

/// Root of `count` Column nodes of `shape`, and the node whose subtree
/// holds about half of them.
fn build(shape: Shape, count: u32) -> (GuiRoot, GuiNodeId) {
    let mut root = GuiRoot::default();
    let column = || GuiNodeData::Container(GuiContainerKind::Column);
    let leaf = GuiNodeStyle {
        height: Some(0.001),
        ..GuiNodeStyle::default()
    };
    root.insert_node_at(
        GuiNodeId(1),
        None,
        0,
        column(),
        Default::default(),
        &GuiNodeStyle::default(),
    )
    .unwrap();
    let half = count / 2;
    for id in 2..=count {
        let parent = match shape {
            Shape::Flat if id > half + 1 => half + 1,
            Shape::Flat => 1,
            Shape::Deep => id - 1,
        };
        root.insert_node_at(
            GuiNodeId(id),
            Some(GuiNodeId(parent)),
            id << 8,
            column(),
            Default::default(),
            &leaf,
        )
        .unwrap();
    }
    let subtree = match shape {
        Shape::Flat => half + 1,
        Shape::Deep => half,
    };
    (root, GuiNodeId(subtree))
}

fn request<'a>(root: &'a GuiRoot, tree: &'a GuiTreeIndex, tick: u64) -> GuiLayoutRequest<'a> {
    GuiLayoutRequest {
        root,
        tree,
        root_incarnation: 1,
        surface_size: [10.0, 10.0],
        units_per_metre: DEFAULT_UNITS_PER_METRE,
        evaluation_tick: tick,
    }
}

/// Retire `node` of a staged root through its row write, as a RemoveNode
/// command does, and update the derived order; returns the live node count
/// left.
fn retire(mut value: ComponentValue, tree: &mut GuiTreeIndex, node: GuiNodeId) -> usize {
    let write = crate::FieldWrite {
        offset: GuiRoot::node_tree_offset(node, GuiNodeTreeProperty::Parent).unwrap(),
        value: crate::FieldValue::Unset,
    };
    registry::write(&mut value, &write).unwrap();
    let ComponentValue::GuiRoot(retired) = &value else {
        unreachable!("GUI root writes keep the component type")
    };
    tree.update(retired, node);
    retired.nodes().len()
}

/// Minimum duration of `run` over three runs, each on fresh input from
/// `prepare`.
fn fastest<T>(mut prepare: impl FnMut() -> T, mut run: impl FnMut(T)) -> Duration {
    (0..3)
        .map(|_| {
            let input = prepare();
            let start = Instant::now();
            run(input);
            start.elapsed()
        })
        .min()
        .unwrap()
}

/// Durations of every step for one root.
fn measure(shape: Shape, count: u32) -> [(&'static str, Duration); 6] {
    let (root, subtree) = build(shape, count);
    let tree = GuiTreeIndex::new(&root, 1);
    let entity = EntityId::from_bits(3);

    let validate = fastest(|| (), |_| root.validate_complete().unwrap());
    let derive = fastest(|| (), |_| drop(GuiTreeIndex::new(&root, 1)));
    let layout = fastest(GuiLayoutCache::default, |mut cache| {
        cache.evaluate(entity, &request(&root, &tree, 1), &NoResources);
    });

    let mut repainted = root.clone();
    repainted
        .update_node(
            GuiNodeId(count),
            &GuiNodePatch {
                color: Some([0.5, 0.5, 0.5, 1.0]),
                ..GuiNodePatch::default()
            },
        )
        .unwrap();
    let paint = fastest(
        || {
            let mut cache = GuiLayoutCache::default();
            cache.evaluate(entity, &request(&root, &tree, 1), &NoResources);
            cache
        },
        |mut cache| {
            let view = cache.evaluate(entity, &request(&repainted, &tree, 2), &NoResources);
            assert_eq!(view.reflow_count, 1, "a colour edit only repaints");
        },
    );

    let staged = || (ComponentValue::GuiRoot(root.clone()), tree.clone());
    let remove_root = fastest(staged, |(value, mut tree)| {
        assert_eq!(retire(value, &mut tree, GuiNodeId(1)), 0);
        assert!(tree.is_empty());
    });
    let remove_subtree = fastest(staged, |(value, mut tree)| {
        let left = retire(value, &mut tree, subtree);
        assert!(left <= count as usize / 2 + 1);
        assert_eq!(tree.len(), left);
    });

    [
        ("insertion validation", validate),
        ("child order derivation", derive),
        ("full layout", layout),
        ("paint-only refresh", paint),
        ("root removal", remove_root),
        ("half-tree removal", remove_subtree),
    ]
}

#[test]
fn tree_work_grows_near_linearly_with_node_count() {
    for shape in [Shape::Flat, Shape::Deep] {
        let results: Vec<_> = SIZES.iter().map(|&count| measure(shape, count)).collect();
        eprintln!("{shape:?} GUI tree (debug build unless built with --release):");
        eprintln!(
            "{:<24} {:>12} {:>12} {:>12} {:>12}",
            "step", "1,000", "4,000", "8,000", "16,000"
        );
        for step in 0..6 {
            let name = results[0][step].0;
            let times: Vec<Duration> = results.iter().map(|result| result[step].1).collect();
            eprintln!(
                "{name:<24} {:>12.2?} {:>12.2?} {:>12.2?} {:>12.2?}",
                times[0], times[1], times[2], times[3]
            );
            let scale = SIZES[3] / SIZES[0];
            let bound = times[0] * scale * 3 + Duration::from_millis(5);
            assert!(
                times[3] <= bound,
                "{shape:?} {name} grew from {:?} to {:?} over {scale}x nodes",
                times[0],
                times[3]
            );
        }
    }
}
