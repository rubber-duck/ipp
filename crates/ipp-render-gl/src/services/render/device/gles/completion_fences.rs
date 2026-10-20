//! Nonblocking GPU completion, independent of readback or presentation frames.

use super::GlesRenderDevice;
use crate::RenderError;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
#[path = "completion_fences_tests.rs"]
mod tests;

impl GlesRenderDevice {
    pub(super) fn create_fence(&mut self) -> Result<u64, RenderError> {
        // Process-wide allocation also prevents a stale name from matching a
        // fence on another device/context. Exhaustion fails without wrapping.
        let id = NEXT_FENCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map_err(|_| RenderError::RenderDevice("Completion fence identity exhausted".into()))?;
        // SAFETY: The Host keeps this device's context current. The returned
        // sync is exclusively owned here and released by this device only.
        let fence = unsafe { self.gl.fence_sync(0x9117, 0) };
        // SAFETY: No pointers are passed; submission must progress even when
        // the Host stops drawing or swapping while waiting for retirement.
        unsafe { self.gl.flush() };
        if let Err(error) = self.check() {
            if matches!(error, RenderError::ContextLost) {
                self.completion_fences.clear();
            } else if !fence.is_null() {
                // SAFETY: This newly created, unpublished sync is still owned.
                unsafe { self.gl.delete_sync(fence) };
            }
            return Err(error);
        }
        if fence.is_null() {
            return Err(RenderError::RenderDevice(
                "Completion fence allocation failed".into(),
            ));
        }
        self.completion_fences.insert(id, fence);
        Ok(id)
    }

    pub(super) fn poll_fence(&mut self, id: u64) -> Result<bool, RenderError> {
        let fence = *self.completion_fences.get(&id).ok_or_else(|| {
            RenderError::RenderDevice("Unknown or released completion fence".into())
        })?;
        // SAFETY: Lookup proves ownership by this device; the Host keeps its
        // context current. Flags=0 and timeout=0 never block or drive a frame.
        let status = unsafe { self.gl.client_wait_sync(fence, 0, 0) };
        if let Err(error) = self.check() {
            if matches!(error, RenderError::ContextLost) {
                self.completion_fences.clear();
            }
            return Err(error);
        }
        match status {
            0x911A | 0x911C => Ok(true),
            0x911B => Ok(false),
            _ => Err(RenderError::RenderDevice(
                "GPU completion fence wait failed".into(),
            )),
        }
    }

    pub(super) fn delete_fence(&mut self, id: u64) {
        if let Some(fence) = self.completion_fences.remove(&id) {
            // SAFETY: Removal transfers the device's sole sync ownership here;
            // the creating context remains current through device operations.
            unsafe { self.gl.delete_sync(fence) };
        }
    }

    pub(super) fn release_completion_fences(&mut self) {
        for (_, fence) in std::mem::take(&mut self.completion_fences) {
            // SAFETY: Device drop retains the creating context and entrypoints;
            // these are the remaining exclusively owned syncs, invalidated first.
            unsafe { self.gl.delete_sync(fence) };
        }
    }
}
