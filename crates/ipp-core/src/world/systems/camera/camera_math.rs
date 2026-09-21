use super::*;

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
#[cfg(feature = "skeletal-animation")]
pub fn inverse_model_matrix(transform: &Transform) -> Result<[f32; 16], ErrorReason> {
    matrix_f32(CameraAffineTransform::new(transform)?.inverse_matrix())
}

/// Prepare an explicit camera pose and projection, also usable for spot shadow views.
pub fn prepare(
    entity: EntityId,
    camera: &Camera,
    transform: &Transform,
    width: u32,
    height: u32,
) -> Result<PreparedCamera, ErrorReason> {
    prepare_affine(
        entity,
        camera,
        &crate::systems::hierarchy::affine(transform)?,
        width,
        height,
    )
}

/// Prepare a complete evaluated affine camera pose, preserving parent shear.
pub fn prepare_affine(
    entity: EntityId,
    camera: &Camera,
    affine: &crate::systems::geometry::GeometryShapeTransform,
    width: u32,
    height: u32,
) -> Result<PreparedCamera, ErrorReason> {
    camera.validate()?;
    if width == 0 || height == 0 {
        return Err(ErrorReason::InvalidValue);
    }
    let aspect = f64::from(width) / f64::from(height);
    let near = f64::from(camera.near);
    let far = f64::from(camera.far);
    let mut projection = [0.0; 16];
    if camera.projection == 0 {
        let f = 1.0 / (f64::from(camera.fov_y) * 0.5).tan();
        projection[0] = f / aspect;
        projection[5] = f;
        projection[10] = (far + near) / (near - far);
        projection[11] = -1.0;
        projection[14] = 2.0 * far * near / (near - far);
    } else {
        projection[0] = 2.0 / (f64::from(camera.ortho_height) * aspect);
        projection[5] = 2.0 / f64::from(camera.ortho_height);
        projection[10] = 2.0 / (near - far);
        projection[14] = (far + near) / (near - far);
        projection[15] = 1.0;
    }
    let inverse = affine.inverse_matrix();
    let product = std::array::from_fn(|i| {
        let (column, row) = (i / 4, i % 4);
        (0..4)
            .map(|k| projection[k * 4 + row] * inverse[column * 4 + k])
            .sum()
    });
    let view_projection = matrix_f32(product)?;
    if !invertible(view_projection) {
        return Err(ErrorReason::InvalidValue);
    }

    Ok(PreparedCamera {
        entity,
        view_projection,
    })
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

fn matrix_f32(matrix: [f64; 16]) -> Result<[f32; 16], ErrorReason> {
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
