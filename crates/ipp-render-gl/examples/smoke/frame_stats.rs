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
    fn render_stats(
        &mut self,
        world: &mut ipp_core::WorldContext<'_>,
        width: u32,
        height: u32,
    ) -> Result<FrameStats, ipp_render_gl::RenderError>;
}

impl<D: ipp_render_gl::RenderDevice> RenderFrameStats for ipp_render_gl::RenderService<D> {
    fn render_stats(
        &mut self,
        world: &mut ipp_core::WorldContext<'_>,
        width: u32,
        height: u32,
    ) -> Result<FrameStats, ipp_render_gl::RenderError> {
        let summary = self.render(world, width, height)?;
        Ok(FrameStats::new(summary, *self.statistics()))
    }
}
