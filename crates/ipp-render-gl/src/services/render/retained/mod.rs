//! Retained GUI presentation: shape records and batches, the glyph atlas and its
//! batches, analytic glyph streams, per-Surface storage and draw order.

pub(super) mod analytic_glyphs;
pub(super) mod box_records;
pub(super) mod draw_order;
pub(super) mod glyph_atlas;
pub(super) mod records;
pub(super) mod shape_batches;
mod storage;
pub(super) mod surface_paint;
