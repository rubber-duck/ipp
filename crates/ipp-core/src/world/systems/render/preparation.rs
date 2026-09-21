//! Render membership and resource compatibility compiled outside frame evaluation.

use super::*;
use crate::{components::registry::ComponentStorage, world::component_binding::ComponentBinding};

pub(super) struct DebugGeometryEntry {
    pub(super) entity: EntityId,
    pub(super) bounds: Option<ComponentBinding<crate::components::BoundingGeometry>>,
    pub(super) picking: Option<ComponentBinding<crate::components::PickingGeometry>>,
}

pub(super) struct LightEntry {
    light: ComponentBinding<Light>,
    model: crate::systems::hierarchy::ObjectTransformBinding,
    entity: EntityId,
}

impl LightEntry {
    pub(super) fn sample(
        &self,
        storage: &ComponentStorage,
    ) -> Option<(EntityId, [f32; 16], Light)> {
        let light = *self.light.get(storage);
        if light.intensity <= 0.0 {
            return None;
        }
        Some((
            self.entity,
            self.model.borrow(storage)?.ok()?.render_matrix().ok()?,
            light,
        ))
    }
}

pub(super) struct RenderEntry {
    item: RenderItem,
    transform: ComponentBinding<Transform>,
    model: crate::systems::hierarchy::ObjectTransformBinding,
    material: MaterialBinding,
    #[cfg(feature = "mesh-poses")]
    pose: Option<ComponentBinding<MeshPose>>,
    #[cfg(feature = "skeletal-animation")]
    skin: Option<ComponentBinding<crate::components::Skin>>,
    #[cfg(feature = "particles")]
    particles: bool,
}

enum MaterialBinding {
    Fixed,
    Unlit(ComponentBinding<UnlitMaterial>),
    Pbr(ComponentBinding<PbrMaterial>),
    #[cfg(feature = "particles")]
    Sprite(ComponentBinding<crate::components::ParticleSprite>),
}

impl RenderReadAccess<'_> {
    pub(super) fn compile_auxiliary_entries(
        &self,
        debug: &mut Vec<DebugGeometryEntry>,
        lights: &mut Vec<LightEntry>,
    ) {
        debug.clear();
        lights.clear();
        let storage = &self.world.components;
        for &entity in self.world.state.entities.keys() {
            let index = entity.index() as usize;
            // SAFETY: The owning RenderSystem clears these bindings together with
            // render entries before component incarnation or membership changes.
            unsafe {
                let bounds = storage
                    .bounding_geometry_ptr(index)
                    .map(|p| ComponentBinding::new(p));
                let picking = storage
                    .picking_geometry_ptr(index)
                    .map(|p| ComponentBinding::new(p));
                if bounds.is_some() || picking.is_some() {
                    debug.push(DebugGeometryEntry {
                        entity,
                        bounds,
                        picking,
                    });
                }
                if storage.transform(index).is_some()
                    && let Some(light) = storage.light_ptr(index)
                {
                    lights.push(LightEntry {
                        entity,
                        light: ComponentBinding::new(light),
                        model: crate::systems::hierarchy::ObjectTransformBinding::bind(
                            storage, entity,
                        ),
                    });
                }
            }
        }
    }

    pub(super) fn compile_entries(&self, entries: &mut Vec<RenderEntry>) -> bool {
        entries.clear();
        let mut pending_resources = false;
        let storage = &self.world.components;
        for &entity in self.world.state.entities.keys() {
            let index = entity.index() as usize;
            let Some(&transform) = storage.transform(index) else {
                continue;
            };
            let Some(item) = self.compile_render_item(entity, transform) else {
                pending_resources |= self.render_resources_pending(entity);
                continue;
            };
            // SAFETY: RenderSystem clears all entries synchronously before any
            // component destruction/replacement or membership change. Reads borrow
            // this World's storage; no component or asset references escape a phase.
            let entry = unsafe {
                let mut material = if item.solid_fallback {
                    MaterialBinding::Fixed
                } else if let Some(pointer) = storage.pbr_material_ptr(index) {
                    MaterialBinding::Pbr(ComponentBinding::new(pointer))
                } else if let Some(pointer) = storage.unlit_material_ptr(index) {
                    MaterialBinding::Unlit(ComponentBinding::new(pointer))
                } else {
                    MaterialBinding::Fixed
                };
                #[cfg(feature = "particles")]
                if !item.solid_fallback
                    && let Some(pointer) = storage.particle_sprite_ptr(index)
                {
                    material = MaterialBinding::Sprite(ComponentBinding::new(pointer));
                }
                #[cfg(not(feature = "particles"))]
                let _ = &mut material;
                RenderEntry {
                    item,
                    transform: ComponentBinding::new(storage.transform_ptr(index).unwrap()),
                    model: crate::systems::hierarchy::ObjectTransformBinding::bind(storage, entity),
                    material,
                    #[cfg(feature = "mesh-poses")]
                    pose: item
                        .pose
                        .and_then(|_| storage.mesh_pose_ptr(index))
                        .map(|p| ComponentBinding::new(p)),
                    #[cfg(feature = "skeletal-animation")]
                    skin: storage.skin_ptr(index).map(|p| ComponentBinding::new(p)),
                    #[cfg(feature = "particles")]
                    particles: storage.particle_emitter(index).is_some()
                        || storage.particle_playback(index).is_some(),
                }
            };
            entries.push(entry);
        }
        pending_resources
    }
}

impl RenderEntry {
    fn sample(&self, storage: &ComponentStorage, item: &mut RenderItem) -> Option<()> {
        #[cfg(feature = "skeletal-animation")]
        if self.skin.is_some_and(|s| {
            let s = s.get(storage);
            !s.runtime.valid || s.runtime.palette.is_none()
        }) {
            return None;
        }
        item.transform = *self.transform.get(storage);
        let model = self.model.borrow(storage)?.ok()?;
        let matrix = model.render_matrix().ok()?;
        if item.model != matrix {
            item.model = matrix;
            if item.pbr.is_some() || item.custom_material {
                item.normal = model.render_normal_matrix();
            }
        }
        match self.material {
            MaterialBinding::Fixed => {}
            MaterialBinding::Unlit(binding) => item.material = *binding.get(storage),
            MaterialBinding::Pbr(binding) => {
                let pbr = *binding.get(storage);
                item.material = UnlitMaterial {
                    r: pbr.r,
                    g: pbr.g,
                    b: pbr.b,
                };
                item.pbr = Some(pbr);
            }
            #[cfg(feature = "particles")]
            MaterialBinding::Sprite(binding) => {
                let sprite = binding.get(storage);
                item.material = UnlitMaterial {
                    r: sprite.r,
                    g: sprite.g,
                    b: sprite.b,
                };
            }
        }
        #[cfg(feature = "mesh-poses")]
        if let Some(pose) = self.pose {
            item.pose.as_mut().unwrap().1 = pose.get(storage).weight;
        }
        Some(())
    }

    pub(super) fn append(
        &self,
        world: &WorldSimulationState,
        items: &mut Vec<RenderItem>,
        written: &mut usize,
    ) {
        #[cfg(feature = "particles")]
        if self.particles {
            let mut item = self.item;
            if self.sample(&world.components, &mut item).is_some() {
                crate::systems::particles::prepare_particles(world, item, items, written);
            }
            return;
        }
        if items.len() == *written {
            items.push(self.item);
        }
        let item = &mut items[*written];
        if item.entity != self.item.entity {
            *item = self.item;
        }
        if self.sample(&world.components, item).is_some() {
            *written += 1;
        }
    }
}
