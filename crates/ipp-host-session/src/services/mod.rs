//! Shared host service adapters.

pub mod asset_provider;
#[cfg(feature = "gui")]
pub mod gui_input;

#[cfg(all(feature = "gui", feature = "diagnostics"))]
pub mod gui_layout_statistics;

pub(crate) mod connection;
pub(crate) mod presentation;
