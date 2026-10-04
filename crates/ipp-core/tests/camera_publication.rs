//! Frozen camera math over actual Host publications; no transport or presentation claim.

mod support;

use support::selection::CAMERA;

use ipp_core::components::{Camera, Transform};
use ipp_core::systems::{camera::CameraPublication, geometry::GeometryShapeTransform};
use ipp_core::{Batch, Command, ComponentValue, EntityRef, ErrorReason, OutputKind, WorldPlane};

fn publication(camera: Camera) -> CameraPublication {
    let mut host = crate::support::task_scheduler::host();
    let world = host.create_world(Default::default(), CAMERA).unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(EntityRef::Alias(1), ComponentValue::Camera(camera)),
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Transform(Transform::default()),
                ),
            ],
        })
        .unwrap();
    let entity = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()[0]
        .1;
    let reference = host.world_ref(world).unwrap();
    let output = host
        .bind_output(reference, entity, OutputKind::Camera)
        .unwrap();
    host.frame(0.0).unwrap();
    host.output(host.latest_publication(world).unwrap(), output)
        .unwrap()
        .data::<CameraPublication>()
        .unwrap()
        .clone()
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

#[test]
fn frozen_camera_rays_preserve_affine_clip_planes_and_viewport_aspect() {
    let mut camera = publication(Camera {
        fov_y: std::f32::consts::FRAC_PI_2,
        near: 1.0,
        far: 10.0,
        ..Default::default()
    });
    camera.pose = GeometryShapeTransform::new([
        2.0, 0.0, 0.0, 0.0, 0.5, 3.0, 0.0, 0.0, 0.0, 0.25, 4.0, 0.0, 5.0, 6.0, 7.0, 1.0,
    ])
    .unwrap();
    let ray = camera.ray([1.0, 0.0], 640, 480).unwrap();
    let extent = (f64::from(camera.projection.fov_y) * 0.5).tan();
    let expected = camera.pose.vector([extent * 4.0 / 3.0, extent, -1.0]);
    let length = expected
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    assert_eq!(ray.ray.origin, [5.0, 6.0, 7.0]);
    for (actual, expected) in ray.ray.direction.into_iter().zip(expected) {
        close(actual, expected / length);
    }
    close(ray.near, length);
    close(ray.far, 10.0 * length);
    assert_ne!(
        ray.ray.direction,
        camera.ray([1.0, 0.0], 1, 1).unwrap().ray.direction
    );
}

#[test]
fn frozen_orthographic_projection_uses_only_explicit_plane_and_captured_pose() {
    let camera = publication(Camera {
        projection: 1,
        ortho_height: 4.0,
        ..Default::default()
    });
    let plane = WorldPlane {
        point: [0.0, 0.0, -5.0],
        normal: [0.0, 0.0, 1.0],
    };
    assert_eq!(
        camera.project([1.0, 0.0], 640, 320, plane).unwrap(),
        Some([4.0, 2.0, -5.0])
    );
    assert_eq!(
        camera.project([1.5, 0.5], 640, 320, plane).unwrap(),
        Some([8.0, 0.0, -5.0])
    );
    assert_eq!(
        camera
            .project(
                [0.5, 0.5],
                640,
                480,
                WorldPlane {
                    point: [0.0, 0.0, 5.0],
                    ..plane
                }
            )
            .unwrap(),
        None
    );
    assert_eq!(
        camera
            .project(
                [0.5, 0.5],
                640,
                480,
                WorldPlane {
                    normal: [1.0, 0.0, 0.0],
                    ..plane
                }
            )
            .unwrap(),
        None
    );
}

#[test]
fn frozen_camera_queries_reject_invalid_coordinates_dimensions_and_planes() {
    let camera = publication(Camera::default());
    for (point, width, height) in [
        ([f32::NAN, 0.5], 640, 480),
        ([0.5, f32::INFINITY], 640, 480),
        ([0.5, 0.5], 0, 480),
        ([0.5, 0.5], 640, 0),
    ] {
        assert_eq!(
            camera.ray(point, width, height),
            Err(ErrorReason::InvalidViewport)
        );
    }
    for plane in [
        WorldPlane {
            point: [0.0; 3],
            normal: [0.0; 3],
        },
        WorldPlane {
            point: [f32::NAN, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        },
    ] {
        assert_eq!(
            camera.project([0.5, 0.5], 640, 480, plane),
            Err(ErrorReason::InvalidValue)
        );
    }
}
