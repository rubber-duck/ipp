//! Tests for retained analytic glyph instance streams.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use super::{ANALYTIC_INSTANCE_BYTES, AnalyticGlyphCache, AnalyticGlyphRun};
use crate::services::render::device::{SurfacePathDescriptor, SurfacePathInstance};
use crate::services::render::retained_surfaces::{RetainedSurfaceSubmission, SurfacePaint};
use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::services::asset_management::AssetKey;
use ipp_core::systems::surface::{SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle};
use ipp_core::{EntityId, SurfaceItemId};

/// Counts stream allocations, replacements, releases and instanced draws.
#[derive(Default)]
struct MockDevice {
    created: u32,
    updated: u32,
    deleted: u32,
    live: i32,
    /// Instance count of every draw.
    draws: Vec<usize>,
    fail_update: bool,
}

impl RenderDevice for MockDevice {
    type Program = ();
    type Mesh = ();
    type Texture = ();
    type SurfacePath = ();
    type SurfaceCacheTarget = ();
    type SurfaceInstances = usize;
    #[cfg(feature = "shadows")]
    type ShadowMap = ();
    #[cfg(feature = "gui")]
    type GuiBatch = ();
    #[cfg(feature = "gui")]
    type GlyphBatch = ();
    #[cfg(feature = "gui")]
    type GlyphAtlasPage = ();

    #[cfg(feature = "gui")]
    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture {
        page
    }

    fn set_lighting(
        &mut self,
        _: &(),
        _: &[f32; 16],
        _: &[f32; 16],
        _: &[f32; 3],
        _: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn create_shadow_map(&mut self, _: u32) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn begin_shadow(&mut self, _: &(), _: u32, _: u32) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn end_shadow(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn bind_shadow(
        &mut self,
        _: &(),
        _: &(),
        _: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn delete_shadow_map(&mut self, _: ()) {}

    fn create_surface_instances(
        &mut self,
        _: &(),
        instances: &[SurfacePathInstance],
    ) -> Result<usize, RenderError> {
        self.created += 1;
        self.live += 1;
        Ok(instances.len())
    }

    fn update_surface_instances(
        &mut self,
        stream: &mut usize,
        _: &(),
        instances: &[SurfacePathInstance],
    ) -> Result<(), RenderError> {
        self.updated += 1;
        if self.fail_update {
            return Err(RenderError::RenderDevice("injected update failure".into()));
        }

        *stream = instances.len();
        Ok(())
    }

    fn delete_surface_instances(&mut self, _: usize) {
        self.deleted += 1;
        self.live -= 1;
    }

    fn draw_surface_instances(
        &mut self,
        _: &(),
        _: &(),
        stream: &usize,
        _: &[f32; 16],
        _: &[f32; 4],
        _: u32,
    ) -> Result<(), RenderError> {
        self.draws.push(*stream);
        Ok(())
    }

    fn create_program(&mut self, _: &str, _: &str) -> Result<(), RenderError> {
        Ok(())
    }

    fn create_mesh(&mut self, _: &ipp_core::MeshAsset) -> Result<(), RenderError> {
        Ok(())
    }

    fn create_texture(&mut self, _: u32, _: u32, _: &[u8]) -> Result<(), RenderError> {
        Ok(())
    }

    fn allocate_texture(&mut self, _: u32, _: u32) -> Result<(), RenderError> {
        Ok(())
    }

    fn upload_texture_rows(
        &mut self,
        _: &(),
        _: u32,
        _: u32,
        _: u32,
        _: &[u8],
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn begin_frame(&mut self, _: u32, _: u32, _: &[f32; 4]) -> Result<(), RenderError> {
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        _: &(),
        _: &(),
        _: &[f32; 16],
        _: &[f32; 3],
        #[cfg(feature = "mesh-poses")] _: Option<(&(), f32)>,
        _: Option<&()>,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn delete_mesh(&mut self, _: ()) {}
    fn delete_texture(&mut self, _: ()) {}
    fn delete_program(&mut self, _: ()) {}
}

const FONT: AssetKey = AssetKey {
    slot: 3,
    generation: 1,
};

fn style(id: u32, x: f32) -> SurfacePrimitiveStyle {
    SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Authored(SurfaceItemId(id)),
        position: [x, 0.1],
        scale: [1.0, 1.0],
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
            position: [0.1 * index as f32, 0.0],
            color: None,
        })
        .collect()
}

fn run<'a>(
    entity: u64,
    style: &'a SurfacePrimitiveStyle,
    glyphs: &'a [SurfaceGlyph],
) -> AnalyticGlyphRun<'a> {
    AnalyticGlyphRun {
        entity: EntityId::from_bits(entity),
        style,
        clip: [0.0, 0.0, 10.0, 10.0],
        font_key: FONT,
        font_size: 0.1,
        glyphs,
    }
}

type Cache = AnalyticGlyphCache<MockDevice>;

/// Draw `run` with one instance per glyph; returns the frame's stats.
fn draw(cache: &mut Cache, run: &AnalyticGlyphRun<'_>, paint: SurfacePaint) -> RenderStats {
    let mut stats = RenderStats::default();
    let mut scratch = Vec::new();
    let glyphs = run.glyphs;
    cache
        .draw_run(
            &(),
            &(),
            run,
            paint,
            &[0.0; 16],
            &mut scratch,
            |instances| {
                instances.extend(glyphs.iter().map(|glyph| SurfacePathInstance {
                    bounds: [0.0, 0.0, 1.0, 1.0],
                    placement: [glyph.position[0], glyph.position[1], 1.0, 1.0],
                    color: [1.0; 4],
                    descriptor: SurfacePathDescriptor::new([glyph.glyph_id, 1], 0),
                }))
            },
            &mut stats,
        )
        .unwrap();
    stats
}

fn submitted(cache: &mut Cache, live: &[u64], submitted: &[u64]) {
    let live: BTreeSet<_> = live.iter().copied().map(EntityId::from_bits).collect();
    let submitted: BTreeSet<_> = submitted.iter().copied().map(EntityId::from_bits).collect();
    cache.finish_frame(Some(&RetainedSurfaceSubmission {
        live: &live,
        submitted: &submitted,
    }));
}

#[test]
fn unchanged_runs_keep_their_stream_and_upload_nothing() {
    let device = Rc::new(RefCell::new(MockDevice::default()));
    let mut cache = Cache::new(device.clone());
    let style = style(1, 0.1);
    let text = glyphs(&[1, 2, 3, 4]);

    let cold = draw(&mut cache, &run(1, &style, &text), SurfacePaint::UNKNOWN);
    assert_eq!(cold.uploaded_bytes as usize, 4 * ANALYTIC_INSTANCE_BYTES);
    assert_eq!((cold.draw_calls, cold.triangles), (1, 8));
    submitted(&mut cache, &[1], &[1]);

    for _ in 0..3 {
        let idle = draw(&mut cache, &run(1, &style, &text), SurfacePaint::UNKNOWN);
        assert_eq!(idle.uploaded_bytes, 0);
        assert_eq!(idle.draw_calls, 1);
        submitted(&mut cache, &[1], &[1]);
    }

    let device = device.borrow();
    assert_eq!((device.created, device.updated), (1, 0));
    assert_eq!(device.draws, [4, 4, 4, 4]);
    assert_eq!(cache.resident_bytes(), 4 * ANALYTIC_INSTANCE_BYTES);
}

#[test]
fn edited_runs_replace_their_stream_in_place() {
    let device = Rc::new(RefCell::new(MockDevice::default()));
    let mut cache = Cache::new(device.clone());
    let first = style(1, 0.1);
    let moved = style(1, 0.2);
    let text = glyphs(&[1, 2]);
    let longer = glyphs(&[1, 2, 3]);

    draw(&mut cache, &run(1, &first, &text), SurfacePaint::UNKNOWN);
    let edited = draw(&mut cache, &run(1, &moved, &text), SurfacePaint::UNKNOWN);
    assert_eq!(edited.uploaded_bytes as usize, 2 * ANALYTIC_INSTANCE_BYTES);
    let typed = draw(&mut cache, &run(1, &moved, &longer), SurfacePaint::UNKNOWN);
    assert_eq!(typed.uploaded_bytes as usize, 3 * ANALYTIC_INSTANCE_BYTES);

    let device = device.borrow();
    assert_eq!((device.created, device.updated, device.live), (1, 2, 1));
    assert_eq!(device.draws, [2, 2, 3]);
    assert_eq!(cache.resident_bytes(), 3 * ANALYTIC_INSTANCE_BYTES);
}

#[test]
fn reusable_paint_revisions_skip_hashing_until_the_revision_changes() {
    let device = Rc::new(RefCell::new(MockDevice::default()));
    let mut cache = Cache::new(device.clone());
    let first = style(1, 0.1);
    let moved = style(1, 0.4);
    let text = glyphs(&[1, 2]);
    let paint = |revision, reusable| SurfacePaint {
        revision,
        reusable,
    };

    draw(&mut cache, &run(1, &first, &text), paint(7, false));

    // A reusable revision promises unchanged inputs, so they are not hashed.
    let reused = draw(&mut cache, &run(1, &moved, &text), paint(7, true));
    assert_eq!(reused.uploaded_bytes, 0);

    // A new revision hashes the run and replaces the stream.
    let replaced = draw(&mut cache, &run(1, &moved, &text), paint(8, false));
    assert_eq!(
        replaced.uploaded_bytes as usize,
        2 * ANALYTIC_INSTANCE_BYTES
    );
    assert_eq!(device.borrow().updated, 1);
}

#[test]
fn runs_without_visible_instances_hold_no_stream() {
    let device = Rc::new(RefCell::new(MockDevice::default()));
    let mut cache = Cache::new(device.clone());
    let style = style(1, 0.1);
    let text = glyphs(&[1, 2]);
    let empty = glyphs(&[]);

    draw(&mut cache, &run(1, &style, &text), SurfacePaint::UNKNOWN);
    let cleared = draw(&mut cache, &run(1, &style, &empty), SurfacePaint::UNKNOWN);
    assert_eq!((cleared.uploaded_bytes, cleared.draw_calls), (0, 0));
    assert_eq!(device.borrow().live, 0);
    assert_eq!(cache.resident_bytes(), 0);

    // An unchanged empty run neither rebuilds nor draws.
    let idle = draw(&mut cache, &run(1, &style, &empty), SurfacePaint::UNKNOWN);
    assert_eq!((idle.uploaded_bytes, idle.draw_calls), (0, 0));
    assert_eq!(device.borrow().created, 1);
}

#[test]
fn completed_frames_release_streams_of_removed_runs_and_surfaces_only() {
    let device = Rc::new(RefCell::new(MockDevice::default()));
    let mut cache = Cache::new(device.clone());
    let (a, b, c) = (style(1, 0.1), style(2, 0.1), style(1, 0.1));
    let text = glyphs(&[1, 2]);

    // Surface 1 draws runs a and b; Surface 2 draws run c.
    draw(&mut cache, &run(1, &a, &text), SurfacePaint::UNKNOWN);
    draw(&mut cache, &run(1, &b, &text), SurfacePaint::UNKNOWN);
    draw(&mut cache, &run(2, &c, &text), SurfacePaint::UNKNOWN);
    submitted(&mut cache, &[1, 2], &[1, 2]);
    assert_eq!(device.borrow().live, 3);

    // Surface 2 is culled or reused from a cache image: it keeps its stream, while
    // submitted Surface 1 releases run b, which it no longer draws analytically.
    draw(&mut cache, &run(1, &a, &text), SurfacePaint::UNKNOWN);
    submitted(&mut cache, &[1, 2], &[1]);
    assert_eq!(device.borrow().live, 2);
    assert_eq!(cache.resident_bytes(), 2 * 2 * ANALYTIC_INSTANCE_BYTES);

    // Incomplete frames keep everything.
    cache.finish_frame(None);
    assert_eq!(device.borrow().live, 2);

    // A destroyed Surface releases its streams.
    draw(&mut cache, &run(1, &a, &text), SurfacePaint::UNKNOWN);
    submitted(&mut cache, &[1], &[1]);
    assert_eq!(device.borrow().live, 1);

    cache.clear();
    assert_eq!((device.borrow().live, cache.resident_bytes()), (0, 0));
}

#[test]
fn failed_replacement_releases_the_stream_instead_of_drawing_stale_instances() {
    let device = Rc::new(RefCell::new(MockDevice::default()));
    let mut cache = Cache::new(device.clone());
    let first = style(1, 0.1);
    let moved = style(1, 0.3);
    let text = glyphs(&[1, 2]);

    draw(&mut cache, &run(1, &first, &text), SurfacePaint::UNKNOWN);
    device.borrow_mut().fail_update = true;
    let mut stats = RenderStats::default();
    let failed = cache.draw_run(
        &(),
        &(),
        &run(1, &moved, &text),
        SurfacePaint::UNKNOWN,
        &[0.0; 16],
        &mut Vec::new(),
        |instances| {
            instances.push(SurfacePathInstance {
                bounds: [0.0; 4],
                placement: [0.0; 4],
                color: [1.0; 4],
                descriptor: SurfacePathDescriptor::new([0, 1], 0),
            })
        },
        &mut stats,
    );
    assert!(failed.is_err());
    assert_eq!((device.borrow().live, device.borrow().draws.len()), (0, 1));
    assert_eq!(cache.resident_bytes(), 0);

    // The next frame recreates the stream.
    device.borrow_mut().fail_update = false;
    draw(&mut cache, &run(1, &moved, &text), SurfacePaint::UNKNOWN);
    assert_eq!((device.borrow().created, device.borrow().live), (2, 1));
}
