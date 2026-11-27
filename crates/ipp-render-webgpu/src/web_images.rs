//! Immutable browser images admitted by the Host for completed Canvas publications.
use crate::{
    BrowserSurface,
    device::{COLOR, texture_descriptor},
};
use ipp_core::{
    HostRuntime,
    services::web_content::{WebContentImageDescription, WebContentImageId, WebContentRequestId},
};
use std::{
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use wasm_bindgen::JsValue;

/// One outstanding admission. Dropping it releases the private image on the next sweep.
pub struct WebImageReservation {
    image: WebContentImageId,
    request: WebContentRequestId,
    _lifetime: Rc<()>,
}

struct Image {
    texture: wgpu::Texture,
    binding: wgpu::BindGroup,
    pending: Option<Weak<()>>,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct WebImages {
    images: BTreeMap<WebContentImageId, Image>,
    pub invalidated: bool,
}

impl WebImages {
    pub fn binding(&self, id: WebContentImageId) -> Option<wgpu::BindGroup> {
        let image = self.images.get(&id)?;
        image.pending.is_none().then(|| image.binding.clone())
    }
}

impl BrowserSurface {
    /// Release cancelled private images and images no longer retained by any publication.
    pub fn refresh_web_images(&self, host: &mut HostRuntime) {
        let mut registry = self.gpu.web_images.borrow_mut();
        let mut invalidated = false;
        registry.images.retain(|id, image| {
            if image.pending.is_none()
                && !host
                    .web_content()
                    .image_validity(*id)
                    .is_some_and(|v| v.is_valid())
            {
                invalidated = true;
                return false;
            }
            if image
                .pending
                .as_ref()
                .is_some_and(|ticket| ticket.strong_count() == 0)
            {
                host.web_content_mut().discard_image(*id);
            }
            host.web_content().is_image_retained(*id)
        });
        registry.invalidated |= invalidated;
    }

    /// Reserve both producer staging and immutable destination storage before capture.
    pub fn reserve_web_image(
        &self,
        host: &mut HostRuntime,
        request: WebContentRequestId,
        description: WebContentImageDescription,
        staging: usize,
    ) -> Result<WebImageReservation, String> {
        self.refresh_web_images(host);
        let [width, height] = description.pixel_size;
        if [width, height].iter().any(|v| !(1..=4096).contains(v)) {
            return Err("Invalid browser image extent".into());
        }
        let bytes = width as usize * height as usize * 4;
        let mut images = self.gpu.web_images.borrow_mut();
        let used: usize = images
            .images
            .values()
            .map(|i| i.bytes * (1 + usize::from(i.pending.is_some())))
            .sum();
        if images.images.len() >= 256
            || used.saturating_add(bytes).saturating_add(staging) > 256 << 20
        {
            return Err("HTML image capacity exhausted".into());
        }
        let image = host
            .web_content_mut()
            .reserve_image(request, description)
            .map_err(|e| e.to_string())?;
        let texture = self.gpu.device.create_texture(&texture_descriptor(
            "Browser content",
            [width, height],
            COLOR,
            wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
        ));
        let binding = self
            .gpu
            .canvas
            .bind(&self.gpu.device, &texture.create_view(&Default::default()));
        let lifetime = Rc::new(());
        images.images.insert(
            image,
            Image {
                texture,
                binding,
                pending: Some(Rc::downgrade(&lifetime)),
                bytes,
            },
        );
        Ok(WebImageReservation {
            image,
            request,
            _lifetime: lifetime,
        })
    }

    /// Import a one-use browser snapshot into the exact reserved texture.
    pub fn import_web_image(
        &self,
        host: &mut HostRuntime,
        ticket: WebImageReservation,
        stage: u32,
        upload: &js_sys::Function,
    ) -> Result<(), String> {
        let mut images = self.gpu.web_images.borrow_mut();
        let image = images
            .images
            .get_mut(&ticket.image)
            .ok_or("HTML image reservation expired")?;
        upload
            .call2(
                &JsValue::UNDEFINED,
                &stage.into(),
                image.texture.as_webgpu().expect("browser texture").as_ref(),
            )
            .map_err(|e| format!("{e:?}"))?;
        host.web_content_mut()
            .complete_image(ticket.request, ticket.image)
            .map_err(|e| e.to_string())?;
        image.pending = None;
        Ok(())
    }
}
