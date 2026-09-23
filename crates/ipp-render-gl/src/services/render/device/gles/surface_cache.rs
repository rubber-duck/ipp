//! Native whole-Surface cache targets: one SRGB8_ALPHA8 colour texture and
//! framebuffer per cached Surface, repainted through the ordinary Surface draws
//! and composited as one premultiplied quad.
//!
//! Targets have no depth attachment. Repaint primitives are already clipped to
//! the content rectangle and compose in painter order with depth writes off, so
//! depth would never reject a fragment; without a depth buffer the enabled depth
//! test passes every fragment. Scene depth applies only when the image is
//! composited in the main pass.

use super::{GlesRenderDevice, GlesRenderProgram};
use crate::RenderError;
use std::ptr;

/// Largest cache image edge requested from a context, independent of how much
/// larger the device could allocate. Keeps a single image within 16 MiB.
const SURFACE_CACHE_MAX_DIMENSION: u32 = 2048;

const TEXTURE_2D: u32 = 0x0DE1;
const FRAMEBUFFER: u32 = 0x8D40;
const READ_FRAMEBUFFER: u32 = 0x8CA8;
const DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const READ_FRAMEBUFFER_BINDING: u32 = 0x8CAA;
const VIEWPORT: u32 = 0x0BA2;
const DEPTH_WRITEMASK: u32 = 0x0B72;
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
    draw: u32,
    read: u32,
    viewport: [i32; 4],
    surface_viewport: [f32; 2],
    depth_mask: bool,
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

        // SAFETY: The current context writes new exclusively owned names and the
        // saved bindings into locals. Storage uses no client pointer. The previous
        // draw/read framebuffers and viewport are borrowed host state, rebound
        // unchanged before returning, so no binding refers to a partial target.
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
            let mut draw = 0;
            let mut read = 0;
            let mut viewport = [0i32; 4];
            (self.gl.get_integer)(DRAW_FRAMEBUFFER_BINDING, &mut draw);
            (self.gl.get_integer)(READ_FRAMEBUFFER_BINDING, &mut read);
            (self.gl.get_integer)(VIEWPORT, viewport.as_mut_ptr());
            (self.gl.bind_framebuffer)(FRAMEBUFFER, target.framebuffer);
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
                (self.gl.viewport)(0, 0, width as i32, height as i32);
                (self.gl.clear_color)(0.0, 0.0, 0.0, 0.0);
                (self.gl.clear)(COLOR_BUFFER_BIT);
            }
            (self.gl.bind_framebuffer)(DRAW_FRAMEBUFFER, draw as u32);
            (self.gl.bind_framebuffer)(READ_FRAMEBUFFER, read as u32);
            (self.gl.viewport)(viewport[0], viewport[1], viewport[2], viewport[3]);
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

        // SAFETY: Queries write the borrowed host bindings, viewport and depth
        // mask into exclusive locals; nothing is retained by GL.
        let binding = unsafe {
            let mut draw = 0;
            let mut read = 0;
            let mut viewport = [0i32; 4];
            let mut depth_mask = 1;
            (self.gl.get_integer)(DRAW_FRAMEBUFFER_BINDING, &mut draw);
            (self.gl.get_integer)(READ_FRAMEBUFFER_BINDING, &mut read);
            (self.gl.get_integer)(VIEWPORT, viewport.as_mut_ptr());
            (self.gl.get_integer)(DEPTH_WRITEMASK, &mut depth_mask);
            GlesSurfaceCacheBinding {
                framebuffer: target.framebuffer,
                draw: draw as u32,
                read: read as u32,
                viewport,
                surface_viewport: self.surface_viewport,
                depth_mask: depth_mask != 0,
            }
        };
        self.surface_cache_target = Some(binding);

        // Box, path and glyph antialiasing now sizes pixels from the target.
        self.surface_viewport = [target.width as f32, target.height as f32];

        // SAFETY: The target's framebuffer is live and owned by this context; the
        // saved host bindings are restored by end. Scalar state only.
        unsafe {
            (self.gl.bind_framebuffer)(FRAMEBUFFER, target.framebuffer);
            (self.gl.viewport)(0, 0, target.width as i32, target.height as i32);
            (self.gl.disable)(0x0C11); // SCISSOR_TEST
            (self.gl.disable)(0x0B90); // STENCIL_TEST
            (self.gl.depth_mask)(0);
            (self.gl.color_mask)(1, 1, 1, 1);
            (self.gl.clear_color)(0.0, 0.0, 0.0, 0.0);
            (self.gl.clear)(COLOR_BUFFER_BIT);
        }

        if let Err(error) = self.check() {
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

        // SAFETY: Rebinds borrowed host handles and scalar state captured at begin
        // in this same current context; the device takes no ownership of them.
        unsafe {
            (self.gl.bind_framebuffer)(DRAW_FRAMEBUFFER, binding.draw);
            (self.gl.bind_framebuffer)(READ_FRAMEBUFFER, binding.read);
            (self.gl.viewport)(
                binding.viewport[0],
                binding.viewport[1],
                binding.viewport[2],
                binding.viewport[3],
            );
            (self.gl.depth_mask)(u8::from(binding.depth_mask));
        }
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

        let placement_location = self.surface_location(program, c"u_placement");
        let clip_location = self.surface_location(program, c"u_clip");
        let texture_location = self.surface_location(program, c"u_surface_cache");
        let rectangle = [0.0, 0.0, size[0], size[1]];
        let premultiplied =
            self.submission.blend.replace(Some(PREMULTIPLIED_BLEND)) != Some(PREMULTIPLIED_BLEND);

        // SAFETY: Program, texture and VAO are live names of this current context,
        // borrowed through the draw. Uniform calls copy the fixed local arrays
        // synchronously. The attribute-less VAO derives corners from gl_VertexID,
        // so no buffer or client pointer is read.
        unsafe {
            if premultiplied {
                // Opacity was applied once while painting; the image's colour is
                // already multiplied by its coverage alpha.
                (self.gl.enable)(0x0BE2); // BLEND
                (self.gl.blend_equation)(0x8006); // FUNC_ADD
                (self.gl.blend_func)(1, 0x0303, 1, 0x0303); // ONE, ONE_MINUS_SRC_ALPHA
                (self.gl.depth_mask)(0);
            }
            self.use_program(program.id);
            (self.gl.uniform_matrix)(program.mvp, 1, 0, mvp.as_ptr());
            (self.gl.uniform_vec4)(placement_location, 1, rectangle.as_ptr());
            (self.gl.uniform_vec4)(clip_location, 1, rectangle.as_ptr());
            (self.gl.uniform_int)(texture_location, 0);
            (self.gl.active_texture)(0x84C0);
            (self.gl.bind_sampler)(0, 0);
            (self.gl.bind_texture)(TEXTURE_2D, target.texture);
            self.bind_vertex_array(self.surface_quad_vao);
            (self.gl.draw_arrays)(0x0005, 0, 4); // TRIANGLE_STRIP
            self.bind_vertex_array(0);
            (self.gl.bind_texture)(TEXTURE_2D, 0);
        }

        // One check per composited Surface so context loss reaches recovery.
        self.check()
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
