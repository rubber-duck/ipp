//! Callback-borrowed IOSurface import through ANGLE Metal, without CPU pixels.

use super::{GlesRenderDevice, external_image::State};
use crate::RenderError;
use std::{
    ffi::{CStr, c_char, c_void},
    marker::PhantomData,
    ptr,
    rc::Rc,
};

type Handle = *mut c_void;
type Result<T> = std::result::Result<T, RenderError>;

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceGetWidth(surface: Handle) -> usize;
    fn IOSurfaceGetHeight(surface: Handle) -> usize;
    fn IOSurfaceGetBytesPerRow(surface: Handle) -> usize;
    fn IOSurfaceGetAllocSize(surface: Handle) -> usize;
    fn IOSurfaceGetPlaneCount(surface: Handle) -> usize;
    fn IOSurfaceGetPixelFormat(surface: Handle) -> u32;
}

/// Full BGRA premultiplied-sRGB producer storage, borrowed until GPU copy returns.
pub struct GlesIOSurfaceImage<'a> {
    surface: Handle,
    width: u32,
    height: u32,
    bytes: usize,
    _borrow: PhantomData<&'a c_void>,
}

impl GlesIOSurfaceImage<'_> {
    /// Validate a producer's actual layout, including its padded allocation.
    ///
    /// # Safety
    /// `surface` is a live IOSurface whose owner keeps its storage and pixels
    /// stable for this value's lifetime. No concurrent writer may reuse it until
    /// `copy_io_surface` completes. This value neither retains nor releases it.
    pub unsafe fn from_raw(surface: Handle, width: u32, height: u32) -> Result<Self> {
        if surface.is_null() || width == 0 || height == 0 {
            return Err(RenderError::RenderDevice("empty IOSurface image".into()));
        }
        // SAFETY: The caller lends a live IOSurface for this entire lifetime;
        // these metadata queries retain no pointers and do not read CPU pixels.
        let (actual_width, actual_height, stride, bytes, planes, format) = unsafe {
            (
                IOSurfaceGetWidth(surface),
                IOSurfaceGetHeight(surface),
                IOSurfaceGetBytesPerRow(surface),
                IOSurfaceGetAllocSize(surface),
                IOSurfaceGetPlaneCount(surface),
                IOSurfaceGetPixelFormat(surface),
            )
        };
        if actual_width != width as usize
            || actual_height != height as usize
            || planes > 1
            || format != u32::from_be_bytes(*b"BGRA")
            || stride < width as usize * 4
            || stride
                .checked_mul(height as usize)
                .is_none_or(|size| size > bytes)
        {
            return Err(RenderError::RenderDevice(
                "IOSurface requires a complete BGRA single-plane image".into(),
            ));
        }
        Ok(Self {
            surface,
            width,
            height,
            bytes,
            _borrow: PhantomData,
        })
    }

    /// Actual allocation footprint, for the Host's pre-reserved staging budget.
    pub fn allocation_bytes(&self) -> usize {
        self.bytes
    }
}

/// Exact current EGL display/context and entrypoints borrowed from the Host.
pub struct GlesIOSurfaceImport {
    display: Handle,
    context: Handle,
    config: Handle,
    current_display: unsafe extern "system" fn() -> Handle,
    current_context: unsafe extern "system" fn() -> Handle,
    create: unsafe extern "system" fn(Handle, u32, Handle, Handle, *const i32) -> Handle,
    destroy: unsafe extern "system" fn(Handle, Handle) -> u32,
    bind: unsafe extern "system" fn(Handle, Handle, i32) -> u32,
    release: unsafe extern "system" fn(Handle, Handle, i32) -> u32,
    srgb: bool,
    _thread: PhantomData<Rc<()>>,
}

impl GlesIOSurfaceImport {
    /// Construct against the current ANGLE Metal context.
    ///
    /// # Safety
    /// The loader returns exact EGL/GL signatures callable until this value is
    /// dropped. The same live context owns the receiving GlesRenderDevice and
    /// is exclusively used on this thread. The Host outlives both objects.
    pub unsafe fn from_loader(mut loader: impl FnMut(&CStr) -> *const c_void) -> Result<Self> {
        macro_rules! load {
            ($name:literal, $ty:ty) => {{
                let address = loader($name);
                if address.is_null() {
                    return Err(RenderError::RenderDevice(format!(
                        "IOSurface entrypoint unavailable: {:?}",
                        $name
                    )));
                }
                // SAFETY: The constructor promises this exact signature and code lifetime.
                unsafe { std::mem::transmute::<*const c_void, $ty>(address) }
            }};
        }
        let current_display = load!(
            c"eglGetCurrentDisplay",
            unsafe extern "system" fn() -> Handle
        );
        let current_context = load!(
            c"eglGetCurrentContext",
            unsafe extern "system" fn() -> Handle
        );
        let query = load!(
            c"eglQueryString",
            unsafe extern "system" fn(Handle, i32) -> *const c_char
        );
        let gl_string = load!(
            c"glGetString",
            unsafe extern "system" fn(u32) -> *const c_char
        );
        let choose = load!(
            c"eglChooseConfig",
            unsafe extern "system" fn(Handle, *const i32, *mut Handle, i32, *mut i32) -> u32
        );
        let attribute = load!(
            c"eglGetConfigAttrib",
            unsafe extern "system" fn(Handle, Handle, i32, *mut i32) -> u32
        );
        // SAFETY: Queries borrow the current thread's EGL objects and strings.
        let (display, context) = unsafe { (current_display(), current_context()) };
        if display.is_null() || context.is_null() {
            return Err(RenderError::RenderDevice(
                "IOSurface requires a current EGL context".into(),
            ));
        }
        let has = |value: *const c_char, name: &str| {
            if value.is_null() {
                return false;
            }
            // SAFETY: Only live EGL/GL-owned terminated query results are passed here.
            unsafe { CStr::from_ptr(value) }
                .to_string_lossy()
                .split_whitespace()
                .any(|value| value == name)
        };
        // SAFETY: Current display/context owns these strings throughout the queries.
        if !has(
            // SAFETY: The current display owns this terminated extension string.
            unsafe { query(display, 0x3055) },
            "EGL_ANGLE_iosurface_client_buffer",
        ) {
            return Err(RenderError::RenderDevice(
                "ANGLE IOSurface import unavailable".into(),
            ));
        }
        let mut config = ptr::null_mut();
        let mut count = 0;
        let mut target = 0;
        // SAFETY: The attributes terminate; outputs are exclusive stack slots.
        if unsafe {
            choose(
                display,
                [0x3033, 1, 0x3040, 0x40, 0x3038].as_ptr(),
                &mut config,
                1,
                &mut count,
            )
        } == 0
            || count != 1
            // SAFETY: config is live and target is an exclusive output slot.
            || unsafe { attribute(display, config, 0x348D, &mut target) } == 0
            || target != 0x305F
        {
            return Err(RenderError::RenderDevice(
                "IOSurface requires ANGLE Metal TEXTURE_2D config".into(),
            ));
        }
        // SAFETY: GL extension string is owned by the current context.
        let srgb = has(unsafe { gl_string(0x1F03) }, "GL_EXT_sRGB_write_control");
        Ok(Self {
            display,
            context,
            config,
            current_display,
            current_context,
            srgb,
            create: load!(
                c"eglCreatePbufferFromClientBuffer",
                unsafe extern "system" fn(Handle, u32, Handle, Handle, *const i32) -> Handle
            ),
            destroy: load!(
                c"eglDestroySurface",
                unsafe extern "system" fn(Handle, Handle) -> u32
            ),
            bind: load!(
                c"eglBindTexImage",
                unsafe extern "system" fn(Handle, Handle, i32) -> u32
            ),
            release: load!(
                c"eglReleaseTexImage",
                unsafe extern "system" fn(Handle, Handle, i32) -> u32
            ),
            _thread: PhantomData,
        })
    }
}

impl GlesRenderDevice {
    /// Copy a callback-borrowed IOSurface into a fresh matching device texture.
    /// All GPU reads finish before return, including failure; no producer handle
    /// escapes. Touched GL state is restored before the caller resumes rendering.
    pub fn copy_io_surface(
        &mut self,
        importer: &GlesIOSurfaceImport,
        image: &GlesIOSurfaceImage<'_>,
        destination: &u32,
    ) -> Result<()> {
        // SAFETY: Host retains the importer entrypoints; these query current-thread handles.
        if unsafe { (importer.current_display)() } != importer.display
            // SAFETY: This entrypoint queries only current-thread context state.
            || unsafe { (importer.current_context)() } != importer.context
        {
            return Err(RenderError::RenderDevice(
                "IOSurface context changed".into(),
            ));
        }
        if image.width > self.max_texture_size
            || image.height > self.max_texture_size
            || self.texture_extents.get(destination) != Some(&(image.width, image.height))
        {
            return Err(RenderError::RenderDevice(
                "IOSurface destination must match device-owned texture".into(),
            ));
        }
        self.check()?;
        let state = State::save(&self.gl, importer.srgb);
        let mut imported = 0;
        let mut framebuffer = 0;
        let mut surface = ptr::null_mut();
        let mut bound = false;
        let result = (|| {
            self.prepare_external_image_copy()?;
            let attributes = [
                0x3057,
                image.width as i32,
                0x3056,
                image.height as i32,
                0x3080,
                0x305E,
                0x3081,
                0x305F,
                0x345A,
                0,
                0x345C,
                0x1401,
                0x345D,
                0x80E1,
                0x3038,
            ];
            // SAFETY: The validated IOSurface is stable through callback return.
            // EGL imports it synchronously; its temporary surface is destroyed below.
            surface = unsafe {
                (importer.create)(
                    importer.display,
                    0x3454,
                    image.surface,
                    importer.config,
                    attributes.as_ptr(),
                )
            };
            if surface.is_null() {
                return Err(RenderError::RenderDevice(
                    "EGL IOSurface import failed".into(),
                ));
            }
            state.prepare(&self.gl);
            let copy = self
                .external_image_copy
                .as_ref()
                .expect("prepared external image copy");
            // SAFETY: Objects belong to this current context; exclusive outputs
            // and borrowed producer storage remain live until finish and cleanup.
            unsafe {
                self.gl.gen_textures(1, &mut imported);
                self.gl.gen_framebuffers(1, &mut framebuffer);
                self.gl.bind_texture(0x0DE1, imported);
                bound = (importer.bind)(importer.display, surface, 0x3084) != 0;
                if !bound || imported == 0 || framebuffer == 0 {
                    return Err(RenderError::RenderDevice(
                        "IOSurface texture binding failed".into(),
                    ));
                }
                self.gl.tex_parameter(0x0DE1, 0x2801, 0x2600);
                self.gl.tex_parameter(0x0DE1, 0x2800, 0x2600);
                self.gl.tex_parameter(0x0DE1, 0x2802, 0x812F);
                self.gl.tex_parameter(0x0DE1, 0x2803, 0x812F);
                self.gl.bind_framebuffer(0x8D40, framebuffer);
                self.gl
                    .framebuffer_texture(0x8D40, 0x8CE0, 0x0DE1, *destination, 0);
                if self.gl.check_framebuffer(0x8D40) != 0x8CD5 {
                    return Err(RenderError::RenderDevice(
                        "IOSurface copy framebuffer incomplete".into(),
                    ));
                }
                self.gl
                    .viewport(0, 0, image.width as i32, image.height as i32);
                self.gl.use_program(copy.program);
                self.gl.bind_vertex_array(copy.vao);
                self.gl.draw_arrays(0x0004, 0, 3);
            }
            self.check()
        })();
        // SAFETY: Finish all possible producer reads before releasing its EGL
        // binding. All objects are exclusive and the live context stays current.
        unsafe {
            if !surface.is_null() {
                self.gl.finish();
            }
            if bound {
                (importer.release)(importer.display, surface, 0x3084);
            }
        }
        let finished = self.check();
        state.restore(&self.gl);
        // SAFETY: Temporary objects are no longer bound or used by pending GPU
        // reads; deletion occurs once, while the producer still lends its surface.
        unsafe {
            self.gl.delete_framebuffers(1, &framebuffer);
            self.gl.delete_textures(1, &imported);
            if !surface.is_null() {
                (importer.destroy)(importer.display, surface);
            }
        }
        result.and(finished).and(self.check())
    }
}
