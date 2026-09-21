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
}

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
    }

    fn detach(&mut self, world: &mut ipp_core::WorldContext<'_>) {
        self.renderer.set_asset_context_active(false);
        self.renderer.unload(world);
        self.active = false;
        self.tick = 0;
        self.stats = RenderStats::default();
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
