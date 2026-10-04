use super::*;
use crate::systems::geometry::{GeometryBounds, GeometryRay};

#[test]
fn flat_samples_inverse_bounds_and_affine_share_content_coordinates() {
    let surface = FlatSurface {
        width: 4.0,
        height: 2.0,
        layer_spacing: -0.5,
    };
    for content in [[0.0, 0.0], [4.0, 2.0], [1.3, 0.8]] {
        for offset in [-2.0, 0.0, 3.0] {
            let sample = surface.sample(content, offset).unwrap();
            let ray = GeometryRay {
                origin: [sample.position[0], sample.position[1], 8.0],
                direction: [0.0, 0.0, -2.0],
            };
            let hits = surface
                .ray_intersections(&ray, offset, SurfaceDomain::Content)
                .unwrap();
            assert_eq!(hits.len(), 1);
            assert!((hits[0].distance - (8.0 - offset) / 2.0).abs() < 1e-12);
            for (actual, expected) in hits[0].content.into_iter().zip(content) {
                assert!((actual - expected).abs() < 1e-12);
            }
            assert_eq!(hits[0].front_normal, sample.front_normal);
            let affine = surface.exact_affine(offset).unwrap();
            assert_eq!(
                [
                    affine[0] * content[0] + affine[4] * content[1] + affine[12],
                    affine[1] * content[0] + affine[5] * content[1] + affine[13],
                    affine[2] * content[0] + affine[6] * content[1] + affine[14]
                ],
                sample.position
            );
        }
    }
    assert_eq!(
        surface.bounds([-2.0, 3.0]).unwrap().bounds().unwrap(),
        [[-2.0, -1.0, -2.0], [2.0, 1.0, 3.0]]
    );
    assert_eq!(
        surface.approximation_error([0.0, 0.0, 4.0, 2.0], 3.0),
        Ok(0.0)
    );
}

#[test]
fn flat_continuation_and_invalid_geometry_are_explicit() {
    let surface = FlatSurface::default();
    let ray = GeometryRay {
        origin: [2.0, 1.0, 3.0],
        direction: [0.0, 0.0, -1.0],
    };
    assert!(
        surface
            .ray_intersections(&ray, 0.0, SurfaceDomain::Content)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        surface
            .ray_intersections(&ray, 0.0, SurfaceDomain::Continuation)
            .unwrap()[0]
            .content,
        [2.5, -0.5]
    );
    let parallel = GeometryRay {
        direction: [1.0, 0.0, 0.0],
        ..ray
    };
    assert!(
        surface
            .ray_intersections(&parallel, 0.0, SurfaceDomain::Continuation)
            .unwrap()
            .is_empty()
    );
    assert!(surface.validate_offsets([1.0, -1.0]).is_err());
    assert!(surface.sample([f64::NAN, 0.0], 0.0).is_err());
    assert!(surface.bounds([0.0, f64::INFINITY]).is_err());
}
