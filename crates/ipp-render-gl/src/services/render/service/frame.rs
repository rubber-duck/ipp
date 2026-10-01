//! Frame submission: program demand, draw preparation and debug drawing.

use super::super::assets::{GlMeshData, GlTextureData};
use super::super::frame_scratch::{RenderDrawItem as Item, RenderFrameScratch};
use super::super::frame_statistics::{RenderFrameSummary, RenderFrameWork};
use super::super::scene::{RenderScene, SceneDebug, SceneItem};
use super::super::shader::RenderShaderConfig;
use super::{BACKGROUND, RenderError, RenderService, prepared_model, prepared_normal};
use crate::RenderDevice;
use ipp_core::services::asset_management::AssetKey;
use ipp_core::systems::camera;

impl<D: RenderDevice> RenderService<D> {
    /// Observe only requested exact outputs in an otherwise ordinary selected draw.
    pub fn draw_observed(
        &mut self,
        host: &ipp_core::HostRuntime,
        selection: ipp_core::OutputRef,
        publication: ipp_core::WorldPublicationId,
        viewport: ipp_core::WorldViewport,
        presentation_time: f64,
        outputs: &mut [ipp_core::OutputPublicationObservation],
    ) -> Result<RenderFrameSummary, RenderError> {
        self.inclusions.begin(outputs);
        let result = self.draw(host, selection, publication, viewport, presentation_time);
        let completed = result
            .as_ref()
            .is_ok_and(|summary| !summary.invalid_camera && summary.failed_draw_calls == 0);
        self.inclusions.finish(outputs, completed);
        result
    }

    /// Clear a root whose selected output is unavailable, without any World access.
    pub fn clear(
        &mut self,
        viewport: ipp_core::WorldViewport,
    ) -> Result<RenderFrameSummary, RenderError> {
        if viewport.width == 0
            || viewport.height == 0
            || viewport.width > i32::MAX as u32
            || viewport.height > i32::MAX as u32
        {
            return Err(RenderError::InvalidViewport);
        }

        self.statistics = Default::default();
        self.device
            .borrow_mut()
            .begin_frame(viewport.width, viewport.height, &BACKGROUND)?;
        self.device.borrow_mut().end_frame()?;
        Ok(RenderFrameSummary::default())
    }

    /// Present the Host's current explicit root output publication and viewport.
    /// Mismatched viewports fail before GPU work; nested outputs use attachment authority.
    pub fn draw(
        &mut self,
        host: &ipp_core::HostRuntime,
        selection: ipp_core::OutputRef,
        publication: ipp_core::WorldPublicationId,
        viewport: ipp_core::WorldViewport,
        presentation_time: f64,
    ) -> Result<RenderFrameSummary, RenderError> {
        let (width, height) = (viewport.width, viewport.height);
        self.inclusions.reset_frame();
        if width == 0
            || height == 0
            || width > i32::MAX as u32
            || height > i32::MAX as u32
            || !presentation_time.is_finite()
        {
            return Err(RenderError::InvalidViewport);
        }

        self.validate_catalog(host)?;
        let root = host
            .root_output(selection.world().id())
            .filter(|(current, _, completed)| *current == selection && *completed == publication);
        if root.is_some_and(|(_, current_viewport, _)| current_viewport != viewport) {
            return Err(RenderError::InvalidViewport);
        }

        self.statistics = Default::default();
        let available = root.ok_or(RenderError::UnavailableOutput).and_then(|_| {
            host.output(publication, selection)
                .ok_or(RenderError::UnavailableOutput)
        });
        let prepass = {
            self.camera_completed.clear();
            self.glyph_frame.clear();
            // One Host frame ages the shared atlas once, before every Canvas it presents
            // publishes demand.
            self.glyph_atlas.begin_frame();
            if available.is_ok() {
                self.prepare_camera_children(
                    host,
                    selection,
                    publication,
                    viewport,
                    presentation_time,
                )?
            } else {
                RenderFrameWork::default()
            }
        };
        self.device
            .borrow_mut()
            .begin_frame(width, height, &BACKGROUND)?;
        let result = available.and_then(|_| {
            if selection.kind() == ipp_core::OutputKind::Canvas {
                let scene =
                    super::super::canvas_scene::CanvasScene::new(host, selection, publication)?;
                let [width, height] = scene.canvas.logical_extent;
                let mvp = super::canvas_composition::plane_matrix(
                    [-1.0, 1.0],
                    [2.0 / f64::from(width), -2.0 / f64::from(height)],
                )?;
                let mut work = prepass;
                self.draw_canvas(&scene, mvp, scene.root_clip(), 1.0, viewport, &mut work)?;
                return Ok(work);
            }
            let scene = RenderScene::new(host, selection, publication)?;
            self.debug.retain(&scene.debug);
            let camera = Some(
                scene
                    .camera
                    .prepare(width, height)
                    .map(|camera| camera.view_projection),
            );
            self.draw_items(&scene, &scene.items, camera, prepass, viewport)
        });
        let finish = self.device.borrow_mut().end_frame();
        self.finish_canvas_caches(result.is_ok() && finish.is_ok());
        let mut work = result?;
        finish?;
        self.inclusions.record(selection, publication);
        work.statistics.uploaded_bytes = work
            .statistics
            .uploaded_bytes
            .saturating_add(self.uploads.take());
        self.publish_retained_surface_statistics(&mut work.statistics);
        self.statistics = work.statistics;
        Ok(work.summary)
    }

    pub(super) fn pose_data<'a>(
        &self,
        world: &'a RenderScene<'_>,
        item: &SceneItem<'_>,
    ) -> Result<Option<&'a GlMeshData<D>>, RenderError> {
        item.pose
            .map(|(key, _)| {
                world
                    .resource(AssetKey::from_u64(key.asset))
                    .and_then(|resource| resource.data()?.as_any().downcast_ref::<GlMeshData<D>>())
                    .ok_or(RenderError::MissingMesh)
            })
            .transpose()
    }

    pub(super) fn draw_items(
        &mut self,
        world: &RenderScene<'_>,
        items: &[SceneItem<'_>],
        camera: Option<Result<[f32; 16], ipp_core::ErrorReason>>,
        prepass: RenderFrameWork,
        viewport: ipp_core::WorldViewport,
    ) -> Result<RenderFrameWork, RenderError> {
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(210, "gl.draw");

        let mut scratch = std::mem::take(&mut self.frame_scratch);
        let result =
            self.draw_prepared_items(world, items, camera, prepass, viewport, &mut scratch);
        // Return capacity even after device errors; no borrowed data is retained.
        scratch.clear();
        self.frame_scratch = scratch;
        result
    }

    fn draw_prepared_items(
        &mut self,
        world: &RenderScene<'_>,
        items: &[SceneItem<'_>],
        camera: Option<Result<[f32; 16], ipp_core::ErrorReason>>,
        prepass: RenderFrameWork,
        viewport: ipp_core::WorldViewport,
        scratch: &mut RenderFrameScratch,
    ) -> Result<RenderFrameWork, RenderError> {
        let view_projection = match camera {
            None => {
                self.clear_shadows();
                return Ok(RenderFrameWork::default());
            }
            Some(Ok(view_projection)) => view_projection,
            Some(Err(_)) => {
                // A host resize can make a previously usable projection exceed
                // f32 representation. Preserve the world and correlated replies.
                let mut work = RenderFrameWork::default();
                work.summary.invalid_camera = true;
                return Ok(work);
            }
        };
        if self.particle_quad.is_none()
            && items
                .iter()
                .any(|item| item.particle.is_some_and(|p| p.sprite))
        {
            self.particle_quad = Some(super::super::particles::quad(self.device.clone())?);
        }
        let camera_model = world
            .camera
            .pose
            .render_matrix()
            .map_err(|_| RenderError::InvalidTransform)?;
        let frustum = ipp_core::systems::geometry::frustum_planes(view_projection);
        // Cache repaints ran before `begin_frame`; their draws count here.
        let mut stats = prepass;
        let customs = self.prepare_custom_materials(world, items)?;
        let shadow_capacity = ((self.device.borrow().shadow_map_limit()
            / super::super::lighting::SHADOW_TILE_SIZE)
            .pow(2) as usize)
            .min(self.shadow_capacity_limit);
        let mut lighting = self
            .light_selections
            .entry(world.selection)
            .or_default()
            .prepare(world, items, &customs, &frustum, shadow_capacity)?;
        let result = (|| {
            self.prepare_shadow_storage(&mut lighting)?;
            self.light_selections
                .get_mut(&world.selection)
                .expect("prepared lighting")
                .assign_shadows(&mut lighting);
            stats.statistics.unshadowed_lights = stats.statistics.unshadowed_lights.saturating_add(
                lighting
                    .requested_shadows
                    .saturating_sub(lighting.shadows.len()) as u32,
            );
            if !lighting.shadows.is_empty() {
                self.draw_shadow_map(world, items, &customs, &lighting, &mut stats, scratch)?;
            }

            let debug = &world.debug;
            super::super::draw_order::prepare(
                &mut scratch.draws,
                items,
                debug,
                &world.surfaces,
                &customs,
                &lighting,
                &view_projection,
            );
            let mut draws = scratch
                .draws
                .iter()
                .map(|draw| draw.index.resolve(items, debug, &world.surfaces))
                .peekable();
            // Particle batching consumes lookahead entries.
            while let Some(item) = draws.next() {
                let item = match item {
                    Item::Visual(item) => item,
                    Item::Debug(item) => {
                        self.device.borrow_mut().set_instances(&[])?;
                        self.device.borrow_mut().set_alpha_blend(false)?;
                        self.draw_debug(world, item, view_projection, &mut stats)?;
                        continue;
                    }
                    Item::Surface(surface) => {
                        if !world.visible(surface.entity, &frustum) {
                            continue;
                        }
                        self.device.borrow_mut().set_instances(&[])?;
                        self.draw_output_surface(
                            world.host,
                            surface,
                            view_projection,
                            viewport,
                            &mut stats,
                        )?;
                        continue;
                    }
                };
                let draw_lighting = lighting.draws.get(&item.entity);
                let custom = customs.get(&item.entity);
                let instances = {
                    scratch.instances.clear();
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
                self.device.borrow_mut().set_instances(instances)?;
                let model = prepared_model(item);
                let instance_count = instances.len().max(1) as u32;
                if !custom.is_some_and(|m| m.custom_vertex && !m.material.conservative_bounds) && {
                    if item.particle.is_some() {
                        !super::super::particles::visible(world, item, instances, &frustum)
                    } else {
                        !draw_lighting.map_or_else(
                            || {
                                if lighting.batched {
                                    lighting.visibility.matches(item.entity, 0)
                                } else {
                                    world.visible(item.entity, &frustum)
                                }
                            },
                            |draw| draw.visible,
                        )
                    }
                } {
                    continue;
                }
                let data = world
                    .resource(AssetKey::from_u64(item.mesh.asset))
                    .and_then(|r| r.data()?.as_any().downcast_ref::<GlMeshData<D>>());
                let data = if item.particle.is_some_and(|p| p.sprite) {
                    self.particle_quad.as_ref()
                } else {
                    data
                };
                let Some(data) = data else {
                    stats.failed_draw();
                    continue;
                };
                let asset = &data.mesh;
                let target = self.pose_data(world, item)?;
                let target_gpu = match target {
                    Some(target) => {
                        let Some(gpu) = target.gpu()? else {
                            stats.failed_draw();
                            continue;
                        };
                        Some(gpu)
                    }
                    None => None,
                };
                let Some(gpu) = data.gpu()? else {
                    stats.failed_draw();
                    continue;
                };

                if let Some(custom) = custom {
                    self.upload_custom_material(world, custom, false)?;
                    let program = Self::custom_program(world, custom.key, false)?;
                    self.device
                        .borrow_mut()
                        .set_alpha_blend(custom.material.alpha_mode == 2)?;
                    if item.skinned
                        && let Some(palette) = item.published.palette.as_deref()
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
                        target_gpu
                            .as_deref()
                            .zip(item.pose.map(|(_, weight)| weight)),
                        None,
                    )?;
                    stats.draw((asset.index_count() / 3) as u32 * instance_count);
                    continue;
                }
                self.device.borrow_mut().set_alpha_blend(false)?;
                if let Some(p) = item.particle.filter(|p| p.sprite) {
                    self.device.borrow_mut().set_alpha_blend(true)?;
                    self.device.borrow_mut().set_additive(p.additive)?;
                }

                let texture = item
                    .texture
                    .map(|key| {
                        world
                            .resource(AssetKey::from_u64(key.asset))
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
                        stats.failed_draw();
                        continue;
                    }
                };

                let config = super::super::draw_order::builtin_config(
                    item,
                    draw_lighting.is_some_and(|draw| draw.frame.shadow_count > 0),
                );
                let palette = item
                    .skinned
                    .then_some(item.published.palette.as_deref())
                    .flatten();
                let Some(program) = self.builtin_program(world, config, false) else {
                    stats.failed_draw();
                    continue;
                };

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
                    if let Some(map) = self.shadow_map.as_ref() {
                        self.device.borrow_mut().bind_shadow(program, map, frame)?;
                    }
                }
                self.device.borrow_mut().draw(
                    program,
                    &gpu,
                    &mvp,
                    &material,
                    target_gpu
                        .as_deref()
                        .zip(item.pose.map(|(_, weight)| weight)),
                    texture,
                )?;
                stats.draw((asset.index_count() / 3) as u32 * instance_count);
            }

            Ok(stats)
        })();
        self.light_selections
            .get_mut(&world.selection)
            .expect("prepared lighting")
            .recycle(lighting);
        self.custom_materials = customs;
        result
    }
    fn draw_debug(
        &mut self,
        world: &RenderScene<'_>,
        item: &SceneDebug,
        view_projection: [f32; 16],
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        let config = RenderShaderConfig::default().with_debug_geometry(true);
        let Some(program) = self.builtin_program(world, config, false) else {
            stats.failed_draw();
            return Ok(());
        };
        let (mesh, uploaded) = self.debug.get(&item.geometry)?;
        let Some(mesh) = mesh else {
            stats.failed_draw();
            return Ok(());
        };
        let mvp = camera::multiply(view_projection, item.model);
        self.device.borrow_mut().draw(
            program,
            mesh.gpu.as_ref().expect("live private mesh"),
            &mvp,
            &item.color,
            None,
            None,
        )?;
        stats.uploaded(uploaded);
        stats.draw(mesh.triangles);
        Ok(())
    }
}
