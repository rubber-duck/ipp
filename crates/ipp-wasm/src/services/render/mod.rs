//! Worker presentation: the WebGL render surface, its packed statistics record
//! and GPU profiling captures.

#[cfg(all(feature = "render", target_arch = "wasm32"))]
mod service;
#[cfg(all(feature = "render", target_arch = "wasm32"))]
pub(crate) use service::RenderSurfaceService;

#[cfg(all(feature = "render", any(test, target_arch = "wasm32")))]
pub(crate) mod statistics;

#[cfg(all(
    feature = "instrumentation",
    feature = "render",
    target_arch = "wasm32"
))]
mod profiling;
