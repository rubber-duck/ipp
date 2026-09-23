use super::RenderDevice;
#[cfg(feature = "gui")]
use super::retained_vertices::{GLYPH_VERTEX_LAYOUT, GUI_BOX_VERTEX_LAYOUT, RetainedVertexLayout};
use crate::RenderError;

#[link(wasm_import_module = "ipp_gl")]
unsafe extern "C" {
    #[cfg(feature = "skeletal-animation")]
    fn mesh_skin(mesh: u32, indices: *const [u8; 4], weights: *const [f32; 4], count: usize)
    -> u32;

    #[cfg(feature = "skeletal-animation")]
    fn set_skin_palette(program: u32, palette: *const [f32; 16], count: usize) -> u32;

    #[cfg(feature = "mesh-poses")]
    fn draw_pose(
        program: u32,
        mesh: u32,
        mvp: *const f32,
        material: *const f32,
        texture: u32,
        target: u32,
        weight: f32,
    ) -> u32;

    #[cfg(feature = "particles")]
    fn set_instances(pointer: *const f32, count: usize) -> u32;

    #[cfg(feature = "particles")]
    fn set_additive(enabled: u32) -> u32;

    fn create_program(vptr: *const u8, vlen: usize, fptr: *const u8, flen: usize) -> u32;

    fn create_mesh(
        pptr: *const [f32; 3],
        vertices: usize,
        cptr: *const [f32; 3],
        iptr: *const u16,
        indices: usize,
        nptr: *const [f32; 3],
    ) -> u32;

    fn create_mesh_uv(
        pptr: *const [f32; 3],
        vertices: usize,
        cptr: *const [f32; 3],
        iptr: *const u16,
        indices: usize,
        nptr: *const [f32; 3],
        uvptr: *const [f32; 2],
        weightptr: *const u8,
    ) -> u32;

    fn create_texture(width: u32, height: u32, pixels: *const u8, bytes: usize) -> u32;

    fn upload_texture_rows(
        texture: u32,
        width: u32,
        first_row: u32,
        rows: u32,
        pixels: *const u8,
        bytes: usize,
    ) -> u32;

    fn draw_textured(
        program: u32,
        mesh: u32,
        mvp: *const f32,
        material: *const f32,
        texture: u32,
    ) -> u32;

    fn delete_texture(id: u32);

    fn set_lighting(
        program: u32,
        model: *const f32,
        normal: *const f32,
        surface: *const f32,
        camera: *const f32,
        ambient: *const f32,
        lights: *const f32,
        count: i32,
        changed: u32,
    ) -> u32;

    #[cfg(feature = "shadows")]
    fn shadow_map_limit() -> u32;

    #[cfg(feature = "shadows")]
    fn create_shadow_map(size: u32) -> u32;

    #[cfg(feature = "shadows")]
    fn begin_shadow(map: u32, slot: u32, grid: u32) -> u32;

    #[cfg(feature = "shadows")]
    fn end_shadow() -> u32;

    #[cfg(feature = "shadows")]
    fn bind_shadow(
        program: u32,
        map: u32,
        matrices: *const f32,
        settings: *const f32,
        count: i32,
        changed: u32,
    ) -> u32;

    #[cfg(feature = "shadows")]
    fn delete_shadow_map(map: u32);

    fn begin_frame(
        width: u32,
        height: u32,
        clear: *const f32,
        vertex: *const u8,
        vertex_len: usize,
        fragment: *const u8,
        fragment_len: usize,
    ) -> u32;

    fn prepare_custom_parameters(program: u32, words: usize, textures: usize) -> u32;

    fn set_custom_parameters(
        program: u32,
        words: *const u32,
        count: usize,
        textures: usize,
        alpha_mode: u32,
        alpha_cutoff: f32,
    ) -> u32;

    fn bind_custom_texture(
        program: u32,
        index: usize,
        name: *const u8,
        length: usize,
        texture: u32,
    ) -> u32;

    fn set_alpha_blend(enabled: u32) -> u32;

    #[cfg(feature = "surfaces")]
    fn set_surface_double_sided(enabled: u32) -> u32;

    #[cfg(feature = "surfaces")]
    fn create_surface_path(
        bounds: *const f32,
        segments: *const f32,
        count: usize,
        bands: *const u32,
        band_count: usize,
    ) -> u32;

    #[cfg(feature = "surfaces")]
    fn draw_surface_path(
        program: u32,
        path: u32,
        bounds: *const f32,
        curve_start: u32,
        curve_count: u32,
        band_offset: u32,
        mvp: *const f32,
        placement: *const f32,
        clip: *const f32,
        color: *const f32,
        fill_rule: u32,
    ) -> u32;

    #[cfg(feature = "surfaces")]
    fn delete_surface_path(path: u32);

    #[cfg(feature = "surfaces")]
    fn draw_surface_path_instances(
        program: u32,
        path: u32,
        instances: *const f32,
        count: usize,
        mvp: *const f32,
        clip: *const f32,
        fill_rule: u32,
    ) -> u32;

    #[cfg(feature = "surfaces")]
    fn draw_surface_bitmap(
        program: u32,
        texture: u32,
        mvp: *const f32,
        placement: *const f32,
        clip: *const f32,
        color: *const f32,
    ) -> u32;

    #[cfg(feature = "surfaces")]
    fn surface_cache_limit() -> u32;

    #[cfg(feature = "surfaces")]
    fn create_surface_cache_target(width: u32, height: u32) -> u32;

    #[cfg(feature = "surfaces")]
    fn resize_surface_cache_target(target: u32, width: u32, height: u32) -> u32;

    #[cfg(feature = "surfaces")]
    fn begin_surface_cache_target(target: u32) -> u32;

    #[cfg(feature = "surfaces")]
    fn end_surface_cache_target() -> u32;

    #[cfg(feature = "surfaces")]
    fn draw_surface_cache(program: u32, target: u32, mvp: *const f32, size: *const f32) -> u32;

    #[cfg(feature = "surfaces")]
    fn delete_surface_cache_target(target: u32);

    #[cfg(feature = "gui")]
    fn create_gui_batch(vertex_ptr: *const f32, byte_length: u32, layout_ptr: *const u32) -> u32;

    #[cfg(feature = "gui")]
    fn update_gui_batch(batch_handle: u32, vertex_ptr: *const f32, byte_length: u32) -> u32;

    #[cfg(feature = "gui")]
    fn delete_gui_batch(batch_handle: u32);

    #[cfg(feature = "gui")]
    fn draw_gui_batch(program: u32, batch_handle: u32, mvp: *const f32, clip: *const f32) -> u32;

    #[cfg(feature = "gui")]
    fn create_glyph_batch(vertex_ptr: *const f32, byte_length: u32, layout_ptr: *const u32) -> u32;

    #[cfg(feature = "gui")]
    fn update_glyph_batch(batch_handle: u32, vertex_ptr: *const f32, byte_length: u32) -> u32;

    #[cfg(feature = "gui")]
    fn delete_glyph_batch(batch_handle: u32);

    #[cfg(feature = "gui")]
    fn draw_glyph_batch(
        program: u32,
        batch_handle: u32,
        atlas_handle: u32,
        mvp: *const f32,
        clip: *const f32,
    ) -> u32;

    #[cfg(feature = "gui")]
    fn create_glyph_atlas_page(width: u32, height: u32) -> u32;

    #[cfg(feature = "gui")]
    fn delete_glyph_atlas_page(page_handle: u32);

    #[cfg(feature = "gui")]
    fn begin_glyph_atlas_page(page_handle: u32) -> u32;

    #[cfg(feature = "gui")]
    fn end_glyph_atlas_page() -> u32;

    #[cfg(feature = "gui")]
    fn glyph_atlas_texture(page_handle: u32) -> u32;

    fn set_draw_checks(enabled: u32);

    fn end_frame() -> u32;

    fn delete_mesh(id: u32);

    fn delete_program(id: u32);

    fn is_context_lost() -> u32;

    fn error_message(ptr: *mut u8, capacity: usize) -> usize;
}

/// WebGL 2 device backed by this crate's narrow `ipp_gl` browser imports.
///
/// Instantiate with `createWebGlDevice(canvas).imports` under `ipp_gl`, then set
/// its WASM memory before calling Rust. Imports must copy synchronously, retain
/// no component ranges and never reenter Rust. Whole-memory JS views are checked
/// against the current memory buffer after growth, with bounds checked per import.
/// One bridge owns one canvas context.
#[derive(Default)]
pub struct WebGlRenderDevice {
    uniform_epoch: u64,
    #[cfg(feature = "surfaces")]
    surface_instance_scratch: Vec<[f32; 16]>,
}

/// Context-owned program and retained uniform upload state.
pub struct WebGlRenderProgram {
    id: u32,
    uniforms: std::cell::RefCell<super::uniform_cache::RenderUniformCache>,
}

impl WebGlRenderDevice {
    /// Select the context supplied by this WASM instance's host imports.
    pub fn new() -> Self {
        Self::default()
    }

    fn attach_skin(&mut self, id: u32, _asset: &ipp_core::MeshAsset) -> Result<u32, RenderError> {
        #[cfg(feature = "skeletal-animation")]
        if let (Some(indices), Some(weights)) = (_asset.joint_indices(), _asset.joint_weights()) {
            // SAFETY: Validated equal-length arrays are copied synchronously;
            // the bridge retains GPU buffers only and cannot reenter Rust.
            let result = self
                .check(unsafe { mesh_skin(id, indices.as_ptr(), weights.as_ptr(), indices.len()) });
            if let Err(error) = result {
                self.delete_mesh(id);
                return Err(error);
            }
        }
        Ok(id)
    }

    fn error(&self) -> RenderError {
        // SAFETY: No pointers are passed; the import only queries its context.
        if unsafe { is_context_lost() } != 0 {
            return RenderError::ContextLost;
        }

        let mut bytes = [0u8; 2048];
        // SAFETY: The host writes at most capacity bytes synchronously to this
        // exclusively borrowed live array, retaining no pointer or view.
        let count = unsafe { error_message(bytes.as_mut_ptr(), bytes.len()) }.min(bytes.len());
        RenderError::RenderDevice(String::from_utf8_lossy(&bytes[..count]).into_owned())
    }

    fn check(&self, status: u32) -> Result<(), RenderError> {
        if status == 0 {
            Err(self.error())
        } else {
            Ok(())
        }
    }
}

/// Context-local atlas target and its distinct sampleable texture handle.
#[cfg(feature = "gui")]
pub struct WebGlGlyphAtlasPage {
    target: u32,
    texture: u32,
}

/// Byte length of retained `vertices` and the address of their `'static` layout table.
#[cfg(feature = "gui")]
fn retained_upload<V, const N: usize>(
    vertices: &[V],
    layout: &'static RetainedVertexLayout<N>,
) -> Result<(u32, *const u32), RenderError> {
    debug_assert_eq!(std::mem::size_of::<V>(), layout.stride as usize);
    let bytes = u32::try_from(std::mem::size_of_val(vertices))
        .map_err(|_| RenderError::RenderDevice("retained batch exceeds bridge limits".into()))?;
    Ok((bytes, std::ptr::from_ref(layout).cast()))
}

impl RenderDevice for WebGlRenderDevice {
    fn set_exhaustive_draw_checks(&mut self, enabled: bool) {
        // SAFETY: The bridge copies a scalar diagnostic setting, retaining no pointers.
        unsafe {
            set_draw_checks(u32::from(enabled));
        }
    }

    type Program = WebGlRenderProgram;

    type Mesh = u32;

    type Texture = u32;

    #[cfg(feature = "surfaces")]
    type SurfacePath = u32;

    #[cfg(feature = "surfaces")]
    type SurfaceCacheTarget = u32;

    #[cfg(feature = "shadows")]
    type ShadowMap = u32;

    #[cfg(feature = "gui")]
    type GuiBatch = u32;

    #[cfg(feature = "gui")]
    type GlyphBatch = u32;

    #[cfg(feature = "gui")]
    type GlyphAtlasPage = WebGlGlyphAtlasPage;

    fn set_lighting(
        &mut self,
        program: &WebGlRenderProgram,
        model: &[f32; 16],
        normal: &[f32; 16],
        surface: &[f32; 3],
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        let changed = program
            .uniforms
            .borrow_mut()
            .lighting(self.uniform_epoch, surface, frame);
        // SAFETY: The bridge copies the live immutable arrays (16, 16, 3, 4, 3 and 128
        // floats) synchronously, retaining no views and never reentering Rust.
        self.check(unsafe {
            set_lighting(
                program.id,
                model.as_ptr(),
                normal.as_ptr(),
                surface.as_ptr(),
                frame.camera.as_ptr(),
                frame.ambient.as_ptr(),
                frame.lights.as_ptr(),
                frame.count,
                changed,
            )
        })
    }

    #[cfg(feature = "shadows")]
    fn shadow_map_limit(&self) -> u32 {
        // SAFETY: The context bridge returns scalar cached device limits and retains no references.
        unsafe { shadow_map_limit() }
    }

    #[cfg(feature = "shadows")]
    fn create_shadow_map(&mut self, size: u32) -> Result<u32, RenderError> {
        // SAFETY: Only a scalar size is passed; the bridge owns allocation/validation.
        let id = unsafe { create_shadow_map(size) };
        self.check(id)?;
        Ok(id)
    }

    #[cfg(feature = "shadows")]
    fn begin_shadow(&mut self, map: &u32, slot: u32, grid: u32) -> Result<(), RenderError> {
        // SAFETY: The bridge validates its context-scoped handle, no CPU pointer passed.
        self.check(unsafe { begin_shadow(*map, slot, grid) })
    }

    #[cfg(feature = "shadows")]
    fn end_shadow(&mut self) -> Result<(), RenderError> {
        // SAFETY: The bridge restores its saved target, no CPU data accessed.
        self.check(unsafe { end_shadow() })
    }

    #[cfg(feature = "shadows")]
    fn bind_shadow(
        &mut self,
        program: &WebGlRenderProgram,
        map: &u32,
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        let changed = program
            .uniforms
            .borrow_mut()
            .shadows(self.uniform_epoch, frame);
        // SAFETY: Handles are bridge validated. The live immutable matrix/settings arrays
        // is copied synchronously; no view/pointer survives WASM memory growth.
        self.check(unsafe {
            bind_shadow(
                program.id,
                *map,
                frame.shadow_matrices.as_ptr(),
                frame.shadow_settings.as_ptr(),
                frame.count,
                changed,
            )
        })
    }

    #[cfg(feature = "shadows")]
    fn delete_shadow_map(&mut self, map: u32) {
        // SAFETY: Consumes this context's handle once; stale handles are harmless.
        unsafe { delete_shadow_map(map) };
    }

    fn prepare_custom_parameters(
        &mut self,
        program: &WebGlRenderProgram,
        words: usize,
        textures: usize,
    ) -> Result<(), RenderError> {
        // SAFETY: The bridge receives scalar limits and a live program handle;
        // allocation happens in its own context with no borrowed CPU data.
        self.check(unsafe { prepare_custom_parameters(program.id, words, textures) })
    }

    fn set_custom_parameters<'a>(
        &mut self,
        program: &WebGlRenderProgram,
        words: &[u32],
        textures: impl ExactSizeIterator<Item = Result<(&'a str, &'a u32), RenderError>>,
        alpha_mode: u32,
        alpha_cutoff: f32,
    ) -> Result<(), RenderError> {
        // SAFETY: Imports copy borrowed words and names synchronously. Resource
        // handles remain owned by this live context throughout each call.
        self.check(unsafe {
            set_custom_parameters(
                program.id,
                words.as_ptr(),
                words.len(),
                textures.len(),
                alpha_mode,
                alpha_cutoff,
            )
        })?;
        for (index, texture) in textures.enumerate() {
            let (name, texture) = texture?;
            // SAFETY: The live name is borrowed only during this synchronous call.
            self.check(unsafe {
                bind_custom_texture(program.id, index, name.as_ptr(), name.len(), *texture)
            })?;
        }
        Ok(())
    }

    fn set_alpha_blend(&mut self, enabled: bool) -> Result<(), RenderError> {
        // SAFETY: The import changes only the current context's scalar state.
        self.check(unsafe { set_alpha_blend(u32::from(enabled)) })
    }

    #[cfg(feature = "surfaces")]
    fn set_surface_double_sided(&mut self, enabled: bool) -> Result<(), RenderError> {
        // SAFETY: The import changes only the current context's scalar rasterization state.
        self.check(unsafe { set_surface_double_sided(u32::from(enabled)) })
    }

    #[cfg(feature = "surfaces")]
    fn create_surface_path(
        &mut self,
        bounds: &[f32; 4],
        segments: &[[f32; 8]],
        bands: &[[u32; 2]],
    ) -> Result<u32, RenderError> {
        // SAFETY: The bridge synchronously copies the fixed bounds and complete
        // packed segment slice and retains no WASM memory views.
        let path = unsafe {
            create_surface_path(
                bounds.as_ptr(),
                segments.as_ptr().cast(),
                segments.len(),
                bands.as_ptr().cast(),
                bands.len(),
            )
        };
        self.check(path)?;
        Ok(path)
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_path(
        &mut self,
        program: &Self::Program,
        path: &u32,
        bounds: &[f32; 4],
        descriptor: super::SurfacePathDescriptor,
        mvp: &[f32; 16],
        placement: &[f32; 4],
        clip: &[f32; 4],
        color: &[f32; 4],
        fill_rule: u32,
    ) -> Result<(), RenderError> {
        // SAFETY: The bridge validates the handle and synchronously copies all
        // fixed live arrays without retaining aliases into WASM memory.
        self.check(unsafe {
            draw_surface_path(
                program.id,
                *path,
                bounds.as_ptr(),
                descriptor.curve_range[0],
                descriptor.curve_range[1],
                descriptor.band_offset,
                mvp.as_ptr(),
                placement.as_ptr(),
                clip.as_ptr(),
                color.as_ptr(),
                fill_rule,
            )
        })
    }

    #[cfg(feature = "surfaces")]
    fn delete_surface_path(&mut self, path: u32) {
        // SAFETY: Consumes the context-owned handle once; stale context handles are harmless.
        unsafe { delete_surface_path(path) };
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_path_instances(
        &mut self,
        program: &Self::Program,
        path: &u32,
        instances: &[super::SurfacePathInstance],
        mvp: &[f32; 16],
        clip: &[f32; 4],
        fill_rule: u32,
    ) -> Result<(), RenderError> {
        if !super::surface_instances_exact(instances) {
            return Err(RenderError::RenderDevice(
                "surface instance atlas exceeds exact descriptor limits".into(),
            ));
        }
        super::pack_surface_instances(instances, &mut self.surface_instance_scratch);
        // SAFETY: The bridge synchronously copies the complete packed stream and
        // fixed uniforms and retains no aliases into WASM memory.
        self.check(unsafe {
            draw_surface_path_instances(
                program.id,
                *path,
                self.surface_instance_scratch.as_ptr().cast(),
                self.surface_instance_scratch.len(),
                mvp.as_ptr(),
                clip.as_ptr(),
                fill_rule,
            )
        })
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
        // SAFETY: Handles are bridge-validated and fixed arrays are copied synchronously.
        self.check(unsafe {
            draw_surface_bitmap(
                program.id,
                *texture,
                mvp.as_ptr(),
                placement.as_ptr(),
                clip.as_ptr(),
                color.as_ptr(),
            )
        })
    }

    #[cfg(feature = "surfaces")]
    fn surface_cache_limit(&self) -> u32 {
        // SAFETY: A scalar capability query; no Rust memory crosses the boundary and
        // the bridge cannot reenter Rust. A lost context reports zero.
        unsafe { surface_cache_limit() }
    }

    #[cfg(feature = "surfaces")]
    fn create_surface_cache_target(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Self::SurfaceCacheTarget, RenderError> {
        // SAFETY: Only scalar dimensions cross the boundary; no Rust memory is borrowed.
        // The bridge returns a never-reused handle, or zero on failure.
        let id = unsafe { create_surface_cache_target(width, height) };
        if id == 0 {
            Err(self.error())
        } else {
            Ok(id)
        }
    }

    #[cfg(feature = "surfaces")]
    fn resize_surface_cache_target(
        &mut self,
        target: &mut Self::SurfaceCacheTarget,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        // SAFETY: Only scalars cross the boundary; the bridge validates the handle.
        self.check(unsafe { resize_surface_cache_target(*target, width, height) })
    }

    #[cfg(feature = "surfaces")]
    fn begin_surface_cache_target(
        &mut self,
        target: &Self::SurfaceCacheTarget,
    ) -> Result<(), RenderError> {
        // SAFETY: Only the scalar handle crosses the boundary; the bridge validates it.
        self.check(unsafe { begin_surface_cache_target(*target) })
    }

    #[cfg(feature = "surfaces")]
    fn end_surface_cache_target(&mut self) -> Result<(), RenderError> {
        // SAFETY: No arguments; the bridge restores its saved target and checks errors.
        self.check(unsafe { end_surface_cache_target() })
    }

    #[cfg(feature = "surfaces")]
    fn draw_surface_cache(
        &mut self,
        program: &Self::Program,
        target: &Self::SurfaceCacheTarget,
        mvp: &[f32; 16],
        size: &[f32; 2],
    ) -> Result<(), RenderError> {
        // SAFETY: Handles are bridge-validated and the borrowed fixed arrays are
        // copied synchronously into uniforms; the bridge keeps no view and cannot
        // reenter Rust, so neither borrow is aliased or invalidated.
        self.check(unsafe { draw_surface_cache(program.id, *target, mvp.as_ptr(), size.as_ptr()) })
    }

    #[cfg(feature = "surfaces")]
    fn delete_surface_cache_target(&mut self, target: Self::SurfaceCacheTarget) {
        // SAFETY: Only the scalar handle crosses the boundary; the bridge ignores
        // unknown handles, tolerates context loss and cannot reenter Rust.
        unsafe { delete_surface_cache_target(target) };
    }

    #[cfg(feature = "gui")]
    fn create_gui_batch(
        &mut self,
        vertices: &[super::GuiBoxVertex],
    ) -> Result<Self::GuiBatch, RenderError> {
        let (bytes, layout) = retained_upload(vertices, &GUI_BOX_VERTEX_LAYOUT)?;
        // SAFETY: `vertices` is a live, 4-byte-aligned slice of exactly `bytes` bytes of
        // `#[repr(C)]` vertices, and `layout` points at a `'static` `#[repr(C)]` table of
        // `u32` words describing them. The bridge bounds-checks both ranges and copies
        // them into GL before returning. It keeps no view of WASM memory and cannot
        // reenter Rust, so neither shared borrow is aliased mutably or invalidated.
        let id = unsafe { create_gui_batch(vertices.as_ptr().cast(), bytes, layout) };
        if id == 0 {
            Err(self.error())
        } else {
            Ok(id)
        }
    }

    #[cfg(feature = "gui")]
    fn update_gui_batch(
        &mut self,
        batch: &mut Self::GuiBatch,
        vertices: &[super::GuiBoxVertex],
    ) -> Result<(), RenderError> {
        let (bytes, _) = retained_upload(vertices, &GUI_BOX_VERTEX_LAYOUT)?;
        // SAFETY: `vertices` is a live, 4-byte-aligned slice of exactly `bytes` bytes of
        // `#[repr(C)]` vertices. The bridge rejects stale batch handles, then
        // bounds-checks and copies that range before returning. It keeps no view of
        // WASM memory and cannot reenter Rust.
        self.check(unsafe { update_gui_batch(*batch, vertices.as_ptr().cast(), bytes) })
    }

    #[cfg(feature = "gui")]
    fn delete_gui_batch(&mut self, batch: Self::GuiBatch) {
        // SAFETY: Only a scalar handle crosses the boundary; no Rust memory is borrowed.
        // The bridge ignores unknown handles and cannot reenter Rust.
        unsafe { delete_gui_batch(batch) };
    }

    #[cfg(feature = "gui")]
    fn draw_gui_batch(
        &mut self,
        program: &Self::Program,
        batch: &Self::GuiBatch,
        mvp: &[f32; 16],
        clip: &[f32; 4],
    ) -> Result<(), RenderError> {
        // SAFETY: `mvp` (16 f32) and `clip` (4 f32) are live borrowed arrays that the
        // bridge reads as uniforms before returning; program and batch handles are
        // validated. No view of WASM memory is kept and the bridge cannot reenter Rust.
        self.check(unsafe { draw_gui_batch(program.id, *batch, mvp.as_ptr(), clip.as_ptr()) })
    }

    #[cfg(feature = "gui")]
    fn create_glyph_batch(
        &mut self,
        vertices: &[super::GlyphVertex],
    ) -> Result<Self::GlyphBatch, RenderError> {
        let (bytes, layout) = retained_upload(vertices, &GLYPH_VERTEX_LAYOUT)?;
        // SAFETY: `vertices` is a live, 4-byte-aligned slice of exactly `bytes` bytes of
        // `#[repr(C)]` vertices, and `layout` points at a `'static` `#[repr(C)]` table of
        // `u32` words describing them. The bridge bounds-checks both ranges and copies
        // them into GL before returning. It keeps no view of WASM memory and cannot
        // reenter Rust, so neither shared borrow is aliased mutably or invalidated.
        let id = unsafe { create_glyph_batch(vertices.as_ptr().cast(), bytes, layout) };
        if id == 0 {
            Err(self.error())
        } else {
            Ok(id)
        }
    }

    #[cfg(feature = "gui")]
    fn update_glyph_batch(
        &mut self,
        batch: &mut Self::GlyphBatch,
        vertices: &[super::GlyphVertex],
    ) -> Result<(), RenderError> {
        let (bytes, _) = retained_upload(vertices, &GLYPH_VERTEX_LAYOUT)?;
        // SAFETY: `vertices` is a live, 4-byte-aligned slice of exactly `bytes` bytes of
        // `#[repr(C)]` vertices. The bridge rejects stale batch handles, then
        // bounds-checks and copies that range before returning. It keeps no view of
        // WASM memory and cannot reenter Rust.
        self.check(unsafe { update_glyph_batch(*batch, vertices.as_ptr().cast(), bytes) })
    }

    #[cfg(feature = "gui")]
    fn delete_glyph_batch(&mut self, batch: Self::GlyphBatch) {
        // SAFETY: Only a scalar handle crosses the boundary; no Rust memory is borrowed.
        // The bridge ignores unknown handles and cannot reenter Rust.
        unsafe { delete_glyph_batch(batch) };
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
        // SAFETY: `mvp` (16 f32) and `clip` (4 f32) are live borrowed arrays that the
        // bridge reads as uniforms before returning; program, batch and atlas texture
        // handles are validated. No view of WASM memory is kept and the bridge cannot
        // reenter Rust.
        self.check(unsafe {
            draw_glyph_batch(program.id, *batch, *atlas, mvp.as_ptr(), clip.as_ptr())
        })
    }

    #[cfg(feature = "gui")]
    fn create_glyph_atlas_page(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Self::GlyphAtlasPage, RenderError> {
        // SAFETY: Only scalar dimensions cross the boundary; no Rust memory is borrowed.
        // The bridge returns zero on failure and cannot reenter Rust.
        let id = unsafe { create_glyph_atlas_page(width, height) };
        if id == 0 {
            Err(self.error())
        } else {
            Ok(WebGlGlyphAtlasPage {
                target: id,
                // SAFETY: This just-created page owns the returned context-local
                // texture handle; the scalar lookup borrows no Rust memory.
                texture: unsafe { glyph_atlas_texture(id) },
            })
        }
    }

    #[cfg(feature = "gui")]
    fn delete_glyph_atlas_page(&mut self, page: Self::GlyphAtlasPage) {
        // SAFETY: Only the scalar target handle crosses the boundary; the bridge also
        // releases the page's texture handle, ignores unknown handles and cannot
        // reenter Rust. No Rust memory is borrowed.
        unsafe { delete_glyph_atlas_page(page.target) };
    }

    #[cfg(feature = "gui")]
    fn begin_glyph_atlas_page(&mut self, page: &Self::GlyphAtlasPage) -> Result<(), RenderError> {
        // SAFETY: Only the scalar target handle crosses the boundary; the bridge rejects
        // stale handles and cannot reenter Rust. No Rust memory is borrowed.
        self.check(unsafe { begin_glyph_atlas_page(page.target) })
    }

    #[cfg(feature = "gui")]
    fn end_glyph_atlas_page(&mut self) -> Result<(), RenderError> {
        // SAFETY: The import takes no arguments and borrows no Rust memory; it restores
        // bridge-owned bindings, checks GL errors and cannot reenter Rust.
        self.check(unsafe { end_glyph_atlas_page() })
    }

    #[cfg(feature = "gui")]
    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture {
        &page.texture
    }

    #[cfg(feature = "particles")]
    fn set_instances(&mut self, instances: &[[f32; 20]]) -> Result<(), RenderError> {
        // SAFETY: The host synchronously copies this live immutable WASM slice;
        // no pointer or alias is retained after the import returns.
        self.check(unsafe { set_instances(instances.as_ptr().cast(), instances.len()) })
    }

    #[cfg(feature = "particles")]
    fn set_additive(&mut self, enabled: bool) -> Result<(), RenderError> {
        // SAFETY: Scalar host import with no borrowed memory.
        self.check(unsafe { set_additive(u32::from(enabled)) })
    }

    fn create_program(
        &mut self,
        vertex: &str,
        fragment: &str,
    ) -> Result<WebGlRenderProgram, RenderError> {
        // SAFETY: Both source slices live through this synchronous call; the
        // bridge copies their UTF8 bytes and retains no WASM memory view.
        let id = unsafe {
            create_program(
                vertex.as_ptr(),
                vertex.len(),
                fragment.as_ptr(),
                fragment.len(),
            )
        };
        self.check(id)?;
        Ok(WebGlRenderProgram {
            id,
            uniforms: Default::default(),
        })
    }

    fn create_mesh(&mut self, asset: &ipp_core::MeshAsset) -> Result<u32, RenderError> {
        let normals = asset
            .normals()
            .map_or(std::ptr::null(), |values| values.as_ptr());
        let colors = asset
            .colors()
            .map_or(std::ptr::null(), |values| values.as_ptr());
        if let Some(uvs) = asset.uvs() {
            let weights = asset
                .texture_weights()
                .map_or(std::ptr::null(), |values| values.as_ptr());
            // SAFETY: Validated streams live immutably through the synchronous
            // import; null denotes absence. No JS view survives memory growth.
            let id = unsafe {
                create_mesh_uv(
                    asset.positions().as_ptr(),
                    asset.vertex_count(),
                    colors,
                    asset.indices().as_ptr(),
                    asset.indices().len(),
                    normals,
                    uvs.as_ptr(),
                    weights,
                )
            };
            self.check(id)?;
            return self.attach_skin(id, asset);
        }

        // SAFETY: The bridge copies live position/color/normal/index slices before
        // returning, retains no views, and treats null optional streams as absent.
        let id = unsafe {
            create_mesh(
                asset.positions().as_ptr(),
                asset.vertex_count(),
                colors,
                asset.indices().as_ptr(),
                asset.indices().len(),
                normals,
            )
        };
        self.check(id)?;
        self.attach_skin(id, asset)
    }

    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<u32, RenderError> {
        // SAFETY: The bridge checks dimensions and the slice length, copies
        // synchronously and retains no pointer or WASM memory view.
        let id = unsafe { create_texture(width, height, pixels.as_ptr(), pixels.len()) };
        self.check(id)?;
        Ok(id)
    }

    fn allocate_texture(&mut self, width: u32, height: u32) -> Result<u32, RenderError> {
        // SAFETY: Null/zero requests allocation only; the bridge reads no CPU data.
        let id = unsafe { create_texture(width, height, std::ptr::null(), 0) };
        self.check(id)?;
        Ok(id)
    }

    fn upload_texture_rows(
        &mut self,
        texture: &u32,
        width: u32,
        first_row: u32,
        rows: u32,
        pixels: &[u8],
    ) -> Result<(), RenderError> {
        // SAFETY: The bridge validates dimensions, handle and slice extent before
        // synchronously copying. It retains no pointer/view and cannot reenter Rust.
        let status = unsafe {
            upload_texture_rows(
                *texture,
                width,
                first_row,
                rows,
                pixels.as_ptr(),
                pixels.len(),
            )
        };
        self.check(status)
    }

    fn begin_frame(
        &mut self,
        width: u32,
        height: u32,
        clear: &[f32; 4],
    ) -> Result<(), RenderError> {
        self.uniform_epoch = self.uniform_epoch.wrapping_add(1);
        // SAFETY: The bridge reads exactly four live f32 values synchronously.
        {
            let vertex = include_str!("../shaders/present.vert");
            let fragment = include_str!("../shaders/present.frag");
            // SAFETY: Imports synchronously copy source/clear bytes; every slice
            // remains live for the call and no views survive memory growth.
            self.check(unsafe {
                begin_frame(
                    width,
                    height,
                    clear.as_ptr(),
                    vertex.as_ptr(),
                    vertex.len(),
                    fragment.as_ptr(),
                    fragment.len(),
                )
            })
        }
    }

    #[cfg(feature = "skeletal-animation")]
    fn set_skin_palette(
        &mut self,
        program: &WebGlRenderProgram,
        palette: &[[f32; 16]],
    ) -> Result<(), RenderError> {
        // SAFETY: Validated matrix slices live through the synchronous import;
        // JS copies uniforms from current, bounds-checked ranges without retaining component pointers.
        self.check(unsafe { set_skin_palette(program.id, palette.as_ptr(), palette.len()) })
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        program: &WebGlRenderProgram,
        mesh: &u32,
        mvp: &[f32; 16],
        material: &[f32; 3],
        #[cfg(feature = "mesh-poses")] pose: Option<(&Self::Mesh, f32)>,
        texture: Option<&u32>,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "mesh-poses")]
        if let Some((target, weight)) = pose {
            let texture = texture.copied().unwrap_or(0);
            // SAFETY: Uniform arrays are copied synchronously. Live GPU mesh
            // handles are borrowed only for this draw; the bridge releases VAO
            // target-buffer references before returning and never reenters Rust.
            return self.check(unsafe {
                draw_pose(
                    program.id,
                    *mesh,
                    mvp.as_ptr(),
                    material.as_ptr(),
                    texture,
                    *target,
                    weight,
                )
            });
        }
        // SAFETY: Handles are context-scoped and bridge-validated. The uniform
        // arrays are live immutable borrows copied synchronously by WebGL.

        // SAFETY: Same context handles and synchronous immutable uniform reads
        // as the untextured call; zero explicitly means no texture.
        let status = unsafe {
            draw_textured(
                program.id,
                *mesh,
                mvp.as_ptr(),
                material.as_ptr(),
                texture.copied().unwrap_or(0),
            )
        };
        self.check(status)
    }

    fn end_frame(&mut self) -> Result<(), RenderError> {
        // SAFETY: The import checks context state and retains no Rust data.
        self.check(unsafe { end_frame() })
    }

    fn delete_mesh(&mut self, mesh: u32) {
        // SAFETY: The bridge removes only this context's handle. Unknown/stale
        // handles after context loss are harmless and IDs are never recycled.
        unsafe { delete_mesh(mesh) };
    }

    fn delete_texture(&mut self, texture: u32) {
        // SAFETY: The bridge consumes only this handle; stale IDs are harmless.
        unsafe { delete_texture(texture) };
    }

    fn delete_program(&mut self, program: WebGlRenderProgram) {
        // SAFETY: As for mesh deletion, stale context handles are harmless.
        unsafe { delete_program(program.id) };
    }
}
