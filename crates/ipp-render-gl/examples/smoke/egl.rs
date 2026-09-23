use std::ffi::{CStr, CString, c_char, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use ipp_render_gl::GlesRenderDevice;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Handle = *mut c_void;
type GetProc = unsafe extern "system" fn(*const c_char) -> *const c_void;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, flags: i32) -> Handle;
    fn dlsym(library: Handle, name: *const c_char) -> *const c_void;
    fn dlclose(library: Handle) -> i32;
    fn dlerror() -> *const c_char;
}

struct Library(Handle);

impl Library {
    fn open(path: &Path) -> Result<Self> {
        let path = CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: The terminated path lives through dlopen; this runner loads
        // user-selected trusted driver code, retaining it through context drop.
        let library = unsafe { dlopen(path.as_ptr(), 2) }; // RTLD_NOW, local symbols
        if library.is_null() {
            // SAFETY: dlerror returns a borrowed terminated loader-owned string
            // valid until the next loader call. Copy it immediately.
            let detail = unsafe {
                let error = dlerror();
                if error.is_null() {
                    "unknown loader error".into()
                } else {
                    CStr::from_ptr(error).to_string_lossy().into_owned()
                }
            };
            return Err(format!("dlopen {path:?}: {detail}").into());
        }

        Ok(Self(library))
    }

    fn symbol(&self, name: &CStr) -> *const c_void {
        // SAFETY: Library handle and terminated name remain live; returned
        // addresses are only used while this Library is retained by Context.
        unsafe { dlsym(self.0, name.as_ptr()) }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: All EGL/GL resources and callable pointers have finished use
        // before the exclusively owned library reference is closed.
        unsafe { dlclose(self.0) };
    }
}

macro_rules! entry {
    ($address:expr, $signature:ty) => {{
        let address = $address;
        if address.is_null() {
            return Err(concat!("missing driver entry: ", stringify!($address)).into());
        }

        // SAFETY: Each invocation supplies the exact EGL/GLES signature for the
        // named symbol. Context retains its library while the pointer is used.
        unsafe { std::mem::transmute::<*const c_void, $signature>(address) }
    }};
}

pub struct Context {
    egl: Library,
    gles: Library,
    get_proc: GetProc,
    display: Handle,
    surface: Handle,
    context: Handle,
    initialized: bool,
    width: u32,
    height: u32,
    make_current: unsafe extern "system" fn(Handle, Handle, Handle, Handle) -> u32,
    destroy_context: unsafe extern "system" fn(Handle, Handle) -> u32,
    destroy_surface: unsafe extern "system" fn(Handle, Handle) -> u32,
    terminate: unsafe extern "system" fn(Handle) -> u32,
    error: unsafe extern "system" fn() -> i32,
}

impl Context {
    pub fn new(directory: &Path, width: u32, height: u32) -> Result<Self> {
        let egl_path = if directory.join("libEGL.so").exists() {
            directory.join("libEGL.so")
        } else {
            directory.join("libEGL.so.1")
        };
        let gles_path = if directory.join("libGLESv2.so").exists() {
            directory.join("libGLESv2.so")
        } else {
            directory.join("libGLESv2.so.2")
        };
        let egl = Library::open(&egl_path)?;
        let gles = Library::open(&gles_path)?;
        let get_proc = entry!(egl.symbol(c"eglGetProcAddress"), GetProc);
        let make_current = entry!(
            egl.symbol(c"eglMakeCurrent"),
            unsafe extern "system" fn(Handle, Handle, Handle, Handle) -> u32
        );
        let destroy_context = entry!(
            egl.symbol(c"eglDestroyContext"),
            unsafe extern "system" fn(Handle, Handle) -> u32
        );
        let destroy_surface = entry!(
            egl.symbol(c"eglDestroySurface"),
            unsafe extern "system" fn(Handle, Handle) -> u32
        );
        let terminate = entry!(
            egl.symbol(c"eglTerminate"),
            unsafe extern "system" fn(Handle) -> u32
        );
        let error = entry!(
            egl.symbol(c"eglGetError"),
            unsafe extern "system" fn() -> i32
        );
        // Construct cleanup ownership before the first EGL resource allocation.
        let mut host = Self {
            egl,
            gles,
            get_proc,
            make_current,
            destroy_context,
            destroy_surface,
            terminate,
            error,
            display: ptr::null_mut(),
            surface: ptr::null_mut(),
            context: ptr::null_mut(),
            initialized: false,
            width,
            height,
        };
        let platform = entry!(
            host.proc(c"eglGetPlatformDisplayEXT"),
            unsafe extern "system" fn(u32, Handle, *const i32) -> Handle
        );
        let initialize = entry!(
            host.egl.symbol(c"eglInitialize"),
            unsafe extern "system" fn(Handle, *mut i32, *mut i32) -> u32
        );
        let bind_api = entry!(
            host.egl.symbol(c"eglBindAPI"),
            unsafe extern "system" fn(u32) -> u32
        );
        let choose = entry!(
            host.egl.symbol(c"eglChooseConfig"),
            unsafe extern "system" fn(Handle, *const i32, *mut Handle, i32, *mut i32) -> u32
        );
        let surface = entry!(
            host.egl.symbol(c"eglCreatePbufferSurface"),
            unsafe extern "system" fn(Handle, Handle, *const i32) -> Handle
        );
        let context = entry!(
            host.egl.symbol(c"eglCreateContext"),
            unsafe extern "system" fn(Handle, Handle, Handle, *const i32) -> Handle
        );
        let query = entry!(
            host.egl.symbol(c"eglQueryString"),
            unsafe extern "system" fn(Handle, i32) -> *const c_char
        );
        // SAFETY: EGL_EXT_client_extensions allows the no-display query. The
        // terminated result belongs to the live library and is copied now.
        let extensions = unsafe { query(ptr::null_mut(), 0x3055) };
        if extensions.is_null() {
            return Err(host.failure("eglQueryString client extensions"));
        }
        // SAFETY: The non-null EGL-owned extension string remains live.
        let extensions = unsafe { CStr::from_ptr(extensions) }.to_string_lossy();
        // ANGLE and Mesa supply actual headless contexts; select
        // only an advertised platform. No windowing/display server is involved.
        let (platform_kind, platform_attributes) = if extensions
            .split_whitespace()
            .any(|name| name == "EGL_ANGLE_platform_angle")
        {
            (0x3202, vec![0x3203, 0x3450, 0x3209, 0x3487, 0x3038])
        } else if extensions
            .split_whitespace()
            .any(|name| name == "EGL_MESA_platform_surfaceless")
        {
            (0x31DD, vec![0x3038])
        } else {
            return Err(format!("no ANGLE or Mesa surfaceless EGL platform: {extensions}").into());
        };
        let config_attributes = [
            0x3033, 1, // EGL_SURFACE_TYPE = EGL_PBUFFER_BIT
            0x3040, 0x0040, // EGL_RENDERABLE_TYPE = EGL_OPENGL_ES3_BIT
            0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3021, 8, // RGBA8
            0x3025, 24, // EGL_DEPTH_SIZE
            0x3038,
        ];
        let surface_attributes = [0x3057, width as i32, 0x3056, height as i32, 0x3038];
        let context_attributes = [0x3098, 3, 0x3038];
        let mut config = ptr::null_mut();
        let mut count = 0;
        let mut major = 0;
        let mut minor = 0;

        // SAFETY: Entry points have exact EGL signatures; all attribute arrays
        // are terminated and live for their synchronous call, outputs exclusively
        // borrowed. Host owns each returned resource before another fallible step.
        unsafe {
            host.display = platform(platform_kind, ptr::null_mut(), platform_attributes.as_ptr());
            if host.display.is_null() {
                return Err(host.failure("eglGetPlatformDisplayEXT"));
            }
            if initialize(host.display, &mut major, &mut minor) == 0 {
                return Err(host.failure("eglInitialize"));
            }
            host.initialized = true;
            if bind_api(0x30A0) == 0 {
                return Err(host.failure("eglBindAPI"));
            }
            if choose(
                host.display,
                config_attributes.as_ptr(),
                &mut config,
                1,
                &mut count,
            ) == 0
                || count != 1
            {
                return Err(host.failure("eglChooseConfig RGBA8/depth24/GLES3 pbuffer"));
            }

            host.surface = surface(host.display, config, surface_attributes.as_ptr());
            if host.surface.is_null() {
                return Err(host.failure("eglCreatePbufferSurface"));
            }
            host.context = context(
                host.display,
                config,
                ptr::null_mut(),
                context_attributes.as_ptr(),
            );
            if host.context.is_null() {
                return Err(host.failure("eglCreateContext GLES3"));
            }
            if (host.make_current)(host.display, host.surface, host.surface, host.context) == 0 {
                return Err(host.failure("eglMakeCurrent"));
            }
        }

        Ok(host)
    }

    fn proc(&self, name: &CStr) -> *const c_void {
        // SAFETY: EGL library and terminated name remain live; caller retains
        // Context for the entire lifetime of any returned function pointer.
        unsafe { (self.get_proc)(name.as_ptr()) }
    }

    pub fn gl(&self, name: &CStr) -> *const c_void {
        let direct = self.gles.symbol(name);
        if direct.is_null() {
            self.proc(name)
        } else {
            direct
        }
    }

    fn failure(&self, operation: &str) -> Box<dyn std::error::Error> {
        // SAFETY: The EGL library is live; getError has no pointer arguments.
        let code = unsafe { (self.error)() };
        format!("{operation} failed: EGL error 0x{code:04x}").into()
    }

    /// Record GL_INVALID_ENUM in the current context, as a failed call would.
    #[cfg(feature = "surfaces")]
    pub fn raise_gl_error(&self) -> Result<()> {
        let enable = entry!(self.gl(c"glEnable"), unsafe extern "system" fn(u32));
        // SAFETY: The context is current on this thread. Enabling an unknown
        // capability only records INVALID_ENUM and changes no other state.
        unsafe { enable(0xFFFF) };
        Ok(())
    }

    pub fn device(&self) -> Result<GlesRenderDevice> {
        // SAFETY: This runner keeps Context alive/current on this thread until
        // after RenderService drops. It loads actual matching GLES symbols, and no
        // other thread or owner mutates the context.
        Ok(unsafe { GlesRenderDevice::from_loader(|name| self.gl(name))? })
    }

    pub fn info(&self) -> Result<String> {
        let get = entry!(
            self.gl(c"glGetString"),
            unsafe extern "system" fn(u32) -> *const c_char
        );
        let mut values = Vec::new();
        for (label, query) in [
            ("version", 0x1F02),
            ("vendor", 0x1F00),
            ("renderer", 0x1F01),
        ] {
            // SAFETY: Current context owns each terminated string. Copy it before
            // any context teardown; a null result is handled explicitly.
            let pointer = unsafe { get(query) };
            if pointer.is_null() {
                return Err("GL environment query failed".into());
            }
            // SAFETY: Non-null context-owned GL string is live and terminated.
            let value = unsafe { CStr::from_ptr(pointer) }.to_string_lossy();
            values.push(format!("{label}: {value}"));
        }
        Ok(values.join("\n"))
    }

    pub fn capture(&self) -> Result<Vec<u8>> {
        self.finish()?;
        let read = entry!(
            self.gl(c"glReadPixels"),
            unsafe extern "system" fn(i32, i32, i32, i32, u32, u32, *mut c_void)
        );
        let error = entry!(self.gl(c"glGetError"), unsafe extern "system" fn() -> u32);
        let mut pixels = vec![0; self.width as usize * self.height as usize * 4];
        // SAFETY: Pbuffer is current with no pixel-pack buffer bound. RGBA/u8
        // rows are 4-byte aligned and fit the exclusive output allocation. GL
        // finishes and copies synchronously, retaining no CPU pointer.
        unsafe {
            read(
                0,
                0,
                self.width as i32,
                self.height as i32,
                0x1908,
                0x1401,
                pixels.as_mut_ptr().cast(),
            );
            let code = error();
            if code != 0 {
                return Err(format!("capture GL error 0x{code:04x}").into());
            }
        }

        let stride = self.width as usize * 4;
        for y in 0..self.height as usize / 2 {
            let (top, bottom) = pixels.split_at_mut((self.height as usize - y - 1) * stride);
            top[y * stride..(y + 1) * stride].swap_with_slice(&mut bottom[..stride]);
        }
        Ok(pixels)
    }

    /// Complete submitted GPU work without allocating a framebuffer capture.
    pub fn finish(&self) -> Result<()> {
        let finish = entry!(self.gl(c"glFinish"), unsafe extern "system" fn());
        let error = entry!(self.gl(c"glGetError"), unsafe extern "system" fn() -> u32);
        // SAFETY: This thread owns the current live context. These synchronous
        // calls retain no pointers and complete work before benchmark timing ends.
        unsafe {
            finish();
            let code = error();
            if code != 0 {
                return Err(format!("finish GL error 0x{code:04x}").into());
            }
        }
        Ok(())
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: This runner has dropped RenderService first. EGL handles belong to
        // this host and are destroyed once, before Library fields unload symbols.
        unsafe {
            if self.initialized {
                (self.make_current)(
                    self.display,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                if !self.context.is_null() {
                    (self.destroy_context)(self.display, self.context);
                }
                if !self.surface.is_null() {
                    (self.destroy_surface)(self.display, self.surface);
                }
                (self.terminate)(self.display);
            }
        }
    }
}
