use super::super::loader::Functions;

// No device cache is changed by the copy. Restoring exact physical state keeps
// retained renderer submission and target caches truthful on every return path.
pub(super) struct State {
    draw: i32,
    read: i32,
    program: i32,
    vao: i32,
    viewport: [i32; 4],
    active: i32,
    texture: i32,
    sampler: i32,
    mask: [i32; 4],
    enabled: [bool; 8],
    srgb: Option<bool>,
}

const CAPS: [u32; 8] = [
    0x0BE2, 0x0B71, 0x0C11, 0x0B44, 0x0B90, 0x8C89, 0x0BD0, 0x809E,
];

impl State {
    pub(super) fn save(gl: &Functions, srgb_write_control: bool) -> Self {
        let mut state = Self {
            draw: 0,
            read: 0,
            program: 0,
            vao: 0,
            viewport: [0; 4],
            active: 0,
            texture: 0,
            sampler: 0,
            mask: [0; 4],
            enabled: [false; 8],
            srgb: None,
        };
        // SAFETY: The device contract gives a current context. All query outputs
        // are exclusive stack storage of the specified scalar/four-value lengths.
        unsafe {
            gl.get_integer(0x8CA6, &mut state.draw);
            gl.get_integer(0x8CAA, &mut state.read);
            gl.get_integer(0x8B8D, &mut state.program);
            gl.get_integer(0x85B5, &mut state.vao);
            gl.get_integer(0x0BA2, state.viewport.as_mut_ptr());
            gl.get_integer(0x84E0, &mut state.active);
            gl.get_integer(0x0C23, state.mask.as_mut_ptr());
            gl.active_texture(0x84C0);
            gl.get_integer(0x8069, &mut state.texture);
            gl.get_integer(0x8919, &mut state.sampler);
            state.enabled = CAPS.map(|cap| gl.is_enabled(cap) != 0);
            if srgb_write_control {
                state.srgb = Some(gl.is_enabled(0x8DB9) != 0);
            }
        }
        state
    }

    pub(super) fn prepare(&self, gl: &Functions) {
        // SAFETY: All capabilities are core GLES3, except the extension flag
        // whose availability was established during importer construction.
        unsafe {
            for cap in CAPS {
                gl.disable(cap);
            }
            if self.srgb.is_some() {
                gl.enable(0x8DB9);
            }
            gl.color_mask(1, 1, 1, 1);
            gl.bind_sampler(0, 0);
        }
    }

    pub(super) fn restore(&self, gl: &Functions) {
        // SAFETY: Saved objects remain owned by the caller/device for the whole
        // operation. No deletion occurs until these bindings have been restored.
        unsafe {
            gl.bind_framebuffer(0x8CA9, self.draw as u32);
            gl.bind_framebuffer(0x8CA8, self.read as u32);
            gl.use_program(self.program as u32);
            gl.bind_vertex_array(self.vao as u32);
            gl.viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            gl.color_mask(
                self.mask[0] as u8,
                self.mask[1] as u8,
                self.mask[2] as u8,
                self.mask[3] as u8,
            );
            gl.bind_texture(0x0DE1, self.texture as u32);
            gl.bind_sampler(0, self.sampler as u32);
            gl.active_texture(self.active as u32);
            for (cap, enabled) in CAPS.into_iter().zip(self.enabled) {
                if enabled {
                    gl.enable(cap);
                } else {
                    gl.disable(cap);
                }
            }
            if let Some(enabled) = self.srgb {
                if enabled {
                    gl.enable(0x8DB9);
                } else {
                    gl.disable(0x8DB9);
                }
            }
        }
    }
}
