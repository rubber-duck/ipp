//! The dedicated worker's presentation surface. Composition stays in the shared renderer.
use crate::{
    RenderError, WebGpuRenderService,
    device::{Graphics, Target},
};
use std::rc::Rc;

/// One WebGPU device and its transferred browser canvas.
pub struct BrowserSurface {
    pub(super) gpu: Rc<Graphics>,
    surface: wgpu::Surface<'static>,
}

impl BrowserSurface {
    /// Request the browser adapter and validate the shared pipelines before admitting the Host.
    pub async fn open(canvas: web_sys::OffscreenCanvas) -> Result<Self, String> {
        let (width, height) = (canvas.width(), canvas.height());
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas))
            .map_err(|e| e.to_string())?;
        let gpu = Rc::new(Graphics::open(&instance, Some(&surface)).await?);
        let result = Self {
            gpu,
            surface,
        };
        let validation = result
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        result.configure(width, height);
        // Configuration and acquisition may fail even after device creation.
        let frame = result.acquire().map_err(|e| e.to_string())?;
        result.gpu.queue.present(frame);
        if let Some(error) = validation.pop().await {
            return Err(error.to_string());
        }
        result.gpu.check()?;
        Ok(result)
    }

    /// Expose this exact device to the worker for device-loss observation and HTML image imports.
    pub fn device(&self) -> wasm_bindgen::JsValue {
        self.gpu
            .device
            .as_webgpu()
            .expect("browser device")
            .clone()
            .into()
    }

    /// Register the shared mesh, texture and shader loaders for this device.
    pub fn install(&self, host: &mut ipp_core::HostRuntime) -> Result<WebGpuRenderService, String> {
        WebGpuRenderService::with_graphics(host, self.gpu.clone())
    }

    /// Resize and configure the canvas for the IPP top-left, encoded RGBA output.
    pub fn configure(&self, width: u32, height: u32) {
        self.surface.configure(
            &self.gpu.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Rgba8Unorm,
                color_space: wgpu::SurfaceColorSpace::Srgb,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                desired_maximum_frame_latency: 2,
                view_formats: vec![],
            },
        );
    }

    fn acquire(&self) -> Result<wgpu::SurfaceTexture, RenderError> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(frame),
            status => Err(RenderError::Device(format!(
                "Canvas acquisition failed: {status:?}"
            ))),
        }
    }

    pub(super) fn present(
        &self,
        target: &Target,
        mut encoder: wgpu::CommandEncoder,
    ) -> Result<(), RenderError> {
        let frame = self.acquire()?;
        encoder.copy_texture_to_texture(
            target.color.as_image_copy(),
            frame.texture.as_image_copy(),
            target.color.size(),
        );
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(frame);
        Ok(())
    }
}
