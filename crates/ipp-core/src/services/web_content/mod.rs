//! Headless authority for platform-produced, immutable Canvas images.
//!
//! Platform adapters keep their browser objects and GPU storage outside this
//! service. Only a live request may reserve and complete an image. Completed
//! publications retain exact image identities independently of later requests.

mod service;

#[cfg(test)]
mod service_tests;

pub use service::WebContentService;

use crate::{WorldRef, systems::canvas::CanvasTarget};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Exact owner-issued source registration; replacement never reuses authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WebContentSourceId(u64, u64);

/// One source/extent request, scoped to its issuing service and target lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WebContentRequestId(u64, u64);

/// One immutable renderer-owned full image, never a mutable texture slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WebContentImageId(u64, u64);

/// Exact World and Canvas component lifetime requesting content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WebContentTarget {
    /// Exact owning World incarnation.
    pub world: WorldRef,
    /// Exact owning entity and CanvasHtml component incarnation.
    pub canvas: CanvasTarget,
}

/// Owned request metadata. Platform handles never enter this boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct WebContentRequest {
    /// Authority for completions of this request only.
    pub id: WebContentRequestId,
    /// Consumer lifetime which issued the request.
    pub target: WebContentTarget,
    /// Resolved adapter registration; never follows a replacement by name.
    pub source: WebContentSourceId,
    /// Registered name for adapter routing and diagnostics.
    pub source_name: Arc<str>,
    /// Evaluated Canvas extent, before visual scaling.
    pub logical_size: [f32; 2],
}

/// Actual dimensions of an admitted full image; color/import details are renderer-owned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebContentImageDescription {
    /// Nonzero width and height of the complete image in pixels.
    pub pixel_size: [u32; 2],
}

/// Revocation witness for pixels derived from an image. It retains no image
/// storage and survives ordinary image/publication retirement. Equality compares
/// authority lifetimes, while `is_valid` observes their current validity.
#[derive(Clone, Debug)]
pub struct WebContentImageValidity {
    source: Arc<AtomicBool>,
    target: Arc<AtomicBool>,
    renderer: Arc<AtomicBool>,
}

impl WebContentImageValidity {
    /// Whether the source, exact component lifetime and renderer context still
    /// authorize these pixels. Ordinary repaint and request cancellation do not revoke them.
    pub fn is_valid(&self) -> bool {
        self.source.load(Ordering::Acquire)
            && self.target.load(Ordering::Acquire)
            && self.renderer.load(Ordering::Acquire)
    }
}

impl PartialEq for WebContentImageValidity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
            && Arc::ptr_eq(&self.target, &other.target)
            && Arc::ptr_eq(&self.renderer, &other.renderer)
    }
}

impl Eq for WebContentImageValidity {}

/// Observable state without exposing a platform object or manufactured pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebContentStatus {
    /// A registered source has not yet supplied an image.
    Pending,
    /// Latest completed image eligible for the next World publication.
    Ready(WebContentImageId),
    /// Source or accelerated producer capability is unavailable.
    Unavailable,
}

/// Rejected authority, malformed dimensions or an outstanding import reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebContentError {
    /// Request no longer has authority, including after context loss.
    StaleRequest,
    /// Dimensions or the request/image pairing are invalid.
    InvalidImage,
    /// One private import already holds this request's reservation.
    ReservationPending,
}

impl std::fmt::Display for WebContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::StaleRequest => "web-content request is no longer current",
            Self::InvalidImage => "invalid web-content image or reservation",
            Self::ReservationPending => "web-content request already has an import reservation",
        })
    }
}

impl std::error::Error for WebContentError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WebContentPublicationLease(u64);
