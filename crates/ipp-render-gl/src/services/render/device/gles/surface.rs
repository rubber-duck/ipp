#[cfg(feature = "gui")]
use super::super::retained_vertices::{GUI_VERTEX_LAYOUT, RetainedVertexLayout};
#[cfg(feature = "gui")]
use super::{ARRAY_BUFFER, DYNAMIC_DRAW, FLOAT, TRIANGLES};
use super::{GlesRenderDevice, GlesRenderProgram, GlesSurfacePath};
use crate::RenderError;
#[cfg(feature = "gui")]
use std::ptr;

impl GlesRenderDevice {
    pub(super) fn surface_location(
        &self,
        program: &GlesRenderProgram,
        name: &'static std::ffi::CStr,
    ) -> i32 {
        let key = name.to_string_lossy();
        let mut locations = program.parameter_locations.borrow_mut();
        *locations.entry(key.into_owned()).or_insert_with(|| {
            // SAFETY: The program is live and GL borrows this static name only for the call.
            unsafe { (self.gl.uniform_location)(program.id, name.as_ptr()) }
        })
    }

    /// Enable and point every attribute of `layout` at the bound array buffer.
    ///
    /// # Safety
    ///
    /// The device context is current with the destination vertex array and its array
    /// buffer bound, so each offset addresses that buffer rather than client memory.
    #[cfg(feature = "gui")]
    unsafe fn point_retained_attributes<const N: usize>(&self, layout: &RetainedVertexLayout<N>) {
        for attribute in &layout.attributes {
            // SAFETY: The caller binds the destination vertex array and buffer in the
            // current context; GL copies these scalar arguments during the call.
            unsafe {
                (self.gl.enable_attrib)(attribute.location);
                (self.gl.attrib_pointer)(
                    attribute.location,
                    attribute.components as i32,
                    FLOAT,
                    0,
                    layout.stride as i32,
                    ptr::without_provenance(attribute.offset as usize),
                );
            }
        }
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
        self.use_program(program.id);
        let location = |name| self.surface_location(program, name);
        self.program_mat4(program, program.mvp, mvp);
        self.program_vec4(program, location(c"u_bounds"), bounds);
        self.program_vec4(program, location(c"u_placement"), placement);
        self.program_vec4(program, location(c"u_clip"), clip);
        self.program_vec4(program, location(c"u_color"), color);
        self.program_int(program, location(c"u_curves"), 0);
        self.program_int(
            program,
            location(c"u_curve_count"),
            descriptor.curve_range[1] as i32,
        );
        self.program_int(program, location(c"u_fill_rule"), fill_rule as i32);
        self.program_int(
            program,
            location(c"u_curve_start"),
            descriptor.curve_range[0] as i32,
        );
        self.program_int(program, location(c"u_curve_width"), path.texture_width);
        self.program_int(program, location(c"u_bands"), 1);
        self.program_int(
            program,
            location(c"u_band_offset"),
            descriptor.band_offset as i32,
        );
        self.program_int(program, location(c"u_band_width"), path.band_width);
        let viewport = [self.surface_viewport[0], self.surface_viewport[1], 0.0, 0.0];
        self.program_vec4(program, location(c"u_viewport"), &viewport);
        self.bind_vertex_array(path.vao);
        // SAFETY: Textures and the attribute-less VAO are owned by this current
        // context and stay bound only for the draw; no client pointer is read.
        unsafe {
            self.bind_path_textures(path);
            (self.gl.draw_arrays)(0x0005, 0, 4); // TRIANGLE_STRIP
            self.release_band_texture();
        }
        self.check_draw()
    }

    /// Bind a path's curve texture to unit 0 and its band texture to unit 1.
    ///
    /// # Safety
    ///
    /// The device context is current and `path` owns live textures in it.
    unsafe fn bind_path_textures(&self, path: &GlesSurfacePath) {
        // SAFETY: Upheld by the caller; binding copies scalar names only.
        unsafe {
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(0x0DE1, path.texture);
            (self.gl.active_texture)(0x84C1);
            (self.gl.bind_texture)(0x0DE1, path.band_texture);
        }
    }

    /// Unbind unit 1 after a path draw and select unit 0 again. Unit 1 also
    /// holds the shadow map, so its binding cache is cleared.
    ///
    /// # Safety
    ///
    /// The device context is current with texture unit 1 active.
    unsafe fn release_band_texture(&self) {
        // SAFETY: Upheld by the caller; unbinding copies scalar names only.
        unsafe {
            (self.gl.bind_texture)(0x0DE1, 0);
            (self.gl.active_texture)(0x84C0);
        }
        #[cfg(feature = "shadows")]
        self.submission.shadow_texture.set(None);
    }

    pub(super) fn delete_surface_path(&mut self, path: GlesSurfacePath) {
        // A deleted bound VAO rebinds zero and GL may reuse its name.
        self.submission.invalidate();

        // SAFETY: Consumes exclusive context handles; zero is accepted by GL.
        unsafe {
            (self.gl.delete_textures)(1, &path.texture);
            (self.gl.delete_textures)(1, &path.band_texture);
            (self.gl.delete_vertex_arrays)(1, &path.vao);
        }
    }

    /// Validate descriptors of `instances` against `path` and pack them.
    fn pack_surface_instances(
        &mut self,
        path: &GlesSurfacePath,
        instances: &[super::SurfacePathInstance],
    ) -> Result<i32, RenderError> {
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

        let count = i32::try_from(instances.len())
            .map_err(|_| RenderError::RenderDevice("too many surface instances".into()))?;
        super::super::pack_surface_instances(instances, &mut self.surface_instance_scratch);
        Ok(count)
    }

    pub(super) fn create_surface_instances(
        &mut self,
        path: &GlesSurfacePath,
        instances: &[super::SurfacePathInstance],
    ) -> Result<super::GlesSurfaceInstances, RenderError> {
        self.submission.invalidate();
        let count = self.pack_surface_instances(path, instances)?;
        let bytes = std::mem::size_of_val(self.surface_instance_scratch.as_slice());
        let mut vao = 0;
        let mut vbo = 0;

        // SAFETY: GL allocates exclusive handles for the current context and copies the
        // packed instance slice synchronously. Attribute pointers are byte offsets into
        // the newly bound buffer; the instanced shader reads no per-vertex attributes.
        unsafe {
            (self.gl.gen_vertex_arrays)(1, &mut vao);
            (self.gl.gen_buffers)(1, &mut vbo);
            if vao == 0 || vbo == 0 {
                if vao != 0 {
                    (self.gl.delete_vertex_arrays)(1, &vao);
                }
                if vbo != 0 {
                    (self.gl.delete_buffers)(1, &vbo);
                }
                return Err(RenderError::RenderDevice(
                    "surface instance allocation failed".into(),
                ));
            }
            self.bind_vertex_array(vao);
            (self.gl.bind_buffer)(super::ARRAY_BUFFER, vbo);
            (self.gl.buffer_data)(
                super::ARRAY_BUFFER,
                bytes as isize,
                self.surface_instance_scratch.as_ptr().cast(),
                super::STATIC_DRAW,
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
            self.bind_vertex_array(0);
            (self.gl.bind_buffer)(super::ARRAY_BUFFER, 0);
        }

        let stream = super::GlesSurfaceInstances {
            vao,
            vbo,
            count,
        };
        if let Err(error) = self.check() {
            self.delete_surface_instances(stream);
            return Err(error);
        }

        Ok(stream)
    }

    pub(super) fn update_surface_instances(
        &mut self,
        stream: &mut super::GlesSurfaceInstances,
        path: &GlesSurfacePath,
        instances: &[super::SurfacePathInstance],
    ) -> Result<(), RenderError> {
        let count = self.pack_surface_instances(path, instances)?;
        let bytes = std::mem::size_of_val(self.surface_instance_scratch.as_slice());

        // SAFETY: BufferData replaces the complete store and copies the packed slice
        // synchronously. GL preserves storage needed by queued draws.
        unsafe {
            (self.gl.bind_buffer)(super::ARRAY_BUFFER, stream.vbo);
            (self.gl.buffer_data)(
                super::ARRAY_BUFFER,
                bytes as isize,
                self.surface_instance_scratch.as_ptr().cast(),
                super::STATIC_DRAW,
            );
            (self.gl.bind_buffer)(super::ARRAY_BUFFER, 0);
        }

        // Outside exhaustive mode this frame's end checks the replacement.
        self.check_draw()?;
        self.error_checks.note_retained_upload();
        stream.count = count;
        Ok(())
    }

    pub(super) fn delete_surface_instances(&mut self, stream: super::GlesSurfaceInstances) {
        self.submission.invalidate();

        // SAFETY: Consumes exclusive context handles; zero and invalid handles are tolerated.
        unsafe {
            if stream.vao != 0 {
                (self.gl.delete_vertex_arrays)(1, &stream.vao);
            }
            if stream.vbo != 0 {
                (self.gl.delete_buffers)(1, &stream.vbo);
            }
        }
    }

    pub(super) fn draw_surface_instances(
        &mut self,
        program: &GlesRenderProgram,
        path: &GlesSurfacePath,
        stream: &super::GlesSurfaceInstances,
        mvp: &[f32; 16],
        clip: &[f32; 4],
        fill_rule: u32,
    ) -> Result<(), RenderError> {
        if stream.count == 0 {
            return Ok(());
        }

        self.alpha_blend(true)?;
        self.use_program(program.id);
        let location = |name| self.surface_location(program, name);
        self.program_mat4(program, program.mvp, mvp);
        self.program_vec4(program, location(c"u_clip"), clip);
        self.program_int(program, location(c"u_curves"), 0);
        self.program_int(program, location(c"u_curve_width"), path.texture_width);
        self.program_int(program, location(c"u_bands"), 1);
        self.program_int(program, location(c"u_band_width"), path.band_width);
        self.program_int(program, location(c"u_fill_rule"), fill_rule as i32);
        let viewport = [self.surface_viewport[0], self.surface_viewport[1], 0.0, 0.0];
        self.program_vec4(program, location(c"u_viewport"), &viewport);
        self.bind_vertex_array(stream.vao);
        // SAFETY: The stream's vertex array points at its own live instance buffer
        // and the path's textures are live in this current context.
        unsafe {
            self.bind_path_textures(path);
            (self.gl.draw_arrays_instances)(0x0005, 0, 4, stream.count);
            self.release_band_texture();
        }
        self.check_draw()
    }

    #[cfg(feature = "gui")]
    pub(super) fn create_gui_batch(
        &mut self,
        capacity: usize,
    ) -> Result<super::GlesGuiBatch, RenderError> {
        self.submission.invalidate();
        let bytes = capacity
            .checked_mul(std::mem::size_of::<
                crate::services::render::gui_batch::GuiVertex,
            >())
            .and_then(|bytes| isize::try_from(bytes).ok())
            .filter(|_| i32::try_from(capacity).is_ok())
            .ok_or_else(|| RenderError::RenderDevice("too many gui batch vertices".into()))?;
        // GLES leaves new buffer contents undefined; storage relies on zero vertices.
        let zeros = vec![0u8; bytes as usize];
        let mut vao = 0;
        let mut vbo = 0;
        // SAFETY: GL allocates exclusive handles for the current context. BufferData
        // copies the live `zeros` slice synchronously, and the attribute offsets index
        // that newly bound buffer rather than client memory.
        unsafe {
            (self.gl.gen_vertex_arrays)(1, &mut vao);
            (self.gl.gen_buffers)(1, &mut vbo);
            if vao == 0 || vbo == 0 {
                if vao != 0 {
                    (self.gl.delete_vertex_arrays)(1, &vao);
                }
                if vbo != 0 {
                    (self.gl.delete_buffers)(1, &vbo);
                }
                return Err(RenderError::RenderDevice(
                    "gui batch allocation failed".into(),
                ));
            }
            self.bind_vertex_array(vao);
            (self.gl.bind_buffer)(ARRAY_BUFFER, vbo);
            (self.gl.buffer_data)(ARRAY_BUFFER, bytes, zeros.as_ptr().cast(), DYNAMIC_DRAW);
            self.point_retained_attributes(&GUI_VERTEX_LAYOUT);
            self.bind_vertex_array(0);
            (self.gl.bind_buffer)(ARRAY_BUFFER, 0);
        }
        let batch = super::GlesGuiBatch {
            vao,
            vbo,
            capacity,
        };
        if let Err(error) = self.check() {
            self.delete_gui_batch(batch);
            return Err(error);
        }
        Ok(batch)
    }

    #[cfg(feature = "gui")]
    pub(super) fn write_gui_batch(
        &mut self,
        batch: &mut super::GlesGuiBatch,
        first: usize,
        vertices: &[crate::services::render::gui_batch::GuiVertex],
    ) -> Result<(), RenderError> {
        if first
            .checked_add(vertices.len())
            .is_none_or(|end| end > batch.capacity)
        {
            return Err(RenderError::RenderDevice(
                "gui batch write exceeds its storage".into(),
            ));
        }
        let stride = std::mem::size_of::<crate::services::render::gui_batch::GuiVertex>();
        // SAFETY: The range lies within the allocated store, checked above. BufferSubData
        // copies the live `vertices` slice synchronously; GL keeps the previous contents
        // for queued draws that read them.
        unsafe {
            (self.gl.bind_buffer)(ARRAY_BUFFER, batch.vbo);
            (self.gl.buffer_sub_data)(
                ARRAY_BUFFER,
                (first * stride) as isize,
                std::mem::size_of_val(vertices) as isize,
                vertices.as_ptr().cast(),
            );
            (self.gl.bind_buffer)(ARRAY_BUFFER, 0);
        }

        // Outside exhaustive mode this frame's end checks the write.
        self.check_draw()?;
        self.error_checks.note_retained_upload();
        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(super) fn delete_gui_batch(&mut self, batch: super::GlesGuiBatch) {
        self.submission.invalidate();
        // SAFETY: Handle deletion is context-checked; invalid handles are tolerated.
        unsafe {
            if batch.vao != 0 {
                (self.gl.delete_vertex_arrays)(1, &batch.vao);
            }
            if batch.vbo != 0 {
                (self.gl.delete_buffers)(1, &batch.vbo);
            }
        }
    }

    #[cfg(feature = "gui")]
    pub(super) fn draw_gui_batch(
        &mut self,
        program: &GlesRenderProgram,
        batch: &super::GlesGuiBatch,
        atlas_texture: Option<&u32>,
        mvp: &[f32; 16],
        first: usize,
        count: usize,
    ) -> Result<(), RenderError> {
        if first
            .checked_add(count)
            .is_none_or(|end| end > batch.capacity)
        {
            return Err(RenderError::RenderDevice(
                "gui batch draw exceeds its storage".into(),
            ));
        }
        self.alpha_blend(true)?;
        self.use_program(program.id);
        let viewport = [self.surface_viewport[0], self.surface_viewport[1], 0.0, 0.0];
        self.program_mat4(program, program.mvp, mvp);
        self.program_vec4(
            program,
            self.surface_location(program, c"u_viewport"),
            &viewport,
        );
        self.program_int(program, self.surface_location(program, c"u_atlas"), 0);
        self.bind_vertex_array(batch.vao);
        // SAFETY: The bound VAO encapsulates this storage's attribute pointers and the
        // drawn range lies within it, checked above. A glyph range binds its live
        // atlas texture, which stays on unit 0 until rebound; box-only ranges never
        // sample unit 0.
        unsafe {
            if let Some(texture) = atlas_texture {
                (self.gl.active_texture)(0x84C0);
                (self.gl.bind_texture)(0x0DE1, *texture);
            }
            (self.gl.draw_arrays)(TRIANGLES, first as i32, count as i32);
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
        self.use_program(program.id);
        let location = |name| self.surface_location(program, name);
        self.program_mat4(program, program.mvp, mvp);
        self.program_vec4(program, location(c"u_placement"), placement);
        self.program_vec4(program, location(c"u_clip"), clip);
        self.program_vec4(program, location(c"u_color"), color);
        self.program_int(program, location(c"u_texture"), 0);
        self.bind_vertex_array(self.surface_quad_vao);
        // SAFETY: The texture and attribute-less VAO are live names of this
        // current context; unit 0 keeps the texture until another draw binds one.
        unsafe {
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(0x0DE1, *texture);
            (self.gl.draw_arrays)(0x0005, 0, 4);
        }
        self.check_draw()
    }

    #[cfg(feature = "gui")]
    pub(super) fn create_glyph_atlas_page(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<super::GlesGlyphAtlasPage, RenderError> {
        self.submission.invalidate();
        let mut texture = 0;
        let mut framebuffer = 0;

        let previous = self.current_target();

        // SAFETY: Context owns new texture and framebuffer handles. Texture is initialized
        // to single-channel R8 coverage with linear filtering and edge clamping, and
        // cleared to zero coverage. The previous draw/read framebuffers and viewport are
        // tracked state, rebound unchanged before returning.
        let complete = unsafe {
            (self.gl.gen_textures)(1, &mut texture);
            (self.gl.gen_framebuffers)(1, &mut framebuffer);
            (self.gl.bind_texture)(0x0DE1, texture);
            (self.gl.tex_parameter)(0x0DE1, 0x2801, 0x2601); // MIN_FILTER LINEAR
            (self.gl.tex_parameter)(0x0DE1, 0x2800, 0x2601); // MAG_FILTER LINEAR
            (self.gl.tex_parameter)(0x0DE1, 0x2802, 0x812F); // WRAP_S CLAMP_TO_EDGE
            (self.gl.tex_parameter)(0x0DE1, 0x2803, 0x812F); // WRAP_T CLAMP_TO_EDGE
            (self.gl.tex_image)(
                0x0DE1,
                0,
                0x8229, // R8
                width as i32,
                height as i32,
                0,
                0x1903, // RED
                0x1401, // UNSIGNED_BYTE
                ptr::null(),
            );
            (self.gl.bind_texture)(0x0DE1, 0);

            // A failed allocation must not bind and clear the borrowed host target.
            let complete = texture != 0 && framebuffer != 0 && {
                self.bind_framebuffers(framebuffer, framebuffer);
                (self.gl.framebuffer_texture)(0x8D40, 0x8CE0, 0x0DE1, texture, 0);
                (self.gl.check_framebuffer)(0x8D40) == 0x8CD5
            };
            if complete {
                self.set_viewport([0, 0, width as i32, height as i32]);
                (self.gl.clear_color)(0.0, 0.0, 0.0, 0.0);
                (self.gl.clear)(0x00004000); // COLOR_BUFFER_BIT
            }
            self.bind_framebuffers(previous.draw, previous.read);
            self.set_viewport(previous.viewport);
            complete
        };

        let checked = self.check();
        if !complete || checked.is_err() {
            // SAFETY: Releases partially allocated texture and framebuffer on failure.
            unsafe {
                if texture != 0 {
                    (self.gl.delete_textures)(1, &texture);
                }
                if framebuffer != 0 {
                    (self.gl.delete_framebuffers)(1, &framebuffer);
                }
            }

            // Preserve context loss so the service reaches recovery.
            return Err(checked.err().unwrap_or_else(|| {
                RenderError::RenderDevice("glyph atlas allocation failed".into())
            }));
        }

        Ok(super::GlesGlyphAtlasPage {
            texture,
            framebuffer,
            width,
            height,
        })
    }

    #[cfg(feature = "gui")]
    pub(super) fn delete_glyph_atlas_page(&mut self, page: super::GlesGlyphAtlasPage) {
        self.submission.invalidate();
        self.forget_framebuffer(page.framebuffer);

        // SAFETY: Context owns these handles and tolerates invalid handles.
        unsafe {
            if page.framebuffer != 0 {
                (self.gl.delete_framebuffers)(1, &page.framebuffer);
            }
            if page.texture != 0 {
                (self.gl.delete_textures)(1, &page.texture);
            }
        }
    }

    #[cfg(feature = "gui")]
    pub(super) fn begin_glyph_atlas_page(
        &mut self,
        page: &super::GlesGlyphAtlasPage,
    ) -> Result<(), RenderError> {
        self.submission.invalidate();

        // Switching between pages keeps the target saved by the first begin: the
        // host target, or a Surface cache target when population nests in a repaint.
        if self.glyph_atlas_target.is_none() {
            self.glyph_atlas_target = Some((self.current_target(), self.surface_viewport));
        }
        self.surface_viewport = [page.width as f32, page.height as f32];

        // Directs subsequent draw commands to the atlas page framebuffer and viewport.
        self.bind_framebuffers(page.framebuffer, page.framebuffer);
        self.set_viewport([0, 0, page.width as i32, page.height as i32]);

        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(super) fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        self.submission.invalidate();

        if let Some((target, surface_viewport)) = self.glyph_atlas_target.take() {
            self.surface_viewport = surface_viewport;

            // Restores the saved framebuffer bindings and viewport.
            self.bind_framebuffers(target.draw, target.read);
            self.set_viewport(target.viewport);
        }

        self.check()
    }
}
