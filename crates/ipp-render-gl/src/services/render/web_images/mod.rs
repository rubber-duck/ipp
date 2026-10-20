//! Immutable, publication-owned web images. Platform adapters import only into
//! private storage; the Host service admits image identities at its boundary.

mod registry;
mod reservation;
mod service;

pub(super) use registry::WebImageRegistry;
pub use reservation::{WebImageReservation, WebImageStaging};

/// Charged storage includes images awaiting publication or GPU retirement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WebImageUsage {
    /// Allocations, including outstanding producer staging and GPU retirement.
    pub images: usize,
    /// Combined charged bytes, including staging across context loss.
    pub bytes: usize,
    /// Private textures waiting for a completion fence.
    pub retiring_images: usize,
    /// Outstanding producer capture/transfer reservations.
    pub staging_images: usize,
}

/// Import failure preserves every previously admitted image.
#[derive(Debug)]
pub enum WebImageImportError {
    /// A full frame must have nonzero dimensions within the device limit.
    InvalidSize,
    /// New storage would exceed the combined allocation or byte bound.
    Capacity,
    /// The ticket's creating renderer context is no longer available.
    ContextUnavailable,
    /// The Host rejected the request or image authority.
    Request(ipp_core::services::web_content::WebContentError),
    /// Allocation, platform import or completion fencing failed.
    Device(crate::RenderError),
}

impl std::fmt::Display for WebImageImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSize => f.write_str("web image dimensions exceed renderer bounds"),
            Self::Capacity => f.write_str("web image storage capacity exhausted"),
            Self::ContextUnavailable => f.write_str("web image context is unavailable"),
            Self::Request(error) => write!(f, "web image request: {error}"),
            Self::Device(error) => write!(f, "web image device: {error}"),
        }
    }
}

impl std::error::Error for WebImageImportError {}
