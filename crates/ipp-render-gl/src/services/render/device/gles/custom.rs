use super::*;

impl GlesRenderDevice {
    pub(super) fn prepare_parameter_storage(
        &mut self,
        program: &GlesRenderProgram,
        words: usize,
        textures: usize,
    ) -> Result<(), RenderError> {
        let bytes = words.saturating_mul(4);
        if bytes > self.max_parameter_bytes || textures + 2 > self.max_parameter_textures {
            return Err(RenderError::RenderDevice(
                "custom material exceeds uniform buffer or texture unit limits".into(),
            ));
        }
        if words == 0 || program.parameters == u32::MAX || bytes <= self.parameter_capacity {
            return self.check_draw();
        }
        // SAFETY: The current context owns the buffer. Null data reserves only
        // GPU storage; capacity is committed only after the allocation check.
        unsafe {
            if self.parameter_buffer == 0 {
                (self.gl.gen_buffers)(1, &mut self.parameter_buffer);
            }
            if self.parameter_buffer == 0 {
                return Err(RenderError::RenderDevice(
                    "parameter buffer allocation failed".into(),
                ));
            }
            let capacity = bytes
                .max(self.parameter_capacity.saturating_mul(2))
                .max(256);
            (self.gl.bind_buffer)(0x8A11, self.parameter_buffer);
            (self.gl.buffer_data)(0x8A11, capacity as isize, ptr::null(), 0x88E0);
            self.check()?;
            self.parameter_capacity = capacity;
        }
        Ok(())
    }

    pub(super) fn upload_custom_parameters<'a>(
        &mut self,
        program: &GlesRenderProgram,
        words: &[u32],
        textures: impl ExactSizeIterator<Item = Result<(&'a str, &'a u32), RenderError>>,
        alpha_mode: u32,
        alpha_cutoff: f32,
    ) -> Result<(), RenderError> {
        self.prepare_parameter_storage(program, words.len(), textures.len())?;
        // SAFETY: The current context owns the program and buffer. Uploads copy
        // live borrowed words synchronously and retain no CPU pointers or aliases.
        unsafe {
            self.use_program(program.id);
            if !words.is_empty() && program.parameters != u32::MAX {
                (self.gl.bind_buffer)(0x8A11, self.parameter_buffer);
                let bytes = std::mem::size_of_val(words);
                (self.gl.buffer_sub_data)(0x8A11, 0, bytes as isize, words.as_ptr().cast());
                (self.gl.bind_buffer_base)(0x8A11, 0, self.parameter_buffer);
                (self.gl.uniform_block_binding)(program.id, program.parameters, 0);
            }
            for (index, texture) in textures.enumerate() {
                let (name, texture) = texture?;
                let unit = (index + 2) as u32;
                let location = self.custom_texture_location(program, name)?;
                (self.gl.active_texture)(0x84C0 + unit);
                (self.gl.bind_sampler)(unit, 0);
                (self.gl.bind_texture)(0x0DE1, *texture);
                self.program_int(program, location, unit as i32);
            }
            (self.gl.uniform_int)(program.alpha_mode, alpha_mode as i32);
            (self.gl.uniform_float)(program.alpha_cutoff, alpha_cutoff);
        }
        self.check_draw()
    }

    fn custom_texture_location(
        &self,
        program: &GlesRenderProgram,
        name: &str,
    ) -> Result<i32, RenderError> {
        let mut locations = program.parameter_locations.borrow_mut();
        if let Some(location) = locations.get(name) {
            return Ok(*location);
        }
        let c_name = std::ffi::CString::new(format!("p_{name}"))
            .map_err(|_| RenderError::RenderDevice("invalid parameter name".into()))?;
        // SAFETY: Program is live in the current context. The name is terminated,
        // borrowed only for this synchronous lookup, and never retained by GL.
        let location = unsafe { (self.gl.uniform_location)(program.id, c_name.as_ptr()) };
        locations.insert(name.into(), location);
        Ok(location)
    }

    pub(super) fn alpha_blend(&mut self, enabled: bool) -> Result<(), RenderError> {
        let mode = if enabled {
            2
        } else {
            1
        };
        if self.submission.blend.replace(Some(mode)) == Some(mode) {
            return self.check_draw();
        }
        // SAFETY: Context is current; these calls copy scalar state only.
        unsafe {
            if enabled {
                (self.gl.enable)(0x0BE2);
                (self.gl.blend_equation)(0x8006);
                (self.gl.blend_func)(0x0302, 0x0303, 1, 0x0303);
            } else {
                (self.gl.disable)(0x0BE2);
            }
            (self.gl.depth_mask)(u8::from(!enabled));
        }
        self.check_draw()
    }
}
