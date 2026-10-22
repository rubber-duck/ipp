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
fn analytic_bounds_include_curvature_and_separated_shells() {
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
    assert!(separated[0] > plain[0] * 1.2);
    let sphere = SphereSurface {
        width: 4.0,
        height: 3.0,
        curvature: 0.4,
        layer_spacing: 0.0,
    };
    let sphere_demand =
        surface_demand(&sphere, &[0.0, 1.0], orthographic(), viewport(400, 400)).unwrap();
    assert!(sphere_demand[0] > plain[0] * 1.3);
    let repeated =
        surface_demand(&cylinder, &[0.0; 64], orthographic(), viewport(400, 400)).unwrap();
    assert_eq!(repeated, plain);
}

#[test]
fn clipping_keeps_visible_density_without_reserving_offscreen_pixels() {
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
        Some([400, 200])
    );
    let mut offscreen = orthographic();
    offscreen[12] = -1.8;
    assert_eq!(
        plane_demand([4.0, 2.0], offscreen, viewport(400, 400)).unwrap(),
        plane_demand([4.0, 2.0], orthographic(), viewport(400, 400)).unwrap()
    );
    offscreen[12] = 1.8;
    assert_eq!(
        plane_demand([4.0, 2.0], offscreen, viewport(400, 400)).unwrap(),
        [1.0; 2]
    );
}

#[test]
fn perspective_bounds_preserve_full_pane_density_but_limit_allocation() {
    let mut mvp = orthographic();
    mvp[3] = -0.2;
    let demand = plane_demand([4.0, 2.0], mvp, viewport(400, 400)).unwrap();
    // The far corner has W=0.2: the full bounds span 2000 x 1000 pixels.
    // Coverage is clipped separately; texture dimensions still describe the
    // entire content rectangle, and memory/device policy caps the image.
    assert!((demand[0] - 2000.0).abs() < 0.01, "{demand:?}");
    assert_eq!(image_size(demand, 1.0, 512, None), Some([512, 256]));
}

#[test]
fn eye_plane_crossing_is_bounded_and_invalid_projection_is_rejected() {
    let mut mvp = orthographic();
    mvp[3] = -0.5;
    let demand = plane_demand([4.0, 2.0], mvp, viewport(400, 400)).unwrap();
    assert_eq!(image_size(demand, 1.0, 2048, None), Some([2048, 1024]));
    mvp[0] = f32::NAN;
    assert!(plane_demand([4.0, 2.0], mvp, viewport(400, 400)).is_err());
    assert!(plane_demand([4.0, 2.0], orthographic(), viewport(0, 400)).is_err());
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
fn small_continuous_changes_reuse_active_quality() {
    let mut size = image_size([320.0, 160.0], 1.0, 2048, None).unwrap();
    let mut changes = 0;
    for step in 1..=150 {
        let width = 320.0 + f64::from(step) * 0.2;
        let next = image_size([width, width / 2.0], 1.0, 2048, Some(size)).unwrap();
        changes += usize::from(next != size);
        assert!(f64::from(next[0]) >= width - 1e-3);
        size = next;
    }
    assert_eq!(changes, 1);
    assert_eq!(size, [352, 176]);
    assert_eq!(
        image_size([2049.0, 1024.5], 1.0, 2048, Some([2048, 1024])),
        Some([2048, 1024])
    );
}

#[test]
fn shear_uses_projected_enclosure_instead_of_local_triangle_derivatives() {
    let mut mvp = orthographic();
    mvp[4] = 0.5;
    let demand = plane_demand([4.0, 2.0], mvp, viewport(400, 400)).unwrap();
    assert_eq!(demand, [600.0, 300.0]);
}
