//! Unlit and direct-light GL rendering with statically selected WebGL 2 and GLES 3 devices.
//!
//! Hosts own contexts, surfaces and scheduling; native context libraries and
//! shims stay in hosts.

/// A GLSL source under `src/services/render`, embedded without comments by
/// the crate's build script; the source files keep their documentation.
macro_rules! embedded_shader {
    ($path:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/render/", $path))
    };
}
pub(crate) use embedded_shader;

mod assets;
mod canvas;
mod device;
mod frame;
#[cfg(feature = "instrumentation")]
mod gpu_profiling;
mod lifecycle;
mod lighting;
mod materials;
mod outputs;
mod plot;
mod prepare;
mod retained;
mod service;
mod statistics;
mod surface;
#[cfg(test)]
mod test_support;

pub use outputs::scene::RenderEntity;

pub use assets::paths::{
    SurfaceBandTexels, SurfaceCurveTexels, SurfacePathAtlas, SurfacePathTexels, pack_surface_paths,
};
pub use device::SurfacePathDescriptor;
pub use device::SurfacePathInstance;
pub use device::{PlatformRenderDevice, RenderDevice, ViewportLimits};
/// Retained GUI records are the layouts of the public [`RenderDevice`] GUI batch
/// operations; device-level hosts generate box records through these.
pub use retained::box_records::generate_box_records as generate_gui_box_records;
#[cfg(any(test, feature = "instrumentation"))]
pub use retained::glyph_atlas::{
    GlyphAtlasLimits, MIN_POPULATES_PER_FRAME as GLYPH_MIN_POPULATES_PER_FRAME,
};
pub use retained::records::{GuiGlyphRecord, GuiRecord, GuiRecordKind, GuiShapeRecord};
pub use statistics::RenderFrameSummary;
pub use statistics::RenderStatistics;

#[cfg(target_arch = "wasm32")]
pub use device::WebGlRenderDevice;

#[cfg(not(target_arch = "wasm32"))]
pub use device::GlesRenderDevice;

pub use assets::export::RenderAssetExportDelay;
pub use canvas::paint::{
    CANVAS_PAINT_SLOTS, CANVAS_PAINT_VECTORS, CanvasPaintFallback, CanvasPaintFallbackReason,
};
pub use lighting::lights::RenderLightingFrame;
pub use materials::custom_material::CustomMaterialFallback;
pub use service::{RenderError, RenderService};
pub use surface::texture_cache::{
    SURFACE_CACHE_ANIMATED_FRAMES, SURFACE_CACHE_BUDGET_BYTES, SURFACE_CACHE_SETTLE_FRAMES,
    SurfaceCacheDiagnostic, SurfaceCachePresentation,
};

#[cfg(feature = "instrumentation")]
pub use device::RenderGlCallCounts;
#[cfg(feature = "instrumentation")]
pub use device::{RenderGpuAvailability, RenderGpuCapability, RenderGpuQueryToken};

#[cfg(feature = "instrumentation")]
pub use gpu_profiling::{
    RenderGlCallWindow, RenderGlStopReason, RenderGpuIdentity, RenderGpuSample, RenderGpuSampling,
    RenderGpuScope,
};
