//! Host admission around a bounded platform GPU import.

use ipp_core::services::web_content::{
    WebContentError, WebContentImageDescription, WebContentImageId, WebContentRequestId,
};

use super::{WebImageImportError, WebImageReservation, WebImageStaging, WebImageUsage};
use crate::{RenderDevice, RenderError, RenderService};

impl<D: RenderDevice> RenderService<D> {
    /// Reserve private destination storage and capture/transfer bytes BEFORE
    /// asking a producer for one frame. Persistent capture surfaces use a
    /// separate `reserve_web_image_staging` permit for their actual lifetime.
    ///
    /// `staging_bytes` is the upper bound of this capture's managed working
    /// storage, including padded rows where applicable. A zero bound is valid
    /// only when another live staging permit already covers that storage.
    pub fn reserve_web_image(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        request: WebContentRequestId,
        description: WebContentImageDescription,
        staging_bytes: usize,
    ) -> Result<WebImageReservation, WebImageImportError> {
        self.refresh_web_images(host)
            .map_err(WebImageImportError::Device)?;
        let image = host
            .web_content_mut()
            .reserve_image(request, description)
            .map_err(WebImageImportError::Request)?;
        match self
            .web_images
            .reserve(image, request, description, staging_bytes)
        {
            Ok(ticket) => {
                self.web_images.bind_host(host.identity());
                Ok(ticket)
            }
            Err(error) => {
                host.web_content_mut().discard_image(image);
                Err(error)
            }
        }
    }

    /// Reserve reusable producer-owned capture surfaces before allocating them.
    /// Keep the returned permit until every covered resource has been released.
    pub fn reserve_web_image_staging(
        &mut self,
        images: usize,
        bytes: usize,
    ) -> Result<WebImageStaging, WebImageImportError> {
        self.web_images.reserve_staging(images, bytes)
    }

    /// Fill private top-left-first SRGB8_ALPHA8 storage with straight alpha,
    /// then enqueue its immutable identity for ordinary Canvas publication.
    ///
    /// This trusted Host/device adapter must fill the complete allocation and
    /// must not retain its texture handle or reenter a World. Borrowed producer
    /// buffers must remain valid until all GPU reads finish; a fence alone does
    /// not extend them. Release owned snapshots/imports on ALL outcomes, including when
    /// admission rejects before invoking the callback. Platform release may be
    /// idempotent so a Host `finally` can also cover early rejection.
    /// Never close CEF-owned FDs: returning its callback relinquishes their use.
    pub fn import_reserved_web_image(
        &mut self,
        host: &mut ipp_core::HostRuntime,
        ticket: WebImageReservation,
        import: impl FnOnce(&mut D, &D::Texture) -> Result<(), RenderError>,
    ) -> Result<WebContentImageId, WebImageImportError> {
        // A foreign Host cannot even drive cleanup: its retained set says
        // nothing about allocations belonging to the renderer's current owner.
        self.validate_catalog(host)
            .map_err(WebImageImportError::Device)?;
        let image = ticket.image;
        let result = (|| {
            self.refresh_web_images(host)
                .map_err(WebImageImportError::Device)?;
            if !host.web_content().is_image_retained(image) {
                return Err(WebImageImportError::Request(WebContentError::StaleRequest));
            }
            let texture = self
                .web_images
                .reserved_texture(&ticket)
                .ok_or(WebImageImportError::ContextUnavailable)?;
            import(&mut *self.device.borrow_mut(), texture).map_err(WebImageImportError::Device)?;
            host.web_content_mut()
                .complete_image(ticket.request, image)
                .map_err(WebImageImportError::Request)?;
            self.web_images.completed(&ticket);
            Ok(image)
        })();
        drop(ticket);
        if result.is_err() {
            host.web_content_mut().discard_image(image);
            // Preserve the primary failure. A retirement failure keeps private
            // storage charged and is retried on later service work.
            let _ = self.web_images.collect(host.web_content_mut());
        }
        result
    }

    /// Progress canceled reservations and retirement without drawing a World.
    pub fn refresh_web_images(
        &mut self,
        host: &mut ipp_core::HostRuntime,
    ) -> Result<(), RenderError> {
        self.validate_catalog(host)?;
        self.web_images.synchronize(host.web_content_mut());
        self.web_images.collect(host.web_content_mut())
    }

    /// Combined resident, pending, staging and retirement storage pressure.
    pub fn web_image_usage(&self) -> WebImageUsage {
        self.web_images.usage()
    }

    /// Override admission bounds for deterministic pressure tests.
    #[cfg(any(test, feature = "instrumentation"))]
    pub fn set_web_image_limits(&mut self, images: usize, bytes: usize) {
        self.web_images.set_limits(images, bytes);
    }
}
