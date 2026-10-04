//! One GLES surface selected by the Host presentation coordinator, independent of sessions.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Instant;

use ipp_core::{HostRuntime, OutputRef, WorldPublicationId, WorldViewport};
use ipp_host_session::{HostPresentationFailure, HostServices, PresentationDrawSummary};
use ipp_protocol::presentation::{PresentationError, PresentationSurface};
use ipp_render_gl::{GlesRenderDevice, RenderError, RenderService};
use ipp_server::services::NativeHostServices;

use super::channel::{PresentationControl, PresentationOutput, PresentationTesting};
use super::statistics::StatisticsTotals;
use crate::egl::Context;

/// Native Host services plus one GLES presentation of the attached World.
pub(crate) struct GlesHostServices {
    native: NativeHostServices,
    // Declared before `context`: the renderer releases GPU state first.
    renderer: RenderService<GlesRenderDevice>,
    #[cfg(feature = "instrumentation")]
    profile: Option<super::profiling::RenderProfileCapture>,
    context: Context,
    control: Receiver<PresentationControl>,
    output: Sender<PresentationOutput>,
    rendered_output: Option<OutputRef>,
    /// Whether the renderer is attached; false while a simulated loss lasts.
    active: bool,
    /// Attachments so far; a restore after a simulated loss increments it.
    generation: u64,
    viewport: (u32, u32),
    limits: (u32, u32),
    totals: StatisticsTotals,
    device: [String; 3],
    readback_ms: f64,
}

impl HostServices for GlesHostServices {
    const NAME: &'static str = "gles";

    fn asset_gpu_formats(
        &self,
        kind: ipp_core::services::asset_management::AssetTypeId,
    ) -> Vec<ipp_core::services::asset_management::export::AssetExportFormat> {
        self.renderer.asset_export_formats(kind)
    }

    fn asset_gpu_export(
        &mut self,
        provider: &ipp_core::services::asset_management::AssetProvider,
        format: ipp_core::services::asset_management::export::AssetExportFormat,
        observer: std::rc::Rc<
            dyn ipp_core::services::asset_management::export::AssetOutputObserver,
        >,
    ) -> Result<ipp_core::services::asset_management::export::AssetExportFuture, String> {
        self.renderer.export_asset(provider, format, observer)
    }

    #[cfg(feature = "instrumentation")]
    fn render_profile_start(
        &mut self,
        capture: u64,
        host: u64,
        options: ipp_protocol::profiling::ProfileRenderOptions,
    ) -> ipp_protocol::profiling::ProfileGpuCapability {
        let state = super::profiling::RenderProfileCapture::start(
            &mut self.renderer,
            capture,
            host,
            options,
        );
        let capability = state.capability();
        self.profile = Some(state);
        capability
    }

    #[cfg(feature = "instrumentation")]
    fn render_profile_stop(&mut self, capture: u64) -> ipp_protocol::profiling::ProfileGpuCapture {
        let state = self
            .profile
            .take()
            .expect("accepted Host capture owns render profile");
        assert!(state.matches(capture), "Host capture generation mismatch");
        state.stop(&mut self.renderer)
    }

    #[cfg(feature = "instrumentation")]
    fn render_profile_cancel(&mut self, capture: u64) {
        if self
            .profile
            .as_ref()
            .is_some_and(|state| state.matches(capture))
        {
            self.profile.take().unwrap().cancel(&mut self.renderer);
        }
    }

    fn record_frame(&mut self, host: &mut HostRuntime, frame: &ipp_core::HostFrameReport) {
        self.totals.record_frame(host, frame);
    }

    fn gui_input(
        &mut self,
    ) -> Option<&mut ipp_host_session::services::gui_input::GuiHostInputService> {
        self.native.gui_input()
    }

    fn initialize(
        host: &mut HostRuntime,
        schedulers: &ipp_host_session::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        let native = NativeHostServices::initialize(host, schedulers)?;
        let setup = super::take_setup()?;
        for (prefix, root) in setup.read_sources {
            let source = ipp_server::services::io::FileSystemIoSource::new(
                &prefix,
                root,
                false,
                schedulers.io(),
            )?;
            host.io_mut().register(&prefix, source)?;
        }

        let context = Context::new(
            &setup.egl_directory,
            super::PBUFFER_SIZE,
            super::PBUFFER_SIZE,
        )
        .map_err(|error| error.to_string())?;
        let device = context
            .info()
            .map_err(|error| error.to_string())?
            .lines()
            .map(|line| {
                line.split_once(": ")
                    .map_or(line, |(_, value)| value)
                    .to_owned()
            })
            .collect::<Vec<_>>();
        let mut renderer = RenderService::new(context.device().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        renderer.set_asset_export_delay(std::rc::Rc::new(
            ipp_host_session::services::task_scheduler::native_delay,
        ));
        renderer.install(host).map_err(|error| error.to_string())?;
        renderer.set_asset_context_active(true);

        let mut services = Self {
            native,
            renderer,
            #[cfg(feature = "instrumentation")]
            profile: None,
            context,
            control: setup.control,
            output: setup.output,
            rendered_output: None,
            active: true,
            generation: 1,
            viewport: (320, 240),
            limits: (0, 0),
            totals: StatisticsTotals::default(),
            device: [0, 1, 2].map(|index| device.get(index).cloned().unwrap_or_default()),
            readback_ms: 0.0,
        };
        services.refresh_limits();
        Ok(services)
    }

    fn render_viewport(&self) -> Option<(u32, u32)> {
        self.active.then_some(self.viewport)
    }

    fn presentation_surface(&self) -> Result<PresentationSurface, PresentationError> {
        if !self.active {
            return Err(PresentationError::Unavailable);
        }
        Ok(PresentationSurface {
            id: 1,
            context: self.generation,
            max_width: self.limits.0,
            max_height: self.limits.1,
        })
    }

    fn configure_presentation(&mut self, viewport: WorldViewport) -> Result<(), PresentationError> {
        self.presentation_surface()?;
        if viewport.width == 0
            || viewport.height == 0
            || viewport.width > self.limits.0
            || viewport.height > self.limits.1
            || !viewport.device_pixel_ratio.is_finite()
            || viewport.device_pixel_ratio <= 0.0
        {
            return Err(PresentationError::InvalidViewport);
        }
        self.viewport = (viewport.width, viewport.height);
        Ok(())
    }

    fn prepare_presentation(
        &mut self,
        host: &mut HostRuntime,
        selected: Option<(OutputRef, WorldPublicationId)>,
    ) -> Result<(), HostPresentationFailure> {
        self.renderer
            .prepare(host, selected)
            .map_err(presentation_failure)
    }

    fn present(
        &mut self,
        host: &HostRuntime,
        output: OutputRef,
        publication: WorldPublicationId,
        viewport: WorldViewport,
        presentation_time: f64,
        completion: ipp_host_session::PresentationCompletion<'_>,
    ) -> Result<PresentationDrawSummary, HostPresentationFailure> {
        let ipp_host_session::PresentationCompletion {
            capture,
            outputs,
        } = completion;
        if !self.active {
            return Err(presentation_failure(RenderError::ContextLost));
        }
        if self.viewport != (viewport.width, viewport.height) {
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
        self.totals.accumulate(self.renderer.statistics());
        self.rendered_output = Some(output);
        if let Some(capture) = capture {
            let started = Instant::now();
            let pixels = self
                .context
                .capture_region(viewport.width, viewport.height)
                .map_err(|error| HostPresentationFailure {
                    scope: ipp_protocol::RuntimeFailureScope::Context,
                    message: error.to_string(),
                })?;
            if pixels.len() != capture.len() {
                return Err(presentation_failure(RenderError::InvalidViewport));
            }
            capture.copy_from_slice(&pixels);
            self.readback_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        Ok(PresentationDrawSummary {
            draw_calls: summary.draw_calls,
            triangles: summary.triangles,
            failed_draw_calls: summary.failed_draw_calls,
        })
    }

    fn prepare_task_poll(&mut self, host: &mut HostRuntime) -> Result<(), String> {
        if let Err(error) = self.context.ensure_current() {
            self.renderer.set_asset_context_active(false);
            // Invalidate publication authority before any already completed GPU
            // export can detach. The inactive renderer lease skips unsafe GL deletes.
            let keys: Vec<_> = host
                .asset_resources()
                .iter()
                .filter(|provider| provider.graphics_ready().is_some())
                .map(|provider| provider.key())
                .collect();
            for key in keys {
                host.asset_resources_mut().invalidate_graphics(key);
            }
            host.flush_resource_lifecycle();
            if self.active {
                self.active = false;
                let _ = self
                    .output
                    .send(PresentationOutput::Failure(error.to_string()));
            }
            // CPU and I/O tasks remain runnable. GPU loaders await recovery;
            // unfinished typed exports lose availability and fail.
            return Ok(());
        }

        self.receive_controls(host);
        Ok(())
    }

    fn service_resources(&mut self, host: &mut HostRuntime) -> Result<(), String> {
        self.native.service_resources(host)
    }

    fn progress_assets(&mut self, host: &mut HostRuntime) {
        self.receive_controls(host);
        self.progress_resources(host);
    }

    fn progress_resources(&mut self, host: &mut HostRuntime) {
        if self.active {
            host.progress_assets();
        } else {
            host.progress_evaluation_assets();
        }
    }
}

impl GlesHostServices {
    /// Apply client requests at the Host frame boundary, before evaluation.
    fn receive_controls(&mut self, host: &mut HostRuntime) {
        loop {
            let control = match self.control.try_recv() {
                Ok(control) => control,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            };
            if let Err(message) = self.apply(control, host) {
                let _ = self.output.send(PresentationOutput::Failure(message));
            }
        }
    }

    fn apply(
        &mut self,
        control: PresentationControl,
        host: &mut HostRuntime,
    ) -> Result<(), String> {
        match control {
            PresentationControl::Statistics {
                id,
            } => {
                let output = self.rendered_output.ok_or("no completed presentation")?;
                let statistics = super::statistics::snapshot(
                    &mut self.renderer,
                    output.world().id(),
                    &self.totals,
                    self.readback_ms,
                    &self.device,
                );
                let _ = self.output.send(PresentationOutput::Statistics {
                    id,
                    snapshot: statistics,
                });
                Ok(())
            }
            PresentationControl::Testing(control) => self.apply_testing(control, host),
        }
    }

    #[cfg(not(feature = "instrumentation"))]
    fn apply_testing(
        &mut self,
        _control: PresentationTesting,
        _host: &mut HostRuntime,
    ) -> Result<(), String> {
        Err(
            "presentation testing controls require an instrumentation build of the GLES test host"
                .into(),
        )
    }

    #[cfg(feature = "instrumentation")]
    fn apply_testing(
        &mut self,
        control: PresentationTesting,
        host: &mut HostRuntime,
    ) -> Result<(), String> {
        match control {
            PresentationTesting::GlyphAtlasLimits {
                max_pages,
                idle_page_frames,
            } => {
                self.renderer
                    .set_glyph_atlas_limits(ipp_render_gl::GlyphAtlasLimits {
                        max_pages: max_pages as usize,
                        idle_page_frames: u64::from(idle_page_frames),
                    });
            }
            PresentationTesting::SurfaceCacheBudget {
                bytes,
            } => {
                self.renderer.set_surface_cache_budget(bytes as usize);
            }
            PresentationTesting::ExhaustiveDrawChecks {
                enabled,
            } => {
                self.renderer.set_exhaustive_draw_checks(enabled);
            }
            PresentationTesting::ContextLoss => {
                if self.active {
                    #[cfg(feature = "instrumentation")]
                    self.renderer.invalidate_gpu_context();
                    // The same recovery path as the WASM detach export: context
                    // state goes, the Host and its Worlds keep their logical assets.
                    self.renderer
                        .prepare(host, None)
                        .map_err(|error| error.to_string())?;
                    self.renderer
                        .unload_host(host)
                        .map_err(|error| error.to_string())?;
                    self.renderer.set_asset_context_active(false);
                    host.flush_resource_lifecycle();
                    self.active = false;
                    self.rendered_output = None;
                }
            }
            PresentationTesting::ContextRestore => {
                if !self.active {
                    self.generation = self
                        .generation
                        .checked_add(1)
                        .ok_or("presentation context identity exhausted")?;
                    self.renderer.set_asset_context_active(true);
                    self.active = true;
                    self.refresh_limits();
                }
            }
        }

        Ok(())
    }

    /// Report the device limits, bounded by the pbuffer, and re-bound the viewport.
    fn refresh_limits(&mut self) {
        let device = self.renderer.viewport_limits();
        let limits = (
            device.map_or(super::PBUFFER_SIZE, |limits| {
                limits.max_width.min(super::PBUFFER_SIZE)
            }),
            device.map_or(super::PBUFFER_SIZE, |limits| {
                limits.max_height.min(super::PBUFFER_SIZE)
            }),
        );
        if limits != self.limits {
            self.limits = limits;
            let _ = self.output.send(PresentationOutput::ViewportLimits {
                max_width: limits.0,
                max_height: limits.1,
            });
        }
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
