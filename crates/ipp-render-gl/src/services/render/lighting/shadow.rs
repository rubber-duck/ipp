//! Shadow map allocation and depth-only shadow passes.

use super::super::assets::loaders::GlMeshData;
use super::super::frame::scratch::RenderFrameScratch;
use super::super::materials::shader::RenderShaderConfig;
use super::super::outputs::scene::{RenderEntity, RenderScene, SceneItem};
use super::super::service::{prepared_model, prepared_normal};
use super::super::statistics::RenderFrameWork;
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::math;
use ipp_core::services::asset_management::AssetKey;
use std::collections::BTreeMap;

impl<D: RenderDevice> RenderService<D> {
    pub(in crate::services::render) fn clear_shadows(&mut self) {
        if let Some(map) = self.shadow_map.take() {
            self.device.borrow_mut().delete_shadow_map(map);
        }
    }

    pub(in crate::services::render) fn prepare_shadow_storage(
        &mut self,
        lighting: &mut super::selection::PreparedLighting,
    ) -> Result<(), RenderError> {
        while !lighting.shadows.is_empty() {
            let grid = (lighting.shadows.len() as f64).sqrt().ceil() as u32;
            let size = super::lights::SHADOW_TILE_SIZE * grid;
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

    pub(in crate::services::render) fn draw_shadow_map(
        &mut self,
        world: &RenderScene<'_>,
        items: &[SceneItem<'_>],
        customs: &BTreeMap<
            RenderEntity,
            super::super::materials::custom_material::PreparedCustomMaterial,
        >,
        lighting: &super::selection::PreparedLighting,
        stats: &mut RenderFrameWork,
        scratch: &mut RenderFrameScratch,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(226, "gl.shadow-pass");

        stats.statistics.shadow_resident_bytes = self
            .shadow_map_size
            .saturating_mul(self.shadow_map_size)
            .saturating_mul(4);

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
            if item.particle.is_some() {
                for (view, frustum) in scratch.shadow_frusta.iter().enumerate() {
                    scratch.shadow_visibility[view * items.len() + item_index] = unbounded
                        || ((!lighting.batched
                            || lighting
                                .visibility
                                .matches(item.entity, scratch.shadow_queries[view]))
                            && super::super::frame::particles::visible(
                                world,
                                item,
                                &[super::super::frame::particles::instance(
                                    item,
                                    super::super::frame::particles::identity(),
                                )],
                                frustum,
                            ));
                }
                continue;
            }
            for (view, frustum) in scratch.shadow_frusta.iter().enumerate() {
                scratch.shadow_visibility[view * items.len() + item_index] =
                    unbounded || world.visible(item.entity, frustum);
            }
        }
        for (view, (_, frame)) in frames.iter().enumerate() {
            self.draw_shadow_view(world, items, customs, (view, frame), stats, scratch)?;
        }
        Ok(())
    }

    fn draw_shadow_view(
        &mut self,
        world: &RenderScene<'_>,
        items: &[SceneItem<'_>],
        customs: &BTreeMap<
            RenderEntity,
            super::super::materials::custom_material::PreparedCustomMaterial,
        >,
        view: (usize, &crate::RenderLightingFrame),
        stats: &mut RenderFrameWork,
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
        for (item_index, item) in items.iter().enumerate() {
            let custom = customs.get(&item.entity);
            if !scratch.shadow_visibility[view_index * items.len() + item_index] {
                continue;
            }
            let data = world
                .resource(AssetKey::from_u64(item.mesh.asset))
                .and_then(|r| r.data()?.as_any().downcast_ref::<GlMeshData<D>>())
                .ok_or(RenderError::MissingMesh)?;
            let Some(_gpu) = data.gpu()? else {
                stats.failed_draw();
                continue;
            };
            let _target = match self.pose_data(world, item)? {
                Some(target) => {
                    let Some(gpu) = target.gpu()? else {
                        stats.failed_draw();
                        continue;
                    };
                    Some(gpu)
                }
                None => None,
            };
            let config = RenderShaderConfig::default();
            let config = config.with_skinning(item.skinned);
            let config = config.with_mesh_pose(item.pose.is_some());
            let config = config.with_particles(item.particle.is_some(), false);
            if custom.is_none() && self.builtin_program(world, config, true).is_none() {
                continue;
            }
            scratch
                .casters
                .push(super::super::frame::scratch::RenderShadowCaster {
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
            while let Some(caster) = casters.next() {
                let item = &items[caster.item];
                let config = caster.config;
                // Assets cannot be invalidated during this immutable World borrow.
                // Keep GPU Ref guards local to the pass, never in retained scratch.
                let data = world
                    .resource(AssetKey::from_u64(item.mesh.asset))
                    .and_then(|r| r.data()?.as_any().downcast_ref::<GlMeshData<D>>())
                    .ok_or(RenderError::MissingMesh)?;
                let gpu = data.gpu()?.ok_or(RenderError::MissingMesh)?;
                let target = self.pose_data(world, item)?;
                let target = match target {
                    Some(target) => Some(target.gpu()?.ok_or(RenderError::MissingMesh)?),
                    None => None,
                };
                let model = {
                    if item.particle.is_some() {
                        scratch.instances.clear();
                        scratch
                            .instances
                            .push(super::super::frame::particles::instance(
                                item,
                                super::super::frame::particles::identity(),
                            ));
                        while let Some(next) = casters.peek() {
                            if items[next.item].entity != item.entity
                                || items[next.item].particle.is_none()
                            {
                                break;
                            }
                            scratch
                                .instances
                                .push(super::super::frame::particles::instance(
                                    &items[next.item],
                                    super::super::frame::particles::identity(),
                                ));
                            casters.next();
                        }
                        self.device.borrow_mut().set_instances(&scratch.instances)?;
                    } else {
                        self.device.borrow_mut().set_instances(&[])?;
                    }
                    prepared_model(item)
                };
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
                if item.skinned
                    && let Some(palette) = item.published.palette.as_deref()
                {
                    self.device
                        .borrow_mut()
                        .set_skin_palette(program, palette)?;
                }
                self.device.borrow_mut().draw(
                    program,
                    &gpu,
                    &math::multiply(matrix, *model),
                    &[1.0; 3],
                    target.as_deref().zip(item.pose.map(|(_, weight)| weight)),
                    None,
                )?;
                stats.statistics.shadow_draw_calls += 1;
            }
            Ok(())
        });
        let finish = self.device.borrow_mut().end_shadow();
        result.and(finish)
    }
}
