use super::*;
use crate::{
    ComponentValue,
    components::schema::ComponentLifecycle,
    systems::geometry::{GeometryBounds, GeometryRay},
};

fn shapes(k: f32) -> Vec<Box<dyn Surface>> {
    vec![
        Box::new(CylinderSurface {
            width: 4.0,
            height: 3.0,
            curvature: k,
            layer_spacing: 0.2,
        }),
        Box::new(SphereSurface {
            width: 4.0,
            height: 3.0,
            curvature: k,
            layer_spacing: 0.2,
        }),
    ]
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected}"
    );
}

#[test]
fn independent_arc_points_and_signed_shells_preserve_tangent_orientation() {
    for k in [-0.5_f32, 0.5] {
        for shape in shapes(k) {
            let angle = f64::from(k);
            let base = [
                angle.sin() / f64::from(k),
                0.0,
                (angle.cos() - 1.0) / f64::from(k),
            ];
            let normal = [angle.sin(), 0.0, angle.cos()];
            let sample = shape.sample([3.0, 1.5], 0.3).unwrap();
            for axis in 0..3 {
                near(
                    sample.position[axis],
                    base[axis] + 0.3 * normal[axis],
                    1e-12,
                );
                near(sample.front_normal[axis], normal[axis], 1e-12);
            }
            assert_eq!(
                shape.sample([2.0, 1.5], 0.3).unwrap().position,
                [0.0, 0.0, 0.3]
            );
        }
        let sphere = SphereSurface {
            width: 4.0,
            height: 3.0,
            curvature: k,
            ..Default::default()
        };
        let r = 1.25_f64;
        let angle = f64::from(k) * r;
        let sample = sphere.sample([2.75, 0.5], 0.0).unwrap();
        near(sample.position[0], 0.75 * angle.sin() / angle, 1e-12);
        near(sample.position[1], angle.sin() / angle, 1e-12);
        near(
            sample.position[2],
            (angle.cos() - 1.0) / f64::from(k),
            1e-12,
        );
    }
}

#[test]
fn conformance_inverse_distances_normals_and_bounds_cover_both_facing_shells() {
    for k in [-0.5_f32, -1e-9, 0.0, 1e-9, 0.5] {
        for shape in shapes(k) {
            let enclosure = shape.bounds([-0.2, 0.3]).unwrap().bounds().unwrap();
            for ix in 0..=20 {
                for iy in 0..=20 {
                    let content = [4.0 * f64::from(ix) / 20.0, 3.0 * f64::from(iy) / 20.0];
                    for offset in [-0.2, 0.0, 0.3] {
                        let sample = shape.sample(content, offset).unwrap();
                        for axis in 0..3 {
                            assert!(
                                sample.position[axis] >= enclosure[0][axis] - 1e-12
                                    && sample.position[axis] <= enclosure[1][axis] + 1e-12,
                                "sample {:?} outside {:?}",
                                sample,
                                enclosure
                            );
                        }
                        let ray = GeometryRay {
                            origin: std::array::from_fn(|axis| {
                                sample.position[axis] + 3.0 * sample.front_normal[axis]
                            }),
                            direction: sample.front_normal.map(|value| -2.0 * value),
                        };
                        let intersections = shape
                            .ray_intersections(&ray, offset, SurfaceDomain::Continuation)
                            .unwrap();
                        let hit = intersections
                            .iter()
                            .min_by(|a, b| {
                                (a.distance - 1.5)
                                    .abs()
                                    .total_cmp(&(b.distance - 1.5).abs())
                            })
                            .expect("sample has inverse intersection");
                        near(hit.distance, 1.5, 2e-6);
                        for (actual, expected) in hit.content.into_iter().zip(content) {
                            near(actual, expected, 2e-6);
                        }
                        for axis in 0..3 {
                            near(hit.front_normal[axis], sample.front_normal[axis], 2e-6);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn zero_and_tiny_curvature_have_stable_continuous_flat_limits() {
    let flat = FlatSurface {
        width: 4.0,
        height: 3.0,
        ..Default::default()
    };
    for k in [0.0, -1e-20, 1e-20, -1e-7, 1e-7] {
        for shape in shapes(k) {
            for content in [[0.0, 0.0], [4.0, 3.0], [2.0, 1.5]] {
                let actual = shape.sample(content, 0.2).unwrap();
                let expected = flat.sample(content, 0.2).unwrap();
                for axis in 0..3 {
                    near(actual.position[axis], expected.position[axis], 1e-6);
                    near(actual.front_normal[axis], expected.front_normal[axis], 1e-6);
                }
                let ray = GeometryRay {
                    origin: [expected.position[0], expected.position[1], 3.0],
                    direction: [0.0, 0.0, -1.0],
                };
                let hits = shape
                    .ray_intersections(&ray, 0.2, SurfaceDomain::Continuation)
                    .unwrap();
                assert!(hits.iter().any(|hit| (hit.distance - 2.8).abs() < 1e-6));
            }
            assert_eq!(shape.exact_affine(0.2).is_some(), k == 0.0);
        }
    }
}

#[test]
fn continuation_does_not_invent_hits_and_every_candidate_is_domain_filtered() {
    for shape in shapes(0.5) {
        let sample = shape.sample([4.5, 1.5], 0.0).unwrap();
        let ray = GeometryRay {
            origin: std::array::from_fn(|axis| sample.position[axis] + sample.front_normal[axis]),
            direction: sample.front_normal.map(|value| -value),
        };
        assert!(
            shape
                .ray_intersections(&ray, 0.0, SurfaceDomain::Content)
                .unwrap()
                .iter()
                .all(|hit| (hit.distance - 1.0).abs() > 1e-6)
        );
        assert!(
            shape
                .ray_intersections(&ray, 0.0, SurfaceDomain::Continuation)
                .unwrap()
                .iter()
                .any(|hit| (hit.distance - 1.0).abs() < 1e-6)
        );
        let miss = GeometryRay {
            origin: [10.0, 0.0, 3.0],
            direction: [0.0, 0.0, -1.0],
        };
        assert!(
            shape
                .ray_intersections(&miss, 0.0, SurfaceDomain::Continuation)
                .unwrap()
                .is_empty()
        );
        assert!(shape.sample([100.0, 1.5], 0.0).is_err());
    }
    // First sphere root lies outside the rectangle; farther root inside remains eligible.
    let sphere = SphereSurface {
        width: 7.5,
        height: 1.0,
        curvature: 0.5,
        ..Default::default()
    };
    let ray = GeometryRay {
        origin: [-4.0, 0.0, -3.0],
        direction: [1.0, 0.0, 0.1],
    };
    let all = sphere
        .ray_intersections(&ray, 0.0, SurfaceDomain::Continuation)
        .unwrap();
    let bounded = sphere
        .ray_intersections(&ray, 0.0, SurfaceDomain::Content)
        .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(bounded.len(), 1);
    assert_eq!(bounded[0], all[1]);
}

#[test]
fn parameters_and_entire_offset_ranges_reject_ambiguous_or_inverted_shells() {
    for component in [
        ComponentValue::CylinderSurface(CylinderSurface {
            width: 7.0,
            curvature: 1.0,
            ..Default::default()
        }),
        ComponentValue::SphereSurface(SphereSurface {
            width: 6.0,
            height: 3.0,
            curvature: 1.0,
            ..Default::default()
        }),
    ] {
        assert!(component.validate_lifecycle().is_err());
    }
    assert!(
        CylinderSurface {
            curvature: f32::NAN,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    for k in [-0.5, 0.5] {
        for shape in shapes(k) {
            let collapsed = -1.0 / f64::from(k);
            assert!(shape.validate_offsets([collapsed, collapsed]).is_err());
            assert!(
                shape
                    .bounds([collapsed.min(0.0), collapsed.max(0.0)])
                    .is_err()
            );
            assert!(shape.sample([2.0, 1.5], collapsed).is_err());
            let ray = GeometryRay {
                origin: [0.0, 0.0, 4.0],
                direction: [0.0, 0.0, -1.0],
            };
            assert!(
                shape
                    .ray_intersections(&ray, collapsed, SurfaceDomain::Content)
                    .is_err()
            );
        }
    }
}

#[test]
fn approximation_bounds_cover_both_triangle_interpolants_at_every_shell() {
    for k in [-0.5, 0.0, 0.5] {
        for shape in shapes(k) {
            for offset in [-0.2, 0.3] {
                let patch = [0.2, 0.3, 3.6, 2.8];
                let bound = shape.approximation_error(patch, offset).unwrap();
                let points = [
                    [patch[0], patch[1]],
                    [patch[2], patch[1]],
                    [patch[0], patch[3]],
                    [patch[2], patch[3]],
                ];
                let vertices = points.map(|p| shape.sample(p, offset).unwrap().position);
                for indices in [[0, 1, 3], [0, 3, 2]] {
                    for i in 0..=20 {
                        for j in 0..=20 - i {
                            let weights = [
                                f64::from(i) / 20.0,
                                f64::from(j) / 20.0,
                                1.0 - f64::from(i + j) / 20.0,
                            ];
                            let point = std::array::from_fn(|axis| {
                                indices
                                    .iter()
                                    .zip(weights)
                                    .map(|(&index, weight)| points[index][axis] * weight)
                                    .sum()
                            });
                            let linear: [f64; 3] = std::array::from_fn(|axis| {
                                indices
                                    .iter()
                                    .zip(weights)
                                    .map(|(&index, weight)| vertices[index][axis] * weight)
                                    .sum()
                            });
                            let sample = shape.sample(point, offset).unwrap();
                            let error = sample
                                .position
                                .iter()
                                .zip(linear)
                                .map(|(a, b)| (a - b).powi(2))
                                .sum::<f64>()
                                .sqrt();
                            assert!(error <= bound + 1e-12, "error {error} > bound {bound}");
                        }
                    }
                }
            }
        }
    }
}
