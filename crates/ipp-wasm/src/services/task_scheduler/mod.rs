//! Browser worker task scheduling: enqueue-only event-loop wakeups, worker
//! timers and the trusted scheduler probe.

pub(crate) mod wakeup;

#[cfg(target_arch = "wasm32")]
pub(crate) mod timer;

#[cfg(all(feature = "instrumentation", target_arch = "wasm32"))]
pub(crate) mod testing;
