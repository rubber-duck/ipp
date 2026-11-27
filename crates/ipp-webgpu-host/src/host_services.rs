use ipp_core::{HostRuntime, OutputRef, WorldPublicationId, WorldViewport};
use ipp_host_session::{
    HostPresentationFailure, HostServices, PresentationCompletion, PresentationDrawSummary,
    services::{gui_input::GuiHostInputService, task_scheduler::TaskSchedulers},
};
use ipp_protocol::{
    host::presentation::{PresentationError, PresentationSurface},
    world::RuntimeFailureScope,
};
use ipp_render_webgpu::{RenderError, WebGpuRenderService};
use ipp_server::services::NativeHostServices;

use ipp_server::services::io::FileSystemIoSource;
use std::{path::PathBuf, sync::OnceLock};
static READ_SOURCES: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();

pub(super) fn set_read_sources(sources: Vec<(String, PathBuf)>) -> Result<(), &'static str> {
    READ_SOURCES
        .set(sources)
        .map_err(|_| "WebGPU Host sources already configured")
}

pub(super) struct WebGpuHostServices {
    native: NativeHostServices,
    renderer: WebGpuRenderService,
    context: u64,
    recovery_pending: bool,
}

impl HostServices for WebGpuHostServices {
    const NAME: &'static str = "webgpu";

    fn initialize(host: &mut HostRuntime, schedulers: &TaskSchedulers) -> Result<Self, String> {
        let native = NativeHostServices::initialize(host, schedulers)?;
        for (prefix, root) in READ_SOURCES.get().into_iter().flatten() {
            let source = FileSystemIoSource::new(prefix, root.clone(), false, schedulers.io())?;
            host.io_mut().register(prefix, source)?;
        }
        let renderer = WebGpuRenderService::install(host)?;
        ipp_core::diagnostic!(Info, "[IPP WebGPU] device={}", renderer.device_name());
        Ok(Self {
            native,
            renderer,
            context: 1,
            recovery_pending: false,
        })
    }

    fn gui_input(&mut self) -> Option<&mut GuiHostInputService> {
        self.native.gui_input()
    }

    fn service_resources(&mut self, host: &mut HostRuntime) -> Result<(), String> {
        self.native.service_resources(host)
    }

    fn presentation_surface(&self) -> Result<PresentationSurface, PresentationError> {
        Ok(PresentationSurface {
            id: 1,
            context: self.context,
            max_width: self.renderer.max_extent(),
            max_height: self.renderer.max_extent(),
        })
    }

    fn prepare_presentation(
        &mut self,
        host: &mut HostRuntime,
        _: Option<(OutputRef, WorldPublicationId)>,
    ) -> Result<(), HostPresentationFailure> {
        if self.recovery_pending {
            let failure = |message| HostPresentationFailure {
                scope: RuntimeFailureScope::Context,
                message,
            };
            self.renderer.recover(host).map_err(failure)?;
            self.context = self
                .context
                .checked_add(1)
                .ok_or_else(|| failure("WebGPU context identity exhausted".into()))?;
            self.recovery_pending = false;
        }
        Ok(())
    }

    fn configure_presentation(&mut self, viewport: WorldViewport) -> Result<(), PresentationError> {
        self.renderer.configure(viewport).map_err(|e| match e {
            RenderError::InvalidViewport => PresentationError::InvalidViewport,
            RenderError::Device(_) => {
                self.recovery_pending = true;
                PresentationError::Unavailable
            }
            RenderError::Draw(_) => PresentationError::Unavailable,
        })
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
        let frame = self
            .renderer
            .draw(
                host,
                output,
                publication,
                viewport,
                time,
                completion.capture,
            )
            .map_err(|e| {
                let scope = if matches!(e, RenderError::Device(_)) {
                    self.recovery_pending = true;
                    RuntimeFailureScope::Context
                } else {
                    RuntimeFailureScope::Draw
                };
                HostPresentationFailure {
                    scope,
                    message: e.to_string(),
                }
            })?;
        for observation in completion.outputs {
            observation.publication = if frame.failed == 0 {
                frame.included.get(&observation.output).copied()
            } else {
                None
            };
        }
        Ok(PresentationDrawSummary {
            draw_calls: frame.draws,
            triangles: frame.triangles,
            failed_draw_calls: frame.failed,
        })
    }
}
