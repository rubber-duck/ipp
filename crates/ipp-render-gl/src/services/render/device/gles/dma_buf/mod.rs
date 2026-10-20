//! Linux borrowed DMA-BUF import; no browser engine types cross this boundary.

#[cfg(target_os = "linux")]
mod copy;
#[cfg(target_os = "linux")]
mod entrypoints;
#[cfg(target_os = "linux")]
mod state;

#[cfg(target_os = "linux")]
pub(super) use copy::GlesDmaBufCopy;
#[cfg(target_os = "linux")]
pub use entrypoints::GlesDmaBufImport;

/// DMA-BUF import is unavailable on this GLES platform.
#[cfg(not(target_os = "linux"))]
pub struct GlesDmaBufImport;

#[cfg(not(target_os = "linux"))]
impl GlesDmaBufImport {
    /// Report the unsupported platform without loading any entrypoints.
    ///
    /// # Safety
    /// Matches the Linux constructor; this implementation never calls the loader.
    pub unsafe fn from_loader(
        _loader: impl FnMut(&std::ffi::CStr) -> *const std::ffi::c_void,
    ) -> Result<Self, crate::RenderError> {
        Err(crate::RenderError::RenderDevice(
            "DMA-BUF import requires Linux EGL".into(),
        ))
    }
}

/// The producer's byte order. Only the proven BGRA path is currently admitted.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlesDmaBufFormat {
    /// DRM AR24: blue, green, red and alpha bytes in producer memory.
    Bgra8888,
    /// Explicitly unsupported until independently validated.
    Rgba8888,
}

/// One plane borrowed from the producer for the complete copy operation.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
pub struct GlesDmaBufPlane<'a> {
    /// Open producer descriptor, borrowed without closing or duplicating it.
    pub fd: std::os::fd::BorrowedFd<'a>,
    /// Distance in bytes between adjacent rows.
    pub stride: u32,
    /// First pixel's byte offset into the allocation.
    pub offset: u64,
}

/// A complete premultiplied-sRGB frame; cropped frames are not supported.
/// The producer must keep the allocation and its contents stable until copy returns.
/// Producer writes use DMA-BUF implicit synchronization; explicit input fences
/// are not supported by this adapter.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
pub struct GlesDmaBufImage<'a> {
    /// Complete frame width in pixels; must match the destination allocation.
    pub width: u32,
    /// Complete frame height in pixels; must match the destination allocation.
    pub height: u32,
    /// Producer byte order; BGRA is the only admitted format.
    pub format: GlesDmaBufFormat,
    /// DRM modifier; only zero (linear) is admitted.
    pub modifier: u64,
    /// Exactly one plane is admitted; its contents must remain stable through copy.
    pub planes: &'a [GlesDmaBufPlane<'a>],
}
