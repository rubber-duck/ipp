//! Browser adapters for the shared host session policy.

mod host;
pub(crate) use host::WasmHostServices;

#[cfg(all(feature = "render", target_arch = "wasm32"))]
pub(crate) mod render;

#[cfg(all(
    feature = "render",
    feature = "diagnostics",
    any(test, target_arch = "wasm32")
))]
pub(crate) mod render_statistics;
