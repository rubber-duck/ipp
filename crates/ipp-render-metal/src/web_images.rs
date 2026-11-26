//! Metal-owned accelerated web textures leased to exact World publications.

use std::{collections::BTreeMap, ffi::c_void};

use ipp_core::{
    HostRuntime, WorldPublicationId,
    services::web_content::{WebContentImageDescription, WebContentImageId, WebContentRequestId},
};

use crate::device::{Device, Object};

const MAX_WEB_IMAGES: usize = 32;
const MAX_WEB_IMAGE_BYTES: usize = 128 * 1024 * 1024;

/// The private linear-premultiplied texture produced from a borrowed IOSurface.
pub struct MetalWebImage {
    texture: Object,
    size: [u32; 2],
}

#[derive(Default)]
pub(super) struct WebImages {
    images: BTreeMap<WebContentImageId, MetalWebImage>,
}

impl WebImages {
    pub(super) fn resource(
        &self,
        host: &HostRuntime,
        publication: WorldPublicationId,
        id: WebContentImageId,
    ) -> Option<&Object> {
        let description = host.publication_web_image(publication, id)?;
        let image = self.images.get(&id)?;
        (image.size == description.pixel_size).then_some(&image.texture)
    }

    pub(super) fn prune(&mut self, host: &HostRuntime) {
        self.images
            .retain(|&id, _| host.web_content().is_image_retained(id));
    }

    pub(super) fn can_reserve(&self, bytes: usize, count: usize) -> bool {
        let used = self
            .images
            .values()
            .map(|image| private_bytes(image.size))
            .sum::<usize>();
        self.images.len().saturating_add(count) <= MAX_WEB_IMAGES
            && used.saturating_add(bytes) <= MAX_WEB_IMAGE_BYTES
    }

    pub(super) fn complete(
        &mut self,
        host: &mut HostRuntime,
        request: WebContentRequestId,
        id: WebContentImageId,
        image: MetalWebImage,
    ) -> Result<(), String> {
        host.web_content_mut()
            .complete_image(request, id)
            .map_err(|error| error.to_string())?;
        self.images.insert(id, image);
        self.prune(host);
        Ok(())
    }
}

pub(super) fn reserve(
    host: &mut HostRuntime,
    request: WebContentRequestId,
    size: [u32; 2],
) -> Result<WebContentImageId, String> {
    host.web_content_mut()
        .reserve_image(
            request,
            WebContentImageDescription {
                pixel_size: size,
            },
        )
        .map_err(|error| error.to_string())
}

impl MetalWebImage {
    pub(super) unsafe fn import(
        device: &Device,
        surface: *mut c_void,
        size: [u32; 2],
        budget: usize,
    ) -> Result<Self, String> {
        // SAFETY: The adapter lends the callback-scoped IOSurface. Device import
        // waits for all GPU reads before allowing the adapter to return it.
        let texture = unsafe { device.import_web_image(surface, size, budget) }?;
        Ok(Self {
            texture,
            size,
        })
    }
}

/// Bytes of private storage used for one linear RGBA16Float web image.
pub fn private_bytes([width, height]: [u32; 2]) -> usize {
    (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(8)
}
