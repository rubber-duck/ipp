//! Unlit and direct-light GL rendering with statically selected WebGL 2 and GLES 3 devices.
//!
//! Hosts own contexts, surfaces and scheduling; native context libraries and
//! shims stay in hosts.

mod assets;
mod custom_material;
mod custom_shader;
mod debug_geometry;
mod device;
mod draw_lighting;
mod draw_order;
mod frame_scratch;
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
mod surface_path;
mod template;

#[cfg(feature = "gui")]
pub mod gui_batch;

#[cfg(feature = "gui")]
pub use device::SurfaceBoxShape;
#[cfg(feature = "surfaces")]
pub use device::SurfacePathDescriptor;
#[cfg(feature = "surfaces")]
pub use device::SurfacePathInstance;
pub use device::{PlatformRenderDevice, RenderDevice};
#[cfg(feature = "gui")]
pub use gui_batch::{GuiBoxVertex, GuiPartClass};

#[cfg(target_arch = "wasm32")]
pub use device::WebGlRenderDevice;

#[cfg(not(target_arch = "wasm32"))]
pub use device::GlesRenderDevice;

pub use lighting::RenderLightingFrame;
pub use service::{RenderError, RenderService, RenderStats};
