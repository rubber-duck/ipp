//! Frame submission: program demand, draw preparation and debug drawing.

use super::super::assets::{GlMeshData, GlTextureData};
use super::super::frame_scratch::{RenderDrawItem as Item, RenderFrameScratch};
use super::super::shader::RenderShaderConfig;
use super::{BACKGROUND, RenderError, RenderService, RenderStats, prepared_model, prepared_normal};
use crate::RenderDevice;
use ipp_core::systems::camera;
use ipp_core::{WorldContext, services::asset_management::AssetKey};

impl<D: RenderDevice> RenderService<D> {
    /// Draw final effective world components using the world's active camera.
    pub fn render(
        &mut self,
        world: &mut WorldContext<'_>,
        width: u32,
        height: u32,
    ) -> Result<RenderStats, RenderError> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(209, "gl.render");

        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(RenderError::InvalidViewport);
        }

        world.set_render_viewport(Some((width, height)));
        self.prepare_programs(world)?;
        // Program demand needs mutable asset access; completed inputs are borrowed
        // only after that phase, through the synchronous draw submission.
        let snapshot =
            (!ipp_core::render_buffer_reuse_enabled()).then(|| world.render_items().to_vec());
        let items = snapshot.as_deref().unwrap_or_else(|| world.render_items());
        #[cfg(feature = "surfaces")]
        let surface_items = world.surface_render_items();
        self.debug.retain(world.debug_render_items());

        // One preparation serves glyph demand and drawing: `None` without a selected
        // camera, an error when it cannot represent this viewport.
        let camera = world.active_camera().map(|_| {
            world
                .prepare_camera(width, height)
                .map(|camera| camera.view_projection)
        });

        // Surface cache presentation is planned before glyph demand, which follows it.
        #[cfg(feature = "surfaces")]
        let planned = match camera {
            Some(Ok(view_projection)) => {
                self.plan_surface_caches(world, surface_items, view_projection)
            }
            _ => Ok(()),
        };
        #[cfg(not(feature = "surfaces"))]
        let planned: Result<(), RenderError> = Ok(());

        #[cfg(feature = "gui")]
        {
            self.glyph_frame.clear();
            // Without a usable camera no Surface is submitted, so the previous demand stays.
            if planned.is_ok()
                && let Some(Ok(view_projection)) = camera
            {
                self.prepare_glyph_demand(world, surface_items, view_projection, (width, height));
            }
        }

        // Frames that repaint cache images populate atlas misses outside the
        // repaints and repaint before `begin_frame`, so the main pass stays whole.
        #[cfg(feature = "surfaces")]
        let early = self.surface_repaints_planned(world.id());
        #[cfg(feature = "surfaces")]
        let prepass = planned.and_then(|()| {
            if !early {
                return Ok(RenderStats::default());
            }

            #[cfg(feature = "gui")]
            self.populate_glyph_misses(world)?;
            let mut instances = std::mem::take(&mut self.frame_scratch.surface_instances);
            let repainted = self.repaint_surface_caches(world, surface_items, &mut instances);
            self.frame_scratch.surface_instances = instances;
            repainted
        });
        #[cfg(not(feature = "surfaces"))]
        let prepass = planned.map(|()| RenderStats::default());

        let begun = prepass.and_then(|stats| {
            self.device
                .borrow_mut()
                .begin_frame(width, height, &BACKGROUND)
                .map(|()| stats)
        });
        let prepass = match begun {
            Ok(stats) => stats,
            Err(error) => {
                #[cfg(feature = "surfaces")]
                self.finish_retained_surfaces(world.id(), surface_items, None);
                #[cfg(feature = "surfaces")]
                self.finish_surface_caches(world, None);
                return Err(error);
            }
        };

        // Populate atlas misses before the main pass, binding each page once.
        #[cfg(feature = "gui")]
        let populated = if early {
            Ok(())
        } else {
            self.populate_glyph_misses(world)
        };
        #[cfg(not(feature = "gui"))]
        let populated = Ok(());
        // Always release draw bindings, including when upload/draw fails.
        #[cfg_attr(not(feature = "surfaces"), allow(unused_mut))]
        let mut result = populated.and_then(|()| {
            self.draw_items(
                world,
                items,
                #[cfg(feature = "surfaces")]
                surface_items,
                camera,
                prepass,
            )
        });
        let finish = self.device.borrow_mut().end_frame();
        #[cfg(feature = "surfaces")]
        self.finish_retained_surfaces(world.id(), surface_items, result.as_mut().ok());
        #[cfg(feature = "surfaces")]
        self.finish_surface_caches(world, result.as_mut().ok());

        let stats = result?;
        finish?;
        Ok(stats)
    }

    #[cfg(feature = "mesh-poses")]
    pub(super) fn pose_data<'a>(
        &self,
        world: &'a WorldContext<'_>,
        item: &ipp_core::RenderItem,
    ) -> Result<Option<&'a GlMeshData<D>>, RenderError> {
        item.pose
            .map(|(key, _)| {
                world
                    .asset_resources()
                    .get(AssetKey::from_u64(key.asset))
                    .and_then(|resource| resource.data()?.as_any().downcast_ref::<GlMeshData<D>>())
                    .ok_or(RenderError::MissingMesh)
            })
            .transpose()
    }

    fn draw_items(
        &mut self,
        world: &WorldContext<'_>,
        items: &[ipp_core::RenderItem],
        #[cfg(feature = "surfaces")] surfaces: &[ipp_core::SurfaceRenderItem],
        camera: Option<Result<[f32; 16], ipp_core::ErrorReason>>,
        prepass: RenderStats,
    ) -> Result<RenderStats, RenderError> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(210, "gl.draw");

        let mut scratch = if ipp_core::render_buffer_reuse_enabled() {
            std::mem::take(&mut self.frame_scratch)
        } else {
            RenderFrameScratch::default()
        };
        let result = self.draw_prepared_items(
            world,
            items,
            #[cfg(feature = "surfaces")]
            surfaces,
            camera,
            prepass,
            &mut scratch,
        );
        // Return capacity even after device errors; no borrowed data is retained.
        scratch.clear();
        self.frame_scratch = scratch;
        result
    }

    fn draw_prepared_items(
        &mut self,
        world: &WorldContext<'_>,
        items: &[ipp_core::RenderItem],
        #[cfg(feature = "surfaces")] surfaces: &[ipp_core::SurfaceRenderItem],
        camera: Option<Result<[f32; 16], ipp_core::ErrorReason>>,
        prepass: RenderStats,
        scratch: &mut RenderFrameScratch,
    ) -> Result<RenderStats, RenderError> {
        // `render` publishes retained GUI residency after every completed frame.
        let view_projection = match camera {
            None => {
                #[cfg(feature = "shadows")]
                self.clear_shadows();
                return Ok(RenderStats {
                    uploaded_bytes: self.uploaded.replace(0),
                    debug_resident_bytes: self.debug.resident_bytes() as u32,
                    ..RenderStats::default()
                });
            }
            Some(Ok(view_projection)) => view_projection,
            Some(Err(_)) => {
                // A host resize can make a previously usable projection exceed
                // f32 representation. Preserve the world and correlated replies.
                return Ok(RenderStats {
                    uploaded_bytes: self.uploaded.replace(0),
                    invalid_camera: true,
                    debug_resident_bytes: self.debug.resident_bytes() as u32,
                    ..RenderStats::default()
                });
            }
        };
        // From here every visible Surface reaches submission unless the frame fails.
        #[cfg(feature = "surfaces")]
        {
            self.submitted_surfaces = Some(Default::default());
        }
        #[cfg(feature = "particles")]
        if self.particle_quad.is_none()
            && items
                .iter()
                .any(|item| item.particle.is_some_and(|p| p.sprite))
        {
            self.particle_quad = Some(super::super::particles::quad(self.device.clone())?);
        }
        #[cfg(feature = "particles")]
        let camera_model = world
            .world_matrix(world.active_camera().unwrap())
            .map_err(|_| RenderError::InvalidTransform)?;
        let frustum = ipp_core::systems::geometry::frustum_planes(view_projection);
        // Cache repaints ran before `begin_frame`; their draws count here.
        let mut stats = prepass;
        let customs = self.prepare_custom_materials(world, items)?;
        #[cfg(feature = "shadows")]
        let shadow_capacity = ((self.device.borrow().shadow_map_limit()
            / super::super::lighting::SHADOW_TILE_SIZE)
            .pow(2) as usize)
            .min(self.shadow_capacity_limit);
        #[cfg(not(feature = "shadows"))]
        let shadow_capacity = 0;
        let mut lighting = self
            .light_selections
            .entry(world.id())
            .or_default()
            .prepare(world, items, &customs, &frustum, shadow_capacity)?;
        let result = (|| {
            #[cfg(feature = "shadows")]
            self.prepare_shadow_storage(&mut lighting)?;
            self.light_selections
                .get_mut(&world.id())
                .expect("prepared lighting")
                .assign_shadows(&mut lighting);
            stats.unshadowed_lights = lighting
                .requested_shadows
                .saturating_sub(lighting.shadows.len())
                as u32;
            #[cfg(feature = "shadows")]
            if !lighting.shadows.is_empty() {
                self.draw_shadow_map(world, items, &customs, &lighting, &mut stats, scratch)?;
            }

            let debug = world.debug_render_items();
            super::super::draw_order::prepare(
                &mut scratch.draws,
                items,
                debug,
                #[cfg(feature = "surfaces")]
                surfaces,
                &customs,
                &lighting,
                &view_projection,
            );
            let mut draws = scratch
                .draws
                .iter()
                .map(|draw| {
                    draw.index.resolve(
                        items,
                        debug,
                        #[cfg(feature = "surfaces")]
                        surfaces,
                    )
                })
                .peekable();
            // Particle batching consumes lookahead entries; lean builds retain the same loop.
            #[cfg_attr(not(feature = "particles"), allow(clippy::while_let_on_iterator))]
            while let Some(item) = draws.next() {
                let item = match item {
                    Item::Visual(item) => item,
                    Item::Debug(item) => {
                        #[cfg(feature = "particles")]
                        self.device.borrow_mut().set_instances(&[])?;
                        self.device.borrow_mut().set_alpha_blend(false)?;
                        self.draw_debug(world, item, view_projection, &mut stats)?;
                        continue;
                    }
                    #[cfg(feature = "surfaces")]
                    Item::Surface(item) => {
                        if !world.geometry_visible(item.entity, &frustum) {
                            continue;
                        }
                        // Cached Surfaces composite their image at this painter-order slot.
                        if self.composite_surface_cache(world, item, view_projection, &mut stats)? {
                            continue;
                        }
                        if let Some(submitted) = &mut self.submitted_surfaces {
                            submitted.insert(item.entity);
                        }
                        self.draw_surface(
                            world,
                            item,
                            camera::multiply(view_projection, item.model),
                            &mut stats,
                            &mut scratch.surface_instances,
                        )?;
                        continue;
                    }
                };
                let draw_lighting = lighting.draws.get(&item.entity);
                let custom = customs.get(&item.entity);
                #[cfg(feature = "particles")]
                let instances = {
                    scratch.instances.clear();
                    if !ipp_core::render_buffer_reuse_enabled() {
                        scratch.instances = Vec::new();
                    }
                    if item.particle.is_some() {
                        scratch
                            .instances
                            .push(super::super::particles::instance(item, camera_model));
                        while let Some(Item::Visual(next)) = draws.peek() {
                            if next.entity != item.entity || next.particle.is_none() {
                                break;
                            }
                            scratch
                                .instances
                                .push(super::super::particles::instance(next, camera_model));
                            draws.next();
                        }
                    }
                    &scratch.instances
                };
                #[cfg(feature = "particles")]
                self.device.borrow_mut().set_instances(instances)?;
                let model = prepared_model(item);
                #[cfg(feature = "particles")]
                let instance_count = instances.len().max(1) as u32;
                #[cfg(not(feature = "particles"))]
                let instance_count = 1;
                if !custom.is_some_and(|m| m.custom_vertex && !m.material.conservative_bounds) && {
                    #[cfg(feature = "particles")]
                    if item.particle.is_some() {
                        !super::super::particles::visible(world, item, instances, &frustum)
                    } else {
                        !draw_lighting.map_or_else(
                            || {
                                if lighting.batched {
                                    lighting.visibility.matches(item.entity, 0)
                                } else {
                                    world.geometry_visible(item.entity, &frustum)
                                }
                            },
                            |draw| draw.visible,
                        )
                    }
                    #[cfg(not(feature = "particles"))]
                    {
                        !draw_lighting.map_or_else(
                            || {
                                if lighting.batched {
                                    lighting.visibility.matches(item.entity, 0)
                                } else {
                                    world.geometry_visible(item.entity, &frustum)
                                }
                            },
                            |draw| draw.visible,
                        )
                    }
                } {
                    continue;
                }
                let data = world
                    .asset_resources()
                    .get(AssetKey::from_u64(item.mesh.asset))
                    .and_then(|r| r.data()?.as_any().downcast_ref::<GlMeshData<D>>());
                #[cfg(feature = "particles")]
                let data = if item.particle.is_some_and(|p| p.sprite) {
                    self.particle_quad.as_ref()
                } else {
                    data
                };
                let Some(data) = data else {
                    stats.failed_draw_calls += 1;
                    continue;
                };
                let asset = &data.mesh;
                #[cfg(feature = "mesh-poses")]
                let target = self.pose_data(world, item)?;
                #[cfg(feature = "mesh-poses")]
                let target_gpu = match target {
                    Some(target) => {
                        let Some(gpu) = target.gpu()? else {
                            stats.failed_draw_calls += 1;
                            continue;
                        };
                        Some(gpu)
                    }
                    None => None,
                };
                let Some(gpu) = data.gpu()? else {
                    stats.failed_draw_calls += 1;
                    continue;
                };

                if let Some(custom) = custom {
                    self.upload_custom_material(world, custom, false)?;
                    let program = Self::custom_program(world, custom.key, false)?;
                    self.device
                        .borrow_mut()
                        .set_alpha_blend(custom.material.alpha_mode == 2)?;
                    #[cfg(feature = "skeletal-animation")]
                    if item.skinned
                        && let Some(palette) = world.skin_palette(item.entity)
                    {
                        self.device
                            .borrow_mut()
                            .set_skin_palette(program, palette)?;
                    }
                    let normal = prepared_normal(item)?;
                    let frame = draw_lighting.map_or(&lighting.unlit, |draw| &draw.frame);
                    self.device.borrow_mut().set_lighting(
                        program,
                        model,
                        normal,
                        &[0.0, 0.0, f32::from(custom.material.receives_shadows)],
                        frame,
                    )?;
                    #[cfg(feature = "shadows")]
                    if custom.material.receives_light
                        && custom.material.receives_shadows
                        && let Some(map) = &self.shadow_map
                    {
                        self.device.borrow_mut().bind_shadow(program, map, frame)?;
                    }
                    self.device.borrow_mut().draw(
                        program,
                        &gpu,
                        &camera::multiply(view_projection, *model),
                        &[1.0; 3],
                        #[cfg(feature = "mesh-poses")]
                        target_gpu
                            .as_deref()
                            .zip(item.pose.map(|(_, weight)| weight)),
                        None,
                    )?;
                    stats.draw_calls += 1;
                    stats.triangles += (asset.index_count() / 3) as u32 * instance_count;
                    continue;
                }
                self.device.borrow_mut().set_alpha_blend(false)?;
                #[cfg(feature = "particles")]
                if let Some(p) = item.particle.filter(|p| p.sprite) {
                    self.device.borrow_mut().set_alpha_blend(true)?;
                    self.device.borrow_mut().set_additive(p.additive)?;
                }

                let texture = item
                    .texture
                    .map(|key| {
                        world
                            .asset_resources()
                            .get(AssetKey::from_u64(key.asset))
                            .and_then(|resource| {
                                resource.data()?.as_any().downcast_ref::<GlTextureData<D>>()
                            })
                            .and_then(|data| data.gpu.as_ref())
                            .ok_or(RenderError::MissingTexture)
                    })
                    .transpose();
                let texture = match texture {
                    Ok(texture) => texture,
                    Err(_) => {
                        stats.failed_draw_calls += 1;
                        continue;
                    }
                };

                let config = super::super::draw_order::builtin_config(
                    item,
                    draw_lighting.is_some_and(|draw| draw.frame.shadow_count > 0),
                );
                #[cfg(feature = "skeletal-animation")]
                let palette = item
                    .skinned
                    .then(|| world.skin_palette(item.entity))
                    .flatten();
                let Some(program) = self.builtin_program(world, config, false) else {
                    stats.failed_draw_calls += 1;
                    continue;
                };

                #[cfg(feature = "skeletal-animation")]
                if let Some(palette) = palette {
                    self.device
                        .borrow_mut()
                        .set_skin_palette(program, palette)?;
                }
                let mvp = camera::multiply(view_projection, *model);
                let material = [item.material.r, item.material.g, item.material.b];
                if let Some(material) = item.pbr {
                    let frame = &draw_lighting.expect("prepared lit draw").frame;
                    let normal = if item.normals {
                        prepared_normal(item)?
                    } else {
                        &[0.0; 16]
                    };
                    self.device.borrow_mut().set_lighting(
                        program,
                        model,
                        normal,
                        &[
                            material.metallic,
                            material.roughness,
                            f32::from(material.receive_shadows),
                        ],
                        frame,
                    )?;
                    #[cfg(feature = "shadows")]
                    if let Some(map) = self.shadow_map.as_ref() {
                        self.device.borrow_mut().bind_shadow(program, map, frame)?;
                    }
                }
                self.device.borrow_mut().draw(
                    program,
                    &gpu,
                    &mvp,
                    &material,
                    #[cfg(feature = "mesh-poses")]
                    target_gpu
                        .as_deref()
                        .zip(item.pose.map(|(_, weight)| weight)),
                    texture,
                )?;
                stats.draw_calls += 1;
                stats.triangles += (asset.index_count() / 3) as u32 * instance_count;
            }

            stats.uploaded_bytes = stats
                .uploaded_bytes
                .saturating_add(self.uploaded.replace(0));

            stats.debug_resident_bytes = self.debug.resident_bytes() as u32;

            Ok(stats)
        })();
        self.light_selections
            .get_mut(&world.id())
            .expect("prepared lighting")
            .recycle(lighting);
        self.custom_materials = customs;
        result
    }
    fn draw_debug(
        &mut self,
        world: &WorldContext<'_>,
        item: &ipp_core::DebugRenderItem,
        view_projection: [f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        let config = RenderShaderConfig::default().with_debug_geometry(true);
        let Some(program) = self.builtin_program(world, config, false) else {
            stats.failed_draw_calls += 1;
            return Ok(());
        };
        let (mesh, uploaded) = self.debug.get(&item.geometry)?;
        let Some(mesh) = mesh else {
            stats.failed_draw_calls += 1;
            return Ok(());
        };
        let mvp = camera::multiply(view_projection, item.model);
        self.device.borrow_mut().draw(
            program,
            mesh.gpu.as_ref().expect("live private mesh"),
            &mvp,
            &item.color,
            #[cfg(feature = "mesh-poses")]
            None,
            None,
        )?;
        stats.uploaded_bytes = stats.uploaded_bytes.saturating_add(uploaded);
        stats.draw_calls += 1;
        stats.triangles += mesh.triangles;
        Ok(())
    }
}
