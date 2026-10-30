use super::super::GlesRenderDevice;
use crate::RenderError;

pub(in super::super) struct GlesExternalImageCopy {
    pub(in super::super) program: u32,
    pub(in super::super) vao: u32,
}

impl GlesRenderDevice {
    pub(in super::super) fn prepare_external_image_copy(&mut self) -> Result<(), RenderError> {
        if self.external_image_copy.is_some() {
            return Ok(());
        }
        let vertex = self.shader(0x8B31, include_str!("copy.vert"))?;
        let fragment = match self.shader(0x8B30, include_str!("copy.frag")) {
            Ok(shader) => shader,
            Err(error) => {
                // SAFETY: This unpublished shader is owned by this context.
                unsafe { self.gl.delete_shader(vertex) };
                return Err(error);
            }
        };
        let mut vao = 0;
        // SAFETY: Exact GL calls on the owning current context; shader/program
        // names are private here, and all outputs are exclusive stack storage.
        let (program, linked) = unsafe {
            let program = self.gl.create_program();
            self.gl.attach_shader(program, vertex);
            self.gl.attach_shader(program, fragment);
            self.gl.link_program(program);
            self.gl.delete_shader(vertex);
            self.gl.delete_shader(fragment);
            let mut linked = 0;
            self.gl.program_iv(program, 0x8B82, &mut linked);
            self.gl.gen_vertex_arrays(1, &mut vao);
            (program, linked)
        };
        let result = if linked == 0 || vao == 0 {
            Err(RenderError::RenderDevice(format!(
                "External image copy program: {}",
                self.log(program, false)
            )))
        } else {
            self.check()
        };
        if let Err(error) = result {
            // SAFETY: Failed unpublished allocations remain exclusively owned.
            unsafe {
                self.gl.delete_program(program);
                self.gl.delete_vertex_arrays(1, &vao);
            }
            return Err(error);
        }
        self.external_image_copy = Some(GlesExternalImageCopy {
            program,
            vao,
        });
        Ok(())
    }

    pub(in super::super) fn release_external_image_copy(&mut self) {
        if let Some(copy) = self.external_image_copy.take() {
            // SAFETY: Device drop keeps its context current; these private names
            // have no borrowed external storage and are invalidated before delete.
            unsafe {
                self.gl.delete_program(copy.program);
                self.gl.delete_vertex_arrays(1, &copy.vao);
            }
        }
    }
}
