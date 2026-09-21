use crate::systems::camera::{CameraReadAccess, CameraSystem};
use crate::world::WorldSimulationState;
use crate::{
    ErrorReason, GeometryPickHit, GeometryPickQuery, WorldPlane,
    components::Camera,
    systems::geometry::GeometryShapeTransform,
    systems::geometry::{GeometryBounds, dot},
};

struct GeometryRay {
    origin: [f64; 3],
    direction: [f64; 3],
    near: f64,
    far: f64,
}

impl GeometryRay {
    fn new(
        camera: &Camera,
        affine: &GeometryShapeTransform,
        query: GeometryPickQuery,
    ) -> Result<Self, ErrorReason> {
        let aspect = f64::from(query.width) / f64::from(query.height);
        let x = f64::from(query.x) * 2.0 - 1.0;
        let y = 1.0 - f64::from(query.y) * 2.0;
        let (origin, direction) = if camera.projection == 0 {
            let extent = (f64::from(camera.fov_y) * 0.5).tan();
            (
                affine.point([0.0; 3]),
                affine.vector([x * extent * aspect, y * extent, -1.0]),
            )
        } else {
            let extent = f64::from(camera.ortho_height) * 0.5;
            (
                affine.point([x * extent * aspect, y * extent, 0.0]),
                affine.vector([0.0, 0.0, -1.0]),
            )
        };
        let length = dot(direction, direction).sqrt();
        if !length.is_finite() || length <= 0.0 {
            return Err(ErrorReason::InvalidGeometry);
        }
        Ok(Self {
            origin,
            direction: direction.map(|value| value / length),
            near: f64::from(camera.near) * length,
            far: f64::from(camera.far) * length,
        })
    }
}

pub(in crate::world) struct GeometryQueryAccess<'a> {
    world: &'a WorldSimulationState,
    camera: CameraReadAccess<'a>,
}

impl<'a> GeometryQueryAccess<'a> {
    pub(in crate::world) fn new(
        world: &'a WorldSimulationState,
        camera: CameraReadAccess<'a>,
    ) -> Self {
        Self {
            world,
            camera,
        }
    }

    pub(in crate::world) fn camera_project(
        &self,
        query: crate::CameraProjectQuery,
    ) -> Result<Option<[f32; 3]>, ErrorReason> {
        if query.width == 0
            || query.height == 0
            || !query.x.is_finite()
            || !query.y.is_finite()
            || self
                .camera
                .render_viewport()
                .is_some_and(|viewport| viewport != (query.width, query.height))
        {
            return Err(ErrorReason::InvalidViewport);
        }

        if !query
            .plane
            .point
            .iter()
            .chain(&query.plane.normal)
            .all(|v| v.is_finite())
            || query.plane.normal.iter().all(|&v| v == 0.0)
        {
            return Err(ErrorReason::InvalidValue);
        }

        let camera = self.camera.prepare_camera(query.width, query.height)?;
        let index = camera.entity.index() as usize;
        let ray = GeometryRay::new(
            self.world
                .components
                .camera(index)
                .ok_or(ErrorReason::MissingComponent)?,
            &crate::systems::hierarchy::evaluated_affine(self.world, camera.entity)?,
            GeometryPickQuery {
                x: query.x,
                y: query.y,
                width: query.width,
                height: query.height,
                include_view_plane: false,
            },
        )?;

        let normal = query.plane.normal.map(f64::from);
        let denominator = dot(ray.direction, normal);
        if denominator == 0.0 {
            return Ok(None);
        }

        let offset = std::array::from_fn(|i| f64::from(query.plane.point[i]) - ray.origin[i]);
        let distance = dot(offset, normal) / denominator;
        if distance < 0.0 {
            return Ok(None);
        }

        let position =
            std::array::from_fn(|i| (ray.origin[i] + distance * ray.direction[i]) as f32);
        if !position.iter().all(|v| v.is_finite()) {
            return Err(ErrorReason::InvalidValue);
        }

        Ok(Some(position))
    }

    pub(in crate::world) fn geometry_pick(
        &self,
        query: GeometryPickQuery,
    ) -> Result<Option<GeometryPickHit>, ErrorReason> {
        if query.width == 0
            || query.height == 0
            || !query.x.is_finite()
            || !query.y.is_finite()
            || !(0.0..=1.0).contains(&query.x)
            || !(0.0..=1.0).contains(&query.y)
            || self
                .camera
                .render_viewport()
                .is_some_and(|viewport| viewport != (query.width, query.height))
        {
            return Err(ErrorReason::InvalidViewport);
        }
        let camera = self.camera.prepare_camera(query.width, query.height)?;
        let camera_index = camera.entity.index() as usize;
        let camera_transform =
            crate::systems::hierarchy::evaluated_affine(self.world, camera.entity)?;
        let ray = GeometryRay::new(
            self.world
                .components
                .camera(camera_index)
                .ok_or(ErrorReason::MissingComponent)?,
            &camera_transform,
            query,
        )?;

        let mut nearest: Option<(f64, GeometryPickHit)> = None;
        let mut intervals = Vec::new();
        // Entity ordering and stable authored part order resolve exact ties.
        for &entity in self.world.state.entities.keys() {
            let Some(value) = self
                .world
                .components
                .picking_geometry(entity.index() as usize)
            else {
                continue;
            };
            let Some(geometry) = &value.runtime.evaluation else {
                continue;
            };
            let geometry = geometry.evaluated()?;
            let hit = geometry.ray_intersection_with_scratch(
                &crate::systems::geometry::GeometryRay {
                    origin: ray.origin,
                    direction: ray.direction,
                },
                ray.near,
                ray.far,
                &mut intervals,
            );
            if let Some(hit) = hit
                && nearest
                    .as_ref()
                    .is_none_or(|(previous, _)| hit.distance < *previous)
            {
                let position = std::array::from_fn(|i| {
                    (ray.origin[i] + ray.direction[i] * hit.distance) as f32
                });
                if !(hit.distance as f32).is_finite() || !position.iter().all(|v| v.is_finite()) {
                    return Err(ErrorReason::InvalidGeometry);
                }
                nearest = Some((
                    hit.distance,
                    GeometryPickHit {
                        entity,
                        position,
                        distance: hit.distance as f32,
                        part: hit.part,
                        view_plane: None,
                    },
                ));
            }
        }

        let Some((_, mut hit)) = nearest else {
            return Ok(None);
        };

        if query.include_view_plane {
            let forward = camera_transform.vector([0.0, 0.0, -1.0]);
            let length = dot(forward, forward).sqrt();
            hit.view_plane = Some(WorldPlane {
                point: hit.position,
                normal: forward.map(|value| (value / length) as f32),
            });
        }

        Ok(Some(hit))
    }
}

/// Queued requests evaluated against the final camera and geometry component results.
pub(in crate::world) enum GeometryQueryCommand {
    Project {
        request_id: u64,
        query: crate::CameraProjectQuery,
    },
    Pick {
        request_id: u64,
        query: GeometryPickQuery,
    },
}

impl crate::WorldContext<'_> {
    /// Queue a stateless plane projection for the final effective camera.
    pub fn enqueue_camera_project(
        &mut self,
        request_id: u64,
        query: crate::CameraProjectQuery,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command(
            CameraSystem::ID,
            0,
            GeometryQueryCommand::Project {
                request_id,
                query,
            },
        )
    }

    /// Queue a read-only query without advancing time or resizing the surface.
    pub fn enqueue_geometry_pick(
        &mut self,
        request_id: u64,
        query: GeometryPickQuery,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command(
            CameraSystem::ID,
            0,
            GeometryQueryCommand::Pick {
                request_id,
                query,
            },
        )
    }
}
