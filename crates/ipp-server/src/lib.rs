//! Native headless hosting with independently selectable transports and rendering.
//!
//! The `websocket` feature provides a bounded loopback host. Each connection
//! negotiates with the Host before creating or attaching to a named World;
//! mutations use core batches and the validated World schedule. Native context creation and renderer integration remain future
//! implementations.

#[cfg(all(feature = "diagnostics", feature = "websocket"))]
macro_rules! diagnostic {
    ($($args:tt)*) => { ipp_core::diagnostic!($($args)*); };
}

#[cfg(all(not(feature = "diagnostics"), feature = "websocket"))]
macro_rules! diagnostic {
    ($($args:tt)*) => {{}};
}

#[cfg(feature = "diagnostics")]
pub mod diagnostics;

#[cfg(feature = "websocket")]
mod host;

pub mod services;
#[cfg(feature = "websocket")]
use host::NativeHost;

#[cfg(feature = "websocket")]
pub mod websocket;
