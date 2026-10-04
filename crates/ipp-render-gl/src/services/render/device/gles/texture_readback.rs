//! Pixel-pack staging with zero-timeout fences and bounded immutable mapping.

use super::{GlesRenderDevice, RenderError};
use std::{cell::RefCell, ffi::c_void, ptr, rc::Rc};

const PIXEL_PACK_BUFFER: u32 = 0x88EB;

pub(super) struct ReadbackStorage {
    buffer: u32,
    fence: *mut c_void,
    bytes: usize,
    ready: bool,
}

/// Exclusively owned pending readback, released only through its creating device.
pub struct GlesTextureReadback(Rc<RefCell<Option<ReadbackStorage>>>);

impl GlesRenderDevice {
    pub(super) fn stage_texture(
        &mut self,
        texture: u32,
        width: u32,
        height: u32,
    ) -> Result<GlesTextureReadback, RenderError> {
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .filter(|_| {
                width != 0
                    && height != 0
                    && width <= self.max_texture_size
                    && height <= self.max_texture_size
            })
            .ok_or_else(|| RenderError::RenderDevice("Invalid texture readback size".into()))?;
        let size = isize::try_from(bytes).map_err(|_| {
            RenderError::RenderDevice("Texture staging exceeds address range".into())
        })?;
        let target = self.current_target();
        let mut previous = 0;
        let mut pack = [0; 4];
        let mut framebuffer = 0;
        let mut buffer = 0;
        let mut fence = ptr::null_mut();
        // SAFETY: Host keeps the creating context current. Integer queries and
        // generated names write only live exclusive locals; readPixels receives
        // offset zero in an allocated GPU buffer, never a CPU pointer. Texture
        // storage row zero is the authored top row; no display-capture flip or
        // sRGB conversion is applied to this exact RGBA8 storage read.
        let result = unsafe {
            self.gl.get_integer(0x88ED, &mut previous);
            for (slot, name) in pack.iter_mut().zip([0x0D05, 0x0D02, 0x0D03, 0x0D04]) {
                self.gl.get_integer(name, slot);
            }
            self.gl.gen_framebuffers(1, &mut framebuffer);
            self.gl.gen_buffers(1, &mut buffer);
            if framebuffer == 0 || buffer == 0 {
                Err(RenderError::RenderDevice(
                    "Texture staging allocation failed".into(),
                ))
            } else {
                self.bind_framebuffers(framebuffer, framebuffer);
                self.gl
                    .framebuffer_texture(0x8D40, 0x8CE0, 0x0DE1, texture, 0);
                self.gl.read_buffer(0x8CE0);
                if self.gl.check_framebuffer(0x8D40) != 0x8CD5 {
                    Err(RenderError::RenderDevice(
                        "Texture readback framebuffer incomplete".into(),
                    ))
                } else {
                    self.gl.bind_buffer(PIXEL_PACK_BUFFER, buffer);
                    self.gl
                        .buffer_data(PIXEL_PACK_BUFFER, size, ptr::null(), 0x88E1);
                    self.gl.pixel_store(0x0D05, 1);
                    for name in [0x0D02, 0x0D03, 0x0D04] {
                        self.gl.pixel_store(name, 0);
                    }
                    self.gl.read_pixels(
                        0,
                        0,
                        width as i32,
                        height as i32,
                        0x1908,
                        0x1401,
                        ptr::null_mut(),
                    );
                    fence = self.gl.fence_sync(0x9117, 0);
                    // A paused Host need not draw/swap again to submit this work.
                    self.gl.flush();
                    if fence.is_null() {
                        Err(RenderError::RenderDevice(
                            "Texture readback fence allocation failed".into(),
                        ))
                    } else {
                        self.check()
                    }
                }
            }
        };
        // SAFETY: Restore the exact saved buffer/pack/framebuffer state before
        // returning to any draw. The transient framebuffer is no longer bound.
        unsafe {
            self.gl.bind_buffer(PIXEL_PACK_BUFFER, previous as u32);
            for (value, name) in pack.into_iter().zip([0x0D05, 0x0D02, 0x0D03, 0x0D04]) {
                self.gl.pixel_store(name, value);
            }
            self.bind_framebuffers(target.draw, target.read);
            if framebuffer != 0 {
                self.gl.delete_framebuffers(1, &framebuffer);
            }
        }
        if let Err(error) = result {
            // SAFETY: Partial objects belong only to this still-current creating
            // context and have never escaped; no mapped CPU alias exists.
            unsafe {
                if !fence.is_null() {
                    self.gl.delete_sync(fence);
                }
                if buffer != 0 {
                    self.gl.delete_buffers(1, &buffer);
                }
            }
            return Err(error);
        }
        let storage = Rc::new(RefCell::new(Some(ReadbackStorage {
            buffer,
            fence,
            bytes,
            ready: false,
        })));
        self.readbacks.retain(|stage| stage.strong_count() != 0);
        self.readbacks.push(Rc::downgrade(&storage));
        Ok(GlesTextureReadback(storage))
    }

    pub(super) fn poll_texture_stage(
        &self,
        stage: &GlesTextureReadback,
    ) -> Result<bool, RenderError> {
        let mut borrowed = stage.0.borrow_mut();
        let storage = borrowed
            .as_mut()
            .ok_or_else(|| RenderError::RenderDevice("Texture readback retired".into()))?;
        if storage.ready {
            return Ok(true);
        }
        // SAFETY: Fence belongs to this live creating context. Timeout zero and
        // flags zero perform a readiness query; no wait or retained CPU pointer.
        match unsafe { self.gl.client_wait_sync(storage.fence, 0, 0) } {
            0x911A | 0x911C => {
                storage.ready = true;
                Ok(true)
            }
            0x911B => Ok(false),
            _ => {
                self.check()?;
                Err(RenderError::RenderDevice(
                    "Texture readback fence failed".into(),
                ))
            }
        }
    }

    pub(super) fn copy_texture_stage(
        &self,
        stage: &GlesTextureReadback,
        offset: usize,
        destination: &mut [u8],
    ) -> Result<(), RenderError> {
        let borrowed = stage.0.borrow();
        let storage = borrowed
            .as_ref()
            .ok_or_else(|| RenderError::RenderDevice("Texture readback retired".into()))?;
        if !storage.ready
            || offset
                .checked_add(destination.len())
                .is_none_or(|end| end > storage.bytes)
        {
            return Err(RenderError::RenderDevice(
                "Texture readback range is unavailable".into(),
            ));
        }
        if destination.is_empty() {
            return Ok(());
        }
        let mut previous = 0;
        // SAFETY: Completed GPU storage is mapped read-only over the checked
        // exact range. Its pointer is copied into an exclusive, nonoverlapping
        // destination, retained by neither GL nor Rust after unmapping; no alias
        // or map survives this call or the next asynchronous work boundary.
        unsafe {
            self.gl.get_integer(0x88ED, &mut previous);
            self.gl.bind_buffer(PIXEL_PACK_BUFFER, storage.buffer);
            let mapped = self.gl.map_buffer_range(
                PIXEL_PACK_BUFFER,
                offset as isize,
                destination.len() as isize,
                1,
            );
            if mapped.is_null() {
                self.gl.bind_buffer(PIXEL_PACK_BUFFER, previous as u32);
                self.check()?;
                return Err(RenderError::RenderDevice(
                    "Texture staging map failed".into(),
                ));
            }
            ptr::copy_nonoverlapping(
                mapped.cast::<u8>(),
                destination.as_mut_ptr(),
                destination.len(),
            );
            let valid = self.gl.unmap_buffer(PIXEL_PACK_BUFFER);
            self.gl.bind_buffer(PIXEL_PACK_BUFFER, previous as u32);
            self.check()?;
            if valid == 0 {
                return Err(RenderError::RenderDevice(
                    "Texture staging contents invalidated".into(),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn release_texture_stage(&self, stage: &GlesTextureReadback) {
        if let Some(storage) = stage.0.borrow_mut().take() {
            // SAFETY: Caller validates the creating context generation and Host
            // keeps it current. Ownership is removed before deletion and no map
            // or later future can reuse these names.
            unsafe {
                self.gl.delete_sync(storage.fence);
                self.gl.delete_buffers(1, &storage.buffer);
            }
        }
    }

    pub(super) fn release_texture_readbacks(&mut self) {
        for stage in self.readbacks.drain(..).filter_map(|stage| stage.upgrade()) {
            if let Some(storage) = stage.borrow_mut().take() {
                // SAFETY: Existing device destruction contract retains its old
                // context current before replacement. Clear shared owners before
                // freeing handles, so later cancellation cannot delete new names.
                unsafe {
                    self.gl.delete_sync(storage.fence);
                    self.gl.delete_buffers(1, &storage.buffer);
                }
            }
        }
    }
}
