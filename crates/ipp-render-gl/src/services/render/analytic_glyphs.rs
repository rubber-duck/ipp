//! Retained instance streams for analytic Surface glyph runs.
//!
//! Text that the glyph atlas cannot serve (without the GUI capability, below or
//! above the atlas band range, or while its entries are still populating) draws
//! each glyph from its font's quadratic curves as one instance. RenderService keeps
//! each run's packed instances resident between frames, keyed by World, Surface
//! entity and primitive identity, and replaces them only when the run's inputs
//! change. Unchanged runs upload nothing; with a reusable Surface paint revision
//! they are not even hashed.
//!
//! Streams follow the retained Surface lifecycle in [`super::retained_surfaces`]:
//! a completed frame releases the streams of destroyed Surfaces and those a
//! submitted Surface no longer draws analytically, culled or cache-reused Surfaces
//! keep theirs, and context loss, unload and forgetting the World release all.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use super::device::SurfacePathInstance;
use super::retained_surfaces::{RetainedSurfaceSubmission, SurfacePaint};
use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::EntityId;
use ipp_core::services::asset_management::AssetKey;
use ipp_core::systems::surface::{
    SurfaceClipRect, SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
};

/// Bytes the device packs per analytic instance: sixteen `f32` lanes.
pub const ANALYTIC_INSTANCE_BYTES: usize = 16 * std::mem::size_of::<f32>();

/// Evaluated inputs of one analytic glyph run.
#[derive(Clone, Copy, Debug)]
pub struct AnalyticGlyphRun<'a> {
    /// Live entity owning the Surface component.
    pub entity: EntityId,
    /// Evaluated style carrying the stable primitive identity.
    pub style: &'a SurfacePrimitiveStyle,
    /// Effective clip rectangle in Surface metres.
    pub clip: SurfaceClipRect,
    /// Ready font asset incarnation; its curves and bounds are immutable.
    pub font_key: AssetKey,
    /// Metres per em.
    pub font_size: f32,
    /// Positioned glyphs in painter order.
    pub glyphs: &'a [SurfaceGlyph],
}

impl AnalyticGlyphRun<'_> {
    /// Hash every input that shapes the run's instances.
    fn hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(self.font_key.to_u64());
        hasher.write_u32(self.font_size.to_bits());
        for lane in self
            .clip
            .iter()
            .chain(&self.style.position)
            .chain(&self.style.scale)
            .chain(&self.style.color)
        {
            hasher.write_u32(lane.to_bits());
        }
        hasher.write_u32(self.style.opacity.to_bits());

        for glyph in self.glyphs {
            hasher.write_u32(glyph.glyph_id);
            hasher.write_u32(glyph.position[0].to_bits());
            hasher.write_u32(glyph.position[1].to_bits());
            glyph
                .color
                .map(|color| color.map(f32::to_bits))
                .hash(&mut hasher);
        }

        hasher.finish()
    }
}

/// One run's retained instance stream.
struct RetainedAnalyticRun<D: RenderDevice> {
    /// `None` while the run has no visible instance.
    stream: Option<D::SurfaceInstances>,
    hash: u64,
    /// Surface paint revision `hash` was computed under; zero when unknown.
    revision: u64,
    instances: u32,
    bytes: usize,
    /// Frame that last drew the run analytically.
    seen: u64,
}

/// Retained analytic glyph instance streams of one World.
pub struct AnalyticGlyphCache<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    runs: BTreeMap<(EntityId, SurfacePrimitiveIdentity), RetainedAnalyticRun<D>>,
    /// Sum of `bytes` over every retained stream.
    resident: usize,
    frame: u64,
}

impl<D: RenderDevice> AnalyticGlyphCache<D> {
    /// Create an empty cache bound to `device`.
    pub fn new(device: Rc<RefCell<D>>) -> Self {
        Self {
            device,
            runs: BTreeMap::new(),
            resident: 0,
            frame: 0,
        }
    }

    /// Resident bytes of every retained instance stream.
    pub fn resident_bytes(&self) -> usize {
        self.resident
    }

    /// Release every retained stream.
    pub fn clear(&mut self) {
        let mut device = self.device.borrow_mut();
        for (_, run) in std::mem::take(&mut self.runs) {
            if let Some(stream) = run.stream {
                device.delete_surface_instances(stream);
            }
        }
        self.resident = 0;
    }

    /// Draw one run from its retained stream, replacing the stream first when the
    /// run changed. `build` packs the run's visible instances into `scratch`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_run(
        &mut self,
        program: &D::Program,
        path: &D::SurfacePath,
        run: &AnalyticGlyphRun<'_>,
        paint: SurfacePaint,
        mvp: &[f32; 16],
        scratch: &mut Vec<SurfacePathInstance>,
        build: impl FnOnce(&mut Vec<SurfacePathInstance>),
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        let key = (run.entity, run.style.identity);
        let current = self.runs.get(&key).and_then(|retained| {
            if paint.reuses(retained.revision) {
                return Some(retained.hash);
            }

            let hash = run.hash();
            (retained.hash == hash).then_some(hash)
        });
        let hash = match current {
            Some(hash) => hash,
            None => {
                let hash = run.hash();
                self.replace(key, path, hash, scratch, build, stats)?;
                hash
            }
        };

        let retained = self.runs.get_mut(&key).expect("retained analytic run");
        retained.hash = hash;
        retained.revision = paint.revision;
        retained.seen = self.frame;

        let Some(stream) = &retained.stream else {
            return Ok(());
        };

        let clip = run.clip;
        self.device
            .borrow_mut()
            .draw_surface_instances(program, path, stream, mvp, &clip, 0)?;
        stats.draw_calls += 1;
        stats.triangles += retained.instances * 2;
        Ok(())
    }

    /// Rebuild a run's instances and replace or create its stream.
    fn replace(
        &mut self,
        key: (EntityId, SurfacePrimitiveIdentity),
        path: &D::SurfacePath,
        hash: u64,
        scratch: &mut Vec<SurfacePathInstance>,
        build: impl FnOnce(&mut Vec<SurfacePathInstance>),
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        scratch.clear();
        if !ipp_core::render_buffer_reuse_enabled() {
            *scratch = Vec::new();
        }
        build(scratch);

        let retained = self.runs.entry(key).or_insert_with(|| RetainedAnalyticRun {
            stream: None,
            hash,
            revision: 0,
            instances: 0,
            bytes: 0,
            seen: self.frame,
        });
        self.resident -= retained.bytes;
        retained.bytes = 0;
        retained.instances = 0;

        let mut device = self.device.borrow_mut();
        if scratch.is_empty() {
            if let Some(stream) = retained.stream.take() {
                device.delete_surface_instances(stream);
            }
            return Ok(());
        }

        let replaced = match retained.stream.as_mut() {
            Some(stream) => device.update_surface_instances(stream, path, scratch),
            None => device
                .create_surface_instances(path, scratch)
                .map(|stream| retained.stream = Some(stream)),
        };
        if let Err(error) = replaced {
            // A failed replacement leaves the stream's contents unknown; release it
            // so no later frame draws stale instances.
            if let Some(stream) = self.runs.remove(&key).and_then(|run| run.stream) {
                device.delete_surface_instances(stream);
            }
            return Err(error);
        }

        let bytes = scratch.len() * ANALYTIC_INSTANCE_BYTES;
        retained.instances = scratch.len() as u32;
        retained.bytes = bytes;
        self.resident += bytes;
        stats.uploaded_bytes = stats.uploaded_bytes.saturating_add(bytes as u32);
        Ok(())
    }

    /// End-of-frame maintenance: release streams of destroyed Surfaces and those a
    /// submitted Surface no longer draws analytically.
    ///
    /// `surfaces` is `None` when submission did not complete; every stream is kept.
    pub fn finish_frame(&mut self, surfaces: Option<&RetainedSurfaceSubmission<'_>>) {
        if let Some(surfaces) = surfaces {
            let frame = self.frame;
            let mut device = self.device.borrow_mut();
            let resident = &mut self.resident;
            self.runs.retain(|&(entity, _), run| {
                if !surfaces.is_stale(entity, run.seen == frame) {
                    return true;
                }

                *resident -= run.bytes;
                if let Some(stream) = run.stream.take() {
                    device.delete_surface_instances(stream);
                }
                false
            });
        }

        self.frame += 1;
    }
}

impl<D: RenderDevice> Drop for AnalyticGlyphCache<D> {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
#[path = "analytic_glyphs_tests.rs"]
mod tests;
