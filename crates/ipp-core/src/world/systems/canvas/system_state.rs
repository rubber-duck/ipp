use super::{CanvasGlyph, CanvasPaintEntry, CanvasPrimitiveStyle, CanvasPublication};
use crate::EntityId;
use crate::services::asset_management::AssetKey;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct CanvasSystemState {
    /// Committed extent and density, changed only at the mutation boundary.
    pub canvas: super::CanvasState,
    /// Extent of the latest evaluation, with its tick.
    pub evaluated: Option<super::CanvasEvaluatedExtent>,
    /// The World canvas's latest output; absent while it cannot be evaluated.
    pub publication: Option<CanvasPublication>,
    /// Painter order: depth-first over the top-level entities in sibling order.
    pub order: Vec<EntityId>,
    pub leaves: BTreeMap<(EntityId, u16), CanvasPreparedLeaf>,
    pub slots: BTreeMap<EntityId, Arc<CanvasPaintEntry>>,
    pub dirty: bool,
    pub geometry_dirty: BTreeSet<(EntityId, u16)>,
    pub tree_dirty: bool,
    pub initialized: bool,
    pub revision: u64,
    #[cfg(feature = "gui")]
    pub layout_revision: u64,
    #[cfg(feature = "gui")]
    pub gui: crate::systems::gui::presentation::GuiCanvasState,
    #[cfg(feature = "gui")]
    pub gui_revision: u64,
    #[cfg(feature = "gui")]
    pub gui_dirty: bool,
    /// Evaluated bounds of the latest pass, written to `CanvasBounds`.
    #[cfg(feature = "gui")]
    pub bounds: Vec<(EntityId, [f32; 4])>,
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

pub(super) struct CanvasPreparedLeaf {
    pub layout_size: Option<[f32; 2]>,
    pub incarnation: u64,
    pub geometry: Option<CanvasGeometry>,
    pub style: CanvasPrimitiveStyle,
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
