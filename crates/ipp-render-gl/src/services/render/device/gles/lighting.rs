//! Native uniform binding and depth-only target operations.
use super::super::uniform_cache::*;
use super::*;

pub(super) struct GlesLightingLocations {
    model: i32,
    normal: i32,
    camera: i32,
    ambient: i32,
    surface: i32,
    lights: i32,
    count: i32,
    #[cfg(feature = "shadows")]
    shadow_map: i32,
    #[cfg(feature = "shadows")]
    shadow_matrix: i32,
    #[cfg(feature = "shadows")]
    shadow_settings: i32,
}

impl GlesLightingLocations {
    pub(super) fn load(gl: &Functions, id: u32) -> Self {
        // SAFETY: GlesRenderProgram creation supplies its live context-owned program. Static
        // names are terminated; these queries retain no pointers or CPU aliases.
        unsafe {
            Self {
                model: (gl.uniform_location)(id, c"u_model".as_ptr()),
                normal: (gl.uniform_location)(id, c"u_normal".as_ptr()),
                camera: (gl.uniform_location)(id, c"u_camera".as_ptr()),
                ambient: (gl.uniform_location)(id, c"u_ambient".as_ptr()),
                surface: (gl.uniform_location)(id, c"u_surface".as_ptr()),
                lights: (gl.uniform_location)(id, c"u_lights[0]".as_ptr()),
                count: (gl.uniform_location)(id, c"u_light_count".as_ptr()),
                #[cfg(feature = "shadows")]
                shadow_map: (gl.uniform_location)(id, c"u_shadow_map".as_ptr()),
                #[cfg(feature = "shadows")]
                shadow_matrix: (gl.uniform_location)(id, c"u_shadow_matrix[0]".as_ptr()),
                #[cfg(feature = "shadows")]
                shadow_settings: (gl.uniform_location)(id, c"u_shadow_settings[0]".as_ptr()),
            }
        }
    }
}

impl GlesRenderDevice {
    pub(super) fn lighting_uniforms(
        &self,
        program: &GlesRenderProgram,
        model: &[f32; 16],
        normal: &[f32; 16],
        surface: &[f32; 3],
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        let locations = &program.lighting;
        let changed =
            program
                .uniforms
                .borrow_mut()
                .lighting(self.submission.epoch.get(), surface, frame);
        // SAFETY: The owning context is current and all uniform slices remain live,
        // immutably borrowed and synchronously copied. No pointer survives the call.
        unsafe {
            self.use_program(program.id);
            if locations.model >= 0 {
                (self.gl.uniform_matrix)(locations.model, 1, 0, model.as_ptr());
            }
            if locations.normal >= 0 {
                (self.gl.uniform_matrix)(locations.normal, 1, 0, normal.as_ptr());
            }
            if changed & CAMERA != 0 && locations.camera >= 0 {
                (self.gl.uniform_vec4)(locations.camera, 1, frame.camera.as_ptr());
            }
            if changed & AMBIENT != 0 && locations.ambient >= 0 {
                (self.gl.uniform_rgb)(locations.ambient, 1, frame.ambient.as_ptr());
            }
            if changed & SURFACE != 0 && locations.surface >= 0 {
                (self.gl.uniform_rgb)(locations.surface, 1, surface.as_ptr());
            }
            if changed & LIGHTS != 0 && locations.lights >= 0 && frame.count > 0 {
                (self.gl.uniform_vec4)(locations.lights, frame.count * 4, frame.lights.as_ptr());
            }
            if changed & COUNT != 0 && locations.count >= 0 {
                (self.gl.uniform_int)(locations.count, frame.count);
            }
        }
        self.check_draw()
    }
}

/// One exclusively owned depth texture and framebuffer in the native context.
#[cfg(feature = "shadows")]
pub struct GlesShadowMap {
    texture: u32,
    framebuffer: u32,
    size: u32,
}

#[cfg(feature = "shadows")]
impl GlesRenderDevice {
    pub(super) fn allocate_shadow(&self, size: u32) -> Result<GlesShadowMap, RenderError> {
        if size == 0
            || size > self.max_texture_size
            || self.max_viewport.iter().any(|&limit| size > limit as u32)
        {
            return Err(RenderError::RenderDevice(
                "shadow map exceeds GLES limits".into(),
            ));
        }
        let mut map = GlesShadowMap {
            texture: 0,
            framebuffer: 0,
            size,
        };
        let mut draw = 0;
        let mut read = 0;
        // SAFETY: Context lifetime is guaranteed by from_loader. GL writes only
        // exclusive locals; null pixel input allocates storage. Partial names are
        // deleted on failure, framebuffer bindings restored, no CPU borrow retained.
        let complete = unsafe {
            (self.gl.get_integer)(0x8CA6, &mut draw);
            (self.gl.get_integer)(0x8CAA, &mut read);
            (self.gl.gen_textures)(1, &mut map.texture);
            (self.gl.gen_framebuffers)(1, &mut map.framebuffer);
            if map.texture == 0 || map.framebuffer == 0 {
                self.free_shadow(map);
                return Err(RenderError::RenderDevice(
                    "GLES shadow allocation failed".into(),
                ));
            }
            self.submission.shadow_texture.set(None);
            (self.gl.active_texture)(0x84C1);
            (self.gl.bind_texture)(0x0DE1, map.texture);
            (self.gl.bind_buffer)(0x88EC, 0);
            (self.gl.tex_parameter)(0x0DE1, 0x2801, 0x2600);
            (self.gl.tex_parameter)(0x0DE1, 0x2800, 0x2600);
            (self.gl.tex_parameter)(0x0DE1, 0x2802, 0x812F);
            (self.gl.tex_parameter)(0x0DE1, 0x2803, 0x812F);
            (self.gl.tex_parameter)(0x0DE1, 0x813D, 0);
            (self.gl.tex_image)(
                0x0DE1,
                0,
                0x81A6,
                size as i32,
                size as i32,
                0,
                0x1902,
                0x1405,
                ptr::null(),
            );
            (self.gl.bind_framebuffer)(0x8D40, map.framebuffer);
            (self.gl.framebuffer_texture)(0x8D40, 0x8D00, 0x0DE1, map.texture, 0);
            (self.gl.draw_buffers)(1, &0);
            (self.gl.read_buffer)(0);
            let complete = (self.gl.check_framebuffer)(0x8D40) == 0x8CD5;
            (self.gl.bind_framebuffer)(0x8CA9, draw as u32);
            (self.gl.bind_framebuffer)(0x8CA8, read as u32);
            (self.gl.bind_texture)(0x0DE1, 0);
            complete
        };
        let result = self.check().and_then(|()| {
            if complete && map.texture != 0 && map.framebuffer != 0 {
                Ok(())
            } else {
                Err(RenderError::RenderDevice(
                    "GLES depth framebuffer unavailable".into(),
                ))
            }
        });
        if let Err(error) = result {
            self.free_shadow(map);
            return Err(error);
        }
        Ok(map)
    }

    pub(super) fn start_shadow(
        &mut self,
        map: &GlesShadowMap,
        slot: u32,
        grid: u32,
    ) -> Result<(), RenderError> {
        self.submission.blend.set(None);
        let mut target = 0;
        let mut viewport = [0; 4];
        // SAFETY: Queries write their scalar/four-element outputs to exclusive
        // locals. Bound names belong to this live context; no CPU pointers retained.
        unsafe {
            (self.gl.get_integer)(0x8CA6, &mut target);
            (self.gl.get_integer)(0x0BA2, viewport.as_mut_ptr());
            self.shadow_target = Some((target as u32, viewport));
            self.submission.shadow_texture.set(None);
            (self.gl.active_texture)(0x84C1);
            (self.gl.bind_texture)(0x0DE1, 0);
            (self.gl.bind_framebuffer)(0x8CA9, map.framebuffer);
            let tile = (map.size / grid) as i32;
            (self.gl.viewport)(
                (slot % grid) as i32 * tile,
                (slot / grid) as i32 * tile,
                tile,
                tile,
            );
            (self.gl.color_mask)(0, 0, 0, 0);
            if slot == 0 {
                (self.gl.depth_mask)(1);
                (self.gl.clear_depth)(1.0);
                (self.gl.clear)(0x00000100);
            }
        }
        self.check()
    }

    pub(super) fn finish_shadow(&mut self) -> Result<(), RenderError> {
        if let Some((target, viewport)) = self.shadow_target.take() {
            // SAFETY: Restores the host-owned live target and copied viewport, with
            // no retained CPU pointer or alias. Handles are never transferred.
            unsafe {
                (self.gl.bind_framebuffer)(0x8CA9, target);
                (self.gl.viewport)(viewport[0], viewport[1], viewport[2], viewport[3]);
                (self.gl.color_mask)(1, 1, 1, 1);
            }
        }
        self.check()
    }

    pub(super) fn shadow_uniforms(
        &self,
        program: &GlesRenderProgram,
        map: &GlesShadowMap,
        frame: &crate::RenderLightingFrame,
    ) -> Result<(), RenderError> {
        if program.lighting.shadow_map < 0
            && program.lighting.shadow_matrix < 0
            && program.lighting.shadow_settings < 0
        {
            return self.check_draw();
        }
        let changed = program
            .uniforms
            .borrow_mut()
            .shadows(self.submission.epoch.get(), frame);
        // SAFETY: Context owns all names; uploads consume active immutable slices
        // synchronously. The cache owns copies and retains no CPU source pointers.
        unsafe {
            self.use_program(program.id);
            if self.submission.shadow_texture.replace(Some(map.texture)) != Some(map.texture) {
                (self.gl.active_texture)(0x84C1);
                (self.gl.bind_sampler)(1, 0);
                (self.gl.bind_texture)(0x0DE1, map.texture);
            }
            if changed & SHADOW_SAMPLER != 0 {
                (self.gl.uniform_int)(program.lighting.shadow_map, 1);
            }
            if changed & SHADOW_MATRICES != 0 && frame.count > 0 {
                (self.gl.uniform_matrix)(
                    program.lighting.shadow_matrix,
                    frame.count,
                    0,
                    frame.shadow_matrices.as_ptr(),
                );
            }
            if changed & SHADOW_SETTINGS != 0 && frame.count > 0 {
                (self.gl.uniform_vec4)(
                    program.lighting.shadow_settings,
                    frame.count,
                    frame.shadow_settings.as_ptr(),
                );
            }
        }
        self.check_draw()
    }

    pub(super) fn free_shadow(&self, map: GlesShadowMap) {
        self.submission.shadow_texture.set(None);
        // SAFETY: Consumes exclusively owned names once in the current context;
        // GL tolerates zero names and context loss. No CPU data is accessed.
        unsafe {
            (self.gl.delete_framebuffers)(1, &map.framebuffer);
            (self.gl.delete_textures)(1, &map.texture);
        }
    }
}
