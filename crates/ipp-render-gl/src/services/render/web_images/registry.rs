//! Private full-frame allocation and fenced retirement under one combined bound.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};

use ipp_core::services::web_content::{
    WebContentImageDescription, WebContentImageId, WebContentRequestId, WebContentService,
};

use super::{
    WebImageImportError, WebImageReservation, WebImageStaging, WebImageUsage,
    reservation::StagingUsage,
};
use crate::services::render::assets::context::{RenderAssetContext, RenderAssetLease};
use crate::{RenderDevice, RenderError};

const MAX_IMAGES: usize = 128;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_AXIS: u32 = 4096;

struct Image<T> {
    texture: T,
    bytes: usize,
    pending: Option<Weak<()>>,
}

pub(in crate::services::render) struct WebImageRegistry<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    context: RenderAssetContext,
    lease: RenderAssetLease,
    images: BTreeMap<WebContentImageId, Image<D::Texture>>,
    retiring: BTreeMap<u64, Vec<Image<D::Texture>>>,
    usage: WebImageUsage,
    staging: Rc<Cell<StagingUsage>>,
    max_images: usize,
    max_bytes: usize,
    host: Option<u64>,
}

impl<D: RenderDevice> WebImageRegistry<D> {
    pub(in crate::services::render) fn new(
        device: Rc<RefCell<D>>,
        context: RenderAssetContext,
    ) -> Self {
        let lease = context.lease();
        Self {
            device,
            context,
            lease,
            images: BTreeMap::new(),
            retiring: BTreeMap::new(),
            usage: WebImageUsage::default(),
            staging: Rc::default(),
            max_images: MAX_IMAGES,
            max_bytes: MAX_BYTES,
            host: None,
        }
    }

    pub(super) fn usage(&self) -> WebImageUsage {
        let staging = self.staging.get();
        WebImageUsage {
            images: self.usage.images + staging.images,
            bytes: self.usage.bytes + staging.bytes,
            staging_images: staging.images,
            ..self.usage
        }
    }

    pub(in crate::services::render) fn validate_host(&self, host: u64) -> Result<(), RenderError> {
        if self.host.is_some_and(|owner| owner != host) {
            return Err(RenderError::HostCatalogMismatch);
        }
        Ok(())
    }

    pub(super) fn bind_host(&mut self, host: u64) {
        self.host = Some(host);
    }

    pub(in crate::services::render) fn unbind_host(&mut self, service: &mut WebContentService) {
        if self.host.take().is_some() {
            service.renderer_invalidated();
        }
    }

    /// A loss/detach generation never makes an old image current again.
    pub(super) fn synchronize(&mut self, service: &mut WebContentService) {
        if !self.lease.generation_is_current() {
            if self.host.is_some() {
                service.renderer_invalidated();
            }
            self.images.clear();
            self.retiring.clear();
            self.usage = WebImageUsage::default();
            self.lease = self.context.lease();
        }
    }

    pub(in crate::services::render) fn texture(
        &self,
        image: WebContentImageId,
    ) -> Option<&D::Texture> {
        self.lease
            .is_current()
            .then(|| self.images.get(&image))
            .flatten()
            .map(|entry| &entry.texture)
    }

    pub(super) fn reserve(
        &mut self,
        image: WebContentImageId,
        request: WebContentRequestId,
        description: WebContentImageDescription,
        staging_bytes: usize,
    ) -> Result<WebImageReservation, WebImageImportError> {
        if !self.context.is_active() || !self.lease.is_current() {
            return Err(WebImageImportError::ContextUnavailable);
        }
        let [width, height] = description.pixel_size;
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|value| value.checked_mul(4));
        let Some(bytes) =
            bytes.filter(|_| width > 0 && height > 0 && width <= MAX_AXIS && height <= MAX_AXIS)
        else {
            return Err(WebImageImportError::InvalidSize);
        };
        let usage = self.usage();
        let new_images = 1 + usize::from(staging_bytes > 0);
        if usage
            .images
            .checked_add(new_images)
            .is_none_or(|total| total > self.max_images)
            || usage
                .bytes
                .checked_add(bytes)
                .and_then(|total| total.checked_add(staging_bytes))
                .is_none_or(|total| total > self.max_bytes)
        {
            return Err(WebImageImportError::Capacity);
        }
        // The service issues each ID once. An attempted duplicate cannot replace pixels.
        if self.images.contains_key(&image) {
            return Err(WebImageImportError::Device(RenderError::RenderDevice(
                "web image identity already allocated".into(),
            )));
        }
        let texture = self
            .device
            .borrow_mut()
            .allocate_texture(width, height)
            .map_err(WebImageImportError::Device)?;
        let lifetime = Rc::new(());
        self.images.insert(
            image,
            Image {
                texture,
                bytes,
                pending: Some(Rc::downgrade(&lifetime)),
            },
        );
        self.usage.images += 1;
        self.usage.bytes += bytes;
        let staging = self.charge_staging(usize::from(staging_bytes > 0), staging_bytes);
        Ok(WebImageReservation {
            image,
            request,
            description,
            lifetime,
            _staging: staging,
        })
    }

    pub(super) fn reserve_staging(
        &mut self,
        images: usize,
        bytes: usize,
    ) -> Result<WebImageStaging, WebImageImportError> {
        if images == 0 || bytes == 0 {
            return Err(WebImageImportError::InvalidSize);
        }
        let usage = self.usage();
        if usage
            .images
            .checked_add(images)
            .is_none_or(|total| total > self.max_images)
            || usage
                .bytes
                .checked_add(bytes)
                .is_none_or(|total| total > self.max_bytes)
        {
            return Err(WebImageImportError::Capacity);
        }
        Ok(self.charge_staging(images, bytes))
    }

    fn charge_staging(&self, images: usize, bytes: usize) -> WebImageStaging {
        let staging = self.staging.get();
        self.staging.set(StagingUsage {
            images: staging.images + images,
            bytes: staging.bytes + bytes,
        });
        WebImageStaging {
            usage: self.staging.clone(),
            images,
            bytes,
        }
    }

    pub(super) fn reserved_texture(&self, ticket: &WebImageReservation) -> Option<&D::Texture> {
        let image = self.images.get(&ticket.image)?;
        let lifetime = image.pending.as_ref()?.upgrade()?;
        (self.lease.is_current() && Rc::ptr_eq(&lifetime, &ticket.lifetime))
            .then_some(&image.texture)
    }

    pub(super) fn completed(&mut self, ticket: &WebImageReservation) {
        self.images
            .get_mut(&ticket.image)
            .expect("completed reserved image")
            .pending = None;
    }

    /// Removal from the Host's retained set starts retirement, never immediate reuse.
    pub(super) fn collect(&mut self, service: &mut WebContentService) -> Result<(), RenderError> {
        if !self.lease.is_current() {
            return Ok(());
        }
        let mut ready = Vec::new();
        for fence in self.retiring.keys() {
            if self.device.borrow_mut().poll_completion_fence(*fence)? {
                ready.push(*fence);
            }
        }
        for fence in ready {
            self.device.borrow_mut().delete_completion_fence(fence);
            for image in self
                .retiring
                .remove(&fence)
                .expect("completed image retirement")
            {
                self.device.borrow_mut().delete_texture(image.texture);
                self.usage.images -= 1;
                self.usage.retiring_images -= 1;
                self.usage.bytes -= image.bytes;
            }
        }

        for (id, image) in &mut self.images {
            if image
                .pending
                .as_ref()
                .is_some_and(|ticket| ticket.strong_count() == 0)
            {
                service.discard_image(*id);
                image.pending = None;
            }
        }
        // A live producer ticket can still own capture/transfer storage after
        // request invalidation. Its adapter must release it before retirement.
        let unused: Vec<_> = self
            .images
            .iter()
            .filter(|(id, image)| image.pending.is_none() && !service.is_image_retained(**id))
            .map(|(id, _)| *id)
            .collect();
        if !unused.is_empty() {
            // All previous imports/draws precede this fence. Failure keeps charged
            // storage live, so retry cannot exceed capacity or reuse in-flight pixels.
            let fence = self.device.borrow_mut().create_completion_fence()?;
            let images = unused
                .into_iter()
                .map(|id| self.images.remove(&id).expect("retiring image"))
                .collect::<Vec<_>>();
            self.usage.retiring_images += images.len();
            assert!(
                self.retiring.insert(fence, images).is_none(),
                "device reused a completion fence"
            );
        }
        Ok(())
    }

    /// Context replacement has its own lifetime boundary. GL deletion may defer
    /// physical reclamation; the old context is destroyed before new submissions.
    pub(in crate::services::render) fn release_context(&mut self) {
        if self.lease.is_current() {
            let mut device = self.device.borrow_mut();
            for (_, image) in std::mem::take(&mut self.images) {
                device.delete_texture(image.texture);
            }
            for (fence, images) in std::mem::take(&mut self.retiring) {
                device.delete_completion_fence(fence);
                for image in images {
                    device.delete_texture(image.texture);
                }
            }
        } else {
            self.images.clear();
            self.retiring.clear();
        }
        self.usage = WebImageUsage::default();
    }

    /// A normal detach may reuse the SAME GL context. Finish and delete its
    /// managed objects before invalidating the lease; only actual context loss
    /// permits abandoning names. Producer permits keep their independent charge.
    pub(in crate::services::render) fn unload(&mut self) -> Result<(), RenderError> {
        if self.lease.is_current() && (!self.images.is_empty() || !self.retiring.is_empty()) {
            match self.device.borrow_mut().finish_submitted_work() {
                Ok(()) => {}
                Err(RenderError::ContextLost) => self.context.set_active(false),
                Err(error) => return Err(error),
            }
        }
        self.release_context();
        Ok(())
    }

    #[cfg(any(test, feature = "instrumentation"))]
    pub(super) fn set_limits(&mut self, images: usize, bytes: usize) {
        self.max_images = images;
        self.max_bytes = bytes;
    }
}

impl<D: RenderDevice> Drop for WebImageRegistry<D> {
    fn drop(&mut self) {
        self.release_context();
    }
}
