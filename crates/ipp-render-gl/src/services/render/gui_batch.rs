//! Retained GUI triangle batches and safe GPU storage replacement.
//!
//! GUI controls emit parameterized box primitives. RenderService retains CPU
//! geometry and GPU batches keyed by live primitive identity and content revision.
//! Warm unchanged frames upload zero geometry bytes. Changing a control rebuilds
//! only the affected batch, uploading its complete contents via storage replacement.
//! Culled Surfaces keep their retained work; destruction, or a submission that no
//! longer uses a batch, releases it.
//!
//! Batch boundaries within a run of compatible boxes follow primitive identities rather
//! than positions, so inserting, removing or resizing a box rebuilds only its own batch.
//! A box whose geometry changed recently is volatile and batches apart from stable
//! boxes, so an animated control re-uploads only its own small batch each frame.
//! Volatility is the only partition: backgrounds, fills, icons and focus rings of one
//! clip share batches. The clip is a draw-time uniform: clip-only changes reuse vertex
//! storage.

use std::cell::RefCell;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::surface_paint::SurfacePaint;
use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::systems::surface::{
    GuiShapeFill, GuiShapeGlow, SurfaceClipRect, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
    SurfaceRenderPrimitive,
};

/// One vertex in a non-indexed GUI triangle batch (136 bytes).
///
/// Shape parameters, material stops and glow properties are associated with every
/// vertex so multiple compatible boxes can be batched into one draw call without
/// uniforms or instancing.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiBoxVertex {
    /// Local position in Surface content metres `[x, y]`.
    pub position: [f32; 2],
    /// Placed origin and size in Surface metres `[pos_x, pos_y, size_x, size_y]`.
    pub placement: [f32; 4],
    /// Shape metrics `[corner_rx, corner_ry, border_width, reserved]`.
    pub shape: [f32; 4],
    /// Fill start / solid straight linear RGBA.
    pub color0: [f32; 4],
    /// Fill end straight linear RGBA (for gradients).
    pub color1: [f32; 4],
    /// Border straight linear RGBA.
    pub border_color: [f32; 4],
    /// Gradient coordinates `[start_x, start_y, end_x, end_y]` or `[center_x, center_y, radius, 0.0]`.
    pub gradient_coords: [f32; 4],
    /// Material parameters: `[fill_type, glow_intensity, glow_radius, glow_falloff]`.
    pub material_params: [f32; 4],
    /// Glow straight linear RGBA.
    pub glow_color: [f32; 4],
}

// GLES attribute strides and the WebGL bridge read exactly this many bytes per vertex.
const _: () = assert!(std::mem::size_of::<GuiBoxVertex>() == 136);

/// Exterior margin in Surface metres that generated box geometry adds beyond paint.
///
/// `surface_box.vert` declares the same value and extends exterior vertices only by
/// the part of its projected antialias footprint this margin does not already cover.
pub const GUI_BOX_ANTIALIAS_PAD: f32 = 0.002;

/// Surface participation in one completed frame, deciding which retained work is stale.
///
/// Destroyed Surfaces release everything. A submitted Surface drew every primitive it
/// still owns, so its unused work is stale. Live Surfaces skipped by culling keep their
/// retained work for the frame they become visible again.
pub struct RetainedSurfaceSubmission<'a> {
    /// Every live Surface entity in the World.
    pub live: &'a BTreeSet<ipp_core::EntityId>,
    /// Surfaces whose primitives this frame submitted.
    pub submitted: &'a BTreeSet<ipp_core::EntityId>,
}

impl RetainedSurfaceSubmission<'_> {
    /// Whether work retained for `entity` is stale, given whether this frame used it.
    pub fn is_stale(&self, entity: ipp_core::EntityId, used: bool) -> bool {
        !self.live.contains(&entity) || (!used && self.submitted.contains(&entity))
    }
}

/// Stable key identifying one retained GPU batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuiBatchKey {
    /// Live entity owning the Surface component.
    pub entity: ipp_core::EntityId,
    /// Identity of the initial primitive in this batch.
    pub initial_primitive: SurfacePrimitiveIdentity,
}

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
    /// Content hash of all geometry and material inputs.
    pub hash: u64,
    /// Surface paint revision `hash` was computed under; zero when unknown.
    revision: u64,
    /// Whether the identity may start a batch; fixed per identity.
    boundary: bool,
    /// Explicit vertices forming triangles (counter-clockwise front face).
    pub vertices: Vec<GuiBoxVertex>,
    /// Frame before which the box stays volatile after a geometry change.
    volatile_until: u64,
    /// Frame that last submitted the box.
    seen: u64,
}

/// Retained GPU batch holding a device buffer handle and revision metadata.
pub struct RetainedGuiBatch<D: RenderDevice> {
    /// Context-owned GPU batch buffer.
    pub gpu: D::GuiBatch,
    /// Combined content hash of the constituent primitives; the clip is excluded.
    pub hash: u64,
    /// Allocated GPU bytes.
    pub bytes: usize,
    /// Total vertex count.
    pub vertex_count: usize,
    /// Frame that last drew the batch.
    seen: u64,
}

/// One box of the run being batched.
struct RunBox {
    identity: SurfacePrimitiveIdentity,
    hash: u64,
    volatile: bool,
    boundary: bool,
}

/// Renderer-owned retained cache of CPU geometry and GPU batches.
pub struct GuiBatchRenderCache<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    retained_batches: BTreeMap<GuiBatchKey, RetainedGuiBatch<D>>,
    cpu_primitives: BTreeMap<PrimitiveKey, CachedPrimitiveGeometry>,
    /// Sum of `bytes` over `retained_batches`.
    resident: usize,
    scratch_vertices: Vec<GuiBoxVertex>,
    run_boxes: Vec<RunBox>,
    frame: u64,
}

impl<D: RenderDevice> GuiBatchRenderCache<D> {
    /// Create a new empty batch render cache bound to the given device.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            retained_batches: BTreeMap::new(),
            cpu_primitives: BTreeMap::new(),
            resident: 0,
            scratch_vertices: Vec::new(),
            run_boxes: Vec::new(),
            frame: 0,
        }
    }

    /// Clear all retained GPU batches and CPU geometry cache entries.
    pub fn clear(&mut self) {
        let mut device = self.device.borrow_mut();
        for (_, batch) in std::mem::take(&mut self.retained_batches) {
            device.delete_gui_batch(batch.gpu);
        }
        self.cpu_primitives.clear();
        self.resident = 0;
        self.scratch_vertices.clear();
        self.run_boxes.clear();
    }

    /// Total resident bytes occupied by retained GPU batch allocations.
    pub fn resident_bytes(&self) -> usize {
        self.resident
    }

    /// Submit one contiguous run of compatible box primitives as bounded batches.
    ///
    /// Stable boxes split at identity-selected boundaries; volatile boxes form their own
    /// small batches. Each batch replaces its complete storage only when its boxes change.
    /// Boxes whose retained hash was computed under the Surface's reusable `paint`
    /// revision are not hashed again.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_box_batch(
        &mut self,
        program: &D::Program,
        entity: ipp_core::EntityId,
        clip: SurfaceClipRect,
        paint: SurfacePaint,
        boxes: &[&SurfaceRenderPrimitive],
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        // Refresh every box's geometry and volatility before choosing batch boundaries.
        self.run_boxes.clear();
        for &primitive in boxes {
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
                )
            };
            let key = PrimitiveKey {
                entity,
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

            self.draw_run_batch(program, entity, clip, start..index, mvp, stats)?;
            start = index;
        }

        Ok(())
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

    /// Draw one batch of the current run, replacing its storage only when it changed.
    #[allow(clippy::too_many_arguments)]
    fn draw_run_batch(
        &mut self,
        program: &D::Program,
        entity: ipp_core::EntityId,
        clip: SurfaceClipRect,
        range: std::ops::Range<usize>,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        let boxes = &self.run_boxes[range];
        let batch_key = GuiBatchKey {
            entity,
            initial_primitive: boxes[0].identity,
        };

        let mut batch_hasher = std::collections::hash_map::DefaultHasher::new();
        for run_box in boxes {
            run_box.identity.hash(&mut batch_hasher);
            batch_hasher.write_u64(run_box.hash);
        }
        let batch_hash = batch_hasher.finish();

        self.scratch_vertices.clear();
        let unchanged = self
            .retained_batches
            .get(&batch_key)
            .is_some_and(|retained| retained.hash == batch_hash);
        if !unchanged {
            for run_box in boxes {
                self.scratch_vertices.extend_from_slice(
                    &self.cpu_primitives[&PrimitiveKey {
                        entity,
                        identity: run_box.identity,
                    }]
                        .vertices,
                );
            }
        }

        if let Some(retained) = self.retained_batches.get_mut(&batch_key) {
            retained.seen = self.frame;

            if !unchanged {
                // Batch is dirty: replace GPU storage with complete contents.
                let replaced = self
                    .device
                    .borrow_mut()
                    .update_gui_batch(&mut retained.gpu, &self.scratch_vertices);
                if let Err(error) = replaced {
                    // A failed replacement leaves the storage contents unknown. Release the
                    // batch so no later frame draws it with stale counts or vertices.
                    if let Some(batch) = self.retained_batches.remove(&batch_key) {
                        self.resident -= batch.bytes;
                        self.device.borrow_mut().delete_gui_batch(batch.gpu);
                    }
                    return Err(error);
                }

                let bytes = self.scratch_vertices.len() * std::mem::size_of::<GuiBoxVertex>();
                self.resident = self.resident - retained.bytes + bytes;
                retained.hash = batch_hash;
                retained.bytes = bytes;
                retained.vertex_count = self.scratch_vertices.len();

                stats.uploaded_bytes = stats.uploaded_bytes.saturating_add(bytes as u32);
                stats.gui_allocations += 1;
            }

            // An unchanged batch uploads zero geometry bytes, whatever its clip.
            self.device
                .borrow_mut()
                .draw_gui_batch(program, &retained.gpu, mvp, &clip)?;
            stats.draw_calls += 1;
            stats.triangles += (retained.vertex_count / 3) as u32;
            stats.gui_batches += 1;
            return Ok(());
        }

        // New batch allocation.
        let gpu = self
            .device
            .borrow_mut()
            .create_gui_batch(&self.scratch_vertices)?;
        let bytes = self.scratch_vertices.len() * std::mem::size_of::<GuiBoxVertex>();
        let vertex_count = self.scratch_vertices.len();

        stats.uploaded_bytes = stats.uploaded_bytes.saturating_add(bytes as u32);
        stats.gui_allocations += 1;

        let result = self
            .device
            .borrow_mut()
            .draw_gui_batch(program, &gpu, mvp, &clip);
        if let Err(error) = result {
            self.device.borrow_mut().delete_gui_batch(gpu);
            return Err(error);
        }
        stats.draw_calls += 1;
        stats.triangles += (vertex_count / 3) as u32;
        stats.gui_batches += 1;

        self.resident += bytes;
        self.retained_batches.insert(
            batch_key,
            RetainedGuiBatch {
                gpu,
                hash: batch_hash,
                bytes,
                vertex_count,
                seen: self.frame,
            },
        );

        Ok(())
    }

    /// End-of-frame maintenance: release stale GPU batches and CPU geometry.
    ///
    /// `surfaces` is `None` when submission did not complete. Unused keys then cannot be
    /// told apart from undrawn ones, so every retained batch is kept.
    pub fn finish_frame(&mut self, surfaces: Option<&RetainedSurfaceSubmission<'_>>) {
        if let Some(surfaces) = surfaces {
            let frame = self.frame;
            let stale: Vec<GuiBatchKey> = self
                .retained_batches
                .iter()
                .filter(|(key, batch)| surfaces.is_stale(key.entity, batch.seen == frame))
                .map(|(key, _)| *key)
                .collect();

            let mut device = self.device.borrow_mut();
            for key in stale {
                if let Some(batch) = self.retained_batches.remove(&key) {
                    self.resident -= batch.bytes;
                    device.delete_gui_batch(batch.gpu);
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
pub fn generate_box_vertices(
    style: &SurfacePrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
    fill: &GuiShapeFill,
    glow: Option<&GuiShapeGlow>,
) -> Vec<GuiBoxVertex> {
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

    let make_vertex = |x: f32, y: f32| -> GuiBoxVertex {
        GuiBoxVertex {
            position: [x, y],
            placement,
            shape,
            color0,
            color1,
            border_color: border,
            gradient_coords,
            material_params,
            glow_color,
        }
    };

    let make_quad = |rx0: f32, ry0: f32, rx1: f32, ry1: f32| -> [GuiBoxVertex; 6] {
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
pub fn hash_box_inputs(
    style: &SurfacePrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
    fill: &GuiShapeFill,
    glow: Option<&GuiShapeGlow>,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

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
