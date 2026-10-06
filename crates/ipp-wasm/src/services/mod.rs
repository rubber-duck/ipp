//! Browser adapters for the shared host session policy.

mod host_services;
pub(crate) use host_services::WasmHost;

#[cfg(all(feature = "render", any(test, target_arch = "wasm32")))]
pub(crate) mod render;

pub(crate) mod task_scheduler;
