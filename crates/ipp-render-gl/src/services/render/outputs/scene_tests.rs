use super::*;

#[test]
fn normal_composition_preserves_perpendicularity_under_shear_and_reflection() {
    let placement = GeometryShapeTransform::new([
        -2.0, 0.0, 0.0, 0.0, 0.7, 3.0, 0.0, 0.0, 0.4, 0.5, 4.0, 0.0, 5.0, 6.0, 7.0, 1.0,
    ])
    .unwrap();
    let identity = GeometryShapeTransform::default().render_matrix().unwrap();
    let normal = compose_normal(Ok(identity), placement).unwrap();
    for normal_axis in 0..3 {
        for tangent_axis in 0..3 {
            if normal_axis == tangent_axis {
                continue;
            }
            let tangent =
                placement.vector(std::array::from_fn(|axis| f64::from(axis == tangent_axis)));
            let dot: f64 = (0..3)
                .map(|axis| tangent[axis] * f64::from(normal[normal_axis * 4 + axis]))
                .sum();
            assert!(
                dot.abs() < 1e-6,
                "normal axis {normal_axis}, tangent axis {tangent_axis}: {dot}"
            );
        }
    }
    assert!(normal[0] < 0.0);
    assert_eq!(normal[12..], [0.0; 4]);
}

#[test]
fn model_composition_preserves_zero_scale_and_current_outer_translation() {
    let mut local = GeometryShapeTransform::default().render_matrix().unwrap();
    local[0] = 0.0;
    local[12] = 2.0;
    let mut outer = GeometryShapeTransform::default().matrix();
    outer[0] = -3.0;
    outer[12] = 8.0;
    let model = compose_model(local, GeometryShapeTransform::new(outer).unwrap()).unwrap();
    assert_eq!(model[0], 0.0);
    assert_eq!(model[12], 2.0);
}
