//! Frame summaries returned by every render and statistics kept only in
//! `diagnostics` builds.
//!
//! The summary is the host's observation of a completed render. Statistics are
//! optional diagnostics: they compile out of lean builds, are read on demand and
//! never drive eviction, release or readiness.

/// Work of one completed render call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderFrameSummary {
    /// GPU draw submissions, in stable prepared order.
    pub draw_calls: u32,
    /// Submitted triangles, including repeated instances of shared geometry.
    pub triangles: u32,
    /// Instances skipped because their mesh could not acquire GPU residency.
    /// CPU geometry remains usable; resource reload permits another upload.
    pub failed_draw_calls: u32,
    /// The selected camera cannot represent this viewport; the frame is clear.
    /// Selection and session state remain available for repair or resize.
    pub invalid_camera: bool,
}

/// Diagnostic statistics of the last completed render.
#[cfg(feature = "diagnostics")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStatistics {
    /// Vertex/index/pixel uploads accounted by this submission, including resource
    /// uploads completed since the previous completed render and pending shared
    /// service work once. Later worlds do not recount the same allocation.
    /// Analytic Surface glyph runs count when their retained instance stream is
    /// created or replaced; unchanged runs upload nothing.
    pub uploaded_bytes: u32,
    /// Selected shadow-requesting lights left unshadowed by capacity/allocation fallback.
    pub unshadowed_lights: u32,
    /// Depth-only mesh submissions, separate from visible forward draws.
    #[cfg(feature = "shadows")]
    pub shadow_draw_calls: u32,
    /// Private depth allocation estimate (four bytes per texel).
    #[cfg(feature = "shadows")]
    pub shadow_resident_bytes: u32,
    /// Retained GUI box and glyph batches drawn. Consecutive batches of a Surface
    /// share draws, which [`RenderFrameSummary::draw_calls`] counts.
    #[cfg(feature = "gui")]
    pub gui_batches: u32,
    /// GUI box primitives whose CPU geometry was regenerated, plus glyph batches
    /// rebuilt after a text edit or the retirement of an atlas page they sampled.
    #[cfg(feature = "gui")]
    pub gui_rebuilds: u32,
    /// Retained GUI batches written to GPU storage during this frame.
    #[cfg(feature = "gui")]
    pub gui_allocations: u32,
    /// Resident bytes of retained per-Surface GUI GPU storage across every World
    /// presented through this context.
    #[cfg(feature = "gui")]
    pub gui_resident_bytes: u32,
    /// Distinct glyph atlas entries that visible text demanded but did not find
    /// during this submission, including entries the population budget or a failure
    /// back-off defers to a later frame.
    #[cfg(feature = "gui")]
    pub glyph_misses: u32,
    /// Glyph atlas entries rasterized during this submission, before the main pass.
    /// A frame populates at least [`crate::GLYPH_MIN_POPULATES_PER_FRAME`] missing
    /// entries, then as many as the population time budget covers; later entries
    /// count as misses and their text stays analytic until a following frame
    /// populates them.
    #[cfg(feature = "gui")]
    pub glyph_populates: u32,
    /// Recoverable glyph atlas allocation or rasterization failures during this
    /// submission. Each glyph backs off and its text uses analytic glyphs meanwhile.
    #[cfg(feature = "gui")]
    pub glyph_population_failures: u32,
    /// Glyph atlas pages retired since the previous completed submission: idle past
    /// the renderer's limit, reclaimed under allocation pressure or released when no
    /// World demands any glyph. Context loss is not counted.
    #[cfg(feature = "gui")]
    pub glyph_page_retirements: u32,
    /// Number of resident glyph atlas pages shared by every World on this context.
    #[cfg(feature = "gui")]
    pub glyph_pages: u32,
    /// Total resident bytes occupied by the shared glyph atlas page textures: one
    /// byte per texel of single-channel coverage.
    #[cfg(feature = "gui")]
    pub glyph_resident_bytes: usize,
    /// Resident bytes of retained analytic Surface glyph instance streams across every
    /// World presented through this context, sixteen `f32` lanes per instance.
    #[cfg(feature = "surfaces")]
    pub analytic_glyph_resident_bytes: u32,
    /// Opted-in Surfaces repainted into their cache images during this submission,
    /// before the main pass. Each repaint also counts its primitive draws.
    #[cfg(feature = "surfaces")]
    pub surface_cache_repaints: u32,
    /// Opted-in Surfaces composited from an unchanged cache image, without repainting.
    /// A composite is one draw call of two triangles.
    #[cfg(feature = "surfaces")]
    pub surface_cache_reuses: u32,
    /// Opted-in visible Surfaces presented directly: inside their direct distance,
    /// under GUI interaction, after a fallback, while animated or without cache
    /// support. Culled Surfaces count in none of the cache counters.
    #[cfg(feature = "surfaces")]
    pub surface_cache_direct: u32,
    /// Opted-in Surfaces presented directly because the byte budget, a zero budget,
    /// or a recoverable allocation, repaint or composite failure and its retry
    /// interval left no usable image. Also counted in `surface_cache_direct`.
    #[cfg(feature = "surfaces")]
    pub surface_cache_fallbacks: u32,
    /// Opted-in visible Surfaces presented directly because their paint changed and
    /// was repainted at the refresh cap on every recent frame, where repainting costs
    /// more than drawing directly. Also counted in `surface_cache_direct`.
    #[cfg(feature = "surfaces")]
    pub surface_cache_animated: u32,
    /// Cache images created or resized during this submission; each is repainted
    /// before it is shown.
    #[cfg(feature = "surfaces")]
    pub surface_cache_allocations: u32,
    /// Resident cache images across every World presented through this context,
    /// after this submission's releases.
    #[cfg(feature = "surfaces")]
    pub surface_cache_entries: u32,
    /// Resident bytes of cache images across every World presented through this
    /// context, four per texel.
    #[cfg(feature = "surfaces")]
    pub surface_cache_resident_bytes: u32,
}

/// Work accumulated while one render submits: the summary always, statistics
/// only in `diagnostics` builds.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RenderFrameWork {
    pub(crate) summary: RenderFrameSummary,
    #[cfg(feature = "diagnostics")]
    pub(crate) statistics: RenderStatistics,
}

impl RenderFrameWork {
    /// Count one draw submission of `triangles` triangles.
    pub(crate) fn draw(&mut self, triangles: u32) {
        self.summary.draw_calls += 1;
        self.summary.triangles += triangles;
    }

    /// Count one draw skipped for missing GPU residency.
    pub(crate) fn failed_draw(&mut self) {
        self.summary.failed_draw_calls += 1;
    }

    /// Count bytes this submission wrote to GPU storage.
    #[cfg_attr(not(feature = "diagnostics"), allow(unused_variables))]
    pub(crate) fn uploaded(&mut self, bytes: usize) {
        #[cfg(feature = "diagnostics")]
        {
            self.statistics.uploaded_bytes = self
                .statistics
                .uploaded_bytes
                .saturating_add(u32::try_from(bytes).unwrap_or(u32::MAX));
        }
    }
}

/// Bytes resource loaders upload between completed renders.
///
/// Loaders run in the Host's asset lifecycle, outside `render`, and the device has
/// no accounting of its own, so the Service shares this counter with them. Without
/// `diagnostics` it is empty and counting compiles out.
#[derive(Clone, Debug, Default)]
pub(crate) struct RenderUploadCounter {
    #[cfg(feature = "diagnostics")]
    bytes: std::rc::Rc<std::cell::Cell<u32>>,
}

impl RenderUploadCounter {
    /// Record a completed upload of `bytes`.
    #[cfg_attr(not(feature = "diagnostics"), allow(unused_variables))]
    pub(crate) fn add(&self, bytes: usize) {
        #[cfg(feature = "diagnostics")]
        self.bytes.set(
            self.bytes
                .get()
                .saturating_add(u32::try_from(bytes).unwrap_or(u32::MAX)),
        );
    }

    /// Take the uploads recorded since the last completed render.
    #[cfg(feature = "diagnostics")]
    pub(crate) fn take(&self) -> u32 {
        self.bytes.replace(0)
    }
}
