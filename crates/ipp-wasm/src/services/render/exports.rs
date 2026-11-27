//! Worker-facing presentation exports shared by both browser renderers.
use super::RenderSurfaceService;
use crate::BOUNDARY;

pub(super) fn update(
    action: impl FnOnce(&mut RenderSurfaceService, &mut ipp_core::HostRuntime) -> Result<(), String>,
) -> u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        let result = boundary
            .presentation_host()
            .ok_or_else(|| "session is closed".to_owned())
            .and_then(|(presentation, host)| action(presentation, host));
        match result {
            Ok(()) => 1,
            Err(error) => u32::from(boundary.fail(&error)),
        }
    })
}

pub(super) fn read<T: Default>(read: impl FnOnce(&RenderSurfaceService) -> T) -> T {
    BOUNDARY
        .with_borrow_mut(|boundary| boundary.presentation().map(|p| read(p)).unwrap_or_default())
}

/// Attach after the worker initializes the selected graphics device.
// SAFETY: Unique host symbol; calls are exclusive on the owning worker, no borrowed state escapes.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_attach(width: u32, height: u32) -> u32 {
    update(|presentation, world| presentation.attach(world, width, height))
}

/// Largest drawing-buffer width the device accepts, or zero while it cannot report.
// SAFETY: Unique symbol; returns an owned scalar under exclusive Host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_max_viewport_width() -> u32 {
    read(|presentation| {
        presentation
            .surface()
            .map_or(0, |surface| surface.max_width)
    })
}

/// Largest drawing-buffer height the device accepts, or zero while it cannot report.
// SAFETY: Unique symbol; returns an owned scalar under exclusive Host access.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_max_viewport_height() -> u32 {
    read(|presentation| {
        presentation
            .surface()
            .map_or(0, |surface| surface.max_height)
    })
}

/// Discard context-scoped state before loss/replacement; retain the logical world.
// SAFETY: Unique symbol; resource invalidation and destruction are exclusive and retain no aliases.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_detach() -> u32 {
    update(|presentation, host| presentation.detach_host(host))
}

/// Fill the packed statistics record of the last completed render and return
/// it; [`ipp_render_statistics_len`] gives its length. The layout is documented
/// in `services/render/statistics.rs`. Null while no session is open.
// SAFETY: Unique symbol; the pointer refers to presentation-owned storage that
// stays valid and unaliased by Rust until the next call of this export or the
// session closes. The caller copies it synchronously.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_statistics_ptr() -> *const u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(std::ptr::null(), |presentation| {
                presentation.statistics_words().as_ptr()
            })
    })
}

/// Words in the record at [`ipp_render_statistics_ptr`].
// SAFETY: Unique symbol; returns a constant scalar.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_statistics_len() -> u32 {
    super::statistics::RECORD_WORDS as u32
}

/// Whole-Host ordinary-layout membership JSON, copied synchronously by the worker.
// SAFETY: Unique diagnostic symbol. Exclusive boundary access fills owned storage;
// the returned bytes remain valid until the next call or Host destruction, without Rust aliases.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_gui_layout_ptr() -> *const u8 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(std::ptr::null(), |presentation| {
                presentation.statistics.layout_json().as_ptr()
            })
    })
}

/// Bytes at the last pointer returned by `ipp_render_gui_layout_ptr`.
// SAFETY: Unique diagnostic symbol. Exclusive boundary access returns only an owned scalar.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_gui_layout_len() -> u32 {
    read(|presentation| presentation.statistics.layout_json_len() as u32)
}

/// Build the Surface cache records of the last completed frame and return
/// them, or null when there are none. Each record has
/// `SURFACE_CACHE_RECORD_WORDS` little-endian `u32` words;
/// [`ipp_render_surface_cache_records_len`] gives the word count of this build.
// SAFETY: Unique symbol; the pointer refers to presentation-owned storage that
// stays valid and unaliased by Rust until the next call of this export, a
// render, or the session closes. The caller copies it synchronously.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_records_ptr() -> *const u32 {
    BOUNDARY.with_borrow_mut(|boundary| {
        boundary
            .presentation()
            .map_or(std::ptr::null(), |presentation| {
                let records = presentation.surface_cache_records();
                if records.is_empty() {
                    std::ptr::null()
                } else {
                    records.as_ptr()
                }
            })
    })
}

/// Length in `u32` words of the records the last
/// [`ipp_render_surface_cache_records_ptr`] call built.
// SAFETY: Unique symbol; returns a length without retaining a reference.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_render_surface_cache_records_len() -> u32 {
    read(|presentation| presentation.surface_record_count() as u32)
}
