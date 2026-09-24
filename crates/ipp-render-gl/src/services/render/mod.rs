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

#[cfg(feature = "surfaces")]
mod analytic_glyphs;
mod assets;
mod custom_material;
mod custom_shader;
mod debug_geometry;
mod device;
mod draw_lighting;
mod draw_order;
mod frame_scratch;
mod frame_statistics;
mod light_selection;
mod lighting;

#[cfg(feature = "particles")]
mod particles;

mod program_assets;
mod service;
mod shader;
mod shader_asset;
#[cfg(feature = "surfaces")]
mod surface_assets;
#[cfg(feature = "surfaces")]
mod surface_cache;
#[cfg(feature = "surfaces")]
mod surface_path;
mod template;

#[cfg(feature = "gui")]
pub(crate) mod glyph_atlas;
#[cfg(feature = "gui")]
pub(crate) mod gui_batch;
#[cfg(feature = "gui")]
mod gui_storage;
#[cfg(feature = "surfaces")]
pub(crate) mod retained_surfaces;

#[cfg(feature = "surfaces")]
pub use device::SurfacePathDescriptor;
#[cfg(feature = "surfaces")]
pub use device::SurfacePathInstance;
pub use device::{PlatformRenderDevice, RenderDevice, ViewportLimits};
pub use frame_statistics::RenderFrameSummary;
#[cfg(any(test, feature = "diagnostics"))]
pub use frame_statistics::RenderStatistics;
#[cfg(all(feature = "gui", any(test, feature = "diagnostics")))]
pub use glyph_atlas::{GlyphAtlasLimits, MIN_POPULATES_PER_FRAME as GLYPH_MIN_POPULATES_PER_FRAME};
/// Retained GUI vertices are the layout of the public [`RenderDevice`] GUI batch
/// operations; device-level hosts generate box geometry and interpret its fill
/// code through these.
#[cfg(feature = "gui")]
pub use gui_batch::{
    GUI_FILL_GLYPH, GuiVertex, generate_box_vertices as generate_gui_box_vertices,
};
#[cfg(feature = "surfaces")]
pub use surface_path::{
    SurfaceBandTexels, SurfaceCurveTexels, SurfacePathAtlas, SurfacePathTexels, pack_surface_paths,
};

#[cfg(target_arch = "wasm32")]
pub use device::WebGlRenderDevice;

#[cfg(not(target_arch = "wasm32"))]
pub use device::GlesRenderDevice;

#[cfg(any(test, feature = "diagnostics"))]
pub use custom_material::CustomMaterialFallback;
pub use lighting::RenderLightingFrame;
pub use service::{RenderError, RenderService};
#[cfg(all(feature = "surfaces", any(test, feature = "diagnostics")))]
pub use surface_cache::{
    SURFACE_CACHE_ANIMATED_FRAMES, SURFACE_CACHE_BUDGET_BYTES, SURFACE_CACHE_SETTLE_FRAMES,
    SurfaceCacheDiagnostic, SurfaceCachePresentation,
};
