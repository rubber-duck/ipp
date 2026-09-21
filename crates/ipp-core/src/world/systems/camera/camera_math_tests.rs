use super::*;

fn point(matrix: [f32; 16], position: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|row| {
        (0..4)
            .map(|column| matrix[column * 4 + row] * position[column])
            .sum()
    })
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.00001, "{a} != {b}");
}

#[test]
fn trs_scales_then_rotates_then_translates_column_vectors() {
    let transform = Transform {
        x: 3.0,
        y: 4.0,
        z: 5.0,
        qz: 2.0,
        qw: 2.0,
        sx: 2.0,
        sy: 3.0,
        sz: 4.0,
        ..Transform::default()
    };
    let actual = point(model_matrix(&transform).unwrap(), [1.0, 0.0, 0.0, 1.0]);
    for (actual, expected) in actual.into_iter().zip([3.0, 6.0, 5.0, 1.0]) {
        close(actual, expected);
    }
}

#[test]
fn quaternion_normalization_handles_extreme_finite_magnitudes() {
    for magnitude in [f32::MAX, f32::MIN_POSITIVE, f32::from_bits(1)] {
        let transform = Transform {
            qw: magnitude,
            ..Transform::default()
        };
        assert_eq!(
            model_matrix(&transform),
            model_matrix(&Transform::default())
        );
    }
}

#[test]
fn projection_rejects_finite_parameters_that_collapse_target_matrix_axes() {
    let camera = Camera {
        projection: 1,
        ortho_height: f32::MAX,
        ..Camera::default()
    };
    let transform = Transform {
        sx: f32::MAX,
        sy: f32::MAX,
        ..Transform::default()
    };
    assert_eq!(
        prepare(EntityId::from_bits(1), &camera, &transform, 100, 100),
        Err(ErrorReason::InvalidValue)
    );
}

#[test]
fn shared_projection_matches_camera_pose_aspect_and_both_clipping_planes() {
    let transform = Transform {
        x: 2.0,
        y: 3.0,
        z: 5.0,
        qy: 0.5,
        qw: 0.5,
        sx: 2.0,
        sy: 3.0,
        sz: 4.0,
        ..Transform::default()
    };
    for projection in [0, 1] {
        let camera = Camera {
            projection,
            ..Camera::default()
        };
        let prepare = |width| {
            prepare(EntityId::from_bits(1), &camera, &transform, width, 100)
                .unwrap()
                .view_projection
        };
        let local_to_clip = multiply(prepare(100), model_matrix(&transform).unwrap());
        for (z, expected) in [(-camera.near, -1.0), (-camera.far, 1.0)] {
            let clip = point(local_to_clip, [0.0, 0.0, z, 1.0]);
            close(clip[0], 0.0);
            close(clip[1], 0.0);
            close(clip[2] / clip[3], expected);
        }
        let a = point(prepare(100), [1.0, 0.0, 0.0, 1.0]);
        let b = point(prepare(200), [1.0, 0.0, 0.0, 1.0]);
        close(a[0], b[0] * 2.0);
        close(a[1], b[1]);
        close(a[2], b[2]);
        close(a[3], b[3]);
    }
}
