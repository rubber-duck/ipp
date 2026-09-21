use super::*;

pub(super) struct GlesLinearTarget {
    framebuffer: u32,
    color: u32,
    depth: u32,
    vao: u32,
    program: GlesRenderProgram,
    width: u32,
    height: u32,
}

impl GlesRenderDevice {
    pub(super) fn begin_linear_target(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        if width > self.max_texture_size || height > self.max_texture_size {
            return Err(RenderError::InvalidViewport);
        }
        let mut previous = 0;
        let mut previous_read = 0;
        let mut viewport = [0; 4];
        // SAFETY: Current context writes only exclusive local state snapshots.
        unsafe {
            (self.gl.get_integer)(0x8CA6, &mut previous);
            (self.gl.get_integer)(0x8CAA, &mut previous_read);
            (self.gl.get_integer)(0x0BA2, viewport.as_mut_ptr());
        }
        if self
            .linear_target
            .as_ref()
            .is_some_and(|t| t.width != width || t.height != height)
        {
            self.release_linear_target();
        }
        if self.linear_target.is_none() {
            let program = self.create_program(
                include_str!("../../shaders/present.vert"),
                include_str!("../../shaders/present.frag"),
            )?;
            let mut target = GlesLinearTarget {
                framebuffer: 0,
                color: 0,
                depth: 0,
                vao: 0,
                program,
                width,
                height,
            };
            // SAFETY: Allocations are exclusively owned by target and use no CPU data.
            // Failure releases every partial object and restores the host framebuffer.
            let complete = unsafe {
                (self.gl.gen_framebuffers)(1, &mut target.framebuffer);
                (self.gl.gen_textures)(1, &mut target.color);
                (self.gl.gen_renderbuffers)(1, &mut target.depth);
                (self.gl.gen_vertex_arrays)(1, &mut target.vao);
                (self.gl.bind_texture)(0x0DE1, target.color);
                (self.gl.tex_parameter)(0x0DE1, 0x2801, 0x2600);
                (self.gl.tex_parameter)(0x0DE1, 0x2800, 0x2600);
                (self.gl.tex_parameter)(0x0DE1, 0x2802, 0x812F);
                (self.gl.tex_parameter)(0x0DE1, 0x2803, 0x812F);
                (self.gl.tex_image)(
                    0x0DE1,
                    0,
                    // SRGB8_ALPHA8: linear blending with perceptual storage precision.
                    // Sampling decodes back to linear for the presentation shader.
                    0x8C43,
                    width as i32,
                    height as i32,
                    0,
                    0x1908,
                    0x1401,
                    ptr::null(),
                );
                (self.gl.bind_renderbuffer)(0x8D41, target.depth);
                (self.gl.renderbuffer_storage)(0x8D41, 0x81A6, width as i32, height as i32);
                (self.gl.bind_framebuffer)(0x8D40, target.framebuffer);
                (self.gl.framebuffer_texture)(0x8D40, 0x8CE0, 0x0DE1, target.color, 0);
                (self.gl.framebuffer_renderbuffer)(0x8D40, 0x8D00, 0x8D41, target.depth);
                (self.gl.check_framebuffer)(0x8D40) == 0x8CD5
                    && target.framebuffer != 0
                    && target.color != 0
                    && target.depth != 0
                    && target.vao != 0
            };
            self.linear_target = Some(target);
            if !complete || self.check().is_err() {
                self.release_linear_target();
                // SAFETY: Restores the borrowed host handle without taking ownership.
                unsafe {
                    (self.gl.bind_framebuffer)(0x8CA9, previous as u32);
                    (self.gl.bind_framebuffer)(0x8CA8, previous_read as u32);
                }
                return Err(RenderError::RenderDevice(
                    "linear render target allocation failed".into(),
                ));
            }
        }
        self.presentation_target = Some((previous as u32, previous_read as u32, viewport));
        // SAFETY: Target belongs to this current device and survives through presentation.
        unsafe {
            (self.gl.bind_framebuffer)(0x8D40, self.linear_target.as_ref().unwrap().framebuffer);
        }
        Ok(())
    }

    pub(super) fn present_linear_target(&mut self) -> Result<(), RenderError> {
        let Some((previous, previous_read, viewport)) = self.presentation_target.take() else {
            return Ok(());
        };
        let target = self.linear_target.as_ref().expect("active linear target");
        // SAFETY: Target and program remain live; fullscreen geometry uses gl_VertexID
        // without any CPU pointer. The host framebuffer and viewport are restored.
        unsafe {
            (self.gl.bind_framebuffer)(0x8CA9, previous);
            (self.gl.bind_framebuffer)(0x8CA8, previous_read);
            (self.gl.viewport)(0, 0, target.width as i32, target.height as i32);
            (self.gl.disable)(0x0B71);
            (self.gl.disable)(0x0B44);
            (self.gl.disable)(0x0BE2);
            (self.gl.depth_mask)(0);
            self.use_program(target.program.id);
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_sampler)(0, 0);
            (self.gl.bind_texture)(0x0DE1, target.color);
            (self.gl.uniform_int)(target.program.texture, 0);
            self.bind_vertex_array(target.vao);
            (self.gl.draw_arrays)(TRIANGLES, 0, 3);
            (self.gl.viewport)(viewport[0], viewport[1], viewport[2], viewport[3]);
            (self.gl.depth_mask)(1);
        }
        self.check()
    }

    fn release_linear_target(&mut self) {
        if let Some(target) = self.linear_target.take() {
            // SAFETY: These are exclusively owned handles in the current context;
            // no later draw or Rust reference can use them after removal.
            unsafe {
                (self.gl.delete_framebuffers)(1, &target.framebuffer);
                (self.gl.delete_textures)(1, &target.color);
                (self.gl.delete_renderbuffers)(1, &target.depth);
                (self.gl.delete_vertex_arrays)(1, &target.vao);
            }
            self.delete_program(target.program);
        }
    }
}

impl Drop for GlesRenderDevice {
    fn drop(&mut self) {
        self.release_linear_target();
        // SAFETY: The embedding contract keeps the context and functions live
        // through device drop; this buffer is exclusively owned by the device.
        unsafe {
            (self.gl.delete_buffers)(1, &self.parameter_buffer);
            #[cfg(feature = "surfaces")]
            (self.gl.delete_vertex_arrays)(1, &self.surface_quad_vao);
            #[cfg(feature = "surfaces")]
            (self.gl.delete_buffers)(1, &self.surface_instance_buffer);
            #[cfg(feature = "particles")]
            (self.gl.delete_buffers)(1, &self.instance_buffer);
        }
    }
}
