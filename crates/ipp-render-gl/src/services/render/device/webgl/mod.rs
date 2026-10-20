//! WebGL 2 device for WebAssembly builds, over the `ipp_gl` imports its bridge
//! supplies.

mod commands;
mod completion_fences;
#[cfg(feature = "instrumentation")]
pub(super) mod gpu_queries;
mod imports;
mod texture_readback;
mod web_images;

pub use commands::WebGlRenderDevice;
