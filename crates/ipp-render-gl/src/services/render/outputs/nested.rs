//! Child-first Camera and optional Canvas targets; root-only display conversion.

use super::super::canvas::layering::{CanvasChild, canvas_attachment, plane_matrix};
use super::super::canvas::scene::CanvasScene;
use super::super::service::BACKGROUND;
use super::super::statistics::RenderFrameWork;
use super::super::surface::cache_refresh::CanvasCacheRequest;
use super::scene::{RenderScene, world_interaction_eligible};
use crate::RenderService;
use ipp_core::systems::canvas::CanvasClip;

#[derive(Clone, Copy)]
struct CanvasView {
    plane: [f32; 16],
    clip: CanvasClip,
    opacity: f32,
    camera: Option<[f64; 3]>,
    raster_mvp: [f32; 16],
    raster_viewport: WorldViewport,
}

struct OutputJob {
    selection: OutputRef,
    publication: WorldPublicationId,
    viewport: WorldViewport,
    projection_extent: [f64; 2],
    canvas: Option<CanvasView>,
    cache: Option<CanvasCacheRequest>,
    projected: Option<super::scene::SceneOutputSurface>,
    projected_mvp: [f32; 16],
    projected_viewport: WorldViewport,
    visible: bool,
    interaction_eligible: bool,
}

fn distance(camera: Option<[f64; 3]>, plane: [f32; 16], extent: [f32; 2]) -> f32 {
    let Some(camera) = camera else {
        return 0.0;
    };

    let centre = [
        plane[12] + plane[0] * extent[0] * 0.5 + plane[4] * extent[1] * 0.5,
        plane[13] + plane[1] * extent[0] * 0.5 + plane[5] * extent[1] * 0.5,
        plane[14] + plane[2] * extent[0] * 0.5 + plane[6] * extent[1] * 0.5,
    ];
    centre
        .iter()
        .zip(camera)
        .map(|(value, camera)| (f64::from(*value) - camera).powi(2))
        .sum::<f64>()
        .sqrt() as f32
}

use crate::{RenderDevice, RenderError};
use ipp_core::{HostRuntime, OutputKind, OutputRef, WorldPublicationId, WorldViewport};

impl<D: RenderDevice> RenderService<D> {
    pub(in crate::services::render) fn prepare_camera_children(
        &mut self,
        host: &HostRuntime,
        selection: OutputRef,
        publication: WorldPublicationId,
        viewport: WorldViewport,
        presentation_time: f64,
    ) -> Result<RenderFrameWork, RenderError> {
        let canvas = if selection.kind() == OutputKind::Canvas {
            let scene = CanvasScene::new(host, selection, publication)?;
            Some(CanvasView {
                plane: plane_matrix([0.0; 2], [1.0; 2])?,
                clip: scene.root_clip(),
                opacity: 1.0,
                camera: None,
                raster_mvp: plane_matrix(
                    [-1.0, 1.0],
                    [
                        2.0 / f64::from(scene.canvas.logical_extent[0]),
                        -2.0 / f64::from(scene.canvas.logical_extent[1]),
                    ],
                )?,
                raster_viewport: viewport,
            })
        } else {
            None
        };

        let mut pending = vec![(
            OutputJob {
                selection,
                publication,
                viewport,
                projection_extent: [f64::from(viewport.width), f64::from(viewport.height)],
                canvas,
                cache: None,
                projected: None,
                projected_mvp: [0.0; 16],
                projected_viewport: viewport,
                visible: true,
                interaction_eligible: world_interaction_eligible(host, selection.world()),
            },
            false,
        )];
        let mut prepared = Vec::new();
        let mut interactive = std::collections::BTreeSet::new();
        while let Some((job, visited)) = pending.pop() {
            if visited {
                if job.selection != selection {
                    prepared.push(job);
                }
                continue;
            }

            let mut children = Vec::new();
            match job.selection.kind() {
                OutputKind::Camera => {
                    let scene = RenderScene::new(host, job.selection, job.publication)?;
                    let camera = Some(scene.camera.pose.point([0.0; 3]));
                    let prepared_camera =
                        scene.camera.prepare_for_extent(job.projection_extent).ok();
                    let frustum = prepared_camera.as_ref().map(|camera| {
                        ipp_core::systems::geometry::frustum_planes(camera.view_projection)
                    });
                    for surface in &scene.surfaces {
                        let interaction_eligible = job.interaction_eligible
                            && surface.interaction_eligible
                            && world_interaction_eligible(host, surface.selection.world());
                        let visible = job.visible
                            && frustum
                                .as_ref()
                                .is_none_or(|planes| scene.surface_visible(surface, planes));
                        let Some(camera_view) = prepared_camera.as_ref() else {
                            continue;
                        };
                        let projected_mvp =
                            ipp_core::math::multiply(camera_view.view_projection, surface.model);
                        let pixel_demand = super::super::surface::quality::surface_demand(
                            &*surface.geometry,
                            &[0.0, f64::from(surface.layer_depth)],
                            projected_mvp,
                            job.viewport,
                        )?;
                        let Some(size) = super::super::surface::quality::image_size(
                            pixel_demand,
                            surface.cache_policy.map_or(1.0, |p| p.resolution_scale),
                            self.device.borrow().surface_cache_limit(),
                            (surface.selection.kind() == OutputKind::Camera)
                                .then(|| {
                                    self.camera_targets
                                        .get(&surface.selection)
                                        .map(|(_, size, _)| *size)
                                })
                                .flatten(),
                        ) else {
                            continue;
                        };
                        let size = if surface.geometry.exact_affine(0.0).is_none() {
                            self.projected_raster_size(host, surface, projected_mvp, job.viewport)?
                        } else {
                            size
                        };
                        let viewport = WorldViewport {
                            width: size[0],
                            height: size[1],
                            ..job.viewport
                        };

                        let mut canvas = None;
                        let mut cache = None;
                        if surface.selection.kind() == OutputKind::Canvas {
                            let child =
                                CanvasScene::new(host, surface.selection, surface.publication)?;
                            let extent = child.canvas.logical_extent;
                            let [width, height] = surface.extent.map(f64::from);
                            let content = if surface.geometry.exact_affine(0.0).is_some() {
                                ipp_core::math::multiply(
                                    super::super::canvas::layering::affine_matrix(
                                        &*surface.geometry,
                                    )?,
                                    plane_matrix(
                                        [0.0; 2],
                                        [
                                            width / f64::from(extent[0]),
                                            height / f64::from(extent[1]),
                                        ],
                                    )?,
                                )
                            } else {
                                // Only a quality estimate for descendants: projected
                                // content itself uses sampled geometry, never this plane.
                                plane_matrix(
                                    [-width * 0.5, height * 0.5],
                                    [width / f64::from(extent[0]), -height / f64::from(extent[1])],
                                )?
                            };
                            let plane = ipp_core::math::multiply(surface.model, content);
                            let clip = child.root_clip();
                            canvas = Some(CanvasView {
                                plane,
                                clip,
                                opacity: 1.0,
                                camera,
                                raster_mvp: plane_matrix(
                                    [-1.0, 1.0],
                                    [2.0 / f64::from(extent[0]), -2.0 / f64::from(extent[1])],
                                )?,
                                raster_viewport: viewport,
                            });
                            cache = surface
                                .cache_policy
                                .filter(|_| surface.geometry.exact_affine(0.0).is_some())
                                .map(|policy| CanvasCacheRequest {
                                    owner: surface.entity.world,
                                    anchor: surface.entity.entity,
                                    token: surface.token.clone(),
                                    pixel_demand,
                                    policy,
                                    distance: distance(camera, plane, extent),
                                    clip,
                                    opacity: 1.0,
                                    visible,
                                    interaction: false,
                                    layered: surface.layered(),
                                });
                        }
                        children.push(OutputJob {
                            selection: surface.selection,
                            publication: surface.publication,
                            viewport,
                            projection_extent: surface.extent.map(f64::from),
                            canvas,
                            cache,
                            projected: surface
                                .geometry
                                .exact_affine(0.0)
                                .is_none()
                                .then(|| surface.clone()),
                            projected_mvp,
                            projected_viewport: job.viewport,
                            visible,
                            interaction_eligible,
                        });
                    }
                }
                OutputKind::Canvas => {
                    let scene = CanvasScene::new(host, job.selection, job.publication)?;
                    if job.interaction_eligible && scene.canvas.interaction.requires_direct() {
                        interactive.insert(job.selection);
                    }
                    let view = job.canvas.expect("Canvas view");
                    for (index, entry) in scene.canvas.entries.iter().enumerate() {
                        let ipp_core::systems::canvas::CanvasPaintEntry::Attachment(slot) =
                            entry.as_ref()
                        else {
                            continue;
                        };

                        let attachment = scene.attachment(slot);
                        let Some((selection, publication)) = attachment.output() else {
                            continue;
                        };

                        let Some(child) =
                            canvas_attachment(&scene, index, view.plane, view.clip, view.opacity)?
                        else {
                            continue;
                        };

                        let Some(raster_child) = canvas_attachment(
                            &scene,
                            index,
                            view.raster_mvp,
                            view.clip,
                            view.opacity,
                        )?
                        else {
                            continue;
                        };
                        let raster_mvp = match raster_child {
                            CanvasChild::Canvas {
                                mvp,
                                ..
                            }
                            | CanvasChild::Camera {
                                mvp,
                                ..
                            } => mvp,
                        };
                        let logical_extent = match &child {
                            CanvasChild::Canvas {
                                scene,
                                ..
                            } => scene.canvas.logical_extent,
                            CanvasChild::Camera {
                                extent,
                                ..
                            } => *extent,
                        };
                        let pixel_demand = super::super::surface::quality::plane_demand(
                            logical_extent,
                            raster_mvp,
                            view.raster_viewport,
                        )?;
                        let policy = attachment.edge.and_then(|edge| edge.surface_cache_policy);
                        let Some(size) = super::super::surface::quality::image_size(
                            pixel_demand,
                            policy.map_or(1.0, |p| p.resolution_scale),
                            self.device.borrow().surface_cache_limit(),
                            (selection.kind() == OutputKind::Camera)
                                .then(|| {
                                    self.camera_targets
                                        .get(&selection)
                                        .map(|(_, size, _)| *size)
                                })
                                .flatten(),
                        ) else {
                            continue;
                        };
                        let viewport = WorldViewport {
                            width: size[0],
                            height: size[1],
                            ..job.viewport
                        };
                        let (canvas, cache) = match child {
                            CanvasChild::Canvas {
                                scene: child,
                                mvp: plane,
                                clip,
                                opacity,
                            } => {
                                let canvas = Some(CanvasView {
                                    plane,
                                    clip,
                                    opacity,
                                    camera: view.camera,
                                    raster_mvp,
                                    raster_viewport: view.raster_viewport,
                                });

                                let cache = attachment
                                    .edge
                                    .and_then(|edge| edge.surface_cache_policy)
                                    .map(|policy| CanvasCacheRequest {
                                        owner: scene.publication.world,
                                        anchor: slot.anchor,
                                        token: slot.token.clone(),
                                        pixel_demand,
                                        policy,
                                        distance: distance(
                                            view.camera,
                                            plane,
                                            child.canvas.logical_extent,
                                        ),
                                        clip,
                                        opacity,
                                        visible: job.visible,
                                        interaction: false,
                                        // A nested slot presents on one plane.
                                        layered: false,
                                    });
                                (canvas, cache)
                            }
                            CanvasChild::Camera {
                                ..
                            } => (None, None),
                        };
                        children.push(OutputJob {
                            selection,
                            publication,
                            viewport,
                            projection_extent: slot.physical_extent,
                            canvas,
                            cache,
                            projected: None,
                            projected_mvp: [0.0; 16],
                            projected_viewport: viewport,
                            visible: job.visible,
                            interaction_eligible: job.interaction_eligible
                                && world_interaction_eligible(host, selection.world()),
                        });
                    }
                }
            }
            pending.push((job, true));
            for child in children.into_iter().rev() {
                pending.push((child, false));
            }
        }

        let mut requests = Vec::new();
        for job in &prepared {
            if let Some(mut cache) = job.cache.clone() {
                cache.interaction = super::super::canvas::scene::output_order(
                    host,
                    job.selection,
                    job.publication,
                )?
                .iter()
                .any(|(output, _)| interactive.contains(output));
                requests.push((job.selection, job.publication, cache));
            }
        }
        let required = prepared
            .iter()
            .filter_map(|job| job.projected.as_ref().map(|s| s.selection))
            .collect();
        self.retain_projected_outputs(&required);
        let visible = prepared
            .iter()
            .filter(|job| job.visible)
            .map(|job| job.selection)
            .collect();
        self.begin_projected_frame(presentation_time, &visible);
        // Reserve visible required active images before optional admission. Idle
        // optional capacity and padding must not lower required raster quality.
        let mut required_bytes = 0usize;
        self.required_image_demand.clear();
        for job in prepared.iter().filter(|job| job.visible) {
            let count = if job.selection.kind() == OutputKind::Camera {
                1
            } else if let Some(surface) = &job.projected {
                if surface.layer_spacing == 0.0 {
                    1
                } else {
                    let scene = CanvasScene::new(host, job.selection, job.publication)?;
                    scene
                        .canvas
                        .entries
                        .iter()
                        .map(|entry| entry.layer())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        .max(1)
                }
            } else {
                0
            };
            let bytes = count * job.viewport.width as usize * job.viewport.height as usize * 4;
            self.required_image_demand.insert(job.selection, bytes);
            required_bytes = required_bytes.saturating_add(bytes);
        }
        if required_bytes > self.surface_cache.budget() {
            // Active quality cannot all fit. Reserve bounded proportional shares
            // so future requests allow downshifts instead of excluding every image.
            let ratio = self.surface_cache.budget() as f64 / required_bytes as f64;
            for bytes in self.required_image_demand.values_mut() {
                *bytes = ((*bytes as f64 * ratio) as usize / 4 * 4).max(4);
            }
        }
        self.reclaim_inactive_images();
        self.reclaim_required_padding()?;
        self.plan_canvas_caches(
            host,
            selection,
            &requests,
            presentation_time,
            required_bytes,
        )?;
        let mut work = RenderFrameWork::default();
        for job in prepared {
            if !job.visible {
                continue;
            }
            let selection = job.selection;
            if selection.kind() == OutputKind::Canvas {
                let scene = CanvasScene::new(host, selection, job.publication)?;
                if let Some(surface) = job.projected {
                    let interaction = super::super::canvas::scene::output_order(
                        host,
                        selection,
                        job.publication,
                    )?
                    .iter()
                    .any(|(output, _)| interactive.contains(output));
                    let result = self.prepare_projected_surface(
                        host,
                        surface,
                        job.projected_mvp,
                        job.projected_viewport,
                        presentation_time,
                        interaction,
                        &mut work,
                    );
                    if result == Err(RenderError::ContextLost) {
                        return result.map(|_| work);
                    }
                    if result.is_err() {
                        work.failed_draw();
                    }
                } else {
                    self.paint_canvas_cache(scene, &mut work)?;
                }
                continue;
            }

            let mut camera_viewport = job.viewport;
            if let Some(surface) = job.projected {
                let result = self.prepare_projected_surface(
                    host,
                    surface,
                    job.projected_mvp,
                    job.projected_viewport,
                    presentation_time,
                    false,
                    &mut work,
                );
                if result == Err(RenderError::ContextLost) {
                    return result.map(|_| work);
                }
                if result.is_err() {
                    self.camera_completed.remove(&selection);
                    work.failed_draw();
                    continue;
                }
                if let Some(size) = self.projected_size(selection) {
                    camera_viewport.width = size[0];
                    camera_viewport.height = size[1];
                }
            }
            match self.paint_camera_child(
                host,
                (selection, job.publication),
                camera_viewport,
                job.projection_extent,
                work,
                presentation_time,
            ) {
                Ok(completed) => {
                    work = completed;
                    self.projected_camera_painted(selection);
                }
                Err(RenderError::ContextLost) => return Err(RenderError::ContextLost),
                Err(_) => {
                    self.camera_completed.remove(&selection);
                    work.failed_draw();
                }
            }
        }
        Ok(work)
    }

    fn paint_camera_child(
        &mut self,
        host: &HostRuntime,
        output: (OutputRef, WorldPublicationId),
        mut viewport: WorldViewport,
        projection_extent: [f64; 2],
        work: RenderFrameWork,
        presentation_time: f64,
    ) -> Result<RenderFrameWork, RenderError> {
        let (selection, publication) = output;
        let scene = RenderScene::new(host, selection, publication)?;
        let camera = Some(
            scene
                .camera
                .prepare_for_extent(projection_extent)
                .map(|camera| camera.view_projection),
        );
        if camera.as_ref().is_some_and(Result::is_err) {
            return Err(RenderError::InvalidTransform);
        }

        let previous = self.camera_targets.remove(&selection);
        let available = self.surface_cache.budget().saturating_sub(
            self.projected_image_bytes()
                + self.surface_cache.resident().1 as usize
                + self.remaining_required_bytes(selection),
        );
        let mut size = [viewport.width, viewport.height];
        while size[0] as usize * size[1] as usize * 4 > available {
            if size == [1, 1] {
                if let Some((target, _, _)) = previous {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                }
                return Err(RenderError::UnavailableOutput);
            }
            size = size.map(|v| (v / 2).max(1));
        }
        let (width, height) = (size[0], size[1]);
        viewport.width = width;
        viewport.height = height;
        let mut capacity = previous
            .as_ref()
            .filter(|(_, _, capacity)| super::super::surface::quality::fits(size, *capacity))
            .map_or_else(
                || {
                    super::super::surface::quality::image_capacity(
                        size,
                        self.device.borrow().surface_cache_limit(),
                    )
                },
                |(_, _, capacity)| *capacity,
            );
        if capacity[0] as usize * capacity[1] as usize * 4 > available {
            capacity = size;
        }
        let mut target = match previous {
            Some((target, _, old_capacity)) if old_capacity == capacity => target,
            Some((mut target, _, _)) => {
                if let Err(error) = self.device.borrow_mut().resize_surface_cache_target(
                    &mut target,
                    capacity[0],
                    capacity[1],
                ) {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                    return Err(error);
                }
                target
            }
            None => self
                .device
                .borrow_mut()
                .create_surface_cache_target(capacity[0], capacity[1])?,
        };
        if let Err(error) = self
            .device
            .borrow_mut()
            .set_surface_cache_target_active_size(&mut target, width, height)
        {
            self.device.borrow_mut().delete_surface_cache_target(target);
            return Err(error);
        }

        let parent_sources = std::mem::take(&mut self.inclusions.active);
        self.inclusions.active.collect_image = true;
        let begun = self
            .device
            .borrow_mut()
            .begin_camera_target(&mut target, &BACKGROUND);
        let result = match begun {
            Ok(()) => {
                self.debug.retain(&scene.debug);
                let result = self.draw_items(&scene, camera, work, viewport, presentation_time);
                let finished = self.device.borrow_mut().end_surface_cache_target();
                result.and_then(|work| finished.map(|()| work))
            }
            Err(error) => Err(error),
        };

        if result.is_ok() {
            self.inclusions.record(selection, publication);
            self.inclusions.save_image(selection, parent_sources);
            self.camera_targets
                .insert(selection, (target, [width, height], capacity));
            self.camera_completed.insert(selection);
            self.camera_used.insert(selection, presentation_time);
        } else {
            self.inclusions.active = parent_sources;
            self.device.borrow_mut().delete_surface_cache_target(target);
        }
        result
    }

    pub(in crate::services::render) fn draw_camera_image(
        &mut self,
        selection: OutputRef,
        mvp: [f32; 16],
        extent: [f32; 2],
        clip: [f32; 4],
        opacity: f32,
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        if !self.camera_completed.contains(&selection) {
            stats.failed_draw();
            return Ok(());
        }

        self.surface_cache_program()?;
        #[cfg(feature = "instrumentation")]
        let _gpu_composite = if self.camera_targets.contains_key(&selection) {
            self.gpu_scope(
                super::super::gpu_profiling::RenderGpuScope::Composite,
                Some(selection.world()),
                None,
            )
        } else {
            None
        };
        let Some((target, _, _)) = self.camera_targets.get(&selection) else {
            return Ok(());
        };

        self.device.borrow_mut().set_surface_double_sided(true)?;
        let drawn = self.device.borrow_mut().draw_surface_cache(
            self.surface_cache_program
                .as_ref()
                .expect("prepared composite"),
            target,
            &mvp,
            &extent,
            &clip,
            opacity,
        );
        let restored = self.device.borrow_mut().set_surface_double_sided(false);
        drawn.and(restored)?;
        self.inclusions.composite(selection);
        stats.draw(2);
        Ok(())
    }
}
