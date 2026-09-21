//! Shadow map allocation and depth-only shadow passes.

use super::super::assets::GlMeshData;
use super::super::frame_scratch::RenderFrameScratch;
use super::super::shader::RenderShaderConfig;
#[cfg(feature = "particles")]
use super::prepared_model;
use super::{RenderError, RenderService, RenderStats, prepared_normal};
use crate::RenderDevice;
use ipp_core::systems::camera;
use ipp_core::{WorldContext, services::asset_management::AssetKey};
use std::collections::BTreeMap;

impl<D: RenderDevice> RenderService<D> {
    pub(super) fn clear_shadows(&mut self) {
        if let Some(map) = self.shadow_map.take() {
            self.device.borrow_mut().delete_shadow_map(map);
        }
    }

    #[cfg(feature = "shadows")]
    pub(super) fn prepare_shadow_storage(
        &mut self,
        lighting: &mut super::super::light_selection::PreparedLighting,
    ) -> Result<(), RenderError> {
        while !lighting.shadows.is_empty() {
            let grid = (lighting.shadows.len() as f64).sqrt().ceil() as u32;
            let size = super::super::lighting::SHADOW_TILE_SIZE * grid;
            if self.shadow_map_size != size {
                self.clear_shadows();
            }
            self.shadow_map_size = size;
            if self.shadow_map.is_some() {
                return Ok(());
            }
            let allocated = self.device.borrow_mut().create_shadow_map(size);
            match allocated {
                Ok(map) => {
                    self.shadow_map = Some(map);
                    return Ok(());
                }
                Err(RenderError::ContextLost) => return Err(RenderError::ContextLost),
                Err(_) => {
                    self.shadow_capacity_limit = (grid.saturating_sub(1)).pow(2) as usize;
                    lighting.shadows.truncate(self.shadow_capacity_limit);
                }
            }
        }
        self.clear_shadows();
        Ok(())
    }

    #[cfg(feature = "shadows")]
    pub(super) fn draw_shadow_map(
        &mut self,
        world: &WorldContext<'_>,
        items: &[ipp_core::RenderItem],
        customs: &BTreeMap<
            ipp_core::EntityId,
            super::super::custom_material::PreparedCustomMaterial,
        >,
        lighting: &super::super::light_selection::PreparedLighting,
        stats: &mut RenderStats,
        scratch: &mut RenderFrameScratch,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(226, "gl.shadow-pass");

        stats.shadow_resident_bytes = self
            .shadow_map_size
            .saturating_mul(self.shadow_map_size)
            .saturating_mul(4);
        use ipp_core::systems::geometry::GeometryBounds;

        let frames = &lighting.shadows;
        scratch.shadow_queries.clear();
        scratch
            .shadow_queries
            .extend(frames.iter().map(|(entity, _)| {
                lighting
                    .shadow_queries
                    .iter()
                    .find(|(light, _)| light == entity)
                    .map_or(0, |(_, query)| *query)
            }));
        scratch.shadow_frusta.clear();
        scratch
            .shadow_frusta
            .extend(frames.iter().map(|(_, frame)| {
                let matrix = frame.shadow_matrices[..16]
                    .try_into()
                    .expect("light matrix");
                ipp_core::systems::geometry::frustum_planes(matrix)
            }));
        scratch
            .shadow_visibility
            .resize(items.len() * frames.len(), true);
        for (item_index, item) in items.iter().enumerate() {
            let custom = customs.get(&item.entity);
            let casts = custom.map_or_else(
                || item.pbr.is_some_and(|m| m.cast_shadows),
                |m| m.material.casts_shadows && m.material.alpha_mode != 2,
            );
            if !casts {
                for view in 0..frames.len() {
                    scratch.shadow_visibility[view * items.len() + item_index] = false;
                }
                continue;
            }
            let unbounded =
                custom.is_some_and(|m| m.custom_vertex && !m.material.conservative_bounds);
            #[cfg(feature = "particles")]
            if item.particle.is_some() {
                for (view, frustum) in scratch.shadow_frusta.iter().enumerate() {
                    scratch.shadow_visibility[view * items.len() + item_index] = unbounded
                        || ((!lighting.batched
                            || lighting
                                .visibility
                                .matches(item.entity, scratch.shadow_queries[view]))
                            && super::super::particles::visible(
                                world,
                                item,
                                &[super::super::particles::instance(
                                    item,
                                    super::super::particles::identity(),
                                )],
                                frustum,
                            ));
                }
                continue;
            }
            // Resolve and prove the enclosure once per object, then test every
            // selected shadow frustum through the same immutable borrow.
            let shape = if unbounded || lighting.batched {
                None
            } else {
                world.culling_geometry(item.entity)
            };
            for (view, frustum) in scratch.shadow_frusta.iter().enumerate() {
                scratch.shadow_visibility[view * items.len() + item_index] = if lighting.batched {
                    unbounded
                        || lighting
                            .visibility
                            .matches(item.entity, scratch.shadow_queries[view])
                } else {
                    shape.is_none_or(|shape| shape.intersects_frustum(frustum))
                };
            }
        }
        for (view, (_, frame)) in frames.iter().enumerate() {
            self.draw_shadow_view(world, items, customs, (view, frame), stats, scratch)?;
        }
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn draw_shadow_view(
        &mut self,
        world: &WorldContext<'_>,
        items: &[ipp_core::RenderItem],
        customs: &BTreeMap<
            ipp_core::EntityId,
            super::super::custom_material::PreparedCustomMaterial,
        >,
        view: (usize, &crate::RenderLightingFrame),
        stats: &mut RenderStats,
        scratch: &mut RenderFrameScratch,
    ) -> Result<(), RenderError> {
        let (view_index, frame) = view;
        let index = 0;
        let matrix: [f32; 16] = frame.shadow_matrices[index * 16..(index + 1) * 16]
            .try_into()
            .expect("light matrix");
        let slot = frame.shadow_settings[index * 4] as u32;
        let grid = frame.shadow_settings[index * 4 + 3] as u32;
        scratch.casters.clear();
        if !ipp_core::render_buffer_reuse_enabled() {
            scratch.casters = Vec::new();
        }
        for (item_index, item) in items.iter().enumerate() {
            let custom = customs.get(&item.entity);
            if !scratch.shadow_visibility[view_index * items.len() + item_index] {
                continue;
            }
            let data = world
                .asset_resources()
                .get(AssetKey::from_u64(item.mesh.asset))
                .and_then(|r| r.data()?.as_any().downcast_ref::<GlMeshData<D>>())
                .ok_or(RenderError::MissingMesh)?;
            let Some(_gpu) = data.gpu()? else {
                stats.failed_draw_calls += 1;
                continue;
            };
            #[cfg(feature = "mesh-poses")]
            let _target = match self.pose_data(world, item)? {
                Some(target) => {
                    let Some(gpu) = target.gpu()? else {
                        stats.failed_draw_calls += 1;
                        continue;
                    };
                    Some(gpu)
                }
                None => None,
            };
            let config = RenderShaderConfig::default();
            #[cfg(feature = "skeletal-animation")]
            let config = config.with_skinning(item.skinned);
            #[cfg(feature = "mesh-poses")]
            let config = config.with_mesh_pose(item.pose.is_some());
            #[cfg(feature = "particles")]
            let config = config.with_particles(item.particle.is_some(), false);
            if custom.is_none() && self.builtin_program(world, config, true).is_none() {
                continue;
            }
            scratch
                .casters
                .push(super::super::frame_scratch::RenderShadowCaster {
                    item: item_index,
                    config,
                });
        }
        let begin = self.device.borrow_mut().begin_shadow(
            self.shadow_map.as_ref().expect("shadow map"),
            slot,
            grid,
        );
        let result = begin.and_then(|()| {
            let mut casters = scratch.casters.iter().peekable();
            #[cfg_attr(not(feature = "particles"), allow(clippy::while_let_on_iterator))]
            while let Some(caster) = casters.next() {
                let item = &items[caster.item];
                let config = caster.config;
                // Assets cannot be invalidated during this immutable World borrow.
                // Keep GPU Ref guards local to the pass, never in retained scratch.
                let data = world
                    .asset_resources()
                    .get(AssetKey::from_u64(item.mesh.asset))
                    .and_then(|r| r.data()?.as_any().downcast_ref::<GlMeshData<D>>())
                    .ok_or(RenderError::MissingMesh)?;
                let gpu = data.gpu()?.ok_or(RenderError::MissingMesh)?;
                #[cfg(feature = "mesh-poses")]
                let target = self.pose_data(world, item)?;
                #[cfg(feature = "mesh-poses")]
                let target = match target {
                    Some(target) => Some(target.gpu()?.ok_or(RenderError::MissingMesh)?),
                    None => None,
                };
                #[cfg(feature = "particles")]
                let model = {
                    if item.particle.is_some() {
                        scratch.instances.clear();
                        if !ipp_core::render_buffer_reuse_enabled() {
                            scratch.instances = Vec::new();
                        }
                        scratch.instances.push(super::super::particles::instance(
                            item,
                            super::super::particles::identity(),
                        ));
                        while let Some(next) = casters.peek() {
                            if items[next.item].entity != item.entity
                                || items[next.item].particle.is_none()
                            {
                                break;
                            }
                            scratch.instances.push(super::super::particles::instance(
                                &items[next.item],
                                super::super::particles::identity(),
                            ));
                            casters.next();
                        }
                        self.device.borrow_mut().set_instances(&scratch.instances)?;
                    } else {
                        self.device.borrow_mut().set_instances(&[])?;
                    }
                    prepared_model(item)
                };
                #[cfg(not(feature = "particles"))]
                let model = &item.model;
                let custom = customs.get(&item.entity);
                let program = if let Some(custom) = custom {
                    self.upload_custom_material(world, custom, true)?;
                    Self::custom_program(world, custom.key, true)?
                } else {
                    self.builtin_program(world, config, true)
                        .expect("prepared shadow program")
                };
                if let Some(custom) = custom {
                    let normal = prepared_normal(item)?;
                    self.device.borrow_mut().set_lighting(
                        program,
                        model,
                        normal,
                        &[0.0, 0.0, 0.0],
                        frame,
                    )?;
                    let _ = custom;
                }
                #[cfg(feature = "skeletal-animation")]
                if item.skinned
                    && let Some(palette) = world.skin_palette(item.entity)
                {
                    self.device
                        .borrow_mut()
                        .set_skin_palette(program, palette)?;
                }
                self.device.borrow_mut().draw(
                    program,
                    &gpu,
                    &camera::multiply(matrix, *model),
                    &[1.0; 3],
                    #[cfg(feature = "mesh-poses")]
                    target.as_deref().zip(item.pose.map(|(_, weight)| weight)),
                    None,
                )?;
                stats.shadow_draw_calls += 1;
            }
            Ok(())
        });
        let finish = self.device.borrow_mut().end_shadow();
        result.and(finish)
    }
}
