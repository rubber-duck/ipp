//! One-use Host-owned snapshot names; no pixels or JS objects enter WASM memory.

use super::commands::WebGlRenderDevice;
use crate::RenderError;

#[link(wasm_import_module = "ipp_gl")]
unsafe extern "C" {
    fn web_image_copy(image: u32, texture: u32, width: u32, height: u32) -> u32;
}

impl WebGlRenderDevice {
    /// Consume a previously staged browser image into a renderer-reserved private texture.
    /// The bridge closes the image on success, rejection, failure or context loss.
    /// Only the verified producer's top-left, straight-alpha sRGB snapshots qualify.
    pub fn copy_web_image(
        &mut self,
        image: u32,
        destination: &u32,
        pixel_size: [u32; 2],
    ) -> Result<(), RenderError> {
        // SAFETY: Only opaque context-owned names and dimensions cross the bridge.
        // It validates exact names/extents, consumes the snapshot synchronously,
        // retains no Rust reference and never reenters the World or renderer.
        self.check(unsafe { web_image_copy(image, *destination, pixel_size[0], pixel_size[1]) })
    }
}
