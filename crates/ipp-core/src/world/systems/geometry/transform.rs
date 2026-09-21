use super::{GeometryPlane, GeometryRay, dot};
use crate::ErrorReason;

/// Validated invertible affine transform, retaining shear and f64 intermediates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryShapeTransform {
    matrix: [f64; 16],
    inverse: [f64; 16],
}

impl Default for GeometryShapeTransform {
    fn default() -> Self {
        let matrix = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        Self {
            matrix,
            inverse: matrix,
        }
    }
}

impl GeometryShapeTransform {
    /// The caller constructs this pair from validated TRS or affine composition.
    /// Authored arbitrary matrices still go through the general checked inverse.
    pub(crate) fn from_inverse_pair(matrix: [f64; 16], inverse: [f64; 16]) -> Self {
        Self {
            matrix,
            inverse,
        }
    }

    /// Accept a full affine matrix, including a composed skeletal transform.
    // Indexing keeps row/column elimination explicit.
    #[allow(clippy::needless_range_loop)]
    pub fn new(matrix: [f64; 16]) -> Result<Self, ErrorReason> {
        for value in matrix {
            if !value.is_finite() {
                return Err(ErrorReason::InvalidGeometry);
            }
        }
        if [matrix[3], matrix[7], matrix[11], matrix[15]] != [0.0, 0.0, 0.0, 1.0] {
            return Err(ErrorReason::InvalidGeometry);
        }
        let mut rows = [
            [matrix[0], matrix[4], matrix[8], 1.0, 0.0, 0.0],
            [matrix[1], matrix[5], matrix[9], 0.0, 1.0, 0.0],
            [matrix[2], matrix[6], matrix[10], 0.0, 0.0, 1.0],
        ];
        for col in 0..3 {
            let mut pivot = col;
            for row in col + 1..3 {
                if rows[row][col].abs() > rows[pivot][col].abs() {
                    pivot = row;
                }
            }
            if rows[pivot][col] == 0.0 {
                return Err(ErrorReason::InvalidGeometry);
            }
            rows.swap(col, pivot);
            let divisor = rows[col][col];
            for value in &mut rows[col] {
                *value /= divisor;
            }

            let pivot_row = rows[col];
            for row in 0..3 {
                if row == col {
                    continue;
                }
                let factor = rows[row][col];
                for entry in 0..6 {
                    rows[row][entry] -= factor * pivot_row[entry];
                }
            }
        }
        let mut inverse = Self::default().matrix;
        for row in 0..3 {
            for col in 0..3 {
                inverse[col * 4 + row] = rows[row][col + 3];
            }
            inverse[12 + row] = -(rows[row][3] * matrix[12]
                + rows[row][4] * matrix[13]
                + rows[row][5] * matrix[14]);
        }
        for value in inverse {
            if !value.is_finite() {
                return Err(ErrorReason::InvalidGeometry);
            }
        }
        Ok(Self {
            matrix,
            inverse,
        })
    }

    /// Promote a renderer or skeleton matrix before geometric operations.
    pub fn from_matrix(matrix: [f32; 16]) -> Result<Self, ErrorReason> {
        let mut promoted = [0.0; 16];
        for i in 0..16 {
            promoted[i] = f64::from(matrix[i]);
        }
        Self::new(promoted)
    }

    /// Column-major local-to-world matrix.
    pub fn matrix(&self) -> [f64; 16] {
        self.matrix
    }

    /// Borrow the cached matrix without copying its inverse or matrix storage.
    pub(crate) fn matrix_ref(&self) -> &[f64; 16] {
        &self.matrix
    }

    /// Scale the cached inverse transpose before f32 conversion. Normalization
    /// in the consumer removes the common positive scale, including under shear.
    pub(crate) fn render_normal_matrix(&self) -> Result<[f32; 16], ErrorReason> {
        let mut magnitude = 0.0_f64;
        for column in 0..3 {
            for row in 0..3 {
                magnitude = magnitude.max(self.inverse[column * 4 + row].abs());
            }
        }
        if magnitude == 0.0 || !magnitude.is_finite() {
            return Err(ErrorReason::InvalidValue);
        }
        let mut result = [0.0; 16];
        for column in 0..3 {
            for row in 0..3 {
                let value = self.inverse[row * 4 + column] / magnitude;
                let stored = value as f32;
                if value != 0.0 && stored == 0.0 {
                    return Err(ErrorReason::InvalidValue);
                }
                result[column * 4 + row] = stored;
            }
        }
        Ok(result)
    }

    /// World-to-local matrix, retaining the full affine inverse.
    pub fn inverse_matrix(&self) -> [f64; 16] {
        self.inverse
    }

    /// Transform a local direction without translation.
    pub fn vector(&self, vector: [f64; 3]) -> [f64; 3] {
        transform_vector(&self.matrix, vector)
    }

    /// Pull a World direction into local space without translation.
    pub fn inverse_vector(&self, vector: [f64; 3]) -> [f64; 3] {
        transform_vector(&self.inverse, vector)
    }

    /// Convert only at the rendering boundary and reject unrepresentable values.
    #[allow(clippy::needless_range_loop)]
    pub fn render_matrix(&self) -> Result<[f32; 16], ErrorReason> {
        let mut matrix = [0.0; 16];
        for i in 0..16 {
            matrix[i] = self.matrix[i] as f32;
            if !matrix[i].is_finite() {
                return Err(ErrorReason::InvalidGeometry);
            }
        }
        Ok(matrix)
    }

    /// Apply this transform followed by outer, composing their cached inverses.
    pub fn then(&self, outer: &Self) -> Result<Self, ErrorReason> {
        let matrix = multiply(outer.matrix, self.matrix);
        let inverse = multiply(self.inverse, outer.inverse);
        for i in 0..16 {
            if !matrix[i].is_finite() || !inverse[i].is_finite() {
                return Err(ErrorReason::InvalidGeometry);
            }
        }
        Ok(Self {
            matrix,
            inverse,
        })
    }

    /// Transform a local point, including translation.
    #[inline]
    pub fn point(&self, point: [f64; 3]) -> [f64; 3] {
        transform_point(&self.matrix, point)
    }

    /// Transform a world point back into this shape's local space.
    #[inline]
    pub fn inverse_point(&self, point: [f64; 3]) -> [f64; 3] {
        transform_point(&self.inverse, point)
    }

    /// Pull back a ray without normalizing its direction or changing its parameter.
    #[inline]
    pub fn inverse_ray(&self, ray: &GeometryRay) -> GeometryRay {
        GeometryRay {
            origin: transform_point(&self.inverse, ray.origin),
            direction: transform_vector(&self.inverse, ray.direction),
        }
    }

    /// GeometryPlane pullback M^T preserves signed support values without normalization.
    #[inline]
    pub fn local_plane(&self, plane: &GeometryPlane) -> GeometryPlane {
        let m = &self.matrix;
        let [x, y, z] = plane.normal;
        GeometryPlane {
            normal: [
                m[0] * x + m[1] * y + m[2] * z,
                m[4] * x + m[5] * y + m[6] * z,
                m[8] * x + m[9] * y + m[10] * z,
            ],
            offset: plane.offset + m[12] * x + m[13] * y + m[14] * z,
        }
    }

    /// Upper bound on the largest singular value, tight for orthogonal axes.
    pub fn maximum_stretch(&self) -> f64 {
        let m = &self.matrix;
        let x = [m[0], m[1], m[2]];
        let y = [m[4], m[5], m[6]];
        let z = [m[8], m[9], m[10]];
        let xy = dot(x, y).abs();
        let xz = dot(x, z).abs();
        let yz = dot(y, z).abs();
        let row_x = dot(x, x) + xy + xz;
        let row_y = dot(y, y) + xy + yz;
        let row_z = dot(z, z) + xz + yz;
        row_x.max(row_y).max(row_z).sqrt()
    }
}

#[inline]
fn transform_vector(m: &[f64; 16], [x, y, z]: [f64; 3]) -> [f64; 3] {
    [
        m[0] * x + m[4] * y + m[8] * z,
        m[1] * x + m[5] * y + m[9] * z,
        m[2] * x + m[6] * y + m[10] * z,
    ]
}

#[inline]
fn transform_point(m: &[f64; 16], [x, y, z]: [f64; 3]) -> [f64; 3] {
    [
        m[0] * x + m[4] * y + m[8] * z + m[12],
        m[1] * x + m[5] * y + m[9] * z + m[13],
        m[2] * x + m[6] * y + m[10] * z + m[14],
    ]
}

pub(in crate::world) fn multiply(a: [f64; 16], b: [f64; 16]) -> [f64; 16] {
    let mut result = [0.0; 16];
    for col in 0..4 {
        let base = col * 4;
        for row in 0..4 {
            result[base + row] = a[row] * b[base]
                + a[4 + row] * b[base + 1]
                + a[8 + row] * b[base + 2]
                + a[12 + row] * b[base + 3];
        }
    }
    result
}

#[cfg(test)]
#[path = "normal_matrix_tests.rs"]
mod tests;
