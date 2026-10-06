//! Shared right-handed affine and matrix mathematics for CPU and GL consumers.

use crate::{ErrorReason, components::Transform, components::schema::ComponentLifecycle};

/// Column-major matrix multiplication acting on column vectors.
pub fn multiply(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    std::array::from_fn(|i| {
        let (column, row) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + row] * b[column * 4 + k]).sum()
    })
}

/// Convert authored TRS to a column-major model matrix, normalizing the quaternion.
pub fn model_matrix(transform: &Transform) -> Result<[f32; 16], ErrorReason> {
    matrix_f32(CameraAffineTransform::new(transform)?.matrix())
}

/// Inverse authored model transform for mesh-to-skeleton space conversion.
pub fn inverse_model_matrix(transform: &Transform) -> Result<[f32; 16], ErrorReason> {
    matrix_f32(CameraAffineTransform::new(transform)?.inverse_matrix())
}

// Finite f32 coefficients alone do not establish a usable projection: extreme
// valid TRS/extents can underflow its axes to zero during target conversion.
pub(crate) fn invertible(matrix: [f32; 16]) -> bool {
    let mut rows: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| f64::from(matrix[column * 4 + row]))
    });
    for column in 0..4 {
        let pivot = (column..4)
            .max_by(|&a, &b| rows[a][column].abs().total_cmp(&rows[b][column].abs()))
            .expect("remaining row");
        if rows[pivot][column] == 0.0 {
            return false;
        }
        rows.swap(column, pivot);
        let pivot_row = rows[column];
        for row in rows.iter_mut().skip(column + 1) {
            let factor = row[column] / pivot_row[column];
            for (value, pivot_value) in row.iter_mut().zip(pivot_row).skip(column + 1) {
                *value -= factor * pivot_value;
            }
        }
    }
    true
}

pub(crate) fn matrix_f32(matrix: [f64; 16]) -> Result<[f32; 16], ErrorReason> {
    let matrix = matrix.map(|value| value as f32);
    if matrix.iter().all(|value| value.is_finite()) {
        Ok(matrix)
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

/// f64 intermediates avoid overflow and underflow for finite authored f32 TRS.
pub(crate) struct CameraAffineTransform {
    rotation: [[f64; 3]; 3],
    scale: [f64; 3],
    translation: [f64; 3],
}

impl CameraAffineTransform {
    pub(crate) fn new(t: &Transform) -> Result<Self, ErrorReason> {
        t.validate()?;
        let q = [t.qx, t.qy, t.qz, t.qw].map(f64::from);
        let length = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        let [x, y, z, w] = q.map(|v| v / length);
        Ok(Self {
            rotation: [
                [
                    1.0 - 2.0 * (y * y + z * z),
                    2.0 * (x * y + z * w),
                    2.0 * (x * z - y * w),
                ],
                [
                    2.0 * (x * y - z * w),
                    1.0 - 2.0 * (x * x + z * z),
                    2.0 * (y * z + x * w),
                ],
                [
                    2.0 * (x * z + y * w),
                    2.0 * (y * z - x * w),
                    1.0 - 2.0 * (x * x + y * y),
                ],
            ],
            scale: [t.sx, t.sy, t.sz].map(f64::from),
            translation: [t.x, t.y, t.z].map(f64::from),
        })
    }

    pub(crate) fn point(&self, point: [f64; 3]) -> [f64; 3] {
        let vector = self.vector(point);
        std::array::from_fn(|i| vector[i] + self.translation[i])
    }

    pub(crate) fn vector(&self, vector: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|row| {
            (0..3)
                .map(|column| self.rotation[column][row] * self.scale[column] * vector[column])
                .sum()
        })
    }

    pub(crate) fn matrix(&self) -> [f64; 16] {
        let mut matrix = [0.0; 16];
        for column in 0..3 {
            for row in 0..3 {
                matrix[column * 4 + row] = self.rotation[column][row] * self.scale[column];
            }
            matrix[12 + column] = self.translation[column];
        }
        matrix[15] = 1.0;
        matrix
    }

    pub(crate) fn inverse_matrix(&self) -> [f64; 16] {
        let mut matrix = [0.0; 16];
        for row in 0..3 {
            for column in 0..3 {
                matrix[column * 4 + row] = self.rotation[row][column] / self.scale[row];
            }
            matrix[12 + row] = -(0..3)
                .map(|i| self.rotation[row][i] * self.translation[i])
                .sum::<f64>()
                / self.scale[row];
        }
        matrix[15] = 1.0;
        matrix
    }
}

#[cfg(test)]
#[path = "math_tests.rs"]
mod tests;
