use crate::{
    assets,
    device::{Graphics, PALETTE_BYTES, Target},
};
use ipp_core::{HostRuntime, OutputRef, WorldPublicationId, WorldViewport};
use std::{cell::RefCell, fmt, rc::Rc};
#[cfg(not(target_arch = "wasm32"))]
use std::{collections::VecDeque, sync::mpsc};

/// A failed submission preserves the Host and its World publications.
#[derive(Debug)]
pub enum RenderError {
    /// The binding exceeds the negotiated extent.
    InvalidViewport,
    /// An unavailable output or unsupported draw.
    Draw(&'static str),
    /// Device allocation, submission or readback failed.
    Device(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidViewport => f.write_str("Invalid WebGPU viewport"),
            Self::Draw(m) => f.write_str(m),
            Self::Device(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for RenderError {}

/// GPU work and output provenance for a composed root draw.
#[derive(Default)]
pub struct FrameSummary {
    /// Submitted draw calls.
    pub draws: u32,
    /// Submitted triangle instances.
    pub triangles: u32,
    /// Unavailable draws.
    pub failed: u32,
    /// Output publications that contributed to the rendered frame.
    pub included: std::collections::BTreeMap<OutputRef, WorldPublicationId>,
}

struct Submission {
    gpu: Rc<Graphics>,
    target: Option<Target>,
    white: wgpu::BindGroup,
    identity: wgpu::BindGroup,
    composition: crate::composition::Composition,
    #[cfg(not(target_arch = "wasm32"))]
    pending: VecDeque<wgpu::SubmissionIndex>,
}

impl Submission {
    fn new(gpu: Rc<Graphics>) -> Result<Self, String> {
        let (white, canvas_white, _) = gpu
            .texture(1, 1, &[255; 4])
            .ok_or("Could not create default texture")?;
        let (_, identity) = gpu.uniform(2, PALETTE_BYTES);
        gpu.check()?;
        let composition = crate::composition::Composition::new(&gpu, canvas_white);
        Ok(Self {
            gpu,
            target: None,
            white,
            identity,
            composition,
            #[cfg(not(target_arch = "wasm32"))]
            pending: VecDeque::new(),
        })
    }
}

/// Owns the WebGPU device, uploaded assets and offscreen targets for one Host.
pub struct WebGpuRenderService {
    #[cfg(not(target_arch = "wasm32"))]
    loaders: Rc<RefCell<Rc<Graphics>>>,
    state: Submission,
}

impl WebGpuRenderService {
    /// Open a verified hardware Metal adapter and register GPU asset loaders.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn install(host: &mut HostRuntime) -> Result<Self, String> {
        Self::with_graphics(host, Rc::new(Graphics::new()?))
    }

    pub(super) fn with_graphics(host: &mut HostRuntime, gpu: Rc<Graphics>) -> Result<Self, String> {
        let state = Submission::new(gpu.clone())?;
        let loaders = Rc::new(RefCell::new(gpu));
        let mesh = loaders.clone();
        host.asset_resources_mut()
            .register_graphics_loader(ipp_core::MESH_TYPE, move || {
                assets::mesh_loader(mesh.borrow().clone())
            })?;
        let texture = loaders.clone();
        host.asset_resources_mut()
            .register_graphics_loader(ipp_core::TEXTURE_TYPE, move || {
                assets::texture_loader(texture.borrow().clone())
            })?;
        let shader = loaders.clone();
        host.asset_resources_mut().register_graphics_loader(
            ipp_core::services::asset_management::formats::shader::SHADER_TYPE,
            move || crate::shaders::loader(shader.borrow().clone()),
        )?;
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            loaders,
            state,
        })
    }

    /// The hardware backend and device name reported by wgpu.
    pub fn device_name(&self) -> &str {
        &self.state.gpu.name
    }

    /// Limit bounded by both device capability and Host capture memory.
    pub fn max_extent(&self) -> u32 {
        self.state
            .gpu
            .device
            .limits()
            .max_texture_dimension_2d
            .min(4096)
    }

    /// Replace a failed device and invalidate its publication-scoped graphics leases.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn recover(&mut self, host: &mut HostRuntime) -> Result<(), String> {
        let gpu = Rc::new(Graphics::new()?);
        let mut replacement = Submission::new(gpu.clone())?;
        if let Some(previous) = &self.state.target {
            replacement.target = Some(Target::new(
                &gpu,
                [previous.color.width(), previous.color.height()],
            ));
        }
        gpu.check()?;
        self.loaders.replace(gpu);
        self.state = replacement;

        let resources = host.asset_resources_mut();
        let keys: Vec<_> = resources
            .iter()
            .filter(|r| {
                [
                    ipp_core::MESH_TYPE,
                    ipp_core::TEXTURE_TYPE,
                    ipp_core::services::asset_management::formats::shader::SHADER_TYPE,
                ]
                .contains(&r.source().kind)
            })
            .map(|r| r.key())
            .collect();
        for key in keys {
            resources.invalidate_graphics(key);
        }
        Ok(())
    }

    /// Allocate a root target matching the exact selected viewport.
    pub fn configure(&mut self, viewport: WorldViewport) -> Result<(), RenderError> {
        if !(1..=self.max_extent()).contains(&viewport.width)
            || !(1..=self.max_extent()).contains(&viewport.height)
            || !viewport.device_pixel_ratio.is_finite()
            || viewport.device_pixel_ratio <= 0.0
        {
            return Err(RenderError::InvalidViewport);
        }
        let target = Target::new(&self.state.gpu, [viewport.width, viewport.height]);
        self.state.gpu.check().map_err(RenderError::Device)?;
        self.state.composition.damage = None;
        self.state.target = Some(target);
        Ok(())
    }

    /// Compose a selected output and optionally copy tightly packed top-left RGBA8.
    pub fn draw(
        &mut self,
        host: &HostRuntime,
        output: OutputRef,
        publication: WorldPublicationId,
        viewport: WorldViewport,
        time: f64,
        #[cfg(not(target_arch = "wasm32"))] pixels: Option<&mut [u8]>,
        #[cfg(target_arch = "wasm32")] surface: &crate::BrowserSurface,
    ) -> Result<FrameSummary, RenderError> {
        #[cfg(feature = "instrumentation")]
        let _span = ipp_core::profiling::CpuTraceScope::new("WebGpuRenderService.draw");

        let state = &mut self.state;
        let gpu = &state.gpu;
        #[cfg(target_arch = "wasm32")]
        if std::mem::take(&mut gpu.web_images.borrow_mut().invalidated) {
            state.composition.invalidate_web_images();
        }
        let target = state.target.as_ref().ok_or(RenderError::InvalidViewport)?;
        if target.color.width() != viewport.width || target.color.height() != viewport.height {
            return Err(RenderError::InvalidViewport);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if pixels
            .as_ref()
            .is_some_and(|p| p.len() != viewport.width as usize * viewport.height as usize * 4)
        {
            return Err(RenderError::InvalidViewport);
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let summary = state.composition.draw(
            gpu,
            host,
            target,
            &mut encoder,
            &state.white,
            &state.identity,
            output,
            publication,
            time,
        )?;
        #[cfg(target_arch = "wasm32")]
        surface.present(target, encoder)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let capture = pixels.as_ref().map(|_| target.copy(gpu, &mut encoder));
            if state.pending.len() >= 3
                && let Some(oldest) = state.pending.pop_front()
            {
                gpu.wait(oldest).map_err(RenderError::Device)?;
            }
            let index = gpu.queue.submit([encoder.finish()]);
            if let (Some(pixels), Some((buffer, stride))) = (pixels, capture) {
                let (tx, rx) = mpsc::sync_channel(1);
                buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        let _ = tx.send(result);
                    });
                gpu.wait(index).map_err(RenderError::Device)?;
                rx.try_recv()
                    .map_err(|e| RenderError::Device(e.to_string()))?
                    .map_err(|e| RenderError::Device(e.to_string()))?;
                let bytes = buffer
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| RenderError::Device(e.to_string()))?;
                for (dst, src) in pixels
                    .chunks_exact_mut(viewport.width as usize * 4)
                    .zip(bytes.chunks_exact(stride))
                {
                    dst.copy_from_slice(&src[..dst.len()]);
                }
                drop(bytes);
                buffer.unmap();
                state.pending.clear();
            } else {
                state.pending.push_back(index);
            }
        }
        gpu.check().map_err(RenderError::Device)?;
        Ok(summary)
    }
}
