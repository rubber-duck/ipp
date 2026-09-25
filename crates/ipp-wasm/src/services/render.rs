//! The WASM host integrates stage 7 before publishing stage 8 outcomes/events.
//! GPU state belongs to the context; the world's retained assets survive detach.
//!
//! Every build exports the frame summary and the device viewport limits.
//! `diagnostics` builds also export one packed statistics record (see
//! [`super::render_statistics`]), Surface cache records built when read, and
//! the renderer's testing overrides.

use ipp_render_gl::{RenderError, RenderFrameSummary, RenderService, WebGlRenderDevice};

use crate::BOUNDARY;

pub(crate) struct RenderSurfaceService {
    renderer: RenderService<WebGlRenderDevice>,
    active: bool,
    width: u32,
    height: u32,
    tick: u64,
    summary: RenderFrameSummary,
    /// Packed record export and the totals it accumulates across renders.
    #[cfg(feature = "diagnostics")]
    statistics: super::render_statistics::RenderStatisticsRecord,
    /// World of the last completed render, whose records a capture reads.
    #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
    rendered_world: Option<ipp_core::WorldId>,
    /// Reused scratch for the records export.
    #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
    surface_cache_diagnostics: Vec<ipp_render_gl::SurfaceCacheDiagnostic>,
    /// Packed [`SURFACE_CACHE_RECORD_WORDS`]-word records, built when read.
    #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
    surface_cache_records: Vec<u32>,
}

/// Words per exported Surface cache record: entity low and high words,
/// presentation code, band, width, height, repaints, reuses, World time of the
/// last repaint in milliseconds, and resident bytes.
#[cfg(all(feature = "surfaces", feature = "diagnostics"))]
const SURFACE_CACHE_RECORD_WORDS: usize = 10;

impl RenderSurfaceService {
    pub(crate) fn new(world: &mut ipp_core::HostRuntime) -> Self {
        let renderer = RenderService::new(WebGlRenderDevice::new()).expect("empty renderer");
        renderer.set_asset_context_active(false);
        renderer
            .install(world)
            .expect("factories registered before source use");
        Self {
            renderer,
            active: false,
            width: 1,
            height: 1,
            tick: 0,
            summary: RenderFrameSummary::default(),
            #[cfg(feature = "diagnostics")]
            statistics: super::render_statistics::RenderStatisticsRecord::new(),
            #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
            rendered_world: None,
            #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
            surface_cache_diagnostics: Vec::new(),
            #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
            surface_cache_records: Vec::new(),
        }
    }

    pub(crate) fn progress_resources(&mut self, host: &mut ipp_core::HostRuntime) {
        if self.active {
            host.progress_assets();
        } else {
            host.progress_evaluation_assets();
        }
    }

    pub(crate) fn viewport(&self) -> Option<(u32, u32)> {
        self.active.then_some((self.width, self.height))
    }

    /// Accept a drawing-buffer size the device supports. Without a device that
    /// can report its limits, such as a lost context, only zero is rejected;
    /// the next attach validates again.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Err("viewport dimensions must be positive".to_owned());
        }

        if let Some(limits) = self.renderer.viewport_limits()
            && (width > limits.max_width || height > limits.max_height)
        {
            return Err(format!(
                "viewport {width}x{height} exceeds the device limits {}x{}",
                limits.max_width, limits.max_height
            ));
        }

        self.width = width;
        self.height = height;
        Ok(())
    }

    fn attach(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.detach_host(host);
        self.resize(width, height)?;
        self.renderer.set_asset_context_active(true);
        self.active = true;
        Ok(())
    }

    fn detach_host(&mut self, host: &mut ipp_core::HostRuntime) {
        self.renderer.set_asset_context_active(false);
        self.renderer.unload_host(host);
        host.flush_resource_lifecycle();
        self.active = false;
        self.reset_world();
    }

    pub(crate) fn forget_world(&mut self, world: ipp_core::WorldId) {
        self.renderer.forget_world(world);
        self.reset_world();
    }

    pub(crate) fn reset_world(&mut self) {
        self.tick = 0;
        self.summary = RenderFrameSummary::default();
        #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
        {
            self.rendered_world = None;
        }
    }

    fn detach(&mut self, world: &mut ipp_core::WorldContext<'_>) {
        self.renderer.set_asset_context_active(false);
        self.renderer.unload(world);
        self.active = false;
        self.reset_world();
    }

    /// Build the records export for the World of the last completed render.
    #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
    fn surface_cache_records(&mut self) -> &[u32] {
        self.surface_cache_records.clear();
        let Some(world) = self.rendered_world else {
            return &self.surface_cache_records;
        };

        self.surface_cache_diagnostics.clear();
        self.renderer
            .surface_cache_diagnostics(world, &mut self.surface_cache_diagnostics);

        for diagnostic in &self.surface_cache_diagnostics {
            let entity = diagnostic.entity.to_bits();
            let painted_at_ms = (diagnostic.painted_at * 1_000.0)
                .round()
                .clamp(0.0, f64::from(u32::MAX)) as u32;
            self.surface_cache_records.extend([
                entity as u32,
                (entity >> 32) as u32,
                diagnostic.presentation.code(),
                u32::from(diagnostic.band),
                diagnostic.size[0],
                diagnostic.size[1],
                diagnostic.repaints,
                diagnostic.reuses,
                painted_at_ms,
                diagnostic.resident_bytes,
            ]);
        }

        debug_assert_eq!(
            self.surface_cache_records.len(),
            self.surface_cache_diagnostics.len() * SURFACE_CACHE_RECORD_WORDS
        );
        &self.surface_cache_records
    }

    pub(crate) fn render(
        &mut self,
        world: &mut ipp_core::WorldContext<'_>,
    ) -> Result<(), ipp_host_session::HostPresentationFailure> {
        if !self.active {
            return Ok(());
        }

        match self.renderer.render(world, self.width, self.height) {
            Ok(summary) => {
                self.summary = summary;
                self.tick = world.tick();

                #[cfg(feature = "diagnostics")]
                self.statistics.accumulate(self.renderer.statistics());

                #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
                {
                    self.rendered_world = Some(world.id());
                }

                Ok(())
            }
            Err(error) => {
                let scope = match error {
                    RenderError::ContextLost => {
                        self.detach(world);
                        ipp_protocol::RuntimeFailureScope::Context
                    }
                    RenderError::MissingMesh | RenderError::MissingTexture => {
                        ipp_protocol::RuntimeFailureScope::Resource
                    }
                    _ => ipp_protocol::RuntimeFailureScope::Draw,
                };
                Err(ipp_host_session::HostPresentationFailure {
                    scope,
                    message: error.to_string(),
                })
            }
        }
    }
}

fn update(
    action: impl FnOnce(&mut RenderSurfaceService, &mut ipp_core::HostRuntime) -> Result<(), String>,
) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        let result = boundary
            .presentation_host()
            .ok_or_else(|| "session is closed".to_owned())
            .and_then(|(presentation, host)| action(presentation, host));
        match result {
            Ok(()) => 1,
            Err(error) => u32::from(boundary.fail(&error)),
        }
    })
}

fn read<T: Default>(read: impl FnOnce(&RenderSurfaceService) -> T) -> T {
    BOUNDARY
        .with_borrow_mut(|boundary| boundary.presentation().map(|p| read(p)).unwrap_or_default())
}

/// Attach only after the host binds WASM memory to its live WebGL device.
// SAFETY: Unique host symbol; calls are exclusive on the owning worker, no borrowed state escapes.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_attach(width: u32, height: u32) -> u32 {
    update(|presentation, world| presentation.attach(world, width, height))
}

/// Resize the host-owned viewport without changing world state or advancing
/// time. Sizes beyond [`ipp_render_max_viewport_width`] and
/// [`ipp_render_max_viewport_height`] fail.
// SAFETY: Unique symbol; scalar arguments and exclusive access to owned renderer state.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_resize(width: u32, height: u32) -> u32 {
    update(|presentation, _world| presentation.resize(width, height))
}

/// Largest drawing-buffer width the device accepts, or zero while it cannot report.
// SAFETY: Unique symbol; returns an owned scalar under exclusive Host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_max_viewport_width() -> u32 {
    read(|presentation| {
        presentation
            .renderer
            .viewport_limits()
            .map_or(0, |limits| limits.max_width)
    })
}

/// Largest drawing-buffer height the device accepts, or zero while it cannot report.
// SAFETY: Unique symbol; returns an owned scalar under exclusive Host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_max_viewport_height() -> u32 {
    read(|presentation| {
        presentation
            .renderer
            .viewport_limits()
            .map_or(0, |limits| limits.max_height)
    })
}

/// Discard context-scoped state before loss/replacement; retain the logical world.
// SAFETY: Unique symbol; resource invalidation and destruction are exclusive and retain no aliases.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_detach() {
    BOUNDARY.with_borrow_mut(|boundary| {
        if let Some((presentation, host)) = boundary.presentation_host() {
            presentation.detach_host(host);
        }
    });
}

/// Last world tick submitted to this context; zero after detach or before drawing.
// SAFETY: Unique symbol; returns an owned scalar, no reference or pointer escapes.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_tick() -> u64 {
    read(|presentation| presentation.tick)
}

/// Draw submissions of the last completed frame.
// SAFETY: Unique symbol; read-only scalar snapshot under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_draw_calls() -> u32 {
    read(|presentation| presentation.summary.draw_calls)
}

/// Submitted triangles of the last completed frame.
// SAFETY: Unique symbol; read-only scalar snapshot under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_triangles() -> u32 {
    read(|presentation| presentation.summary.triangles)
}

/// Instances skipped after a mesh upload failure in the last completed frame.
// SAFETY: Unique symbol; returns an owned scalar under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_failed_draw_calls() -> u32 {
    read(|presentation| presentation.summary.failed_draw_calls)
}

/// Whether the last completed frame cleared because its camera was unusable.
// SAFETY: Unique symbol; returns an owned scalar under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_invalid_camera() -> u32 {
    read(|presentation| u32::from(presentation.summary.invalid_camera))
}

/// Shadow submissions for the opt-in benchmark, separate from main-pass draws.
// SAFETY: Unique diagnostic symbol; owned scalar under exclusive Host access.
#[cfg(all(feature = "profiling", feature = "diagnostics"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_shadow_draw_calls() -> u32 {
    #[cfg(feature = "shadows")]
    {
        read(|presentation| presentation.renderer.statistics().shadow_draw_calls)
    }

    #[cfg(not(feature = "shadows"))]
    {
        0
    }
}

/// Fill the packed statistics record of the last completed render and return
/// it; [`ipp_render_statistics_len`] gives its length. The layout is documented
/// in `services/render_statistics.rs`. Null while no session is open.
// SAFETY: Unique symbol; the pointer refers to presentation-owned storage that
// stays valid and unaliased by Rust until the next call of this export or the
// session closes. The caller copies it synchronously.
#[cfg(feature = "diagnostics")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_statistics_ptr() -> *const u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(std::ptr::null(), |presentation| {
                let statistics = *presentation.renderer.statistics();
                presentation.statistics.fill(&statistics).as_ptr()
            })
    })
}

/// Words in the record at [`ipp_render_statistics_ptr`].
// SAFETY: Unique symbol; returns a constant scalar.
#[cfg(feature = "diagnostics")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_statistics_len() -> u32 {
    super::render_statistics::RECORD_WORDS as u32
}

/// Build the Surface cache records of the last completed frame and return
/// them, or null when there are none. Each record has
/// `SURFACE_CACHE_RECORD_WORDS` little-endian `u32` words;
/// [`ipp_render_surface_cache_records_len`] gives the word count of this build.
// SAFETY: Unique symbol; the pointer refers to presentation-owned storage that
// stays valid and unaliased by Rust until the next call of this export, a
// render, or the session closes. The caller copies it synchronously.
#[cfg(all(feature = "surfaces", feature = "diagnostics"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_records_ptr() -> *const u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(std::ptr::null(), |presentation| {
                let records = presentation.surface_cache_records();
                if records.is_empty() {
                    std::ptr::null()
                } else {
                    records.as_ptr()
                }
            })
    })
}

/// Length in `u32` words of the records the last
/// [`ipp_render_surface_cache_records_ptr`] call built.
// SAFETY: Unique symbol; returns a length without retaining a reference.
#[cfg(all(feature = "surfaces", feature = "diagnostics"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_records_len() -> u32 {
    read(|presentation| presentation.surface_cache_records.len() as u32)
}

/// Testing override of this context's renderer-owned glyph atlas bounds: its
/// resident page budget and the demand publications a page without demand stays
/// resident. Limits survive context loss.
// SAFETY: Unique symbol; scalar arguments and exclusive access to owned renderer state.
#[cfg(all(feature = "gui", feature = "diagnostics"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_set_glyph_atlas_limits(
    max_pages: u32,
    idle_page_publications: u32,
) -> u32 {
    update(|presentation, _host| {
        presentation
            .renderer
            .set_glyph_atlas_limits(ipp_render_gl::GlyphAtlasLimits {
                max_pages: max_pages as usize,
                idle_page_publications: u64::from(idle_page_publications),
            });
        Ok(())
    })
}

/// Testing override of the renderer-owned Surface cache image budget on this
/// context. Zero disables caching; the budget survives context loss.
// SAFETY: Unique symbol; scalar argument and exclusive access to owned renderer state.
#[cfg(all(feature = "surfaces", feature = "diagnostics"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_set_surface_cache_budget(bytes: u32) -> u32 {
    update(|presentation, _host| {
        presentation
            .renderer
            .set_surface_cache_budget(bytes as usize);
        Ok(())
    })
}

/// Testing mode attributing each GL error to its failing call instead of the
/// sampled pass-boundary checks.
// SAFETY: Unique symbol; scalar argument and exclusive access to owned renderer state.
#[cfg(feature = "diagnostics")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_set_exhaustive_draw_checks(enabled: u32) -> u32 {
    update(|presentation, _host| {
        presentation
            .renderer
            .set_exhaustive_draw_checks(enabled != 0);
        Ok(())
    })
}
