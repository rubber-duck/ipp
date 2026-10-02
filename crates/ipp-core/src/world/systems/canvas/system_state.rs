use super::walk::Placement;
use super::{CanvasGlyph, CanvasInteractionPriority, CanvasPaintEntry, CanvasPrimitiveStyle};
use super::{CanvasPublication, CanvasWork};
use crate::EntityId;
use crate::services::asset_management::AssetKey;
use crate::systems::gui::layout::scroll_bars::GuiScrollBar;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Default)]
pub(super) struct CanvasSystemState {
    /// Committed extent and density, changed only at the mutation boundary.
    pub canvas: super::CanvasState,
    /// Extent of the latest evaluation, with its tick.
    pub evaluated: Option<super::CanvasEvaluatedExtent>,
    /// The World canvas's latest output; absent while it cannot be evaluated.
    pub publication: Option<CanvasPublication>,
    /// Tree order: depth-first over the top-level entities in sibling order.
    pub order: Vec<EntityId>,
    pub leaves: BTreeMap<(EntityId, u16), CanvasPreparedLeaf>,
    pub slots: BTreeMap<EntityId, Arc<CanvasPaintEntry>>,
    /// A change the canvas cannot patch: the next evaluation walks it whole.
    pub dirty: bool,
    pub geometry_dirty: BTreeSet<(EntityId, u16)>,
    /// Changes that affect neither structure, layout nor layers, which the
    /// next evaluation patches unless something requires the whole walk.
    pub patch: CanvasPatchInputs,
    /// The latest walk, which a patch re-enters.
    pub walk: CanvasWalkState,
    pub tree_dirty: bool,
    pub initialized: bool,
    pub revision: u64,
    pub layout_revision: u64,
    pub gui: crate::systems::gui::presentation::GuiCanvasState,
    pub gui_revision: u64,
    pub gui_dirty: bool,
    /// Evaluated bounds of the latest pass, written to `CanvasBounds`.
    pub bounds: Vec<(EntityId, [f32; 4])>,
    /// Work of the latest evaluation.
    pub work: CanvasWork,
}

impl CanvasSystemState {
    pub fn next_revision(&mut self) -> u64 {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("Canvas revision exhausted");
        self.revision
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CanvasPreparedLeaf {
    pub layout_size: Option<[f32; 2]>,
    pub incarnation: u64,
    pub geometry: Option<CanvasGeometry>,
    pub style: CanvasPrimitiveStyle,
    /// The CanvasPaint lifetime the leaf's box took, when it is a painted box.
    pub paint: Option<super::CanvasTarget>,
    pub geometry_revision: u64,
    pub entry: Option<Arc<CanvasPaintEntry>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::world::systems) enum CanvasGeometry {
    Glyphs {
        font: AssetKey,
        font_size: f32,
        glyphs: Arc<[CanvasGlyph]>,
        size: [f32; 2],
    },
    Drawing {
        drawing: AssetKey,
    },
    Bitmap {
        bitmap: AssetKey,
        size: [f32; 2],
    },
    Box {
        size: [f32; 2],
        corner_radius: [f32; 2],
    },
}

/// Changed entities whose effects a patch can apply: a style, a leaf value or
/// a sampled GUI transition re-walks the entity's subtree, and a paint
/// property write replaces only its paint instance.
#[derive(Clone, Default)]
pub(super) struct CanvasPatchInputs {
    pub subtrees: BTreeSet<EntityId>,
    pub paints: BTreeSet<EntityId>,
}

impl CanvasPatchInputs {
    pub fn is_empty(&self) -> bool {
        self.subtrees.is_empty() && self.paints.is_empty()
    }

    pub fn clear(&mut self) {
        self.subtrees.clear();
        self.paints.clear();
    }
}

/// Output lengths in tree order: entries, hits, controls and overlays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CanvasWalkMarks {
    pub entries: u32,
    pub hits: u32,
    pub controls: u32,
    pub overlays: u32,
}

/// What the walk derived for one entity: what its descendants inherit and
/// where its subtree's output lies.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CanvasWalkRecord {
    pub entity: EntityId,
    /// Its own placement, on its resolved layer.
    pub placed: Placement,
    /// The placement its children inherit: its own, clipped to its viewport
    /// when it scrolls.
    pub inherited: Placement,
    /// Its scroll bars, which nested scroll views avoid.
    pub bars: Vec<GuiScrollBar>,
    /// Nearest focus scope at or above it.
    pub scope: Option<EntityId>,
    /// Whether it lies in an open hint, which is inert to input.
    pub inert: bool,
    /// Tree-order output before its subtree and after it, including the scroll
    /// parts it paints over its descendants.
    pub start: CanvasWalkMarks,
    pub end: CanvasWalkMarks,
    /// Walk position after its subtree.
    pub subtree_end: usize,
    /// Interaction its control requests while eligible.
    pub interaction: CanvasInteractionPriority,
}

/// The latest walk of the canvas, in tree order over the shown entities.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct CanvasWalkState {
    pub positions: BTreeMap<EntityId, usize>,
    pub records: Vec<CanvasWalkRecord>,
    /// Published index of each tree-order entry, hit and overlay; empty while
    /// the canvas publishes in tree order, on one layer.
    pub entry_order: Vec<u32>,
    pub hit_order: Vec<u32>,
    pub overlay_order: Vec<u32>,
    /// Eligible controls requesting each interaction: focused, hovered,
    /// pressed and captured.
    pub interaction: [u32; 4],
}

impl CanvasWalkState {
    pub fn entry(&self, index: usize) -> usize {
        self.entry_order.get(index).map_or(index, |&at| at as usize)
    }

    pub fn hit(&self, index: usize) -> usize {
        self.hit_order.get(index).map_or(index, |&at| at as usize)
    }

    pub fn overlay(&self, index: usize) -> usize {
        self.overlay_order
            .get(index)
            .map_or(index, |&at| at as usize)
    }

    pub fn priority(&self) -> CanvasInteractionPriority {
        CanvasInteractionPriority {
            focused: self.interaction[0] > 0,
            hovered: self.interaction[1] > 0,
            pressed: self.interaction[2] > 0,
            captured: self.interaction[3] > 0,
        }
    }
}

/// Interaction requests as counts: focused, hovered, pressed and captured.
pub(super) fn interaction_counts(priority: CanvasInteractionPriority) -> [u32; 4] {
    [
        priority.focused,
        priority.hovered,
        priority.pressed,
        priority.captured,
    ]
    .map(u32::from)
}
