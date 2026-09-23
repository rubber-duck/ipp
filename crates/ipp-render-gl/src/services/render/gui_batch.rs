//! Retained GUI triangle batches and their per-Surface GPU storage.
//!
//! GUI controls emit parameterized box primitives. RenderService retains CPU
//! geometry keyed by live primitive identity and content revision, and groups each
//! Surface's boxes into bounded batches. Box batches and atlas text batches share
//! one vertex layout and program and occupy [per-Surface storage](super::gui_storage),
//! so consecutive GUI work of a Surface draws together whatever its clips. Warm
//! unchanged frames upload zero geometry bytes. Changing a control rewrites only the
//! affected batch. Culled Surfaces keep their retained work; destruction, or a
//! submission that no longer uses a batch, releases it.
//!
//! Batch boundaries within a run of boxes follow primitive identities rather than
//! positions, so inserting, removing or resizing a box rewrites only its own batch.
//! A box whose geometry changed recently is volatile and batches apart from stable
//! boxes, so an animated control rewrites only its own small batch each frame.
//! Volatility is the only partition: backgrounds, fills, icons and focus rings share
//! batches. Each vertex carries its primitive's clip, so a clip change rewrites the
//! batches of the boxes it clips.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::gui_storage::{
    GuiCommitScratch, GuiPiece, GuiPieceKey, GuiPieceSource, GuiSurfaceStorage,
    commit_surface_storage,
};
pub use super::retained_surfaces::RetainedSurfaceSubmission;
use super::retained_surfaces::SurfacePaint;
use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::systems::surface::{
    GuiShapeFill, GuiShapeGlow, SurfaceClipRect, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
    SurfaceRenderPrimitive,
};

/// One vertex of a non-indexed GUI triangle list (152 bytes).
///
/// Boxes and atlas glyph quads share this layout. Shape parameters, material stops,
/// glow properties and the clip rectangle are associated with every vertex, so
/// consecutive boxes and glyphs under different clips draw in one call without
/// uniforms or instancing. Glyph quads use fill type [`GUI_FILL_GLYPH`], carry their
/// atlas coordinates in `gradient_coords` and their tint in `color0`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiVertex {
    /// Local position in Surface content metres `[x, y]`.
    pub position: [f32; 2],
    /// Placed origin and size in Surface metres `[pos_x, pos_y, size_x, size_y]`.
    pub placement: [f32; 4],
    /// Shape metrics `[corner_rx, corner_ry, border_width, reserved]`.
    pub shape: [f32; 4],
    /// Fill start / solid straight linear RGBA; glyph tint.
    pub color0: [f32; 4],
    /// Fill end straight linear RGBA (for gradients).
    pub color1: [f32; 4],
    /// Border straight linear RGBA.
    pub border_color: [f32; 4],
    /// Gradient coordinates `[start_x, start_y, end_x, end_y]` or
    /// `[center_x, center_y, radius, 0.0]`; glyph atlas coordinates `[u, v, 0.0, 0.0]`.
    pub gradient_coords: [f32; 4],
    /// Material parameters: `[fill_type, glow_intensity, glow_radius, glow_falloff]`.
    pub material_params: [f32; 4],
    /// Glow straight linear RGBA.
    pub glow_color: [f32; 4],
    /// Effective clip rectangle `[min_x, min_y, max_x, max_y]` in Surface metres.
    pub clip: [f32; 4],
}

impl GuiVertex {
    /// All-zero vertex. Triangles of it are degenerate and rasterize nothing.
    pub const EMPTY: Self = Self {
        position: [0.0; 2],
        placement: [0.0; 4],
        shape: [0.0; 4],
        color0: [0.0; 4],
        color1: [0.0; 4],
        border_color: [0.0; 4],
        gradient_coords: [0.0; 4],
        material_params: [0.0; 4],
        glow_color: [0.0; 4],
        clip: [0.0; 4],
    };
}

// GLES attribute strides and the WebGL bridge read exactly this many bytes per vertex.
const _: () = assert!(std::mem::size_of::<GuiVertex>() == 152);

/// Exterior margin in Surface metres that generated box geometry adds beyond paint.
///
/// `surface_gui.vert` declares the same value and extends exterior vertices only by
/// the part of its projected antialias footprint this margin does not already cover.
pub const GUI_BOX_ANTIALIAS_PAD: f32 = 0.002;

/// Fill type of glyph quads, after the solid (0), linear (1) and radial (2) box fills.
///
/// `surface_gui.vert` and `surface_gui.frag` declare the same value.
pub const GUI_FILL_GLYPH: f32 = 3.0;

/// Stable key identifying one live primitive's CPU geometry cache entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimitiveKey {
    /// Live entity owning the Surface component.
    pub entity: ipp_core::EntityId,
    /// Live primitive identity.
    pub identity: SurfacePrimitiveIdentity,
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

/// Cached CPU geometry for one box primitive.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedPrimitiveGeometry {
    /// Content hash of all geometry, material and clip inputs.
    pub hash: u64,
    /// Surface paint revision `hash` was computed under; zero when unknown.
    revision: u64,
    /// Whether the identity may start a batch; fixed per identity.
    boundary: bool,
    /// Explicit vertices forming triangles (counter-clockwise front face).
    pub vertices: Vec<GuiVertex>,
    /// Frame before which the box stays volatile after a geometry change.
    volatile_until: u64,
    /// Frame that last submitted the box.
    seen: u64,
}

/// One box of the run being batched.
struct RunBox {
    identity: SurfacePrimitiveIdentity,
    hash: u64,
    volatile: bool,
    boundary: bool,
}

/// Renderer-owned retained GUI geometry of one World: box CPU geometry and the GPU
/// storage of each Surface's box and glyph batches.
pub struct GuiBatchRenderCache<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    cpu_primitives: BTreeMap<PrimitiveKey, CachedPrimitiveGeometry>,
    storage: BTreeMap<ipp_core::EntityId, GuiSurfaceStorage<D>>,
    /// Sum of allocated bytes over `storage`.
    resident: usize,
    /// Surface whose pieces are being collected.
    entity: ipp_core::EntityId,
    /// Retained batches of that Surface in painter order.
    pieces: Vec<GuiPiece>,
    /// Box identities referenced by box pieces.
    piece_boxes: Vec<SurfacePrimitiveIdentity>,
    run_boxes: Vec<RunBox>,
    scratch: GuiCommitScratch,
    frame: u64,
}

impl<D: RenderDevice> GuiBatchRenderCache<D> {
    /// Create a new empty batch render cache bound to the given device.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            cpu_primitives: BTreeMap::new(),
            storage: BTreeMap::new(),
            resident: 0,
            entity: ipp_core::EntityId::from_bits(0),
            pieces: Vec::new(),
            piece_boxes: Vec::new(),
            run_boxes: Vec::new(),
            scratch: GuiCommitScratch::default(),
            frame: 0,
        }
    }

    /// Clear all retained GPU storage and CPU geometry cache entries.
    pub fn clear(&mut self) {
        let mut device = self.device.borrow_mut();
        for (_, storage) in std::mem::take(&mut self.storage) {
            storage.delete(&mut device);
        }
        self.cpu_primitives.clear();
        self.resident = 0;
        self.pieces.clear();
        self.piece_boxes.clear();
        self.run_boxes.clear();
    }

    /// Total resident bytes occupied by retained GPU storage.
    pub fn resident_bytes(&self) -> usize {
        self.resident
    }

    /// Start collecting the retained batches of one Surface submission.
    pub fn begin_surface(&mut self, entity: ipp_core::EntityId) {
        self.entity = entity;
        self.pieces.clear();
        self.piece_boxes.clear();
    }

    /// Batches collected for the current Surface so far.
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// Append one contiguous run of box primitives, each with its effective clip, as
    /// bounded batches.
    ///
    /// Stable boxes split at identity-selected boundaries; volatile boxes form their own
    /// small batches. Boxes whose retained hash was computed under the Surface's reusable
    /// `paint` revision are not hashed again.
    pub fn push_boxes(
        &mut self,
        paint: SurfacePaint,
        boxes: &[(&SurfaceRenderPrimitive, SurfaceClipRect)],
        stats: &mut RenderStats,
    ) {
        // Refresh every box's geometry and volatility before choosing batch boundaries.
        self.run_boxes.clear();
        for &(primitive, clip) in boxes {
            let SurfaceRenderPrimitive::Box {
                style,
                size,
                corner_radius,
                border_width,
                border_color,
                fill,
                glow,
            } = primitive
            else {
                continue;
            };

            let hash = || {
                hash_box_inputs(
                    style,
                    size,
                    corner_radius,
                    *border_width,
                    border_color,
                    fill,
                    glow.as_ref(),
                    clip,
                )
            };
            let generate = || {
                generate_box_vertices(
                    style,
                    size,
                    corner_radius,
                    *border_width,
                    border_color,
                    fill,
                    glow.as_ref(),
                    clip,
                )
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
                            cached.vertices = generate();
                            cached.volatile_until = self.frame + VOLATILE_FRAMES;
                            stats.gui_rebuilds += 1;
                        }
                        cached.revision = paint.revision;
                    }
                    cached
                }
                Entry::Vacant(entry) => {
                    stats.gui_rebuilds += 1;
                    entry.insert(CachedPrimitiveGeometry {
                        hash: hash(),
                        revision: paint.revision,
                        boundary: identity_starts_batch(style.identity),
                        vertices: generate(),
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
            len += self.cpu_primitives[&PrimitiveKey {
                entity: self.entity,
                identity: run_box.identity,
            }]
                .vertices
                .len();
            self.piece_boxes.push(run_box.identity);
        }

        self.pieces.push(GuiPiece {
            key: GuiPieceKey::Boxes(boxes[0].identity),
            hash: hasher.finish(),
            len,
            page: None,
            source: GuiPieceSource::Boxes(first..self.piece_boxes.len()),
        });
    }

    /// Append retained glyph batches of the current Surface, in painter order.
    pub(crate) fn push_glyphs(&mut self, pieces: impl IntoIterator<Item = GuiPiece>) {
        self.pieces.extend(pieces);
    }

    /// Place the collected batches in the Surface's storage, writing only changed ones.
    ///
    /// `glyphs` returns the vertices of atlas page batch `index` of a text run. A failed
    /// write releases the Surface's storage, so no later frame draws unknown contents.
    pub fn commit_surface<'g>(
        &mut self,
        glyphs: impl Fn(SurfacePrimitiveIdentity, u32) -> &'g [GuiVertex],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        if self.pieces.is_empty() {
            return Ok(());
        }

        let entity = self.entity;
        let mut storage = self.storage.remove(&entity);
        let before = storage.as_ref().map_or(0, GuiSurfaceStorage::bytes);
        let cpu_primitives = &self.cpu_primitives;
        let piece_boxes = &self.piece_boxes;
        let pieces = &self.pieces;
        let mut fill = |index: usize, vertices: &mut Vec<GuiVertex>| match &pieces[index].source {
            GuiPieceSource::Boxes(range) => {
                for &identity in &piece_boxes[range.clone()] {
                    vertices.extend_from_slice(
                        &cpu_primitives[&PrimitiveKey {
                            entity,
                            identity,
                        }]
                            .vertices,
                    );
                }
            }
            GuiPieceSource::Glyphs(identity, batch) => {
                vertices.extend_from_slice(glyphs(*identity, *batch));
            }
        };
        let result = commit_surface_storage(
            &mut *self.device.borrow_mut(),
            &mut storage,
            pieces,
            &mut fill,
            &mut self.scratch,
            stats,
        );

        let after = storage.as_ref().map_or(0, GuiSurfaceStorage::bytes);
        self.resident = self.resident - before + after;
        if let Some(mut storage) = storage {
            storage.seen = self.frame;
            self.storage.insert(entity, storage);
        }

        result
    }

    /// Draw collected batches `range` of the committed Surface in painter order: one
    /// draw per atlas page change. `atlas` returns the texture of an atlas page.
    pub fn draw_pieces<'t>(
        &mut self,
        program: &D::Program,
        range: std::ops::Range<usize>,
        atlas: impl Fn(usize) -> Option<&'t D::Texture>,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
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
        storage.draw(
            &mut *self.device.borrow_mut(),
            program,
            &self.pieces,
            range,
            atlas,
            mvp,
            stats,
        )
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
        }

        self.frame += 1;
    }
}

/// Whether a stable box identity may start a batch. Boundaries depend only on
/// identities, so edits elsewhere in a run keep the batches that do not contain them.
fn identity_starts_batch(identity: SurfacePrimitiveIdentity) -> bool {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut hasher);
    hasher.finish().is_multiple_of(BATCH_BOUNDARY_PERIOD)
}

impl<D: RenderDevice> Drop for GuiBatchRenderCache<D> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Compute explicit vertices for a box with counter-clockwise front winding.
///
/// Returns 6 vertices for filled/small quads, or 24 vertices (4 edge strips)
/// for large hollow shapes, including their outer glow, to skip empty interiors.
/// Every vertex carries the effective `clip`.
#[allow(clippy::too_many_arguments)]
pub fn generate_box_vertices(
    style: &SurfacePrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
    fill: &GuiShapeFill,
    glow: Option<&GuiShapeGlow>,
    clip: SurfaceClipRect,
) -> Vec<GuiVertex> {
    let pos = style.position;
    let scale = style.scale;
    let placed_size = [size[0] * scale[0], size[1] * scale[1]];

    let placement = [pos[0], pos[1], placed_size[0], placed_size[1]];
    let shape = [corner_radius[0], corner_radius[1], border_width, 0.0];
    let border = [
        border_color[0],
        border_color[1],
        border_color[2],
        border_color[3] * style.opacity,
    ];

    let (fill_type, color0, color1, gradient_coords) = match fill {
        GuiShapeFill::Solid(color) => (
            0.0f32,
            [color[0], color[1], color[2], color[3] * style.opacity],
            [color[0], color[1], color[2], color[3] * style.opacity],
            [0.0; 4],
        ),
        GuiShapeFill::LinearGradient {
            start,
            end,
            start_color,
            end_color,
        } => (
            1.0f32,
            [
                start_color[0],
                start_color[1],
                start_color[2],
                start_color[3] * style.opacity,
            ],
            [
                end_color[0],
                end_color[1],
                end_color[2],
                end_color[3] * style.opacity,
            ],
            [
                start[0] * scale[0],
                start[1] * scale[1],
                end[0] * scale[0],
                end[1] * scale[1],
            ],
        ),
        GuiShapeFill::RadialGradient {
            center,
            radius,
            start_color,
            end_color,
        } => (
            2.0f32,
            [
                start_color[0],
                start_color[1],
                start_color[2],
                start_color[3] * style.opacity,
            ],
            [
                end_color[0],
                end_color[1],
                end_color[2],
                end_color[3] * style.opacity,
            ],
            [
                center[0] * scale[0],
                center[1] * scale[1],
                *radius * scale[0].abs().max(scale[1].abs()),
                0.0,
            ],
        ),
    };

    let (glow_intensity, glow_radius, glow_falloff, glow_color) = match glow {
        Some(g) if g.is_valid() && g.intensity > 0.0 && g.radius > 0.0 => (
            g.intensity,
            g.cutoff_distance() * scale[0].abs().max(scale[1].abs()),
            g.falloff,
            [
                g.color[0],
                g.color[1],
                g.color[2],
                g.color[3] * style.opacity,
            ],
        ),
        _ => (0.0, 0.0, 1.0, [0.0; 4]),
    };

    let material_params = [fill_type, glow_intensity, glow_radius, glow_falloff];

    let pad = glow_radius.max(0.0) + GUI_BOX_ANTIALIAS_PAD;

    let x0 = pos[0].min(pos[0] + placed_size[0]) - pad;
    let y0 = pos[1].min(pos[1] + placed_size[1]) - pad;
    let x1 = pos[0].max(pos[0] + placed_size[0]) + pad;
    let y1 = pos[1].max(pos[1] + placed_size[1]) + pad;

    let make_vertex = |x: f32, y: f32| -> GuiVertex {
        GuiVertex {
            position: [x, y],
            placement,
            shape,
            color0,
            color1,
            border_color: border,
            gradient_coords,
            material_params,
            glow_color,
            clip,
        }
    };

    let make_quad = |rx0: f32, ry0: f32, rx1: f32, ry1: f32| -> [GuiVertex; 6] {
        let tl = make_vertex(rx0, ry0);
        let bl = make_vertex(rx0, ry1);
        let br = make_vertex(rx1, ry1);
        let tr = make_vertex(rx1, ry0);
        // Tri 1: TL -> BL -> BR (CCW in object space with flipped Y)
        // Tri 2: TL -> BR -> TR (CCW in object space with flipped Y)
        [tl, bl, br, tl, br, tr]
    };

    let is_border_only = border_width > 0.0
        && matches!(fill, GuiShapeFill::Solid(col) if col[3] <= 0.0 || style.opacity <= 0.0);

    let strip_thickness = border_width.max(corner_radius[0]).max(corner_radius[1]) + pad;
    let can_use_strips = is_border_only
        && placed_size[0].abs() >= 3.0 * strip_thickness
        && placed_size[1].abs() >= 3.0 * strip_thickness;

    if can_use_strips {
        let mut vertices = Vec::with_capacity(24);
        // Top strip: covers top-left, top edge, top-right
        vertices.extend_from_slice(&make_quad(x0, y0, x1, y0 + strip_thickness));
        // Bottom strip: covers bottom-left, bottom edge, bottom-right
        vertices.extend_from_slice(&make_quad(x0, y1 - strip_thickness, x1, y1));
        // Left strip: between top and bottom strips
        vertices.extend_from_slice(&make_quad(
            x0,
            y0 + strip_thickness,
            x0 + strip_thickness,
            y1 - strip_thickness,
        ));
        // Right strip: between top and bottom strips
        vertices.extend_from_slice(&make_quad(
            x1 - strip_thickness,
            y0 + strip_thickness,
            x1,
            y1 - strip_thickness,
        ));
        vertices
    } else {
        make_quad(x0, y0, x1, y1).to_vec()
    }
}

/// Hash every evaluated input that [`generate_box_vertices`] reads.
///
/// The style colour lane is excluded: boxes paint only their fill, border and glow,
/// and solid fills already carry the evaluated colour. A colour transition on a
/// gradient box therefore changes no vertex and uploads nothing.
#[allow(clippy::too_many_arguments)]
pub fn hash_box_inputs(
    style: &SurfacePrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
    fill: &GuiShapeFill,
    glow: Option<&GuiShapeGlow>,
    clip: SurfaceClipRect,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    for lane in clip {
        hasher.write_u32(lane.to_bits());
    }

    hasher.write_u32(style.position[0].to_bits());
    hasher.write_u32(style.position[1].to_bits());
    hasher.write_u32(style.scale[0].to_bits());
    hasher.write_u32(style.scale[1].to_bits());
    hasher.write_u32(style.opacity.to_bits());

    hasher.write_u32(size[0].to_bits());
    hasher.write_u32(size[1].to_bits());
    hasher.write_u32(corner_radius[0].to_bits());
    hasher.write_u32(corner_radius[1].to_bits());
    hasher.write_u32(border_width.to_bits());

    for lane in border_color {
        hasher.write_u32(lane.to_bits());
    }

    match fill {
        GuiShapeFill::Solid(color) => {
            hasher.write_u8(0);
            for lane in color {
                hasher.write_u32(lane.to_bits());
            }
        }
        GuiShapeFill::LinearGradient {
            start,
            end,
            start_color,
            end_color,
        } => {
            hasher.write_u8(1);
            hasher.write_u32(start[0].to_bits());
            hasher.write_u32(start[1].to_bits());
            hasher.write_u32(end[0].to_bits());
            hasher.write_u32(end[1].to_bits());
            for lane in start_color {
                hasher.write_u32(lane.to_bits());
            }
            for lane in end_color {
                hasher.write_u32(lane.to_bits());
            }
        }
        GuiShapeFill::RadialGradient {
            center,
            radius,
            start_color,
            end_color,
        } => {
            hasher.write_u8(2);
            hasher.write_u32(center[0].to_bits());
            hasher.write_u32(center[1].to_bits());
            hasher.write_u32(radius.to_bits());
            for lane in start_color {
                hasher.write_u32(lane.to_bits());
            }
            for lane in end_color {
                hasher.write_u32(lane.to_bits());
            }
        }
    }

    if let Some(glow) = glow {
        hasher.write_u8(1);
        for lane in glow.color {
            hasher.write_u32(lane.to_bits());
        }
        hasher.write_u32(glow.intensity.to_bits());
        hasher.write_u32(glow.radius.to_bits());
        hasher.write_u32(glow.falloff.to_bits());
    } else {
        hasher.write_u8(0);
    }

    hasher.finish()
}

#[cfg(test)]
#[path = "gui_batch_tests.rs"]
mod tests;
