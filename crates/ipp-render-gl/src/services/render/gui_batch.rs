//! Retained GUI triangle batches and safe GPU storage replacement.
//!
//! GUI controls emit parameterized box primitives. RenderService retains CPU
//! geometry and GPU batches keyed by live primitive identity and content revision.
//! Warm unchanged frames upload zero geometry bytes. Changing a control rebuilds
//! only the affected batch, uploading its complete contents via storage replacement.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::systems::surface::{
    GuiPrimitivePart, GuiShapeFill, GuiShapeGlow, SurfaceClipRect, SurfacePrimitiveIdentity,
    SurfacePrimitiveStyle, SurfaceRenderPrimitive,
};

/// One vertex in a non-indexed GUI triangle batch (72 bytes).
///
/// Shape parameters are associated with every vertex so multiple compatible boxes
/// can be batched into one draw call without uniforms or instancing.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiBoxVertex {
    /// Local position in Surface content metres `[x, y]`.
    pub position: [f32; 2],
    /// Placed origin and size in Surface metres `[pos_x, pos_y, size_x, size_y]`.
    pub placement: [f32; 4],
    /// Straight linear RGBA fill tint/color.
    pub color: [f32; 4],
    /// Straight linear RGBA border color.
    pub border_color: [f32; 4],
    /// Shape metrics `[corner_rx, corner_ry, border_width, reserved]`.
    pub shape: [f32; 4],
}

/// Compatibility partition distinguishing frequently changing cursor/control work
/// from static backgrounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPartClass {
    /// Resizable panel, container and card backgrounds.
    Background,
    /// Slider value fill and progress bars.
    Fill,
    /// Transient focus indicator ring.
    FocusRing,
    /// Authored items or unclassified parts.
    Other,
}

impl GuiPartClass {
    /// Classify a primitive identity into a volatility partition.
    pub fn from_identity(identity: SurfacePrimitiveIdentity) -> Self {
        match identity {
            SurfacePrimitiveIdentity::Gui(gui) => match gui.part {
                GuiPrimitivePart::Background => Self::Background,
                GuiPrimitivePart::Fill => Self::Fill,
                GuiPrimitivePart::FocusRing => Self::FocusRing,
                _ => Self::Other,
            },
            SurfacePrimitiveIdentity::Authored(_) => Self::Other,
        }
    }
}

/// Stable key identifying one retained GPU batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuiBatchKey {
    /// Live entity owning the Surface component.
    pub entity: ipp_core::EntityId,
    /// Identity of the initial primitive in this batch.
    pub initial_primitive: SurfacePrimitiveIdentity,
    /// Volatility partition of the batch.
    pub part_class: GuiPartClass,
}

/// Stable key identifying one live primitive's CPU geometry cache entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimitiveKey {
    /// Live entity owning the Surface component.
    pub entity: ipp_core::EntityId,
    /// Live primitive identity.
    pub identity: SurfacePrimitiveIdentity,
}

/// Cached CPU geometry for one box primitive (6 vertices forming 2 triangles).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CachedPrimitiveGeometry {
    /// Content hash of all geometry and material inputs.
    pub hash: u64,
    /// Explicit 6-vertex quad (counter-clockwise front face).
    pub vertices: [GuiBoxVertex; 6],
}

/// Retained GPU batch holding a device buffer handle and revision metadata.
pub struct RetainedGuiBatch<D: RenderDevice> {
    /// Context-owned GPU batch buffer.
    pub gpu: D::GuiBatch,
    /// Effective clip rectangle in Surface metres.
    pub clip: SurfaceClipRect,
    /// Combined content hash of all constituent primitives and clip.
    pub hash: u64,
    /// Allocated GPU bytes.
    pub bytes: usize,
    /// Total vertex count (`primitive_count * 6`).
    pub vertex_count: usize,
}

/// Renderer-owned retained cache of CPU geometry and GPU batches.
pub struct GuiBatchRenderCache<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    retained_batches: BTreeMap<GuiBatchKey, RetainedGuiBatch<D>>,
    cpu_primitives: BTreeMap<PrimitiveKey, CachedPrimitiveGeometry>,
    used_batches: BTreeSet<GuiBatchKey>,
    used_primitives: BTreeSet<PrimitiveKey>,
    scratch_vertices: Vec<GuiBoxVertex>,
}

impl<D: RenderDevice> GuiBatchRenderCache<D> {
    /// Construct a new empty cache borrowing the shared render device.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            retained_batches: BTreeMap::new(),
            cpu_primitives: BTreeMap::new(),
            used_batches: BTreeSet::new(),
            used_primitives: BTreeSet::new(),
            scratch_vertices: Vec::new(),
        }
    }

    /// Discard all retained GPU batches and CPU geometry (e.g. on context loss).
    pub fn clear(&mut self) {
        self.retained_batches.clear();
        self.cpu_primitives.clear();
        self.used_batches.clear();
        self.used_primitives.clear();
        self.scratch_vertices.clear();
    }

    /// Return total resident GPU bytes across all live retained batches.
    pub fn resident_bytes(&self) -> usize {
        self.retained_batches.values().map(|b| b.bytes).sum()
    }

    /// Submit a run of compatible consecutive box primitives as a retained batch.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_box_batch(
        &mut self,
        program: &D::Program,
        entity: ipp_core::EntityId,
        clip: SurfaceClipRect,
        part_class: GuiPartClass,
        boxes: &[&SurfaceRenderPrimitive],
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        if boxes.is_empty() {
            return Ok(());
        }

        let initial_identity = boxes[0].style().identity;
        let batch_key = GuiBatchKey {
            entity,
            initial_primitive: initial_identity,
            part_class,
        };

        self.used_batches.insert(batch_key);
        self.scratch_vertices.clear();

        let mut batch_hasher = std::collections::hash_map::DefaultHasher::new();
        for coord in clip {
            batch_hasher.write_u32(coord.to_bits());
        }

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

            let prim_key = PrimitiveKey {
                entity,
                identity: style.identity,
            };
            self.used_primitives.insert(prim_key);

            let prim_hash = hash_box_inputs(
                style,
                size,
                corner_radius,
                *border_width,
                border_color,
                fill,
                glow.as_ref(),
            );

            style.identity.hash(&mut batch_hasher);
            batch_hasher.write_u64(prim_hash);

            let vertices = match self.cpu_primitives.get(&prim_key) {
                Some(cached) if cached.hash == prim_hash => cached.vertices,
                _ => {
                    let quad = generate_box_vertices(
                        style,
                        size,
                        corner_radius,
                        *border_width,
                        border_color,
                    );
                    self.cpu_primitives.insert(
                        prim_key,
                        CachedPrimitiveGeometry {
                            hash: prim_hash,
                            vertices: quad,
                        },
                    );
                    stats.gui_rebuilds += 1;
                    quad
                }
            };

            self.scratch_vertices.extend_from_slice(&vertices);
        }

        let batch_hash = batch_hasher.finish();

        if let Some(retained) = self.retained_batches.get_mut(&batch_key) {
            if retained.hash == batch_hash {
                // Batch is completely warm and unchanged: zero geometry bytes uploaded!
                self.device
                    .borrow_mut()
                    .draw_gui_batch(program, &retained.gpu, mvp, &clip)?;
                stats.draw_calls += 1;
                stats.triangles += (retained.vertex_count / 3) as u32;
                stats.gui_batches += 1;
                return Ok(());
            }

            // Batch is dirty: replace GPU storage with complete contents.
            self.device
                .borrow_mut()
                .update_gui_batch(&mut retained.gpu, &self.scratch_vertices)?;
            let bytes = self.scratch_vertices.len() * std::mem::size_of::<GuiBoxVertex>();
            retained.hash = batch_hash;
            retained.clip = clip;
            retained.bytes = bytes;
            retained.vertex_count = self.scratch_vertices.len();

            stats.uploaded_bytes = stats.uploaded_bytes.saturating_add(bytes as u32);
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

        self.device
            .borrow_mut()
            .draw_gui_batch(program, &gpu, mvp, &clip)?;
        stats.draw_calls += 1;
        stats.triangles += (vertex_count / 3) as u32;
        stats.gui_batches += 1;

        self.retained_batches.insert(
            batch_key,
            RetainedGuiBatch {
                gpu,
                clip,
                hash: batch_hash,
                bytes,
                vertex_count,
            },
        );

        Ok(())
    }

    /// End-of-frame maintenance: prune unreferenced batches and account resident memory.
    pub fn finish_frame(
        &mut self,
        device: &mut D,
        live_surfaces: &BTreeSet<ipp_core::EntityId>,
        stats: &mut RenderStats,
    ) {
        // Prune batches: remove batches belonging to destroyed surfaces or unreferenced
        // within an active surface that was rendered.
        let dead_keys: Vec<GuiBatchKey> = self
            .retained_batches
            .keys()
            .copied()
            .filter(|key| !live_surfaces.contains(&key.entity) || !self.used_batches.contains(key))
            .collect();

        for key in dead_keys {
            if let Some(batch) = self.retained_batches.remove(&key) {
                device.delete_gui_batch(batch.gpu);
            }
        }

        // Prune dead CPU primitive geometries.
        self.cpu_primitives.retain(|key, _| {
            live_surfaces.contains(&key.entity) && self.used_primitives.contains(key)
        });

        self.used_batches.clear();
        self.used_primitives.clear();

        stats.gui_resident_bytes = self.resident_bytes() as u32;
    }
}

/// Compute 6 explicit vertices for a quad with counter-clockwise front winding.
pub fn generate_box_vertices(
    style: &SurfacePrimitiveStyle,
    size: &[f32; 2],
    corner_radius: &[f32; 2],
    border_width: f32,
    border_color: &[f32; 4],
) -> [GuiBoxVertex; 6] {
    let pos = style.position;
    let scale = style.scale;
    let placed_size = [size[0] * scale[0], size[1] * scale[1]];

    let placement = [pos[0], pos[1], placed_size[0], placed_size[1]];
    let color = [
        style.color[0],
        style.color[1],
        style.color[2],
        style.color[3] * style.opacity,
    ];
    let border = [
        border_color[0],
        border_color[1],
        border_color[2],
        border_color[3] * style.opacity,
    ];
    let shape = [corner_radius[0], corner_radius[1], border_width, 0.0];

    let x0 = pos[0];
    let y0 = pos[1];
    let x1 = pos[0] + placed_size[0];
    let y1 = pos[1] + placed_size[1];

    let v_tl = GuiBoxVertex {
        position: [x0, y0],
        placement,
        color,
        border_color: border,
        shape,
    };
    let v_bl = GuiBoxVertex {
        position: [x0, y1],
        placement,
        color,
        border_color: border,
        shape,
    };
    let v_br = GuiBoxVertex {
        position: [x1, y1],
        placement,
        color,
        border_color: border,
        shape,
    };
    let v_tr = GuiBoxVertex {
        position: [x1, y0],
        placement,
        color,
        border_color: border,
        shape,
    };

    // Tri 1: TL -> BL -> BR (CCW in object space with flipped Y)
    // Tri 2: TL -> BR -> TR (CCW in object space with flipped Y)
    [v_tl, v_bl, v_br, v_tl, v_br, v_tr]
}

/// Hash all evaluated geometry and material lanes of a box primitive.
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

    for lane in style.color {
        hasher.write_u32(lane.to_bits());
    }
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
