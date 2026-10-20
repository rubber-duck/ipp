//! Capture permits retain staging pressure independently of a GPU context.

use std::{cell::Cell, rc::Rc};

use ipp_core::services::web_content::{
    WebContentImageDescription, WebContentImageId, WebContentRequestId,
};

#[derive(Clone, Copy, Default)]
pub(super) struct StagingUsage {
    pub images: usize,
    pub bytes: usize,
}

/// Capacity reserved before a producer captures or transfers one complete frame.
///
/// The Host adapter must release owned producer snapshots/imports before dropping this ticket
/// or returning from import, including on rejection. The ticket is deliberately
/// not cloneable. Dropping it cancels admission on the renderer's next refresh.
/// Context loss does not release its staging charge while a capture still owns it.
/// Borrowed CEF descriptors stay owned by CEF and must never be closed here.
pub struct WebImageReservation {
    pub(super) image: WebContentImageId,
    pub(super) request: WebContentRequestId,
    pub(super) description: WebContentImageDescription,
    pub(super) lifetime: Rc<()>,
    pub(super) _staging: WebImageStaging,
}

impl WebImageReservation {
    /// Exact full-frame extent reserved for the producer.
    pub fn pixel_size(&self) -> [u32; 2] {
        self.description.pixel_size
    }
}

/// Charged producer-owned GPU working storage, independent of any one frame.
///
/// Reserve before allocating capture surfaces and keep the permit until those
/// resources are destroyed. This covers managed storage, not browser engine or
/// graphics driver overhead. It remains charged through renderer context loss.
pub struct WebImageStaging {
    pub(super) usage: Rc<Cell<StagingUsage>>,
    pub(super) images: usize,
    pub(super) bytes: usize,
}

impl Drop for WebImageStaging {
    fn drop(&mut self) {
        let usage = self.usage.get();
        self.usage.set(StagingUsage {
            images: usage.images - self.images,
            bytes: usage.bytes - self.bytes,
        });
    }
}
