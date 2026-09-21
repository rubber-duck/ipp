use super::{GlesRenderDevice, GlesRenderProgram, GlesSurfacePath};
use crate::RenderError;

impl GlesRenderDevice {
    fn surface_location(&self, program: &GlesRenderProgram, name: &'static std::ffi::CStr) -> i32 {
        let key = name.to_string_lossy();
        let mut locations = program.parameter_locations.borrow_mut();
        *locations.entry(key.into_owned()).or_insert_with(|| {
            // SAFETY: The program is live and GL borrows this static name only for the call.
            unsafe { (self.gl.uniform_location)(program.id, name.as_ptr()) }
        })
    }

    pub(super) fn create_surface_path(
        &mut self,
        _bounds: &[f32; 4],
        segments: &[[f32; 8]],
        bands: &[[u32; 2]],
    ) -> Result<GlesSurfacePath, RenderError> {
        if segments.is_empty() {
            return Err(RenderError::RenderDevice(
                "surface path exceeds device limits".into(),
            ));
        }
        let texels = segments.len().saturating_mul(2);
        let width = texels.min(self.max_texture_size as usize & !1).max(2);
        let height = texels.div_ceil(width);
        if height > self.max_texture_size as usize {
            return Err(RenderError::RenderDevice(
                "surface path exceeds device limits".into(),
            ));
        }
        let mut upload = vec![0.0f32; width * height * 4];
        upload[..segments.len() * 8].copy_from_slice(segments.as_flattened());
        let band_texels = bands.len();
        let band_width = band_texels.min(self.max_texture_size as usize).max(1);
        let band_height = band_texels.div_ceil(band_width);
        if bands.is_empty() || band_height > self.max_texture_size as usize {
            return Err(RenderError::RenderDevice(
                "surface bands exceed device limits".into(),
            ));
        }
        let mut band_upload = vec![[0u32; 2]; band_width * band_height];
        band_upload[..bands.len()].copy_from_slice(bands);
        let mut path = GlesSurfacePath {
            texture: 0,
            band_texture: 0,
            vao: 0,
            segment_count: segments.len() as i32,
            texture_width: width as i32,
            band_count: bands.len() as u32,
            band_width: band_width as i32,
        };
        // SAFETY: GL writes exclusive names and synchronously copies the packed
        // immutable f32 slice. No CPU pointer survives texture upload.
        unsafe {
            (self.gl.gen_textures)(1, &mut path.texture);
            (self.gl.gen_textures)(1, &mut path.band_texture);
            (self.gl.gen_vertex_arrays)(1, &mut path.vao);
            if path.texture == 0 || path.band_texture == 0 || path.vao == 0 {
                self.delete_surface_path(path);
                return Err(RenderError::RenderDevice(
                    "surface path allocation failed".into(),
                ));
            }
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(0x0DE1, path.texture);
            (self.gl.bind_buffer)(0x88EC, 0);
            (self.gl.pixel_store)(0x0CF5, 1);
            (self.gl.tex_parameter)(0x0DE1, 0x2801, 0x2600);
            (self.gl.tex_parameter)(0x0DE1, 0x2800, 0x2600);
            (self.gl.tex_parameter)(0x0DE1, 0x2802, 0x812F);
            (self.gl.tex_parameter)(0x0DE1, 0x2803, 0x812F);
            (self.gl.tex_image)(
                0x0DE1,
                0,
                0x8814,
                width as i32,
                height as i32,
                0,
                0x1908,
                0x1406,
                upload.as_ptr().cast(),
            );
            (self.gl.bind_texture)(0x0DE1, path.band_texture);
            (self.gl.tex_parameter)(0x0DE1, 0x2801, 0x2600);
            (self.gl.tex_parameter)(0x0DE1, 0x2800, 0x2600);
            (self.gl.tex_parameter)(0x0DE1, 0x2802, 0x812F);
            (self.gl.tex_parameter)(0x0DE1, 0x2803, 0x812F);
            (self.gl.tex_image)(
                0x0DE1,
                0,
                0x823C,
                band_width as i32,
                band_height as i32,
                0,
                0x8228,
                0x1405,
                band_upload.as_ptr().cast(),
            );
            (self.gl.bind_texture)(0x0DE1, 0);
        }
        if let Err(error) = self.check() {
            self.delete_surface_path(path);
            return Err(error);
        }
        Ok(path)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_surface_path(
        &mut self,
        program: &GlesRenderProgram,
        path: &GlesSurfacePath,
        bounds: &[f32; 4],
        descriptor: super::SurfacePathDescriptor,
        mvp: &[f32; 16],
        placement: &[f32; 4],
        clip: &[f32; 4],
        color: &[f32; 4],
        fill_rule: u32,
    ) -> Result<(), RenderError> {
        self.submission.invalidate();
        if descriptor.curve_range[1] == 0
            || descriptor.curve_range[0].saturating_add(descriptor.curve_range[1])
                > path.segment_count as u32
            || descriptor.band_offset.saturating_add(32) > path.band_count
        {
            return Err(RenderError::RenderDevice(
                "invalid surface curve range".into(),
            ));
        }
        self.alpha_blend(true)?;
        let locations = (
            program.id,
            program.mvp,
            self.surface_location(program, c"u_bounds"),
            self.surface_location(program, c"u_placement"),
            self.surface_location(program, c"u_clip"),
            self.surface_location(program, c"u_color"),
            self.surface_location(program, c"u_curves"),
            self.surface_location(program, c"u_curve_count"),
            self.surface_location(program, c"u_fill_rule"),
            self.surface_location(program, c"u_curve_start"),
            self.surface_location(program, c"u_curve_width"),
            self.surface_location(program, c"u_bands"),
            self.surface_location(program, c"u_band_offset"),
            self.surface_location(program, c"u_band_width"),
            self.surface_location(program, c"u_viewport"),
        );
        // SAFETY: Uniform calls synchronously copy fixed live arrays. Handles are
        // owned by this current context; the VAO needs no vertex buffers.
        unsafe {
            (self.gl.use_program)(locations.0);
            (self.gl.uniform_matrix)(locations.1, 1, 0, mvp.as_ptr());
            (self.gl.uniform_vec4)(locations.2, 1, bounds.as_ptr());
            (self.gl.uniform_vec4)(locations.3, 1, placement.as_ptr());
            (self.gl.uniform_vec4)(locations.4, 1, clip.as_ptr());
            (self.gl.uniform_vec4)(locations.5, 1, color.as_ptr());
            (self.gl.uniform_int)(locations.6, 0);
            (self.gl.uniform_int)(locations.7, descriptor.curve_range[1] as i32);
            (self.gl.uniform_int)(locations.8, fill_rule as i32);
            (self.gl.uniform_int)(locations.9, descriptor.curve_range[0] as i32);
            (self.gl.uniform_int)(locations.10, path.texture_width);
            (self.gl.uniform_int)(locations.11, 1);
            (self.gl.uniform_int)(locations.12, descriptor.band_offset as i32);
            (self.gl.uniform_int)(locations.13, path.band_width);
            let viewport = [self.surface_viewport[0], self.surface_viewport[1], 0.0, 0.0];
            (self.gl.uniform_vec4)(locations.14, 1, viewport.as_ptr());
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(0x0DE1, path.texture);
            (self.gl.active_texture)(0x84C1);
            (self.gl.bind_texture)(0x0DE1, path.band_texture);
            (self.gl.bind_vertex_array)(path.vao);
            (self.gl.draw_arrays)(0x0005, 0, 4); // TRIANGLE_STRIP
            (self.gl.bind_texture)(0x0DE1, 0);
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_vertex_array)(0);
        }
        self.check_draw()
    }

    pub(super) fn delete_surface_path(&mut self, path: GlesSurfacePath) {
        // SAFETY: Consumes exclusive context handles; zero is accepted by GL.
        unsafe {
            (self.gl.delete_textures)(1, &path.texture);
            (self.gl.delete_textures)(1, &path.band_texture);
            (self.gl.delete_vertex_arrays)(1, &path.vao);
        }
    }

    pub(super) fn draw_surface_path_instances(
        &mut self,
        program: &GlesRenderProgram,
        path: &GlesSurfacePath,
        instances: &[super::SurfacePathInstance],
        mvp: &[f32; 16],
        clip: &[f32; 4],
        fill_rule: u32,
    ) -> Result<(), RenderError> {
        self.submission.invalidate();
        if instances.is_empty() {
            return Ok(());
        }
        if !super::super::surface_instances_exact(instances) {
            return Err(RenderError::RenderDevice(
                "surface instance atlas exceeds exact descriptor limits".into(),
            ));
        }
        if instances.iter().any(|instance| {
            instance.descriptor.curve_range[1] == 0
                || instance.descriptor.curve_range[0]
                    .saturating_add(instance.descriptor.curve_range[1])
                    > path.segment_count as u32
                || instance.descriptor.band_offset.saturating_add(32) > path.band_count
        }) {
            return Err(RenderError::RenderDevice(
                "invalid surface instance range".into(),
            ));
        }
        super::super::pack_surface_instances(instances, &mut self.surface_instance_scratch);
        self.alpha_blend(true)?;
        let bytes = std::mem::size_of_val(self.surface_instance_scratch.as_slice());
        // SAFETY: GL copies the packed instance slice synchronously. Attribute
        // pointers are byte offsets into the exclusively owned instance buffer.
        unsafe {
            if self.surface_instance_buffer == 0 {
                (self.gl.gen_buffers)(1, &mut self.surface_instance_buffer);
            }
            if self.surface_instance_buffer == 0 {
                return Err(RenderError::RenderDevice(
                    "surface instance allocation failed".into(),
                ));
            }
            (self.gl.bind_vertex_array)(path.vao);
            (self.gl.bind_buffer)(super::ARRAY_BUFFER, self.surface_instance_buffer);
            if bytes > self.surface_instance_capacity {
                self.surface_instance_capacity =
                    bytes.max(self.surface_instance_capacity.saturating_mul(2));
                (self.gl.buffer_data)(
                    super::ARRAY_BUFFER,
                    self.surface_instance_capacity as isize,
                    std::ptr::null(),
                    0x88E8,
                );
            }
            (self.gl.buffer_sub_data)(
                super::ARRAY_BUFFER,
                0,
                bytes as isize,
                self.surface_instance_scratch.as_ptr().cast(),
            );
            for slot in 0..4u32 {
                (self.gl.enable_attrib)(slot);
                (self.gl.attrib_pointer)(
                    slot,
                    4,
                    super::FLOAT,
                    0,
                    64,
                    (slot as usize * 16) as *const _,
                );
                (self.gl.attrib_divisor)(slot, 1);
            }
            (self.gl.use_program)(program.id);
            (self.gl.uniform_matrix)(program.mvp, 1, 0, mvp.as_ptr());
            (self.gl.uniform_vec4)(self.surface_location(program, c"u_clip"), 1, clip.as_ptr());
            (self.gl.uniform_int)(self.surface_location(program, c"u_curves"), 0);
            (self.gl.uniform_int)(
                self.surface_location(program, c"u_curve_width"),
                path.texture_width,
            );
            (self.gl.uniform_int)(self.surface_location(program, c"u_bands"), 1);
            (self.gl.uniform_int)(
                self.surface_location(program, c"u_band_width"),
                path.band_width,
            );
            (self.gl.uniform_int)(
                self.surface_location(program, c"u_fill_rule"),
                fill_rule as i32,
            );
            let viewport = [self.surface_viewport[0], self.surface_viewport[1], 0.0, 0.0];
            (self.gl.uniform_vec4)(
                self.surface_location(program, c"u_viewport"),
                1,
                viewport.as_ptr(),
            );
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(0x0DE1, path.texture);
            (self.gl.active_texture)(0x84C1);
            (self.gl.bind_texture)(0x0DE1, path.band_texture);
            (self.gl.draw_arrays_instances)(0x0005, 0, 4, instances.len() as i32);
            for slot in 0..4u32 {
                (self.gl.attrib_divisor)(slot, 0);
                (self.gl.disable_attrib)(slot);
            }
            (self.gl.bind_vertex_array)(0);
            (self.gl.bind_texture)(0x0DE1, 0);
            (self.gl.active_texture)(0x84C0);
        }
        self.check_draw()
    }

    #[cfg(feature = "gui")]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_surface_box(
        &mut self,
        program: &GlesRenderProgram,
        mvp: &[f32; 16],
        placement: &[f32; 4],
        clip: &[f32; 4],
        color: &[f32; 4],
        border: &[f32; 4],
        shape: super::SurfaceBoxShape,
    ) -> Result<(), RenderError> {
        self.submission.invalidate();
        self.alpha_blend(true)?;
        if self.surface_quad_vao == 0 {
            // SAFETY: GL writes one new name owned by this current context.
            unsafe { (self.gl.gen_vertex_arrays)(1, &mut self.surface_quad_vao) };
            if self.surface_quad_vao == 0 {
                return Err(RenderError::RenderDevice(
                    "surface quad allocation failed".into(),
                ));
            }
        }
        let shape = shape.pack();
        let placement_location = self.surface_location(program, c"u_placement");
        let clip_location = self.surface_location(program, c"u_clip");
        let color_location = self.surface_location(program, c"u_color");
        let border_location = self.surface_location(program, c"u_border_color");
        let shape_location = self.surface_location(program, c"u_box");
        // SAFETY: Uniform calls copy fixed arrays synchronously. The context owns
        // every handle; the procedural box needs no texture or vertex buffers.
        unsafe {
            (self.gl.use_program)(program.id);
            (self.gl.uniform_matrix)(program.mvp, 1, 0, mvp.as_ptr());
            (self.gl.uniform_vec4)(placement_location, 1, placement.as_ptr());
            (self.gl.uniform_vec4)(clip_location, 1, clip.as_ptr());
            (self.gl.uniform_vec4)(color_location, 1, color.as_ptr());
            (self.gl.uniform_vec4)(border_location, 1, border.as_ptr());
            (self.gl.uniform_vec4)(shape_location, 1, shape.as_ptr());
            (self.gl.bind_vertex_array)(self.surface_quad_vao);
            (self.gl.draw_arrays)(0x0005, 0, 4);
            (self.gl.bind_vertex_array)(0);
        }
        self.check_draw()
    }

    pub(super) fn draw_surface_bitmap(
        &mut self,
        program: &GlesRenderProgram,
        texture: &u32,
        mvp: &[f32; 16],
        placement: &[f32; 4],
        clip: &[f32; 4],
        color: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.submission.invalidate();
        self.alpha_blend(true)?;
        if self.surface_quad_vao == 0 {
            // SAFETY: GL writes one new name owned by this current context.
            unsafe { (self.gl.gen_vertex_arrays)(1, &mut self.surface_quad_vao) };
            if self.surface_quad_vao == 0 {
                return Err(RenderError::RenderDevice(
                    "surface quad allocation failed".into(),
                ));
            }
        }
        let placement_location = self.surface_location(program, c"u_placement");
        let clip_location = self.surface_location(program, c"u_clip");
        let color_location = self.surface_location(program, c"u_color");
        let texture_location = self.surface_location(program, c"u_texture");
        // SAFETY: Uniform calls copy fixed arrays synchronously. The context owns
        // every handle and the texture remains borrowed through the draw.
        unsafe {
            (self.gl.use_program)(program.id);
            (self.gl.uniform_matrix)(program.mvp, 1, 0, mvp.as_ptr());
            (self.gl.uniform_vec4)(placement_location, 1, placement.as_ptr());
            (self.gl.uniform_vec4)(clip_location, 1, clip.as_ptr());
            (self.gl.uniform_vec4)(color_location, 1, color.as_ptr());
            (self.gl.uniform_int)(texture_location, 0);
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(0x0DE1, *texture);
            (self.gl.bind_vertex_array)(self.surface_quad_vao);
            (self.gl.draw_arrays)(0x0005, 0, 4);
            (self.gl.bind_vertex_array)(0);
            (self.gl.bind_texture)(0x0DE1, 0);
        }
        self.check_draw()
    }
}
