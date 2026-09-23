#[cfg(feature = "gui")]
use super::super::retained_vertices::{
    GLYPH_VERTEX_LAYOUT, GUI_BOX_VERTEX_LAYOUT, RetainedVertexLayout,
};
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
    pub(super) fn create_gui_batch(
        &mut self,
        vertices: &[crate::services::render::gui_batch::GuiBoxVertex],
    ) -> Result<super::GlesGuiBatch, RenderError> {
        self.submission.invalidate();
        let vertex_count = i32::try_from(vertices.len())
            .map_err(|_| RenderError::RenderDevice("too many gui batch vertices".into()))?;
        let bytes = std::mem::size_of_val(vertices);
        let mut vao = 0;
        let mut vbo = 0;
        // SAFETY: GL allocates exclusive handles for the current context. BufferData
        // copies the live `vertices` slice synchronously, and the attribute offsets
        // index that newly bound buffer rather than client memory.
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
            (self.gl.buffer_data)(
                ARRAY_BUFFER,
                bytes as isize,
                vertices.as_ptr().cast(),
                DYNAMIC_DRAW,
            );
            self.point_retained_attributes(&GUI_BOX_VERTEX_LAYOUT);
            self.bind_vertex_array(0);
            (self.gl.bind_buffer)(ARRAY_BUFFER, 0);
        }
        if let Err(error) = self.check() {
            self.delete_gui_batch(super::GlesGuiBatch {
                vao,
                vbo,
                vertex_count,
                bytes,
            });
            return Err(error);
        }
        Ok(super::GlesGuiBatch {
            vao,
            vbo,
            vertex_count,
            bytes,
        })
    }

    #[cfg(feature = "gui")]
    pub(super) fn update_gui_batch(
        &mut self,
        batch: &mut super::GlesGuiBatch,
        vertices: &[crate::services::render::gui_batch::GuiBoxVertex],
    ) -> Result<(), RenderError> {
        self.submission.invalidate();
        let vertex_count = i32::try_from(vertices.len())
            .map_err(|_| RenderError::RenderDevice("too many gui batch vertices".into()))?;
        let bytes = std::mem::size_of_val(vertices);
        // SAFETY: BufferData replaces the complete store and copies the live slice
        // synchronously. GL preserves storage needed by queued consumers; replacement
        // permits driver retirement but does not promise stall-free allocation.
        unsafe {
            (self.gl.bind_buffer)(ARRAY_BUFFER, batch.vbo);
            (self.gl.buffer_data)(
                ARRAY_BUFFER,
                bytes as isize,
                vertices.as_ptr().cast(),
                DYNAMIC_DRAW,
            );
            (self.gl.bind_buffer)(ARRAY_BUFFER, 0);
        }

        // Counts describe the store only after GL accepted the replacement.
        self.check()?;
        batch.vertex_count = vertex_count;
        batch.bytes = bytes;
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
        mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.submission.invalidate();
        self.alpha_blend(true)?;
        let clip_location = self.surface_location(program, c"u_clip");
        let viewport_location = self.surface_location(program, c"u_viewport");
        // SAFETY: The VAO encapsulates vertex attribute pointers; draw_arrays draws the
        // current vertex count. Uniforms are synchronously copied.
        unsafe {
            (self.gl.use_program)(program.id);
            (self.gl.uniform_matrix)(program.mvp, 1, 0, mvp.as_ptr());
            (self.gl.uniform_vec4)(clip_location, 1, clip.as_ptr());
            let viewport = [self.surface_viewport[0], self.surface_viewport[1], 0.0, 0.0];
            (self.gl.uniform_vec4)(viewport_location, 1, viewport.as_ptr());
            self.bind_vertex_array(batch.vao);
            (self.gl.draw_arrays)(TRIANGLES, 0, batch.vertex_count);
            self.bind_vertex_array(0);
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

    #[cfg(feature = "gui")]
    pub(super) fn create_glyph_batch(
        &mut self,
        vertices: &[crate::services::render::glyph_atlas::GlyphVertex],
    ) -> Result<super::GlesGlyphBatch, RenderError> {
        self.submission.invalidate();
        let vertex_count = i32::try_from(vertices.len())
            .map_err(|_| RenderError::RenderDevice("too many glyph batch vertices".into()))?;
        let bytes = std::mem::size_of_val(vertices);
        let mut vao = 0;
        let mut vbo = 0;

        // SAFETY: GL allocates exclusive handles for the current context. BufferData
        // copies the live `vertices` slice synchronously, and the attribute offsets
        // index that newly bound buffer rather than client memory.
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
                    "glyph batch allocation failed".into(),
                ));
            }
            self.bind_vertex_array(vao);
            (self.gl.bind_buffer)(ARRAY_BUFFER, vbo);
            (self.gl.buffer_data)(
                ARRAY_BUFFER,
                bytes as isize,
                vertices.as_ptr().cast(),
                DYNAMIC_DRAW,
            );
            self.point_retained_attributes(&GLYPH_VERTEX_LAYOUT);
            self.bind_vertex_array(0);
            (self.gl.bind_buffer)(ARRAY_BUFFER, 0);
        }

        if let Err(error) = self.check() {
            self.delete_glyph_batch(super::GlesGlyphBatch {
                vao,
                vbo,
                vertex_count,
                bytes,
            });
            return Err(error);
        }

        Ok(super::GlesGlyphBatch {
            vao,
            vbo,
            vertex_count,
            bytes,
        })
    }

    #[cfg(feature = "gui")]
    pub(super) fn update_glyph_batch(
        &mut self,
        batch: &mut super::GlesGlyphBatch,
        vertices: &[crate::services::render::glyph_atlas::GlyphVertex],
    ) -> Result<(), RenderError> {
        self.submission.invalidate();
        let vertex_count = i32::try_from(vertices.len())
            .map_err(|_| RenderError::RenderDevice("too many glyph batch vertices".into()))?;
        let bytes = std::mem::size_of_val(vertices);

        // SAFETY: BufferData replaces the complete store and copies the live slice
        // synchronously. GL preserves storage needed by queued consumers; replacement
        // permits driver retirement but does not promise stall-free allocation.
        unsafe {
            (self.gl.bind_buffer)(ARRAY_BUFFER, batch.vbo);
            (self.gl.buffer_data)(
                ARRAY_BUFFER,
                bytes as isize,
                vertices.as_ptr().cast(),
                DYNAMIC_DRAW,
            );
            (self.gl.bind_buffer)(ARRAY_BUFFER, 0);
        }

        // Counts describe the store only after GL accepted the replacement.
        self.check()?;
        batch.vertex_count = vertex_count;
        batch.bytes = bytes;
        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(super) fn delete_glyph_batch(&mut self, batch: super::GlesGlyphBatch) {
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
    pub(super) fn draw_glyph_batch(
        &mut self,
        program: &GlesRenderProgram,
        batch: &super::GlesGlyphBatch,
        atlas_texture: &u32,
        mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.submission.invalidate();
        self.alpha_blend(true)?;
        let clip_location = self.surface_location(program, c"u_clip");
        let atlas_location = self.surface_location(program, c"u_atlas");

        // SAFETY: Attribute pointers are bound in VAO; draw_arrays draws vertex_count.
        // Texture and uniforms are bound and synchronously copied.
        unsafe {
            (self.gl.use_program)(program.id);
            (self.gl.uniform_matrix)(program.mvp, 1, 0, mvp.as_ptr());
            (self.gl.uniform_vec4)(clip_location, 1, clip.as_ptr());
            (self.gl.uniform_int)(atlas_location, 0);
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(0x0DE1, *atlas_texture);
            self.bind_vertex_array(batch.vao);
            (self.gl.draw_arrays)(TRIANGLES, 0, batch.vertex_count);
            self.bind_vertex_array(0);
            (self.gl.bind_texture)(0x0DE1, 0);
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

        // SAFETY: Context owns new texture and framebuffer handles. Texture is initialized
        // to RGBA8 with linear filtering and edge clamping, and cleared to zero alpha.
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
                0x8058, // RGBA8
                width as i32,
                height as i32,
                0,
                0x1908, // RGBA
                0x1401, // UNSIGNED_BYTE
                ptr::null(),
            );
            let mut draw = 0;
            let mut read = 0;
            let mut vp = [0i32; 4];
            (self.gl.get_integer)(0x8CA6, &mut draw);
            (self.gl.get_integer)(0x8CAA, &mut read);
            (self.gl.get_integer)(0x0BA2, vp.as_mut_ptr());
            (self.gl.bind_framebuffer)(0x8D40, framebuffer);
            (self.gl.framebuffer_texture)(0x8D40, 0x8CE0, 0x0DE1, texture, 0);
            let complete = (self.gl.check_framebuffer)(0x8D40) == 0x8CD5;
            (self.gl.viewport)(0, 0, width as i32, height as i32);
            (self.gl.clear_color)(0.0, 0.0, 0.0, 0.0);
            (self.gl.clear)(0x00004000); // COLOR_BUFFER_BIT
            (self.gl.bind_framebuffer)(0x8CA9, draw as u32);
            (self.gl.bind_framebuffer)(0x8CA8, read as u32);
            (self.gl.viewport)(vp[0], vp[1], vp[2], vp[3]);
            (self.gl.bind_texture)(0x0DE1, 0);
            complete && texture != 0 && framebuffer != 0
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

        // Switching between pages keeps the host target saved by the first begin.
        if self.glyph_atlas_target.is_none() {
            // SAFETY: Saves the borrowed host framebuffer and viewport bindings before
            // directing rendering into the atlas page framebuffer.
            let (draw, read, viewport) = unsafe {
                let mut draw = 0;
                let mut read = 0;
                let mut vp = [0i32; 4];
                (self.gl.get_integer)(0x8CA6, &mut draw);
                (self.gl.get_integer)(0x8CAA, &mut read);
                (self.gl.get_integer)(0x0BA2, vp.as_mut_ptr());
                (draw as u32, read as u32, vp)
            };

            self.glyph_atlas_target = Some((draw, read, viewport, self.surface_viewport));
        }
        self.surface_viewport = [page.width as f32, page.height as f32];

        // SAFETY: Directs subsequent draw commands to the atlas page framebuffer and viewport.
        unsafe {
            (self.gl.bind_framebuffer)(0x8D40, page.framebuffer);
            (self.gl.viewport)(0, 0, page.width as i32, page.height as i32);
        }

        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(super) fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        self.submission.invalidate();

        if let Some((draw, read, viewport, surface_vp)) = self.glyph_atlas_target.take() {
            self.surface_viewport = surface_vp;

            // SAFETY: Restores the saved host framebuffer bindings and viewport.
            unsafe {
                (self.gl.bind_framebuffer)(0x8CA9, draw);
                (self.gl.bind_framebuffer)(0x8CA8, read);
                (self.gl.viewport)(viewport[0], viewport[1], viewport[2], viewport[3]);
            }
        }

        self.check()
    }
}
