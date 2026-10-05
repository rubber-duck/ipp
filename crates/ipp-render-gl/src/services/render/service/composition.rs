//! Child-first Camera and optional Canvas targets; root-only display conversion.

use super::super::canvas_scene::CanvasScene;
use super::super::{
    frame_statistics::RenderFrameWork,
    scene::{RenderScene, world_interaction_eligible},
};
use super::canvas_composition::{CanvasChild, canvas_attachment, plane_matrix};
use super::surface_cache::CanvasCacheRequest;
use super::{BACKGROUND, RenderService};
use ipp_core::systems::canvas::CanvasClip;

#[derive(Clone, Copy)]
struct CanvasView {
    plane: [f32; 16],
    clip: CanvasClip,
    opacity: f32,
    camera: Option<[f64; 3]>,
}

struct OutputJob {
    selection: OutputRef,
    publication: WorldPublicationId,
    viewport: WorldViewport,
    projection_extent: [f64; 2],
    canvas: Option<CanvasView>,
    cache: Option<CanvasCacheRequest>,
    projected: Option<super::super::scene::SceneOutputSurface>,
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
    pub(super) fn prepare_camera_children(
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
                    let frustum = scene
                        .camera
                        .prepare_for_extent(job.projection_extent)
                        .ok()
                        .map(|camera| {
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
                        let Ok(viewport) = self.camera_viewport(surface.extent, job.viewport)
                        else {
                            continue;
                        };

                        let mut canvas = None;
                        let mut cache = None;
                        if surface.selection.kind() == OutputKind::Canvas {
                            let child =
                                CanvasScene::new(host, surface.selection, surface.publication)?;
                            let extent = child.canvas.logical_extent;
                            let [width, height] = surface.extent.map(f64::from);
                            let content = if surface.geometry.exact_affine(0.0).is_some() {
                                ipp_core::systems::camera::multiply(
                                    super::canvas_composition::affine_matrix(&*surface.geometry)?,
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
                            let plane = ipp_core::systems::camera::multiply(surface.model, content);
                            let clip = child.root_clip();
                            canvas = Some(CanvasView {
                                plane,
                                clip,
                                opacity: 1.0,
                                camera,
                            });
                            cache = surface
                                .cache_policy
                                .filter(|_| surface.geometry.exact_affine(0.0).is_some())
                                .map(|policy| CanvasCacheRequest {
                                    owner: surface.entity.world,
                                    anchor: surface.entity.entity,
                                    token: surface.token.clone(),
                                    extent: surface.extent,
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

                        let extent = slot.physical_extent.map(|value| value as f32);
                        let Ok(viewport) = self.camera_viewport(extent, job.viewport) else {
                            continue;
                        };

                        let Some(child) =
                            canvas_attachment(&scene, index, view.plane, view.clip, view.opacity)?
                        else {
                            continue;
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
                                });

                                let cache = attachment
                                    .edge
                                    .and_then(|edge| edge.surface_cache_policy)
                                    .map(|policy| CanvasCacheRequest {
                                        owner: scene.publication.world,
                                        anchor: slot.anchor,
                                        token: slot.token.clone(),
                                        extent,
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
                cache.interaction =
                    super::super::canvas_scene::output_order(host, job.selection, job.publication)?
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
        self.plan_canvas_caches(host, selection, &requests, presentation_time)?;
        let mut work = RenderFrameWork::default();
        self.begin_projected_frame();
        for job in prepared {
            if !job.visible {
                continue;
            }
            let selection = job.selection;
            if selection.kind() == OutputKind::Canvas {
                let scene = CanvasScene::new(host, selection, job.publication)?;
                if let Some(surface) = job.projected {
                    let interaction =
                        super::super::canvas_scene::output_order(host, selection, job.publication)?
                            .iter()
                            .any(|(output, _)| interactive.contains(output));
                    let result = self.prepare_projected_surface(
                        host,
                        surface,
                        job.viewport,
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
                    job.viewport,
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

    fn camera_viewport(
        &self,
        extent: [f32; 2],
        viewport: WorldViewport,
    ) -> Result<WorldViewport, RenderError> {
        let limit = self.device.borrow().surface_cache_limit();
        if limit == 0
            || extent
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err(RenderError::InvalidViewport);
        }

        let ratio = f64::from(extent[0]) / f64::from(extent[1]);
        let width = viewport.width.min(limit).max(1);
        let height = (f64::from(width) / ratio)
            .round()
            .clamp(1.0, f64::from(limit)) as u32;
        let width = (f64::from(height) * ratio)
            .round()
            .clamp(1.0, f64::from(limit)) as u32;
        Ok(WorldViewport {
            width,
            height,
            ..viewport
        })
    }

    fn paint_camera_child(
        &mut self,
        host: &HostRuntime,
        output: (OutputRef, WorldPublicationId),
        viewport: WorldViewport,
        projection_extent: [f64; 2],
        work: RenderFrameWork,
        presentation_time: f64,
    ) -> Result<RenderFrameWork, RenderError> {
        let (selection, publication) = output;
        let scene = RenderScene::new(host, selection, publication)?;
        let (width, height) = (viewport.width, viewport.height);
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
        let mut target = match previous {
            Some((target, size)) if size == [width, height] => target,
            other => {
                if let Some((target, _)) = other {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                }

                self.device
                    .borrow_mut()
                    .create_surface_cache_target(width, height)?
            }
        };

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
                .insert(selection, (target, [width, height]));
            self.camera_completed.insert(selection);
        } else {
            self.inclusions.active = parent_sources;
            self.device.borrow_mut().delete_surface_cache_target(target);
        }
        result
    }

    pub(super) fn draw_camera_image(
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
                super::RenderGpuScope::Composite,
                Some(selection.world()),
                None,
            )
        } else {
            None
        };
        let Some((target, _)) = self.camera_targets.get(&selection) else {
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
