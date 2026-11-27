//! WebGPU rendering of completed IPP Canvas and Camera publications.
#![cfg(any(target_os = "macos", target_arch = "wasm32"))]

mod assets;
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::BrowserSurface;
#[cfg(target_arch = "wasm32")]
mod web_images;
#[cfg(target_arch = "wasm32")]
pub use web_images::WebImageReservation;

mod camera;
mod camera_commands;
mod camera_images;
mod canvas;
mod canvas_buffers;
mod canvas_device;
mod canvas_images;
mod composition;
mod custom_mesh;
mod derived_meshes;
mod device;
mod path_masks;
mod paths;
mod scene;
mod service;
mod shaders;

pub use service::{FrameSummary, RenderError, WebGpuRenderService};
