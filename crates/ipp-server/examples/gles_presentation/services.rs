//! Host services that present the attached World through a GLES `RenderService`.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Instant;

use ipp_core::{HostRuntime, WorldContext, WorldId};
use ipp_host_session::{HostPresentationFailure, HostServices};
use ipp_render_gl::{GlesRenderDevice, RenderError, RenderFrameSummary, RenderService};
use ipp_server::services::NativeHostServices;

use super::channel::{PresentationControl, PresentationOutput};
use super::statistics::{FrameHeader, StatisticsTotals};
use crate::egl::Context;

/// A pending frame request of the presentation client.
struct FrameRequest {
    session: u64,
    after_tick: u64,
    readback: bool,
}

/// Native Host services plus one GLES presentation of the attached World.
pub(crate) struct GlesHostServices {
    native: NativeHostServices,
    // Declared before `context`: the renderer releases GPU state first.
    renderer: RenderService<GlesRenderDevice>,
    context: Context,
    control: Receiver<PresentationControl>,
    output: Sender<PresentationOutput>,
    /// The single presented World, as a worker presents its connection's World.
    world: Option<WorldId>,
    /// Whether the renderer is attached; false while a simulated loss lasts.
    active: bool,
    /// Attachments so far; a restore after a simulated loss increments it.
    generation: u32,
    requested: (u32, u32),
    viewport: (u32, u32),
    limits: (u32, u32),
    requests: BTreeMap<u32, FrameRequest>,
    /// World tick and summary of the last completed render; zero before it.
    tick: u64,
    summary: RenderFrameSummary,
    totals: StatisticsTotals,
    device: [String; 3],
}

impl HostServices for GlesHostServices {
    const NAME: &'static str = "gles";

    fn initialize(host: &mut HostRuntime) -> Result<Self, String> {
        let native = NativeHostServices::initialize(host)?;
        let setup = super::take_setup()?;
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
        let renderer = RenderService::new(context.device().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        renderer.install(host).map_err(|error| error.to_string())?;
        renderer.set_asset_context_active(true);

        let mut services = Self {
            native,
            renderer,
            context,
            control: setup.control,
            output: setup.output,
            world: None,
            active: true,
            generation: 1,
            requested: (320, 240),
            viewport: (320, 240),
            limits: (0, 0),
            requests: BTreeMap::new(),
            tick: 0,
            summary: RenderFrameSummary::default(),
            totals: StatisticsTotals::default(),
            device: [0, 1, 2].map(|index| device.get(index).cloned().unwrap_or_default()),
        };
        services.refresh_limits();
        Ok(services)
    }

    fn render_viewport(&self) -> Option<(u32, u32)> {
        self.active.then_some(self.viewport)
    }

    fn attach_world(&mut self, world: WorldId) -> Result<(), String> {
        if let Some(previous) = self.world.replace(world)
            && previous != world
        {
            self.renderer.forget_world(previous);
        }
        self.tick = 0;
        Ok(())
    }

    fn detach_world(&mut self, world: WorldId) {
        if self.world == Some(world) {
            self.world = None;
            self.renderer.forget_world(world);
            self.tick = 0;
        }
    }

    fn present(&mut self, world: &mut WorldContext<'_>) -> Result<(), HostPresentationFailure> {
        if !self.active || self.world != Some(world.id()) {
            return Ok(());
        }

        let (width, height) = self.viewport;
        match self.renderer.render(world, width, height) {
            Ok(summary) => {
                self.summary = summary;
                self.tick = world.tick();
                self.totals.accumulate(self.renderer.statistics());
                #[cfg(feature = "gui")]
                self.totals.record_layout(
                    world
                        .system::<ipp_core::GuiLayoutSystem>(ipp_core::GuiLayoutSystem::ID)
                        .map(ipp_core::GuiLayoutSystem::statistics)
                        .unwrap_or_default(),
                );
                self.answer_requests(world.id());
                Ok(())
            }
            Err(error) => Err(HostPresentationFailure {
                scope: match error {
                    RenderError::ContextLost => ipp_protocol::RuntimeFailureScope::Context,
                    RenderError::MissingMesh | RenderError::MissingTexture => {
                        ipp_protocol::RuntimeFailureScope::Resource
                    }
                    _ => ipp_protocol::RuntimeFailureScope::Draw,
                },
                message: error.to_string(),
            }),
        }
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
            PresentationControl::Frame {
                id,
                session,
                after_tick,
                readback,
            } => {
                if self.requests.len() >= 4 || self.requests.contains_key(&id) {
                    let _ = self.output.send(PresentationOutput::FrameError {
                        id,
                        message: "Frame request queue is full".into(),
                    });
                } else {
                    self.requests.insert(
                        id,
                        FrameRequest {
                            session,
                            after_tick,
                            readback,
                        },
                    );
                }
            }
            PresentationControl::Cancel {
                id,
            } => {
                self.requests.remove(&id);
            }
            PresentationControl::Disconnected => self.requests.clear(),
            PresentationControl::Resize {
                width,
                height,
            } => {
                if width == 0 || height == 0 {
                    return Err("Viewport dimensions must be positive integers".into());
                }
                self.requested = (width, height);
                self.viewport = self.bounded(self.requested);
            }
            PresentationControl::GlyphAtlasLimits {
                max_pages,
                idle_page_publications,
            } => {
                #[cfg(feature = "gui")]
                self.renderer
                    .set_glyph_atlas_limits(ipp_render_gl::GlyphAtlasLimits {
                        max_pages: max_pages as usize,
                        idle_page_publications: u64::from(idle_page_publications),
                    });

                #[cfg(not(feature = "gui"))]
                {
                    let _ = (max_pages, idle_page_publications);
                    return Err(
                        "The glyph atlas limits testing override requires a GUI build".into(),
                    );
                }
            }
            PresentationControl::SurfaceCacheBudget {
                bytes,
            } => {
                self.renderer.set_surface_cache_budget(bytes as usize);
            }
            PresentationControl::ExhaustiveDrawChecks {
                enabled,
            } => {
                self.renderer.set_exhaustive_draw_checks(enabled);
            }
            PresentationControl::ContextLoss => {
                if self.active {
                    // The same recovery path as the WASM detach export: context
                    // state goes, the Host and its Worlds keep their logical assets.
                    self.renderer.set_asset_context_active(false);
                    self.renderer.unload_host(host);
                    host.flush_resource_lifecycle();
                    self.active = false;
                    self.tick = 0;
                }
            }
            PresentationControl::ContextRestore => {
                if !self.active {
                    self.renderer.set_asset_context_active(true);
                    self.active = true;
                    self.generation += 1;
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
        self.viewport = self.bounded(self.requested);
    }

    /// The worker's `boundViewport`: scale uniformly into the limits, keeping the aspect.
    fn bounded(&self, (width, height): (u32, u32)) -> (u32, u32) {
        let (max_width, max_height) = self.limits;
        let scale = (f64::from(max_width) / f64::from(width))
            .min(f64::from(max_height) / f64::from(height))
            .min(1.0);
        if scale >= 1.0 {
            return (width, height);
        }

        (
            ((f64::from(width) * scale).floor() as u32).clamp(1, max_width),
            ((f64::from(height) * scale).floor() as u32).clamp(1, max_height),
        )
    }

    /// Answer every request the frame just rendered satisfies, in request order.
    fn answer_requests(&mut self, world: WorldId) {
        if self.requests.is_empty() {
            return;
        }

        let ready: Vec<u32> = self
            .requests
            .iter()
            .filter(|(_, request)| self.tick >= request.after_tick)
            .map(|(&id, _)| id)
            .collect();
        for id in ready {
            let request = self.requests.remove(&id).expect("ready request");
            let message = match self.frame(world, &request) {
                Ok((header, pixels)) => PresentationOutput::Frame {
                    id,
                    header,
                    pixels,
                },
                Err(message) => PresentationOutput::FrameError {
                    id,
                    message,
                },
            };
            let _ = self.output.send(message);
        }
    }

    fn frame(
        &mut self,
        world: WorldId,
        request: &FrameRequest,
    ) -> Result<(String, Option<Vec<u8>>), String> {
        let (width, height) = self.viewport;
        let mut header = FrameHeader {
            session: request.session,
            tick: self.tick,
            width,
            height,
            summary: self.summary,
            context_generation: self.generation,
            statistics: None,
        };
        if !request.readback {
            return Ok((header.to_json(), None));
        }

        let started = Instant::now();
        let pixels = self
            .context
            .capture_region(width, height)
            .map_err(|error| error.to_string())?;
        let readback_ms = started.elapsed().as_secs_f64() * 1_000.0;
        header.statistics = Some(super::statistics::snapshot(
            &mut self.renderer,
            world,
            &self.totals,
            readback_ms,
            &self.device,
        ));
        Ok((header.to_json(), Some(pixels)))
    }
}
