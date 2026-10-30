//! Linux implementation handoff: supply an EGL-compatible native window,
//! bounded physical input, metrics and main-thread event pumping here.

use super::WindowEvent;
use std::ffi::c_void;

pub(crate) struct Window;

impl Window {
    pub(crate) fn open() -> Result<Self, String> {
        Err("--window is not implemented on Linux yet; omit it for the offscreen GLES Host".into())
    }

    pub(crate) fn resize_surface(&mut self, _width: u32, _height: u32) {}

    pub(crate) fn layer(&self) -> *mut c_void {
        std::ptr::null_mut()
    }

    pub(crate) fn pump(&mut self) -> Result<&[WindowEvent], String> {
        Ok(&[])
    }

    #[cfg(feature = "instrumentation")]
    pub(crate) fn test_event(&mut self, _kind: u8, _x: f32, _y: f32) -> Result<(), String> {
        Err("Linux window testing is not implemented".into())
    }
}
