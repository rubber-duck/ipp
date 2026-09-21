//! Conservative whole-effect bounds, independent of cameras and particle draw batching.

use super::{
    GeometryShape, GeometryShapeTransform, TransformedGeometryShape, update::GeometryReadAccess,
};
use crate::{EntityId, ErrorReason, components::Transform, systems::camera};

impl GeometryReadAccess<'_> {
    pub(super) fn particle_enclosure(
        &self,
        entity: EntityId,
    ) -> Result<TransformedGeometryShape, ErrorReason> {
        let index = entity.index() as usize;
        #[cfg(feature = "skeletal-animation")]
        if self.world.components.skin(index).is_some() {
            return Err(ErrorReason::GeometryUnavailable);
        }
        #[cfg(feature = "mesh-poses")]
        if self.world.components.mesh_pose(index).is_some() {
            return Err(ErrorReason::GeometryUnavailable);
        }
        let state = crate::systems::particles::particle_state(self.world, index)
            .ok_or(ErrorReason::GeometryUnavailable)?;
        let sprite = self.world.components.particle_sprite(index);
        let (min, max) = if sprite.is_some() {
            ([-0.5f32; 3], [0.5f32; 3])
        } else {
            let mesh = self
                .world
                .components
                .particle_mesh(index)
                .ok_or(ErrorReason::GeometryUnavailable)?;
            let key = self
                .assets
                .find_source(self.world.id, crate::MESH_TYPE, &mesh.source, mesh.variant)
                .ok_or(ErrorReason::GeometryUnavailable)?;
            self.assets
                .get_typed::<crate::services::asset_management::mesh_metadata::MeshMetadata>(key)
                .ok_or(ErrorReason::GeometryUnavailable)?
                .bounds()
        };
        let local_radius = (0..3)
            .map(|i| f64::from(min[i].abs().max(max[i].abs())).powi(2))
            .sum::<f64>()
            .sqrt();
        let parent = self.geometry_model(entity)?.render_matrix()?;
        let mut bounds: Option<[[f64; 3]; 2]> = None;
        for particle in &state.particles {
            let age = (particle.age / particle.lifetime).clamp(0.0, 1.0) as f32;
            let size = particle.size * sprite.map_or(1.0, |s| 1.0 + (s.end_size - 1.0) * age);
            if size <= 0.0 {
                continue;
            }
            let local = camera::model_matrix(&Transform {
                x: particle.position[0],
                y: particle.position[1],
                z: particle.position[2],
                qx: particle.rotation[0],
                qy: particle.rotation[1],
                qz: particle.rotation[2],
                qw: particle.rotation[3],
                sx: size,
                sy: size,
                sz: size,
            })?;
            let model = if state.space == 0 {
                camera::multiply(parent, local)
            } else {
                local
            };
            let scale = (0..3)
                .flat_map(|c| (0..3).map(move |r| f64::from(model[c * 4 + r]).powi(2)))
                .sum::<f64>()
                .sqrt();
            let radius = local_radius * scale;
            let particle_bounds = [
                std::array::from_fn(|i| f64::from(model[12 + i]) - radius),
                std::array::from_fn(|i| f64::from(model[12 + i]) + radius),
            ];
            bounds = Some(bounds.map_or(particle_bounds, |previous| {
                [
                    std::array::from_fn(|i| previous[0][i].min(particle_bounds[0][i])),
                    std::array::from_fn(|i| previous[1][i].max(particle_bounds[1][i])),
                ]
            }));
        }
        let [min, max] = bounds.ok_or(ErrorReason::GeometryUnavailable)?;
        Ok(TransformedGeometryShape {
            shape: GeometryShape::Box {
                min,
                max,
            },
            transform: GeometryShapeTransform::default(),
        })
    }
}
