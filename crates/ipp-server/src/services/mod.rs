//! Native Host service adapters: the platform `HostServices` implementation and
//! the native implementations of the core I/O traits.

mod host_services;
pub use host_services::NativeHostServices;

#[cfg(target_os = "macos")]
pub mod cef;

pub mod io;
