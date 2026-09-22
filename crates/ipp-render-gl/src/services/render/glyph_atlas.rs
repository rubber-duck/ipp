//! Retained text geometry batches and shared glyph coverage atlases.
//!
//! Repeated text quads sample coverage images from shared atlas pages.
//! Entries are keyed by font identity, glyph ID and resolution band.
//! Retained batches reuse local vertex geometry across camera motion within
//! a stable resolution band and rebuild only when affected text edits occur.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use ipp_core::services::asset_management::AssetKey;
use ipp_core::systems::surface::{
    SurfaceClipRect, SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
};

use crate::{RenderDevice, RenderError, RenderStats};

/// Page width and height in texels for each atlas page texture.
pub const ATLAS_PAGE_SIZE: u32 = 512;

/// Maximum number of resident atlas pages before allocation rejection.
pub const MAX_ATLAS_PAGES: usize = 4;

/// Maximum number of new glyph coverage entries populated per frame.
pub const MAX_POPULATES_PER_FRAME: usize = 32;

/// Supported discrete resolution bands (nominal pixel heights per em).
pub const RESOLUTION_BANDS: [u16; 5] = [16, 24, 32, 48, 64];

/// Select a stable presentation resolution band for the requested pixel height.
///
/// Returns `None` for extreme scales (< 10.0 or > 80.0 px) which fall back to
/// analytic curve rendering.
pub fn select_resolution_band(pixel_height: f32) -> Option<u16> {
    if pixel_height < 10.0 || pixel_height > 80.0 {
        return None;
    }

    for &band in &RESOLUTION_BANDS {
        if band as f32 >= pixel_height * 0.88 {
            return Some(band);
        }
    }

    Some(RESOLUTION_BANDS[RESOLUTION_BANDS.len() - 1])
}

/// One vertex in a retained glyph quad batch (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphVertex {
    /// Placed position in Surface content metres `[x, y]`.
    pub position: [f32; 2],
    /// Normalized atlas UV coordinates `[u, v]`.
    pub uv: [f32; 2],
    /// Straight linear RGBA color tint.
    pub color: [f32; 4],
}

/// Cache identity for a single cached glyph coverage entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphKey {
    /// Ready font asset incarnation (slot plus generation).
    pub font_key: AssetKey,
    /// Original font glyph identifier.
    pub glyph_id: u32,
    /// Rasterization resolution band.
    pub resolution_band: u16,
}

/// Metadata and normalized atlas coordinates for a cached glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphAtlasEntry {
    /// Atlas page index owning this entry's texture.
    pub page_index: usize,
    /// Normalized UV bounds `[u0, v0, u1, v1]` within the atlas page texture.
    pub uv: [f32; 4],
    /// Rasterized pixel dimensions `[width, height]`.
    pub pixel_size: [u32; 2],
    /// Font unit bounds `[min_x, -max_y, max_x, -min_y]` (Y-down).
    pub font_bounds: [f32; 4],
}

/// One allocated atlas page texture and its shelf-packing state.
pub struct AtlasPage<D: RenderDevice> {
    /// Context-owned atlas page texture and framebuffer.
    pub handle: D::GlyphAtlasPage,
    current_x: u32,
    current_y: u32,
    row_height: u32,
    entries_count: usize,
}

impl<D: RenderDevice> AtlasPage<D> {
    /// Create a new empty atlas page bound to the given device allocation.
    pub fn new(handle: D::GlyphAtlasPage) -> Self {
        Self {
            handle,
            current_x: 0,
            current_y: 0,
            row_height: 0,
            entries_count: 0,
        }
    }

    /// Allocate a padded slot of `slot_width x slot_height` on this page.
    pub fn allocate(&mut self, slot_width: u32, slot_height: u32) -> Option<[u32; 2]> {
        if slot_width > ATLAS_PAGE_SIZE || slot_height > ATLAS_PAGE_SIZE {
            return None;
        }

        if self.current_x + slot_width > ATLAS_PAGE_SIZE {
            self.current_x = 0;
            self.current_y += self.row_height;
            self.row_height = 0;
        }

        if self.current_y + slot_height > ATLAS_PAGE_SIZE {
            return None;
        }

        let pos = [self.current_x, self.current_y];
        self.current_x += slot_width;
        self.row_height = self.row_height.max(slot_height);
        self.entries_count += 1;
        Some(pos)
    }
}

/// Renderer-owned shared glyph coverage atlas.
pub struct GlyphAtlas<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    pages: Vec<AtlasPage<D>>,
    entries: BTreeMap<GlyphKey, GlyphAtlasEntry>,
}

impl<D: RenderDevice> GlyphAtlas<D> {
    /// Create an empty glyph atlas bound to the given render device.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            pages: Vec::new(),
            entries: BTreeMap::new(),
        }
    }

    /// Release all atlas pages and invalidate all cached glyph entries.
    pub fn clear(&mut self) {
        let mut device = self.device.borrow_mut();
        for page in self.pages.drain(..) {
            device.delete_glyph_atlas_page(page.handle);
        }
        self.entries.clear();
    }

    /// Number of resident atlas pages.
    pub fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }

    /// Total resident bytes occupied by atlas page textures (RGBA8).
    pub fn resident_bytes(&self) -> usize {
        self.pages.len() * (ATLAS_PAGE_SIZE as usize * ATLAS_PAGE_SIZE as usize * 4)
    }

    /// Look up a cached glyph entry.
    pub fn get(&self, key: &GlyphKey) -> Option<&GlyphAtlasEntry> {
        self.entries.get(key)
    }

    /// Remove a cached glyph entry.
    pub fn remove(&mut self, key: &GlyphKey) -> Option<GlyphAtlasEntry> {
        self.entries.remove(key)
    }

    /// Borrow the underlying color texture of an atlas page for sampling.
    pub fn page_texture(&self, page_index: usize) -> Option<&D::Texture> {
        let page = self.pages.get(page_index)?;
        // SAFETY: The page handle is retained inside self.pages and remains valid.
        let device_ref = unsafe { &*self.device.as_ptr() };
        Some(device_ref.glyph_atlas_texture(&page.handle))
    }

    /// Allocate slot coordinates and UV mapping for a new glyph entry.
    ///
    /// The slot is allocated with a 1-pixel border on each side to prevent
    /// bilinear sampling bleed.
    pub fn allocate_slot(
        &mut self,
        key: GlyphKey,
        px_width: u32,
        px_height: u32,
        font_bounds: [f32; 4],
    ) -> Result<([u32; 2], usize, GlyphAtlasEntry), RenderError> {
        let slot_w = px_width + 2;
        let slot_h = px_height + 2;

        let mut target_page = None;
        let mut slot_pos = None;

        for (idx, page) in self.pages.iter_mut().enumerate() {
            if let Some(pos) = page.allocate(slot_w, slot_h) {
                target_page = Some(idx);
                slot_pos = Some(pos);
                break;
            }
        }

        if target_page.is_none() {
            if self.pages.len() >= MAX_ATLAS_PAGES {
                return Err(RenderError::RenderDevice(
                    "glyph atlas page capacity exceeded".into(),
                ));
            }

            let handle = self
                .device
                .borrow_mut()
                .create_glyph_atlas_page(ATLAS_PAGE_SIZE, ATLAS_PAGE_SIZE)?;
            let mut page = AtlasPage::new(handle);
            let pos = page
                .allocate(slot_w, slot_h)
                .ok_or_else(|| RenderError::RenderDevice("glyph exceeds atlas page size".into()))?;
            let idx = self.pages.len();
            self.pages.push(page);
            target_page = Some(idx);
            slot_pos = Some(pos);
        }

        let page_index = target_page.unwrap();
        let [slot_x, slot_y] = slot_pos.unwrap();

        // 1-pixel border offset: content starts at [slot_x + 1, slot_y + 1]
        let content_x = slot_x + 1;
        let content_y = slot_y + 1;
        let dim = ATLAS_PAGE_SIZE as f32;
        let uv = [
            content_x as f32 / dim,
            1.0 - content_y as f32 / dim,
            (content_x + px_width) as f32 / dim,
            1.0 - (content_y + px_height) as f32 / dim,
        ];

        let entry = GlyphAtlasEntry {
            page_index,
            uv,
            pixel_size: [px_width, px_height],
            font_bounds,
        };

        self.entries.insert(key, entry);
        Ok(([content_x, content_y], page_index, entry))
    }

    /// Access the raw page handle for drawing.
    pub fn page_handle(&self, page_index: usize) -> Option<&D::GlyphAtlasPage> {
        self.pages.get(page_index).map(|p| &p.handle)
    }
}

/// Stable key identifying one retained text run's GPU batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextRunKey {
    /// Live entity owning the Surface component.
    pub entity: ipp_core::EntityId,
    /// Stable primitive identity.
    pub identity: SurfacePrimitiveIdentity,
}

/// Retained GPU batch holding text glyph triangle geometry.
pub struct RetainedGlyphBatch<D: RenderDevice> {
    /// Context-owned GPU batch buffer.
    pub gpu: D::GlyphBatch,
    /// Atlas page index this batch samples from.
    pub page_index: usize,
    /// Effective clip rectangle in Surface metres.
    pub clip: SurfaceClipRect,
    /// Content hash of glyph layout, font incarnation, colors and clip.
    pub hash: u64,
    /// Allocated GPU bytes.
    pub bytes: usize,
    /// Total vertex count.
    pub vertex_count: usize,
    /// Active resolution band.
    pub resolution_band: u16,
}

/// Renderer-owned retained cache of text run GPU batches.
pub struct GlyphBatchRenderCache<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    retained_runs: BTreeMap<TextRunKey, RetainedGlyphBatch<D>>,
    scratch_vertices: Vec<GlyphVertex>,
    used_runs: BTreeSet<TextRunKey>,
}

impl<D: RenderDevice> GlyphBatchRenderCache<D> {
    /// Create a new empty text batch render cache.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            retained_runs: BTreeMap::new(),
            scratch_vertices: Vec::new(),
            used_runs: BTreeSet::new(),
        }
    }

    /// Clear all retained GPU text batches.
    pub fn clear(&mut self) {
        let mut device = self.device.borrow_mut();
        let runs = std::mem::take(&mut self.retained_runs);
        for (_, run) in runs {
            device.delete_glyph_batch(run.gpu);
        }
        self.scratch_vertices.clear();
        self.used_runs.clear();
    }

    /// Total resident bytes occupied by retained text run GPU batches.
    pub fn resident_bytes(&self) -> usize {
        self.retained_runs.values().map(|r| r.bytes).sum()
    }

    /// Submit one retained text run. Reuses existing GPU storage when unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_run(
        &mut self,
        program: &D::Program,
        atlas: &GlyphAtlas<D>,
        entity: ipp_core::EntityId,
        clip: SurfaceClipRect,
        style: &SurfacePrimitiveStyle,
        font_key: AssetKey,
        font_size: f32,
        font_units_per_em: u32,
        glyphs: &[SurfaceGlyph],
        resolution_band: u16,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        let run_key = TextRunKey {
            entity,
            identity: style.identity,
        };
        self.used_runs.insert(run_key);

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        hasher.write_u64(font_key.to_u64());
        hasher.write_u32(font_size.to_bits());
        hasher.write_u32(font_units_per_em);
        hasher.write_u16(resolution_band);
        for &coord in &clip {
            hasher.write_u32(coord.to_bits());
        }
        for &coord in &style.position {
            hasher.write_u32(coord.to_bits());
        }
        for &coord in &style.scale {
            hasher.write_u32(coord.to_bits());
        }
        for &coord in &style.color {
            hasher.write_u32(coord.to_bits());
        }
        hasher.write_u32(style.opacity.to_bits());
        for glyph in glyphs {
            hasher.write_u32(glyph.glyph_id);
            hasher.write_u32(glyph.position[0].to_bits());
            hasher.write_u32(glyph.position[1].to_bits());
            if let Some(col) = glyph.color {
                for c in col {
                    hasher.write_u32(c.to_bits());
                }
            }
        }
        let hash = hasher.finish();

        if let Some(retained) = self.retained_runs.get_mut(&run_key) {
            let matches = retained.hash == hash && retained.resolution_band == resolution_band;
            let texture = matches
                .then(|| atlas.page_texture(retained.page_index))
                .flatten();
            if let Some(texture) = texture {
                self.device.borrow_mut().draw_glyph_batch(
                    program,
                    &retained.gpu,
                    texture,
                    mvp,
                    &clip,
                )?;
                stats.draw_calls += 1;
                stats.triangles += (retained.vertex_count / 3) as u32;
                stats.gui_batches += 1;
                return Ok(());
            }
        }

        self.scratch_vertices.clear();
        let mut target_page_index = 0;
        let unit = font_size / font_units_per_em as f32;

        for glyph in glyphs {
            let key = GlyphKey {
                font_key,
                glyph_id: glyph.glyph_id,
                resolution_band,
            };
            let Some(entry) = atlas.get(&key) else {
                continue;
            };
            target_page_index = entry.page_index;

            let b = entry.font_bounds;
            if b[0] >= b[2] || b[1] >= b[3] {
                continue; // Omit empty/whitespace glyphs
            }

            let tint = glyph.color.unwrap_or(style.color);
            let color = [tint[0], tint[1], tint[2], tint[3] * style.opacity];
            if color[3] <= 0.0 {
                continue;
            }

            let origin_x = style.position[0] + glyph.position[0] * style.scale[0];
            let origin_y = style.position[1] + glyph.position[1] * style.scale[1];

            let x0 = origin_x + b[0] * unit * style.scale[0];
            let y0 = origin_y + b[1] * unit * style.scale[1];
            let x1 = origin_x + b[2] * unit * style.scale[0];
            let y1 = origin_y + b[3] * unit * style.scale[1];

            let [u0, v0, u1, v1] = entry.uv;

            // Emit 6 vertices: CCW front face [TL, BL, BR, TL, BR, TR]
            let tl = GlyphVertex {
                position: [x0, y0],
                uv: [u0, v0],
                color,
            };
            let bl = GlyphVertex {
                position: [x0, y1],
                uv: [u0, v1],
                color,
            };
            let br = GlyphVertex {
                position: [x1, y1],
                uv: [u1, v1],
                color,
            };
            let tr = GlyphVertex {
                position: [x1, y0],
                uv: [u1, v0],
                color,
            };

            self.scratch_vertices
                .extend_from_slice(&[tl, bl, br, tl, br, tr]);
        }

        if self.scratch_vertices.is_empty() {
            return Ok(());
        }

        let vertex_count = self.scratch_vertices.len();
        let bytes = std::mem::size_of_val(self.scratch_vertices.as_slice());

        let Some(texture) = atlas.page_texture(target_page_index) else {
            return Err(RenderError::RenderDevice(
                "atlas page texture missing".into(),
            ));
        };

        if let Some(retained) = self.retained_runs.get_mut(&run_key) {
            self.device
                .borrow_mut()
                .update_glyph_batch(&mut retained.gpu, &self.scratch_vertices)?;
            retained.page_index = target_page_index;
            retained.clip = clip;
            retained.hash = hash;
            retained.bytes = bytes;
            retained.vertex_count = vertex_count;
            retained.resolution_band = resolution_band;

            stats.uploaded_bytes += bytes as u32;
            stats.gui_rebuilds += 1;
            stats.gui_allocations += 1;

            self.device.borrow_mut().draw_glyph_batch(
                program,
                &retained.gpu,
                texture,
                mvp,
                &clip,
            )?;
        } else {
            let gpu = self
                .device
                .borrow_mut()
                .create_glyph_batch(&self.scratch_vertices)?;

            stats.uploaded_bytes += bytes as u32;
            stats.gui_rebuilds += 1;
            stats.gui_allocations += 1;

            self.device
                .borrow_mut()
                .draw_glyph_batch(program, &gpu, texture, mvp, &clip)?;

            self.retained_runs.insert(
                run_key,
                RetainedGlyphBatch {
                    gpu,
                    page_index: target_page_index,
                    clip,
                    hash,
                    bytes,
                    vertex_count,
                    resolution_band,
                },
            );
        }

        stats.draw_calls += 1;
        stats.triangles += (vertex_count / 3) as u32;
        stats.gui_batches += 1;
        Ok(())
    }

    /// Conclude frame submission, reclaiming runs not referenced during this frame.
    pub fn finish_frame(&mut self) {
        let dead_keys: Vec<TextRunKey> = self
            .retained_runs
            .keys()
            .copied()
            .filter(|key| !self.used_runs.contains(key))
            .collect();

        let mut device = self.device.borrow_mut();
        for key in dead_keys {
            if let Some(run) = self.retained_runs.remove(&key) {
                device.delete_glyph_batch(run.gpu);
            }
        }

        self.used_runs.clear();
    }
}

/// Root code bitmask helper for quadratic Bezier root finding.
#[inline]
fn root_code(p1: f32, p2: f32, p3: f32) -> u32 {
    let i1 = (p1.to_bits() >> 31) & 1;
    let i2 = (p2.to_bits() >> 30) & 2;
    let i3 = (p3.to_bits() >> 29) & 4;
    let shift = (i2 & 2) | (i1 & !2);
    let shift = (i3 & 4) | (shift & !4);
    (0x2E74u32 >> shift) & 0x0101
}

/// Horizontal ray crossing solution for quadratic Bezier segment.
fn solve_horizontal(p12: [f32; 4], p3: [f32; 2], a: [f32; 2], b: [f32; 2], line: bool) -> [f32; 2] {
    if line {
        let denom = p12[1] - p3[1];
        let t = if denom.abs() > 1e-7 {
            p12[1] / denom
        } else {
            0.0
        };
        let crossing = p12[0] + (p3[0] - p12[0]) * t;
        return [crossing, crossing];
    }

    let d_sq = b[1] * b[1] - a[1] * p12[1];
    let d = d_sq.max(0.0).sqrt();
    let t = if a[1].abs() > 1.0 / 65536.0 {
        [(b[1] - d) / a[1], (b[1] + d) / a[1]]
    } else if b[1].abs() > 1e-7 {
        let v = p12[1] / (2.0 * b[1]);
        [v, v]
    } else {
        [0.0, 0.0]
    };

    [
        (a[0] * t[0] - 2.0 * b[0]) * t[0] + p12[0],
        (a[0] * t[1] - 2.0 * b[0]) * t[1] + p12[0],
    ]
}

/// Vertical ray crossing solution for quadratic Bezier segment.
fn solve_vertical(p12: [f32; 4], p3: [f32; 2], a: [f32; 2], b: [f32; 2], line: bool) -> [f32; 2] {
    let q = [p12[1], p12[0], p12[3], p12[2]];
    solve_horizontal(q, [p3[1], p3[0]], [a[1], a[0]], [b[1], b[0]], line)
}

/// Evaluate exact subpixel quadratic curve coverage at `sample` position.
pub fn evaluate_glyph_coverage(
    curves: &[[f32; 8]],
    sample: [f32; 2],
    pixels_per_unit: [f32; 2],
) -> f32 {
    let mut xcov = 0.0f32;
    let mut ycov = 0.0f32;
    let mut xwgt = 0.0f32;
    let mut ywgt = 0.0f32;

    for curve in curves {
        let p12 = [
            curve[0] - sample[0],
            curve[1] - sample[1],
            curve[2] - sample[0],
            curve[3] - sample[1],
        ];
        let p3 = [curve[4] - sample[0], curve[5] - sample[1]];
        let b = [p12[2] - p12[0], p12[3] - p12[1]];
        let a = [p3[0] - p12[2] - b[0], p3[1] - p12[3] - b[1]];
        let line = curve[6] == 0.0;

        let code_h = root_code(p12[1], p12[3], p3[1]);
        if code_h != 0 {
            let r = [
                solve_horizontal(p12, p3, a, b, line)[0] * pixels_per_unit[0],
                solve_horizontal(p12, p3, a, b, line)[1] * pixels_per_unit[0],
            ];
            if (code_h & 1) != 0 {
                xcov += (r[0] + 0.5).clamp(0.0, 1.0);
                xwgt = xwgt.max((1.0 - r[0].abs() * 2.0).clamp(0.0, 1.0));
            }
            if code_h > 1 {
                xcov -= (r[1] + 0.5).clamp(0.0, 1.0);
                xwgt = xwgt.max((1.0 - r[1].abs() * 2.0).clamp(0.0, 1.0));
            }
        }

        let code_v = root_code(p12[0], p12[2], p3[0]);
        if code_v != 0 {
            let r = [
                solve_vertical(p12, p3, a, b, line)[0] * pixels_per_unit[1],
                solve_vertical(p12, p3, a, b, line)[1] * pixels_per_unit[1],
            ];
            if (code_v & 1) != 0 {
                ycov -= (r[0] + 0.5).clamp(0.0, 1.0);
                ywgt = ywgt.max((1.0 - r[0].abs() * 2.0).clamp(0.0, 1.0));
            }
            if code_v > 1 {
                ycov += (r[1] + 0.5).clamp(0.0, 1.0);
                ywgt = ywgt.max((1.0 - r[1].abs() * 2.0).clamp(0.0, 1.0));
            }
        }
    }

    let denom = (xwgt + ywgt).max(1.0 / 65536.0);
    let cov = ((xcov * xwgt + ycov * ywgt).abs() / denom).max(xcov.abs().min(ycov.abs()));
    cov.clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "glyph_atlas_tests.rs"]
mod tests;
