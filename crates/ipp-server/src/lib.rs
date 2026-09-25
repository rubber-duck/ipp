//! Native headless hosting with independently selectable transports and rendering.
//!
//! The `websocket` feature provides a bounded loopback host. Each connection
//! negotiates with the Host before creating or attaching to a named World;
//! mutations use core batches and the validated World schedule. The server
//! itself does not render: [`websocket::serve_with`] accepts composed platform
//! services, which the `gles_host` testing example uses to present and capture
//! through a native GLES context.

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

pub mod services;

#[cfg(feature = "websocket")]
pub mod websocket;
