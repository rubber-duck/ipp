use crate::RenderError;
use std::ffi::{CStr, c_char, c_void};

macro_rules! gl_functions {
    ($($(#[$meta:meta])* $name:ident : $symbol:literal ($($argument:ident: $arg:ty),*) -> $result:ty => $category:ident;)*) => {
        pub(super) struct Functions {
            #[cfg(feature = "instrumentation")]
            pub(super) calls: super::super::gl_call_counts::GlCallCounters,
            $($(#[$meta])* pub(super) $name: unsafe extern "system" fn($($arg),*) -> $result,)*
        }

        impl Functions {
            #[cfg(all(test, feature = "instrumentation"))]
            pub(super) fn test_pointer(symbol: &CStr) -> *const c_void {
                    $(if symbol == $symbol {
                        unsafe extern "system" fn stub($($argument: $arg),*) -> $result {
                            $(let _ = $argument;)*
                            // SAFETY: Listed GL return types are integers, raw pointers or unit;
                            // zero is valid for each, and the stub retains no arguments.
                            unsafe { std::mem::zeroed() }
                        }
                        return stub as *const () as *const c_void;
                    })*
                    std::ptr::null()
            }

            $($(#[$meta])* #[inline(always)]
            // Exact GL ABI signatures deliberately retain their physical parameters.
            #[allow(clippy::too_many_arguments)]
            pub(super) unsafe fn $name(&self, $($argument: $arg),*) -> $result {
                #[cfg(feature = "instrumentation")]
                self.calls.record(super::super::gl_call_counts::GlCallCategory::$category);
                // SAFETY: Caller satisfies the exact loaded entry point's context, arguments and lifetime requirements.
                unsafe { (self.$name)($($argument),*) }
            })*

            pub(super) unsafe fn load(mut loader: impl FnMut(&CStr) -> *const c_void) -> Result<Self, RenderError> {
                Ok(Self {
                    #[cfg(feature = "instrumentation")]
                    calls: Default::default(),
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
    get_string: c"glGetString"(arg0: u32) -> *const u8 => Other;
    get_integer: c"glGetIntegerv"(arg0: u32, arg1: *mut i32) -> () => Other;
    get_error: c"glGetError"() -> u32 => Other;
    create_shader: c"glCreateShader"(arg0: u32) -> u32 => Other;
    shader_source: c"glShaderSource"(arg0: u32, arg1: i32, arg2: *const *const c_char, arg3: *const i32) -> () => Other;
    compile_shader: c"glCompileShader"(arg0: u32) -> () => Other;
    shader_iv: c"glGetShaderiv"(arg0: u32, arg1: u32, arg2: *mut i32) -> () => Other;
    shader_log: c"glGetShaderInfoLog"(arg0: u32, arg1: i32, arg2: *mut i32, arg3: *mut c_char) -> () => Other;
    delete_shader: c"glDeleteShader"(arg0: u32) -> () => Other;
    create_program: c"glCreateProgram"() -> u32 => Other;
    attach_shader: c"glAttachShader"(arg0: u32, arg1: u32) -> () => Other;
    link_program: c"glLinkProgram"(arg0: u32) -> () => Other;
    program_iv: c"glGetProgramiv"(arg0: u32, arg1: u32, arg2: *mut i32) -> () => Other;
    program_log: c"glGetProgramInfoLog"(arg0: u32, arg1: i32, arg2: *mut i32, arg3: *mut c_char) -> () => Other;
    delete_program: c"glDeleteProgram"(arg0: u32) -> () => Other;
    uniform_location: c"glGetUniformLocation"(arg0: u32, arg1: *const c_char) -> i32 => State;
    gen_buffers: c"glGenBuffers"(arg0: i32, arg1: *mut u32) -> () => Other;
    bind_buffer: c"glBindBuffer"(arg0: u32, arg1: u32) -> () => State;
    buffer_data: c"glBufferData"(arg0: u32, arg1: isize, arg2: *const c_void, arg3: u32) -> () => Upload;
    buffer_sub_data: c"glBufferSubData"(arg0: u32, arg1: isize, arg2: isize, arg3: *const c_void) -> () => Upload;
    delete_buffers: c"glDeleteBuffers"(arg0: i32, arg1: *const u32) -> () => Other;
    gen_vertex_arrays: c"glGenVertexArrays"(arg0: i32, arg1: *mut u32) -> () => Other;
    bind_vertex_array: c"glBindVertexArray"(arg0: u32) -> () => State;
    delete_vertex_arrays: c"glDeleteVertexArrays"(arg0: i32, arg1: *const u32) -> () => Other;
    attrib_rgb: c"glVertexAttrib3f"(arg0: u32, arg1: f32, arg2: f32, arg3: f32) -> () => State;
    enable_attrib: c"glEnableVertexAttribArray"(arg0: u32) -> () => State;
    attrib_pointer: c"glVertexAttribPointer"(arg0: u32, arg1: i32, arg2: u32, arg3: u8, arg4: i32, arg5: *const c_void) -> () => State;
    viewport: c"glViewport"(arg0: i32, arg1: i32, arg2: i32, arg3: i32) -> () => State;
    enable: c"glEnable"(arg0: u32) -> () => State;
    disable: c"glDisable"(arg0: u32) -> () => State;
    depth_func: c"glDepthFunc"(arg0: u32) -> () => State;
    depth_mask: c"glDepthMask"(arg0: u8) -> () => State;
    color_mask: c"glColorMask"(arg0: u8, arg1: u8, arg2: u8, arg3: u8) -> () => State;
    front_face: c"glFrontFace"(arg0: u32) -> () => State;
    cull_face: c"glCullFace"(arg0: u32) -> () => State;
    clear_color: c"glClearColor"(arg0: f32, arg1: f32, arg2: f32, arg3: f32) -> () => State;
    clear_depth: c"glClearDepthf"(arg0: f32) -> () => State;
    clear: c"glClear"(arg0: u32) -> () => State;
    use_program: c"glUseProgram"(arg0: u32) -> () => State;
    uniform_matrix: c"glUniformMatrix4fv"(arg0: i32, arg1: i32, arg2: u8, arg3: *const f32) -> () => State;
    uniform_rgb: c"glUniform3fv"(arg0: i32, arg1: i32, arg2: *const f32) -> () => State;
    gen_textures: c"glGenTextures"(arg0: i32, arg1: *mut u32) -> () => Other;
    delete_textures: c"glDeleteTextures"(arg0: i32, arg1: *const u32) -> () => Other;
    active_texture: c"glActiveTexture"(arg0: u32) -> () => State;
    bind_texture: c"glBindTexture"(arg0: u32, arg1: u32) -> () => State;
    bind_sampler: c"glBindSampler"(arg0: u32, arg1: u32) -> () => State;
    pixel_store: c"glPixelStorei"(arg0: u32, arg1: i32) -> () => State;
    tex_parameter: c"glTexParameteri"(arg0: u32, arg1: u32, arg2: i32) -> () => State;
    tex_image: c"glTexImage2D"(arg0: u32, arg1: i32, arg2: i32, arg3: i32, arg4: i32, arg5: i32, arg6: u32, arg7: u32, arg8: *const c_void) -> () => Upload;
    tex_sub_image: c"glTexSubImage2D"(arg0: u32, arg1: i32, arg2: i32, arg3: i32, arg4: i32, arg5: i32, arg6: u32, arg7: u32, arg8: *const c_void) -> () => Upload;
    uniform_float: c"glUniform1f"(arg0: i32, arg1: f32) -> () => State;
    disable_attrib: c"glDisableVertexAttribArray"(arg0: u32) -> () => State;
    uniform_int: c"glUniform1i"(arg0: i32, arg1: i32) -> () => State;
    uniform_vec4: c"glUniform4fv"(arg0: i32, arg1: i32, arg2: *const f32) -> () => State;
    gen_framebuffers: c"glGenFramebuffers"(arg0: i32, arg1: *mut u32) -> () => Other;
    delete_framebuffers: c"glDeleteFramebuffers"(arg0: i32, arg1: *const u32) -> () => Other;
    bind_framebuffer: c"glBindFramebuffer"(arg0: u32, arg1: u32) -> () => State;
    framebuffer_texture: c"glFramebufferTexture2D"(arg0: u32, arg1: u32, arg2: u32, arg3: u32, arg4: i32) -> () => State;
    check_framebuffer: c"glCheckFramebufferStatus"(arg0: u32) -> u32 => Other;
    draw_buffers: c"glDrawBuffers"(arg0: i32, arg1: *const u32) -> () => State;
    read_buffer: c"glReadBuffer"(arg0: u32) -> () => Other;
    uniform_block_index: c"glGetUniformBlockIndex"(arg0: u32, arg1: *const c_char) -> u32 => State;
    uniform_block_binding: c"glUniformBlockBinding"(arg0: u32, arg1: u32, arg2: u32) -> () => State;
    bind_buffer_base: c"glBindBufferBase"(arg0: u32, arg1: u32, arg2: u32) -> () => State;
    blend_func: c"glBlendFuncSeparate"(arg0: u32, arg1: u32, arg2: u32, arg3: u32) -> () => State;
    blend_equation: c"glBlendEquation"(arg0: u32) -> () => State;
    gen_renderbuffers: c"glGenRenderbuffers"(arg0: i32, arg1: *mut u32) -> () => Other;
    bind_renderbuffer: c"glBindRenderbuffer"(arg0: u32, arg1: u32) -> () => State;
    renderbuffer_storage: c"glRenderbufferStorage"(arg0: u32, arg1: u32, arg2: i32, arg3: i32) -> () => State;
    framebuffer_renderbuffer: c"glFramebufferRenderbuffer"(arg0: u32, arg1: u32, arg2: u32, arg3: u32) -> () => State;
    delete_renderbuffers: c"glDeleteRenderbuffers"(arg0: i32, arg1: *const u32) -> () => Other;
    draw_arrays: c"glDrawArrays"(arg0: u32, arg1: i32, arg2: i32) -> () => Draw;
    draw_instances: c"glDrawElementsInstanced"(arg0: u32, arg1: i32, arg2: u32, arg3: *const c_void, arg4: i32) -> () => Draw;
    attrib_divisor: c"glVertexAttribDivisor"(arg0: u32, arg1: u32) -> () => State;
    draw_arrays_instances: c"glDrawArraysInstanced"(arg0: u32, arg1: i32, arg2: i32, arg3: i32) -> () => Draw;
    draw_elements: c"glDrawElements"(arg0: u32, arg1: i32, arg2: u32, arg3: *const c_void) -> () => Draw;
}
