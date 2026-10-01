//! One worker graphics context, selected by the shared Host presentation coordinator.
//! Draw completion and synchronous top-left readback use the common protocol; no
//! session, World tick or JavaScript frame queue authorizes presentation.
//! `ipp_presentation` imports resize the exact surface and copy pixels without
//! reentering WASM. Diagnostics exports remain separate observations.

use ipp_core::{HostRuntime, OutputRef, WorldPublicationId, WorldViewport};
use ipp_host_session::{HostPresentationFailure, PresentationDrawSummary};
use ipp_protocol::presentation::{PresentationError, PresentationSurface};
use ipp_render_gl::{RenderError, RenderService, WebGlRenderDevice};

use crate::BOUNDARY;

#[link(wasm_import_module = "ipp_presentation")]
unsafe extern "C" {
    fn resize(width: u32, height: u32) -> u32;
    fn capture(pointer: *mut u8, length: usize) -> u32;
}

pub(crate) struct RenderSurfaceService {
    renderer: RenderService<WebGlRenderDevice>,
    active: bool,
    width: u32,
    height: u32,
    context: u64,
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
    #[cfg(all(feature = "gui", feature = "diagnostics"))]
    pub(crate) fn record_frame(
        &mut self,
        host: &mut HostRuntime,
        frame: &ipp_core::HostFrameReport,
    ) {
        self.statistics.record_frame(host, frame);
    }

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
            context: 0,
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

    pub(crate) fn surface(&self) -> Result<PresentationSurface, PresentationError> {
        if !self.active {
            return Err(PresentationError::Unavailable);
        }
        let limits = self
            .renderer
            .viewport_limits()
            .ok_or(PresentationError::Unavailable)?;
        Ok(PresentationSurface {
            id: 1,
            context: self.context,
            max_width: limits.max_width,
            max_height: limits.max_height,
        })
    }

    pub(crate) fn configure(&mut self, viewport: WorldViewport) -> Result<(), PresentationError> {
        let surface = self.surface()?;
        if viewport.width == 0
            || viewport.height == 0
            || viewport.width > surface.max_width
            || viewport.height > surface.max_height
            || !viewport.device_pixel_ratio.is_finite()
            || viewport.device_pixel_ratio <= 0.0
        {
            return Err(PresentationError::InvalidViewport);
        }
        // SAFETY: The synchronous platform import takes only scalars, never reenters
        // the runtime, and reports the exact drawing-buffer dimensions it accepted.
        if unsafe { resize(viewport.width, viewport.height) } != 1 {
            return Err(PresentationError::InvalidViewport);
        }
        self.width = viewport.width;
        self.height = viewport.height;
        Ok(())
    }

    fn attach(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.detach_host(host)?;
        self.context = self
            .context
            .checked_add(1)
            .ok_or("presentation context identity exhausted")?;
        self.renderer.set_asset_context_active(true);
        self.active = true;
        if let Err(error) = self.configure(WorldViewport {
            width,
            height,
            device_pixel_ratio: 1.0,
        }) {
            self.active = false;
            self.renderer.set_asset_context_active(false);
            return Err(format!("presentation attach failed: {error:?}"));
        }
        Ok(())
    }

    fn detach_host(&mut self, host: &mut ipp_core::HostRuntime) -> Result<(), String> {
        self.renderer
            .prepare(host, None)
            .map_err(|error| error.to_string())?;
        self.renderer
            .unload_host(host)
            .map_err(|error| error.to_string())?;
        self.renderer.set_asset_context_active(false);
        host.flush_resource_lifecycle();
        self.active = false;
        #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
        {
            self.rendered_world = None;
        }
        Ok(())
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

    pub(crate) fn prepare(
        &mut self,
        host: &mut HostRuntime,
        selected: Option<(OutputRef, WorldPublicationId)>,
    ) -> Result<(), HostPresentationFailure> {
        self.renderer
            .prepare(host, selected)
            .map_err(presentation_failure)
    }

    pub(crate) fn present(
        &mut self,
        host: &HostRuntime,
        output: OutputRef,
        publication: WorldPublicationId,
        viewport: WorldViewport,
        presentation_time: f64,
        completion: ipp_host_session::PresentationCompletion<'_>,
    ) -> Result<PresentationDrawSummary, HostPresentationFailure> {
        let ipp_host_session::PresentationCompletion {
            capture: pixels,
            outputs,
        } = completion;
        if !self.active {
            return Err(presentation_failure(RenderError::ContextLost));
        }
        if (self.width, self.height) != (viewport.width, viewport.height) {
            return Err(presentation_failure(RenderError::InvalidViewport));
        }
        let summary = self
            .renderer
            .draw_observed(
                host,
                output,
                publication,
                viewport,
                presentation_time,
                outputs,
            )
            .map_err(presentation_failure)?;
        if summary.invalid_camera {
            return Err(HostPresentationFailure {
                scope: ipp_protocol::RuntimeFailureScope::Draw,
                message: "selected camera is invalid".into(),
            });
        }
        #[cfg(feature = "diagnostics")]
        self.statistics.accumulate(self.renderer.statistics());
        #[cfg(all(feature = "surfaces", feature = "diagnostics"))]
        {
            self.rendered_world = Some(output.world().id());
        }
        if let Some(pixels) = pixels {
            // SAFETY: The exclusive slice stays live for this synchronous import.
            // The worker copies exactly length bytes, retains no pointer and must
            // not reenter WASM while the Host and this capture are borrowed.
            if unsafe { capture(pixels.as_mut_ptr(), pixels.len()) } != 1 {
                return Err(HostPresentationFailure {
                    scope: ipp_protocol::RuntimeFailureScope::Context,
                    message: "selected frame readback failed".into(),
                });
            }
        }
        Ok(PresentationDrawSummary {
            draw_calls: summary.draw_calls,
            triangles: summary.triangles,
            failed_draw_calls: summary.failed_draw_calls,
        })
    }
}

fn presentation_failure(error: RenderError) -> HostPresentationFailure {
    HostPresentationFailure {
        scope: match error {
            RenderError::ContextLost => ipp_protocol::RuntimeFailureScope::Context,
            RenderError::MissingMesh | RenderError::MissingTexture => {
                ipp_protocol::RuntimeFailureScope::Resource
            }
            _ => ipp_protocol::RuntimeFailureScope::Draw,
        },
        message: error.to_string(),
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
pub extern "C" fn ipp_render_detach() -> u32 {
    update(|presentation, host| presentation.detach_host(host))
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

/// Whole-Host ordinary-layout membership JSON, copied synchronously by the worker.
// SAFETY: Unique diagnostic symbol. Exclusive boundary access fills owned storage;
// the returned bytes remain valid until the next call or Host destruction, without Rust aliases.
#[cfg(all(feature = "gui", feature = "diagnostics"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_gui_layout_ptr() -> *const u8 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(std::ptr::null(), |presentation| {
                presentation.statistics.layout_json().as_ptr()
            })
    })
}

/// Bytes at the last pointer returned by `ipp_render_gui_layout_ptr`.
// SAFETY: Unique diagnostic symbol. Exclusive boundary access returns only an owned scalar.
#[cfg(all(feature = "gui", feature = "diagnostics"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_gui_layout_len() -> u32 {
    read(|presentation| presentation.statistics.layout_json_len() as u32)
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
/// resident page budget and the Host frames a page without demand stays
/// resident. Limits survive context loss.
// SAFETY: Unique symbol; scalar arguments and exclusive access to owned renderer state.
#[cfg(all(feature = "gui", feature = "diagnostics"))]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_set_glyph_atlas_limits(max_pages: u32, idle_page_frames: u32) -> u32 {
    update(|presentation, _host| {
        presentation
            .renderer
            .set_glyph_atlas_limits(ipp_render_gl::GlyphAtlasLimits {
                max_pages: max_pages as usize,
                idle_page_frames: u64::from(idle_page_frames),
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
