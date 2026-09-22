//! Tests for glyph coverage atlas packing, resolution band selection,
//! CPU coverage evaluation, and retained text batch caching.

use std::cell::RefCell;
use std::rc::Rc;

use super::{
    ATLAS_PAGE_SIZE, GlyphAtlas, GlyphBatchRenderCache, GlyphKey, GlyphVertex, MAX_ATLAS_PAGES,
    RESOLUTION_BANDS, evaluate_glyph_coverage, select_resolution_band,
};
use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::services::asset_management::AssetKey;
use ipp_core::systems::surface::{
    SurfaceClipRect, SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
};

#[derive(Default)]
struct MockAtlasDevice {
    created_batches: Vec<(usize, usize)>,
    updated_batches: Vec<(usize, usize)>,
    deleted_batches: Vec<usize>,
    draws: Vec<(usize, [f32; 4])>,
    created_pages: Vec<u32>,
    deleted_pages: Vec<u32>,
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
        _atlas: &Self::Texture,
        _mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.draws.push((batch.id, *clip));
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

    fn glyph_atlas_texture<'a>(&'a self, page: &'a Self::GlyphAtlasPage) -> &'a Self::Texture {
        page
    }
}

#[test]
fn resolution_band_selection_selects_expected_band_or_none_for_extremes() {
    // Extreme scales below 10.0 px or above 80.0 px return None to fall back to analytic curves.
    assert_eq!(select_resolution_band(5.0), None);
    assert_eq!(select_resolution_band(9.9), None);
    assert_eq!(select_resolution_band(80.1), None);
    assert_eq!(select_resolution_band(120.0), None);

    // Stable discrete resolution bands: 16, 24, 32, 48, 64.
    assert_eq!(select_resolution_band(12.0), Some(16));
    assert_eq!(select_resolution_band(16.0), Some(16));
    assert_eq!(select_resolution_band(20.0), Some(24));
    assert_eq!(select_resolution_band(24.0), Some(24));
    assert_eq!(select_resolution_band(28.0), Some(32));
    assert_eq!(select_resolution_band(32.0), Some(32));
    assert_eq!(select_resolution_band(40.0), Some(48));
    assert_eq!(select_resolution_band(48.0), Some(48));
    assert_eq!(select_resolution_band(56.0), Some(64));
    assert_eq!(select_resolution_band(64.0), Some(64));
    assert_eq!(select_resolution_band(75.0), Some(64));

    for &band in &RESOLUTION_BANDS {
        assert_eq!(select_resolution_band(band as f32), Some(band));
    }
}

#[test]
fn atlas_page_shelf_packing_allocates_slots_and_wraps_rows() {
    let device = Rc::new(RefCell::new(MockAtlasDevice::default()));
    let mut atlas = GlyphAtlas::new(device.clone());

    let font_key = AssetKey {
        slot: 1,
        generation: 1,
    };

    // Allocate first glyph: 30x20 pixels
    let key1 = GlyphKey {
        font_key,
        glyph_id: 65,
        resolution_band: 24,
    };
    let (pos1, page1, entry1) = atlas
        .allocate_slot(key1, 30, 20, [0.0, -100.0, 300.0, 100.0])
        .unwrap();

    assert_eq!(page1, 0);
    // 1-pixel border pad: slot_x is 0, content_x is 1
    assert_eq!(pos1, [1, 1]);
    assert_eq!(entry1.pixel_size, [30, 20]);
    assert_eq!(atlas.page_count(), 1);

    // Allocate second glyph in same row: 20x20
    let key2 = GlyphKey {
        font_key,
        glyph_id: 66,
        resolution_band: 24,
    };
    let (pos2, page2, entry2) = atlas
        .allocate_slot(key2, 20, 20, [0.0, -100.0, 200.0, 100.0])
        .unwrap();

    assert_eq!(page2, 0);
    // slot_w of previous was 30 + 2 = 32, so slot_x = 32, content_x = 33
    assert_eq!(pos2, [33, 1]);
    assert_eq!(entry2.pixel_size, [20, 20]);
}

#[test]
fn glyph_atlas_multi_page_allocation_and_capacity_limit() {
    let device = Rc::new(RefCell::new(MockAtlasDevice::default()));
    let mut atlas = GlyphAtlas::new(device.clone());

    let font_key = AssetKey {
        slot: 2,
        generation: 1,
    };

    // Allocate large glyphs that consume pages: 500x500 pixels (padded to 502x502)
    for i in 0..MAX_ATLAS_PAGES {
        let key = GlyphKey {
            font_key,
            glyph_id: i as u32,
            resolution_band: 64,
        };
        let (_, page_idx, _) = atlas
            .allocate_slot(key, 500, 500, [0.0, 0.0, 1000.0, 1000.0])
            .unwrap();
        assert_eq!(page_idx, i);
    }

    assert_eq!(atlas.page_count(), MAX_ATLAS_PAGES as u32);
    assert_eq!(
        atlas.resident_bytes(),
        MAX_ATLAS_PAGES * (ATLAS_PAGE_SIZE as usize * ATLAS_PAGE_SIZE as usize * 4)
    );

    // Next allocation exceeding capacity should fail
    let overflow_key = GlyphKey {
        font_key,
        glyph_id: 999,
        resolution_band: 64,
    };
    let result = atlas.allocate_slot(overflow_key, 500, 500, [0.0, 0.0, 1000.0, 1000.0]);
    assert!(result.is_err());

    // Clear releases all pages
    atlas.clear();
    assert_eq!(atlas.page_count(), 0);
    assert_eq!(atlas.resident_bytes(), 0);
    assert_eq!(device.borrow().deleted_pages.len(), MAX_ATLAS_PAGES);
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
fn retained_glyph_batch_cache_warm_reuse_and_zero_uploads() {
    let device = Rc::new(RefCell::new(MockAtlasDevice::default()));
    let mut atlas = GlyphAtlas::new(device.clone());
    let mut cache = GlyphBatchRenderCache::new(device.clone());

    let font_key = AssetKey {
        slot: 1,
        generation: 1,
    };

    // Pre-populate glyph 10 in atlas
    let glyph_key = GlyphKey {
        font_key,
        glyph_id: 10,
        resolution_band: 32,
    };
    atlas
        .allocate_slot(glyph_key, 20, 20, [0.0, -800.0, 600.0, 200.0])
        .unwrap();

    let entity = ipp_core::EntityId::from_bits(42);
    let clip: SurfaceClipRect = [0.0, 0.0, 10.0, 10.0];
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Authored(ipp_core::SurfaceItemId(1)),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: Some(clip),
    };
    let glyphs = vec![SurfaceGlyph {
        glyph_id: 10,
        position: [0.1, 0.1],
        color: None,
    }];
    let mvp = [1.0; 16];
    let program = 1;

    // Frame 1: Initial creation
    let mut stats1 = RenderStats::default();
    cache
        .draw_text_run(
            &program,
            &atlas,
            entity,
            clip,
            &style,
            font_key,
            0.05,
            1000,
            &glyphs,
            32,
            &mvp,
            &mut stats1,
        )
        .unwrap();

    assert_eq!(stats1.gui_rebuilds, 1);
    assert_eq!(stats1.gui_batches, 1);
    assert!(stats1.uploaded_bytes > 0);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().updated_batches.len(), 0);
    assert_eq!(device.borrow().draws.len(), 1);

    // Frame 2: Identical text run (warm frame)
    let mut stats2 = RenderStats::default();
    cache
        .draw_text_run(
            &program,
            &atlas,
            entity,
            clip,
            &style,
            font_key,
            0.05,
            1000,
            &glyphs,
            32,
            &mvp,
            &mut stats2,
        )
        .unwrap();

    // 0 uploads on warm frame!
    assert_eq!(stats2.gui_rebuilds, 0);
    assert_eq!(stats2.gui_allocations, 0);
    assert_eq!(stats2.uploaded_bytes, 0);
    assert_eq!(stats2.gui_batches, 1);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().updated_batches.len(), 0);
    assert_eq!(device.borrow().draws.len(), 2);
}

#[test]
fn retained_glyph_batch_cache_rebuilds_on_text_edits() {
    let device = Rc::new(RefCell::new(MockAtlasDevice::default()));
    let mut atlas = GlyphAtlas::new(device.clone());
    let mut cache = GlyphBatchRenderCache::new(device.clone());

    let font_key = AssetKey {
        slot: 1,
        generation: 1,
    };

    // Pre-populate glyph 10 and 20 in atlas
    for gid in [10, 20] {
        let glyph_key = GlyphKey {
            font_key,
            glyph_id: gid,
            resolution_band: 32,
        };
        atlas
            .allocate_slot(glyph_key, 20, 20, [0.0, -800.0, 600.0, 200.0])
            .unwrap();
    }

    let entity = ipp_core::EntityId::from_bits(42);
    let clip: SurfaceClipRect = [0.0, 0.0, 10.0, 10.0];
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Authored(ipp_core::SurfaceItemId(1)),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: Some(clip),
    };
    let glyphs = vec![SurfaceGlyph {
        glyph_id: 10,
        position: [0.1, 0.1],
        color: None,
    }];
    let mvp = [1.0; 16];
    let program = 1;

    let mut stats1 = RenderStats::default();
    cache
        .draw_text_run(
            &program,
            &atlas,
            entity,
            clip,
            &style,
            font_key,
            0.05,
            1000,
            &glyphs,
            32,
            &mvp,
            &mut stats1,
        )
        .unwrap();

    // Edit text run: append glyph 20
    let edited_glyphs = vec![
        SurfaceGlyph {
            glyph_id: 10,
            position: [0.1, 0.1],
            color: None,
        },
        SurfaceGlyph {
            glyph_id: 20,
            position: [0.2, 0.1],
            color: None,
        },
    ];

    let mut stats2 = RenderStats::default();
    cache
        .draw_text_run(
            &program,
            &atlas,
            entity,
            clip,
            &style,
            font_key,
            0.05,
            1000,
            &edited_glyphs,
            32,
            &mvp,
            &mut stats2,
        )
        .unwrap();

    // Rebuild occurred and replaced batch storage
    assert_eq!(stats2.gui_rebuilds, 1);
    assert_eq!(device.borrow().created_batches.len(), 1);
    assert_eq!(device.borrow().updated_batches.len(), 1);
    // Updated batch has 2 glyphs * 6 vertices = 12 vertices
    assert_eq!(device.borrow().updated_batches[0].1, 12);
}

#[test]
fn whitespace_glyphs_omitted_from_batch_quads() {
    let device = Rc::new(RefCell::new(MockAtlasDevice::default()));
    let mut atlas = GlyphAtlas::new(device.clone());
    let mut cache = GlyphBatchRenderCache::new(device.clone());

    let font_key = AssetKey {
        slot: 1,
        generation: 1,
    };

    // Glyph 32 is space (empty font bounds [0, 0, 0, 0])
    atlas
        .allocate_slot(
            GlyphKey {
                font_key,
                glyph_id: 32,
                resolution_band: 32,
            },
            0,
            0,
            [0.0, 0.0, 0.0, 0.0],
        )
        .unwrap();

    // Glyph 65 is 'A' (non-empty font bounds)
    atlas
        .allocate_slot(
            GlyphKey {
                font_key,
                glyph_id: 65,
                resolution_band: 32,
            },
            20,
            20,
            [0.0, -800.0, 600.0, 200.0],
        )
        .unwrap();

    let entity = ipp_core::EntityId::from_bits(42);
    let clip: SurfaceClipRect = [0.0, 0.0, 10.0, 10.0];
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Authored(ipp_core::SurfaceItemId(1)),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: Some(clip),
    };
    // Text: "A A" (space between two 'A's)
    let glyphs = vec![
        SurfaceGlyph {
            glyph_id: 65,
            position: [0.0, 0.0],
            color: None,
        },
        SurfaceGlyph {
            glyph_id: 32,
            position: [0.1, 0.0],
            color: None,
        },
        SurfaceGlyph {
            glyph_id: 65,
            position: [0.2, 0.0],
            color: None,
        },
    ];
    let mvp = [1.0; 16];
    let program = 1;

    let mut stats = RenderStats::default();
    cache
        .draw_text_run(
            &program, &atlas, entity, clip, &style, font_key, 0.05, 1000, &glyphs, 32, &mvp,
            &mut stats,
        )
        .unwrap();

    // Only 2 visible glyphs emitted quads (2 * 6 = 12 vertices), space was omitted
    assert_eq!(device.borrow().created_batches[0].1, 12);
}

#[test]
fn retained_text_batches_pruned_on_finish_frame() {
    let device = Rc::new(RefCell::new(MockAtlasDevice::default()));
    let mut atlas = GlyphAtlas::new(device.clone());
    let mut cache = GlyphBatchRenderCache::new(device.clone());

    let font_key = AssetKey {
        slot: 1,
        generation: 1,
    };

    atlas
        .allocate_slot(
            GlyphKey {
                font_key,
                glyph_id: 1,
                resolution_band: 32,
            },
            20,
            20,
            [0.0, -800.0, 600.0, 200.0],
        )
        .unwrap();

    let entity = ipp_core::EntityId::from_bits(42);
    let clip: SurfaceClipRect = [0.0, 0.0, 10.0, 10.0];
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Authored(ipp_core::SurfaceItemId(1)),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: Some(clip),
    };
    let glyphs = vec![SurfaceGlyph {
        glyph_id: 1,
        position: [0.0, 0.0],
        color: None,
    }];
    let mvp = [1.0; 16];
    let program = 1;

    // Frame 1: draw run
    let mut stats1 = RenderStats::default();
    cache
        .draw_text_run(
            &program,
            &atlas,
            entity,
            clip,
            &style,
            font_key,
            0.05,
            1000,
            &glyphs,
            32,
            &mvp,
            &mut stats1,
        )
        .unwrap();

    assert_eq!(
        cache.resident_bytes(),
        6 * std::mem::size_of::<GlyphVertex>()
    );
    cache.finish_frame();

    // Frame 2: text run is NOT drawn
    cache.finish_frame();

    // Unreferenced run is pruned and deleted on GPU
    assert_eq!(cache.resident_bytes(), 0);
    assert_eq!(device.borrow().deleted_batches.len(), 1);
}
