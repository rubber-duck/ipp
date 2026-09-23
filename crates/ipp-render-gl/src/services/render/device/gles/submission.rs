//! Context-local bindings, invalidated at Host and resource lifetime boundaries.

use super::*;
use std::cell::Cell;

#[derive(Default)]
pub(super) struct GlesSubmissionState {
    pub epoch: Cell<u64>,
    #[cfg(feature = "shadows")]
    pub shadow_texture: Cell<Option<u32>>,
    program: Cell<Option<u32>>,
    vertex_array: Cell<Option<u32>>,
    /// Applied blend mode: 1 opaque, 2 straight alpha, 3 additive and 4
    /// premultiplied Surface cache composition.
    pub blend: Cell<Option<u8>>,
}

impl GlesSubmissionState {
    pub(super) fn invalidate(&self) {
        self.epoch.set(self.epoch.get().wrapping_add(1));
        #[cfg(feature = "shadows")]
        self.shadow_texture.set(None);
        self.program.set(None);
        self.vertex_array.set(None);
        self.blend.set(None);
    }
}

impl GlesRenderDevice {
    pub(super) fn use_program(&self, program: u32) {
        if self.submission.program.replace(Some(program)) != Some(program) {
            // SAFETY: The current context owns the live program. All renderer
            // program changes use this helper; deletion and Host entry invalidate
            // the cache before names can be reused. No CPU pointer is passed.
            unsafe { (self.gl.use_program)(program) };
        }
    }

    pub(super) fn bind_vertex_array(&self, vao: u32) {
        if self.submission.vertex_array.replace(Some(vao)) != Some(vao) {
            // SAFETY: The current context owns this VAO. Upload, draw and cleanup
            // use this helper; deletion and Host entry invalidate cached names.
            // Attribute changes still happen on the selected VAO as before.
            unsafe { (self.gl.bind_vertex_array)(vao) };
        }
    }
}
