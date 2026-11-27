//! Worker presentation for the selected browser renderer, statistics and capture.

#[cfg(all(feature = "render", target_arch = "wasm32"))]
mod service;
#[cfg(target_arch = "wasm32")]
mod web_content;
#[cfg(all(feature = "render", target_arch = "wasm32"))]
pub(crate) use service::RenderSurfaceService;

#[cfg(all(
    any(feature = "render", feature = "webgpu"),
    any(test, target_arch = "wasm32")
))]
pub(crate) mod statistics;

#[cfg(all(
    feature = "instrumentation",
    feature = "render",
    target_arch = "wasm32"
))]
mod profiling;

#[cfg(target_arch = "wasm32")]
mod exports;

#[cfg(all(feature = "webgpu", not(feature = "render"), target_arch = "wasm32"))]
mod webgpu;
#[cfg(all(feature = "webgpu", not(feature = "render"), target_arch = "wasm32"))]
pub(crate) use webgpu::RenderSurfaceService;
