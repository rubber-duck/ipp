use super::*;
use ipp_core::{CylinderSurface, SphereSurface};

fn viewport(width: u32, height: u32) -> WorldViewport {
    WorldViewport {
        width,
        height,
        device_pixel_ratio: 1.0,
    }
}

fn orthographic() -> [f32; 16] {
    [
        0.5, 0.0, 0.0, 0.0, 0.0, -0.5, 0.0, 0.0, 0.0, 0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

#[test]
fn device_pixels_and_content_aspect_determine_resolution() {
    let demand = plane_demand([3.8, 2.4], orthographic(), viewport(320, 320)).unwrap();
    assert_eq!(image_size(demand, 1.0, 2048, None), Some([304, 192]));
    let high_dpr = plane_demand([3.8, 2.4], orthographic(), viewport(640, 640)).unwrap();
    assert_eq!(image_size(high_dpr, 1.0, 2048, None), Some([608, 384]));
    assert_eq!(image_size(high_dpr, 0.5, 2048, None), Some([304, 192]));
    assert_eq!(
        image_size([8000.0, 4000.0], 1.0, 4096, None),
        Some([2048, 1024])
    );
    assert_eq!(
        image_size([8000.0, 4000.0], 1.0, 512, None),
        Some([512, 256])
    );
}

#[test]
fn curvature_and_separated_shells_raise_local_demand() {
    let cylinder = CylinderSurface {
        width: 4.0,
        height: 3.0,
        curvature: 0.4,
        layer_spacing: 0.0,
    };
    let plain = surface_demand(&cylinder, &[0.0], orthographic(), viewport(400, 400)).unwrap();
    let separated =
        surface_demand(&cylinder, &[0.0, 1.0], orthographic(), viewport(400, 400)).unwrap();
    assert!(plain[0] >= 399.0 && plain[0] <= 401.0);
    assert!(separated[0] > plain[0] * 1.3);
    let sphere = SphereSurface {
        width: 4.0,
        height: 3.0,
        curvature: 0.4,
        layer_spacing: 0.0,
    };
    let sphere_demand =
        surface_demand(&sphere, &[0.0, 1.0], orthographic(), viewport(400, 400)).unwrap();
    assert!(sphere_demand[0] > plain[0] * 1.3);
    let capped = surface_demand(&cylinder, &[0.0; 33], orthographic(), viewport(400, 400)).unwrap();
    assert_eq!(image_size(capped, 1.0, 2048, None), Some([2048, 1536]));
}

#[test]
fn near_plane_crossing_is_bounded_and_offscreen_clipping_keeps_density() {
    let mut mvp = orthographic();
    mvp[2] = 1.0;
    mvp[14] = -2.0;
    assert_eq!(
        image_size(
            plane_demand([4.0, 2.0], mvp, viewport(400, 400)).unwrap(),
            1.0,
            2048,
            None
        ),
        Some([2048, 1024])
    );
    let mut offscreen = orthographic();
    offscreen[12] = 1.8;
    assert_eq!(
        plane_demand([4.0, 2.0], offscreen, viewport(400, 400)).unwrap(),
        plane_demand([4.0, 2.0], orthographic(), viewport(400, 400)).unwrap()
    );
}

#[test]
fn selected_size_has_growth_and_shrink_hysteresis() {
    let previous = Some([320, 160]);
    for width in [305.0, 319.0, 320.0] {
        assert_eq!(
            image_size([width, width / 2.0], 1.0, 2048, previous),
            previous
        );
    }
    assert_eq!(
        image_size([350.0, 175.0], 1.0, 2048, previous),
        Some([352, 176])
    );
    assert_eq!(
        image_size([230.0, 115.0], 1.0, 2048, previous),
        Some([232, 116])
    );
    assert_eq!(
        image_size([320.0, 240.0], 1.0, 2048, previous),
        Some([320, 240])
    );
}

#[test]
fn diagonal_stretch_requires_the_largest_singular_value() {
    let matrix = [[1.0, 1.0], [0.0, 1.0]];
    assert!((spectral_norm(matrix) - 1.618033988749895).abs() < 1e-12);
    assert!(spectral_norm(matrix) > 2.0_f64.sqrt());
    let mut mvp = orthographic();
    mvp[4] = 0.5;
    let demand = plane_demand([4.0, 2.0], mvp, viewport(400, 400)).unwrap();
    assert!((demand[0] - 400.0 * 1.618033988749895).abs() < 1e-3);
    assert_eq!(
        image_size([321.0, 160.5], 1.0, 2048, Some([320, 160])),
        Some([328, 164])
    );
}

#[test]
fn perspective_triangle_bound_covers_independent_interior_derivatives() {
    let mut matrix = orthographic();
    matrix[3] = 0.1;
    let positions = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [0.0, 2.0, 0.0],
        [2.0, 2.0, 0.0],
    ];
    let bound =
        grid_demand(&positions, 1, [2.0, 2.0], matrix, viewport(400, 400)).unwrap()[0] / 2.0;
    for y in 0..=20 {
        for x in 0..=20 {
            let (u, v) = (x as f64 / 10.0, y as f64 / 10.0);
            let w = 1.0 + u * f64::from(matrix[3]);
            let jacobian = [
                [100.0 / w.powi(2), 0.0],
                [100.0 * v * f64::from(matrix[3]) / w.powi(2), -100.0 / w],
            ];
            assert!(spectral_norm(jacobian) <= bound + 1e-9);
        }
    }
}
