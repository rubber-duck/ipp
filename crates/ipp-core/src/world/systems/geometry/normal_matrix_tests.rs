use super::GeometryShapeTransform;
use crate::{components::Transform, systems::camera};

#[test]
fn normals_stay_perpendicular_under_nonuniform_scale_and_rotation() {
    let model = camera::model_matrix(&Transform {
        sx: 1.6,
        sy: 0.65,
        sz: 1.1,
        qy: 0.3f32.sin(),
        qw: 0.3f32.cos(),
        ..Transform::default()
    })
    .unwrap();
    let normal = GeometryShapeTransform::from_matrix(model)
        .unwrap()
        .render_normal_matrix()
        .unwrap();
    let transform = |matrix: [f32; 16], vector: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|row| {
            (0..3)
                .map(|column| matrix[column * 4 + row] * vector[column])
                .sum()
        })
    };
    let n = transform(normal, [1.0, 1.0, 1.0]);
    for tangent in [[1.0, -1.0, 0.0], [0.0, 1.0, -1.0]] {
        let t = transform(model, tangent);
        assert!(n.into_iter().zip(t).map(|(a, b)| a * b).sum::<f32>().abs() < 1e-6);
    }
    // Uniform magnitude disappears; tiny/huge valid models need no huge inverse.
    for scale in [1e-30, 1e30] {
        let mut model = [0.0; 16];
        model[15] = 1.0;
        for axis in 0..3 {
            model[axis * 5] = scale;
        }
        let normal = GeometryShapeTransform::from_matrix(model)
            .unwrap()
            .render_normal_matrix()
            .unwrap();
        assert_eq!([normal[0], normal[5], normal[10]], [1.0; 3]);
    }
    assert!(GeometryShapeTransform::from_matrix([0.0; 16]).is_err());
}

#[test]
fn cached_inverse_normals_preserve_orthogonality_under_shear_and_reflection() {
    for i in 0..128 {
        let t = f64::from(i) * 0.03;
        let matrix = [
            -1.0 - t,
            0.1,
            0.0,
            0.0,
            t.sin(),
            0.7 + t,
            0.2,
            0.0,
            0.1,
            t.cos(),
            1.3,
            0.0,
            3.0,
            2.0,
            -1.0,
            1.0,
        ];
        let transform = GeometryShapeTransform::new(matrix).unwrap();
        let normal = transform.render_normal_matrix().unwrap();
        for axis in 0..3 {
            let n: [f64; 3] = std::array::from_fn(|r| f64::from(normal[axis * 4 + r]));
            let length = n.iter().map(|x| x * x).sum::<f64>().sqrt();
            assert!(length > 0.0);
            for tangent in 0..3 {
                let dot = (0..3).map(|r| n[r] * matrix[tangent * 4 + r]).sum::<f64>();
                if tangent == axis {
                    assert!(dot > 0.0);
                } else {
                    assert!(dot.abs() < 1e-6 * length * (1.0 + t));
                }
            }
        }
    }
}
