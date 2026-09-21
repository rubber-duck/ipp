use super::Camera;
use super::{CameraAffineTransform, CameraMotion};
use crate::{EntityId, ErrorReason, components::Transform};

/// Calculate from producer inputs only; the caller resolves state_overlays on the staged result.
pub(crate) fn navigate(
    entity: EntityId,
    camera: Camera,
    transform: Transform,
    motion: CameraMotion,
) -> Result<(Camera, Transform), ErrorReason> {
    super::prepare(entity, &camera, &transform, 1, 1)?;

    let affine = CameraAffineTransform::new(&transform)?;
    let pivot = affine.point([0.0, 0.0, -f64::from(camera.focus_distance)]);
    let mut next_camera = camera;
    let mut next_transform = transform;
    match motion {
        CameraMotion::Rotate {
            yaw,
            pitch,
        } => {
            if !yaw.is_finite() || !pitch.is_finite() {
                return Err(ErrorReason::InvalidValue);
            }
            if yaw == 0.0 && pitch == 0.0 {
                return Ok((camera, transform));
            }

            let q = [transform.qx, transform.qy, transform.qz, transform.qw].map(f64::from);
            let length = q.iter().map(|value| value * value).sum::<f64>().sqrt();
            let (sy, cy) = (f64::from(yaw) * 0.5).sin_cos();
            let (sx, cx) = (f64::from(pitch) * 0.5).sin_cos();
            let rotation = quaternion_product(
                quaternion_product(q.map(|value| value / length), [0.0, sy, 0.0, cy]),
                [sx, 0.0, 0.0, cx],
            );
            [
                next_transform.qx,
                next_transform.qy,
                next_transform.qz,
                next_transform.qw,
            ] = rotation.map(|value| value as f32);

            // Use the rounded quaternion that will actually be committed, with
            // authored scale, so its local -Z focus remains at the old world pivot.
            preserve_pivot(pivot, next_camera.focus_distance, &mut next_transform)?;
        }
        CameraMotion::Pan {
            x,
            y,
            width,
            height,
        } => {
            if !x.is_finite() || !y.is_finite() || width == 0 || height == 0 {
                return Err(ErrorReason::InvalidValue);
            }
            super::prepare(entity, &camera, &transform, width, height)?;

            let extent = if camera.projection == 0 {
                2.0 * f64::from(camera.focus_distance) * (f64::from(camera.fov_y) * 0.5).tan()
            } else {
                f64::from(camera.ortho_height)
            };
            let local = [
                -f64::from(x) * f64::from(width) / f64::from(height) * extent,
                f64::from(y) * extent,
                0.0,
            ];
            set_position(&mut next_transform, affine.point(local))?;
            super::prepare(entity, &next_camera, &next_transform, width, height)?;
        }
        CameraMotion::Zoom {
            amount,
        } => {
            if !amount.is_finite() {
                return Err(ErrorReason::InvalidValue);
            }
            if amount == 0.0 {
                return Ok((camera, transform));
            }

            // Apply the submitted logarithmic delta exactly; unrepresentable
            // results reject with the rest of the staged component update.
            let factor = f64::from(amount).exp();
            if camera.projection == 0 {
                next_camera.focus_distance = (f64::from(camera.focus_distance) * factor) as f32;
                preserve_pivot(pivot, next_camera.focus_distance, &mut next_transform)?;
            } else {
                next_camera.ortho_height = (f64::from(camera.ortho_height) * factor) as f32;
            }
        }
    }

    super::prepare(entity, &next_camera, &next_transform, 1, 1)?;
    Ok((next_camera, next_transform))
}

fn quaternion_product(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn preserve_pivot(
    pivot: [f64; 3],
    focus_distance: f32,
    transform: &mut Transform,
) -> Result<(), ErrorReason> {
    let offset =
        CameraAffineTransform::new(transform)?.vector([0.0, 0.0, -f64::from(focus_distance)]);
    set_position(transform, std::array::from_fn(|i| pivot[i] - offset[i]))
}

fn set_position(transform: &mut Transform, position: [f64; 3]) -> Result<(), ErrorReason> {
    let position = position.map(|value| value as f32);
    if position.iter().any(|value| !value.is_finite()) {
        return Err(ErrorReason::InvalidValue);
    }

    [transform.x, transform.y, transform.z] = position;
    Ok(())
}
