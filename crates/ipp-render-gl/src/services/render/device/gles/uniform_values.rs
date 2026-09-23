//! Last values this device uploaded to a program's per-draw uniforms.
//!
//! Uniform values belong to their program object and persist across draws,
//! frames and target changes. Only this device sets them and programs are never
//! relinked, so an identical value need not be sent again; the record lives
//! and dies with its program. Values compare bitwise, so every distinct value,
//! including a signed zero, is still uploaded.

use super::{GlesRenderDevice, GlesRenderProgram};

#[derive(Clone, Copy, PartialEq, Eq)]
enum GlesUniformValue {
    Int(i32),
    #[cfg(feature = "surfaces")]
    Float(u32),
    #[cfg(feature = "surfaces")]
    Vec4([u32; 4]),
    #[cfg(feature = "surfaces")]
    Mat4([u32; 16]),
}

/// Uploaded values by uniform location, for the few uniforms a program uses.
#[derive(Default)]
pub(super) struct GlesUniformValues {
    values: Vec<(i32, GlesUniformValue)>,
}

impl GlesUniformValues {
    /// Record `value` at `location`, returning whether it must be uploaded.
    /// Inactive locations (`-1`) never upload.
    fn replace(&mut self, location: i32, value: GlesUniformValue) -> bool {
        if location < 0 {
            return false;
        }
        match self.values.iter_mut().find(|(known, _)| *known == location) {
            Some((_, current)) if *current == value => false,
            Some((_, current)) => {
                *current = value;
                true
            }
            None => {
                self.values.push((location, value));
                true
            }
        }
    }
}

impl GlesRenderDevice {
    /// Set an integer or sampler uniform of the current `program` if it changed.
    pub(super) fn program_int(&self, program: &GlesRenderProgram, location: i32, value: i32) {
        let changed = program
            .values
            .borrow_mut()
            .replace(location, GlesUniformValue::Int(value));
        if changed {
            // SAFETY: The caller made `program` current in this context; the
            // location belongs to it and GL copies the scalar.
            unsafe { (self.gl.uniform_int)(location, value) };
        }
    }

    /// Set a float uniform of the current `program` if it changed.
    #[cfg(feature = "surfaces")]
    pub(super) fn program_float(&self, program: &GlesRenderProgram, location: i32, value: f32) {
        let changed = program
            .values
            .borrow_mut()
            .replace(location, GlesUniformValue::Float(value.to_bits()));
        if changed {
            // SAFETY: The caller made `program` current in this context; the
            // location belongs to it and GL copies the scalar.
            unsafe { (self.gl.uniform_float)(location, value) };
        }
    }

    /// Set a vec4 uniform of the current `program` if it changed.
    #[cfg(feature = "surfaces")]
    pub(super) fn program_vec4(
        &self,
        program: &GlesRenderProgram,
        location: i32,
        value: &[f32; 4],
    ) {
        let changed = program
            .values
            .borrow_mut()
            .replace(location, GlesUniformValue::Vec4(value.map(f32::to_bits)));
        if changed {
            // SAFETY: The caller made `program` current in this context; GL
            // copies the four live floats synchronously and keeps no pointer.
            unsafe { (self.gl.uniform_vec4)(location, 1, value.as_ptr()) };
        }
    }

    /// Set a mat4 uniform of the current `program` if it changed.
    #[cfg(feature = "surfaces")]
    pub(super) fn program_mat4(
        &self,
        program: &GlesRenderProgram,
        location: i32,
        value: &[f32; 16],
    ) {
        let changed = program
            .values
            .borrow_mut()
            .replace(location, GlesUniformValue::Mat4(value.map(f32::to_bits)));
        if changed {
            // SAFETY: The caller made `program` current in this context; GL
            // copies the sixteen live floats synchronously and keeps no pointer.
            unsafe { (self.gl.uniform_matrix)(location, 1, 0, value.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_values_upload_once_per_location() {
        let mut values = GlesUniformValues::default();
        assert!(values.replace(3, GlesUniformValue::Int(0)));
        assert!(!values.replace(3, GlesUniformValue::Int(0)));
        assert!(values.replace(4, GlesUniformValue::Int(0)));
        assert!(values.replace(3, GlesUniformValue::Int(1)));
        assert!(!values.replace(3, GlesUniformValue::Int(1)));
    }

    #[test]
    fn inactive_locations_never_upload() {
        let mut values = GlesUniformValues::default();
        assert!(!values.replace(-1, GlesUniformValue::Int(5)));
        assert!(values.values.is_empty());
    }

    #[cfg(feature = "surfaces")]
    #[test]
    fn values_compare_bitwise() {
        let mut values = GlesUniformValues::default();
        let vec4 = |value: [f32; 4]| GlesUniformValue::Vec4(value.map(f32::to_bits));
        assert!(values.replace(0, vec4([0.0; 4])));
        assert!(values.replace(0, vec4([-0.0, 0.0, 0.0, 0.0])));
        assert!(!values.replace(0, vec4([-0.0, 0.0, 0.0, 0.0])));
        let nan = vec4([f32::NAN, 0.0, 0.0, 0.0]);
        assert!(values.replace(0, nan));
        assert!(!values.replace(0, nan));
    }
}
