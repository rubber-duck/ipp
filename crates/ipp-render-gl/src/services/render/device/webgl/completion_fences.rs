//! Device-owned GPU retirement fences, independent of asynchronous pixel readback.

use super::commands::WebGlRenderDevice;
use crate::RenderError;

#[link(wasm_import_module = "ipp_gl")]
unsafe extern "C" {
    fn completion_fence_create() -> u32;

    fn completion_fence_poll(fence: u32) -> u32;

    fn completion_fence_delete(fence: u32);

    fn finish_submitted_work() -> u32;
}

impl WebGlRenderDevice {
    pub(super) fn finish_work(&mut self) -> Result<(), RenderError> {
        // SAFETY: The bridge completes its live context's submissions without
        // receiving pointers, retaining Rust state or reentering the renderer.
        self.check(unsafe { finish_submitted_work() })
    }

    pub(super) fn create_fence(&mut self) -> Result<u64, RenderError> {
        // SAFETY: Bridge owns the current context and allocates a nonreused name;
        // the synchronous import takes no pointers and cannot reenter Rust.
        let fence = unsafe { completion_fence_create() };
        self.check(fence)?;
        self.completion_fences.insert(fence);
        Ok(u64::from(fence))
    }

    pub(super) fn poll_fence(&self, fence: u64) -> Result<bool, RenderError> {
        let fence = u32::try_from(fence)
            .ok()
            .filter(|fence| self.completion_fences.contains(fence))
            .ok_or_else(|| RenderError::RenderDevice("Completion fence retired".into()))?;
        // SAFETY: This device owns the exact bridge name; zero-timeout polling
        // passes no Rust references, never waits and cannot reenter the renderer.
        let result = unsafe { completion_fence_poll(fence) };
        self.check(result)?;
        Ok(result == 2)
    }

    pub(super) fn delete_fence(&mut self, fence: u64) {
        if let Ok(fence) = u32::try_from(fence)
            && self.completion_fences.remove(&fence)
        {
            // SAFETY: Removal invalidates this device's ownership before deletion.
            // Missing or context-lost bridge names are inert and never reused.
            unsafe { completion_fence_delete(fence) };
        }
    }

    pub(super) fn release_fences(&mut self) {
        for fence in std::mem::take(&mut self.completion_fences) {
            // SAFETY: Device teardown invalidates all owned names before freeing
            // them. The bridge tolerates context loss and retains no Rust aliases.
            unsafe { completion_fence_delete(fence) };
        }
    }
}
