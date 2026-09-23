//! Retained text geometry batches and shared glyph coverage atlases.
//!
//! Repeated text quads sample coverage images from shared atlas pages. Entries are keyed
//! by font identity, glyph ID and resolution band. Each text run keeps its band with
//! hysteresis, so camera motion near a band threshold changes neither its entries nor its
//! retained geometry. Runs publish reference-counted demand before their World draws, and
//! the Service populates the missing entries of a frame before its main pass.
//!
//! Pages without demand stay resident. They retire after a configurable number of idle
//! demand publications, when allocation pressure needs their space, or all at once when
//! no World demands any glyph. Pressure reclaims a partially live page only when no idle
//! page remains. Retained batches check the pages they sample, so retiring one page
//! rebuilds only the runs that reference it. A glyph whose population fails backs off
//! before retrying; its text keeps the analytic path meanwhile.
//!
//! Context loss releases page textures and retained batches but keeps the atlas layout,
//! demand and run bands. Recovery repopulates demanded entries into their original
//! slots, so recovered text samples the same coverage as before the loss.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use ipp_core::services::asset_management::AssetKey;
use ipp_core::systems::surface::{
    SurfaceClipRect, SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
};

use super::gui_batch::{GUI_FILL_GLYPH, GuiVertex};
use super::gui_storage::{GuiPiece, GuiPieceKey, GuiPieceSource};
use super::retained_surfaces::SurfacePaint;
use crate::{RenderDevice, RenderError, RenderStats};

/// Page width and height in texels for each atlas page texture.
pub const ATLAS_PAGE_SIZE: u32 = 512;

/// Bytes per atlas texel: pages store single-channel R8 coverage.
pub const ATLAS_BYTES_PER_TEXEL: usize = 1;

/// Glyph coverage entries a frame populates whenever that many are missing, however
/// long they take.
pub const MIN_POPULATES_PER_FRAME: usize = 32;

/// Glyph coverage entries one frame populates at most; later misses wait a frame.
pub const MAX_POPULATES_PER_FRAME: usize = 512;

/// Default time one frame may spend populating glyphs beyond
/// [`MIN_POPULATES_PER_FRAME`], in milliseconds.
pub const DEFAULT_POPULATE_BUDGET_MS: f64 = 2.0;

/// Estimated population cost per glyph, in milliseconds, before a measurement and on
/// platforms without a clock. Native population measures its own passes; WebGL draws
/// are queued to another process, so its estimate stays fixed. Both are calibrated
/// from cold terminal and dashboard populations (see `RenderStats::glyph_populates`).
#[cfg(not(target_arch = "wasm32"))]
const ESTIMATED_POPULATE_MS_PER_GLYPH: f64 = 0.005;
#[cfg(target_arch = "wasm32")]
const ESTIMATED_POPULATE_MS_PER_GLYPH: f64 = 0.02;

/// Demand publications a glyph waits after its first failed population.
pub const POPULATE_RETRY_TICKS: u64 = 4;

/// Each further failure doubles the wait, at most this many times.
pub const MAX_POPULATE_RETRY_DOUBLINGS: u32 = 8;

/// Supported discrete resolution bands (nominal pixel heights per em).
pub const RESOLUTION_BANDS: [u16; 5] = [16, 24, 32, 48, 64];

/// Relative distance a projected em height must move beyond the range of a run's band
/// before the run selects another band or leaves the atlas.
pub const BAND_HYSTERESIS: f32 = 0.1;

/// Projected em heights, in pixels, that atlas bands serve; others use analytic curves.
const ATLAS_PIXEL_HEIGHTS: [f32; 2] = [10.0, 80.0];

/// A band serves projected heights up to its nominal height divided by this factor.
const BAND_COVERAGE: f32 = 0.88;

/// Glyph quads one retained batch holds at most.
const MAX_BATCH_GLYPHS: usize = 256;

/// Bounds a Host may place on the shared glyph atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphAtlasLimits {
    /// Resident page budget, at least one. Allocation beyond it reclaims idle pages first.
    pub max_pages: usize,
    /// Demand publications a page without demand stays resident before it retires.
    /// Every World render that publishes glyph demand counts once.
    pub idle_page_publications: u64,
}

impl GlyphAtlasLimits {
    /// Four pages; an idle page retires after about ten seconds of one World at 60 Hz.
    pub const DEFAULT: Self = Self {
        max_pages: 4,
        idle_page_publications: 600,
    };
}

impl Default for GlyphAtlasLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Select the resolution band for a projected em height, keeping `current` with hysteresis.
///
/// A run keeps its band while the height stays within [`BAND_HYSTERESIS`] of that band's
/// range. Otherwise the nearest band serving the height is selected, or `None` for
/// extreme scales (below 10 px or above 80 px), which fall back to analytic curves.
pub fn select_resolution_band(pixel_height: f32, current: Option<u16>) -> Option<u16> {
    if !pixel_height.is_finite() {
        return None;
    }

    if let Some([low, high]) = current.and_then(band_heights)
        && pixel_height >= low * (1.0 - BAND_HYSTERESIS)
        && pixel_height <= high * (1.0 + BAND_HYSTERESIS)
    {
        return current;
    }

    if !(ATLAS_PIXEL_HEIGHTS[0]..=ATLAS_PIXEL_HEIGHTS[1]).contains(&pixel_height) {
        return None;
    }

    RESOLUTION_BANDS
        .iter()
        .copied()
        .find(|&band| f32::from(band) >= pixel_height * BAND_COVERAGE)
        .or(RESOLUTION_BANDS.last().copied())
}

/// Projected heights `[low, high]` for which [`select_resolution_band`] selects `band`
/// without history.
fn band_heights(band: u16) -> Option<[f32; 2]> {
    let index = RESOLUTION_BANDS.iter().position(|&value| value == band)?;
    let low = index
        .checked_sub(1)
        .map_or(ATLAS_PIXEL_HEIGHTS[0], |previous| {
            f32::from(RESOLUTION_BANDS[previous]) / BAND_COVERAGE
        });
    let high = if index + 1 == RESOLUTION_BANDS.len() {
        ATLAS_PIXEL_HEIGHTS[1]
    } else {
        f32::from(band) / BAND_COVERAGE
    };
    Some([low, high])
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

/// One atlas page: its texture, shelf-packing state and demand accounting.
struct AtlasPage<D: RenderDevice> {
    /// Context-owned texture and framebuffer; `None` after context loss until an
    /// entry on the page is repopulated.
    handle: Option<D::GlyphAtlasPage>,
    current_x: u32,
    current_y: u32,
    row_height: u32,
    /// Entries allocated on this page.
    entries: usize,
    /// Entries on this page that at least one text run demands.
    demanded: usize,
    /// Demand publication at which the page last had no demanded entry.
    idle_since: u64,
}

impl<D: RenderDevice> AtlasPage<D> {
    fn new(handle: D::GlyphAtlasPage, tick: u64) -> Self {
        Self {
            handle: Some(handle),
            current_x: 0,
            current_y: 0,
            row_height: 0,
            entries: 0,
            demanded: 0,
            idle_since: tick,
        }
    }

    /// Allocate a padded slot of `slot_width x slot_height` on this page.
    fn allocate(&mut self, slot_width: u32, slot_height: u32) -> Option<[u32; 2]> {
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
        Some(pos)
    }

    /// Entries no run demands; pressure reclaims the page holding the most.
    fn stale_entries(&self) -> usize {
        self.entries - self.demanded
    }
}

/// One allocated atlas slot.
struct AtlasSlot {
    entry: GlyphAtlasEntry,
    /// Whether the page texture holds this entry's coverage. Context loss keeps the
    /// slot and clears this until recovery repopulates it.
    populated: bool,
}

/// Renderer-owned shared glyph coverage atlas.
pub struct GlyphAtlas<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    limits: GlyphAtlasLimits,
    pages: BTreeMap<usize, AtlasPage<D>>,
    next_page: usize,
    entries: BTreeMap<GlyphKey, AtlasSlot>,
    /// Text runs demanding each entry, across every World.
    demand: BTreeMap<GlyphKey, u32>,
    demand_tick: u64,
    /// Advances whenever resident entries are removed, invalidating residency checks.
    residency_generation: u64,
    needs_reclaim: bool,
    retired_pages: u32,
    population_backoff: BTreeMap<GlyphKey, GlyphPopulationBackoff>,
}

impl<D: RenderDevice> GlyphAtlas<D> {
    /// Create an empty glyph atlas bound to the given render device.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            limits: GlyphAtlasLimits::DEFAULT,
            pages: BTreeMap::new(),
            next_page: 0,
            entries: BTreeMap::new(),
            demand: BTreeMap::new(),
            demand_tick: 0,
            residency_generation: 0,
            needs_reclaim: false,
            retired_pages: 0,
            population_backoff: BTreeMap::new(),
        }
    }

    /// Replace the atlas bounds. A lowered page budget applies at the next publication.
    pub fn set_limits(&mut self, limits: GlyphAtlasLimits) {
        self.limits = GlyphAtlasLimits {
            max_pages: limits.max_pages.max(1),
            ..limits
        };
    }

    /// Release all atlas pages, entries and demand.
    pub fn clear(&mut self) {
        let mut device = self.device.borrow_mut();
        for (_, page) in std::mem::take(&mut self.pages) {
            if let Some(handle) = page.handle {
                device.delete_glyph_atlas_page(handle);
            }
        }
        self.entries.clear();
        self.demand.clear();
        self.population_backoff.clear();
        self.needs_reclaim = false;
        self.residency_generation = self.residency_generation.wrapping_add(1);
    }

    /// Release page textures before the graphics context goes away.
    ///
    /// Slots, demand and back-off survive, so recovery repopulates each demanded entry
    /// into its original slot. Until then no entry is resident.
    pub fn release_context(&mut self) {
        let mut device = self.device.borrow_mut();
        for page in self.pages.values_mut() {
            if let Some(handle) = page.handle.take() {
                device.delete_glyph_atlas_page(handle);
            }
        }
        for slot in self.entries.values_mut() {
            slot.populated = false;
        }
        self.residency_generation = self.residency_generation.wrapping_add(1);
    }

    /// Add one run's reference to each of `keys`, which holds no key twice.
    pub fn acquire(&mut self, keys: &[GlyphKey]) {
        for key in keys {
            let count = self.demand.entry(*key).or_default();
            *count += 1;
            if *count > 1 {
                continue;
            }

            if let Some(slot) = self.entries.get(key)
                && let Some(page) = self.pages.get_mut(&slot.entry.page_index)
            {
                page.demanded += 1;
            }
        }
    }

    /// Remove one run's reference to each of `keys`, which holds no key twice.
    pub fn release(&mut self, keys: &[GlyphKey]) {
        for key in keys {
            let Some(count) = self.demand.get_mut(key) else {
                continue;
            };
            *count -= 1;
            if *count > 0 {
                continue;
            }

            self.demand.remove(key);
            self.population_backoff.remove(key);
            if let Some(slot) = self.entries.get(key)
                && let Some(page) = self.pages.get_mut(&slot.entry.page_index)
            {
                page.demanded -= 1;
                if page.demanded == 0 {
                    page.idle_since = self.demand_tick;
                }
            }
        }
    }

    /// Start one World's demand publication before its runs check residency.
    ///
    /// Advances the back-off and idle clocks, retires expired idle pages, answers
    /// pressure recorded by a failed allocation and applies a lowered page budget.
    pub fn begin_publication(&mut self) {
        self.demand_tick += 1;

        let expired: Vec<usize> = self
            .pages
            .iter()
            .filter(|(_, page)| {
                page.demanded == 0
                    && self.demand_tick - page.idle_since >= self.limits.idle_page_publications
            })
            .map(|(&id, _)| id)
            .collect();
        for id in expired {
            self.retire_page(id);
        }

        if std::mem::take(&mut self.needs_reclaim)
            && let Some(id) = self.idle_page().or_else(|| self.most_stale_page())
        {
            self.retire_page(id);
        }

        while self.pages.len() > self.limits.max_pages {
            let fewest_demanded = self
                .pages
                .iter()
                .min_by_key(|&(&id, page)| (page.demanded, std::cmp::Reverse(id)))
                .map(|(&id, _)| id);
            let Some(id) = self.idle_page().or(fewest_demanded) else {
                break;
            };
            self.retire_page(id);
        }

        // Back-off matters only while a glyph stays demanded and unpopulated.
        self.population_backoff.retain(|key, _| {
            self.demand.contains_key(key)
                && !self.entries.get(key).is_some_and(|slot| slot.populated)
        });
    }

    /// Release every page once no World demands any glyph.
    pub fn release_if_unused(&mut self) {
        if !self.demand.is_empty() {
            return;
        }

        let pages: Vec<usize> = self.pages.keys().copied().collect();
        for id in pages {
            self.retire_page(id);
        }
    }

    /// The page without demand that has been idle longest.
    fn idle_page(&self) -> Option<usize> {
        self.pages
            .iter()
            .filter(|(_, page)| page.demanded == 0)
            .min_by_key(|&(&id, page)| (page.idle_since, id))
            .map(|(&id, _)| id)
    }

    /// The partially live page holding the most entries no run demands.
    fn most_stale_page(&self) -> Option<usize> {
        self.pages
            .iter()
            .filter(|(_, page)| page.stale_entries() > 0)
            .max_by_key(|&(&id, page)| (page.stale_entries(), std::cmp::Reverse(id)))
            .map(|(&id, _)| id)
    }

    fn retire_page(&mut self, id: usize) {
        let Some(page) = self.pages.remove(&id) else {
            return;
        };

        self.entries.retain(|_, slot| slot.entry.page_index != id);
        self.residency_generation = self.residency_generation.wrapping_add(1);
        self.retired_pages += 1;
        if let Some(handle) = page.handle {
            self.device.borrow_mut().delete_glyph_atlas_page(handle);
        }
    }

    /// Number of atlas pages holding a resident texture.
    pub fn page_count(&self) -> u32 {
        self.pages
            .values()
            .filter(|page| page.handle.is_some())
            .count() as u32
    }

    /// Total resident bytes occupied by atlas page textures (R8 coverage).
    pub fn resident_bytes(&self) -> usize {
        self.page_count() as usize
            * (ATLAS_PAGE_SIZE as usize * ATLAS_PAGE_SIZE as usize * ATLAS_BYTES_PER_TEXEL)
    }

    /// Pages retired since the last call, by idle expiry, pressure or lost demand.
    pub fn take_retired_pages(&mut self) -> u32 {
        std::mem::take(&mut self.retired_pages)
    }

    /// Advances whenever resident entries are removed; equal values mean every entry
    /// resident at the earlier check is still resident.
    pub fn residency_generation(&self) -> u64 {
        self.residency_generation
    }

    /// Look up a resident glyph entry.
    pub fn get(&self, key: &GlyphKey) -> Option<&GlyphAtlasEntry> {
        self.entries
            .get(key)
            .filter(|slot| slot.populated)
            .map(|slot| &slot.entry)
    }

    /// Whether a page still holds its texture.
    pub fn has_page(&self, page_index: usize) -> bool {
        self.pages
            .get(&page_index)
            .is_some_and(|page| page.handle.is_some())
    }

    /// Discard an entry whose coverage was never written and delay its next population.
    ///
    /// Only the population that allocated an entry can fail it, before any retained run
    /// samples its UVs, so every other entry and retained batch stays valid.
    pub fn abandon_population(&mut self, key: GlyphKey) {
        self.discard_population(key);

        let backoff = self.population_backoff.entry(key).or_default();
        backoff.failures = backoff.failures.saturating_add(1);
        let doublings = (backoff.failures - 1).min(MAX_POPULATE_RETRY_DOUBLINGS);
        backoff.retry_tick = self.demand_tick + (POPULATE_RETRY_TICKS << doublings);
    }

    /// Discard an entry whose coverage was never written, without delaying a retry.
    pub fn discard_population(&mut self, key: GlyphKey) {
        let Some(slot) = self.entries.remove(&key) else {
            return;
        };

        if let Some(page) = self.pages.get_mut(&slot.entry.page_index) {
            page.entries -= 1;
            if self.demand.contains_key(&key) {
                page.demanded -= 1;
                if page.demanded == 0 {
                    page.idle_since = self.demand_tick;
                }
            }
        }
    }

    /// Whether a glyph whose population failed is still waiting to be retried.
    pub fn population_deferred(&self, key: &GlyphKey) -> bool {
        self.population_backoff
            .get(key)
            .is_some_and(|backoff| backoff.retry_tick > self.demand_tick)
    }

    /// Borrow the underlying color texture of an atlas page for sampling.
    pub fn page_texture(&self, page_index: usize) -> Option<&D::Texture> {
        let handle = self.pages.get(&page_index)?.handle.as_ref()?;
        Some(D::glyph_atlas_texture(handle))
    }

    /// Allocate slot coordinates and UV mapping for a new glyph entry.
    ///
    /// The slot is allocated with a 1-pixel border on each side to prevent bilinear
    /// sampling bleed. When no page has room and the page budget is exhausted, the
    /// longest idle page retires; with no idle page the allocation fails and the next
    /// publication reclaims the most stale page. An entry kept through context loss
    /// returns its original slot, recreating its page texture when needed.
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
        if let Some(slot) = self.entries.get(&key) {
            let entry = slot.entry;
            if !slot.populated {
                self.restore_page_texture(entry.page_index)?;
            }
            if let Some(slot) = self.entries.get_mut(&key) {
                slot.populated = true;
            }

            let x = (entry.uv[0] * ATLAS_PAGE_SIZE as f32).round() as u32;
            let y = ((1.0 - entry.uv[1]) * ATLAS_PAGE_SIZE as f32).round() as u32;
            return Ok(([x, y], entry.page_index, entry));
        }
        let slot_w = px_width + 2;
        let slot_h = px_height + 2;

        let existing = self
            .pages
            .iter_mut()
            .find_map(|(&index, page)| Some((index, page.allocate(slot_w, slot_h)?)));
        let (page_index, [slot_x, slot_y]) = match existing {
            Some((index, slot)) => {
                // A page kept through context loss regains its texture on first use.
                self.restore_page_texture(index)?;
                (index, slot)
            }
            None => {
                if self.pages.len() >= self.limits.max_pages {
                    let Some(idle) = self.idle_page() else {
                        self.needs_reclaim = true;
                        return Err(RenderError::RenderDevice(
                            "glyph atlas page capacity exceeded".into(),
                        ));
                    };
                    self.retire_page(idle);
                }

                let handle = self
                    .device
                    .borrow_mut()
                    .create_glyph_atlas_page(ATLAS_PAGE_SIZE, ATLAS_PAGE_SIZE)?;
                let index = self.next_page;
                self.next_page += 1;
                let page = self
                    .pages
                    .entry(index)
                    .or_insert(AtlasPage::new(handle, self.demand_tick));
                let slot = page.allocate(slot_w, slot_h).ok_or_else(|| {
                    RenderError::RenderDevice("glyph exceeds atlas page size".into())
                })?;
                (index, slot)
            }
        };

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

        if let Some(page) = self.pages.get_mut(&page_index) {
            page.entries += 1;
            if self.demand.contains_key(&key) {
                page.demanded += 1;
            }
        }
        self.entries.insert(
            key,
            AtlasSlot {
                entry,
                populated: true,
            },
        );
        Ok(([content_x, content_y], page_index, entry))
    }

    /// Recreate a page texture released by context loss.
    fn restore_page_texture(&mut self, page_index: usize) -> Result<(), RenderError> {
        let Some(page) = self.pages.get_mut(&page_index) else {
            return Err(RenderError::RenderDevice("glyph atlas page missing".into()));
        };
        if page.handle.is_none() {
            page.handle = Some(
                self.device
                    .borrow_mut()
                    .create_glyph_atlas_page(ATLAS_PAGE_SIZE, ATLAS_PAGE_SIZE)?,
            );
        }
        Ok(())
    }

    /// Access the raw page handle for drawing.
    pub fn page_handle(&self, page_index: usize) -> Option<&D::GlyphAtlasPage> {
        self.pages.get(&page_index)?.handle.as_ref()
    }
}

/// Per-frame glyph population allowance from a time budget and a per-glyph cost.
///
/// Each frame populates at least [`MIN_POPULATES_PER_FRAME`] missing entries and at
/// most [`MAX_POPULATES_PER_FRAME`], and between those as many as the budget covers at
/// the estimated cost. Where the platform has a clock, measured population passes
/// refine the estimate, so cold text reaches the atlas in one frame when the budget
/// allows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphPopulationBudget {
    budget_ms: f64,
    ms_per_glyph: f64,
}

impl Default for GlyphPopulationBudget {
    fn default() -> Self {
        Self {
            budget_ms: DEFAULT_POPULATE_BUDGET_MS,
            ms_per_glyph: ESTIMATED_POPULATE_MS_PER_GLYPH,
        }
    }
}

impl GlyphPopulationBudget {
    /// Samples smaller than this many glyphs are dominated by fixed pass costs.
    const MIN_SAMPLE_GLYPHS: usize = 8;

    /// Replace the time budget; zero populates only the floor, and an infinite or
    /// non-finite budget populates up to the cap.
    pub fn set_budget_ms(&mut self, budget_ms: f64) {
        self.budget_ms = if budget_ms.is_nan() {
            f64::INFINITY
        } else {
            budget_ms.max(0.0)
        };
    }

    /// Entries this frame may populate.
    pub fn allowance(&self) -> usize {
        let covered = self.budget_ms / self.ms_per_glyph;
        if covered >= MAX_POPULATES_PER_FRAME as f64 {
            return MAX_POPULATES_PER_FRAME;
        }

        (covered as usize).clamp(MIN_POPULATES_PER_FRAME, MAX_POPULATES_PER_FRAME)
    }

    /// Fold one measured population pass into the per-glyph estimate.
    pub fn record(&mut self, glyphs: usize, elapsed_ms: f64) {
        if glyphs < Self::MIN_SAMPLE_GLYPHS || !elapsed_ms.is_finite() || elapsed_ms < 0.0 {
            return;
        }

        let sample = (elapsed_ms / glyphs as f64).max(f64::MIN_POSITIVE);
        self.ms_per_glyph = 0.5 * self.ms_per_glyph + 0.5 * sample;
    }
}

/// Glyph atlas work one World frame found and performed.
#[derive(Debug, Default)]
pub struct GlyphFrameWork {
    queue: Vec<GlyphKey>,
    missing: BTreeSet<GlyphKey>,
    /// Distinct demanded entries found missing, including deferred ones.
    pub misses: u32,
    /// Entries rasterized into the atlas.
    pub populates: u32,
    /// Recoverable allocation or rasterization failures.
    pub failures: u32,
    /// Missing entries left for a later frame by the per-frame population cap or
    /// time budget.
    capped: u32,
}

impl GlyphFrameWork {
    /// Forget the previous frame's work.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.missing.clear();
        self.misses = 0;
        self.populates = 0;
        self.failures = 0;
        self.capped = 0;
    }

    /// Record a missing entry; queue it within the per-frame cap unless it backs off.
    fn miss(&mut self, atlas: &GlyphAtlas<impl RenderDevice>, key: GlyphKey) {
        if !self.missing.insert(key) {
            return;
        }

        self.misses += 1;
        if atlas.population_deferred(&key) {
            return;
        }

        if self.queue.len() < MAX_POPULATES_PER_FRAME {
            self.queue.push(key);
        } else {
            self.capped += 1;
        }
    }

    /// Whether the per-frame cap or time budget left missing entries that a
    /// following frame will populate; entries backing off after failures do not count.
    pub fn population_capped(&self) -> bool {
        self.capped > 0
    }

    /// Take at most `allowance` queued entries, in the order their runs published
    /// their demand; the rest wait for a later frame.
    pub fn take_queue(&mut self, allowance: usize) -> Vec<GlyphKey> {
        let mut queue = std::mem::take(&mut self.queue);
        if queue.len() > allowance {
            self.capped += (queue.len() - allowance) as u32;
            queue.truncate(allowance);
        }

        queue
    }

    /// Return a consumed queue so its capacity serves later frames.
    pub fn restore_queue(&mut self, mut queue: Vec<GlyphKey>) {
        queue.clear();
        self.queue = queue;
    }

    /// Report this frame's work in the submission statistics.
    pub fn publish(&self, stats: &mut RenderStats) {
        stats.glyph_misses = self.misses;
        stats.glyph_populates = self.populates;
        stats.glyph_population_failures = self.failures;
    }
}

/// Evaluated inputs of one text run, shared by demand publication and drawing.
#[derive(Clone, Copy, Debug)]
pub struct TextRun<'a> {
    /// Live entity owning the Surface component.
    pub entity: ipp_core::EntityId,
    /// Evaluated style carrying the stable primitive identity.
    pub style: &'a SurfacePrimitiveStyle,
    /// Effective clip rectangle in Surface metres.
    pub clip: SurfaceClipRect,
    /// Ready font asset incarnation.
    pub font_key: AssetKey,
    /// Metres per em.
    pub font_size: f32,
    /// Font units per em of the ready font.
    pub units_per_em: u32,
    /// Positioned glyphs in painter order.
    pub glyphs: &'a [SurfaceGlyph],
}

impl TextRun<'_> {
    /// Hash the inputs that select demanded entries, then every input shaping vertices.
    fn hashes(&self, band: Option<u16>) -> (u64, u64) {
        let mut demand = std::collections::hash_map::DefaultHasher::new();
        let mut paint = std::collections::hash_map::DefaultHasher::new();

        demand.write_u64(self.font_key.to_u64());
        demand.write_u32(self.font_size.to_bits());
        demand.write_u32(self.units_per_em);
        band.hash(&mut demand);
        for coord in self
            .clip
            .iter()
            .chain(&self.style.position)
            .chain(&self.style.scale)
        {
            demand.write_u32(coord.to_bits());
        }
        for lane in self.style.color {
            paint.write_u32(lane.to_bits());
        }
        paint.write_u32(self.style.opacity.to_bits());

        for glyph in self.glyphs {
            demand.write_u32(glyph.glyph_id);
            demand.write_u32(glyph.position[0].to_bits());
            demand.write_u32(glyph.position[1].to_bits());
            match glyph.color {
                Some(color) => {
                    paint.write_u8(1);
                    for lane in color {
                        paint.write_u32(lane.to_bits());
                    }
                }
                None => paint.write_u8(0),
            }
        }

        let demand = demand.finish();
        paint.write_u64(demand);
        (demand, paint.finish())
    }
}

/// Retained glyph quads of one text run sampling one atlas page, in painter order.
struct RetainedGlyphBatch {
    vertices: Vec<GuiVertex>,
    page_index: usize,
    /// Cache-unique identity of this content, changed by every rebuild.
    revision: u64,
}

/// One text run's band, published demand and retained batches.
struct RetainedGlyphRun {
    band: Option<u16>,
    demand_hash: Option<u64>,
    geometry_hash: u64,
    /// Paint revision and band the hashes were computed under.
    hashed: Option<(u64, Option<u16>)>,
    /// Sorted demanded entries.
    keys: Vec<GlyphKey>,
    /// Residency generation at which every demanded entry was resident.
    resident: Option<u64>,
    /// Publication that last showed this run.
    seen: u64,
    /// Geometry hash the batches were built from.
    built_hash: Option<u64>,
    batches: Vec<RetainedGlyphBatch>,
}

/// Text runs of one Surface.
struct RetainedGlyphSurface {
    runs: BTreeMap<SurfacePrimitiveIdentity, RetainedGlyphRun>,
    /// Publication that last showed or kept this Surface.
    seen: u64,
    /// Runs shown by publication `seen`; `None` keeps every run of a culled Surface.
    shown: Option<usize>,
}

/// Renderer-owned retained text runs of one World: bands, atlas demand and the glyph
/// quads their Surfaces' [GUI storage](super::gui_storage) draws.
#[derive(Default)]
pub struct GlyphBatchRenderCache {
    surfaces: BTreeMap<ipp_core::EntityId, RetainedGlyphSurface>,
    publication: u64,
    /// Last batch revision handed out.
    revision: u64,
    scratch_keys: Vec<GlyphKey>,
}

impl GlyphBatchRenderCache {
    /// Create a new empty text batch render cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Release every retained text run; demand belongs to the atlas.
    pub fn clear(&mut self) {
        self.surfaces.clear();
    }

    /// Discard retained quads before the graphics context goes away.
    ///
    /// Bands and published demand survive, so recovered runs keep their presentation
    /// quality and rebuild from entries repopulated into their original slots.
    pub fn release_context(&mut self) {
        for run in self
            .surfaces
            .values_mut()
            .flat_map(|surface| surface.runs.values_mut())
        {
            run.batches.clear();
            run.built_hash = None;
            run.resident = None;
        }
    }

    /// Release this World's demand from the atlas and every retained run.
    pub fn release_demand<D: RenderDevice>(&mut self, atlas: &mut GlyphAtlas<D>) {
        for surface in self.surfaces.values() {
            for run in surface.runs.values() {
                atlas.release(&run.keys);
            }
        }
        self.clear();
    }

    /// Start publishing this World's demand for one frame.
    pub fn begin_publication(&mut self) {
        self.publication += 1;
    }

    /// Keep a culled Surface's runs with their bands, demand and batches.
    pub fn keep_surface(&mut self, entity: ipp_core::EntityId) {
        if let Some(surface) = self.surfaces.get_mut(&entity) {
            surface.seen = self.publication;
            surface.shown = None;
        }
    }

    /// Publish one visible run's band and demand, recording entries missing from the atlas.
    ///
    /// `glyph_bounds` returns font-unit bounds for glyphs with coverage; other glyphs need
    /// no entry. Unchanged runs only hash their inputs, and not even that when the
    /// Surface's reusable `paint` revision and the band match their last hashes.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_run<D: RenderDevice>(
        &mut self,
        atlas: &mut GlyphAtlas<D>,
        run: &TextRun<'_>,
        paint: SurfacePaint,
        projected_height: f32,
        glyph_bounds: impl Fn(u32) -> Option<[f32; 4]>,
        work: &mut GlyphFrameWork,
    ) {
        let publication = self.publication;
        let surface = self
            .surfaces
            .entry(run.entity)
            .or_insert_with(|| RetainedGlyphSurface {
                runs: BTreeMap::new(),
                seen: publication,
                shown: Some(0),
            });
        if surface.seen != publication {
            surface.seen = publication;
            surface.shown = Some(0);
        }

        let record = surface
            .runs
            .entry(run.style.identity)
            .or_insert_with(empty_run);
        if record.seen != publication {
            record.seen = publication;
            surface.shown = surface.shown.map(|shown| shown + 1);
        }

        record.band = select_resolution_band(projected_height, record.band);
        let hashed = record
            .hashed
            .is_some_and(|(revision, band)| paint.reuses(revision) && band == record.band);
        let demand_hash = if hashed {
            record.demand_hash
        } else {
            let (demand_hash, geometry_hash) = run.hashes(record.band);
            record.geometry_hash = geometry_hash;
            record.hashed = Some((paint.revision, record.band));
            Some(demand_hash)
        };

        if record.demand_hash != demand_hash {
            self.scratch_keys.clear();
            if let Some(band) = record.band {
                let unit = run.font_size / run.units_per_em as f32;
                for glyph in run.glyphs {
                    if let Some(bounds) = glyph_bounds(glyph.glyph_id)
                        && glyph_intersects_clip(run.style, glyph, bounds, unit, run.clip)
                    {
                        self.scratch_keys.push(GlyphKey {
                            font_key: run.font_key,
                            glyph_id: glyph.glyph_id,
                            resolution_band: band,
                        });
                    }
                }
                self.scratch_keys.sort_unstable();
                self.scratch_keys.dedup();
            }

            // Acquire first, so entries kept by the edit never look idle.
            atlas.acquire(&self.scratch_keys);
            atlas.release(&record.keys);
            std::mem::swap(&mut record.keys, &mut self.scratch_keys);
            record.demand_hash = demand_hash;
            record.resident = None;
        }

        let generation = atlas.residency_generation();
        if record.resident != Some(generation) {
            let mut complete = true;
            for key in &record.keys {
                if atlas.get(key).is_none() {
                    complete = false;
                    work.miss(atlas, *key);
                }
            }
            if complete {
                record.resident = Some(generation);
            }
        }
    }

    /// Release the demand and batches of runs this publication no longer shows.
    ///
    /// Surfaces this publication neither showed nor kept were destroyed or lost their
    /// text. A shown Surface releases the runs it no longer contains.
    pub fn end_publication<D: RenderDevice>(&mut self, atlas: &mut GlyphAtlas<D>) {
        let publication = self.publication;
        self.surfaces.retain(|_, surface| {
            if surface.seen != publication {
                for run in surface.runs.values() {
                    atlas.release(&run.keys);
                }
                return false;
            }

            if surface
                .shown
                .is_some_and(|shown| shown < surface.runs.len())
            {
                surface.runs.retain(|_, run| {
                    if run.seen == publication {
                        return true;
                    }

                    atlas.release(&run.keys);
                    false
                });
            }
            true
        });
    }

    /// Prepare one published run's retained batches for drawing.
    ///
    /// Batches rebuild after an edit or after a page they sample retires. Returns
    /// `false` when the run has no band or a demanded entry is not resident; the caller
    /// then draws analytic glyphs.
    pub fn prepare_text_run<D: RenderDevice>(
        &mut self,
        atlas: &GlyphAtlas<D>,
        run: &TextRun<'_>,
        stats: &mut RenderStats,
    ) -> bool {
        let Some(record) = self
            .surfaces
            .get_mut(&run.entity)
            .and_then(|surface| surface.runs.get_mut(&run.style.identity))
        else {
            return false;
        };
        let Some(band) = record.band else {
            return false;
        };

        let generation = atlas.residency_generation();
        if record.resident != Some(generation) {
            if !record.keys.iter().all(|key| atlas.get(key).is_some()) {
                return false;
            }
            record.resident = Some(generation);
        }

        let reusable = record.built_hash == Some(record.geometry_hash)
            && record
                .batches
                .iter()
                .all(|batch| atlas.has_page(batch.page_index));
        if !reusable {
            rebuild_batches(atlas, record, run, band, &mut self.revision, stats);
        }

        true
    }

    /// Retained batches of a prepared run as GUI storage pieces, in painter order.
    pub(crate) fn run_pieces(
        &self,
        entity: ipp_core::EntityId,
        identity: SurfacePrimitiveIdentity,
    ) -> impl Iterator<Item = GuiPiece> + '_ {
        self.run(entity, identity)
            .into_iter()
            .flat_map(|run| run.batches.iter().enumerate())
            .map(move |(index, batch)| GuiPiece {
                key: GuiPieceKey::Glyphs(identity, index as u32),
                hash: batch.revision,
                len: batch.vertices.len(),
                page: Some(batch.page_index),
                source: GuiPieceSource::Glyphs(identity, index as u32),
            })
    }

    /// Vertices of retained batch `index` of a run; empty when it does not exist.
    pub fn batch_vertices(
        &self,
        entity: ipp_core::EntityId,
        identity: SurfacePrimitiveIdentity,
        index: u32,
    ) -> &[GuiVertex] {
        self.run(entity, identity)
            .and_then(|run| run.batches.get(index as usize))
            .map_or(&[], |batch| &batch.vertices)
    }

    fn run(
        &self,
        entity: ipp_core::EntityId,
        identity: SurfacePrimitiveIdentity,
    ) -> Option<&RetainedGlyphRun> {
        self.surfaces.get(&entity)?.runs.get(&identity)
    }
}

/// A run before its first publication.
fn empty_run() -> RetainedGlyphRun {
    RetainedGlyphRun {
        band: None,
        demand_hash: None,
        geometry_hash: 0,
        hashed: None,
        keys: Vec::new(),
        resident: None,
        seen: 0,
        built_hash: None,
        batches: Vec::new(),
    }
}

/// Glyph quads of one atlas page, in the order they paint.
struct PageBucket {
    page_index: usize,
    vertices: Vec<GuiVertex>,
    /// Quad bounds and colours, kept only when the run mixes colours.
    quads: Vec<([f32; 4], [f32; 4])>,
}

/// Replace a run's batches with complete geometry grouped by atlas page.
///
/// Quads join the last batch of their page unless a later batch holds an overlapping quad
/// of another colour. Same-colour coverage composites identically in either order, so a
/// run of one colour needs one batch per page however its pages interleave.
fn rebuild_batches<D: RenderDevice>(
    atlas: &GlyphAtlas<D>,
    record: &mut RetainedGlyphRun,
    run: &TextRun<'_>,
    band: u16,
    revision: &mut u64,
    stats: &mut RenderStats,
) {
    let style = run.style;
    let unit = run.font_size / run.units_per_em as f32;
    let uniform = run
        .glyphs
        .iter()
        .all(|glyph| glyph.color.is_none_or(|color| color == style.color));
    let mut buckets: Vec<PageBucket> = Vec::new();

    for glyph in run.glyphs {
        let key = GlyphKey {
            font_key: run.font_key,
            glyph_id: glyph.glyph_id,
            resolution_band: band,
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
        let bounds = [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)];
        let clip = run.clip;
        if bounds[2] <= clip[0]
            || bounds[0] >= clip[2]
            || bounds[3] <= clip[1]
            || bounds[1] >= clip[3]
        {
            continue;
        }

        let target = buckets
            .iter()
            .rposition(|bucket| bucket.page_index == entry.page_index)
            .filter(|&index| buckets[index].vertices.len() < MAX_BATCH_GLYPHS * 6)
            .filter(|&index| {
                uniform
                    || !buckets[index + 1..]
                        .iter()
                        .flat_map(|bucket| &bucket.quads)
                        .any(|(other, other_color)| {
                            *other_color != color
                                && other[0] < bounds[2]
                                && bounds[0] < other[2]
                                && other[1] < bounds[3]
                                && bounds[1] < other[3]
                        })
            });
        let bucket = match target {
            Some(index) => &mut buckets[index],
            None => {
                buckets.push(PageBucket {
                    page_index: entry.page_index,
                    vertices: Vec::new(),
                    quads: Vec::new(),
                });
                buckets.last_mut().expect("pushed bucket")
            }
        };

        let [u0, v0, u1, v1] = entry.uv;
        let clip = run.clip;
        let vertex = |position, [u, v]: [f32; 2]| GuiVertex {
            position,
            color0: color,
            gradient_coords: [u, v, 0.0, 0.0],
            material_params: [GUI_FILL_GLYPH, 0.0, 0.0, 1.0],
            clip,
            ..GuiVertex::EMPTY
        };
        let tl = vertex([x0, y0], [u0, v0]);
        let bl = vertex([x0, y1], [u0, v1]);
        let br = vertex([x1, y1], [u1, v1]);
        let tr = vertex([x1, y0], [u1, v0]);

        // Emit 6 vertices: CCW front face [TL, BL, BR, TL, BR, TR]
        bucket.vertices.extend_from_slice(&[tl, bl, br, tl, br, tr]);
        if !uniform {
            bucket.quads.push((bounds, color));
        }
    }

    record.batches.clear();
    for bucket in buckets {
        *revision += 1;
        record.batches.push(RetainedGlyphBatch {
            vertices: bucket.vertices,
            page_index: bucket.page_index,
            revision: *revision,
        });
        stats.gui_rebuilds += 1;
    }

    record.built_hash = Some(record.geometry_hash);
}

impl<D: RenderDevice> Drop for GlyphAtlas<D> {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
#[path = "glyph_atlas_tests.rs"]
mod tests;
