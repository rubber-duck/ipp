//! Browser adapters for the shared host session policy.

mod host;
pub(crate) use host::WasmHostServices;

#[cfg(all(feature = "render", target_arch = "wasm32"))]
pub(crate) mod render;

#[cfg(all(feature = "render", any(test, target_arch = "wasm32")))]
pub(crate) mod render_statistics;

#[cfg(all(
    feature = "instrumentation",
    feature = "render",
    target_arch = "wasm32"
))]
mod render_profiling;

pub(crate) mod task_wakeup;

#[cfg(target_arch = "wasm32")]
pub(crate) mod task_timer;
