//! AppKit shim loaded only by the windowed development Host. No AppKit or
//! Objective-C dependency is linked into the headless server or renderer.

use super::WindowEvent;
use crate::egl::Library;
use std::ffi::{CStr, c_void};

type Handle = *mut c_void;

pub(crate) struct Window {
    handle: Handle,
    events: Box<[WindowEvent; 256]>,
    layer: Handle,
    pump: unsafe extern "C" fn(Handle, *mut WindowEvent, u32) -> u32,
    close: unsafe extern "C" fn(Handle),
    resize: unsafe extern "C" fn(Handle, u32, u32),
    #[cfg(feature = "instrumentation")]
    test: unsafe extern "C" fn(Handle, u32, f64, f64),
    // Keep every function pointer valid until after the window is destroyed.
    _library: Library,
    _main_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Window {
    pub(crate) fn open() -> Result<Self, String> {
        let path = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .with_file_name("libipp-window.dylib");
        let library = Library::open(&path).map_err(|error| {
            format!("{error}; build the gles-host product through tools/ipp.py")
        })?;
        let symbol = |name: &CStr| -> Result<*const c_void, String> {
            let address = library.symbol(name);
            if address.is_null() {
                return Err(format!("missing window shim entry {name:?}"));
            }
            Ok(address)
        };
        // SAFETY: The bundled shim exports these exact C signatures. The library
        // is retained through window destruction, all calls are on the main
        // thread, and the opaque handle has one owner with no shared mutation.
        let (open, layer, pump, close, resize) = unsafe {
            (
                std::mem::transmute::<*const c_void, unsafe extern "C" fn() -> Handle>(symbol(
                    c"ipp_window_open",
                )?),
                std::mem::transmute::<*const c_void, unsafe extern "C" fn(Handle) -> Handle>(
                    symbol(c"ipp_window_layer")?,
                ),
                std::mem::transmute::<
                    *const c_void,
                    unsafe extern "C" fn(Handle, *mut WindowEvent, u32) -> u32,
                >(symbol(c"ipp_window_pump")?),
                std::mem::transmute::<*const c_void, unsafe extern "C" fn(Handle)>(symbol(
                    c"ipp_window_close",
                )?),
                std::mem::transmute::<*const c_void, unsafe extern "C" fn(Handle, u32, u32)>(
                    symbol(c"ipp_window_resize")?,
                ),
            )
        };
        #[cfg(feature = "instrumentation")]
        // SAFETY: Same loaded-shim ABI and ownership contract as the other entries.
        let test = unsafe {
            std::mem::transmute::<*const c_void, unsafe extern "C" fn(Handle, u32, f64, f64)>(
                symbol(c"ipp_window_test")?,
            )
        };
        // SAFETY: AppKit construction and layer access occur on this main thread.
        // The window owns the layer and outlives the borrowing EGL Context.
        let handle = unsafe { open() };
        if handle.is_null() {
            return Err("AppKit could not create a window on the main thread".into());
        }
        // SAFETY: The successfully constructed exclusive window owns its layer.
        let layer = unsafe { layer(handle) };
        Ok(Self {
            handle,
            events: Box::new([WindowEvent::default(); 256]),
            layer,
            pump,
            close,
            resize,
            #[cfg(feature = "instrumentation")]
            test,
            _library: library,
            _main_thread: std::marker::PhantomData,
        })
    }

    pub(crate) fn resize_surface(&mut self, width: u32, height: u32) {
        // SAFETY: This exclusive main-thread window owns its layer. The shim
        // retains no borrowed pointers and EGL observes the next drawable size.
        unsafe { (self.resize)(self.handle, width, height) };
    }

    pub(crate) fn layer(&self) -> Handle {
        self.layer
    }

    pub(crate) fn pump(&mut self) -> Result<&[WindowEvent], String> {
        #[cfg(feature = "instrumentation")]
        let _allocation_scope =
            ipp_core::profiling::AllocationScope::new(227, "native.window-pump");
        // SAFETY: The shim writes at most the supplied capacity into exclusive
        // initialized records and retains no buffer pointer. Window stays live.
        let count = unsafe {
            (self.pump)(
                self.handle,
                self.events.as_mut_ptr(),
                self.events.len() as u32,
            )
        } as usize;
        if count > self.events.len() {
            return Err("window shim exceeded event capacity".into());
        }
        Ok(&self.events[..count])
    }

    #[cfg(feature = "instrumentation")]
    pub(crate) fn test_event(&mut self, kind: u8, x: f32, y: f32) -> Result<(), String> {
        // SAFETY: The shim posts an owned AppKit event; no borrowed data crosses
        // the call, and the receiving window remains live on the main thread.
        unsafe { (self.test)(self.handle, u32::from(kind), f64::from(x), f64::from(y)) };
        Ok(())
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: EGL and the renderer have already dropped. This exclusively
        // owned handle is released exactly once before unloading the shim.
        unsafe { (self.close)(self.handle) };
    }
}
