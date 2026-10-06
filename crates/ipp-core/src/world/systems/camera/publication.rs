//! View-independent final camera inputs retained across viewport changes.

use super::{Camera, CameraSystem, PreparedCamera};
use crate::components::schema::ComponentLifecycle;
use crate::systems::geometry::{GeometryRay, GeometryShapeTransform};
use crate::{ComponentValue, ErrorReason, OutputRef, WorldContext, WorldPlane};

/// A normalized ray with the camera's affine near/far clipping interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraViewRay {
    /// Ray in the camera publication's World space.
    pub ray: GeometryRay,
    /// Distance along the normalized ray to the near plane.
    pub near: f64,
    /// Distance along the normalized ray to the far plane.
    pub far: f64,
}

impl CameraViewRay {
    pub(crate) fn new(
        camera: &Camera,
        pose: &GeometryShapeTransform,
        point: [f32; 2],
        width: u32,
        height: u32,
    ) -> Result<Self, ErrorReason> {
        Self::for_extent(camera, pose, point, [f64::from(width), f64::from(height)])
    }

    fn for_extent(
        camera: &Camera,
        pose: &GeometryShapeTransform,
        point: [f32; 2],
        extent: [f64; 2],
    ) -> Result<Self, ErrorReason> {
        camera.validate()?;
        if !point.iter().all(|value| value.is_finite()) {
            return Err(ErrorReason::InvalidViewport);
        }

        let aspect = super::projection::projection_aspect(extent)?;
        let horizontal = f64::from(point[0]) * 2.0 - 1.0;
        let vertical = 1.0 - f64::from(point[1]) * 2.0;
        let (origin, direction) = if camera.projection == 0 {
            let extent = (f64::from(camera.fov_y) * 0.5).tan();
            (
                pose.point([0.0; 3]),
                pose.vector([horizontal * extent * aspect, vertical * extent, -1.0]),
            )
        } else {
            let extent = f64::from(camera.ortho_height) * 0.5;
            (
                pose.point([horizontal * extent * aspect, vertical * extent, 0.0]),
                pose.vector([0.0, 0.0, -1.0]),
            )
        };
        let length = direction
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        let near = f64::from(camera.near) * length;
        let far = f64::from(camera.far) * length;
        if !origin.iter().all(|value| value.is_finite())
            || !length.is_finite()
            || length <= 0.0
            || !near.is_finite()
            || !far.is_finite()
        {
            return Err(ErrorReason::InvalidGeometry);
        }

        Ok(Self {
            ray: GeometryRay {
                origin,
                direction: direction.map(|value| value / length),
            },
            near,
            far,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
/// View-independent owned inputs for one completed camera output.
pub struct CameraPublication {
    /// Exact selected output; active-camera state does not override it.
    pub selection: OutputRef,
    /// Effective projection inputs from this evaluation.
    pub projection: Camera,
    /// Final affine camera pose retaining parent shear.
    pub pose: GeometryShapeTransform,
}

impl CameraPublication {
    /// Project the frozen camera inputs for the current physical viewport.
    pub fn prepare(&self, width: u32, height: u32) -> Result<PreparedCamera, ErrorReason> {
        self.prepare_for_extent([f64::from(width), f64::from(height)])
    }

    /// Prepare a root pixel extent or nested physical Surface extent, never target rounding.
    pub fn prepare_for_extent(&self, extent: [f64; 2]) -> Result<PreparedCamera, ErrorReason> {
        super::prepare_affine_for_extent(
            self.selection
                .camera_entity()
                .ok_or(ErrorReason::InvalidEntity)?,
            &self.projection,
            &self.pose,
            extent,
        )
    }

    /// Construct a frozen-camera ray; coordinates may lie outside the normalized viewport.
    pub fn ray(
        &self,
        point: [f32; 2],
        width: u32,
        height: u32,
    ) -> Result<CameraViewRay, ErrorReason> {
        self.ray_for_extent(point, [f64::from(width), f64::from(height)])
    }

    /// Construct a ray using the same projection extent as rendering, independent of resolution.
    pub fn ray_for_extent(
        &self,
        point: [f32; 2],
        extent: [f64; 2],
    ) -> Result<CameraViewRay, ErrorReason> {
        self.prepare_for_extent(extent)?;
        CameraViewRay::for_extent(&self.projection, &self.pose, point, extent)
    }

    /// Intersect a forward ray with an explicit World-space plane, without live state.
    pub fn project(
        &self,
        point: [f32; 2],
        width: u32,
        height: u32,
        plane: WorldPlane,
    ) -> Result<Option<[f32; 3]>, ErrorReason> {
        self.project_for_extent(point, [f64::from(width), f64::from(height)], plane)
    }

    /// Project in this Camera domain using the containing Surface's physical aspect when nested.
    pub fn project_for_extent(
        &self,
        point: [f32; 2],
        extent: [f64; 2],
        plane: WorldPlane,
    ) -> Result<Option<[f32; 3]>, ErrorReason> {
        let ray = self.ray_for_extent(point, extent)?.ray;
        if !plane
            .point
            .iter()
            .chain(&plane.normal)
            .all(|value| value.is_finite())
            || plane.normal.iter().all(|&value| value == 0.0)
        {
            return Err(ErrorReason::InvalidValue);
        }

        let normal = plane.normal.map(f64::from);
        let denominator: f64 = ray
            .direction
            .iter()
            .zip(normal)
            .map(|(value, normal)| value * normal)
            .sum();
        if denominator == 0.0 {
            return Ok(None);
        }

        let offset: f64 = plane
            .point
            .iter()
            .zip(ray.origin)
            .zip(normal)
            .map(|((&point, origin), normal)| (f64::from(point) - origin) * normal)
            .sum();
        let distance = offset / denominator;
        if distance < 0.0 {
            return Ok(None);
        }

        let position = std::array::from_fn(|index| {
            (ray.origin[index] + distance * ray.direction[index]) as f32
        });
        if !position.iter().all(|value| value.is_finite()) {
            return Err(ErrorReason::InvalidValue);
        }

        Ok(Some(position))
    }
}

impl CameraSystem {
    pub(super) fn publish(
        &self,
        world: &WorldContext<'_>,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), ErrorReason> {
        for &entity in world.world.state.entities.keys() {
            let Some(projection) = world.world.components.camera(entity.index() as usize) else {
                continue;
            };
            let Ok(pose) = crate::systems::hierarchy::evaluated_affine(world.world, entity) else {
                continue;
            };
            let selection = world.camera_output(entity, ComponentValue::CAMERA)?;
            output.output(
                selection,
                CameraPublication {
                    selection,
                    projection: *projection,
                    pose,
                },
            );
        }
        Ok(())
    }
}
