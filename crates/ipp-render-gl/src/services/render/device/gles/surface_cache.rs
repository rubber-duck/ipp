//! Native whole-Surface cache targets: one SRGB8_ALPHA8 colour texture and
//! framebuffer per cached Surface, repainted through the ordinary Surface draws
//! and composited as one premultiplied quad.
//!
//! Targets have no depth attachment. Repaint primitives are already clipped to
//! the content rectangle and compose in painter order with depth writes off, so
//! depth would never reject a fragment; without a depth buffer the enabled depth
//! test passes every fragment. Scene depth applies only when the image is
//! composited in the main pass.

use super::targets::GlesTarget;
use super::{GlesRenderDevice, GlesRenderProgram};
use crate::RenderError;
use std::ptr;

/// Largest cache image edge requested from a context, independent of how much
/// larger the device could allocate. Keeps a single image within 16 MiB.
const SURFACE_CACHE_MAX_DIMENSION: u32 = 2048;

const TEXTURE_2D: u32 = 0x0DE1;
const FRAMEBUFFER: u32 = 0x8D40;
const FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const COLOR_BUFFER_BIT: u32 = 0x0000_4000;

/// Blend mode tag in the submission cache for premultiplied composition.
const PREMULTIPLIED_BLEND: u8 = 4;

/// Context-owned premultiplied whole-Surface image and its framebuffer.
pub struct GlesSurfaceCacheTarget {
    texture: u32,
    framebuffer: u32,
    width: u32,
    height: u32,
}

/// State saved by `begin_surface_cache_target` and restored by its end.
pub(super) struct GlesSurfaceCacheBinding {
    framebuffer: u32,
    host: GlesTarget,
    surface_viewport: [f32; 2],
}

impl GlesRenderDevice {
    pub(super) fn surface_cache_dimension_limit(&self) -> u32 {
        self.max_texture_size
            .min(self.max_viewport[0].max(0) as u32)
            .min(self.max_viewport[1].max(0) as u32)
            .min(SURFACE_CACHE_MAX_DIMENSION)
    }

    fn validate_surface_cache_size(&self, width: u32, height: u32) -> Result<(), RenderError> {
        let limit = self.surface_cache_dimension_limit();
        if width == 0 || height == 0 || width > limit || height > limit {
            return Err(RenderError::RenderDevice(format!(
                "invalid Surface cache target size {width}x{height} (limit {limit})"
            )));
        }
        Ok(())
    }

    /// Specify level zero of the bound texture as empty SRGB8_ALPHA8 storage.
    ///
    /// # Safety
    ///
    /// The device context is current and `TEXTURE_2D` on the active unit names a
    /// texture this device owns. No pixel pointer is passed or retained.
    unsafe fn specify_surface_cache_storage(&self, width: u32, height: u32) {
        // SAFETY: Upheld by the caller; the null pixel pointer requests
        // uninitialized storage and GL reads no client memory.
        unsafe {
            (self.gl.bind_buffer)(0x88EC, 0); // PIXEL_UNPACK_BUFFER
            (self.gl.tex_image)(
                TEXTURE_2D,
                0,
                // SRGB8_ALPHA8, as the main linear target: blending stays linear
                // and sampling decodes before filtering.
                0x8C43,
                width as i32,
                height as i32,
                0,
                0x1908, // RGBA
                0x1401, // UNSIGNED_BYTE
                ptr::null(),
            );
        }
    }

    pub(super) fn create_surface_cache_target(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<GlesSurfaceCacheTarget, RenderError> {
        self.validate_surface_cache_size(width, height)?;
        self.submission.invalidate();
        let mut target = GlesSurfaceCacheTarget {
            texture: 0,
            framebuffer: 0,
            width,
            height,
        };

        let previous = self.current_target();

        // SAFETY: The current context writes new exclusively owned names into
        // locals. Storage uses no client pointer. The previous draw/read
        // framebuffers and viewport are borrowed host state, rebound unchanged
        // before returning, so no binding refers to a partial target.
        let complete = unsafe {
            (self.gl.gen_textures)(1, &mut target.texture);
            (self.gl.gen_framebuffers)(1, &mut target.framebuffer);
            (self.gl.active_texture)(0x84C0); // TEXTURE0
            (self.gl.bind_texture)(TEXTURE_2D, target.texture);
            (self.gl.tex_parameter)(TEXTURE_2D, 0x2801, 0x2601); // MIN_FILTER LINEAR
            (self.gl.tex_parameter)(TEXTURE_2D, 0x2800, 0x2601); // MAG_FILTER LINEAR
            (self.gl.tex_parameter)(TEXTURE_2D, 0x2802, 0x812F); // WRAP_S CLAMP_TO_EDGE
            (self.gl.tex_parameter)(TEXTURE_2D, 0x2803, 0x812F); // WRAP_T CLAMP_TO_EDGE
            (self.gl.tex_parameter)(TEXTURE_2D, 0x813C, 0); // BASE_LEVEL
            (self.gl.tex_parameter)(TEXTURE_2D, 0x813D, 0); // MAX_LEVEL
            self.specify_surface_cache_storage(width, height);
            (self.gl.bind_texture)(TEXTURE_2D, 0);
            self.bind_framebuffers(target.framebuffer, target.framebuffer);
            (self.gl.framebuffer_texture)(
                FRAMEBUFFER,
                0x8CE0, // COLOR_ATTACHMENT0
                TEXTURE_2D,
                target.texture,
                0,
            );
            let complete = (self.gl.check_framebuffer)(FRAMEBUFFER) == FRAMEBUFFER_COMPLETE;
            if complete {
                // Uninitialized storage must never be composited as an image.
                self.set_viewport([0, 0, width as i32, height as i32]);
                (self.gl.clear_color)(0.0, 0.0, 0.0, 0.0);
                (self.gl.clear)(COLOR_BUFFER_BIT);
            }
            self.bind_framebuffers(previous.draw, previous.read);
            self.set_viewport(previous.viewport);
            complete && target.texture != 0 && target.framebuffer != 0
        };

        let checked = self.check();
        if !complete || checked.is_err() {
            self.delete_surface_cache_target(target);
            // Context loss stays distinguishable so the service reaches recovery.
            return Err(checked.err().unwrap_or_else(|| {
                RenderError::RenderDevice("Surface cache target allocation failed".into())
            }));
        }
        Ok(target)
    }

    pub(super) fn resize_surface_cache_target(
        &mut self,
        target: &mut GlesSurfaceCacheTarget,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        self.validate_surface_cache_size(width, height)?;
        if self
            .surface_cache_target
            .as_ref()
            .is_some_and(|bound| bound.framebuffer == target.framebuffer)
        {
            return Err(RenderError::RenderDevice(
                "cannot resize the bound Surface cache target".into(),
            ));
        }

        // SAFETY: The current context owns the texture. Respecifying level zero
        // keeps the framebuffer attachment and passes no client pointer; the
        // texture is unbound again before any other texture use.
        unsafe {
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_texture)(TEXTURE_2D, target.texture);
            self.specify_surface_cache_storage(width, height);
            (self.gl.bind_texture)(TEXTURE_2D, 0);
        }

        // Dimensions describe the storage only after GL accepted it.
        self.check()?;
        target.width = width;
        target.height = height;
        Ok(())
    }

    pub(super) fn begin_surface_cache_target(
        &mut self,
        target: &GlesSurfaceCacheTarget,
    ) -> Result<(), RenderError> {
        if self.surface_cache_target.is_some() {
            return Err(RenderError::RenderDevice(
                "Surface cache targets cannot nest".into(),
            ));
        }
        #[cfg(feature = "gui")]
        if self.glyph_atlas_target.is_some() {
            return Err(RenderError::RenderDevice(
                "Surface cache target inside glyph atlas population".into(),
            ));
        }
        self.submission.invalidate();

        // The host target is known without a synchronous query once this device
        // has set it; otherwise it is read once for the frame.
        self.surface_cache_target = Some(GlesSurfaceCacheBinding {
            framebuffer: target.framebuffer,
            host: self.current_target(),
            surface_viewport: self.surface_viewport,
        });

        // Box, path and glyph antialiasing now sizes pixels from the target.
        self.surface_viewport = [target.width as f32, target.height as f32];

        // The target's framebuffer is live and owned by this context; end
        // restores the saved host bindings.
        self.bind_framebuffers(target.framebuffer, target.framebuffer);
        self.set_viewport([0, 0, target.width as i32, target.height as i32]);
        self.set_depth_mask(false);
        // SAFETY: Scalar context state in the current context only.
        unsafe {
            (self.gl.disable)(0x0C11); // SCISSOR_TEST
            (self.gl.disable)(0x0B90); // STENCIL_TEST
            (self.gl.color_mask)(1, 1, 1, 1);
            (self.gl.clear_color)(0.0, 0.0, 0.0, 0.0);
            (self.gl.clear)(COLOR_BUFFER_BIT);
        }

        // The end of the repaint checks the whole pass.
        if let Err(error) = self.check_draw() {
            self.restore_surface_cache_binding();
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn end_surface_cache_target(&mut self) -> Result<(), RenderError> {
        self.restore_surface_cache_binding();

        // A failed repaint must never be kept as a complete image.
        self.check()
    }

    /// Rebind the host target, viewport and depth mask saved by begin, if any.
    fn restore_surface_cache_binding(&mut self) {
        let Some(binding) = self.surface_cache_target.take() else {
            return;
        };

        // Repaint draws changed blending and depth writes behind the submission
        // cache; the next draw reapplies its state.
        self.submission.invalidate();
        self.surface_viewport = binding.surface_viewport;

        // Rebinds borrowed host handles and state saved at begin in this same
        // current context; the device takes no ownership of them.
        self.restore_target(binding.host);
    }

    pub(super) fn draw_surface_cache(
        &mut self,
        program: &GlesRenderProgram,
        target: &GlesSurfaceCacheTarget,
        mvp: &[f32; 16],
        size: &[f32; 2],
    ) -> Result<(), RenderError> {
        if self
            .surface_cache_target
            .as_ref()
            .is_some_and(|bound| bound.framebuffer == target.framebuffer)
        {
            return Err(RenderError::RenderDevice(
                "cannot sample the bound Surface cache target".into(),
            ));
        }
        if !(size[0] > 0.0 && size[1] > 0.0 && size[0].is_finite() && size[1].is_finite()) {
            return Err(RenderError::RenderDevice(
                "invalid Surface cache composite size".into(),
            ));
        }
        if self.surface_quad_vao == 0 {
            // SAFETY: GL writes one new name owned by this current context.
            unsafe { (self.gl.gen_vertex_arrays)(1, &mut self.surface_quad_vao) };
            if self.surface_quad_vao == 0 {
                return Err(RenderError::RenderDevice(
                    "surface quad allocation failed".into(),
                ));
            }
        }

        let rectangle = [0.0, 0.0, size[0], size[1]];
        let premultiplied =
            self.submission.blend.replace(Some(PREMULTIPLIED_BLEND)) != Some(PREMULTIPLIED_BLEND);
        if premultiplied {
            // Opacity was applied once while painting; the image's colour is
            // already multiplied by its coverage alpha.
            // SAFETY: Scalar blend state in the current context.
            unsafe {
                (self.gl.enable)(0x0BE2); // BLEND
                (self.gl.blend_equation)(0x8006); // FUNC_ADD
                (self.gl.blend_func)(1, 0x0303, 1, 0x0303); // ONE, ONE_MINUS_SRC_ALPHA
            }
            self.set_depth_mask(false);
        }
        self.use_program(program.id);
        let location = |name| self.surface_location(program, name);
        self.program_mat4(program, program.mvp, mvp);
        self.program_vec4(program, location(c"u_placement"), &rectangle);
        self.program_vec4(program, location(c"u_clip"), &rectangle);
        self.program_int(program, location(c"u_surface_cache"), 0);
        self.bind_vertex_array(self.surface_quad_vao);

        // SAFETY: The texture and attribute-less VAO are live names of this
        // current context, borrowed through the draw; corners derive from
        // gl_VertexID, so no buffer or client pointer is read. The image is
        // unbound again so a later repaint of it never samples its own target.
        unsafe {
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_sampler)(0, 0);
            (self.gl.bind_texture)(TEXTURE_2D, target.texture);
            (self.gl.draw_arrays)(0x0005, 0, 4); // TRIANGLE_STRIP
            (self.gl.bind_texture)(TEXTURE_2D, 0);
        }

        // Composites are routine draws; the frame end reports their errors.
        self.check_draw()
    }

    pub(super) fn delete_surface_cache_target(&mut self, target: GlesSurfaceCacheTarget) {
        // Deleting the bound framebuffer would silently rebind framebuffer zero;
        // restore the saved host target first.
        if self
            .surface_cache_target
            .as_ref()
            .is_some_and(|bound| bound.framebuffer == target.framebuffer)
        {
            self.restore_surface_cache_binding();
        }

        self.forget_framebuffer(target.framebuffer);

        // SAFETY: Consumes names exclusively owned by this target once. GL defers
        // releasing storage still referenced by queued draws, ignores zero names
        // and tolerates names invalidated by context loss.
        unsafe {
            if target.framebuffer != 0 {
                (self.gl.delete_framebuffers)(1, &target.framebuffer);
            }
            if target.texture != 0 {
                (self.gl.delete_textures)(1, &target.texture);
            }
        }
    }
}
