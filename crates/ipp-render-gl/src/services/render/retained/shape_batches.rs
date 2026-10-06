//! Retained GUI shape batches and their per-Surface GPU storage.
//!
//! GUI controls emit parameterized box primitives. RenderService retains CPU
//! [shape records](super::records::GuiShapeRecord) keyed by live primitive identity
//! and content revision, and groups each Surface's boxes into bounded batches. Shape
//! batches occupy the Surface's [shape storage](super::storage) and atlas text
//! batches its glyph storage, each in painter order, and a run of consecutive GUI work
//! draws its shapes and then its glyphs in the [GUI draw order](super::draw_order),
//! whatever its clips. Warm unchanged frames upload zero bytes. Changing a control
//! rewrites only the affected shape batch and editing text only its glyph batches.
//! Culled Surfaces keep their retained work; destruction, or a submission that no
//! longer uses a batch, releases it.
//!
//! Batch boundaries within a run of boxes follow primitive identities rather than
//! positions, so inserting, removing or resizing a box rewrites only its own batch.
//! A box whose geometry changed recently is volatile and batches apart from stable
//! boxes, so an animated control rewrites only its own small batch each frame.
//! Volatility is the only partition: backgrounds, fills, icons and focus rings share
//! batches. Each record carries its primitive's clip, so a clip change rewrites the
//! batches of the boxes it clips.
//!
//! A recoverable storage allocation or write failure releases the failed storage and
//! reports the Surface as unretained: the caller skips its boxes and draws its text
//! analytically while the rest of the frame continues. The Surface retries after
//! [`STORAGE_RETRY_FRAMES`] frames, doubling the wait after each further failure up to
//! [`MAX_STORAGE_RETRY_DOUBLINGS`] times, so a persistent failure is not paid every
//! frame. Success, context recovery and the Surface's release reset the back-off;
//! context loss still fails the frame so recovery runs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::super::canvas::paint::CanvasPaintBlocks;
use super::box_records::{clipped_bounds, hash_painted_box_inputs, painted_box_records};
use super::draw_order::{GuiDrawItem, GuiDrawOrder};
use super::records::{GuiGlyphRecord, GuiRecordKind, GuiShapeRecord};
use super::storage::{
    GuiCommitScratch, GuiPiece, GuiPieceKey, GuiPieceSource, GuiSurfaceStorage,
    commit_surface_storage,
};
pub use super::surface_paint::RetainedSurfaceSubmission;
use super::surface_paint::SurfacePaint;
use crate::services::render::statistics::RenderFrameWork;
use crate::{RenderDevice, RenderError};
use ipp_core::systems::canvas::{CanvasClip, CanvasPrimitive, CanvasPrimitiveId, CanvasShapeFill};

/// Stable key identifying one live primitive's CPU geometry cache entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimitiveKey {
    /// Retained Surface key; a Canvas output uses [`CANVAS_SURFACE`](super::surface_paint::CANVAS_SURFACE).
    pub entity: ipp_core::EntityId,
    /// Live primitive identity.
    pub identity: CanvasPrimitiveId,
}

/// Boxes one retained batch holds at most.
const MAX_BATCH_BOXES: usize = 128;

/// Boxes a batch holds before an identity boundary may end it.
const MIN_BATCH_BOXES: usize = 4;

/// On average one primitive identity in this many starts a new batch.
const BATCH_BOUNDARY_PERIOD: u64 = 32;

/// Frames a box stays volatile after its geometry last changed. Longer than a caret
/// blink period, so blinking and repeated transitions stay in their own batch.
const VOLATILE_FRAMES: u64 = 120;

/// Consecutive volatile boxes one batch holds at most.
const MAX_VOLATILE_BATCH_BOXES: usize = 8;

/// Frames a Surface waits after a recoverable storage failure before it retries.
const STORAGE_RETRY_FRAMES: u64 = 4;

/// Times consecutive storage failures of a Surface double its retry wait.
const MAX_STORAGE_RETRY_DOUBLINGS: u32 = 6;

/// Retry state of a Surface whose storage allocation or write failed.
struct StorageBackoff {
    /// Frame from which the Surface may try to commit again.
    retry_frame: u64,
    /// Consecutive failures since the last successful commit.
    failures: u32,
}

/// Cached CPU geometry for one box primitive.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedPrimitiveGeometry {
    /// Content hash of all geometry, material and clip inputs.
    pub hash: u64,
    /// Surface paint revision `hash` was computed under; zero when unknown.
    revision: u64,
    /// Whether the identity may start a batch; fixed per identity.
    boundary: bool,
    /// One record per covered quad.
    pub records: Vec<GuiShapeRecord>,
    /// Paint bounds of `records` within their clip; `None` when nothing paints.
    bounds: Option<[f32; 4]>,
    /// Frame before which the box stays volatile after a geometry change.
    volatile_until: u64,
    /// Frame that last submitted the box.
    seen: u64,
}

/// One box of the run being batched.
struct RunBox {
    identity: CanvasPrimitiveId,
    hash: u64,
    volatile: bool,
    boundary: bool,
    shape: GuiShapeItem,
}

/// Records and paint bounds of one box in a shape batch, for the draw order.
#[derive(Clone, Copy, Debug)]
struct GuiShapeItem {
    records: usize,
    bounds: Option<[f32; 4]>,
}

/// A collected piece in painter order: an index into the shape or glyph pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuiPieceRef {
    Shapes(usize),
    Glyphs(usize),
}

/// Retained GPU storage of one Surface: its shape and glyph records.
struct GuiSurfaceStorages<D: RenderDevice> {
    shapes: Option<GuiSurfaceStorage<D, GuiShapeRecord>>,
    glyphs: Option<GuiSurfaceStorage<D, GuiGlyphRecord>>,
    /// Frame that last committed this storage.
    seen: u64,
}

impl<D: RenderDevice> GuiSurfaceStorages<D> {
    fn bytes(&self) -> usize {
        self.shapes.as_ref().map_or(0, GuiSurfaceStorage::bytes)
            + self.glyphs.as_ref().map_or(0, GuiSurfaceStorage::bytes)
    }

    fn delete(self, device: &mut D) {
        if let Some(shapes) = self.shapes {
            shapes.delete(device);
        }
        if let Some(glyphs) = self.glyphs {
            glyphs.delete(device);
        }
    }
}

/// Renderer-owned retained GUI geometry of one World: box CPU records and the GPU
/// storage of each Surface's shape and glyph batches.
pub struct GuiBatchRenderCache<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    cpu_primitives: BTreeMap<PrimitiveKey, CachedPrimitiveGeometry>,
    storage: BTreeMap<ipp_core::EntityId, GuiSurfaceStorages<D>>,
    /// Surfaces waiting to retry after a recoverable storage failure.
    backoff: BTreeMap<ipp_core::EntityId, StorageBackoff>,
    /// Sum of allocated bytes over `storage`.
    resident: usize,
    /// Surface whose pieces are being collected.
    entity: ipp_core::EntityId,
    /// Collected batches of that Surface in painter order.
    order: Vec<GuiPieceRef>,
    /// Shape batches in painter order.
    shape_pieces: Vec<GuiPiece>,
    /// Glyph batches in painter order.
    glyph_pieces: Vec<GuiPiece>,
    /// Box identities referenced by shape pieces.
    piece_boxes: Vec<CanvasPrimitiveId>,
    /// Records and bounds of each of `piece_boxes`.
    piece_shapes: Vec<GuiShapeItem>,
    run_boxes: Vec<RunBox>,
    shape_scratch: GuiCommitScratch<GuiShapeRecord>,
    glyph_scratch: GuiCommitScratch<GuiGlyphRecord>,
    draw_order: GuiDrawOrder,
    frame: u64,
    /// Parameter blocks and slot lanes of the Surface's custom paint instances.
    pub(crate) paints: CanvasPaintBlocks,
}

impl<D: RenderDevice> GuiBatchRenderCache<D> {
    /// Create a new empty batch render cache bound to the given device.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            cpu_primitives: BTreeMap::new(),
            storage: BTreeMap::new(),
            backoff: BTreeMap::new(),
            resident: 0,
            entity: ipp_core::EntityId::from_bits(0),
            order: Vec::new(),
            shape_pieces: Vec::new(),
            glyph_pieces: Vec::new(),
            piece_boxes: Vec::new(),
            piece_shapes: Vec::new(),
            run_boxes: Vec::new(),
            shape_scratch: GuiCommitScratch::default(),
            glyph_scratch: GuiCommitScratch::default(),
            draw_order: GuiDrawOrder::default(),
            frame: 0,
            paints: CanvasPaintBlocks::default(),
        }
    }

    /// Clear all retained GPU storage and CPU geometry cache entries.
    pub fn clear(&mut self) {
        let mut device = self.device.borrow_mut();
        for (_, storage) in std::mem::take(&mut self.storage) {
            storage.delete(&mut device);
        }
        self.cpu_primitives.clear();
        self.backoff.clear();
        self.resident = 0;
        self.order.clear();
        self.shape_pieces.clear();
        self.glyph_pieces.clear();
        self.piece_boxes.clear();
        self.piece_shapes.clear();
        self.run_boxes.clear();
        self.paints = CanvasPaintBlocks::default();
    }

    /// Total resident bytes occupied by retained GPU storage.
    pub fn resident_bytes(&self) -> usize {
        self.resident
    }

    /// Start collecting the retained batches of one Surface submission.
    pub fn begin_surface(&mut self, entity: ipp_core::EntityId) {
        self.entity = entity;
        self.order.clear();
        self.shape_pieces.clear();
        self.glyph_pieces.clear();
        self.piece_boxes.clear();
        self.piece_shapes.clear();
    }

    /// Batches collected for the current Surface so far.
    pub fn piece_count(&self) -> usize {
        self.order.len()
    }

    /// Append one contiguous run of box primitives, each with its effective clip and
    /// its entry's paint, as bounded batches.
    ///
    /// Stable boxes split at identity-selected boundaries; volatile boxes form their own
    /// small batches. Boxes whose retained hash was computed under a revision their
    /// `paint` reuses are not hashed again. A custom paint fill takes its instance's
    /// lanes from [`Self::paints`], and draws its colour solidly without them.
    pub fn push_boxes(
        &mut self,
        boxes: &[(&CanvasPrimitive, CanvasClip, SurfacePaint)],
        stats: &mut RenderFrameWork,
    ) {
        // Refresh every box's geometry and volatility before choosing batch boundaries.
        self.run_boxes.clear();
        for &(primitive, clip, paint) in boxes {
            let CanvasPrimitive::Box {
                style,
                size,
                corner_radius,
                border_width,
                border_color,
                fill,
                glow,
                shape,
            } = primitive
            else {
                continue;
            };

            let mut adjusted = *style;
            adjusted.opacity *= paint.opacity;
            let style = &adjusted;
            let lanes = match fill {
                CanvasShapeFill::Paint {
                    paint,
                    ..
                } => self.paints.lanes(*paint),
                _ => None,
            };
            let hash = || {
                hash_painted_box_inputs(
                    style,
                    size,
                    corner_radius,
                    *border_width,
                    border_color,
                    fill,
                    glow.as_ref(),
                    shape,
                    clip,
                    lanes,
                )
            };
            let generate = || {
                let records = painted_box_records(
                    style,
                    size,
                    corner_radius,
                    *border_width,
                    border_color,
                    fill,
                    glow.as_ref(),
                    shape,
                    clip,
                    lanes,
                );
                let bounds = shape_bounds(&records, clip);
                (records, bounds)
            };
            let key = PrimitiveKey {
                entity: self.entity,
                identity: style.identity,
            };
            let cached = match self.cpu_primitives.entry(key) {
                Entry::Occupied(entry) => {
                    let cached = entry.into_mut();
                    if !paint.reuses(cached.revision) {
                        stats.statistics.gui_hashes += 1;
                        let hash = hash();
                        if cached.hash != hash {
                            cached.hash = hash;
                            (cached.records, cached.bounds) = generate();
                            cached.volatile_until = self.frame + VOLATILE_FRAMES;
                            stats.statistics.gui_rebuilds += 1;
                        }
                    }
                    cached.revision = paint.revision;
                    cached
                }
                Entry::Vacant(entry) => {
                    stats.statistics.gui_hashes += 1;
                    stats.statistics.gui_rebuilds += 1;
                    let (records, bounds) = generate();
                    entry.insert(CachedPrimitiveGeometry {
                        hash: hash(),
                        revision: paint.revision,
                        boundary: identity_starts_batch(style.identity),
                        records,
                        bounds,
                        volatile_until: 0,
                        seen: self.frame,
                    })
                }
            };
            cached.seen = self.frame;

            self.run_boxes.push(RunBox {
                identity: style.identity,
                hash: cached.hash,
                volatile: cached.volatile_until > self.frame,
                boundary: cached.boundary,
                shape: GuiShapeItem {
                    records: cached.records.len(),
                    bounds: cached.bounds,
                },
            });
        }

        let mut start = 0;
        for index in 1..=self.run_boxes.len() {
            if index < self.run_boxes.len() && !self.starts_batch(start, index) {
                continue;
            }

            self.push_box_batch(start..index);
            start = index;
        }
    }

    /// Whether the box at `index` starts a new batch after the batch starting at `start`.
    fn starts_batch(&self, start: usize, index: usize) -> bool {
        let length = index - start;
        let current = &self.run_boxes[index];
        if current.volatile != self.run_boxes[index - 1].volatile {
            return true;
        }
        if current.volatile {
            return length >= MAX_VOLATILE_BATCH_BOXES;
        }

        length >= MAX_BATCH_BOXES || (length >= MIN_BATCH_BOXES && current.boundary)
    }

    /// Append one batch of the current run.
    fn push_box_batch(&mut self, range: std::ops::Range<usize>) {
        let boxes = &self.run_boxes[range];
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let first = self.piece_boxes.len();
        let mut len = 0;
        for run_box in boxes {
            run_box.identity.hash(&mut hasher);
            hasher.write_u64(run_box.hash);
            len += run_box.shape.records;
            self.piece_boxes.push(run_box.identity);
            self.piece_shapes.push(run_box.shape);
        }

        self.order
            .push(GuiPieceRef::Shapes(self.shape_pieces.len()));
        self.shape_pieces.push(GuiPiece {
            key: GuiPieceKey::Boxes(boxes[0].identity),
            hash: hasher.finish(),
            len,
            page: None,
            bounds: None,
            source: GuiPieceSource::Boxes(first..self.piece_boxes.len()),
        });
    }

    /// Append retained glyph batches of the current Surface, in painter order.
    pub(crate) fn push_glyphs(&mut self, pieces: impl IntoIterator<Item = GuiPiece>) {
        for piece in pieces {
            self.order
                .push(GuiPieceRef::Glyphs(self.glyph_pieces.len()));
            self.glyph_pieces.push(piece);
        }
    }

    /// Place the collected batches in the Surface's storage, writing only changed ones.
    ///
    /// `glyphs` returns the records of atlas page batch `index` of a text run. Returns
    /// `false` when the Surface has no usable storage this frame: a recoverable
    /// allocation or write failed, or an earlier one is still backing off. The failed
    /// storage is released, so no later frame draws unknown contents. Context loss is
    /// returned as an error.
    pub fn commit_surface<'g>(
        &mut self,
        glyphs: impl Fn(CanvasPrimitiveId, u32) -> &'g [GuiGlyphRecord],
        stats: &mut RenderFrameWork,
    ) -> Result<bool, RenderError> {
        if self.order.is_empty() {
            return Ok(true);
        }

        let entity = self.entity;
        if self
            .backoff
            .get(&entity)
            .is_some_and(|backoff| backoff.retry_frame > self.frame)
        {
            return Ok(false);
        }

        let mut storage = self.storage.remove(&entity).unwrap_or(GuiSurfaceStorages {
            shapes: None,
            glyphs: None,
            seen: 0,
        });
        let before = storage.bytes();
        let cpu_primitives = &self.cpu_primitives;
        let piece_boxes = &self.piece_boxes;
        let shape_pieces = &self.shape_pieces;
        let glyph_pieces = &self.glyph_pieces;
        let mut device = self.device.borrow_mut();
        let mut fill_shapes = |index: usize, records: &mut Vec<GuiShapeRecord>| {
            if let GuiPieceSource::Boxes(range) = &shape_pieces[index].source {
                for &identity in &piece_boxes[range.clone()] {
                    records.extend_from_slice(
                        &cpu_primitives[&PrimitiveKey {
                            entity,
                            identity,
                        }]
                            .records,
                    );
                }
            }
        };
        let mut fill_glyphs = |index: usize, records: &mut Vec<GuiGlyphRecord>| {
            if let GuiPieceSource::Glyphs(identity, batch) = glyph_pieces[index].source {
                records.extend_from_slice(glyphs(identity, batch));
            }
        };
        let result = commit_surface_storage(
            &mut *device,
            &mut storage.shapes,
            shape_pieces,
            &mut fill_shapes,
            &mut self.shape_scratch,
            stats,
        )
        .and_then(|()| {
            commit_surface_storage(
                &mut *device,
                &mut storage.glyphs,
                glyph_pieces,
                &mut fill_glyphs,
                &mut self.glyph_scratch,
                stats,
            )
        });
        drop(device);

        let after = storage.bytes();
        self.resident = self.resident - before + after;
        if storage.shapes.is_some() || storage.glyphs.is_some() {
            storage.seen = self.frame;
            self.storage.insert(entity, storage);
        }

        match result {
            Ok(()) => {
                self.backoff.remove(&entity);
                Ok(true)
            }
            Err(RenderError::ContextLost) => Err(RenderError::ContextLost),
            Err(_) => {
                let backoff = self.backoff.entry(entity).or_insert(StorageBackoff {
                    retry_frame: 0,
                    failures: 0,
                });
                backoff.failures = backoff.failures.saturating_add(1);
                let doublings = (backoff.failures - 1).min(MAX_STORAGE_RETRY_DOUBLINGS);
                backoff.retry_frame = self.frame + (STORAGE_RETRY_FRAMES << doublings);
                Ok(false)
            }
        }
    }

    /// Draw collected batches `range` of the committed Surface: each segment of the
    /// [GUI draw order](super::draw_order) draws its shapes with `shapes`, the
    /// canvas program, then its glyphs with `glyphs`, one draw per atlas page change.
    /// `domain` is the canvas's content rectangle and `atlas` returns the texture of an
    /// atlas page.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_pieces<'t>(
        &mut self,
        shapes: &D::Program,
        glyphs: &D::Program,
        range: std::ops::Range<usize>,
        domain: [f32; 4],
        atlas: impl Fn(usize) -> Option<&'t D::Texture>,
        mvp: &[f32; 16],
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError>
    where
        D::Texture: 't,
    {
        if range.is_empty() {
            return Ok(());
        }

        let storage = self
            .storage
            .get(&self.entity)
            .ok_or_else(|| RenderError::RenderDevice("GUI storage missing".into()))?;
        let missing = || RenderError::RenderDevice("GUI storage missing".into());
        self.draw_order.begin(domain);
        for piece in &self.order[range.clone()] {
            match *piece {
                GuiPieceRef::Shapes(index) => {
                    let mut first = storage
                        .shapes
                        .as_ref()
                        .ok_or_else(missing)?
                        .slot_start(index);
                    let GuiPieceSource::Boxes(boxes) = &self.shape_pieces[index].source else {
                        continue;
                    };
                    for shape in &self.piece_shapes[boxes.clone()] {
                        self.draw_order.push(GuiDrawItem::Shape {
                            first,
                            count: shape.records,
                            bounds: shape.bounds,
                        });
                        first += shape.records;
                    }
                }
                GuiPieceRef::Glyphs(index) => {
                    let piece = &self.glyph_pieces[index];
                    self.draw_order.push(GuiDrawItem::Glyphs {
                        first: storage
                            .glyphs
                            .as_ref()
                            .ok_or_else(missing)?
                            .slot_start(index),
                        count: piece.len,
                        page: piece.page.unwrap_or_default(),
                        bounds: piece.bounds,
                    });
                }
            }
        }

        let mut device = self.device.borrow_mut();
        for draw in self.draw_order.finish() {
            let records = draw.first..draw.end;
            match draw.kind {
                GuiRecordKind::Shape => storage.shapes.as_ref().ok_or_else(missing)?.draw(
                    &mut device,
                    shapes,
                    None,
                    mvp,
                    records,
                )?,
                GuiRecordKind::Glyph => {
                    let texture = draw.page.and_then(&atlas).ok_or_else(|| {
                        RenderError::RenderDevice("atlas page texture missing".into())
                    })?;
                    storage.glyphs.as_ref().ok_or_else(missing)?.draw(
                        &mut device,
                        glyphs,
                        Some(texture),
                        mvp,
                        records,
                    )?;
                }
            }
            stats.draw(2 * draw.records as u32);
        }
        stats.statistics.gui_batches += range.len() as u32;
        Ok(())
    }

    /// End-of-frame maintenance: release stale GPU storage and CPU geometry.
    ///
    /// `surfaces` is `None` when submission did not complete. Unused work then cannot be
    /// told apart from undrawn work, so everything is kept.
    pub fn finish_frame(&mut self, surfaces: Option<&RetainedSurfaceSubmission<'_>>) {
        if let Some(surfaces) = surfaces {
            let frame = self.frame;
            let stale: Vec<ipp_core::EntityId> = self
                .storage
                .iter()
                .filter(|&(&entity, storage)| surfaces.is_stale(entity, storage.seen == frame))
                .map(|(&entity, _)| entity)
                .collect();

            let mut device = self.device.borrow_mut();
            for entity in stale {
                if let Some(storage) = self.storage.remove(&entity) {
                    self.resident -= storage.bytes();
                    storage.delete(&mut device);
                }
            }

            self.cpu_primitives
                .retain(|key, cached| !surfaces.is_stale(key.entity, cached.seen == frame));
            self.backoff
                .retain(|entity, _| surfaces.live.contains(entity));
        }

        self.frame += 1;
    }
}

/// Whether a stable box identity may start a batch. Boundaries depend only on
/// identities, so edits elsewhere in a run keep the batches that do not contain them.
fn identity_starts_batch(identity: CanvasPrimitiveId) -> bool {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut hasher);
    hasher.finish().is_multiple_of(BATCH_BOUNDARY_PERIOD)
}

impl<D: RenderDevice> Drop for GuiBatchRenderCache<D> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Union of the rectangles of `records` within `clip`, or `None` when they cover
/// nothing there: the area a shape can paint, before antialiasing.
fn shape_bounds(records: &[GuiShapeRecord], clip: CanvasClip) -> Option<[f32; 4]> {
    let union = records
        .iter()
        .map(|record| record.rect)
        .filter(|[x0, y0, x1, y1]| x0 < x1 && y0 < y1)
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })?;
    clipped_bounds(union, clip)
}

#[cfg(test)]
#[path = "shape_batches_tests.rs"]
mod tests;
