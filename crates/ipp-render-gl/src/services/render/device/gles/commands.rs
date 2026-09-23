use super::*;

impl RenderDevice for GlesRenderDevice {
    fn set_exhaustive_draw_checks(&mut self, enabled: bool) {
        self.error_checks.set_exhaustive(enabled);
    }

    type Program = GlesRenderProgram;

    type Mesh = GlesRenderMesh;

    type Texture = u32;

    #[cfg(feature = "surfaces")]
    type SurfacePath = super::GlesSurfacePath;

    #[cfg(feature = "surfaces")]
    type SurfaceCacheTarget = super::GlesSurfaceCacheTarget;

    #[cfg(feature = "shadows")]
    type ShadowMap = lighting::GlesShadowMap;

    #[cfg(feature = "gui")]
    type GuiBatch = super::GlesGuiBatch;

    #[cfg(feature = "gui")]
    type GlyphBatch = super::GlesGlyphBatch;

    #[cfg(feature = "gui")]
    type GlyphAtlasPage = super::GlesGlyphAtlasPage;

    fn set_lighting(
        &mut self,
        program: &GlesRenderProgram,
        model: &[f32; 16],
        normal: &[f32; 16],
        surface: &[f32; 3],
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        self.lighting_uniforms(program, model, normal, surface, frame)
    }

    #[cfg(feature = "shadows")]
    fn shadow_map_limit(&self) -> u32 {
        self.max_texture_size
            .min(self.max_viewport[0] as u32)
            .min(self.max_viewport[1] as u32)
    }

    #[cfg(feature = "shadows")]
    fn create_shadow_map(&mut self, size: u32) -> Result<Self::ShadowMap, RenderError> {
        self.allocate_shadow(size)
    }

    #[cfg(feature = "shadows")]
    fn begin_shadow(
        &mut self,
        map: &Self::ShadowMap,
        slot: u32,
        grid: u32,
    ) -> Result<(), RenderError> {
        self.start_shadow(map, slot, grid)
    }

    #[cfg(feature = "shadows")]
    fn end_shadow(&mut self) -> Result<(), RenderError> {
        self.finish_shadow()
    }

    #[cfg(feature = "shadows")]
    fn bind_shadow(
        &mut self,
        program: &GlesRenderProgram,
        map: &Self::ShadowMap,
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        self.shadow_uniforms(program, map, frame)
    }

    #[cfg(feature = "shadows")]
    fn delete_shadow_map(&mut self, map: Self::ShadowMap) {
        self.free_shadow(map);
    }

    fn prepare_custom_parameters(
        &mut self,
        program: &GlesRenderProgram,
        words: usize,
        textures: usize,
    ) -> Result<(), RenderError> {
        self.prepare_parameter_storage(program, words, textures)
    }

    fn set_custom_parameters<'a>(
        &mut self,
        program: &GlesRenderProgram,
        words: &[u32],
        textures: impl ExactSizeIterator<Item = Result<(&'a str, &'a u32), RenderError>>,
        alpha_mode: u32,
        alpha_cutoff: f32,
    ) -> Result<(), RenderError> {
        self.upload_custom_parameters(program, words, textures, alpha_mode, alpha_cutoff)
    }

    fn set_alpha_blend(&mut self, enabled: bool) -> Result<(), RenderError> {
        self.alpha_blend(enabled)
    }

    #[cfg(feature = "surfaces")]
    fn set_surface_double_sided(&mut self, enabled: bool) -> Result<(), RenderError> {
        // SAFETY: This changes only scalar rasterization state in the current
        // context. Frame setup establishes BACK as the ordinary cull face.
        unsafe {
            if enabled {
                (self.gl.disable)(0x0B44); // CULL_FACE
            } else {
                (self.gl.cull_face)(0x0405); // BACK
                (self.gl.enable)(0x0B44); // CULL_FACE
            }
        }
        self.check_draw()
    }

    #[cfg(feature = "surfaces")]
    fn create_surface_path(
        &mut self,
        bounds: &[f32; 4],
        segments: &[[f32; 8]],
        bands: &[[u32; 2]],
    ) -> Result<Self::SurfacePath, RenderError> {
        self.create_surface_path(bounds, segments, bands)
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_path(
        &mut self,
        program: &Self::Program,
        path: &Self::SurfacePath,
        bounds: &[f32; 4],
        descriptor: super::SurfacePathDescriptor,
        mvp: &[f32; 16],
        placement: &[f32; 4],
        clip: &[f32; 4],
        color: &[f32; 4],
        fill_rule: u32,
    ) -> Result<(), RenderError> {
        self.draw_surface_path(
            program, path, bounds, descriptor, mvp, placement, clip, color, fill_rule,
        )
    }

    #[cfg(feature = "surfaces")]
    fn delete_surface_path(&mut self, path: Self::SurfacePath) {
        self.delete_surface_path(path);
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_path_instances(
        &mut self,
        program: &Self::Program,
        path: &Self::SurfacePath,
        instances: &[super::SurfacePathInstance],
        mvp: &[f32; 16],
        clip: &[f32; 4],
        fill_rule: u32,
    ) -> Result<(), RenderError> {
        self.draw_surface_path_instances(program, path, instances, mvp, clip, fill_rule)
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_bitmap(
        &mut self,
        program: &Self::Program,
        texture: &Self::Texture,
        mvp: &[f32; 16],
        placement: &[f32; 4],
        clip: &[f32; 4],
        color: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.draw_surface_bitmap(program, texture, mvp, placement, clip, color)
    }

    #[cfg(feature = "surfaces")]
    fn surface_cache_limit(&self) -> u32 {
        self.surface_cache_dimension_limit()
    }

    #[cfg(feature = "surfaces")]
    fn create_surface_cache_target(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Self::SurfaceCacheTarget, RenderError> {
        self.create_surface_cache_target(width, height)
    }

    #[cfg(feature = "surfaces")]
    fn resize_surface_cache_target(
        &mut self,
        target: &mut Self::SurfaceCacheTarget,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        self.resize_surface_cache_target(target, width, height)
    }

    #[cfg(feature = "surfaces")]
    fn begin_surface_cache_target(
        &mut self,
        target: &Self::SurfaceCacheTarget,
    ) -> Result<(), RenderError> {
        self.begin_surface_cache_target(target)
    }

    #[cfg(feature = "surfaces")]
    fn end_surface_cache_target(&mut self) -> Result<(), RenderError> {
        self.end_surface_cache_target()
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_cache(
        &mut self,
        program: &Self::Program,
        target: &Self::SurfaceCacheTarget,
        mvp: &[f32; 16],
        size: &[f32; 2],
    ) -> Result<(), RenderError> {
        self.draw_surface_cache(program, target, mvp, size)
    }

    #[cfg(feature = "surfaces")]
    fn delete_surface_cache_target(&mut self, target: Self::SurfaceCacheTarget) {
        self.delete_surface_cache_target(target);
    }

    #[cfg(feature = "gui")]
    fn create_gui_batch(
        &mut self,
        vertices: &[super::GuiBoxVertex],
    ) -> Result<Self::GuiBatch, RenderError> {
        self.create_gui_batch(vertices)
    }

    #[cfg(feature = "gui")]
    fn update_gui_batch(
        &mut self,
        batch: &mut Self::GuiBatch,
        vertices: &[super::GuiBoxVertex],
    ) -> Result<(), RenderError> {
        self.update_gui_batch(batch, vertices)
    }

    #[cfg(feature = "gui")]
    fn delete_gui_batch(&mut self, batch: Self::GuiBatch) {
        self.delete_gui_batch(batch);
    }

    #[cfg(feature = "gui")]
    fn draw_gui_batch(
        &mut self,
        program: &Self::Program,
        batch: &Self::GuiBatch,
        mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.draw_gui_batch(program, batch, mvp, clip)
    }

    #[cfg(feature = "gui")]
    fn create_glyph_batch(
        &mut self,
        vertices: &[super::GlyphVertex],
    ) -> Result<Self::GlyphBatch, RenderError> {
        self.create_glyph_batch(vertices)
    }

    #[cfg(feature = "gui")]
    fn update_glyph_batch(
        &mut self,
        batch: &mut Self::GlyphBatch,
        vertices: &[super::GlyphVertex],
    ) -> Result<(), RenderError> {
        self.update_glyph_batch(batch, vertices)
    }

    #[cfg(feature = "gui")]
    fn delete_glyph_batch(&mut self, batch: Self::GlyphBatch) {
        self.delete_glyph_batch(batch);
    }

    #[cfg(feature = "gui")]
    fn draw_glyph_batch(
        &mut self,
        program: &Self::Program,
        batch: &Self::GlyphBatch,
        atlas: &Self::Texture,
        mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.draw_glyph_batch(program, batch, atlas, mvp, clip)
    }

    #[cfg(feature = "gui")]
    fn create_glyph_atlas_page(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Self::GlyphAtlasPage, RenderError> {
        self.create_glyph_atlas_page(width, height)
    }

    #[cfg(feature = "gui")]
    fn delete_glyph_atlas_page(&mut self, page: Self::GlyphAtlasPage) {
        self.delete_glyph_atlas_page(page);
    }

    #[cfg(feature = "gui")]
    fn begin_glyph_atlas_page(&mut self, page: &Self::GlyphAtlasPage) -> Result<(), RenderError> {
        self.begin_glyph_atlas_page(page)
    }

    #[cfg(feature = "gui")]
    fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        self.end_glyph_atlas_page()
    }

    #[cfg(feature = "gui")]
    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture {
        &page.texture
    }

    #[cfg(feature = "particles")]
    fn set_instances(&mut self, instances: &[[f32; 20]]) -> Result<(), RenderError> {
        self.instance_count = i32::try_from(instances.len())
            .map_err(|_| RenderError::RenderDevice("Too many instances".into()))?;
        if instances.is_empty() {
            return Ok(());
        }
        // SAFETY: Current context owns the buffer. GL synchronously copies the live
        // immutable slice and retains only its own storage, with no Rust aliases.
        unsafe {
            if self.instance_buffer == 0 {
                (self.gl.gen_buffers)(1, &mut self.instance_buffer);
            }
            if self.instance_buffer == 0 {
                return Err(RenderError::RenderDevice(
                    "Instance allocation failed".into(),
                ));
            }
            (self.gl.bind_buffer)(ARRAY_BUFFER, self.instance_buffer);
            let bytes = std::mem::size_of_val(instances);
            if bytes > self.instance_capacity {
                let capacity = bytes
                    .max(self.instance_capacity.saturating_mul(2))
                    .max(1024);
                (self.gl.buffer_data)(ARRAY_BUFFER, capacity as isize, ptr::null(), 0x88E0);
                self.check()?;
                self.instance_capacity = capacity;
            }
            (self.gl.buffer_sub_data)(ARRAY_BUFFER, 0, bytes as isize, instances.as_ptr().cast());
        }
        self.check_draw()
    }

    #[cfg(feature = "particles")]
    fn set_additive(&mut self, enabled: bool) -> Result<(), RenderError> {
        if enabled && self.submission.blend.get() != Some(3) {
            self.submission.blend.set(Some(3));
            // SAFETY: Scalar context state only; the context remains current.
            unsafe {
                (self.gl.blend_func)(0x0302, 1, 1, 1);
            }
        }
        self.check_draw()
    }

    fn create_program(
        &mut self,
        vertex: &str,
        fragment: &str,
    ) -> Result<GlesRenderProgram, RenderError> {
        let vertex = self.shader(VERTEX_SHADER, vertex)?;
        let fragment = match self.shader(FRAGMENT_SHADER, fragment) {
            Ok(shader) => shader,
            Err(error) => {
                // SAFETY: vertex is a live exclusively owned context handle.
                unsafe { (self.gl.delete_shader)(vertex) };
                return Err(error);
            }
        };

        // SAFETY: All handles belong to the current context; status outputs are
        // exclusive locals and uniform names are static terminated strings.
        // Shaders and partial programs are released on every failure path.
        unsafe {
            let id = (self.gl.create_program)();
            if id == 0 {
                (self.gl.delete_shader)(vertex);
                (self.gl.delete_shader)(fragment);
                return Err(RenderError::RenderDevice(
                    "program allocation failed".into(),
                ));
            }

            (self.gl.attach_shader)(id, vertex);
            (self.gl.attach_shader)(id, fragment);
            (self.gl.link_program)(id);
            (self.gl.delete_shader)(vertex);
            (self.gl.delete_shader)(fragment);
            let mut status = 0;
            (self.gl.program_iv)(id, LINK_STATUS, &mut status);
            if status == 0 {
                let log = self.log(id, false);
                (self.gl.delete_program)(id);
                return Err(RenderError::RenderDevice(log));
            }

            let mvp = (self.gl.uniform_location)(id, c"u_mvp".as_ptr());
            let material = (self.gl.uniform_location)(id, c"u_material".as_ptr());
            let texture = (self.gl.uniform_location)(id, c"u_texture".as_ptr());

            if let Err(error) = self.check() {
                (self.gl.delete_program)(id);
                return Err(error);
            }

            Ok(GlesRenderProgram {
                id,
                parameters: (self.gl.uniform_block_index)(id, c"IppParameters".as_ptr()),
                alpha_mode: (self.gl.uniform_location)(id, c"u_alpha_mode".as_ptr()),
                alpha_cutoff: (self.gl.uniform_location)(id, c"u_alpha_cutoff".as_ptr()),
                parameter_locations: Default::default(),
                mvp,
                material,
                lighting: lighting::GlesLightingLocations::load(&self.gl, id),
                uniforms: Default::default(),
                #[cfg(feature = "skeletal-animation")]
                joints: (self.gl.uniform_location)(id, c"u_joints[0]".as_ptr()),
                #[cfg(feature = "mesh-poses")]
                pose_weight: (self.gl.uniform_location)(id, c"u_pose_weight".as_ptr()),
                texture,
            })
        }
    }

    fn create_mesh(&mut self, asset: &ipp_core::MeshAsset) -> Result<GlesRenderMesh, RenderError> {
        let count = i32::try_from(asset.indices().len())
            .map_err(|_| RenderError::RenderDevice("too many mesh indices".into()))?;
        let mut mesh = GlesRenderMesh {
            vao: 0,
            buffers: [0; 2],
            indices: count,
            color: 0,
            normal: 0,
            #[cfg(feature = "skeletal-animation")]
            skin: [0; 2],
            uv: 0,
            weight: 0,
        };

        // SAFETY: GL writes names into exclusive locals and copies immutable
        // asset slices synchronously. Attribute pointers are GPU offsets, never
        // retained CPU addresses. The host keeps this context current.
        unsafe {
            (self.gl.gen_vertex_arrays)(1, &mut mesh.vao);
            (self.gl.gen_buffers)(2, mesh.buffers.as_mut_ptr());
            if mesh.vao == 0 || mesh.buffers.contains(&0) {
                self.delete_mesh(mesh);
                return Err(RenderError::RenderDevice("mesh allocation failed".into()));
            }

            self.bind_vertex_array(mesh.vao);
            (self.gl.bind_buffer)(ARRAY_BUFFER, mesh.buffers[0]);
            (self.gl.buffer_data)(
                ARRAY_BUFFER,
                std::mem::size_of_val(asset.positions()) as isize,
                asset.positions().as_ptr().cast(),
                STATIC_DRAW,
            );
            (self.gl.enable_attrib)(0);
            (self.gl.attrib_pointer)(0, 3, FLOAT, 0, 0, ptr::null());
            (self.gl.bind_buffer)(ELEMENT_ARRAY_BUFFER, mesh.buffers[1]);
            (self.gl.buffer_data)(
                ELEMENT_ARRAY_BUFFER,
                std::mem::size_of_val(asset.indices()) as isize,
                asset.indices().as_ptr().cast(),
                STATIC_DRAW,
            );
        }

        let result = (|| {
            if let Some(normals) = asset.normals() {
                mesh.normal = self.upload_attribute(4, 3, FLOAT, false, normals)?;
            }
            if let Some(colors) = asset.colors() {
                mesh.color = self.upload_attribute(1, 3, FLOAT, false, colors)?;
            }
            {
                if let Some(uvs) = asset.uvs() {
                    mesh.uv = self.upload_attribute(2, 2, FLOAT, false, uvs)?;
                }
                if let Some(weights) = asset.texture_weights() {
                    mesh.weight = self.upload_attribute(3, 1, 0x1401, true, weights)?;
                }
            }
            #[cfg(feature = "skeletal-animation")]
            if let (Some(indices), Some(weights)) = (asset.joint_indices(), asset.joint_weights()) {
                mesh.skin[0] = self.upload_attribute(5, 4, 0x1401, false, indices)?;
                mesh.skin[1] = self.upload_attribute(6, 4, FLOAT, false, weights)?;
            }
            self.check()
        })();

        // SAFETY: Unbinding changes only current context state, no CPU aliases.
        unsafe {
            self.bind_vertex_array(0);
            (self.gl.bind_buffer)(ARRAY_BUFFER, 0);
        }
        if let Err(error) = result {
            self.delete_mesh(mesh);
            return Err(error);
        }
        Ok(mesh)
    }

    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<u32, RenderError> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        if width == 0
            || height == 0
            || width > self.max_texture_size
            || height > self.max_texture_size
            || (!pixels.is_empty() && expected != Some(pixels.len()))
        {
            return Err(RenderError::RenderDevice(
                "invalid texture size or MAX_TEXTURE_SIZE exceeded".into(),
            ));
        }

        let mut texture = 0;
        // SAFETY: Names are exclusively owned in the current context. Unpack
        // buffer is unbound, so texImage2D synchronously copies exactly the
        // validated live slice. No pointer or CPU borrow survives this call.
        unsafe {
            (self.gl.gen_textures)(1, &mut texture);
            if texture == 0 {
                return Err(RenderError::RenderDevice(
                    "texture allocation failed".into(),
                ));
            }
            (self.gl.active_texture)(0x84C0); // TEXTURE0
            (self.gl.bind_texture)(0x0DE1, texture); // TEXTURE_2D
            (self.gl.bind_buffer)(0x88EC, 0); // PIXEL_UNPACK_BUFFER
            (self.gl.pixel_store)(0x0CF5, 1); // UNPACK_ALIGNMENT
            (self.gl.pixel_store)(0x0CF2, 0); // UNPACK_ROW_LENGTH
            (self.gl.pixel_store)(0x0CF3, 0); // UNPACK_SKIP_ROWS
            (self.gl.pixel_store)(0x0CF4, 0); // UNPACK_SKIP_PIXELS
            (self.gl.tex_parameter)(0x0DE1, 0x2801, 0x2600); // MIN_FILTER NEAREST
            (self.gl.tex_parameter)(0x0DE1, 0x2800, 0x2600); // MAG_FILTER NEAREST
            (self.gl.tex_parameter)(0x0DE1, 0x2802, 0x2901); // WRAP_S REPEAT
            (self.gl.tex_parameter)(0x0DE1, 0x2803, 0x2901); // WRAP_T REPEAT
            (self.gl.tex_parameter)(0x0DE1, 0x813C, 0); // BASE_LEVEL
            (self.gl.tex_parameter)(0x0DE1, 0x813D, 0); // MAX_LEVEL
            (self.gl.tex_image)(
                0x0DE1,
                0,
                0x8C43, // SRGB8_ALPHA8
                width as i32,
                height as i32,
                0,
                0x1908, // RGBA
                0x1401,
                if pixels.is_empty() {
                    ptr::null()
                } else {
                    pixels.as_ptr().cast()
                },
            );
            (self.gl.bind_texture)(0x0DE1, 0);
        }
        if let Err(error) = self.check() {
            self.delete_texture(texture);
            return Err(error);
        }
        Ok(texture)
    }

    fn allocate_texture(&mut self, width: u32, height: u32) -> Result<u32, RenderError> {
        self.create_texture(width, height, &[])
    }

    fn upload_texture_rows(
        &mut self,
        texture: &u32,
        width: u32,
        first_row: u32,
        rows: u32,
        pixels: &[u8],
    ) -> Result<(), RenderError> {
        let expected = (width as usize)
            .checked_mul(rows as usize)
            .and_then(|n| n.checked_mul(4));
        if width == 0
            || rows == 0
            || width > self.max_texture_size
            || first_row
                .checked_add(rows)
                .is_none_or(|end| end > self.max_texture_size)
            || expected != Some(pixels.len())
        {
            return Err(RenderError::RenderDevice(
                "invalid texture upload rows".into(),
            ));
        }
        // SAFETY: The owning context is current; validated dimensions describe
        // exactly this immutable slice. No unpack buffer is bound, GL copies it
        // synchronously, and no pointer or alias survives the call.
        unsafe {
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(0x0DE1, *texture);
            (self.gl.bind_buffer)(0x88EC, 0);
            (self.gl.pixel_store)(0x0CF5, 1);
            (self.gl.pixel_store)(0x0CF2, 0);
            (self.gl.pixel_store)(0x0CF3, 0);
            (self.gl.pixel_store)(0x0CF4, 0);
            (self.gl.tex_sub_image)(
                0x0DE1,
                0,
                0,
                first_row as i32,
                width as i32,
                rows as i32,
                0x1908, // RGBA
                0x1401,
                pixels.as_ptr().cast(),
            );
            (self.gl.bind_texture)(0x0DE1, 0);
        }
        self.check()
    }

    fn begin_frame(
        &mut self,
        width: u32,
        height: u32,
        clear: &[f32; 4],
    ) -> Result<(), RenderError> {
        if width == 0
            || height == 0
            || width > self.max_viewport[0] as u32
            || height > self.max_viewport[1] as u32
        {
            return Err(RenderError::InvalidViewport);
        }

        self.submission.invalidate();
        #[cfg(feature = "surfaces")]
        {
            self.surface_viewport = [width as f32, height as f32];
        }
        self.begin_linear_target(width, height)?;
        // SAFETY: The host keeps its framebuffer/context current. These calls
        // only set context state and copy scalar arguments; no CPU pointers.
        unsafe {
            (self.gl.viewport)(0, 0, width as i32, height as i32);
            (self.gl.enable)(0x0B71); // DEPTH_TEST
            (self.gl.enable)(0x0B44); // CULL_FACE
            (self.gl.disable)(0x0BE2); // BLEND
            (self.gl.disable)(0x0C11); // SCISSOR_TEST
            (self.gl.disable)(0x0BD0); // DITHER
            (self.gl.disable)(0x8037); // POLYGON_OFFSET_FILL
            (self.gl.disable)(0x809E); // SAMPLE_ALPHA_TO_COVERAGE
            (self.gl.disable)(0x80A0); // SAMPLE_COVERAGE
            (self.gl.disable)(0x8C89); // RASTERIZER_DISCARD
            (self.gl.disable)(0x0B90); // STENCIL_TEST
            (self.gl.depth_func)(0x0201); // LESS
            (self.gl.depth_mask)(1);
            (self.gl.color_mask)(1, 1, 1, 1);
            (self.gl.front_face)(0x0901); // CCW
            (self.gl.cull_face)(0x0405); // BACK
            (self.gl.clear_color)(clear[0], clear[1], clear[2], clear[3]);
            (self.gl.clear_depth)(1.0);
            (self.gl.clear)(0x00004000 | 0x00000100);
        }

        self.check_draw()
    }

    #[cfg(feature = "skeletal-animation")]
    fn set_skin_palette(
        &mut self,
        program: &GlesRenderProgram,
        palette: &[[f32; 16]],
    ) -> Result<(), RenderError> {
        if palette.is_empty() || palette.len() > ipp_core::MAX_JOINTS {
            return Err(RenderError::RenderDevice("invalid joint palette".into()));
        }
        // Authored custom vertices may intentionally omit standard skinning.
        if program.joints < 0 {
            return Ok(());
        }
        // SAFETY: This context owns the program; the live immutable contiguous
        // matrix slice is copied synchronously and no Rust pointer is retained.
        unsafe {
            self.use_program(program.id);
            (self.gl.uniform_matrix)(
                program.joints,
                palette.len() as i32,
                0,
                palette.as_ptr().cast(),
            );
        }
        self.check_draw()
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        program: &GlesRenderProgram,
        mesh: &GlesRenderMesh,
        mvp: &[f32; 16],
        material: &[f32; 3],
        #[cfg(feature = "mesh-poses")] pose: Option<(&Self::Mesh, f32)>,
        texture: Option<&u32>,
    ) -> Result<(), RenderError> {
        // SAFETY: GlesRenderProgram/mesh names remain owned by this renderer/context.
        // Uniform arrays are copied synchronously. drawElements reads the owned
        // index buffer at offset zero, not a CPU pointer; uploaded counts bound it.
        unsafe {
            self.use_program(program.id);
            if program.mvp >= 0 {
                (self.gl.uniform_matrix)(program.mvp, 1, 0, mvp.as_ptr());
            }
            if program.material >= 0 {
                (self.gl.uniform_rgb)(program.material, 1, material.as_ptr());
            }
            if program.texture >= 0 {
                (self.gl.active_texture)(0x84C0);
                (self.gl.bind_sampler)(0, 0);
                (self.gl.bind_texture)(0x0DE1, texture.copied().unwrap_or(0));
                (self.gl.uniform_int)(program.texture, 0);
            }
            self.bind_vertex_array(mesh.vao);
            #[cfg(feature = "mesh-poses")]
            if let Some((target, weight)) = pose {
                (self.gl.uniform_float)(program.pose_weight, weight);
                (self.gl.bind_buffer)(ARRAY_BUFFER, target.buffers[0]);
                (self.gl.enable_attrib)(7);
                (self.gl.attrib_pointer)(7, 3, FLOAT, 0, 0, ptr::null());
                if mesh.normal != 0 && target.normal != 0 {
                    (self.gl.bind_buffer)(ARRAY_BUFFER, target.normal);
                    (self.gl.enable_attrib)(8);
                    (self.gl.attrib_pointer)(8, 3, FLOAT, 0, 0, ptr::null());
                }
            }
            // Generic values are context state, not VAO state. The weight-free
            // shader specializes absent weights to one without consuming slot 3.
            if mesh.color == 0 {
                (self.gl.attrib_rgb)(1, 1.0, 1.0, 1.0);
            }
            if mesh.uv == 0 {
                (self.gl.attrib_rgb)(2, 0.0, 0.0, 0.0);
            }
            if mesh.weight == 0 {
                (self.gl.attrib_rgb)(3, 1.0, 0.0, 0.0);
            }
            if mesh.normal == 0 {
                (self.gl.attrib_rgb)(4, 0.0, 0.0, 0.0);
            }
            #[cfg(feature = "particles")]
            if self.instance_count > 0 {
                (self.gl.bind_buffer)(ARRAY_BUFFER, self.instance_buffer);
                for slot in 9..14 {
                    (self.gl.enable_attrib)(slot);
                    (self.gl.attrib_pointer)(
                        slot,
                        4,
                        FLOAT,
                        0,
                        80,
                        ((slot - 9) * 16) as usize as *const c_void,
                    );
                    (self.gl.attrib_divisor)(slot, 1);
                }
                (self.gl.draw_instances)(
                    TRIANGLES,
                    mesh.indices,
                    UNSIGNED_SHORT,
                    ptr::null(),
                    self.instance_count,
                );
                (self.gl.bind_buffer)(ARRAY_BUFFER, mesh.buffers[0]);
                for slot in 9..14 {
                    (self.gl.attrib_divisor)(slot, 0);
                    (self.gl.disable_attrib)(slot);
                    (self.gl.attrib_pointer)(slot, 3, FLOAT, 0, 0, ptr::null());
                }
            } else {
                (self.gl.draw_elements)(TRIANGLES, mesh.indices, UNSIGNED_SHORT, ptr::null());
            }
            #[cfg(not(feature = "particles"))]
            (self.gl.draw_elements)(TRIANGLES, mesh.indices, UNSIGNED_SHORT, ptr::null());
            #[cfg(feature = "mesh-poses")]
            if pose.is_some() {
                // Release VAO references to borrowed target buffers before their
                // resource can unload. Disabling alone would retain GL storage.
                (self.gl.bind_buffer)(ARRAY_BUFFER, mesh.buffers[0]);
                for slot in [7, 8] {
                    (self.gl.attrib_pointer)(slot, 3, FLOAT, 0, 0, ptr::null());
                    (self.gl.disable_attrib)(slot);
                }
                (self.gl.bind_buffer)(ARRAY_BUFFER, 0);
            }
        }

        self.check_draw()
    }

    fn end_frame(&mut self) -> Result<(), RenderError> {
        self.present_linear_target();
        // SAFETY: The context remains current; unbinding retains no Rust data.
        unsafe {
            self.bind_vertex_array(0);
            self.use_program(0);
            #[cfg(feature = "shadows")]
            {
                (self.gl.active_texture)(0x84C1);
                (self.gl.bind_texture)(0x0DE1, 0);
                (self.gl.bind_sampler)(1, 0);
            }
            {
                (self.gl.active_texture)(0x84C0);
                (self.gl.bind_texture)(0x0DE1, 0);
                (self.gl.bind_sampler)(0, 0);
            }
        }

        self.check_frame_end()
    }

    fn delete_mesh(&mut self, mesh: GlesRenderMesh) {
        self.submission.invalidate();
        // SAFETY: Names are consumed exactly once while the constructor's
        // context lifetime contract holds. GL ignores zero or lost objects.
        unsafe {
            (self.gl.delete_vertex_arrays)(1, &mesh.vao);
            (self.gl.delete_buffers)(2, mesh.buffers.as_ptr());
            (self.gl.delete_buffers)(1, &mesh.color);
            (self.gl.delete_buffers)(1, &mesh.normal);
            #[cfg(feature = "skeletal-animation")]
            (self.gl.delete_buffers)(2, mesh.skin.as_ptr());
            {
                (self.gl.delete_buffers)(1, &mesh.uv);
                (self.gl.delete_buffers)(1, &mesh.weight);
            }
        }
    }

    fn delete_texture(&mut self, texture: u32) {
        // SAFETY: Consumes the owned name once in the current context; GL ignores zero.
        unsafe { (self.gl.delete_textures)(1, &texture) };
    }

    fn delete_program(&mut self, program: GlesRenderProgram) {
        self.submission.invalidate();
        // SAFETY: This consumes the owned program in its still-current context.
        unsafe { (self.gl.delete_program)(program.id) };
    }
}
