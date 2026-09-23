//! The WASM host integrates stage 7 before publishing stage 8 outcomes/events.
//! GPU state belongs to the context; the world's retained assets survive detach.

use ipp_render_gl::{RenderError, RenderService, RenderStats, WebGlRenderDevice};

use crate::BOUNDARY;

pub(crate) struct RenderSurfaceService {
    renderer: RenderService<WebGlRenderDevice>,
    active: bool,
    width: u32,
    height: u32,
    tick: u64,
    stats: RenderStats,
    pending_uploaded_bytes: u32,
    /// Reused diagnostics scratch for the records export.
    #[cfg(feature = "surfaces")]
    surface_cache_diagnostics: Vec<ipp_render_gl::SurfaceCacheDiagnostic>,
    /// Packed [`SURFACE_CACHE_RECORD_WORDS`]-word records of the last completed frame.
    #[cfg(feature = "surfaces")]
    surface_cache_records: Vec<u32>,
}

/// Words per exported Surface cache record: entity low and high words,
/// presentation code, band, width, height, repaints, reuses, World time of the
/// last repaint in milliseconds, and resident bytes.
#[cfg(feature = "surfaces")]
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
            stats: RenderStats::default(),
            pending_uploaded_bytes: 0,
            #[cfg(feature = "surfaces")]
            surface_cache_diagnostics: Vec::new(),
            #[cfg(feature = "surfaces")]
            surface_cache_records: Vec::new(),
        }
    }

    pub(crate) fn progress_assets(&mut self, host: &mut ipp_core::HostRuntime) {
        self.pending_uploaded_bytes = self
            .pending_uploaded_bytes
            .saturating_add(self.renderer.begin_frame());
        self.progress_resources(host);
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

    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if width == 0 || height == 0 || width > 2_048 || height > 2_048 {
            return Err("viewport dimensions must be within 1..=2048".to_owned());
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
        self.stats = RenderStats::default();
        self.pending_uploaded_bytes = 0;
        #[cfg(feature = "surfaces")]
        self.surface_cache_records.clear();
    }

    fn detach(&mut self, world: &mut ipp_core::WorldContext<'_>) {
        self.renderer.set_asset_context_active(false);
        self.renderer.unload(world);
        self.active = false;
        self.tick = 0;
        self.stats = RenderStats::default();
        #[cfg(feature = "surfaces")]
        self.surface_cache_records.clear();
    }

    /// Refill the records export from the renderer after a completed frame.
    #[cfg(feature = "surfaces")]
    fn publish_surface_caches(&mut self, world: ipp_core::WorldId) {
        self.surface_cache_diagnostics.clear();
        self.renderer
            .surface_cache_diagnostics(world, &mut self.surface_cache_diagnostics);
        self.surface_cache_records.clear();
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
    }

    pub(crate) fn render(
        &mut self,
        world: &mut ipp_core::WorldContext<'_>,
    ) -> Result<(), ipp_host_session::HostPresentationFailure> {
        if self.active {
            match self.renderer.render(world, self.width, self.height) {
                Ok(mut stats) => {
                    stats.uploaded_bytes = stats
                        .uploaded_bytes
                        .saturating_add(self.pending_uploaded_bytes);
                    self.pending_uploaded_bytes = 0;
                    self.stats = stats;
                    self.tick = world.tick();
                    #[cfg(feature = "surfaces")]
                    self.publish_surface_caches(world.id());
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
                    return Err(ipp_host_session::HostPresentationFailure {
                        scope,
                        message: error.to_string(),
                    });
                }
            }
        }
        Ok(())
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

/// Attach only after the host binds WASM memory to its live WebGL device.
// SAFETY: Unique host symbol; calls are exclusive on the owning worker, no borrowed state escapes.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_attach(width: u32, height: u32) -> u32 {
    update(|presentation, world| presentation.attach(world, width, height))
}

/// Resize the host-owned viewport without changing world state or advancing time.
// SAFETY: Unique symbol; scalar arguments and exclusive access to owned renderer state.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_resize(width: u32, height: u32) -> u32 {
    update(|presentation, _world| presentation.resize(width, height))
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
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.tick)
    })
}

// SAFETY: Unique symbol; read-only scalar snapshot under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_draw_calls() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.draw_calls)
    })
}

/// Shadow submissions for the opt-in benchmark, separate from main-pass draws.
// SAFETY: Unique diagnostic symbol; owned scalar under exclusive Host access.
#[cfg(feature = "profiling")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_profile_shadow_draw_calls() -> u32 {
    #[cfg(feature = "shadows")]
    {
        BOUNDARY.with_borrow_mut(|boundary| {
            boundary
                .presentation()
                .map_or(0, |presentation| presentation.stats.shadow_draw_calls)
        })
    }

    #[cfg(not(feature = "shadows"))]
    {
        0
    }
}

// SAFETY: Unique symbol; read-only scalar snapshot under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_triangles() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.triangles)
    })
}

// SAFETY: Unique symbol; read-only scalar snapshot under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_uploaded_bytes() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.uploaded_bytes)
    })
}

/// Instances skipped after a mesh upload failure in the last completed frame.
// SAFETY: Unique symbol; returns an owned scalar under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_failed_draw_calls() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.failed_draw_calls)
    })
}

/// Whether the last completed frame cleared because its camera was unusable.
// SAFETY: Unique symbol; returns an owned scalar under exclusive host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_invalid_camera() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        u32::from(
            boundary
                .presentation()
                .is_some_and(|presentation| presentation.stats.invalid_camera),
        )
    })
}

/// Shadow-capacity fallback count for structured frame diagnostics.
// SAFETY: Unique symbol; scalar snapshot under exclusive Host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_unshadowed_lights() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.unshadowed_lights)
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_gui_batches() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.gui_batches)
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_gui_rebuilds() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.gui_rebuilds)
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_gui_allocations() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.gui_allocations)
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_gui_resident_bytes() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.gui_resident_bytes)
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_glyph_misses() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.glyph_misses)
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_glyph_populates() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.glyph_populates)
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_glyph_population_failures() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary.presentation().map_or(0, |presentation| {
            presentation.stats.glyph_population_failures
        })
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_glyph_page_retirements() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.glyph_page_retirements)
    })
}

/// Bound this context's shared glyph atlas: its resident page budget and the demand
/// publications a page without demand stays resident. Limits survive context loss.
// SAFETY: Unique symbol; scalar arguments and exclusive access to owned renderer state.
#[cfg(feature = "gui")]
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

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_glyph_pages() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.glyph_pages)
    })
}

/// Read-only retained Surface work from the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "gui")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_glyph_resident_bytes() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary.presentation().map_or(0, |presentation| {
            presentation.stats.glyph_resident_bytes as u32
        })
    })
}

/// Opted-in Surfaces repainted into cache images in the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_repaints() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.surface_cache_repaints)
    })
}

/// Opted-in Surfaces composited from unchanged cache images in the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_reuses() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.surface_cache_reuses)
    })
}

/// Opted-in visible Surfaces presented directly in the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_direct() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.surface_cache_direct)
    })
}

/// Opted-in Surfaces presented directly after budget or allocation fallback in the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_fallbacks() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.surface_cache_fallbacks)
    })
}

/// Surface cache images allocated or resized in the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_allocations() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary.presentation().map_or(0, |presentation| {
            presentation.stats.surface_cache_allocations
        })
    })
}

/// Resident Surface cache images on this context after the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_entries() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(0, |presentation| presentation.stats.surface_cache_entries)
    })
}

/// Resident Surface cache image bytes on this context after the last completed frame.
// SAFETY: Unique diagnostic symbol returning an owned scalar under exclusive Host access.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_resident_bytes() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary.presentation().map_or(0, |presentation| {
            presentation.stats.surface_cache_resident_bytes
        })
    })
}

/// Read-only Surface cache records of the last completed frame, or null when
/// empty. Each record has `SURFACE_CACHE_RECORD_WORDS` little-endian `u32`
/// words; copy them before the next render, detach or World change.
// SAFETY: Unique symbol; the pointer refers to renderer-owned storage that stays
// valid and unaliased by Rust until the next exclusive render or reset call.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_records_ptr() -> *const u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .filter(|presentation| !presentation.surface_cache_records.is_empty())
            .map_or(std::ptr::null(), |presentation| {
                presentation.surface_cache_records.as_ptr()
            })
    })
}

/// Length in `u32` words of the records at [`ipp_render_surface_cache_records_ptr`].
// SAFETY: Unique symbol; returns a length without retaining a reference.
#[cfg(feature = "surfaces")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_records_len() -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary.presentation().map_or(0, |presentation| {
            presentation.surface_cache_records.len() as u32
        })
    })
}
