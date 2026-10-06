//! Camera projection for a viewport, also usable for spot shadow views.

use super::Camera;
use crate::math::{invertible, matrix_f32};
use crate::{EntityId, ErrorReason, components::Transform, components::schema::ComponentLifecycle};

/// Final effective camera prepared for a host-owned viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreparedCamera {
    /// Explicitly selected camera entity.
    pub entity: EntityId,
    /// Column-major projection * inverse(camera TRS), using GL clip space.
    pub view_projection: [f32; 16],
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
    prepare_affine_for_extent(
        entity,
        camera,
        affine,
        [f64::from(width), f64::from(height)],
    )
}

/// Projection extent is independent of raster resolution; nested views use physical Surface size.
pub fn prepare_affine_for_extent(
    entity: EntityId,
    camera: &Camera,
    affine: &crate::systems::geometry::GeometryShapeTransform,
    extent: [f64; 2],
) -> Result<PreparedCamera, ErrorReason> {
    camera.validate()?;
    let aspect = projection_aspect(extent)?;
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

pub(super) fn projection_aspect(extent: [f64; 2]) -> Result<f64, ErrorReason> {
    let aspect = extent[0] / extent[1];
    if extent.iter().all(|value| value.is_finite() && *value > 0.0)
        && aspect.is_finite()
        && aspect > 0.0
    {
        Ok(aspect)
    } else {
        Err(ErrorReason::InvalidViewport)
    }
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
