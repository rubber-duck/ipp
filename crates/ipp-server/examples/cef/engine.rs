//! Dynamically loaded, versioned C ABI. The optional engine owns only browsers.

use std::ffi::{CStr, CString, c_char, c_void};
use std::os::fd::AsRawFd;
use std::path::Path;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}
use crate::egl::Library;

pub(super) type Paint = unsafe extern "C" fn(*mut c_void, u64, *mut c_void, u32, u32);
pub(super) type Event = unsafe extern "C" fn(*mut c_void, u64, u32, *const c_char);

pub(super) struct CefEngine {
    pub pump: unsafe extern "C" fn(),
    pub create: unsafe extern "C" fn(u64, *const c_char, u32, u32) -> i32,
    pub begin_frame: unsafe extern "C" fn(u64),
    pub close: unsafe extern "C" fn(u64),
    pub pointer: unsafe extern "C" fn(u64, u32, u32, f64, f64, f64, f64),
    pub key: unsafe extern "C" fn(u64, u32, u32),
    pub text: unsafe extern "C" fn(u64, *const c_char),
    pub focus: unsafe extern "C" fn(u64, i32),
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
