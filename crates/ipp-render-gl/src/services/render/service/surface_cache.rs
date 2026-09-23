//! Whole-Surface cache orchestration: presentation planning, the repaint
//! pre-pass, composition and end-of-frame accounting.
//!
//! ipp-s1ge.2.3 delivers planning, repainting and composition; the current
//! submission presents every Surface directly.

use super::super::surface_cache::SurfaceCacheDiagnostic;
use super::{RenderError, RenderService, RenderStats};
use crate::RenderDevice;

impl<D: RenderDevice> RenderService<D> {
    /// Append the cache state of one World's opted-in Surfaces after the last
    /// completed frame, in entity order. Read-only; it never changes presentation.
    pub fn surface_cache_diagnostics(
        &self,
        world: ipp_core::WorldId,
        out: &mut Vec<SurfaceCacheDiagnostic>,
    ) {
        self.surface_cache.diagnostics(world, out);
    }

    /// Bound the resident bytes of Surface cache images on this context.
    ///
    /// Images beyond the budget are evicted, least recently presented first,
    /// before new allocations; Surfaces that still do not fit present directly.
    /// Zero disables caching. The budget survives context loss.
    pub fn set_surface_cache_budget(&mut self, bytes: usize) {
        self.surface_cache.set_budget(bytes);
    }

    /// Current Surface cache image budget in bytes.
    pub fn surface_cache_budget(&self) -> usize {
        self.surface_cache.budget()
    }

    /// Publish context-wide cache residency after a completed frame. Failed
    /// and cameraless frames keep every entry, as retained batches do.
    pub(super) fn finish_surface_caches(&mut self, stats: Option<&mut RenderStats>) {
        let Some(stats) = stats else {
            return;
        };

        (
            stats.surface_cache_entries,
            stats.surface_cache_resident_bytes,
        ) = self.surface_cache.resident();
    }

    /// Composite program: `surface_bitmap.vert` with the premultiplied
    /// `surface_cache.frag`, created on first use and released on unload.
    #[expect(
        dead_code,
        reason = "ipp-s1ge.2.3 composites cached Surfaces through this program"
    )]
    pub(super) fn surface_cache_program(&mut self) -> Result<&D::Program, RenderError> {
        if self.surface_cache_program.is_none() {
            self.surface_cache_program = Some(self.device.borrow_mut().create_program(
                include_str!("../shaders/surface_bitmap.vert"),
                include_str!("../shaders/surface_cache.frag"),
            )?);
        }

        Ok(self
            .surface_cache_program
            .as_ref()
            .expect("created composite program"))
    }
}
