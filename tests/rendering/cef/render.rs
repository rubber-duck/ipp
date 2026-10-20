//! CEF converts callback metadata; the GLES device owns import and synchronization.
use crate::{egl, state_sentinels};
use cef::AcceleratedPaintInfo;
use ipp_render_gl::{
    GlesDmaBufFormat, GlesDmaBufImage, GlesDmaBufImport, GlesDmaBufPlane, GlesRenderDevice,
    RenderDevice,
};
use std::{os::fd::BorrowedFd, path::Path};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Renderer {
    device: GlesRenderDevice,
    importer: GlesDmaBufImport,
    bitmap: Option<<GlesRenderDevice as RenderDevice>::Program>,
    texture: Option<(u32, i32, i32)>,
    context: egl::Context,
}

impl Renderer {
    pub fn new(directory: &Path) -> Result<Self> {
        let context = egl::Context::new(directory, 800, 600)?;
        eprintln!("{}", context.info()?);
        let mut device = state_sentinels::device(&context)?;
        // SAFETY: The context supplies exact EGL/GLES pointers and remains live
        // through both importer and device drop on this thread. It is current.
        let importer = unsafe { GlesDmaBufImport::from_loader(|name| context.gl(name))? };
        let bitmap = device.create_program(
            include_str!(
                "../../../crates/ipp-render-gl/src/services/render/shaders/surface_bitmap.vert"
            ),
            include_str!(
                "../../../crates/ipp-render-gl/src/services/render/shaders/surface_bitmap.frag"
            ),
        )?;
        Ok(Self {
            device,
            importer,
            bitmap: Some(bitmap),
            texture: None,
            context,
        })
    }

    pub fn import(&mut self, info: &AcceleratedPaintInfo) -> Result<()> {
        let (width, height) = (info.extra.coded_size.width, info.extra.coded_size.height);
        if info.plane_count != 1 || !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
            return Err("unsupported CEF image dimensions/planes".into());
        }
        let visible = &info.extra.visible_rect;
        if visible.x != 0 || visible.y != 0 || visible.width != width || visible.height != height {
            return Err("unsupported cropped CEF shared image".into());
        }
        if *info.format.as_ref() != cef::sys::cef_color_type_t::CEF_COLOR_TYPE_BGRA_8888
            || info.modifier != 0
            || info.planes[0].fd < 0
        {
            return Err("only BGRA one-plane linear CEF images are supported".into());
        }
        self.context.ensure_current()?;
        // Each delivery gets private storage, matching the renderer reservation
        // contract. The old completed image stays untouched until this succeeds.
        let texture = self
            .device
            .create_texture(width as u32, height as u32, &[])?;
        let plane = &info.planes[0];
        let planes = [GlesDmaBufPlane {
            // SAFETY: CEF owns this valid descriptor and stable pixels through
            // callback return; device copy finishes all GPU reads before then.
            fd: unsafe { BorrowedFd::borrow_raw(plane.fd) },
            stride: plane.stride,
            offset: plane.offset,
        }];
        let image = GlesDmaBufImage {
            width: width as u32,
            height: height as u32,
            format: GlesDmaBufFormat::Bgra8888,
            modifier: info.modifier,
            planes: &planes,
        };
        if let Err(error) = state_sentinels::copy(
            &self.context,
            &mut self.device,
            &self.importer,
            &image,
            &texture,
        ) {
            self.device.delete_texture(texture);
            return Err(error);
        }
        if let Some((old, _, _)) = self.texture.replace((texture, width, height)) {
            self.device.delete_texture(old);
        }
        Ok(())
    }

    pub fn capture(&mut self) -> Result<Vec<u8>> {
        self.context.ensure_current()?;
        let (texture, _, _) = self.texture.ok_or("no accelerated frame")?;
        self.device
            .begin_frame(800, 600, &[0.015, 0.025, 0.045, 1.0])?;
        self.device.draw_surface_bitmap(
            self.bitmap.as_ref().ok_or("bitmap program unavailable")?,
            &texture,
            &[
                2.0 / 800.0,
                0.0,
                0.0,
                0.0,
                0.0,
                -2.0 / 600.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                -1.0,
                1.0,
                0.0,
                1.0,
            ],
            &[80.0, 100.0, 640.0, 400.0],
            &[80.0, 100.0, 720.0, 500.0],
            &[1.0; 4],
        )?;
        self.device.end_frame()?;
        self.context.capture()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if self.context.ensure_current().is_ok() {
            if let Some((texture, _, _)) = self.texture.take() {
                self.device.delete_texture(texture);
            }
            if let Some(program) = self.bitmap.take() {
                self.device.delete_program(program);
            }
        }
    }
}
