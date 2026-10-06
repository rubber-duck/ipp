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
