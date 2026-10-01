//! Native headless hosting with a WebSocket transport and composable rendering.
//!
//! The [`websocket`] transport provides a bounded loopback host. Each connection
//! negotiates with the Host before creating or attaching to a named World;
//! mutations use core batches and the validated World schedule. The server
//! itself does not render: [`websocket::serve_with`] accepts composed platform
//! services, which the `gles_host` testing example uses to present and capture
//! through a native GLES context.

macro_rules! diagnostic {
    ($($args:tt)*) => { ipp_core::diagnostic!($($args)*); };
}

pub mod diagnostics;

pub mod services;

pub mod websocket;
