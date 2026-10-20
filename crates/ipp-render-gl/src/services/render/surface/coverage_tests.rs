use super::*;

fn identity() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

#[test]
fn coverage_clips_to_screen_and_counts_both_faces_without_occlusion() {
    let mut mvp = identity();
    assert_eq!(plane([-1.0, -1.0, 1.0, 1.0], mvp), 1.0);
    assert_eq!(plane([-1.0, 0.0, 0.0, 1.0], mvp), 0.25);
    mvp[12] = 1.0;
    assert_eq!(plane([-1.0, -1.0, 1.0, 1.0], mvp), 0.5);
    mvp[12] = 3.0;
    assert_eq!(plane([-1.0, -1.0, 1.0, 1.0], mvp), 0.0);
    mvp[12] = 0.0;
    mvp[0] = -1.0;
    assert_eq!(plane([-1.0, -1.0, 1.0, 1.0], mvp), 1.0);
    mvp[15] = -1.0;
    assert_eq!(plane([-1.0, -1.0, 1.0, 1.0], mvp), 0.0);
}

#[test]
fn small_overlay_requests_its_pixel_footprint_and_grows_with_content() {
    let mvp = [
        2.0 / 2048.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / 2048.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -1.0,
        1.0,
        0.0,
        1.0,
    ];
    let viewport = ipp_core::WorldViewport {
        width: 2048,
        height: 2048,
        device_pixel_ratio: 1.0,
    };
    for side in [512, 2048] {
        let extent = [side as f32; 2];
        let demand = super::super::quality::plane_demand(extent, mvp, viewport).unwrap();
        assert_eq!(
            super::super::quality::image_size(demand, 1.0, 2048, None),
            Some([side; 2])
        );
        assert_eq!(
            plane([0.0, 0.0, extent[0], extent[1]], mvp),
            (f64::from(side) / 2048.0).powi(2)
        );
    }
}

#[test]
fn separated_shells_do_not_multiply_screen_area() {
    let flat = ipp_core::FlatSurface {
        width: 2.0,
        height: 2.0,
        ..Default::default()
    };
    assert_eq!(surface(&flat, &[0.0, 0.1, 0.2], identity()).unwrap(), 1.0);
    let curved = ipp_core::CylinderSurface {
        width: 2.0,
        height: 2.0,
        curvature: 0.2,
        layer_spacing: 0.0,
    };
    let one = surface(&curved, &[0.0], identity()).unwrap();
    assert!(one > 0.9 && one < 1.0);
    assert_eq!(surface(&curved, &[0.0, 0.0], identity()).unwrap(), one);
}
