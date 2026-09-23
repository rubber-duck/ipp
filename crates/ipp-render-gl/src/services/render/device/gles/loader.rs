use crate::RenderError;
use std::ffi::{CStr, c_char, c_void};

macro_rules! gl_functions {
    ($($(#[$meta:meta])* $name:ident : $symbol:literal ($($arg:ty),*) -> $result:ty;)*) => {
        pub(super) struct Functions {
            $($(#[$meta])* pub(super) $name: unsafe extern "system" fn($($arg),*) -> $result,)*
        }

        impl Functions {
            pub(super) unsafe fn load(mut loader: impl FnMut(&CStr) -> *const c_void) -> Result<Self, RenderError> {
                Ok(Self {
                    $($(#[$meta])* $name: {
                        let address = loader($symbol);
                        if address.is_null() {
                            return Err(RenderError::RenderDevice(format!("missing GLES entry point {:?}", $symbol)));
                        }

                        // SAFETY: from_loader requires exact GLES signatures,
                        // callable addresses, and lifetime through device drop.
                        unsafe { std::mem::transmute::<*const c_void, unsafe extern "system" fn($($arg),*) -> $result>(address) }
                    },)*
                })
            }
        }
    };
}

gl_functions! {
    get_string: c"glGetString"(u32) -> *const u8;
    get_integer: c"glGetIntegerv"(u32, *mut i32) -> ();
    get_error: c"glGetError"() -> u32;
    create_shader: c"glCreateShader"(u32) -> u32;
    shader_source: c"glShaderSource"(u32, i32, *const *const c_char, *const i32) -> ();
    compile_shader: c"glCompileShader"(u32) -> ();
    shader_iv: c"glGetShaderiv"(u32, u32, *mut i32) -> ();
    shader_log: c"glGetShaderInfoLog"(u32, i32, *mut i32, *mut c_char) -> ();
    delete_shader: c"glDeleteShader"(u32) -> ();
    create_program: c"glCreateProgram"() -> u32;
    attach_shader: c"glAttachShader"(u32, u32) -> ();
    link_program: c"glLinkProgram"(u32) -> ();
    program_iv: c"glGetProgramiv"(u32, u32, *mut i32) -> ();
    program_log: c"glGetProgramInfoLog"(u32, i32, *mut i32, *mut c_char) -> ();
    delete_program: c"glDeleteProgram"(u32) -> ();
    uniform_location: c"glGetUniformLocation"(u32, *const c_char) -> i32;
    gen_buffers: c"glGenBuffers"(i32, *mut u32) -> ();
    bind_buffer: c"glBindBuffer"(u32, u32) -> ();
    buffer_data: c"glBufferData"(u32, isize, *const c_void, u32) -> ();
    buffer_sub_data: c"glBufferSubData"(u32, isize, isize, *const c_void) -> ();
    delete_buffers: c"glDeleteBuffers"(i32, *const u32) -> ();
    gen_vertex_arrays: c"glGenVertexArrays"(i32, *mut u32) -> ();
    bind_vertex_array: c"glBindVertexArray"(u32) -> ();
    delete_vertex_arrays: c"glDeleteVertexArrays"(i32, *const u32) -> ();
    attrib_rgb: c"glVertexAttrib3f"(u32, f32, f32, f32) -> ();
    enable_attrib: c"glEnableVertexAttribArray"(u32) -> ();
    attrib_pointer: c"glVertexAttribPointer"(u32, i32, u32, u8, i32, *const c_void) -> ();
    viewport: c"glViewport"(i32, i32, i32, i32) -> ();
    enable: c"glEnable"(u32) -> ();
    disable: c"glDisable"(u32) -> ();
    depth_func: c"glDepthFunc"(u32) -> ();
    depth_mask: c"glDepthMask"(u8) -> ();
    color_mask: c"glColorMask"(u8, u8, u8, u8) -> ();
    front_face: c"glFrontFace"(u32) -> ();
    cull_face: c"glCullFace"(u32) -> ();
    clear_color: c"glClearColor"(f32, f32, f32, f32) -> ();
    clear_depth: c"glClearDepthf"(f32) -> ();
    clear: c"glClear"(u32) -> ();
    use_program: c"glUseProgram"(u32) -> ();
    uniform_matrix: c"glUniformMatrix4fv"(i32, i32, u8, *const f32) -> ();
    uniform_rgb: c"glUniform3fv"(i32, i32, *const f32) -> ();
    gen_textures: c"glGenTextures"(i32, *mut u32) -> ();
    delete_textures: c"glDeleteTextures"(i32, *const u32) -> ();
    active_texture: c"glActiveTexture"(u32) -> ();
    bind_texture: c"glBindTexture"(u32, u32) -> ();
    bind_sampler: c"glBindSampler"(u32, u32) -> ();
    pixel_store: c"glPixelStorei"(u32, i32) -> ();
    tex_parameter: c"glTexParameteri"(u32, u32, i32) -> ();
    tex_image: c"glTexImage2D"(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void) -> ();
    tex_sub_image: c"glTexSubImage2D"(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void) -> ();
    uniform_float: c"glUniform1f"(i32, f32) -> ();
    #[cfg(any(feature = "mesh-poses", feature = "particles"))]
    disable_attrib: c"glDisableVertexAttribArray"(u32) -> ();
    uniform_int: c"glUniform1i"(i32, i32) -> ();
    uniform_vec4: c"glUniform4fv"(i32, i32, *const f32) -> ();
    gen_framebuffers: c"glGenFramebuffers"(i32, *mut u32) -> ();
    delete_framebuffers: c"glDeleteFramebuffers"(i32, *const u32) -> ();
    bind_framebuffer: c"glBindFramebuffer"(u32, u32) -> ();
    framebuffer_texture: c"glFramebufferTexture2D"(u32, u32, u32, u32, i32) -> ();
    check_framebuffer: c"glCheckFramebufferStatus"(u32) -> u32;
    #[cfg(feature = "shadows")]
    draw_buffers: c"glDrawBuffers"(i32, *const u32) -> ();
    #[cfg(feature = "shadows")]
    read_buffer: c"glReadBuffer"(u32) -> ();
    uniform_block_index: c"glGetUniformBlockIndex"(u32, *const c_char) -> u32;
    uniform_block_binding: c"glUniformBlockBinding"(u32, u32, u32) -> ();
    bind_buffer_base: c"glBindBufferBase"(u32, u32, u32) -> ();
    blend_func: c"glBlendFuncSeparate"(u32, u32, u32, u32) -> ();
    blend_equation: c"glBlendEquation"(u32) -> ();
    gen_renderbuffers: c"glGenRenderbuffers"(i32, *mut u32) -> ();
    bind_renderbuffer: c"glBindRenderbuffer"(u32, u32) -> ();
    renderbuffer_storage: c"glRenderbufferStorage"(u32, u32, i32, i32) -> ();
    framebuffer_renderbuffer: c"glFramebufferRenderbuffer"(u32, u32, u32, u32) -> ();
    delete_renderbuffers: c"glDeleteRenderbuffers"(i32, *const u32) -> ();
    draw_arrays: c"glDrawArrays"(u32, i32, i32) -> ();
    #[cfg(feature = "particles")]
    draw_instances: c"glDrawElementsInstanced"(u32,i32,u32,*const c_void,i32) -> ();
    #[cfg(any(feature = "particles", feature = "surfaces"))]
    attrib_divisor: c"glVertexAttribDivisor"(u32,u32) -> ();
    #[cfg(feature = "surfaces")]
    draw_arrays_instances: c"glDrawArraysInstanced"(u32,i32,i32,i32) -> ();
    draw_elements: c"glDrawElements"(u32, i32, u32, *const c_void) -> ();
}
