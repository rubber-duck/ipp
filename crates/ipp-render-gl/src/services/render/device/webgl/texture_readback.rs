//! Synchronous bounded copies from a previously completed WebGL pixel-pack buffer.

use super::commands::WebGlRenderDevice;
use crate::RenderError;
use std::{cell::Cell, rc::Rc};

#[link(wasm_import_module = "ipp_gl")]
unsafe extern "C" {
    fn texture_readback_begin(texture: u32, width: u32, height: u32) -> u32;

    fn texture_readback_poll(id: u32) -> u32;

    fn texture_readback_copy(id: u32, offset: usize, pointer: *mut u8, length: usize) -> u32;

    fn texture_readback_delete(id: u32);
}

/// Exact bridge staging handle, never reused within its creating context bridge.
pub struct WebGlTextureReadback(pub(super) Rc<Cell<Option<u32>>>);

impl WebGlRenderDevice {
    pub(super) fn stage_texture(
        &mut self,
        texture: u32,
        width: u32,
        height: u32,
    ) -> Result<WebGlTextureReadback, RenderError> {
        // SAFETY: Host keeps the creating WebGL context live. Bridge copies only
        // scalar names into owned GPU objects and retains no CPU/Rust reference.
        let id = unsafe { texture_readback_begin(texture, width, height) };
        self.check(id)?;
        let state = Rc::new(Cell::new(Some(id)));
        self.readbacks.retain(|stage| stage.strong_count() != 0);
        self.readbacks.push(Rc::downgrade(&state));
        Ok(WebGlTextureReadback(state))
    }

    pub(super) fn poll_texture_stage(
        &self,
        stage: &WebGlTextureReadback,
    ) -> Result<bool, RenderError> {
        let id = stage
            .0
            .get()
            .ok_or_else(|| RenderError::RenderDevice("Texture readback retired".into()))?;
        // SAFETY: Scalar exact-generation staging handle. Zero-timeout bridge
        // query neither waits for the GPU nor retains any CPU pointer.
        let status = unsafe { texture_readback_poll(id) };
        self.check(status)?;
        Ok(status == 2)
    }

    pub(super) fn copy_texture_stage(
        &self,
        stage: &WebGlTextureReadback,
        offset: usize,
        destination: &mut [u8],
    ) -> Result<(), RenderError> {
        let id = stage
            .0
            .get()
            .ok_or_else(|| RenderError::RenderDevice("Texture readback retired".into()))?;
        // SAFETY: Bridge checks exact staging/range/readiness then writes only
        // this exclusively borrowed live range of current WASM memory. Copy is
        // synchronous; JS retains no view across return/growth or async boundary.
        self.check(unsafe {
            texture_readback_copy(id, offset, destination.as_mut_ptr(), destination.len())
        })
    }

    pub(super) fn release_texture_stage(&self, stage: &WebGlTextureReadback) {
        if let Some(id) = stage.0.take() {
            // SAFETY: Caller guards creating generation. Bridge removes the
            // nonreused exact name before deletion; loss abandons old GL handles.
            unsafe { texture_readback_delete(id) };
        }
    }
}

impl Drop for WebGlRenderDevice {
    fn drop(&mut self) {
        for stage in self.readbacks.drain(..).filter_map(|stage| stage.upgrade()) {
            if let Some(id) = stage.take() {
                // SAFETY: Old device is destroyed before replacing its context.
                // Bridge names never reuse across restoration and missing old
                // handles are inert. Clearing shared owners fences later Drop.
                unsafe { texture_readback_delete(id) };
            }
        }
    }
}
