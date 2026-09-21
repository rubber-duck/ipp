use super::system::state;
use crate::{RenderItem, components::Transform, systems::camera, world::WorldSimulationState};

/// Evaluated particle presentation data; renderer chooses its upload layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleRenderData {
    /// Stable birth identity, used for deterministic depth ties.
    pub id: u64,
    /// Sprite geometry rather than the selected mesh.
    pub sprite: bool,
    /// Additive transparency.
    pub additive: bool,
    /// Stretch the sprite's Y axis along projected velocity.
    pub velocity_aligned: bool,
    /// Evaluated opacity.
    pub opacity: f32,
    /// World-space velocity for view-specific alignment.
    pub velocity: [f32; 3],
}

pub(in crate::world) fn prepare_particles(
    world: &WorldSimulationState,
    base: RenderItem,
    items: &mut Vec<RenderItem>,
    written: &mut usize,
) {
    let index = base.entity.index() as usize;
    let Some(state) = state(world, index) else {
        return;
    };
    let sprite = world.components.particle_sprite(index);
    if sprite.is_none() && world.components.particle_mesh(index).is_none() {
        return;
    }
    for p in &state.particles {
        let age = (p.age / p.lifetime).clamp(0.0, 1.0) as f32;
        let size = p.size * sprite.map_or(1.0, |s| 1.0 + (s.end_size - 1.0) * age);
        if size <= 0.0 {
            continue;
        }
        let transform = Transform {
            x: p.position[0],
            y: p.position[1],
            z: p.position[2],
            qx: p.rotation[0],
            qy: p.rotation[1],
            qz: p.rotation[2],
            qw: p.rotation[3],
            sx: size,
            sy: size,
            sz: size,
        };
        let Ok(local) = camera::model_matrix(&transform) else {
            continue;
        };
        let model = if state.space == 0 {
            camera::multiply(base.model, local)
        } else {
            local
        };
        let velocity = if state.space == 0 {
            std::array::from_fn(|i| {
                base.model[i] * p.velocity[0]
                    + base.model[i + 4] * p.velocity[1]
                    + base.model[i + 8] * p.velocity[2]
            })
        } else {
            p.velocity
        };
        let item = RenderItem {
            model,
            transform,
            particle: Some(ParticleRenderData {
                id: p.id,
                sprite: sprite.is_some(),
                additive: sprite.is_some_and(|s| s.blend == 1),
                velocity_aligned: sprite.is_some_and(|s| s.alignment == 1),
                opacity: sprite.map_or(1.0, |s| s.opacity + (s.end_opacity - s.opacity) * age),
                velocity,
            }),
            ..base
        };
        if let Some(slot) = items.get_mut(*written) {
            *slot = item;
        } else {
            items.push(item);
        }
        *written += 1;
    }
}

impl crate::WorldContext<'_> {
    /// Explicit evaluated particle enclosure. An implicit source-mesh bound is never used.
    pub fn particle_bounding_geometry(
        &self,
        entity: crate::EntityId,
    ) -> Option<&crate::systems::geometry::CompoundGeometryShape> {
        self.world.state.entities.get(&entity)?;
        let bound = self
            .world
            .components
            .bounding_geometry(entity.index() as usize)?;
        if bound.geometry.is_empty() && bound.source.is_empty() {
            return None;
        }
        self.bounding_geometry(entity).ok()
    }
}
