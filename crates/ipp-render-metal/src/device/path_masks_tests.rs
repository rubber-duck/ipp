use super::*;
use ipp_core::services::asset_management::formats::{
    drawing::FillRule,
    quadratic::{QuadraticContour, QuadraticSegment},
};
use ipp_render_scene::paths::PathRecords;

fn outline(device: &Device, tip: [f32; 2]) -> Object {
    let contour = QuadraticContour {
        start: [1.25, 1.],
        segments: [[7., 1.5], tip]
            .map(|to| QuadraticSegment::Line {
                to,
            })
            .into(),
    };
    let path = PathRecords::contours(&[contour], FillRule::NonZero).unwrap();
    device.path(&path.records).unwrap()
}

#[test]
fn warm_path_pixels_follow_projection_page_and_tint() {
    objc2::rc::autoreleasepool(|_| {
        let device = Device::new().unwrap();
        assert!(device.resize(47, 31));
        let path = outline(&device, [3., 7.]);
        let other = outline(&device, [7., 6.]);

        let draw = |page: &Object, shift: f32, color: [f32; 4], plot: bool| {
            let points = [
                ([-0.9, 0.8], [0., 0.]),
                ([0.55, 0.8], [8., 0.]),
                ([-0.9, -0.8], [0., 8.]),
                ([0.55, -0.8], [8., 8.]),
            ];
            let vertices = points.map(|([x, y], local)| CanvasVertex {
                clip: [x + shift, y, 0., 1.],
                local,
                world: [0.; 4],
                right: [0.; 3],
                down: [0.; 3],
                normal: [0.; 3],
                slot: 0,
            });
            let paint = CanvasPaint {
                tint: color,
                clip: [1.25, 0., 8., 8.],
                options: [0., 3., 0., 0.],
                ..Default::default()
            };
            let mut frame = device.frame().unwrap();
            assert!(frame.pass(None, [0., 0., 0., 1.], None, None));
            assert!(frame.canvas(
                &vertices,
                &paint,
                Some(CanvasBinding::Path {
                    buffer: page,
                    start: 0
                }),
                None,
                CanvasCoverage {
                    analytic: None,
                    depth_test: true,
                    plot,
                    layer_owner: 0
                }
            ));
            let mut pixels = vec![0; 47 * 31 * 4];
            let (_, builds, _) = frame.finish(Some(&mut pixels), None).unwrap();
            (pixels, builds)
        };

        let red = [1., 0., 0., 1.];
        let blue = [0., 0., 1., 0.75];
        let (original, _) = draw(&path, 0., red, false);
        let (tinted, _) = draw(&path, 0., blue, false);
        let (moved, _) = draw(&path, 0.23, red, false);
        let (replaced, _) = draw(&other, 0.23, red, false);
        assert_ne!(original, tinted);
        assert_ne!(original, moved);
        assert_ne!(moved, replaced);

        for (page, shift, expected) in [
            (&path, 0., &original),
            (&path, 0.23, &moved),
            (&other, 0.23, &replaced),
        ] {
            for builds in [0, 1, 0] {
                let (pixels, actual) = draw(page, shift, red, true);
                assert_eq!(&pixels, expected);
                assert_eq!(actual, builds);
            }
            if shift == 0. {
                let (pixels, builds) = draw(page, shift, blue, true);
                assert_eq!(pixels, tinted);
                assert_eq!(builds, 0);
            }
        }
    });
}
