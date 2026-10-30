//! Optional native browser engine. Platform objects never enter World state.

#[cfg(target_os = "macos")]
mod engine;
#[cfg(target_os = "macos")]
mod provider;
#[cfg(target_os = "macos")]
pub(crate) use provider::CefProvider;

/// Explicit installation and permitted local page root, supplied by the launcher.
pub(crate) struct CefSetup {
    pub engine: std::path::PathBuf,
    pub root: std::path::PathBuf,
}
