//! Native Host service adapters and optional asset output.

pub mod asset_output;

pub mod data_source;

#[cfg(any(feature = "websocket", all(test, feature = "builtin-assets")))]
mod host;

#[cfg(feature = "websocket")]
pub(crate) use host::NativeHostServices;
