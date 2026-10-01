//! Publication-fenced Canvas image planning, rasterization and lifetime.

use super::super::canvas_scene::{CanvasScene, OutputContentStamp};
use super::super::frame_statistics::RenderFrameWork;
use super::super::retained_surfaces::CANVAS_SURFACE;
use super::super::surface_cache::SurfaceCacheDiagnostic;
use super::super::surface_cache::SurfaceCacheTargets;
use super::super::surface_cache::{SurfaceCacheAction, SurfaceCacheInput};
use super::{RenderError, RenderService};
use crate::RenderDevice;
use ipp_core::systems::canvas::CanvasClip;
use ipp_core::{
    EntityId, OutputRef, SurfaceCachePolicy, WorldAttachmentToken, WorldId, WorldRef, WorldViewport,
};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct CanvasCacheRequest {
    pub owner: WorldRef,
    pub anchor: EntityId,
    pub token: WorldAttachmentToken,
    pub extent: [f32; 2],
    pub policy: SurfaceCachePolicy,
    pub distance: f32,
    pub clip: CanvasClip,
    pub opacity: f32,
    pub visible: bool,
    pub interaction: bool,
}

pub(super) struct CanvasCacheState {
    root: OutputRef,
    request: CanvasCacheRequest,
    stamp: OutputContentStamp,
    painted_stamp: Option<OutputContentStamp>,
    painted_outputs: std::collections::BTreeSet<OutputRef>,
    /// Raster changes propagate through caches containing independently refreshed images.
    generation: u64,
    raster_dependencies: Vec<(OutputRef, u64)>,
    paint_revision: u64,
    resource_revision: u64,
}

#[derive(Default)]
pub(super) struct CanvasCacheFrame {
    root: Option<OutputRef>,
    bindings: BTreeMap<OutputRef, (WorldId, EntityId)>,
    worlds: Vec<WorldId>,
    repainting: Option<OutputRef>,
    time: f64,
}

/// Adapts a device to the cache store's image operations.
pub(super) struct DeviceCacheTargets<'a, D>(pub(super) &'a mut D);

impl<D: RenderDevice> SurfaceCacheTargets for DeviceCacheTargets<'_, D> {
    type Target = D::SurfaceCacheTarget;

    fn create(&mut self, size: [u32; 2]) -> Result<Self::Target, RenderError> {
        self.0.create_surface_cache_target(size[0], size[1])
    }

    fn resize(&mut self, target: &mut Self::Target, size: [u32; 2]) -> Result<(), RenderError> {
        self.0.resize_surface_cache_target(target, size[0], size[1])
    }

    fn delete(&mut self, target: Self::Target) {
        self.0.delete_surface_cache_target(target);
    }
}

impl<D: RenderDevice> RenderService<D> {
    /// Plan all output domains together so budget arbitration cannot evict a planned child.
    pub(super) fn plan_canvas_caches(
        &mut self,
        host: &ipp_core::HostRuntime,
        root: OutputRef,
        requests: &[(OutputRef, ipp_core::WorldPublicationId, CanvasCacheRequest)],
        time: f64,
    ) -> Result<(), RenderError> {
        self.canvas_cache_frame = CanvasCacheFrame {
            root: Some(root),
            time,
            ..Default::default()
        };

        let mut inputs = Vec::new();
        for (selection, publication, request) in requests {
            let scene = CanvasScene::new(host, *selection, *publication)?;
            if request.visible
                && let Some(cache) = self.glyph_batch_cache.get_mut(selection)
            {
                cache.begin_publication();
                cache.keep_waiting_surface(
                    CANVAS_SURFACE,
                    &self.glyph_atlas,
                    &mut self.glyph_frame,
                );
                cache.end_publication(&mut self.glyph_atlas);
                self.populate_glyph_misses(&scene)?;
            }

            let stamp = OutputContentStamp::read(host, *selection, *publication)?;
            let raster_dependencies: Vec<_> = stamp
                .outputs
                .iter()
                .filter(|node| node.selection != *selection)
                .filter_map(|node| {
                    self.canvas_caches
                        .get(&node.selection)
                        .map(|state| (node.selection, state.generation))
                })
                .collect();
            let replaced: Vec<_> = self
                .canvas_caches
                .iter()
                .filter_map(|(output, state)| {
                    (*output != *selection
                        && state.request.owner == request.owner
                        && state.request.anchor == request.anchor)
                        .then_some(*output)
                })
                .collect();
            for output in replaced {
                self.canvas_caches.remove(&output);
                self.surface_cache.forget_surface(
                    request.owner.id(),
                    request.anchor,
                    &mut DeviceCacheTargets(&mut *self.device.borrow_mut()),
                );
            }

            if self.canvas_caches.get(selection).is_some_and(|state| {
                state.request.owner != request.owner
                    || state.request.anchor != request.anchor
                    || state.request.token != request.token
            }) {
                let state = self
                    .canvas_caches
                    .remove(selection)
                    .expect("previous cache identity");
                self.surface_cache.forget_surface(
                    state.request.owner.id(),
                    state.request.anchor,
                    &mut DeviceCacheTargets(&mut *self.device.borrow_mut()),
                );
            }

            let state = self
                .canvas_caches
                .entry(*selection)
                .or_insert_with(|| CanvasCacheState {
                    root,
                    request: request.clone(),
                    stamp: stamp.clone(),
                    painted_stamp: None,
                    painted_outputs: Default::default(),
                    generation: 0,
                    raster_dependencies: Vec::new(),
                    paint_revision: 0,
                    resource_revision: 0,
                });

            if state.stamp != stamp || state.raster_dependencies != raster_dependencies {
                state.paint_revision += 1;
            }
            state.raster_dependencies = raster_dependencies;
            if !state.stamp.same_dependencies(&stamp)
                || state.request.clip != request.clip
                || state.request.opacity != request.opacity
            {
                state.resource_revision += 1;
            }
            state.root = root;
            state.stamp = stamp;
            state.request = request.clone();
            let world = request.owner.id();
            self.canvas_cache_frame
                .bindings
                .insert(*selection, (world, request.anchor));
            inputs.push((
                world,
                SurfaceCacheInput {
                    entity: request.anchor,
                    policy: request.policy,
                    clip_size: request.extent,
                    paint_revision: state.paint_revision,
                    resource_revision: state.resource_revision,
                    interaction: request.interaction,
                    missing_resident: false,
                    text_populated: self.glyph_batch_cache.get(selection).is_some_and(|cache| {
                        cache.unpopulated_runs(CANVAS_SURFACE, &self.glyph_atlas)
                            < self.surface_cache.unpopulated(world, request.anchor)
                    }),
                    visible: request.visible
                        && request.opacity > 0.0
                        && request.clip[0] < request.clip[2]
                        && request.clip[1] < request.clip[3],
                    distance: request.distance,
                },
            ));
        }

        self.canvas_cache_frame.worlds = self
            .canvas_caches
            .values()
            .filter(|state| state.root == root)
            .map(|state| state.request.owner.id())
            .collect();
        self.canvas_cache_frame.worlds.sort_unstable();
        self.canvas_cache_frame.worlds.dedup();
        let mut device = self.device.borrow_mut();
        let limit = device.surface_cache_limit();
        self.surface_cache.plan_outputs(
            &self.canvas_cache_frame.worlds,
            time,
            limit,
            &inputs,
            &mut DeviceCacheTargets(&mut *device),
        )
    }

    /// Rasterize per-primitive inherited opacity; the resulting premultiplied image blits at one.
    pub(super) fn paint_canvas_cache(
        &mut self,
        scene: CanvasScene<'_>,
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        let selection = scene.canvas.selection;
        let Some(&(world, anchor)) = self.canvas_cache_frame.bindings.get(&selection) else {
            return Ok(());
        };

        if self.surface_cache.action(world, anchor) != Some(SurfaceCacheAction::Repaint) {
            return Ok(());
        }

        let request = self.canvas_caches[&selection].request.clone();
        let Some((target, size)) = self.surface_cache.take_image(world, anchor) else {
            return Ok(());
        };

        self.canvas_cache_frame.repainting = Some(selection);
        let parent_sources = std::mem::take(&mut self.inclusions.active);
        self.inclusions.active.collect_image = true;
        let begun = {
            let mut device = self.device.borrow_mut();
            device
                .set_surface_double_sided(true)
                .and_then(|()| device.begin_surface_cache_target(&target))
        };

        let result = match begun {
            Ok(()) => {
                let extent = scene.canvas.logical_extent;
                let mvp = super::canvas_composition::plane_matrix(
                    [-1.0, -1.0],
                    [2.0 / f64::from(extent[0]), 2.0 / f64::from(extent[1])],
                );
                let before = stats.summary.failed_draw_calls;
                let drawn = mvp.and_then(|mvp| {
                    self.draw_canvas(
                        &scene,
                        mvp,
                        request.clip,
                        request.opacity,
                        WorldViewport {
                            width: size[0],
                            height: size[1],
                            device_pixel_ratio: 1.0,
                        },
                        stats,
                    )
                });

                let finished = self.device.borrow_mut().end_surface_cache_target();
                drawn.and(finished).and({
                    if stats.summary.failed_draw_calls != before || self.surface_gui_unretained {
                        Err(RenderError::UnavailableOutput)
                    } else {
                        Ok(())
                    }
                })
            }
            Err(error) => Err(error),
        };

        let restored = self.device.borrow_mut().set_surface_double_sided(false);
        let result = result.and(restored);
        let current_image = result.is_ok() && !self.inclusions.active.stale_image;
        let painted_outputs = std::mem::take(&mut self.inclusions.active.image_outputs);
        self.inclusions.active = parent_sources;
        let state = self
            .canvas_caches
            .get_mut(&selection)
            .expect("painted cache state");
        state.painted_stamp = current_image.then(|| state.stamp.clone());
        state.painted_outputs = painted_outputs;
        self.canvas_cache_frame.repainting = None;
        self.surface_cache.put_image(world, anchor, target, size);
        self.canvas_caches
            .get_mut(&selection)
            .expect("painted cache state")
            .generation += 1;
        match result {
            Ok(()) => {
                let unpopulated = self.glyph_batch_cache.get(&selection).map_or(0, |cache| {
                    cache.unpopulated_runs(CANVAS_SURFACE, &self.glyph_atlas)
                });

                self.surface_cache.repainted(
                    world,
                    anchor,
                    self.canvas_cache_frame.time,
                    &self.surface_missing,
                    unpopulated,
                );
                Ok(())
            }
            Err(error) => {
                self.surface_cache.failed(
                    world,
                    anchor,
                    self.canvas_cache_frame.time,
                    &mut DeviceCacheTargets(&mut *self.device.borrow_mut()),
                );
                if error == RenderError::ContextLost {
                    Err(error)
                } else {
                    Ok(())
                }
            }
        }
    }

    pub(super) fn draw_cached_canvas(
        &mut self,
        scene: &CanvasScene<'_>,
        mvp: &[f32; 16],
        clip: CanvasClip,
        opacity: f32,
        stats: &mut RenderFrameWork,
    ) -> Result<bool, RenderError> {
        let selection = scene.canvas.selection;
        if self.canvas_cache_frame.repainting == Some(selection) {
            return Ok(false);
        }

        let Some(&(world, anchor)) = self.canvas_cache_frame.bindings.get(&selection) else {
            return Ok(false);
        };

        if !matches!(
            self.surface_cache.action(world, anchor),
            Some(SurfaceCacheAction::Reuse | SurfaceCacheAction::Repaint)
        ) {
            return Ok(false);
        }

        let request = &self.canvas_caches[&selection].request;
        if request.clip != clip || request.opacity != opacity {
            return Ok(false);
        }

        self.surface_cache_program()?;
        let Some((target, _)) = self.surface_cache.image(world, anchor) else {
            return Ok(false);
        };

        self.device.borrow_mut().set_surface_double_sided(true)?;
        let drawn = self.device.borrow_mut().draw_surface_cache(
            self.surface_cache_program
                .as_ref()
                .expect("composite program"),
            target,
            mvp,
            &scene.canvas.logical_extent,
            &scene.root_clip(),
            1.0,
        );
        let restored = self.device.borrow_mut().set_surface_double_sided(false);
        match drawn.and(restored) {
            Ok(()) => {
                self.surface_cache
                    .presented(world, anchor, self.canvas_cache_frame.time);
                let state = &self.canvas_caches[&selection];
                let current_image = state.painted_stamp.as_ref() == Some(&state.stamp);
                self.inclusions.active.stale_image |= !current_image;
                if current_image && self.inclusions.observing() {
                    let stale_parent = std::mem::take(&mut self.inclusions.active.stale_image);
                    for (output, publication) in super::super::canvas_scene::output_order(
                        scene.host,
                        selection,
                        scene.publication.id,
                    )? {
                        if state.painted_outputs.contains(&output) {
                            self.inclusions.record(output, publication);
                        }
                    }
                    self.inclusions.active.stale_image = stale_parent;
                }
                stats.draw(2);
                Ok(true)
            }
            Err(error) => {
                self.surface_cache.failed(
                    world,
                    anchor,
                    self.canvas_cache_frame.time,
                    &mut DeviceCacheTargets(&mut *self.device.borrow_mut()),
                );
                if error == RenderError::ContextLost {
                    Err(error)
                } else {
                    Ok(false)
                }
            }
        }
    }

    pub(super) fn finish_canvas_caches(&mut self, completed: bool) {
        let frame = &self.canvas_cache_frame;
        for world in &frame.worlds {
            self.surface_cache.finish_frame(
                *world,
                frame.time,
                completed,
                &mut DeviceCacheTargets(&mut *self.device.borrow_mut()),
            );
        }

        if completed {
            self.canvas_caches.retain(|selection, state| {
                Some(state.root) != frame.root || frame.bindings.contains_key(selection)
            });
        }
    }

    pub(super) fn retain_canvas_cache_outputs(
        &mut self,
        outputs: &std::collections::BTreeSet<OutputRef>,
    ) {
        let retired: Vec<_> = self
            .canvas_caches
            .iter()
            .filter_map(|(selection, state)| {
                (!outputs.contains(selection) || !outputs.contains(&state.root))
                    .then_some(*selection)
            })
            .collect();
        for selection in retired {
            let state = self
                .canvas_caches
                .remove(&selection)
                .expect("retired Canvas cache");
            self.surface_cache.forget_surface(
                state.request.owner.id(),
                state.request.anchor,
                &mut DeviceCacheTargets(&mut *self.device.borrow_mut()),
            );
            self.canvas_cache_frame.bindings.remove(&selection);
        }
    }

    /// Append the cache state of one World's opted-in Surfaces after the last
    /// completed frame, in entity order. Read-only; it never changes presentation.
    pub fn surface_cache_diagnostics(
        &self,
        world: ipp_core::WorldId,
        out: &mut Vec<SurfaceCacheDiagnostic>,
    ) {
        self.surface_cache.diagnostics(world, out);
    }

    /// Testing override of the renderer-owned Surface cache image budget
    /// ([`crate::SURFACE_CACHE_BUDGET_BYTES`]) on this context.
    ///
    /// Images beyond the budget are evicted, least recently presented first,
    /// before new allocations; Surfaces that still do not fit present directly.
    /// Zero disables caching. The budget survives context loss.
    #[cfg(any(test, feature = "instrumentation"))]
    pub fn set_surface_cache_budget(&mut self, bytes: usize) {
        self.surface_cache.set_budget(bytes);
    }

    /// Current Surface cache image budget in bytes.
    #[cfg(any(test, feature = "instrumentation"))]
    pub fn surface_cache_budget(&self) -> usize {
        self.surface_cache.budget()
    }

    /// Release this World's retained images.
    pub(super) fn forget_surface_caches(&mut self, world: ipp_core::WorldId) {
        let selections: Vec<_> = self
            .canvas_caches
            .iter()
            .filter_map(|(selection, state)| {
                (state.request.owner.id() == world || state.stamp.contains_world(world))
                    .then_some(*selection)
            })
            .collect();
        let mut device = self.device.borrow_mut();
        for selection in selections {
            let state = self
                .canvas_caches
                .remove(&selection)
                .expect("matching cache");
            self.surface_cache.forget_surface(
                state.request.owner.id(),
                state.request.anchor,
                &mut DeviceCacheTargets(&mut *device),
            );
            self.canvas_cache_frame.bindings.remove(&selection);
        }

        self.surface_cache
            .forget_world(world, &mut DeviceCacheTargets(&mut *device));
    }

    /// Release every cache image and the composite program after unload or
    /// context loss; the budget survives.
    pub(super) fn clear_surface_caches(&mut self) {
        self.canvas_caches.clear();
        self.canvas_cache_frame = Default::default();
        let mut device = self.device.borrow_mut();
        self.surface_cache
            .clear(&mut DeviceCacheTargets(&mut *device));
        if let Some(program) = self.surface_cache_program.take() {
            device.delete_program(program);
        }
    }

    /// Composite program: `surface_bitmap.vert` with the premultiplied
    /// `surface_cache.frag`, created on first use and released on unload.
    pub(super) fn surface_cache_program(&mut self) -> Result<&D::Program, RenderError> {
        if self.surface_cache_program.is_none() {
            self.surface_cache_program = Some(self.device.borrow_mut().create_program(
                crate::services::render::embedded_shader!("shaders/surface_bitmap.vert"),
                crate::services::render::embedded_shader!("shaders/surface_cache.frag"),
            )?);
        }

        Ok(self
            .surface_cache_program
            .as_ref()
            .expect("created composite program"))
    }
}
