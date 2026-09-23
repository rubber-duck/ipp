//! Tests for glyph coverage atlas packing, resolution bands with hysteresis, demand
//! accounting and page retirement, and retained text batch caching.

use std::cell::RefCell;
use std::rc::Rc;

use super::{
    ATLAS_PAGE_SIZE, GlyphAtlas, GlyphAtlasLimits, GlyphBatchRenderCache, GlyphFrameWork, GlyphKey,
    GlyphVertex, MAX_POPULATES_PER_FRAME, POPULATE_RETRY_TICKS, RESOLUTION_BANDS, TextRun,
    select_resolution_band,
};
use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::services::asset_management::AssetKey;
use ipp_core::systems::surface::{SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle};

#[derive(Default)]
struct MockAtlasDevice {
    created_batches: Vec<(usize, usize)>,
    updated_batches: Vec<(usize, usize)>,
    deleted_batches: Vec<usize>,
    draws: Vec<(usize, [f32; 4])>,
    created_pages: Vec<u32>,
    deleted_pages: Vec<u32>,
    sampled_pages: Vec<u32>,
    next_id: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct MockBatch {
    id: usize,
    vertex_count: usize,
}

impl RenderDevice for MockAtlasDevice {
    type Program = u32;
    type Mesh = u32;
    type Texture = u32;
    type SurfacePath = u32;
    type SurfaceCacheTarget = ();
    #[cfg(feature = "shadows")]
    type ShadowMap = u32;
    type GuiBatch = MockBatch;
    type GlyphBatch = MockBatch;
    type GlyphAtlasPage = u32;

    fn set_lighting(
        &mut self,
        _program: &Self::Program,
        _model: &[f32; 16],
        _normal: &[f32; 16],
        _surface: &[f32; 3],
        _frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn create_shadow_map(&mut self, _size: u32) -> Result<Self::ShadowMap, RenderError> {
        Ok(0)
    }

    #[cfg(feature = "shadows")]
    fn begin_shadow(
        &mut self,
        _map: &Self::ShadowMap,
        _slot: u32,
        _grid: u32,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn end_shadow(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn bind_shadow(
        &mut self,
        _program: &Self::Program,
        _map: &Self::ShadowMap,
        _frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn delete_shadow_map(&mut self, _map: Self::ShadowMap) {}

    fn create_program(
        &mut self,
        _vertex: &str,
        _fragment: &str,
    ) -> Result<Self::Program, RenderError> {
        self.next_id += 1;
        Ok(self.next_id as u32)
    }

    fn create_mesh(&mut self, _asset: &ipp_core::MeshAsset) -> Result<Self::Mesh, RenderError> {
        self.next_id += 1;
        Ok(self.next_id as u32)
    }

    fn create_texture(
        &mut self,
        _width: u32,
        _height: u32,
        _pixels: &[u8],
    ) -> Result<Self::Texture, RenderError> {
        self.next_id += 1;
        Ok(self.next_id as u32)
    }

    fn allocate_texture(
        &mut self,
        _width: u32,
        _height: u32,
    ) -> Result<Self::Texture, RenderError> {
        self.next_id += 1;
        Ok(self.next_id as u32)
    }

    fn upload_texture_rows(
        &mut self,
        _texture: &Self::Texture,
        _width: u32,
        _first_row: u32,
        _rows: u32,
        _pixels: &[u8],
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn begin_frame(
        &mut self,
        _width: u32,
        _height: u32,
        _clear: &[f32; 4],
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn draw(
        &mut self,
        _program: &Self::Program,
        _mesh: &Self::Mesh,
        _mvp: &[f32; 16],
        _material: &[f32; 3],
        #[cfg(feature = "mesh-poses")] _pose: Option<(&Self::Mesh, f32)>,
        _texture: Option<&Self::Texture>,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn delete_mesh(&mut self, _mesh: Self::Mesh) {}

    fn delete_texture(&mut self, _texture: Self::Texture) {}

    fn delete_program(&mut self, _program: Self::Program) {}

    fn create_glyph_batch(
        &mut self,
        vertices: &[GlyphVertex],
    ) -> Result<Self::GlyphBatch, RenderError> {
        self.next_id += 1;
        let id = self.next_id;
        self.created_batches.push((id, vertices.len()));
        Ok(MockBatch {
            id,
            vertex_count: vertices.len(),
        })
    }

    fn update_glyph_batch(
        &mut self,
        batch: &mut Self::GlyphBatch,
        vertices: &[GlyphVertex],
    ) -> Result<(), RenderError> {
        batch.vertex_count = vertices.len();
        self.updated_batches.push((batch.id, vertices.len()));
        Ok(())
    }

    fn delete_glyph_batch(&mut self, batch: Self::GlyphBatch) {
        self.deleted_batches.push(batch.id);
    }

    fn draw_glyph_batch(
        &mut self,
        _program: &Self::Program,
        batch: &Self::GlyphBatch,
        atlas: &Self::Texture,
        _mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.draws.push((batch.id, *clip));
        self.sampled_pages.push(*atlas);
        Ok(())
    }

    fn create_glyph_atlas_page(
        &mut self,
        _width: u32,
        _height: u32,
    ) -> Result<Self::GlyphAtlasPage, RenderError> {
        self.next_id += 1;
        let id = self.next_id as u32;
        self.created_pages.push(id);
        Ok(id)
    }

    fn delete_glyph_atlas_page(&mut self, page: Self::GlyphAtlasPage) {
        self.deleted_pages.push(page);
    }

    fn begin_glyph_atlas_page(&mut self, _page: &Self::GlyphAtlasPage) -> Result<(), RenderError> {
        Ok(())
    }

    fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture {
        page
    }
}

const FONT: AssetKey = AssetKey {
    slot: 1,
    generation: 1,
};

/// Font-unit bounds of every test glyph with coverage.
const INK: [f32; 4] = [0.0, -800.0, 600.0, 200.0];

/// Glyph without curves, such as a space: it needs no atlas entry.
const SPACE: u32 = 32;

/// Projected em height that selects band 32 without history.
const BAND_32_HEIGHT: f32 = 30.0;

/// Clip containing every test glyph.
const CLIP: [f32; 4] = [0.0, 0.0, 10.0, 10.0];

type Atlas = GlyphAtlas<MockAtlasDevice>;

fn key(glyph_id: u32, resolution_band: u16) -> GlyphKey {
    GlyphKey {
        font_key: FONT,
        glyph_id,
        resolution_band,
    }
}

fn style(item: u32) -> SurfacePrimitiveStyle {
    SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Authored(ipp_core::SurfaceItemId(item)),
        position: [0.0; 2],
        scale: [1.0; 2],
        color: [1.0; 4],
        opacity: 1.0,
        clip: None,
    }
}

fn glyphs(ids: &[u32]) -> Vec<SurfaceGlyph> {
    ids.iter()
        .enumerate()
        .map(|(index, &glyph_id)| SurfaceGlyph {
            glyph_id,
            position: [0.1 * index as f32, 0.1],
            color: None,
        })
        .collect()
}

fn text_run<'a>(
    entity: u64,
    style: &'a SurfacePrimitiveStyle,
    glyphs: &'a [SurfaceGlyph],
) -> TextRun<'a> {
    TextRun {
        entity: ipp_core::EntityId::from_bits(entity),
        style,
        clip: CLIP,
        font_key: FONT,
        font_size: 0.05,
        units_per_em: 1000,
        glyphs,
    }
}

/// One World's retained text runs and per-frame glyph work.
struct TestWorld {
    cache: GlyphBatchRenderCache<MockAtlasDevice>,
    work: GlyphFrameWork,
}

impl TestWorld {
    fn new(device: &Rc<RefCell<MockAtlasDevice>>) -> Self {
        Self {
            cache: GlyphBatchRenderCache::new(device.clone()),
            work: GlyphFrameWork::default(),
        }
    }

    /// Publish one frame: `shown` runs are visible and `kept` Surfaces are culled.
    fn publish(&mut self, atlas: &mut Atlas, shown: &[(TextRun<'_>, f32)], kept: &[u64]) {
        self.work.clear();
        atlas.begin_publication();
        self.cache.begin_publication();
        for &entity in kept {
            self.cache
                .keep_surface(ipp_core::EntityId::from_bits(entity));
        }

        for (run, height) in shown {
            self.cache.publish_run(
                atlas,
                run,
                *height,
                |glyph_id| (glyph_id != SPACE).then_some(INK),
                &mut self.work,
            );
        }

        self.cache.end_publication(atlas);
        atlas.release_if_unused();
    }

    /// Allocate every queued miss at `size` texels, as population does before drawing.
    fn populate(&mut self, atlas: &mut Atlas, size: u32) -> usize {
        let queue = self.work.take_queue();
        for key in &queue {
            atlas.allocate_slot(*key, size, size, INK).unwrap();
        }

        let populated = queue.len();
        self.work.restore_queue(queue);
        populated
    }

    fn draw(&mut self, atlas: &Atlas, run: &TextRun<'_>) -> (bool, RenderStats) {
        let mut stats = RenderStats::default();
        let drawn = self
            .cache
            .draw_text_run(&1, atlas, run, &[1.0; 16], &mut stats)
            .unwrap();
        (drawn, stats)
    }
}

fn setup() -> (Rc<RefCell<MockAtlasDevice>>, Atlas, TestWorld) {
    let device = Rc::new(RefCell::new(MockAtlasDevice::default()));
    let atlas = GlyphAtlas::new(device.clone());
    let world = TestWorld::new(&device);
    (device, atlas, world)
}

#[test]
fn resolution_band_selection_selects_expected_band_or_none_for_extremes() {
    // Extreme scales below 10.0 px or above 80.0 px fall back to analytic curves.
    for height in [5.0, 9.9, 80.1, 120.0, f32::NAN] {
        assert_eq!(select_resolution_band(height, None), None, "{height}");
    }

    // Stable discrete resolution bands: 16, 24, 32, 48, 64.
    for (height, band) in [
        (12.0, 16),
        (16.0, 16),
        (20.0, 24),
        (24.0, 24),
        (28.0, 32),
        (32.0, 32),
        (40.0, 48),
        (48.0, 48),
        (56.0, 64),
        (64.0, 64),
        (75.0, 64),
    ] {
        assert_eq!(select_resolution_band(height, None), Some(band), "{height}");
    }

    for &band in &RESOLUTION_BANDS {
        assert_eq!(select_resolution_band(band as f32, None), Some(band));
    }
}

#[test]
fn band_hysteresis_keeps_the_current_band_near_its_thresholds() {
    // Band 24 serves up to 27.3 px without history; band 32 starts above it.
    assert_eq!(select_resolution_band(27.0, None), Some(24));
    assert_eq!(select_resolution_band(27.6, None), Some(32));

    // Jitter across the threshold keeps whichever band a run already has.
    for height in [26.0, 27.6, 28.5, 29.9, 26.4] {
        assert_eq!(
            select_resolution_band(height, Some(24)),
            Some(24),
            "{height}"
        );
    }
    for height in [28.5, 26.0, 24.6, 27.0] {
        assert_eq!(
            select_resolution_band(height, Some(32)),
            Some(32),
            "{height}"
        );
    }

    // Moving clearly past the widened range selects the nearest band again.
    assert_eq!(select_resolution_band(30.1, Some(24)), Some(32));
    assert_eq!(select_resolution_band(24.4, Some(32)), Some(24));

    // The atlas range edges hold too: text leaves the atlas only well outside it.
    assert_eq!(select_resolution_band(85.0, Some(64)), Some(64));
    assert_eq!(select_resolution_band(88.5, Some(64)), None);
    assert_eq!(select_resolution_band(9.2, Some(16)), Some(16));
    assert_eq!(select_resolution_band(8.9, Some(16)), None);
    assert_eq!(select_resolution_band(f32::NAN, Some(16)), None);
}

#[test]
fn atlas_page_shelf_packing_allocates_slots_and_wraps_rows() {
    let (_, mut atlas, _) = setup();

    // Allocate first glyph: 30x20 pixels
    let (pos1, page1, entry1) = atlas
        .allocate_slot(key(65, 24), 30, 20, [0.0, -100.0, 300.0, 100.0])
        .unwrap();

    assert_eq!(page1, 0);
    // 1-pixel border pad: slot_x is 0, content_x is 1
    assert_eq!(pos1, [1, 1]);
    assert_eq!(entry1.pixel_size, [30, 20]);
    assert_eq!(atlas.page_count(), 1);

    // Allocate second glyph in same row: 20x20
    let (pos2, page2, entry2) = atlas
        .allocate_slot(key(66, 24), 20, 20, [0.0, -100.0, 200.0, 100.0])
        .unwrap();

    assert_eq!(page2, 0);
    // slot_w of previous was 30 + 2 = 32, so slot_x = 32, content_x = 33
    assert_eq!(pos2, [33, 1]);
    assert_eq!(entry2.pixel_size, [20, 20]);
}

#[test]
fn page_budget_rejects_demanded_pages_and_reclaims_idle_ones() {
    let (device, mut atlas, _) = setup();
    let max_pages = GlyphAtlasLimits::DEFAULT.max_pages;

    // Large glyphs fill one page each; every page holds a demanded entry.
    let demanded: Vec<_> = (0..max_pages as u32).map(|id| key(id, 64)).collect();
    atlas.acquire(&demanded);
    for (index, key) in demanded.iter().enumerate() {
        let (_, page, _) = atlas
            .allocate_slot(*key, 500, 500, [0.0, 0.0, 1000.0, 1000.0])
            .unwrap();
        assert_eq!(page, index);
    }

    assert_eq!(atlas.page_count(), max_pages as u32);
    assert_eq!(
        atlas.resident_bytes(),
        max_pages * (ATLAS_PAGE_SIZE as usize * ATLAS_PAGE_SIZE as usize)
    );
    assert!(
        atlas
            .allocate_slot(key(999, 64), 500, 500, [0.0; 4])
            .is_err(),
        "a full budget of demanded pages rejects the allocation"
    );

    // Once one page loses its demand, pressure reclaims it for the new entry.
    atlas.release(&demanded[1..2]);
    let (_, page, _) = atlas
        .allocate_slot(key(999, 64), 500, 500, [0.0; 4])
        .unwrap();
    assert_eq!(page, max_pages);
    assert!(atlas.get(&demanded[1]).is_none());
    assert!(atlas.get(&demanded[0]).is_some());
    assert_eq!(atlas.take_retired_pages(), 1);

    // Clear releases all pages without counting retirements.
    atlas.clear();
    assert_eq!(atlas.page_count(), 0);
    assert_eq!(atlas.resident_bytes(), 0);
    assert_eq!(device.borrow().deleted_pages.len(), max_pages + 1);
    assert_eq!(atlas.take_retired_pages(), 0);
}

#[test]
fn coverage_evaluation_bounds_and_symmetry() {
    // Single vertical line segment: from (0, -10) to (0, 10)
    let curves = [[0.0, -10.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0]];
    let pixels_per_unit = [1.0, 1.0];

    let cov_center = evaluate_glyph_coverage(&curves, [0.0, 0.0], pixels_per_unit);
    assert!((0.0..=1.0).contains(&cov_center));

    // Point far away should have near-zero coverage
    let cov_far = evaluate_glyph_coverage(&curves, [100.0, 100.0], pixels_per_unit);
    assert_eq!(cov_far, 0.0);
}

#[test]
fn warm_runs_reuse_demand_and_batches_without_rebuilding_keys() {
    let (device, mut atlas, mut world) = setup();
    let style = style(1);
    let glyphs = glyphs(&[10, 20, 10]);
    let run = text_run(42, &style, &glyphs);

    // Cold: two distinct misses, populated before drawing.
    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    assert_eq!(world.work.misses, 2);
    assert_eq!(world.populate(&mut atlas, 20), 2);
    let (drawn, cold) = world.draw(&atlas, &run);
    assert!(drawn);
    assert_eq!((cold.gui_rebuilds, cold.gui_batches), (1, 1));
    assert_eq!(cold.uploaded_bytes, 3 * 6 * 32);

    let record = &world.cache.surfaces[&run.entity].runs[&style.identity];
    let keys = record.keys.as_ptr();
    let generation = atlas.residency_generation();

    // Warm: hashing finds the run unchanged; its key list and residency stay as they were.
    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    assert_eq!(world.work.misses, 0);
    let record = &world.cache.surfaces[&run.entity].runs[&style.identity];
    assert_eq!(record.keys.as_ptr(), keys, "no demand set was rebuilt");
    assert_eq!(record.resident, Some(generation));
    let (drawn, warm) = world.draw(&atlas, &run);
    assert!(drawn);
    assert_eq!((warm.gui_rebuilds, warm.gui_allocations), (0, 0));
    assert_eq!(warm.uploaded_bytes, 0);
    assert_eq!(warm.gui_batches, 1);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert!(device.borrow().updated_batches.is_empty());
}

#[test]
fn text_edits_rebuild_the_run_and_update_its_demand() {
    let (device, mut atlas, mut world) = setup();
    let style = style(1);
    let before = glyphs(&[10]);
    let after = glyphs(&[10, 20]);

    world.publish(
        &mut atlas,
        &[(text_run(42, &style, &before), BAND_32_HEIGHT)],
        &[],
    );
    world.populate(&mut atlas, 20);
    world.draw(&atlas, &text_run(42, &style, &before));

    // Appending a glyph misses once, then replaces the batch storage.
    let edited = text_run(42, &style, &after);
    world.publish(&mut atlas, &[(edited, BAND_32_HEIGHT)], &[]);
    assert_eq!(world.work.misses, 1);
    assert!(
        !world.draw(&atlas, &edited).0,
        "a run waits for every demanded entry"
    );
    world.populate(&mut atlas, 20);
    let (drawn, stats) = world.draw(&atlas, &edited);
    assert!(drawn);
    assert_eq!(stats.gui_rebuilds, 1);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().updated_batches.len(), 1);
    assert_eq!(device.borrow().updated_batches[0].1, 12);
    assert_eq!(atlas.demand.get(&key(20, 32)), Some(&1));
}

#[test]
fn whitespace_glyphs_need_no_entries_and_emit_no_quads() {
    let (device, mut atlas, mut world) = setup();
    let style = style(1);
    // "A A": the space between two glyphs has no curves.
    let glyphs = glyphs(&[65, SPACE, 65]);
    let run = text_run(42, &style, &glyphs);

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    assert_eq!(world.work.misses, 1);
    world.populate(&mut atlas, 20);

    // A resident entry with empty bounds is omitted from the quads too.
    atlas.allocate_slot(key(SPACE, 32), 0, 0, [0.0; 4]).unwrap();
    assert!(world.draw(&atlas, &run).0);

    // Only 2 visible glyphs emitted quads (2 * 6 = 12 vertices).
    assert_eq!(device.borrow().created_batches[0].1, 12);
}

#[test]
fn runs_leaving_a_shown_surface_release_demand_and_batches() {
    let (device, mut atlas, mut world) = setup();
    let [first, second] = [style(1), style(2)];
    let glyphs_a = glyphs(&[1]);
    let glyphs_b = glyphs(&[2]);
    let run_a = text_run(42, &first, &glyphs_a);
    let run_b = text_run(42, &second, &glyphs_b);

    world.publish(
        &mut atlas,
        &[(run_a, BAND_32_HEIGHT), (run_b, BAND_32_HEIGHT)],
        &[],
    );
    world.populate(&mut atlas, 20);
    world.draw(&atlas, &run_a);
    world.draw(&atlas, &run_b);
    assert_eq!(
        world.cache.resident_bytes(),
        2 * 6 * std::mem::size_of::<GlyphVertex>()
    );

    // The Surface stays visible without its second run.
    world.publish(&mut atlas, &[(run_a, BAND_32_HEIGHT)], &[]);
    assert_eq!(
        world.cache.resident_bytes(),
        6 * std::mem::size_of::<GlyphVertex>()
    );
    assert_eq!(device.borrow().deleted_batches.len(), 1);
    assert!(!atlas.demand.contains_key(&key(2, 32)));
    assert!(
        atlas.get(&key(2, 32)).is_some(),
        "an entry without demand stays resident"
    );

    // A destroyed Surface releases the rest; with no demand left, the atlas empties.
    world.publish(&mut atlas, &[], &[]);
    assert_eq!(world.cache.resident_bytes(), 0);
    assert_eq!(atlas.page_count(), 0);
    assert_eq!(atlas.take_retired_pages(), 1);
}

#[test]
fn culled_surfaces_keep_runs_bands_and_atlas_demand() {
    let (device, mut atlas, mut world) = setup();
    let style = style(1);
    let glyphs = glyphs(&[1]);
    let run = text_run(1, &style, &glyphs);

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    world.populate(&mut atlas, 20);
    world.draw(&atlas, &run);
    let generation = atlas.residency_generation();

    // Culled for many frames: demand, entries and retained runs all survive.
    for _ in 0..4 {
        world.publish(&mut atlas, &[], &[1]);
    }
    assert_eq!(atlas.residency_generation(), generation);
    assert!(atlas.get(&key(1, 32)).is_some());
    assert!(device.borrow().deleted_pages.is_empty());
    assert!(device.borrow().deleted_batches.is_empty());

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    assert_eq!(world.work.misses, 0);
    let (drawn, visible_again) = world.draw(&atlas, &run);
    assert!(drawn);
    assert_eq!(visible_again.uploaded_bytes, 0);
    assert_eq!(visible_again.gui_rebuilds, 0);
    assert_eq!(visible_again.gui_allocations, 0);
    assert_eq!(device.borrow().created_batches.len(), 1);
}

#[test]
fn band_oscillation_neither_repopulates_nor_retires_pages() {
    let (device, mut atlas, mut world) = setup();
    let style = style(1);
    let glyphs = glyphs(&[1, 2, 3]);
    let run = text_run(1, &style, &glyphs);

    // 26 px selects band 24; camera jitter crosses the 27.3 px threshold repeatedly.
    world.publish(&mut atlas, &[(run, 26.0)], &[]);
    world.populate(&mut atlas, 20);
    world.draw(&atlas, &run);
    let uploads = device.borrow().created_batches.len();
    for height in [27.5, 26.8, 28.9, 27.1, 29.5, 26.2] {
        world.publish(&mut atlas, &[(run, height)], &[]);
        assert_eq!(world.work.misses, 0, "{height}");
        let (drawn, stats) = world.draw(&atlas, &run);
        assert!(drawn);
        assert_eq!(
            (stats.uploaded_bytes, stats.gui_rebuilds),
            (0, 0),
            "{height}"
        );
    }
    assert_eq!(device.borrow().created_batches.len(), uploads);
    assert!(device.borrow().updated_batches.is_empty());

    // A real zoom changes bands once; returning finds the idle entries still resident.
    world.publish(&mut atlas, &[(run, 40.0)], &[]);
    assert_eq!(world.work.misses, 3);
    world.populate(&mut atlas, 20);
    assert!(world.draw(&atlas, &run).0);
    world.publish(&mut atlas, &[(run, 22.0)], &[]);
    assert_eq!(
        world.work.misses, 0,
        "band 24 entries survived without demand"
    );
    assert!(world.draw(&atlas, &run).0);
    assert_eq!(atlas.take_retired_pages(), 0);
    assert!(device.borrow().deleted_pages.is_empty());
}

#[test]
fn idle_pages_persist_until_the_configured_publication_limit() {
    let (_, mut atlas, mut world) = setup();
    atlas.set_limits(GlyphAtlasLimits {
        max_pages: 4,
        idle_page_publications: 3,
    });
    let [kept, dropped] = [style(1), style(2)];
    let glyphs_kept = glyphs(&[1]);
    let glyphs_dropped = glyphs(&[2]);
    let kept_run = text_run(1, &kept, &glyphs_kept);
    let dropped_run = text_run(1, &dropped, &glyphs_dropped);

    // Each run's glyph lands on its own page.
    world.publish(&mut atlas, &[(kept_run, BAND_32_HEIGHT)], &[]);
    world.populate(&mut atlas, 500);
    world.publish(
        &mut atlas,
        &[(kept_run, BAND_32_HEIGHT), (dropped_run, BAND_32_HEIGHT)],
        &[],
    );
    world.populate(&mut atlas, 500);
    assert_eq!(atlas.page_count(), 2);

    // The dropped run's page stays for the configured number of idle publications.
    world.publish(&mut atlas, &[(kept_run, BAND_32_HEIGHT)], &[]);
    for _ in 0..2 {
        world.publish(&mut atlas, &[(kept_run, BAND_32_HEIGHT)], &[]);
        assert_eq!(atlas.page_count(), 2);
    }
    world.publish(&mut atlas, &[(kept_run, BAND_32_HEIGHT)], &[]);
    assert_eq!(atlas.page_count(), 1);
    assert_eq!(atlas.take_retired_pages(), 1);
    assert!(atlas.get(&key(1, 32)).is_some());
}

#[test]
fn retiring_one_page_rebuilds_only_the_runs_that_sample_it() {
    let (device, mut atlas, mut world) = setup();
    atlas.set_limits(GlyphAtlasLimits {
        max_pages: 2,
        idle_page_publications: 600,
    });
    let [first, second, third] = [style(1), style(2), style(3)];
    let [glyphs_a, glyphs_b, glyphs_c] = [glyphs(&[1]), glyphs(&[2]), glyphs(&[3])];
    let run_a = text_run(1, &first, &glyphs_a);
    let run_b = text_run(1, &second, &glyphs_b);
    let run_c = text_run(1, &third, &glyphs_c);

    world.publish(&mut atlas, &[(run_a, BAND_32_HEIGHT)], &[]);
    world.populate(&mut atlas, 500);
    world.publish(
        &mut atlas,
        &[(run_a, BAND_32_HEIGHT), (run_b, BAND_32_HEIGHT)],
        &[],
    );
    world.populate(&mut atlas, 500);
    world.draw(&atlas, &run_a);
    world.draw(&atlas, &run_b);
    assert_eq!(atlas.page_count(), 2);

    // Run A leaves; a new run needs a page, so pressure reclaims A's idle page.
    world.publish(
        &mut atlas,
        &[(run_b, BAND_32_HEIGHT), (run_c, BAND_32_HEIGHT)],
        &[],
    );
    world.populate(&mut atlas, 500);
    assert_eq!(atlas.take_retired_pages(), 1);
    let (_, b_stats) = world.draw(&atlas, &run_b);
    assert_eq!((b_stats.gui_rebuilds, b_stats.uploaded_bytes), (0, 0));

    // Run A returns: its entry misses, repopulates and only A rebuilds.
    world.publish(
        &mut atlas,
        &[
            (run_a, BAND_32_HEIGHT),
            (run_b, BAND_32_HEIGHT),
            (run_c, BAND_32_HEIGHT),
        ],
        &[],
    );
    assert_eq!(world.work.misses, 1);
    assert!(!world.draw(&atlas, &run_a).0);
    let (_, b_stats) = world.draw(&atlas, &run_b);
    assert_eq!((b_stats.gui_rebuilds, b_stats.uploaded_bytes), (0, 0));
    assert_eq!(device.borrow().deleted_pages.len(), 1);
}

#[test]
fn a_retired_page_rebuilds_its_runs_after_repopulation_and_only_then() {
    let (device, mut atlas, mut world) = setup();
    let [first, second] = [style(1), style(2)];
    let [glyphs_a, glyphs_b] = [glyphs(&[1]), glyphs(&[2])];
    let run_a = text_run(1, &first, &glyphs_a);
    let run_b = text_run(1, &second, &glyphs_b);
    let shown = [(run_a, BAND_32_HEIGHT), (run_b, BAND_32_HEIGHT)];

    world.publish(&mut atlas, &shown[..1], &[]);
    world.populate(&mut atlas, 500);
    world.publish(&mut atlas, &shown, &[]);
    world.populate(&mut atlas, 500);
    world.draw(&atlas, &run_a);
    world.draw(&atlas, &run_b);
    let page_a = atlas.get(&key(1, 32)).unwrap().page_index;

    // Retire A's page directly, as a pressure reclaim of a partially live page would.
    atlas.retire_page(page_a);
    world.publish(&mut atlas, &shown, &[]);
    assert_eq!(world.work.misses, 1);
    world.populate(&mut atlas, 500);
    let (drawn, a_stats) = world.draw(&atlas, &run_a);
    assert!(drawn);
    assert_eq!(a_stats.gui_rebuilds, 1, "A samples the retired page");
    let (_, b_stats) = world.draw(&atlas, &run_b);
    assert_eq!(b_stats.gui_rebuilds, 0, "B samples only a live page");

    // The rebuilt run is warm again.
    world.publish(&mut atlas, &shown, &[]);
    let (_, a_stats) = world.draw(&atlas, &run_a);
    assert_eq!((a_stats.gui_rebuilds, a_stats.uploaded_bytes), (0, 0));
    assert_eq!(device.borrow().updated_batches.len(), 1);
}

#[test]
fn pressure_reclaims_a_partially_live_page_when_no_page_is_idle() {
    let (_, mut atlas, mut world) = setup();
    let style = style(1);
    let all = glyphs(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let live = glyphs(&[0, 2, 4, 6]);

    // Two entries per page; afterwards only one entry on every page stays demanded.
    world.publish(
        &mut atlas,
        &[(text_run(1, &style, &all), BAND_32_HEIGHT)],
        &[],
    );
    let queue = world.work.take_queue();
    for key in &queue {
        atlas.allocate_slot(*key, 500, 240, INK).unwrap();
    }
    world.work.restore_queue(queue);
    world.publish(
        &mut atlas,
        &[(text_run(1, &style, &live), BAND_32_HEIGHT)],
        &[],
    );
    assert_eq!(atlas.page_count(), 4);
    let generation = atlas.residency_generation();

    assert!(atlas.allocate_slot(key(9, 32), 500, 240, INK).is_err());
    assert_eq!(
        atlas.page_count(),
        4,
        "no page is idle, so allocation fails"
    );

    // The next publication reclaims the page holding the most stale entries.
    world.publish(
        &mut atlas,
        &[(text_run(1, &style, &live), BAND_32_HEIGHT)],
        &[],
    );
    assert_eq!(atlas.page_count(), 3);
    assert_ne!(atlas.residency_generation(), generation);
    assert!(atlas.get(&key(0, 32)).is_none());
    assert!(atlas.get(&key(2, 32)).is_some());
    assert_eq!(
        world.work.misses, 1,
        "the live entry of the reclaimed page misses"
    );
    atlas.allocate_slot(key(9, 32), 500, 240, INK).unwrap();
    assert_eq!(atlas.page_count(), 4);
}

#[test]
fn lowered_page_budget_retires_pages_at_the_next_publication() {
    let (_, mut atlas, mut world) = setup();
    let style = style(1);
    let glyphs = glyphs(&[1, 2, 3]);
    let run = text_run(1, &style, &glyphs);

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    world.populate(&mut atlas, 500);
    assert_eq!(atlas.page_count(), 3);

    atlas.set_limits(GlyphAtlasLimits {
        max_pages: 1,
        idle_page_publications: 600,
    });
    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    assert_eq!(atlas.page_count(), 1);
    assert_eq!(atlas.take_retired_pages(), 2);
    assert_eq!(world.work.misses, 2);
    assert!(
        !world.draw(&atlas, &run).0,
        "text stays analytic while entries miss"
    );
}

#[test]
fn demand_is_shared_across_worlds_until_the_last_world_releases_it() {
    let (device, mut atlas, mut first) = setup();
    let mut second = TestWorld::new(&device);
    let style = style(1);
    let glyphs = glyphs(&[1]);
    let run = text_run(1, &style, &glyphs);

    first.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    first.populate(&mut atlas, 20);
    second.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    assert_eq!(second.work.misses, 0, "Worlds share font coverage");
    assert_eq!(atlas.demand.get(&key(1, 32)), Some(&2));

    first.cache.release_demand(&mut atlas);
    atlas.release_if_unused();
    assert_eq!(atlas.page_count(), 1);
    assert!(second.draw(&atlas, &run).0);

    second.cache.release_demand(&mut atlas);
    atlas.release_if_unused();
    assert_eq!(atlas.resident_bytes(), 0);
    assert_eq!(device.borrow().deleted_batches.len(), 1);
}

#[test]
fn page_interleaved_runs_draw_once_per_page() {
    let (device, mut atlas, mut world) = setup();
    let style = style(1);
    // Overlapping glyphs of one colour alternate between two pages.
    let mut glyphs = glyphs(&[1, 2, 1, 2, 1]);
    for glyph in &mut glyphs {
        glyph.position = [0.1, 0.1];
    }
    let run = text_run(1, &style, &glyphs);

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    world.populate(&mut atlas, 500);
    let pages = device.borrow().created_pages.clone();
    let (drawn, cold) = world.draw(&atlas, &run);
    assert!(drawn);
    assert_eq!(cold.gui_batches, 2, "{cold:?}");
    assert_eq!(device.borrow().sampled_pages, [pages[0], pages[1]]);

    let (_, warm) = world.draw(&atlas, &run);
    assert_eq!((warm.uploaded_bytes, warm.gui_batches), (0, 2));
    drop(world);
    assert_eq!(device.borrow().deleted_batches.len(), 2);
    drop(atlas);
    assert_eq!(device.borrow().deleted_pages.len(), 2);
}

#[test]
fn overlapping_glyphs_of_different_colours_keep_painter_order_across_pages() {
    let (device, mut atlas, mut world) = setup();
    let style = style(1);
    let mut glyphs = glyphs(&[1, 2, 1, 2]);
    for (index, glyph) in glyphs.iter_mut().enumerate() {
        glyph.color = Some([index as f32 * 0.25, 0.0, 0.0, 1.0]);
    }
    // The last glyph sits apart and may join its page's earlier batch.
    for glyph in &mut glyphs[..3] {
        glyph.position = [0.1, 0.1];
    }
    glyphs[3].position = [5.0, 0.1];
    let run = text_run(1, &style, &glyphs);

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    world.populate(&mut atlas, 500);
    let pages = device.borrow().created_pages.clone();
    world.draw(&atlas, &run);
    assert_eq!(
        device.borrow().sampled_pages,
        [pages[0], pages[1], pages[0]],
        "the third glyph paints over the second"
    );
    assert_eq!(
        device
            .borrow()
            .created_batches
            .iter()
            .map(|(_, vertices)| *vertices)
            .collect::<Vec<_>>(),
        [6, 12, 6]
    );
}

#[test]
fn clipped_glyphs_never_generate_vertices_and_long_runs_split_bounded_batches() {
    let (device, mut atlas, mut world) = setup();
    let style = style(1);
    let mut glyphs = vec![
        SurfaceGlyph {
            glyph_id: 1,
            position: [0.1; 2],
            color: None,
        };
        600
    ];
    glyphs.extend(vec![
        SurfaceGlyph {
            glyph_id: 1,
            position: [20.0; 2],
            color: None,
        };
        600
    ]);
    let run = text_run(1, &style, &glyphs);

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    world.populate(&mut atlas, 20);
    let (_, stats) = world.draw(&atlas, &run);
    assert_eq!(stats.gui_batches, 3);
    assert_eq!(stats.triangles, 1200);
    assert!(
        device
            .borrow()
            .created_batches
            .iter()
            .all(|(_, vertices)| *vertices <= 256 * 6)
    );
    assert_eq!(
        world.cache.resident_bytes(),
        600 * 6 * std::mem::size_of::<GlyphVertex>()
    );
}

#[test]
fn population_queue_is_bounded_per_frame_and_resumes_next_frame() {
    let (_, mut atlas, mut world) = setup();
    let style = style(1);
    let ids: Vec<u32> = (100..100 + MAX_POPULATES_PER_FRAME as u32 + 8).collect();
    let glyphs = glyphs(&ids);
    let run = text_run(1, &style, &glyphs);

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    assert_eq!(world.work.misses, ids.len() as u32);
    assert_eq!(world.populate(&mut atlas, 4), MAX_POPULATES_PER_FRAME);
    assert!(!world.draw(&atlas, &run).0);

    world.publish(&mut atlas, &[(run, BAND_32_HEIGHT)], &[]);
    assert_eq!(world.work.misses, 8);
    assert_eq!(world.populate(&mut atlas, 4), 8);
    assert!(world.draw(&atlas, &run).0);
}

#[test]
fn failed_population_backs_off_without_invalidating_retained_runs() {
    let (_, mut atlas, mut world) = setup();
    let style = style(1);
    let glyphs = glyphs(&[1, 2]);
    let run = text_run(1, &style, &glyphs);
    let shown = [(run, BAND_32_HEIGHT)];

    world.publish(&mut atlas, &shown, &[]);
    atlas.allocate_slot(key(1, 32), 20, 20, INK).unwrap();
    let generation = atlas.residency_generation();

    atlas.allocate_slot(key(2, 32), 20, 20, INK).unwrap();
    atlas.abandon_population(key(2, 32));
    assert!(atlas.get(&key(2, 32)).is_none());
    assert!(atlas.get(&key(1, 32)).is_some());
    assert_eq!(
        atlas.residency_generation(),
        generation,
        "populated runs keep their UVs"
    );

    // Publications until the glyph is queued for population again.
    let deferred_publications = |atlas: &mut Atlas, world: &mut TestWorld| {
        for publications in 1..=64 {
            world.publish(atlas, &shown, &[]);
            let queue = world.work.take_queue();
            let queued = queue == [key(2, 32)];
            world.work.restore_queue(queue);
            if queued {
                return publications;
            }

            assert!(atlas.population_deferred(&key(2, 32)));
        }
        panic!("backed-off glyph never retried");
    };
    assert_eq!(
        deferred_publications(&mut atlas, &mut world),
        POPULATE_RETRY_TICKS
    );

    // A repeated failure doubles the wait instead of retrying every frame.
    atlas.abandon_population(key(2, 32));
    assert_eq!(
        deferred_publications(&mut atlas, &mut world),
        2 * POPULATE_RETRY_TICKS
    );

    // Losing demand clears the back-off state.
    atlas.abandon_population(key(2, 32));
    let single = glyphs[..1].to_vec();
    world.publish(
        &mut atlas,
        &[(text_run(1, &style, &single), BAND_32_HEIGHT)],
        &[],
    );
    assert!(!atlas.population_deferred(&key(2, 32)));
    assert!(atlas.population_backoff.is_empty());
    assert_eq!(atlas.residency_generation(), generation);
}

#[test]
fn context_loss_keeps_slots_and_bands_for_identical_recovered_coverage() {
    let (device, mut atlas, mut world) = setup();
    let [earlier, text] = [style(1), style(2)];
    let [glyphs_earlier, glyphs_text] = [glyphs(&[9]), glyphs(&[1])];
    let earlier_run = text_run(1, &earlier, &glyphs_earlier);
    let run = text_run(1, &text, &glyphs_text);

    // History: an entry that later loses demand occupies the first slot, and camera
    // motion keeps band 24 through hysteresis where a fresh run would select band 32.
    world.publish(&mut atlas, &[(earlier_run, 26.0)], &[]);
    world.populate(&mut atlas, 20);
    world.publish(&mut atlas, &[(run, 26.0)], &[]);
    world.populate(&mut atlas, 20);
    world.publish(&mut atlas, &[(run, 28.5)], &[]);
    assert!(world.draw(&atlas, &run).0);
    let before = *atlas.get(&key(1, 24)).unwrap();
    let deleted_batches = device.borrow().deleted_batches.len();

    world.cache.release_context();
    atlas.release_context();
    assert_eq!(atlas.page_count(), 0);
    assert!(atlas.get(&key(1, 24)).is_none());
    assert_eq!(device.borrow().deleted_pages.len(), 1);
    assert_eq!(device.borrow().deleted_batches.len(), deleted_batches + 1);

    // Recovery repopulates the demanded entry into its original slot at the same band.
    world.publish(&mut atlas, &[(run, 28.5)], &[]);
    assert_eq!(world.work.misses, 1);
    assert_eq!(world.populate(&mut atlas, 20), 1);
    assert_eq!(*atlas.get(&key(1, 24)).unwrap(), before);
    assert!(
        atlas.get(&key(9, 24)).is_none(),
        "entries without demand stay empty"
    );
    assert_eq!(device.borrow().created_pages.len(), 2);
    let (drawn, recovered) = world.draw(&atlas, &run);
    assert!(drawn);
    assert_eq!(recovered.gui_rebuilds, 1);
}

#[test]
fn projected_glyph_quality_accounts_for_perspective_and_rotation() {
    use super::projected_glyph_height;
    let mut m = [0.0; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 4.0;
    assert_eq!(projected_glyph_height(&m, [0.0; 2], 0.2, (800, 600)), 15.0);
    m[15] = 2.0;
    assert_eq!(projected_glyph_height(&m, [0.0; 2], 0.2, (800, 600)), 30.0);
    m[5] = 0.0;
    m[4] = 1.0;
    assert_eq!(projected_glyph_height(&m, [0.0; 2], 0.2, (800, 600)), 40.0);
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

/// CPU reference for the analytic coverage the atlas rasterizer evaluates on the GPU:
/// exact subpixel quadratic curve coverage at `sample`.
fn evaluate_glyph_coverage(
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
            let roots = solve_horizontal(p12, p3, a, b, line);
            let r = [roots[0] * pixels_per_unit[0], roots[1] * pixels_per_unit[0]];
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
            let roots = solve_vertical(p12, p3, a, b, line);
            let r = [roots[0] * pixels_per_unit[1], roots[1] * pixels_per_unit[1]];
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
