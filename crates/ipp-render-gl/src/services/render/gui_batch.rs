//! Retained GUI shape batches and their per-Surface GPU storage.
//!
//! GUI controls emit parameterized box primitives. RenderService retains CPU
//! [shape records](super::gui_records::GuiShapeRecord) keyed by live primitive identity
//! and content revision, and groups each Surface's boxes into bounded batches. Shape
//! batches occupy the Surface's [shape storage](super::gui_storage) and atlas text
//! batches its glyph storage, each in painter order, and a run of consecutive GUI work
//! draws its shapes and then its glyphs in the [GUI draw order](super::gui_draw_order),
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

use super::canvas_paint::{CanvasPaintBlocks, GuiPaintLanes};
use super::gui_draw_order::{GuiDrawItem, GuiDrawOrder};
use super::gui_records::{GuiGlyphRecord, GuiRecord, GuiRecordKind, GuiShapeRecord};
use super::gui_storage::{
    GuiCommitScratch, GuiPiece, GuiPieceKey, GuiPieceSource, GuiSurfaceStorage,
    commit_surface_storage,
};
pub use super::retained_surfaces::RetainedSurfaceSubmission;
use super::retained_surfaces::SurfacePaint;
use crate::services::render::frame_statistics::RenderFrameWork;
use crate::{RenderDevice, RenderError};
use ipp_core::systems::canvas::{
    CanvasBoxShape, CanvasClip, CanvasPrimitive, CanvasPrimitiveId, CanvasPrimitiveStyle,
    CanvasShapeChecker, CanvasShapeFill, CanvasShapeGlow,
};

/// Exterior margin in logical units that generated box geometry adds beyond paint.
///
/// `surface_gui.vert` declares the same value and extends exterior corners only by
/// the part of its projected antialias footprint this margin does not already cover.
pub const GUI_BOX_ANTIALIAS_PAD: f32 = 0.002;

/// Fill type of the hue circle along a gradient axis.
///
/// Fill types run below [`GUI_PAINT_CHECKER`]: solid (0), linear (1), radial (2),
/// hue and [`GUI_FILL_SATURATION_VALUE`], leaving 5 to 7 free for further fills of
/// every shape. `surface_gui.frag` declares the same value.
pub const GUI_FILL_HUE: f32 = 3.0;

/// Fill type of the saturation-value field over the part rectangle.
///
/// `surface_gui.frag` declares the same value.
pub const GUI_FILL_SATURATION_VALUE: f32 = 4.0;

/// Fill type of a custom paint, whose function the canvas program selects by the
/// slot the primitive carries.
///
/// A painted primitive's lanes hold its paint inputs instead of gradient stops:
/// `color0` the straight colour the function receives, without opacity; `color1`
/// the slot and parameter block packed as `block * GUI_PAINT_BLOCK_STRIDE + slot`,
/// the opacity and the signed visual scale; `gradient_coords` the part rectangle's
/// origin from the placement origin and its size in the shape's own units.
/// `surface_gui.frag` declares the same value.
pub const GUI_FILL_PAINT: f32 = 5.0;

/// Multiplier of a painted primitive's parameter block offset in its packed slot
/// lane; slots are smaller. `surface_gui.frag` declares the same value.
pub const GUI_PAINT_BLOCK_STRIDE: f32 = 16.0;

const _: () = assert!(super::canvas_paint::CANVAS_PAINT_SLOTS < GUI_PAINT_BLOCK_STRIDE as usize);

/// Paint offset of a box's checker: a box painting a checker beneath its fill adds
/// this value to its fill type.
///
/// `surface_gui.frag` declares the same value.
pub const GUI_PAINT_CHECKER: f32 = 8.0;

/// Paint offset of strokes: a stroke's paint is its fill type plus this value.
///
/// Shape offsets are multiples of this stride, so a paint is its shape's offset plus a
/// fill type and the checker's offset below it; boxes have offset zero.
/// `surface_gui.frag` declares the same value.
pub const GUI_PAINT_STROKE: f32 = 16.0;

/// Paint offset of arcs: an arc's paint is its fill type plus this value.
///
/// `surface_gui.frag` declares the same value.
pub const GUI_PAINT_ARC: f32 = 32.0;

/// How far beyond a 45-degree cut a border of unit width reaches along each edge:
/// the inset cut meets the inset edge `(sqrt(2) - 1)` widths past the cut's end.
const CUT_BORDER_REACH: f32 = std::f32::consts::SQRT_2 - 1.0;

/// Stable key identifying one live primitive's CPU geometry cache entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimitiveKey {
    /// Retained Surface key; a Canvas output uses [`CANVAS_SURFACE`](super::retained_surfaces::CANVAS_SURFACE).
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

    /// Append one contiguous run of box primitives, each with its effective clip, as
    /// bounded batches.
    ///
    /// Stable boxes split at identity-selected boundaries; volatile boxes form their own
    /// small batches. Boxes whose retained hash was computed under the Surface's reusable
    /// `paint` revision are not hashed again. A custom paint fill takes its instance's
    /// lanes from [`Self::paints`], and draws its colour solidly without them.
    pub fn push_boxes(
        &mut self,
        paint: SurfacePaint,
        boxes: &[(&CanvasPrimitive, CanvasClip)],
        stats: &mut RenderFrameWork,
    ) {
        // Refresh every box's geometry and volatility before choosing batch boundaries.
        self.run_boxes.clear();
        for &(primitive, clip) in boxes {
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
                        let hash = hash();
                        if cached.hash != hash {
                            cached.hash = hash;
                            (cached.records, cached.bounds) = generate();
                            cached.volatile_until = self.frame + VOLATILE_FRAMES;
                            stats.statistics.gui_rebuilds += 1;
                        }
                        cached.revision = paint.revision;
                    }
                    cached
                }
                Entry::Vacant(entry) => {
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
    /// [GUI draw order](super::gui_draw_order) draws its shapes with `shapes`, the
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

/// `bounds` `[x0, y0, x1, y1]` within `clip`, or `None` when they do not overlap.
pub(crate) fn clipped_bounds(bounds: [f32; 4], clip: CanvasClip) -> Option<[f32; 4]> {
    let clipped = [
        bounds[0].max(clip[0]),
        bounds[1].max(clip[1]),
        bounds[2].min(clip[2]),
        bounds[3].min(clip[3]),
    ];
    (clipped[0] < clipped[2] && clipped[1] < clipped[3]).then_some(clipped)
}

/// Compute the shape records of a box, stroke or arc: one per covered rectangle.
///
/// Every record carries the primitive's material lanes and the effective `clip`;
/// [`outline_records`], [`stroke_records`] and [`arc_records`] choose the covered
/// rectangles. A custom paint fill draws its colour solidly here, as it does without
/// a usable paint; the retained batches give admitted paints their lanes.
#[allow(clippy::too_many_arguments)]
pub fn generate_box_records(
    style: &CanvasPrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
    fill: &CanvasShapeFill,
    glow: Option<&CanvasShapeGlow>,
    shape: &CanvasBoxShape,
    clip: CanvasClip,
) -> Vec<GuiShapeRecord> {
    painted_box_records(
        style,
        size,
        corner_radius,
        border_width,
        border_color,
        fill,
        glow,
        shape,
        clip,
        None,
    )
}

/// [`generate_box_records`] with the slot and block `paint` of a custom paint fill.
#[allow(clippy::too_many_arguments)]
pub(crate) fn painted_box_records(
    style: &CanvasPrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
    fill: &CanvasShapeFill,
    glow: Option<&CanvasShapeGlow>,
    shape: &CanvasBoxShape,
    clip: CanvasClip,
    paint: Option<GuiPaintLanes>,
) -> Vec<GuiShapeRecord> {
    let pos = style.position;
    let scale = style.scale;
    let placed_size = [size[0] * scale[0], size[1] * scale[1]];

    let tint = |color: [f32; 4]| {
        [
            color[0] * style.color[0],
            color[1] * style.color[1],
            color[2] * style.color[2],
            color[3] * style.color[3],
        ]
    };
    let border = tint([
        border_color[0],
        border_color[1],
        border_color[2],
        border_color[3] * style.opacity,
    ]);

    // Colour fields paint opaque colours under the tint and opacity, and the
    // saturation-value field carries its hue instead of an end colour.
    let field = tint([1.0, 1.0, 1.0, style.opacity]);
    let solid = |color: &[f32; 4]| {
        let color = tint([color[0], color[1], color[2], color[3] * style.opacity]);
        (0.0f32, color, color, [0.0; 4])
    };
    let (fill_type, color0, color1, gradient_coords) = match fill {
        CanvasShapeFill::Solid(color) => solid(color),
        // A paint receives its colour without the opacity, which the canvas program
        // applies to the result, and the part rectangle in the shape's own units.
        CanvasShapeFill::Paint {
            color,
            ..
        } => match paint {
            Some(lanes) => (
                GUI_FILL_PAINT,
                tint(*color),
                [lanes.packed(), style.opacity, scale[0], scale[1]],
                [0.0, 0.0, size[0], size[1]],
            ),
            None => solid(color),
        },
        CanvasShapeFill::LinearGradient {
            start,
            end,
            start_color,
            end_color,
        } => (
            1.0f32,
            tint([
                start_color[0],
                start_color[1],
                start_color[2],
                start_color[3] * style.opacity,
            ]),
            tint([
                end_color[0],
                end_color[1],
                end_color[2],
                end_color[3] * style.opacity,
            ]),
            [
                start[0] * scale[0],
                start[1] * scale[1],
                end[0] * scale[0],
                end[1] * scale[1],
            ],
        ),
        CanvasShapeFill::RadialGradient {
            center,
            radius,
            start_color,
            end_color,
        } => (
            2.0f32,
            tint([
                start_color[0],
                start_color[1],
                start_color[2],
                start_color[3] * style.opacity,
            ]),
            tint([
                end_color[0],
                end_color[1],
                end_color[2],
                end_color[3] * style.opacity,
            ]),
            [
                center[0] * scale[0],
                center[1] * scale[1],
                *radius * scale[0].abs().max(scale[1].abs()),
                0.0,
            ],
        ),
        CanvasShapeFill::Hue {
            start,
            end,
        } => (
            GUI_FILL_HUE,
            field,
            [0.0; 4],
            [
                start[0] * scale[0],
                start[1] * scale[1],
                end[0] * scale[0],
                end[1] * scale[1],
            ],
        ),
        CanvasShapeFill::SaturationValue {
            hue,
        } => (
            GUI_FILL_SATURATION_VALUE,
            field,
            [
                if hue.is_finite() {
                    hue.rem_euclid(1.0)
                } else {
                    0.0
                },
                0.0,
                0.0,
                0.0,
            ],
            [0.0, 0.0, placed_size[0], placed_size[1]],
        ),
    };

    // Both glow reaches scale with the primitive; the intensity joins the glow alpha,
    // which the falloff then scales.
    let (glow_inner_radius, glow_radius, glow_falloff, glow_color) = match glow {
        Some(glow) if usable_glow(glow) => {
            let length_scale = scale[0].abs().max(scale[1].abs());
            (
                glow.inner_radius * length_scale,
                glow.radius * length_scale,
                glow.falloff,
                [
                    glow.color[0],
                    glow.color[1],
                    glow.color[2],
                    glow.color[3] * style.opacity * glow.intensity,
                ],
            )
        }
        _ => (0.0, 0.0, 1.0, [0.0; 4]),
    };

    let record = GuiShapeRecord {
        rect: [0.0; 4],
        placement: [pos[0], pos[1], placed_size[0], placed_size[1]],
        shape: [corner_radius[0], corner_radius[1], border_width, 0.0],
        corner_cut: [0.0; 4],
        corner_accent: [0.0; 4],
        color0,
        color1,
        border_color: border,
        gradient_coords,
        material_params: [fill_type, glow_inner_radius, glow_radius, glow_falloff],
        glow_color: tint(glow_color),
        clip,
    };
    match *shape {
        CanvasBoxShape::Rect {
            corner_cut,
            corner_accent,
            corner_accent_width,
            checker,
        } => {
            let extent = placed_size.map(f32::abs);
            // A checker takes the accent lanes and paints the interior.
            let checker = checker.as_ref().and_then(|checker| {
                checker_lanes(
                    checker,
                    style.opacity,
                    &tint,
                    scale[0].abs().max(scale[1].abs()),
                )
            });
            let (accent_width, corner_accent, paint) = match checker {
                Some(lanes) => (0.0, lanes, fill_type + GUI_PAINT_CHECKER),
                None => (
                    finite_length(corner_accent_width),
                    clamp_corner_lengths(corner_accent, extent),
                    fill_type,
                ),
            };
            let mut material_params = record.material_params;
            material_params[0] = paint;
            let record = GuiShapeRecord {
                shape: [
                    corner_radius[0],
                    corner_radius[1],
                    border_width,
                    accent_width,
                ],
                corner_cut: clamp_corner_lengths(corner_cut, extent),
                corner_accent,
                material_params,
                ..record
            };
            let transparent = checker.is_none()
                && matches!(fill, CanvasShapeFill::Solid(color) if color[3] <= 0.0 || style.opacity <= 0.0);
            outline_records(&record, transparent)
        }
        CanvasBoxShape::Stroke {
            segments,
        } => stroke_records(&record, &segments),
        CanvasBoxShape::Arc {
            start,
            sweep,
            dashes,
            dash_duty,
        } => arc_records(&record, start, sweep, dashes, dash_duty),
    }
}

/// The record `lanes` covering the axis-aligned rectangle from `[x0, y0]` to
/// `[x1, y1]`.
fn quad(lanes: &GuiShapeRecord, x0: f32, y0: f32, x1: f32, y1: f32) -> GuiShapeRecord {
    GuiShapeRecord {
        rect: [x0, y0, x1, y1],
        ..*lanes
    }
}

/// Geometry of a box whose `record` lanes are complete.
///
/// A filled box, or one too small for sparse coverage to pay off, is one quad over its
/// paint bounds: the placed rectangle grown by the outer glow and the antialias pad. A
/// box whose `transparent` solid fill leaves its interior empty covers only its
/// outline. When every corner paints only as deep as the edges, four edge strips
/// cover it. Otherwise each corner gets a square reaching as far as its radius, cut
/// and accent paint, joined by edge strips as deep as the border and the inner glow,
/// which are omitted when nothing paints along the edges, as for corner brackets
/// alone. Every rectangle is one record with the box's lanes.
fn outline_records(record: &GuiShapeRecord, transparent: bool) -> Vec<GuiShapeRecord> {
    let [x, y, width, height] = record.placement;
    let [rx, ry, border_width, accent_width] = record.shape;
    let [_, glow_inner_radius, glow_radius, _] = record.material_params;
    let (cut, accent) = (record.corner_cut, record.corner_accent);
    let pad = glow_radius + GUI_BOX_ANTIALIAS_PAD;

    let x0 = x.min(x + width) - pad;
    let y0 = y.min(y + height) - pad;
    let x1 = x.max(x + width) + pad;
    let y1 = y.max(y + height) + pad;
    let quad = |x0, y0, x1, y1| quad(record, x0, y0, x1, y1);

    let accented = accent_width > 0.0 && accent.iter().any(|span| *span > 0.0);
    let outline_only = transparent && (border_width > 0.0 || accented);

    // How deep paint reaches inward along the edges, and from each corner along both
    // of its edges: its radius, accent span and the ring along its cut.
    let edge_depth = border_width.max(glow_inner_radius).max(0.0);
    let base_reach = edge_depth.max(rx).max(ry);
    let reach: [f32; 4] = std::array::from_fn(|corner| {
        let depth = if accent[corner] > 0.0 {
            edge_depth.max(accent_width)
        } else {
            edge_depth
        };
        let mut reach = base_reach.max(depth).max(accent[corner]);
        if cut[corner] > 0.0 {
            reach = reach.max(cut[corner] + CUT_BORDER_REACH * depth);
        }
        reach
    });
    let strip_thickness = reach.iter().fold(base_reach, |a, b| a.max(*b)) + pad;
    let can_use_strips = outline_only
        && width.abs() >= 3.0 * strip_thickness
        && height.abs() >= 3.0 * strip_thickness;

    if !can_use_strips {
        return vec![quad(x0, y0, x1, y1)];
    }

    if reach.iter().all(|corner| *corner == base_reach) {
        return vec![
            // Top strip: covers top-left, top edge, top-right
            quad(x0, y0, x1, y0 + strip_thickness),
            // Bottom strip: covers bottom-left, bottom edge, bottom-right
            quad(x0, y1 - strip_thickness, x1, y1),
            // Left strip: between top and bottom strips
            quad(
                x0,
                y0 + strip_thickness,
                x0 + strip_thickness,
                y1 - strip_thickness,
            ),
            // Right strip: between top and bottom strips
            quad(
                x1 - strip_thickness,
                y0 + strip_thickness,
                x1,
                y1 - strip_thickness,
            ),
        ];
    }

    // Corner squares in screen order; a mirrored axis swaps the box's own corners.
    let side = |right: bool, bottom: bool| {
        let corner = match (right != (width < 0.0), bottom != (height < 0.0)) {
            (false, false) => 0,
            (true, false) => 1,
            (true, true) => 2,
            (false, true) => 3,
        };
        reach[corner] + pad
    };
    let [tl, tr, br, bl] = [
        side(false, false),
        side(true, false),
        side(true, true),
        side(false, true),
    ];
    let mut records = Vec::with_capacity(8);
    records.push(quad(x0, y0, x0 + tl, y0 + tl));
    records.push(quad(x1 - tr, y0, x1, y0 + tr));
    records.push(quad(x1 - br, y1 - br, x1, y1));
    records.push(quad(x0, y1 - bl, x0 + bl, y1));

    // Edge strips between the corners, only where the border or a glow paints along
    // the edges.
    if edge_depth > 0.0 || glow_radius > 0.0 {
        let depth = edge_depth + pad;
        records.push(quad(x0 + tl, y0, x1 - tr, y0 + depth));
        records.push(quad(x0 + bl, y1 - depth, x1 - br, y1));
        records.push(quad(x0, y0 + tl, x0 + depth, y1 - bl));
        records.push(quad(x1 - depth, y0 + tr, x1, y1 - br));
    }
    records
}

/// Geometry of a stroke over the part rectangle of `record`: one quad over its
/// segments, their thickness and their glow.
///
/// A stroke's own bounds become its placement, so the vertex shader grows the quad
/// outward; segments are stored from that origin and gradients stay anchored to the
/// part rectangle. Without a segment of length the stroke keeps its slot with an
/// empty record.
fn stroke_records(record: &GuiShapeRecord, segments: &[[f32; 4]; 2]) -> Vec<GuiShapeRecord> {
    let [x, y, width, height] = record.placement;
    let [fill_type, _, glow_radius, _] = record.material_params;
    let thickness = finite_length(record.shape[2]);
    let half_width = thickness * 0.5;

    let mut lanes = [[0.0f32; 4]; 2];
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for (lane, segment) in lanes.iter_mut().zip(segments) {
        let start = [x + segment[0] * width, y + segment[1] * height];
        let end = [x + segment[2] * width, y + segment[3] * height];
        let axis = [end[0] - start[0], end[1] - start[1]];
        let length = axis[0].hypot(axis[1]);
        if !(length.is_finite() && length > 0.0) {
            continue;
        }

        // Butt caps: the segment's rectangle ends at its end points.
        let normal = [
            -axis[1] / length * half_width,
            axis[0] / length * half_width,
        ];
        for point in [start, end] {
            for side in [-1.0, 1.0] {
                let corner = [point[0] + side * normal[0], point[1] + side * normal[1]];
                bounds[0] = bounds[0].min(corner[0]);
                bounds[1] = bounds[1].min(corner[1]);
                bounds[2] = bounds[2].max(corner[0]);
                bounds[3] = bounds[3].max(corner[1]);
            }
        }
        *lane = [
            (start[0] + end[0]) * 0.5,
            (start[1] + end[1]) * 0.5,
            axis[0] * 0.5,
            axis[1] * 0.5,
        ];
    }

    // Bounds grow only from finite segments, so they stay inverted without one.
    if bounds[0] > bounds[2] {
        return vec![GuiShapeRecord::EMPTY];
    }

    let origin = [bounds[0], bounds[1]];
    for lane in &mut lanes {
        if lane[2] != 0.0 || lane[3] != 0.0 {
            lane[0] -= origin[0];
            lane[1] -= origin[1];
        }
    }

    let mut material_params = record.material_params;
    material_params[0] = fill_type + GUI_PAINT_STROKE;
    let stroke = GuiShapeRecord {
        placement: [
            origin[0],
            origin[1],
            bounds[2] - bounds[0],
            bounds[3] - bounds[1],
        ],
        shape: [0.0, 0.0, thickness, 0.0],
        corner_cut: lanes[0],
        corner_accent: lanes[1],
        gradient_coords: reanchored_gradient(record, origin),
        material_params,
        ..*record
    };
    let pad = glow_radius + GUI_BOX_ANTIALIAS_PAD;
    vec![quad(
        &stroke,
        bounds[0] - pad,
        bounds[1] - pad,
        bounds[2] + pad,
        bounds[3] + pad,
    )]
}

/// Gradient coordinates of `record` measured from `origin` instead of its placement
/// origin, so a shape placed at its own bounds keeps its gradient, hue axis or colour
/// field on the part rectangle.
fn reanchored_gradient(record: &GuiShapeRecord, origin: [f32; 2]) -> [f32; 4] {
    let fill_type = record.material_params[0];
    let shift = [
        record.placement[0] - origin[0],
        record.placement[1] - origin[1],
    ];
    let mut gradient_coords = record.gradient_coords;
    // Every fill but solid places its first point; all but the radial gradient,
    // whose third lane is its radius, and a paint, whose last lanes are the part's
    // own size, place a second.
    if fill_type != 0.0 {
        gradient_coords[0] += shift[0];
        gradient_coords[1] += shift[1];
    }
    if fill_type != 0.0 && fill_type != 2.0 && fill_type != GUI_FILL_PAINT {
        gradient_coords[2] += shift[0];
        gradient_coords[3] += shift[1];
    }
    gradient_coords
}

/// Geometry of an arc in the part rectangle of `record`: its sector's tight bounds
/// grown by its glow, one quad, or four strips around a hollow centre.
///
/// The ring's outer radius is half the placed rectangle's shorter side, around the
/// rectangle's centre, and its thickness the border width, up to that radius. Angles
/// are turns clockwise from twelve o'clock in the box's own orientation, so the
/// placed size's signs mirror the arc. The sector's bounds become its placement, as a
/// stroke's do, so the vertex shader grows the quad outward and gradients stay on the
/// part rectangle. An arc that paints nothing keeps its slot with an empty record: a
/// zero sweep, a dash pattern of zero duty, or a sweep too short to reach the first
/// dash.
fn arc_records(
    record: &GuiShapeRecord,
    start: f32,
    sweep: f32,
    dashes: f32,
    dash_duty: f32,
) -> Vec<GuiShapeRecord> {
    use std::f64::consts::{PI, TAU};

    let [x, y, width, height] = record.placement;
    let [fill_type, _, glow_radius, _] = record.material_params;
    let outer = width.abs().min(height.abs()) * 0.5;
    let turns = if sweep.is_finite() {
        sweep.abs().min(1.0)
    } else {
        0.0
    };
    if !(outer.is_finite() && outer > 0.0 && start.is_finite() && turns > 0.0) {
        return vec![GuiShapeRecord::EMPTY];
    }

    // Dashes need cells and a duty short of solid; the first dash starts half a gap
    // after the arc's start.
    let dashed = dashes.is_finite() && dashes > 0.0 && dash_duty < 1.0;
    let (cells, duty) = if dashed {
        (dashes, dash_duty.max(0.0))
    } else {
        (0.0, 1.0)
    };
    if dashed && (duty <= 0.0 || turns * cells <= (1.0 - duty) * 0.5) {
        return vec![GuiShapeRecord::EMPTY];
    }

    let thickness = finite_length(record.shape[2]).min(outer);
    let inner = outer - thickness;
    let full = turns >= 1.0;
    let half = if full {
        std::f32::consts::PI
    } else {
        (f64::from(turns) * PI) as f32
    };
    let (cap_sin, cap_cos) = if full {
        (0.0, -1.0)
    } else {
        let (sin, cos) = (f64::from(turns) * PI).sin_cos();
        (sin as f32, cos as f32)
    };

    // The middle of the sweep, clockwise from twelve o'clock in the box's own Y-down
    // orientation, then mirrored with the placed size.
    let middle = (f64::from(start) + f64::from(sweep.clamp(-1.0, 1.0)) * 0.5).rem_euclid(1.0) * TAU;
    let (sin, cos) = middle.sin_cos();
    let mirror = [width.signum(), height.signum()];
    let middle = [sin as f32 * mirror[0], -cos as f32 * mirror[1]];
    let clockwise = (sweep > 0.0) == (mirror[0] * mirror[1] > 0.0);
    let center = [x + width * 0.5, y + height * 0.5];

    // Sector bounds: both ends' corners and the outer circle's extreme points
    // within the sweep.
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    let mut include = |direction: [f32; 2], radius: f32| {
        let point = [
            center[0] + direction[0] * radius,
            center[1] + direction[1] * radius,
        ];
        bounds[0] = bounds[0].min(point[0]);
        bounds[1] = bounds[1].min(point[1]);
        bounds[2] = bounds[2].max(point[0]);
        bounds[3] = bounds[3].max(point[1]);
    };
    let tangent = [-middle[1], middle[0]];
    for side in [-1.0, 1.0] {
        let end = [
            middle[0] * cap_cos + side * tangent[0] * cap_sin,
            middle[1] * cap_cos + side * tangent[1] * cap_sin,
        ];
        include(end, inner);
        include(end, outer);
    }
    for axis in [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]] {
        if full || axis[0] * middle[0] + axis[1] * middle[1] >= cap_cos {
            include(axis, outer);
        }
    }

    let origin = [bounds[0], bounds[1]];
    let mut material_params = record.material_params;
    material_params[0] = fill_type + GUI_PAINT_ARC;
    let arc = GuiShapeRecord {
        placement: [
            origin[0],
            origin[1],
            bounds[2] - bounds[0],
            bounds[3] - bounds[1],
        ],
        shape: [outer - thickness * 0.5, half, thickness, duty],
        corner_cut: [
            center[0] - origin[0],
            center[1] - origin[1],
            middle[0],
            middle[1],
        ],
        corner_accent: [
            cap_sin,
            cap_cos,
            if clockwise {
                cells
            } else {
                -cells
            },
            0.0,
        ],
        gradient_coords: reanchored_gradient(record, origin),
        material_params,
        ..*record
    };

    let pad = glow_radius + GUI_BOX_ANTIALIAS_PAD;
    let [x0, y0, x1, y1] = [
        bounds[0] - pad,
        bounds[1] - pad,
        bounds[2] + pad,
        bounds[3] + pad,
    ];
    // Nothing paints closer to the centre than the inner radius less the outer glow,
    // so the square inscribed in that disc may stay uncovered. The vertex shader moves
    // the hole's corners toward the placement's centre by the antialias footprint,
    // which shrinks the hole only while that centre lies inside it. A hole under a
    // quarter of the bounds saves too little to split the quad.
    let hole = (inner - pad) * std::f32::consts::FRAC_1_SQRT_2;
    let [h0, h1, h2, h3] = [
        center[0] - hole,
        center[1] - hole,
        center[0] + hole,
        center[1] + hole,
    ];
    let mid = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    let hollow = hole > 0.0
        && 2.0 * hole >= 0.25 * (bounds[2] - bounds[0]).min(bounds[3] - bounds[1])
        && h0 > bounds[0]
        && h1 > bounds[1]
        && h2 < bounds[2]
        && h3 < bounds[3]
        && (h0..=h2).contains(&mid[0])
        && (h1..=h3).contains(&mid[1]);
    if !hollow {
        return vec![quad(&arc, x0, y0, x1, y1)];
    }

    vec![
        quad(&arc, x0, y0, x1, h1),
        quad(&arc, x0, h3, x1, y1),
        quad(&arc, x0, h1, h0, h3),
        quad(&arc, h2, h1, x1, h3),
    ]
}

/// Accent lanes of a box's checker, `[cell side, first colour, second colour,
/// alphas]`, or `None` when it paints nothing: a cell side that is not positive and
/// finite after the primitive's larger scale.
///
/// The colours take the primitive's `tint` and `opacity` like its fill. Each straight
/// RGB is packed as an exactly representable integer of three 8-bit channels on a
/// square-root curve, `r * 65536 + g * 256 + b`, which the shader squares back,
/// staying within two thirds of an 8-bit sRGB step of the colour; the alphas share
/// the last lane as `alpha0 * 256 + alpha1`.
fn checker_lanes(
    checker: &CanvasShapeChecker,
    opacity: f32,
    tint: &impl Fn([f32; 4]) -> [f32; 4],
    length_scale: f32,
) -> Option<[f32; 4]> {
    let cell = checker.size * length_scale;
    if !(cell.is_finite() && cell > 0.0) {
        return None;
    }

    let byte = |value: f32| {
        if value.is_finite() {
            (value.clamp(0.0, 1.0) * 255.0).round() as u32
        } else {
            0
        }
    };
    let [first, second] = checker.colors.map(|color| {
        let color = tint([color[0], color[1], color[2], color[3] * opacity]);
        let rgb = color[..3].iter().fold(0, |packed, channel| {
            packed * 256 + byte(channel.max(0.0).sqrt())
        });
        (rgb as f32, byte(color[3]))
    });
    Some([cell, first.0, second.0, (first.1 * 256 + second.1) as f32])
}

/// A non-negative finite length; anything else is zero.
fn finite_length(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// Whether a glow paints: positive intensity, at least one positive reach, and finite
/// non-negative parameters.
fn usable_glow(glow: &CanvasShapeGlow) -> bool {
    let reach = |value: f32| value.is_finite() && value >= 0.0;
    glow.intensity.is_finite()
        && glow.intensity > 0.0
        && reach(glow.radius)
        && reach(glow.inner_radius)
        && glow.radius + glow.inner_radius > 0.0
        && reach(glow.falloff)
        && glow
            .color
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
}

/// Clamp per-corner lengths `[tl, tr, br, bl]` to a box of `extent` so the lengths of
/// two corners never overlap along the side they share.
///
/// A side whose two corner lengths exceed it scales both to fit, and each corner takes
/// the smaller scale of its two sides. Oversized lengths therefore meet exactly: cuts
/// of half the short side give pointed ends, and two cuts of a whole side a triangle.
fn clamp_corner_lengths(lengths: [f32; 4], extent: [f32; 2]) -> [f32; 4] {
    let lengths = lengths.map(finite_length);
    let side = |a: f32, b: f32, length: f32| {
        if a + b > length {
            length / (a + b)
        } else {
            1.0
        }
    };
    let top = side(lengths[0], lengths[1], extent[0]);
    let right = side(lengths[1], lengths[2], extent[1]);
    let bottom = side(lengths[2], lengths[3], extent[0]);
    let left = side(lengths[3], lengths[0], extent[1]);
    [
        lengths[0] * top.min(left),
        lengths[1] * top.min(right),
        lengths[2] * bottom.min(right),
        lengths[3] * bottom.min(left),
    ]
}

/// Hash every evaluated input that [`generate_box_records`] reads.
///
/// The style colour lane is excluded: boxes paint only their fill, border and glow,
/// and solid fills already carry the evaluated colour. A colour transition on a
/// gradient box therefore changes no record and uploads nothing.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn hash_box_inputs(
    style: &CanvasPrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
    fill: &CanvasShapeFill,
    glow: Option<&CanvasShapeGlow>,
    shape: &CanvasBoxShape,
    clip: CanvasClip,
) -> u64 {
    hash_painted_box_inputs(
        style,
        size,
        corner_radius,
        border_width,
        border_color,
        fill,
        glow,
        shape,
        clip,
        None,
    )
}

/// Hash every input [`painted_box_records`] reads: a paint's slot and block, but
/// not its property values, which live in the canvas's parameter blocks, so a
/// property write changes no record.
#[allow(clippy::too_many_arguments)]
fn hash_painted_box_inputs(
    style: &CanvasPrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
    fill: &CanvasShapeFill,
    glow: Option<&CanvasShapeGlow>,
    shape: &CanvasBoxShape,
    clip: CanvasClip,
    paint: Option<GuiPaintLanes>,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut lanes = |lanes: &[f32]| {
        for lane in lanes {
            hasher.write_u32(lane.to_bits());
        }
    };

    lanes(&clip);
    lanes(&style.position);
    lanes(&style.scale);
    lanes(&[style.opacity]);
    lanes(&style.color);
    lanes(size);
    lanes(corner_radius);
    lanes(&[border_width]);
    lanes(border_color);

    match fill {
        CanvasShapeFill::Solid(color) => {
            lanes(&[0.0]);
            lanes(color);
        }
        CanvasShapeFill::LinearGradient {
            start,
            end,
            start_color,
            end_color,
        } => {
            lanes(&[1.0]);
            lanes(start);
            lanes(end);
            lanes(start_color);
            lanes(end_color);
        }
        CanvasShapeFill::RadialGradient {
            center,
            radius,
            start_color,
            end_color,
        } => {
            lanes(&[2.0]);
            lanes(center);
            lanes(&[*radius]);
            lanes(start_color);
            lanes(end_color);
        }
        CanvasShapeFill::Hue {
            start,
            end,
        } => {
            lanes(&[GUI_FILL_HUE]);
            lanes(start);
            lanes(end);
        }
        CanvasShapeFill::SaturationValue {
            hue,
        } => {
            lanes(&[GUI_FILL_SATURATION_VALUE, *hue]);
        }
        CanvasShapeFill::Paint {
            color,
            ..
        } => {
            lanes(&[GUI_FILL_PAINT]);
            lanes(color);
            lanes(&paint.map_or([-1.0], |lanes| [lanes.packed()]));
        }
    }

    match glow {
        Some(glow) => {
            lanes(&[1.0]);
            lanes(&glow.color);
            lanes(&[glow.intensity, glow.radius, glow.inner_radius, glow.falloff]);
        }
        None => lanes(&[0.0]),
    }

    match shape {
        CanvasBoxShape::Rect {
            corner_cut,
            corner_accent,
            corner_accent_width,
            checker,
        } => {
            lanes(&[0.0]);
            lanes(corner_cut);
            lanes(corner_accent);
            lanes(&[*corner_accent_width]);
            match checker {
                Some(checker) => {
                    lanes(&[1.0, checker.size]);
                    lanes(&checker.colors[0]);
                    lanes(&checker.colors[1]);
                }
                None => lanes(&[0.0]),
            }
        }
        CanvasBoxShape::Stroke {
            segments,
        } => {
            lanes(&[1.0]);
            lanes(&segments[0]);
            lanes(&segments[1]);
        }
        CanvasBoxShape::Arc {
            start,
            sweep,
            dashes,
            dash_duty,
        } => {
            lanes(&[2.0]);
            lanes(&[*start, *sweep, *dashes, *dash_duty]);
        }
    }

    hasher.finish()
}

#[cfg(test)]
#[path = "gui_batch_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "gui_batch_arc_tests.rs"]
mod arc_tests;

#[cfg(test)]
#[path = "gui_batch_colour_tests.rs"]
mod colour_tests;
