//! Host-owned Smol scheduling. Host callbacks request an update and never run tasks.
//!
//! Handles own cancellation. Awaiting a result polls only the handle: the caller
//! remains on its own executor. Native blocking jobs retain their owned inputs
//! until the pool finishes, even when the awaiting task has been cancelled.

#[cfg(not(target_arch = "wasm32"))]
mod delay;
mod service;
mod task;
#[cfg(not(target_arch = "wasm32"))]
pub use delay::native_delay;

#[cfg(not(target_arch = "wasm32"))]
mod io;

pub use service::{HostScheduler, TaskSchedulerService, TaskSchedulers};
pub use task::{TaskCancelled, TaskHandle};

#[cfg(not(target_arch = "wasm32"))]
pub use io::IoScheduler;

/// Browser operations run on the Host worker through platform futures.
#[cfg(target_arch = "wasm32")]
pub type IoScheduler = HostScheduler;

#[cfg(test)]
mod scheduler_tests;
