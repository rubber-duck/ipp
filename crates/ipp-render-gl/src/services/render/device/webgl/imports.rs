//! The narrow `ipp_gl` browser imports the WebGL device calls; the bridge in
//! `webgl.ts` implements them.

#[link(wasm_import_module = "ipp_gl")]
unsafe extern "C" {
    #[cfg(feature = "instrumentation")]
    pub(super) fn gl_calls_start() -> u32;

    #[cfg(feature = "instrumentation")]
    pub(super) fn gl_calls_stop();

    #[cfg(feature = "instrumentation")]
    pub(super) fn gl_calls_count(index: u32) -> u64;

    pub(super) fn mesh_skin(
        mesh: u32,
        indices: *const [u8; 4],
        weights: *const [f32; 4],
        count: usize,
    ) -> u32;

    pub(super) fn set_skin_palette(program: u32, palette: *const [f32; 16], count: usize) -> u32;

    pub(super) fn draw_pose(
        program: u32,
        mesh: u32,
        mvp: *const f32,
        material: *const f32,
        texture: u32,
        target: u32,
        weight: f32,
    ) -> u32;

    pub(super) fn set_instances(pointer: *const f32, count: usize) -> u32;

    pub(super) fn set_additive(enabled: u32) -> u32;

    pub(super) fn create_program(vptr: *const u8, vlen: usize, fptr: *const u8, flen: usize)
    -> u32;

    pub(super) fn create_mesh(
        pptr: *const [f32; 3],
        vertices: usize,
        cptr: *const [f32; 3],
        iptr: *const u16,
        indices: usize,
        nptr: *const [f32; 3],
    ) -> u32;

    pub(super) fn create_mesh_uv(
        pptr: *const [f32; 3],
        vertices: usize,
        cptr: *const [f32; 3],
        iptr: *const u16,
        indices: usize,
        nptr: *const [f32; 3],
        uvptr: *const [f32; 2],
        weightptr: *const u8,
    ) -> u32;

    pub(super) fn create_texture(width: u32, height: u32, pixels: *const u8, bytes: usize) -> u32;

    pub(super) fn upload_texture_rows(
        texture: u32,
        width: u32,
        first_row: u32,
        rows: u32,
        pixels: *const u8,
        bytes: usize,
    ) -> u32;

    pub(super) fn draw_textured(
        program: u32,
        mesh: u32,
        mvp: *const f32,
        material: *const f32,
        texture: u32,
    ) -> u32;

    pub(super) fn delete_texture(id: u32);

    pub(super) fn set_lighting(
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

    pub(super) fn shadow_map_limit() -> u32;

    pub(super) fn create_shadow_map(size: u32) -> u32;

    pub(super) fn begin_shadow(map: u32, slot: u32, grid: u32) -> u32;

    pub(super) fn end_shadow() -> u32;

    pub(super) fn bind_shadow(
        program: u32,
        map: u32,
        matrices: *const f32,
        settings: *const f32,
        count: i32,
        changed: u32,
    ) -> u32;

    pub(super) fn delete_shadow_map(map: u32);

    pub(super) fn begin_frame(
        width: u32,
        height: u32,
        clear: *const f32,
        vertex: *const u8,
        vertex_len: usize,
        fragment: *const u8,
        fragment_len: usize,
    ) -> u32;

    pub(super) fn prepare_custom_parameters(program: u32, words: usize, textures: usize) -> u32;

    pub(super) fn set_custom_parameters(
        program: u32,
        words: *const u32,
        count: usize,
        textures: usize,
        alpha_mode: u32,
        alpha_cutoff: f32,
    ) -> u32;

    pub(super) fn bind_custom_texture(
        program: u32,
        index: usize,
        name: *const u8,
        length: usize,
        texture: u32,
    ) -> u32;

    pub(super) fn set_alpha_blend(enabled: u32) -> u32;
    pub(super) fn set_mesh_opacity(program: u32, opacity: f32) -> u32;

    pub(super) fn set_surface_double_sided(enabled: u32) -> u32;

    pub(super) fn surface_path_texture_limit() -> u32;

    pub(super) fn create_surface_path(
        curves: *const u8,
        curve_count: usize,
        curve_bits: u32,
        curve_scale: f32,
        bands: *const u8,
        band_count: usize,
        band_bits: u32,
    ) -> u32;

    pub(super) fn draw_surface_path(
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

    pub(super) fn delete_surface_path(path: u32);

    pub(super) fn create_surface_instances(path: u32, instances: *const f32, count: usize) -> u32;

    pub(super) fn update_surface_instances(
        stream: u32,
        path: u32,
        instances: *const f32,
        count: usize,
    ) -> u32;

    pub(super) fn delete_surface_instances(stream: u32);

    pub(super) fn draw_surface_instances(
        program: u32,
        path: u32,
        stream: u32,
        mvp: *const f32,
        clip: *const f32,
        fill_rule: u32,
    ) -> u32;

    pub(super) fn draw_surface_bitmap(
        program: u32,
        texture: u32,
        mvp: *const f32,
        placement: *const f32,
        clip: *const f32,
        color: *const f32,
    ) -> u32;

    pub(super) fn surface_cache_limit() -> u32;

    pub(super) fn create_surface_cache_target(width: u32, height: u32) -> u32;

    pub(super) fn resize_surface_cache_target(target: u32, width: u32, height: u32) -> u32;

    pub(super) fn set_surface_cache_target_active_size(target: u32, width: u32, height: u32)
    -> u32;

    pub(super) fn begin_surface_cache_target(target: u32) -> u32;

    pub(super) fn begin_camera_target(target: u32, clear: *const f32) -> u32;

    pub(super) fn end_surface_cache_target() -> u32;

    pub(super) fn draw_surface_cache(
        program: u32,
        target: u32,
        mvp: *const f32,
        size: *const f32,
        clip: *const f32,
        opacity: f32,
    ) -> u32;

    pub(super) fn delete_surface_cache_target(target: u32);

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_surface_image_mesh(
        program: u32,
        target: u32,
        mesh: u32,
        mvp: *const f32,
        size: *const f32,
        clip: *const f32,
        opacity: f32,
        flip_image: u32,
        first_index: u32,
        index_count: u32,
    ) -> u32;

    pub(super) fn create_gui_batch(byte_length: u32, layout_ptr: *const u32) -> u32;

    pub(super) fn write_gui_batch(
        batch_handle: u32,
        byte_offset: u32,
        record_ptr: *const f32,
        byte_length: u32,
    ) -> u32;

    pub(super) fn delete_gui_batch(batch_handle: u32);

    pub(super) fn set_gui_paint_blocks(program: u32, blocks: *const f32, count: u32) -> u32;

    pub(super) fn draw_gui_batch(
        program: u32,
        batch_handle: u32,
        atlas_handle: u32,
        mvp: *const f32,
        first: u32,
        count: u32,
    ) -> u32;

    pub(super) fn create_glyph_atlas_page(width: u32, height: u32) -> u32;

    pub(super) fn delete_glyph_atlas_page(page_handle: u32);

    pub(super) fn begin_glyph_atlas_page(page_handle: u32) -> u32;

    pub(super) fn end_glyph_atlas_page() -> u32;

    pub(super) fn glyph_atlas_texture(page_handle: u32) -> u32;

    pub(super) fn set_draw_checks(enabled: u32);

    pub(super) fn viewport_limit(axis: u32) -> u32;

    pub(super) fn end_frame(check: u32) -> u32;

    pub(super) fn delete_mesh(id: u32);

    pub(super) fn delete_program(id: u32);

    pub(super) fn is_context_lost() -> u32;

    pub(super) fn error_message(ptr: *mut u8, capacity: usize) -> usize;
}
