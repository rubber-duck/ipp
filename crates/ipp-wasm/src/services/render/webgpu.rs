//! WebGPU presentation on the worker's transferred canvas.
use super::statistics::RenderStatisticsRecord;
use ipp_core::{HostRuntime, OutputRef, WorldPublicationId, WorldViewport};
use ipp_host_session::{HostPresentationFailure, PresentationCompletion, PresentationDrawSummary};
use ipp_protocol::{
    host::presentation::{PresentationError, PresentationSurface},
    world::RuntimeFailureScope,
};
use ipp_render_webgpu::{BrowserSurface, WebGpuRenderService};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static PENDING: RefCell<Option<(BrowserSurface,js_sys::Function)>> = const { RefCell::new(None) };
}

/// Acquire the GPU asynchronously, before entering the synchronous Host constructor.
#[wasm_bindgen]
pub async fn open_webgpu(
    canvas: web_sys::OffscreenCanvas,
    upload: js_sys::Function,
) -> Result<JsValue, JsValue> {
    let surface = BrowserSurface::open(canvas)
        .await
        .map_err(|e| JsValue::from_str(&e))?;
    let device = surface.device();
    PENDING.with_borrow_mut(|slot| *slot = Some((surface, upload)));
    Ok(device)
}

#[link(wasm_import_module = "ipp_presentation")]
unsafe extern "C" {
    fn capture(pointer: *mut u8, length: usize) -> u32;
}

pub(crate) struct RenderSurfaceService {
    renderer: WebGpuRenderService,
    pub(super) canvas: BrowserSurface,
    pub(super) upload: js_sys::Function,
    pub(super) web_content: super::web_content::BrowserWebContent,
    size: Option<(u32, u32)>,
    pub(super) statistics: RenderStatisticsRecord,
}

impl RenderSurfaceService {
    pub(crate) fn new(host: &mut HostRuntime) -> Result<Self, String> {
        let (canvas, upload) = PENDING
            .with_borrow_mut(Option::take)
            .ok_or("WebGPU canvas is unavailable")?;
        let renderer = canvas.install(host)?;
        Ok(Self {
            renderer,
            canvas,
            upload,
            web_content: Default::default(),
            size: None,
            statistics: RenderStatisticsRecord::new(),
        })
    }

    pub(super) fn attach(
        &mut self,
        _: &mut HostRuntime,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.configure(WorldViewport {
            width,
            height,
            device_pixel_ratio: 1.,
        })
        .map_err(|e| format!("{e:?}"))
    }

    pub(super) fn detach_host(&mut self, _: &mut HostRuntime) -> Result<(), String> {
        self.size = None;
        Ok(())
    }

    pub(crate) fn viewport(&self) -> Option<(u32, u32)> {
        self.size
    }

    pub(crate) fn configure(&mut self, viewport: WorldViewport) -> Result<(), PresentationError> {
        if !viewport.device_pixel_ratio.is_finite() || viewport.device_pixel_ratio <= 0.0 {
            return Err(PresentationError::InvalidViewport);
        }
        if self.size != Some((viewport.width, viewport.height)) {
            self.renderer
                .configure(viewport)
                .map_err(|_| PresentationError::InvalidViewport)?;
            self.canvas.configure(viewport.width, viewport.height);
            self.size = Some((viewport.width, viewport.height));
        }
        Ok(())
    }

    pub(crate) fn surface(&self) -> Result<PresentationSurface, PresentationError> {
        self.size.ok_or(PresentationError::Unavailable)?;
        let limit = self.renderer.max_extent();
        Ok(PresentationSurface {
            id: 1,
            context: 1,
            max_width: limit,
            max_height: limit,
        })
    }

    pub(crate) fn progress_resources(&mut self, host: &mut HostRuntime) {
        host.progress_assets();
        self.canvas.refresh_web_images(host);
    }

    pub(crate) fn prepare(
        &mut self,
        host: &mut HostRuntime,
        _: Option<(OutputRef, WorldPublicationId)>,
    ) -> Result<(), HostPresentationFailure> {
        self.canvas.refresh_web_images(host);
        Ok(())
    }

    pub(crate) fn present(
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
            .draw(host, output, publication, viewport, time, &self.canvas)
            .map_err(|e| HostPresentationFailure {
                scope: match e {
                    ipp_render_webgpu::RenderError::Device(_) => RuntimeFailureScope::Context,
                    _ => RuntimeFailureScope::Draw,
                },
                message: e.to_string(),
            })?;
        for observed in completion.outputs {
            observed.publication = (frame.failed == 0)
                .then(|| frame.included.get(&observed.output).copied())
                .flatten();
        }
        if let Some(pixels) = completion.capture {
            // SAFETY: The worker copies synchronously into this exclusive slice and never reenters WASM or retains a pointer.
            if unsafe { capture(pixels.as_mut_ptr(), pixels.len()) } != 1 {
                return Err(HostPresentationFailure {
                    scope: RuntimeFailureScope::Context,
                    message: "WebGPU frame readback failed".into(),
                });
            }
        }
        Ok(PresentationDrawSummary {
            draw_calls: frame.draws,
            triangles: frame.triangles,
            failed_draw_calls: frame.failed,
        })
    }

    pub(crate) fn record_frame(
        &mut self,
        host: &mut HostRuntime,
        frame: &ipp_core::HostFrameReport,
    ) {
        self.statistics.record_frame(host, frame);
    }

    pub(super) fn statistics_words(&mut self) -> &[u32] {
        self.statistics.with_layout()
    }

    pub(super) fn surface_cache_records(&self) -> &[u32] {
        &[]
    }

    pub(super) fn surface_record_count(&self) -> usize {
        0
    }
}
