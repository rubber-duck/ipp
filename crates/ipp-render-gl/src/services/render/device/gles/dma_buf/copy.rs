use super::super::GlesRenderDevice;
use super::{GlesDmaBufFormat, GlesDmaBufImage, GlesDmaBufImport, state::State};
use crate::RenderError;
use std::{os::fd::AsRawFd, ptr};

pub(in super::super) struct GlesDmaBufCopy {
    program: u32,
    vao: u32,
}

impl GlesRenderDevice {
    /// Copy a callback-borrowed full BGRA, single-plane, linear frame into fresh
    /// renderer-owned sRGB storage allocated by this device's `create_texture`.
    /// The caller reserves publication ownership; this method never publishes.
    ///
    /// GPU reads of producer memory finish before return, including error paths.
    /// No descriptor, EGL image or producer storage is retained. GL state touched
    /// here is restored exactly, so this can run between ordinary renderer draws.
    pub fn copy_dma_buf(
        &mut self,
        importer: &GlesDmaBufImport,
        image: &GlesDmaBufImage<'_>,
        destination: &u32,
    ) -> Result<(), RenderError> {
        importer.check_current()?;
        if image.format != GlesDmaBufFormat::Bgra8888
            || image.modifier != 0
            || image.planes.len() != 1
        {
            return Err(RenderError::RenderDevice(
                "DMA-BUF requires BGRA, one plane and linear modifier".into(),
            ));
        }
        if image.width == 0
            || image.height == 0
            || image.width > self.max_texture_size
            || image.height > self.max_texture_size
            || image.width > self.max_viewport[0] as u32
            || image.height > self.max_viewport[1] as u32
            || image.width > i32::MAX as u32
            || image.height > i32::MAX as u32
            || self.texture_extents.get(destination) != Some(&(image.width, image.height))
        {
            return Err(RenderError::RenderDevice(
                "DMA-BUF destination must be matching device-owned sRGB texture".into(),
            ));
        }
        let plane = image.planes[0];
        if image
            .width
            .checked_mul(4)
            .is_none_or(|minimum| plane.stride < minimum)
            || plane.stride > i32::MAX as u32
            || plane.offset > i32::MAX as u64
        {
            return Err(RenderError::RenderDevice(
                "DMA-BUF plane stride/offset unavailable".into(),
            ));
        }
        let attributes = [
            0x3057,
            image.width as i32,
            0x3056,
            image.height as i32,
            0x3271,
            i32::from_le_bytes(*b"AR24"),
            0x3272,
            plane.fd.as_raw_fd(),
            0x3273,
            plane.offset as i32,
            0x3274,
            plane.stride as i32,
            0x3443,
            0,
            0x3444,
            0,
            0x3038,
        ];
        self.check()?;
        let state = State::save(&self.gl, importer.srgb_write_control);
        let mut imported = 0;
        let mut framebuffer = 0;
        let mut egl_image = ptr::null_mut();
        let result = (|| {
            self.prepare_dma_buf_copy()?;
            // SAFETY: The caller borrows a stable producer allocation for this
            // entire operation. EGL reads the attributes synchronously and keeps
            // its own image reference; we do not close or retain the borrowed FD.
            egl_image = unsafe {
                (importer.create)(
                    importer.display,
                    ptr::null_mut(),
                    0x3270,
                    ptr::null_mut(),
                    attributes.as_ptr(),
                )
            };
            if egl_image.is_null() {
                return Err(RenderError::RenderDevice(
                    "EGL DMA-BUF import unavailable".into(),
                ));
            }
            let copy = self.dma_buf_copy.as_ref().ok_or_else(|| {
                RenderError::RenderDevice("DMA-BUF copy program unavailable".into())
            })?;
            state.prepare(&self.gl);
            // SAFETY: All allocations belong to the current device context; stack
            // outputs are exclusive. The imported EGL image remains live through
            // finish below, and the destination allocation was validated above.
            unsafe {
                self.gl.gen_textures(1, &mut imported);
                self.gl.gen_framebuffers(1, &mut framebuffer);
                if imported == 0 || framebuffer == 0 {
                    return Err(RenderError::RenderDevice(
                        "DMA-BUF staging allocation failed".into(),
                    ));
                }
                self.gl.bind_texture(0x0DE1, imported);
                (importer.image_target)(0x0DE1, egl_image);
                self.gl.tex_parameter(0x0DE1, 0x2801, 0x2600);
                self.gl.tex_parameter(0x0DE1, 0x2800, 0x2600);
                self.gl.tex_parameter(0x0DE1, 0x2802, 0x812F);
                self.gl.tex_parameter(0x0DE1, 0x2803, 0x812F);
                self.gl.bind_framebuffer(0x8D40, framebuffer);
                self.gl
                    .framebuffer_texture(0x8D40, 0x8CE0, 0x0DE1, *destination, 0);
                if self.gl.check_framebuffer(0x8D40) != 0x8CD5 {
                    return Err(RenderError::RenderDevice(
                        "DMA-BUF copy framebuffer incomplete".into(),
                    ));
                }
                self.gl
                    .viewport(0, 0, image.width as i32, image.height as i32);
                self.gl.use_program(copy.program);
                self.gl.bind_vertex_array(copy.vao);
            }
            self.check()?;
            // SAFETY: The private vertex shader uses gl_VertexID; the live
            // imported image is bound and read only until finish below completes.
            unsafe { self.gl.draw_arrays(0x0004, 0, 3) };
            self.check()
        })();

        // CEF and similar producers may reuse the pool immediately on return.
        // A submitted fence alone does not extend that lifetime. Always finish
        // any possible imported-image reads, even following an earlier GL error.
        if !egl_image.is_null() {
            // SAFETY: The current context remains live. glFinish takes no pointers
            // and completes every possible read before external image destruction.
            unsafe { self.gl.finish() };
        }
        let finished = self.check();
        state.restore(&self.gl);
        // SAFETY: All transient names are exclusively owned here, no longer
        // bound, and finished above. Zero names are legal deletion no-ops.
        unsafe {
            self.gl.delete_framebuffers(1, &framebuffer);
            self.gl.delete_textures(1, &imported);
            if !egl_image.is_null() {
                (importer.destroy)(importer.display, egl_image);
            }
        }
        let restored = self.check();
        result.and(finished).and(restored)
    }

    fn prepare_dma_buf_copy(&mut self) -> Result<(), RenderError> {
        if self.dma_buf_copy.is_some() {
            return Ok(());
        }
        let vertex = self.shader(0x8B31, include_str!("copy.vert"))?;
        let fragment = match self.shader(0x8B30, include_str!("copy.frag")) {
            Ok(shader) => shader,
            Err(error) => {
                // SAFETY: This unpublished shader is owned by this context.
                unsafe { self.gl.delete_shader(vertex) };
                return Err(error);
            }
        };
        let mut vao = 0;
        // SAFETY: Exact GL calls on the owning current context; shader/program
        // names are private here, and all outputs are exclusive stack storage.
        let (program, linked) = unsafe {
            let program = self.gl.create_program();
            self.gl.attach_shader(program, vertex);
            self.gl.attach_shader(program, fragment);
            self.gl.link_program(program);
            self.gl.delete_shader(vertex);
            self.gl.delete_shader(fragment);
            let mut linked = 0;
            self.gl.program_iv(program, 0x8B82, &mut linked);
            self.gl.gen_vertex_arrays(1, &mut vao);
            (program, linked)
        };
        let result = if linked == 0 || vao == 0 {
            Err(RenderError::RenderDevice(format!(
                "DMA-BUF copy program: {}",
                self.log(program, false)
            )))
        } else {
            self.check()
        };
        if let Err(error) = result {
            // SAFETY: Failed unpublished allocations remain exclusively owned.
            unsafe {
                self.gl.delete_program(program);
                self.gl.delete_vertex_arrays(1, &vao);
            }
            return Err(error);
        }
        self.dma_buf_copy = Some(GlesDmaBufCopy {
            program,
            vao,
        });
        Ok(())
    }

    pub(in super::super) fn release_dma_buf_copy(&mut self) {
        if let Some(copy) = self.dma_buf_copy.take() {
            // SAFETY: Device drop keeps its context current; these private names
            // have no borrowed external storage and are invalidated before delete.
            unsafe {
                self.gl.delete_program(copy.program);
                self.gl.delete_vertex_arrays(1, &copy.vao);
            }
        }
    }
}
