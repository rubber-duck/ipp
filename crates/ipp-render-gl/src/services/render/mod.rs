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

mod analytic_glyphs;
mod asset_context;
mod assets;
mod canvas_paint;
mod canvas_scene;
mod custom_material;
mod custom_shader;
mod debug_geometry;
mod device;
mod draw_lighting;
mod draw_order;
mod frame_scratch;
mod frame_statistics;
mod generated_meshes;
mod generated_paths;
mod light_selection;
mod lighting;

mod particles;
mod plot_label_layout;
mod plot_plane_facing;
mod plot_view_placement;

mod program_assets;
mod scene;
pub use scene::RenderEntity;
mod service;
mod shader;
mod shader_asset;
mod surface_assets;
mod surface_cache;
mod surface_mesh;
mod surface_path;
mod template;

pub(crate) mod glyph_atlas;
pub(crate) mod gui_batch;
mod gui_draw_order;
mod gui_records;
mod gui_storage;
pub(crate) mod retained_surfaces;

pub use device::SurfacePathDescriptor;
pub use device::SurfacePathInstance;
pub use device::{PlatformRenderDevice, RenderDevice, ViewportLimits};
pub use frame_statistics::RenderFrameSummary;
pub use frame_statistics::RenderStatistics;
#[cfg(any(test, feature = "instrumentation"))]
pub use glyph_atlas::{GlyphAtlasLimits, MIN_POPULATES_PER_FRAME as GLYPH_MIN_POPULATES_PER_FRAME};
/// Retained GUI records are the layouts of the public [`RenderDevice`] GUI batch
/// operations; device-level hosts generate box records through these.
pub use gui_batch::generate_box_records as generate_gui_box_records;
pub use gui_records::{GuiGlyphRecord, GuiRecord, GuiRecordKind, GuiShapeRecord};
pub use surface_path::{
    SurfaceBandTexels, SurfaceCurveTexels, SurfacePathAtlas, SurfacePathTexels, pack_surface_paths,
};

#[cfg(target_arch = "wasm32")]
pub use device::WebGlRenderDevice;

#[cfg(not(target_arch = "wasm32"))]
pub use device::GlesRenderDevice;

pub use canvas_paint::{
    CANVAS_PAINT_SLOTS, CANVAS_PAINT_VECTORS, CanvasPaintFallback, CanvasPaintFallbackReason,
};
pub use custom_material::CustomMaterialFallback;
pub use lighting::RenderLightingFrame;
pub use service::{RenderAssetExportDelay, RenderError, RenderService};
pub use surface_cache::{
    SURFACE_CACHE_ANIMATED_FRAMES, SURFACE_CACHE_BUDGET_BYTES, SURFACE_CACHE_SETTLE_FRAMES,
    SurfaceCacheDiagnostic, SurfaceCachePresentation,
};

#[cfg(feature = "instrumentation")]
pub use device::RenderGlCallCounts;
#[cfg(feature = "instrumentation")]
pub use device::{RenderGpuAvailability, RenderGpuCapability, RenderGpuQueryToken};

#[cfg(feature = "instrumentation")]
pub use service::{
    RenderGlCallWindow, RenderGlStopReason, RenderGpuIdentity, RenderGpuSample, RenderGpuSampling,
    RenderGpuScope,
};
