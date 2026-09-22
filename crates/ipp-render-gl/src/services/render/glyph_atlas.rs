//! Retained text geometry batches and shared glyph coverage atlases.
//!
//! Repeated text quads sample coverage images from shared atlas pages.
//! Entries are keyed by font identity, glyph ID and resolution band.
//! Retained batches reuse local vertex geometry across camera motion within
//! a stable resolution band and rebuild only when affected text edits occur.
//! A glyph whose population fails backs off before retrying; its text keeps the
//! analytic path meanwhile.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use ipp_core::services::asset_management::AssetKey;
use ipp_core::systems::surface::{
    SurfaceClipRect, SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
};

use super::gui_batch::RetainedSurfaceSubmission;
use crate::{RenderDevice, RenderError, RenderStats};

/// Page width and height in texels for each atlas page texture.
pub const ATLAS_PAGE_SIZE: u32 = 512;

/// Maximum number of resident atlas pages before allocation rejection.
pub const MAX_ATLAS_PAGES: usize = 4;

/// Maximum number of new glyph coverage entries populated per frame.
pub const MAX_POPULATES_PER_FRAME: usize = 32;

/// Demand publications a glyph waits after its first failed population.
pub const POPULATE_RETRY_TICKS: u64 = 4;

/// Each further failure doubles the wait, at most this many times.
pub const MAX_POPULATE_RETRY_DOUBLINGS: u32 = 8;

/// Supported discrete resolution bands (nominal pixel heights per em).
pub const RESOLUTION_BANDS: [u16; 5] = [16, 24, 32, 48, 64];

/// Select a stable presentation resolution band for the requested pixel height.
///
/// Returns `None` for extreme scales (< 10.0 or > 80.0 px) which fall back to
/// analytic curve rendering.
pub fn select_resolution_band(pixel_height: f32) -> Option<u16> {
    if !pixel_height.is_finite() || !(10.0..=80.0).contains(&pixel_height) {
        return None;
    }

    for &band in &RESOLUTION_BANDS {
        if band as f32 >= pixel_height * 0.88 {
            return Some(band);
        }
    }

    Some(RESOLUTION_BANDS[RESOLUTION_BANDS.len() - 1])
}

/// Project an em-height segment at the text origin through the final transform.
/// Homogeneous division is required for perspective and oblique panels.
pub fn projected_glyph_height(
    mvp: &[f32; 16],
    position: [f32; 2],
    height: f32,
    viewport: (u32, u32),
) -> f32 {
    let project = |y: f32| {
        let w = mvp[3] * position[0] + mvp[7] * y + mvp[15];
        if !w.is_finite() || w <= 0.0 {
            return None;
        }
        Some([
            (mvp[0] * position[0] + mvp[4] * y + mvp[12]) / w * viewport.0 as f32 * 0.5,
            (mvp[1] * position[0] + mvp[5] * y + mvp[13]) / w * viewport.1 as f32 * 0.5,
        ])
    };
    let (Some(a), Some(b)) = (project(position[1]), project(position[1] + height)) else {
        return f32::NAN;
    };
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Test glyph paint against the effective clip before preparing any geometry.
pub fn glyph_intersects_clip(
    style: &SurfacePrimitiveStyle,
    glyph: &SurfaceGlyph,
    bounds: [f32; 4],
    unit: f32,
    clip: SurfaceClipRect,
) -> bool {
    let x = style.position[0] + glyph.position[0] * style.scale[0];
    let y = style.position[1] + glyph.position[1] * style.scale[1];
    let [x0, x1] = [
        x + bounds[0] * unit * style.scale[0],
        x + bounds[2] * unit * style.scale[0],
    ];
    let [y0, y1] = [
        y + bounds[1] * unit * style.scale[1],
        y + bounds[3] * unit * style.scale[1],
    ];
    x0.max(x1) > clip[0] && x0.min(x1) < clip[2] && y0.max(y1) > clip[1] && y0.min(y1) < clip[3]
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

// GLES attribute strides and the WebGL bridge read exactly this many bytes per vertex.
const _: () = assert!(std::mem::size_of::<GlyphVertex>() == 32);

/// Glyph demand one live Surface publishes before a frame.
pub enum GlyphSurfaceDemand {
    /// The Surface will be submitted and needs exactly these entries.
    Submitted(BTreeSet<GlyphKey>),
    /// The Surface is culled and keeps the demand of its last submission.
    Culled,
}

/// Delay before a glyph whose population failed may be attempted again.
#[derive(Clone, Copy, Debug, Default)]
struct GlyphPopulationBackoff {
    retry_tick: u64,
    failures: u32,
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
    pages: BTreeMap<usize, AtlasPage<D>>,
    next_page: usize,
    epoch: u64,
    live_keys: BTreeMap<ipp_core::WorldId, BTreeMap<ipp_core::EntityId, BTreeSet<GlyphKey>>>,
    needs_reclaim: bool,
    entries: BTreeMap<GlyphKey, GlyphAtlasEntry>,
    demand_tick: u64,
    population_backoff: BTreeMap<GlyphKey, GlyphPopulationBackoff>,
}

impl<D: RenderDevice> GlyphAtlas<D> {
    /// Create an empty glyph atlas bound to the given render device.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            pages: BTreeMap::new(),
            next_page: 0,
            epoch: 0,
            live_keys: BTreeMap::new(),
            needs_reclaim: false,
            entries: BTreeMap::new(),
            demand_tick: 0,
            population_backoff: BTreeMap::new(),
        }
    }

    /// Release all atlas pages and invalidate all cached glyph entries.
    pub fn clear(&mut self) {
        let mut device = self.device.borrow_mut();
        for (_, page) in std::mem::take(&mut self.pages) {
            device.delete_glyph_atlas_page(page.handle);
        }
        self.entries.clear();
        self.live_keys.clear();
        self.population_backoff.clear();
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// Publish one World's glyph demand before drawing and advance the back-off clock.
    ///
    /// A Surface submitted this frame replaces its demand, a culled Surface keeps the
    /// demand of its last submission and Surfaces absent from `surfaces` release theirs.
    /// Whole pages with no consumers are retired; pressure may reclaim a partially
    /// stale page. Its epoch invalidates retained UVs before any new draws.
    pub fn prepare_world(
        &mut self,
        world: ipp_core::WorldId,
        surfaces: BTreeMap<ipp_core::EntityId, GlyphSurfaceDemand>,
    ) {
        let mut previous = self.live_keys.remove(&world).unwrap_or_default();
        let demand = surfaces
            .into_iter()
            .map(|(entity, surface)| {
                let keys = match surface {
                    GlyphSurfaceDemand::Submitted(keys) => keys,
                    GlyphSurfaceDemand::Culled => previous.remove(&entity).unwrap_or_default(),
                };
                (entity, keys)
            })
            .collect();
        self.live_keys.insert(world, demand);

        self.demand_tick += 1;
        self.reclaim();
    }

    /// Retire demand when a World leaves presentation.
    pub fn forget_world(&mut self, world: ipp_core::WorldId) {
        self.live_keys.remove(&world);
        self.reclaim();
    }

    fn reclaim(&mut self) {
        let live: BTreeSet<_> = self
            .live_keys
            .values()
            .flat_map(BTreeMap::values)
            .flatten()
            .copied()
            .collect();

        let mut counts = BTreeMap::<usize, (usize, usize)>::new();
        for (key, entry) in &self.entries {
            let count = counts.entry(entry.page_index).or_default();
            count.0 += 1;
            count.1 += usize::from(live.contains(key));
        }

        let partial = self
            .needs_reclaim
            .then(|| {
                counts
                    .iter()
                    .filter(|(_, (total, used))| used < total)
                    .max_by_key(|(id, (total, used))| (total - used, std::cmp::Reverse(**id)))
            })
            .flatten()
            .map(|(&id, _)| id);
        let dead: BTreeSet<_> = self
            .pages
            .keys()
            .copied()
            .filter(|id| counts.get(id).is_none_or(|(_, used)| *used == 0) || Some(*id) == partial)
            .collect();

        if !dead.is_empty() {
            self.epoch = self.epoch.wrapping_add(1);
            self.entries
                .retain(|_, entry| !dead.contains(&entry.page_index));

            let mut device = self.device.borrow_mut();
            for id in dead {
                if let Some(page) = self.pages.remove(&id) {
                    device.delete_glyph_atlas_page(page.handle);
                }
            }
        }

        // Back-off matters only while a glyph stays demanded and unpopulated.
        self.population_backoff
            .retain(|key, _| live.contains(key) && !self.entries.contains_key(key));
        self.needs_reclaim = false;
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

    /// Discard an entry whose coverage was never written and delay its next population.
    ///
    /// Only the population that allocated an entry can fail it, before any retained run
    /// samples its UVs, so the epoch and every other entry stay valid.
    pub fn abandon_population(&mut self, key: GlyphKey) {
        self.entries.remove(&key);

        let backoff = self.population_backoff.entry(key).or_default();
        backoff.failures = backoff.failures.saturating_add(1);
        let doublings = (backoff.failures - 1).min(MAX_POPULATE_RETRY_DOUBLINGS);
        backoff.retry_tick = self.demand_tick + (POPULATE_RETRY_TICKS << doublings);
    }

    /// Whether a glyph whose population failed is still waiting to be retried.
    pub fn population_deferred(&self, key: &GlyphKey) -> bool {
        self.population_backoff
            .get(key)
            .is_some_and(|backoff| backoff.retry_tick > self.demand_tick)
    }

    /// Borrow the underlying color texture of an atlas page for sampling.
    pub fn page_texture(&self, page_index: usize) -> Option<&D::Texture> {
        let page = self.pages.get(&page_index)?;
        Some(D::glyph_atlas_texture(&page.handle))
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
        if px_width > ATLAS_PAGE_SIZE - 2 || px_height > ATLAS_PAGE_SIZE - 2 {
            return Err(RenderError::RenderDevice(
                "glyph exceeds atlas page size".into(),
            ));
        }
        if let Some(entry) = self.entries.get(&key) {
            let x = (entry.uv[0] * ATLAS_PAGE_SIZE as f32).round() as u32;
            let y = ((1.0 - entry.uv[1]) * ATLAS_PAGE_SIZE as f32).round() as u32;
            return Ok(([x, y], entry.page_index, *entry));
        }
        let slot_w = px_width + 2;
        let slot_h = px_height + 2;

        let mut target_page = None;
        let mut slot_pos = None;

        for (&idx, page) in &mut self.pages {
            if let Some(pos) = page.allocate(slot_w, slot_h) {
                target_page = Some(idx);
                slot_pos = Some(pos);
                break;
            }
        }

        if target_page.is_none() {
            if self.pages.len() >= MAX_ATLAS_PAGES {
                self.needs_reclaim = true;
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
            let idx = self.next_page;
            self.next_page += 1;
            self.pages.insert(idx, page);
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
        self.pages.get(&page_index).map(|p| &p.handle)
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

struct RetainedGlyphRun<D: RenderDevice> {
    hash: u64,
    batches: Vec<RetainedGlyphBatch<D>>,
}

/// Renderer-owned retained cache of text run GPU batches.
pub struct GlyphBatchRenderCache<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    retained_runs: BTreeMap<TextRunKey, RetainedGlyphRun<D>>,
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
            for batch in run.batches {
                device.delete_glyph_batch(batch.gpu);
            }
        }
        self.scratch_vertices.clear();
        self.used_runs.clear();
    }

    /// Total resident bytes occupied by retained text run GPU batches.
    pub fn resident_bytes(&self) -> usize {
        self.retained_runs
            .values()
            .flat_map(|r| &r.batches)
            .map(|b| b.bytes)
            .sum()
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
        hasher.write_u64(atlas.epoch);
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
            hasher.write_u8(u8::from(glyph.color.is_some()));
            if let Some(col) = glyph.color {
                for c in col {
                    hasher.write_u32(c.to_bits());
                }
            }
        }
        let hash = hasher.finish();

        if let Some(run) = self
            .retained_runs
            .get(&run_key)
            .filter(|run| run.hash == hash)
        {
            return self.draw_batches(program, atlas, &run.batches, mvp, &clip, stats);
        }

        self.scratch_vertices.clear();
        let mut target_page_index = None;
        let mut geometry = Vec::new();
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

            if x0.max(x1) <= clip[0]
                || x0.min(x1) >= clip[2]
                || y0.max(y1) <= clip[1]
                || y0.min(y1) >= clip[3]
            {
                continue;
            }
            if !self.scratch_vertices.is_empty()
                && (target_page_index != Some(entry.page_index)
                    || self.scratch_vertices.len() >= 256 * 6)
            {
                geometry.push((
                    target_page_index.unwrap(),
                    std::mem::take(&mut self.scratch_vertices),
                ));
            }
            target_page_index = Some(entry.page_index);

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

        if !self.scratch_vertices.is_empty() {
            geometry.push((
                target_page_index.unwrap(),
                std::mem::take(&mut self.scratch_vertices),
            ));
        }

        let mut previous = self
            .retained_runs
            .remove(&run_key)
            .map(|run| run.batches)
            .unwrap_or_default()
            .into_iter();
        let mut batches = Vec::new();
        let mut failure = None;

        for (page_index, vertices) in geometry {
            let bytes = std::mem::size_of_val(vertices.as_slice());
            let mut batch = previous.next();
            let result = if let Some(ref mut retained) = batch {
                self.device
                    .borrow_mut()
                    .update_glyph_batch(&mut retained.gpu, &vertices)
            } else {
                self.device
                    .borrow_mut()
                    .create_glyph_batch(&vertices)
                    .map(|gpu| {
                        batch = Some(RetainedGlyphBatch {
                            gpu,
                            page_index,
                            clip,
                            hash,
                            bytes,
                            vertex_count: vertices.len(),
                            resolution_band,
                        });
                    })
            };

            // A failed replacement leaves its storage unknown: release that batch too.
            if let Err(error) = result {
                if let Some(batch) = batch {
                    self.device.borrow_mut().delete_glyph_batch(batch.gpu);
                }
                failure = Some(error);
                break;
            }

            let mut batch = batch.unwrap();
            batch.page_index = page_index;
            batch.bytes = bytes;
            batch.vertex_count = vertices.len();
            batch.clip = clip;
            batch.hash = hash;
            batch.resolution_band = resolution_band;
            batches.push(batch);

            stats.uploaded_bytes = stats.uploaded_bytes.saturating_add(bytes as u32);
            stats.gui_rebuilds += 1;
            stats.gui_allocations += 1;

            self.scratch_vertices = vertices;
            self.scratch_vertices.clear();
        }

        for batch in previous {
            self.device.borrow_mut().delete_glyph_batch(batch.gpu);
        }

        if let Some(error) = failure {
            for batch in batches {
                self.device.borrow_mut().delete_glyph_batch(batch.gpu);
            }
            return Err(error);
        }

        // Retain ownership even if a draw fails, so frame cleanup can release it.
        self.retained_runs.insert(
            run_key,
            RetainedGlyphRun {
                hash,
                batches,
            },
        );
        self.draw_batches(
            program,
            atlas,
            &self.retained_runs[&run_key].batches,
            mvp,
            &clip,
            stats,
        )
    }

    fn draw_batches(
        &self,
        program: &D::Program,
        atlas: &GlyphAtlas<D>,
        batches: &[RetainedGlyphBatch<D>],
        mvp: &[f32; 16],
        clip: &SurfaceClipRect,
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        for batch in batches {
            let texture = atlas
                .page_texture(batch.page_index)
                .ok_or_else(|| RenderError::RenderDevice("atlas page texture missing".into()))?;
            self.device
                .borrow_mut()
                .draw_glyph_batch(program, &batch.gpu, texture, mvp, clip)?;
            stats.draw_calls += 1;
            stats.triangles += (batch.vertex_count / 3) as u32;
            stats.gui_batches += 1;
        }
        Ok(())
    }

    /// Conclude frame submission, releasing runs a completed frame shows to be stale.
    ///
    /// `surfaces` is `None` when submission did not complete; every run is then kept.
    pub fn finish_frame(&mut self, surfaces: Option<&RetainedSurfaceSubmission<'_>>) {
        if let Some(surfaces) = surfaces {
            let stale: Vec<TextRunKey> = self
                .retained_runs
                .keys()
                .copied()
                .filter(|key| surfaces.is_stale(key.entity, self.used_runs.contains(key)))
                .collect();

            let mut device = self.device.borrow_mut();
            for key in stale {
                if let Some(run) = self.retained_runs.remove(&key) {
                    for batch in run.batches {
                        device.delete_glyph_batch(batch.gpu);
                    }
                }
            }
        }

        self.used_runs.clear();
    }
}

impl<D: RenderDevice> Drop for GlyphAtlas<D> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<D: RenderDevice> Drop for GlyphBatchRenderCache<D> {
    fn drop(&mut self) {
        self.clear();
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
