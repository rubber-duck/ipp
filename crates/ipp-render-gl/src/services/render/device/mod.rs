#[cfg(feature = "instrumentation")]
mod gl_call_counts;
#[cfg(feature = "instrumentation")]
mod gpu_queries;
#[cfg(feature = "instrumentation")]
pub use gl_call_counts::RenderGlCallCounts;
#[cfg(feature = "instrumentation")]
pub use gpu_queries::{RenderGpuAvailability, RenderGpuCapability, RenderGpuQueryToken};

mod error_checks;
mod render_device;
mod retained_records;
mod surface_instances;
mod uniform_cache;

pub use super::retained::records::{GuiRecord, GuiRecordKind};
pub use render_device::{RenderDevice, ViewportLimits};
pub use surface_instances::{SurfacePathDescriptor, SurfacePathInstance};

#[cfg(not(target_arch = "wasm32"))]
mod gles;

#[cfg(target_arch = "wasm32")]
mod webgl;

#[cfg(not(target_arch = "wasm32"))]
pub use gles::GlesRenderDevice;

#[cfg(target_arch = "wasm32")]
pub use webgl::WebGlRenderDevice;

/// Compile-time GL device for the current target.
#[cfg(not(target_arch = "wasm32"))]
pub type PlatformRenderDevice = GlesRenderDevice;

/// Compile-time GL device for the current target.
#[cfg(target_arch = "wasm32")]
pub type PlatformRenderDevice = WebGlRenderDevice;
