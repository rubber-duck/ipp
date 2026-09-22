use super::*;

impl GlesRenderDevice {
    /// Load the required GLES entry points and validate the unlit baseline.
    ///
    /// The host target must provide a depth buffer and accept already encoded
    /// sRGB output without a second conversion (for example an RGBA8 target).
    ///
    /// # Safety
    /// The loader must return the exact GLES function signatures for the names
    /// requested, valid until this device and all renderer resources are dropped.
    /// A GLES 3 context must remain current on this thread for every operation,
    /// including destruction. The host must drop the renderer before destroying
    /// that context and must not concurrently mutate its GL state.
    pub unsafe fn from_loader(
        loader: impl FnMut(&CStr) -> *const c_void,
    ) -> Result<Self, RenderError> {
        // SAFETY: The caller guarantees entry point signatures and lifetimes.
        let gl = unsafe { Functions::load(loader)? };
        // SAFETY: The caller provides a current context. GL owns a terminated
        // version string valid while this context remains alive; we only read it.
        let version = unsafe { (gl.get_string)(0x1F02) };
        if version.is_null() {
            return Err(RenderError::RenderDevice("no current GLES context".into()));
        }

        // SAFETY: The non-null GL version pointer is terminated and live above.
        let version = unsafe { CStr::from_ptr(version.cast()) }.to_string_lossy();
        if !version.starts_with("OpenGL ES 3.") {
            return Err(RenderError::RenderDevice(format!(
                "GLES 3 required, got {version}"
            )));
        }

        let mut attributes = 0;
        let mut depth = 0;
        let mut max_viewport = [0; 2];
        // SAFETY: Each query writes its specified scalar/two-element result into
        // live exclusively borrowed storage. The GL context remains current.
        unsafe {
            (gl.get_integer)(0x8869, &mut attributes);
            (gl.get_integer)(0x0D56, &mut depth);
            (gl.get_integer)(0x0D3A, max_viewport.as_mut_ptr());
        }
        let required_attributes = if cfg!(feature = "particles") {
            14
        } else if cfg!(feature = "mesh-poses") {
            9
        } else if cfg!(feature = "skeletal-animation") {
            7
        } else {
            5
        };
        if attributes < required_attributes
            || depth < 16
            || max_viewport.iter().any(|value| *value <= 0)
        {
            return Err(RenderError::RenderDevice(
                "GLES attributes/depth/viewport baseline unavailable".into(),
            ));
        }

        let max_texture_size = {
            let mut limit = 0;
            // SAFETY: Current context writes one integer into exclusive storage.
            unsafe { (gl.get_integer)(0x0D33, &mut limit) };
            if limit <= 0 {
                return Err(RenderError::RenderDevice(
                    "GLES texture size baseline unavailable".into(),
                ));
            }
            limit as u32
        };

        let mut max_parameter_bytes = 0;
        let mut vertex_units = 0;
        let mut fragment_units = 0;
        // SAFETY: The current context writes its fixed limits into exclusive locals.
        unsafe {
            (gl.get_integer)(0x8A30, &mut max_parameter_bytes);
            (gl.get_integer)(0x8B4C, &mut vertex_units);
            (gl.get_integer)(0x8872, &mut fragment_units);
        }

        let device = Self {
            gl,
            parameter_buffer: 0,
            parameter_capacity: 0,
            max_parameter_bytes: max_parameter_bytes.max(0) as usize,
            max_parameter_textures: vertex_units.min(fragment_units).max(0) as usize,
            submission: Default::default(),
            #[cfg(feature = "particles")]
            instance_buffer: 0,
            #[cfg(feature = "particles")]
            instance_capacity: 0,
            #[cfg(feature = "particles")]
            instance_count: 0,
            linear_target: None,
            presentation_target: None,
            max_viewport,
            max_texture_size,
            exhaustive_draw_checks: false,
            #[cfg(feature = "surfaces")]
            surface_quad_vao: 0,
            #[cfg(feature = "gui")]
            surface_box_quad_vao: 0,
            #[cfg(feature = "gui")]
            surface_box_quad_vbo: 0,
            #[cfg(feature = "surfaces")]
            surface_instance_buffer: 0,
            #[cfg(feature = "surfaces")]
            surface_instance_capacity: 0,
            #[cfg(feature = "surfaces")]
            surface_instance_scratch: Vec::new(),
            #[cfg(feature = "surfaces")]
            surface_viewport: [1.0, 1.0],
            #[cfg(feature = "shadows")]
            shadow_target: None,
            _thread: PhantomData,
        };
        device.check()?;
        Ok(device)
    }

    pub(super) fn upload_attribute<T>(
        &self,
        slot: u32,
        width: i32,
        format: u32,
        normalized: bool,
        values: &[T],
    ) -> Result<u32, RenderError> {
        let mut buffer = 0;
        // SAFETY: The caller binds this mesh's VAO; GL copies the live immutable
        // slice synchronously. The enabled attribute stores a GPU offset only.
        // T is used solely to copy its bytes; no CPU alias is constructed.
        unsafe {
            (self.gl.gen_buffers)(1, &mut buffer);
            if buffer == 0 {
                return Err(RenderError::RenderDevice(
                    "attribute allocation failed".into(),
                ));
            }
            (self.gl.bind_buffer)(ARRAY_BUFFER, buffer);
            (self.gl.buffer_data)(
                ARRAY_BUFFER,
                std::mem::size_of_val(values) as isize,
                values.as_ptr().cast(),
                STATIC_DRAW,
            );
            (self.gl.enable_attrib)(slot);
            (self.gl.attrib_pointer)(slot, width, format, u8::from(normalized), 0, ptr::null());
        }
        Ok(buffer)
    }

    pub(super) fn check_draw(&self) -> Result<(), RenderError> {
        if self.exhaustive_draw_checks {
            self.check()
        } else {
            Ok(())
        }
    }

    pub(super) fn check(&self) -> Result<(), RenderError> {
        // SAFETY: The constructor's context/lifetime contract still holds; this
        // query passes no pointers and changes only GL error state.
        match unsafe { (self.gl.get_error)() } {
            0 => Ok(()),
            0x0507 => Err(RenderError::ContextLost),
            error => Err(RenderError::RenderDevice(format!(
                "GLES error 0x{error:04x}"
            ))),
        }
    }

    pub(super) fn shader(&self, kind: u32, source: &str) -> Result<u32, RenderError> {
        let length = i32::try_from(source.len())
            .map_err(|_| RenderError::RenderDevice("shader source too large".into()))?;
        let pointer = source.as_ptr().cast();
        // SAFETY: The context is current. shaderSource copies exactly length
        // live source bytes synchronously. Status writes to exclusive local
        // storage. Failed shader handles are deleted before returning.
        unsafe {
            let shader = (self.gl.create_shader)(kind);
            if shader == 0 {
                return Err(RenderError::RenderDevice("shader allocation failed".into()));
            }

            (self.gl.shader_source)(shader, 1, &pointer, &length);
            (self.gl.compile_shader)(shader);
            let mut status = 0;
            (self.gl.shader_iv)(shader, COMPILE_STATUS, &mut status);
            if status == 0 {
                let log = self.log(shader, true);
                (self.gl.delete_shader)(shader);
                return Err(RenderError::RenderDevice(log));
            }

            Ok(shader)
        }
    }

    pub(super) fn log(&self, id: u32, shader: bool) -> String {
        let mut length = 0;
        let mut written = 0;
        // SAFETY: id is a live shader/program from this context. Queries write
        // only their scalar result. Log output is bounded by the allocated
        // buffer capacity and is not retained by GL.
        unsafe {
            if shader {
                (self.gl.shader_iv)(id, INFO_LOG_LENGTH, &mut length);
            } else {
                (self.gl.program_iv)(id, INFO_LOG_LENGTH, &mut length);
            }
            let mut bytes = vec![0; length.clamp(1, 65536) as usize];
            if shader {
                (self.gl.shader_log)(
                    id,
                    bytes.len() as i32,
                    &mut written,
                    bytes.as_mut_ptr().cast(),
                );
            } else {
                (self.gl.program_log)(
                    id,
                    bytes.len() as i32,
                    &mut written,
                    bytes.as_mut_ptr().cast(),
                );
            }

            let count = (written.max(0) as usize).min(bytes.len());
            String::from_utf8_lossy(&bytes[..count]).into_owned()
        }
    }
}
