//! Native Host service adapters and optional asset output.

pub mod asset_output;

pub mod io;

mod host;

pub use host::NativeHostServices;

mod stream_input;

pub mod http;

mod http_transport;

#[cfg(target_os = "linux")]
pub mod mapped_input;
