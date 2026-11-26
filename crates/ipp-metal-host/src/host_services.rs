use std::{path::PathBuf, sync::OnceLock};

use ipp_core::{HostRuntime, OutputRef, WorldPublicationId, WorldViewport};
use ipp_host_session::{
    HostPresentationFailure, HostServices, PresentationCompletion, PresentationDrawSummary,
};
use ipp_protocol::{
    host::presentation::{PresentationError, PresentationSurface},
    world::RuntimeFailureScope,
};
use ipp_render_metal::{MetalPresentation, MetalRenderError, MetalRenderService};
use ipp_server::services::{NativeHostServices, io::FileSystemIoSource};

use crate::cef::{CefProvider, CefSetup};

static CEF: OnceLock<CefSetup> = OnceLock::new();
static READ_SOURCES: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();

pub(super) fn set_cef(setup: CefSetup) -> Result<(), &'static str> {
    CEF.set(setup)
        .map_err(|_| "Metal Host CEF already configured")
}

pub(super) fn set_read_sources(sources: Vec<(String, PathBuf)>) -> Result<(), &'static str> {
    READ_SOURCES
        .set(sources)
        .map_err(|_| "Metal Host sources already configured")
}

pub(super) struct MetalHostServices {
    cef: Option<CefProvider>,
    native: NativeHostServices,
    renderer: MetalRenderService,
    context: u64,
    recovery_pending: bool,
    #[cfg(feature = "instrumentation")]
    profile: Option<crate::profiling::MetalProfileCapture>,
    #[cfg(feature = "instrumentation")]
    frame: u64,
    #[cfg(feature = "instrumentation")]
    test_recovery_frame: Option<u64>,
}

impl HostServices for MetalHostServices {
    const NAME: &'static str = "metal";

    fn initialize(
        host: &mut HostRuntime,
        schedulers: &ipp_host_session::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        let native = NativeHostServices::initialize(host, schedulers)?;
        for (prefix, root) in READ_SOURCES.get().into_iter().flatten() {
            let source = FileSystemIoSource::new(prefix, root.clone(), false, schedulers.io())?;
            host.io_mut().register(prefix, source)?;
        }
        let renderer = MetalRenderService::install(host)?;
        let cef = CEF
            .get()
            .map(|setup| CefProvider::new(host, setup))
            .transpose()?;
        ipp_core::diagnostic!(Info, "[IPP Metal] device={}", renderer.device_name());
        Ok(Self {
            cef,
            native,
            renderer,
            context: 1,
            recovery_pending: false,
            #[cfg(feature = "instrumentation")]
            profile: None,
            #[cfg(feature = "instrumentation")]
            frame: 0,
            #[cfg(feature = "instrumentation")]
            test_recovery_frame: crate::presentation_testing::recovery_frame(),
        })
    }

    fn gui_input(
        &mut self,
    ) -> Option<&mut ipp_host_session::services::gui_input::GuiHostInputService> {
        self.native.gui_input()
    }

    fn service_resources(&mut self, host: &mut HostRuntime) -> Result<(), String> {
        self.native.service_resources(host)?;
        if let Some(cef) = &mut self.cef {
            cef.tick(host, &mut self.renderer)?;
        }
        Ok(())
    }

    fn presentation_surface(&self) -> Result<PresentationSurface, PresentationError> {
        Ok(PresentationSurface {
            id: 1,
            context: self.context,
            max_width: MetalRenderService::MAX_VIEWPORT,
            max_height: MetalRenderService::MAX_VIEWPORT,
        })
    }

    fn prepare_presentation(
        &mut self,
        host: &mut HostRuntime,
        _: Option<(OutputRef, WorldPublicationId)>,
    ) -> Result<(), HostPresentationFailure> {
        if self.recovery_pending {
            self.renderer
                .recover(host)
                .map_err(|error| HostPresentationFailure {
                    scope: RuntimeFailureScope::Context,
                    message: error.to_string(),
                })?;
            self.context = self
                .context
                .checked_add(1)
                .ok_or_else(|| HostPresentationFailure {
                    scope: RuntimeFailureScope::Context,
                    message: "Metal context identity exhausted".into(),
                })?;
            self.recovery_pending = false;
        }
        Ok(())
    }

    fn configure_presentation(&mut self, viewport: WorldViewport) -> Result<(), PresentationError> {
        self.renderer
            .configure(viewport)
            .map_err(|error| match error {
                MetalRenderError::InvalidViewport => PresentationError::InvalidViewport,
                _ => {
                    self.recovery_pending = true;
                    PresentationError::Unavailable
                }
            })
    }

    #[cfg(feature = "instrumentation")]
    fn render_profile_start(
        &mut self,
        capture: u64,
        host: u64,
        options: ipp_protocol::host::profiling::ProfileRenderOptions,
    ) -> ipp_protocol::host::profiling::ProfileGpuCapability {
        let enabled = self.renderer.set_gpu_timing(
            options.gpu == ipp_protocol::host::profiling::ProfileGpuSampling::Frame,
        );
        let profile =
            crate::profiling::MetalProfileCapture::new(capture, host, options.gpu, enabled);
        let capability = profile.capability();
        self.profile = Some(profile);
        capability
    }

    #[cfg(feature = "instrumentation")]
    fn render_profile_stop(
        &mut self,
        capture: u64,
    ) -> ipp_protocol::host::profiling::ProfileGpuCapture {
        self.renderer.set_gpu_timing(false);
        self.profile
            .take()
            .expect("active Metal capture")
            .finish(capture)
    }

    #[cfg(feature = "instrumentation")]
    fn render_profile_cancel(&mut self, capture: u64) {
        if self
            .profile
            .as_ref()
            .is_some_and(|profile| profile.matches(capture))
        {
            self.renderer.set_gpu_timing(false);
            self.profile = None;
        }
    }

    fn present(
        &mut self,
        host: &HostRuntime,
        output: OutputRef,
        publication: WorldPublicationId,
        viewport: WorldViewport,
        time: f64,
        completion: PresentationCompletion<'_>,
    ) -> Result<PresentationDrawSummary, HostPresentationFailure> {
        #[cfg(feature = "instrumentation")]
        let reads_pixels = completion.capture.is_some();
        let frame = self
            .renderer
            .draw(
                host,
                MetalPresentation {
                    output,
                    publication,
                    viewport,
                    time,
                },
                completion.capture,
                completion.outputs,
            )
            .map_err(|error| {
                let scope = match error {
                    MetalRenderError::Device(_) => {
                        self.recovery_pending = true;
                        RuntimeFailureScope::Context
                    }
                    _ => RuntimeFailureScope::Draw,
                };
                HostPresentationFailure {
                    scope,
                    message: error.to_string(),
                }
            })?;
        #[cfg(feature = "instrumentation")]
        {
            self.frame = self.frame.saturating_add(1);
            if self.test_recovery_frame == Some(self.frame) {
                self.test_recovery_frame = None;
                self.recovery_pending = true;
            }
            if !reads_pixels && let Some(profile) = &mut self.profile {
                profile.record(self.frame, frame.gpu_elapsed_ns);
            }
        }
        Ok(PresentationDrawSummary {
            draw_calls: frame.draw_calls,
            triangles: frame.triangles,
            failed_draw_calls: frame.failed_draw_calls,
        })
    }
}
