use super::sphere_patch_bounds;
use ipp_core::systems::surface::{SphereSurface, Surface};

#[test]
fn sphere_patch_bounds_enclose_the_published_chart_for_both_curvature_signs() {
    for curvature in [1e-12, -1e-12, 0.45, -0.45, 1.3, -1.3] {
        let sphere = SphereSurface {
            width: 4.0,
            height: 2.0,
            curvature,
            layer_spacing: 0.0,
        };

        for (x, y) in [
            ([-2.0, 2.0], [-1.0, 1.0]),
            ([-1.8, -1.0], [-0.9, -0.89]),
            ([1.0, 1.8], [-0.9, 0.75]),
            ([-0.01, 0.01], [0.0, 1.0]),
            ([-1.3, 1.6], [0.7, 0.7]),
            ([0.0, 0.0], [0.0, 0.0]),
        ] {
            let bounds = sphere_patch_bounds(x, y, f64::from(curvature));
            for i in 0..=24 {
                for j in 0..=24 {
                    let u = x[0] + (x[1] - x[0]) * f64::from(i) / 24.0;
                    let v = y[0] + (y[1] - y[0]) * f64::from(j) / 24.0;
                    let point = sphere.sample([2.0 + u, 1.0 - v], 0.0).unwrap().position;
                    for (value, [lo, hi]) in point.into_iter().zip(bounds) {
                        assert!(value >= lo - 1e-12 && value <= hi + 1e-12);
                    }
                }
            }
        }
    }
}
