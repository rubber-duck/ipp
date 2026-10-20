use crate::RenderError;
use std::{
    ffi::{CStr, c_char, c_void},
    marker::PhantomData,
    rc::Rc,
};

type Handle = *mut c_void;

/// Host-supplied EGL entrypoints, bound to one live display/context on this thread.
/// This borrows the Host's EGL lifetime and never initializes or destroys it.
pub struct GlesDmaBufImport {
    pub(super) display: Handle,
    context: Handle,
    current_display: unsafe extern "system" fn() -> Handle,
    current_context: unsafe extern "system" fn() -> Handle,
    pub(super) create: unsafe extern "system" fn(Handle, Handle, u32, Handle, *const i32) -> Handle,
    pub(super) destroy: unsafe extern "system" fn(Handle, Handle) -> u32,
    pub(super) image_target: unsafe extern "system" fn(u32, Handle),
    pub(super) srgb_write_control: bool,
    _thread: PhantomData<Rc<()>>,
}

impl GlesDmaBufImport {
    /// Cache the optional native importer for the current renderer context.
    ///
    /// # Safety
    /// The loader returns the exact EGL/GLES signatures requested, callable for
    /// this value's lifetime. Its current EGL display/context must be the same
    /// live context used to construct and operate the receiving GlesRenderDevice.
    /// Neither may be destroyed/recreated while this value lives. The Host owns
    /// context currentness; no other thread may concurrently use that context.
    pub unsafe fn from_loader(
        mut loader: impl FnMut(&CStr) -> *const c_void,
    ) -> Result<Self, RenderError> {
        macro_rules! load {
            ($name:literal, $ty:ty) => {{
                let pointer = loader($name);
                if pointer.is_null() {
                    return Err(RenderError::RenderDevice(format!(
                        "DMA-BUF entrypoint unavailable: {:?}",
                        $name
                    )));
                }

                // SAFETY: The constructor contract gives this exact signature,
                // callable code lifetime and current context for each name.
                unsafe { std::mem::transmute::<*const c_void, $ty>(pointer) }
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
        let gl_string = load!(c"glGetString", unsafe extern "system" fn(u32) -> *const u8);
        // SAFETY: These current-thread queries take no pointers. The Host keeps
        // the returned EGL objects live for this adapter's entire lifetime.
        let (display, context) = unsafe { (current_display(), current_context()) };
        if display.is_null() || context.is_null() {
            return Err(RenderError::RenderDevice(
                "DMA-BUF import requires current EGL display/context".into(),
            ));
        }

        // SAFETY: EGL/GL own terminated strings on this live current context;
        // extension reads copy no pointers into the resulting adapter.
        let (egl_extensions, gl_extensions) =
            unsafe { (query(display, 0x3055), gl_string(0x1F03).cast()) };
        let has = |pointer: *const c_char, name: &str| {
            if pointer.is_null() {
                return false;
            }

            // SAFETY: Both callers supply the library-owned terminated strings
            // from the live queries above, read only within this constructor.
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .split_whitespace()
                .any(|extension| extension == name)
        };
        if !has(egl_extensions, "EGL_EXT_image_dma_buf_import")
            || !has(egl_extensions, "EGL_EXT_image_dma_buf_import_modifiers")
            || !has(gl_extensions, "GL_OES_EGL_image")
        {
            return Err(RenderError::RenderDevice(
                "EGL DMA-BUF modifier/GL image import unavailable".into(),
            ));
        }

        Ok(Self {
            display,
            context,
            current_display,
            current_context,
            create: load!(
                c"eglCreateImageKHR",
                unsafe extern "system" fn(Handle, Handle, u32, Handle, *const i32) -> Handle
            ),
            destroy: load!(
                c"eglDestroyImageKHR",
                unsafe extern "system" fn(Handle, Handle) -> u32
            ),
            image_target: load!(
                c"glEGLImageTargetTexture2DOES",
                unsafe extern "system" fn(u32, Handle)
            ),
            srgb_write_control: has(gl_extensions, "GL_EXT_sRGB_write_control"),
            _thread: PhantomData,
        })
    }

    pub(super) fn check_current(&self) -> Result<(), RenderError> {
        // SAFETY: The constructor keeps exact entrypoints and EGL object
        // lifetimes valid; these only observe the calling thread's currentness.
        if unsafe {
            (self.current_display)() != self.display || (self.current_context)() != self.context
        } {
            return Err(RenderError::RenderDevice(
                "DMA-BUF importer EGL context is not current".into(),
            ));
        }
        Ok(())
    }
}
