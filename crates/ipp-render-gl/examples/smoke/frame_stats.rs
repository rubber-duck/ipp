//! Frame summaries and diagnostics statistics read together by the smoke fixtures.

/// A completed render's summary together with its diagnostics statistics.
///
/// Statistics fields are reached through `Deref`, so fixtures read every counter of
/// one frame from one value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameStats {
    pub(crate) draw_calls: u32,
    pub(crate) triangles: u32,
    pub(crate) failed_draw_calls: u32,
    pub(crate) invalid_camera: bool,
    statistics: ipp_render_gl::RenderStatistics,
}

impl FrameStats {
    /// Combine a render's summary with the statistics the renderer kept for it.
    pub(crate) fn new(
        summary: ipp_render_gl::RenderFrameSummary,
        statistics: ipp_render_gl::RenderStatistics,
    ) -> Self {
        Self {
            draw_calls: summary.draw_calls,
            triangles: summary.triangles,
            failed_draw_calls: summary.failed_draw_calls,
            invalid_camera: summary.invalid_camera,
            statistics,
        }
    }
}

impl std::ops::Deref for FrameStats {
    type Target = ipp_render_gl::RenderStatistics;

    fn deref(&self) -> &Self::Target {
        &self.statistics
    }
}

impl std::ops::DerefMut for FrameStats {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.statistics
    }
}

/// Render and collect the completed frame's summary and statistics.
pub(crate) trait RenderFrameStats {
    fn draw_stats(
        &mut self,
        host: &ipp_core::HostRuntime,
        world: ipp_core::WorldId,
        width: u32,
        height: u32,
    ) -> Result<FrameStats, ipp_render_gl::RenderError>;
}

impl<D: ipp_render_gl::RenderDevice> RenderFrameStats for ipp_render_gl::RenderService<D> {
    fn draw_stats(
        &mut self,
        host: &ipp_core::HostRuntime,
        world: ipp_core::WorldId,
        width: u32,
        height: u32,
    ) -> Result<FrameStats, ipp_render_gl::RenderError> {
        let viewport = ipp_core::WorldViewport {
            width,
            height,
            device_pixel_ratio: 1.0,
        };
        let summary = if let Some((selection, _, publication)) = host.root_output(world) {
            let time = host
                .publication(publication)
                .expect("selected publication")
                .time;
            self.draw(host, selection, publication, viewport, time)?
        } else {
            self.clear(viewport)?
        };
        Ok(FrameStats::new(summary, *self.statistics()))
    }
}
