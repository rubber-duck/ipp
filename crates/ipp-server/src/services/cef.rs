//! Dynamically loaded, versioned C ABI. The optional engine owns only browsers.

use std::ffi::{CStr, CString, c_char, c_void};
use std::os::fd::AsRawFd;
use std::path::Path;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
    fn dlopen(path: *const c_char, flags: i32) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn dlsym(library: *mut c_void, name: *const c_char) -> *const c_void;
    fn dlclose(library: *mut c_void) -> i32;
}
use std::os::unix::ffi::OsStrExt;

struct Library(*mut c_void);

impl Library {
    fn open(path: &Path) -> Result<Self, String> {
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
        // SAFETY: The configured trusted library path is terminated and lives through dlopen.
        let handle = unsafe { dlopen(path.as_ptr(), 2 | 4) };
        if handle.is_null() {
            // SAFETY: dlerror returns a terminated loader-owned message if present.
            let error = unsafe { dlerror() };
            let message = if error.is_null() {
                "could not open the CEF engine".into()
            } else {
                // SAFETY: Copy the current thread's message before another loader call.
                unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(message);
        }
        Ok(Self(handle))
    }

    fn symbol(&self, name: &CStr) -> *const c_void {
        // SAFETY: The library and terminated name remain live. Function signatures are
        // checked by the versioned ABI loader, and engine code remains mapped.
        unsafe { dlsym(self.0, name.as_ptr()) }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: A library only drops before successful initialization. In-use engines
        // retain process-lifetime Objective-C classes using ManuallyDrop.
        unsafe { dlclose(self.0) };
    }
}

/// Resolve an authored local HTML page inside the Host's canonical source root.
pub fn page_url(root: &Path, name: &Path) -> Result<CString, String> {
    let page = root
        .join(name)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !page.starts_with(root)
        || !page.is_file()
        || page.extension().is_none_or(|extension| extension != "html")
    {
        return Err("CEF page must be HTML within the configured local root".into());
    }
    let encoded: String = page
        .to_string_lossy()
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    CString::new(format!("file://{encoded}")).map_err(|error| error.to_string())
}

/// Callback-borrowed BGRA IOSurface paint.
pub type Paint = unsafe extern "C" fn(*mut c_void, u64, *mut c_void, u32, u32);
/// Callback-borrowed terminated browser event text.
pub type Event = unsafe extern "C" fn(*mut c_void, u64, u32, *const c_char);

/// Dynamically loaded native CEF entry points.
pub struct CefEngine {
    /// Pump browser work on the Host thread.
    pub pump: unsafe extern "C" fn(),
    /// Schedule a browser for an admitted page.
    pub create: unsafe extern "C" fn(u64, *const c_char, u32, u32) -> i32,
    /// Request an accelerated paint from a live browser.
    pub begin_frame: unsafe extern "C" fn(u64),
    /// Close the selected browser.
    pub close: unsafe extern "C" fn(u64),
    /// Forward pointer input to a live browser.
    pub pointer: unsafe extern "C" fn(u64, u32, u32, f64, f64, f64, f64),
    /// Forward a key event to a live browser.
    pub key: unsafe extern "C" fn(u64, u32, u32),
    /// Forward callback-borrowed UTF-8 text.
    pub text: unsafe extern "C" fn(u64, *const c_char),
    /// Forward the current browser focus state.
    pub focus: unsafe extern "C" fn(u64, i32),
    /// Begin or continue shutdown until all browsers have closed.
    pub shutdown: unsafe extern "C" fn() -> i32,
    // CEF registers process-lifetime Objective-C classes. Keep code mapped even
    // after orderly shutdown; unloading those classes is not supported by ObjC.
    _lease: std::fs::File,
    profile: std::path::PathBuf,
    _library: std::mem::ManuallyDrop<Library>,
}

impl CefEngine {
    /// # Safety
    /// The exclusive callback context is stable until shutdown finishes. Both
    /// callbacks obey engine.h and never unwind or reenter CEF. Main thread only.
    pub unsafe fn open(
        directory: &Path,
        paint: Paint,
        event: Event,
        context: *mut c_void,
    ) -> Result<Self, String> {
        let lease_path =
            std::fs::read_to_string(directory.join("runtime-lease")).map_err(|e| e.to_string())?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lease_path.trim())
            .map_err(|e| e.to_string())?;
        // SAFETY: A live owned descriptor is locked shared for this provider's
        // lifetime. Pruning uses the same file's exclusive lock.
        if unsafe { flock(lease.as_raw_fd(), 1) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let library =
            Library::open(&directory.join("libipp-cef.dylib")).map_err(|e| e.to_string())?;
        macro_rules! load {
            ($name:literal, $ty:ty) => {{
                let address = library.symbol($name);
                if address.is_null() {
                    return Err(format!("Missing CEF entrypoint {:?}", $name));
                }
                // SAFETY: The packaged engine exports this C signature and is
                // kept mapped for the process lifetime, on the main thread only.
                unsafe { std::mem::transmute::<*const c_void, $ty>(address) }
            }};
        }
        let abi = load!(c"ipp_cef_abi", unsafe extern "C" fn() -> u32);
        // SAFETY: Loaded signature is checked against the bundled ABI version.
        if unsafe { abi() } != 1 {
            return Err("Unsupported CEF engine ABI".into());
        }
        let initialize = load!(
            c"ipp_cef_initialize",
            unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                Paint,
                Event,
                *mut c_void,
            ) -> *const c_char
        );
        let runtime = std::fs::read_to_string(directory.join("runtime-path"))
            .map_err(|error| error.to_string())?;
        let runtime = CString::new(runtime.trim()).map_err(|error| error.to_string())?;
        let bundle = CString::new(directory.join("IppCEF.app").to_string_lossy().as_bytes())
            .map_err(|error| error.to_string())?;
        let profile = std::env::temp_dir().join(format!(
            "ipp-cef-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        ));
        std::fs::create_dir_all(&profile).map_err(|error| error.to_string())?;
        let profile_string = CString::new(profile.to_string_lossy().as_bytes())
            .map_err(|error| error.to_string())?;
        let result = Self {
            pump: load!(c"ipp_cef_pump", unsafe extern "C" fn()),
            create: load!(
                c"ipp_cef_create",
                unsafe extern "C" fn(u64, *const c_char, u32, u32) -> i32
            ),
            begin_frame: load!(c"ipp_cef_begin_frame", unsafe extern "C" fn(u64)),
            close: load!(c"ipp_cef_close", unsafe extern "C" fn(u64)),
            pointer: load!(
                c"ipp_cef_pointer",
                unsafe extern "C" fn(u64, u32, u32, f64, f64, f64, f64)
            ),
            key: load!(c"ipp_cef_key", unsafe extern "C" fn(u64, u32, u32)),
            text: load!(c"ipp_cef_text", unsafe extern "C" fn(u64, *const c_char)),
            focus: load!(c"ipp_cef_focus", unsafe extern "C" fn(u64, i32)),
            shutdown: load!(c"ipp_cef_shutdown", unsafe extern "C" fn() -> i32),
            _lease: lease,
            profile,
            _library: std::mem::ManuallyDrop::new(library),
        };
        // SAFETY: Strings last for the synchronous initialization, which copies
        // them. The caller retains the callback context until completed shutdown.
        let error = unsafe {
            initialize(
                runtime.as_ptr(),
                bundle.as_ptr(),
                profile_string.as_ptr(),
                paint,
                event,
                context,
            )
        };
        if !error.is_null() {
            // SAFETY: The shim lends a static terminated error string.
            return Err(unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned());
        }
        Ok(result)
    }
}

impl Drop for CefEngine {
    fn drop(&mut self) {
        // The provider completes CEF shutdown before the engine is dropped.
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}
