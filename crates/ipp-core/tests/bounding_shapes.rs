//! Intersection invariants under affine placement and compound unions.

use ipp_core::systems::geometry::{
    CompoundGeometryShape, GeometryBounds, GeometryPlane, GeometryPlaneIntersection, GeometryRay,
    GeometryShape, GeometryShapeTransform, TransformedGeometryShape, frustum_planes,
};

fn ray(origin: [f64; 3], direction: [f64; 3]) -> GeometryRay {
    GeometryRay {
        origin,
        direction,
    }
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn prepared_enclosures_match_independent_plane_support_under_shear_and_reflection() {
    let shapes = [
        GeometryShape::Box {
            min: [-2.0, -0.5, 1.0],
            max: [0.7, 3.0, 2.0],
        },
        GeometryShape::Sphere {
            center: [1.0, -2.0, 0.4],
            radius: 1.7,
        },
        GeometryShape::Pill {
            start: [-1.0, 0.0, 2.0],
            end: [2.0, 4.0, -0.5],
            radius: 0.8,
        },
    ];
    let check = |shape: &dyn GeometryBounds| {
        let bounds = shape.bounds().unwrap();
        for axis in 0..3 {
            let mut normal = [0.0; 3];
            normal[axis] = 1.0;
            let interval = shape
                .plane_interval(&GeometryPlane {
                    normal,
                    offset: 0.0,
                })
                .unwrap();
            close(bounds[0][axis], interval[0]);
            close(bounds[1][axis], interval[1]);
        }
    };
    for sample in 0..80 {
        let t = f64::from(sample) * 0.17;
        let transform = GeometryShapeTransform::new([
            -1.0 - t,
            0.2,
            0.1,
            0.0,
            t.sin(),
            0.7,
            0.3,
            0.0,
            0.1,
            t.cos(),
            1.3,
            0.0,
            t,
            -2.0 * t,
            0.4,
            1.0,
        ])
        .unwrap();
        let compound = CompoundGeometryShape {
            parts: shapes
                .into_iter()
                .map(|shape| TransformedGeometryShape {
                    shape,
                    transform,
                })
                .collect(),
        };
        for shape in &compound.parts {
            check(shape);
        }
        check(&compound);
    }
    assert!(CompoundGeometryShape::default().bounds().is_none());
}

#[test]
fn primitives_handle_tangencies_caps_parallel_rays_and_inside_exits() {
    let sphere = GeometryShape::Sphere {
        center: [0.0; 3],
        radius: 1.0,
    };
    close(
        sphere
            .ray_intersection(&ray([1.0, 0.0, 3.0], [0.0, 0.0, -1.0]), 0.0, 10.0)
            .unwrap()
            .distance,
        3.0,
    );
    close(
        sphere
            .ray_intersection(&ray([0.0; 3], [0.0, 0.0, 2.0]), 0.0, 10.0)
            .unwrap()
            .distance,
        0.5,
    );
    assert!(
        sphere
            .ray_intersection(&ray([1.01, 0.0, 3.0], [0.0, 0.0, -1.0]), 0.0, 10.0)
            .is_none()
    );

    let pill = GeometryShape::Pill {
        start: [0.0, -1.0, 0.0],
        end: [0.0, 1.0, 0.0],
        radius: 0.5,
    };
    close(
        pill.ray_intersection(&ray([0.0, 4.0, 0.0], [0.0, -1.0, 0.0]), 0.0, 10.0)
            .unwrap()
            .distance,
        2.5,
    );
    close(
        pill.ray_intersection(&ray([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]), 0.0, 10.0)
            .unwrap()
            .distance,
        3.5,
    );
    close(
        pill.ray_intersection(&ray([0.0, 1.5, 4.0], [0.0, 0.0, -1.0]), 0.0, 10.0)
            .unwrap()
            .distance,
        4.0,
    );
    assert!(
        pill.ray_intersection(&ray([0.6, 4.0, 0.0], [0.0, -1.0, 0.0]), 0.0, 10.0)
            .is_none()
    );
    let collapsed = GeometryShape::Pill {
        start: [0.0; 3],
        end: [0.0; 3],
        radius: 1.0,
    };
    assert_eq!(
        collapsed.ray_intersection(&ray([0.0; 3], [1.0, 0.0, 0.0]), 0.0, 10.0),
        sphere.ray_intersection(&ray([0.0; 3], [1.0, 0.0, 0.0]), 0.0, 10.0)
    );

    let shape = GeometryShape::default();
    close(
        shape
            .ray_intersection(&ray([0.0; 3], [1.0, 0.0, 0.0]), 0.0, 10.0)
            .unwrap()
            .distance,
        0.5,
    );
    assert!(
        shape
            .ray_intersection(&ray([0.6, 0.0, 4.0], [0.0, 0.0, -1.0]), 0.0, 10.0)
            .is_none()
    );
    assert!(
        shape
            .ray_intersection(&ray([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]), 3.6, 4.4)
            .is_none()
    );
}

#[test]
fn affine_ray_and_plane_tests_preserve_scale_rotation_and_shear() {
    let transform = GeometryShapeTransform::new([
        0.0, 2.0, 0.0, 0.0, -1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 3.0, 4.0, -2.0, 1.0,
    ])
    .unwrap();
    let shape = TransformedGeometryShape {
        shape: GeometryShape::Sphere {
            center: [0.0; 3],
            radius: 1.0,
        },
        transform,
    };
    close(
        shape
            .ray_intersection(&ray([3.0, 4.0, 3.0], [0.0, 0.0, -1.0]), 0.0, 10.0)
            .unwrap()
            .distance,
        4.5,
    );
    let plane = GeometryPlane {
        normal: [0.0, 1.0, 0.0],
        offset: -4.0,
    };
    let interval = shape.plane_interval(&plane).unwrap();
    close(interval[0], -5.0_f64.sqrt());
    close(interval[1], 5.0_f64.sqrt());
    assert_eq!(
        shape.plane_intersection(&plane),
        Some(GeometryPlaneIntersection::Intersecting)
    );
    assert_eq!(
        shape.plane_intersection(&GeometryPlane {
            normal: [0.0, 0.0, 1.0],
            offset: 3.0
        }),
        Some(GeometryPlaneIntersection::Positive)
    );
    assert!(transform.maximum_stretch() >= 5.0_f64.sqrt());
    assert!(GeometryShapeTransform::new([0.0; 16]).is_err());
}

#[test]
fn compound_ray_returns_union_boundary_and_stable_part_identity() {
    let sphere = |x| TransformedGeometryShape {
        shape: GeometryShape::Sphere {
            center: [x, 0.0, 0.0],
            radius: 1.0,
        },
        transform: GeometryShapeTransform::default(),
    };
    let shape = CompoundGeometryShape {
        parts: vec![sphere(0.0), sphere(1.0), sphere(5.0)],
    };
    let hit = shape
        .ray_intersection(&ray([0.5, 0.0, 0.0], [1.0, 0.0, 0.0]), 0.0, 10.0)
        .unwrap();
    close(hit.distance, 1.5);
    assert_eq!(hit.part, 1);
    let hit = shape
        .ray_intersection(&ray([-3.0, 0.0, 0.0], [1.0, 0.0, 0.0]), 0.0, 10.0)
        .unwrap();
    close(hit.distance, 2.0);
    assert_eq!(hit.part, 0);
    assert_eq!(shape.bounds().unwrap(), [[-1.0; 3], [6.0, 1.0, 1.0]]);
}

#[test]
fn frustum_planes_cover_all_six_boundaries_and_keep_straddling_compounds() {
    let planes = frustum_planes(GeometryShapeTransform::default().render_matrix().unwrap());
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let mut center = [0.0; 3];
            center[axis] = sign * 1.5;
            assert!(
                !GeometryShape::Sphere {
                    center,
                    radius: 0.1
                }
                .intersects_frustum(&planes)
            );
            center[axis] = sign * 1.1;
            assert!(
                GeometryShape::Sphere {
                    center,
                    radius: 0.2
                }
                .intersects_frustum(&planes)
            );
        }
    }
    let shape = CompoundGeometryShape {
        parts: vec![
            TransformedGeometryShape {
                shape: GeometryShape::Sphere {
                    center: [3.0, 0.0, 0.0],
                    radius: 0.1,
                },
                transform: GeometryShapeTransform::default(),
            },
            TransformedGeometryShape {
                shape: GeometryShape::default(),
                transform: GeometryShapeTransform::default(),
            },
        ],
    };
    assert!(shape.intersects_frustum(&planes));
    assert!(!CompoundGeometryShape::default().intersects_frustum(&planes));
}
