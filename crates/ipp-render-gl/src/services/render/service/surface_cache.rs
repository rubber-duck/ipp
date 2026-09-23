//! Whole-Surface cache orchestration: presentation planning, the repaint
//! pre-pass, composition and end-of-frame accounting.
//!
//! [`RenderService::render`] plans every opted-in Surface after preparing the
//! camera, publishes glyph demand from that plan, and then, only on frames that
//! repaint, populates glyph atlas misses and repaints images before
//! `begin_frame`, so the main pass is never split by target switches. Each
//! repaint draws the Surface's ordinary primitives (retained box batches,
//! glyph-atlas runs, curve drawings and bitmaps) with a content-space
//! projection into its image, whose size the device uses for antialiasing. In
//! the main pass, cached Surfaces composite their image at their painter-order
//! slot with the current placement. A repaint that skips primitives whose
//! resources are not resident records them, and each plan reports whether one
//! of them is resident now, so an incomplete image repaints as soon as it can
//! be completed. A repaint that drew text analytically while the glyph atlas
//! population bound deferred entries repaints on the next frame, until its
//! text uses the atlas like direct presentation. The store and its policy are
//! documented beside it in `render/surface_cache.rs`.

use super::super::surface_cache::{
    SurfaceCacheAction, SurfaceCacheDiagnostic, SurfaceCacheInput, SurfaceCacheTargets,
};
use super::{RenderError, RenderService, RenderStats};
use crate::RenderDevice;
use ipp_core::services::asset_management::AssetKey;
use ipp_core::{SurfaceRenderItem, WorldContext, systems::camera};

/// Whether a Surface resource has its data and, where it owns any, its GPU data.
fn surface_resource_resident(world: &WorldContext<'_>, key: AssetKey) -> bool {
    world
        .asset_resources()
        .get(key)
        .and_then(|resource| resource.data())
        .is_some_and(|data| data.graphics_ready() != Some(false))
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

/// Where a Surface's primitives rasterize this frame, for glyph demand.
#[cfg(feature = "gui")]
pub(super) enum SurfaceRaster {
    /// Culled or composited from an unchanged image: no primitive work.
    Skip,
    /// Rasterize with this projection into a target of this size in pixels.
    Draw([f32; 16], (u32, u32)),
}

/// Projection from Surface content metres to a cache image: `x_ndc = 2x/w - 1`,
/// `y_ndc = 2y/h - 1`, so texture row 0 holds the content top as bitmap UVs expect.
pub(super) fn content_projection(clip_size: [f32; 2]) -> [f32; 16] {
    [
        2.0 / clip_size[0],
        0.0,
        0.0,
        0.0,
        0.0,
        2.0 / clip_size[1],
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -1.0,
        -1.0,
        0.0,
        1.0,
    ]
}

/// Where the planned frame rasterizes `item`'s primitives: its image for a
/// repaint, nothing when culled or reused, otherwise the scene.
#[cfg(feature = "gui")]
pub(super) fn surface_raster<T>(
    cache: &super::super::surface_cache::SurfaceTextureCache<T>,
    world: &WorldContext<'_>,
    item: &SurfaceRenderItem,
    view_projection: [f32; 16],
    viewport: (u32, u32),
    frustum: &[ipp_core::systems::geometry::GeometryPlane; 6],
) -> SurfaceRaster {
    let action = item
        .cache
        .and_then(|_| cache.action(world.id(), item.entity));
    match action {
        Some(SurfaceCacheAction::Culled | SurfaceCacheAction::Reuse) => SurfaceRaster::Skip,
        Some(SurfaceCacheAction::Repaint) => {
            let size = cache
                .image(world.id(), item.entity)
                .map_or((1, 1), |(_, size)| (size[0], size[1]));
            SurfaceRaster::Draw(content_projection(item.clip_size), size)
        }
        Some(SurfaceCacheAction::Direct) | None => {
            if world.geometry_visible(item.entity, frustum) {
                SurfaceRaster::Draw(camera::multiply(view_projection, item.model), viewport)
            } else {
                SurfaceRaster::Skip
            }
        }
    }
}

impl<D: RenderDevice> RenderService<D> {
    /// Append the cache state of one World's opted-in Surfaces after the last
    /// completed frame, in entity order. Read-only; it never changes presentation.
    pub fn surface_cache_diagnostics(
        &self,
        world: ipp_core::WorldId,
        out: &mut Vec<SurfaceCacheDiagnostic>,
    ) {
        self.surface_cache.diagnostics(world, out);
    }

    /// Bound the resident bytes of Surface cache images on this context.
    ///
    /// Images beyond the budget are evicted, least recently presented first,
    /// before new allocations; Surfaces that still do not fit present directly.
    /// Zero disables caching. The budget survives context loss.
    pub fn set_surface_cache_budget(&mut self, bytes: usize) {
        self.surface_cache.set_budget(bytes);
    }

    /// Current Surface cache image budget in bytes.
    pub fn surface_cache_budget(&self) -> usize {
        self.surface_cache.budget()
    }

    /// Select direct, reused or repainted presentation for every opted-in
    /// Surface and allocate the images this frame repaints.
    ///
    /// Frames without opted-in Surfaces or entries make no cache calls.
    pub(super) fn plan_surface_caches(
        &mut self,
        world: &WorldContext<'_>,
        surfaces: &[SurfaceRenderItem],
        view_projection: [f32; 16],
    ) -> Result<(), RenderError> {
        let mut inputs = std::mem::take(&mut self.surface_cache_inputs);
        inputs.clear();
        if surfaces.iter().any(|item| item.cache.is_some()) {
            let frustum = ipp_core::systems::geometry::frustum_planes(view_projection);
            let eye = world
                .active_camera()
                .and_then(|camera| world.world_matrix(camera).ok())
                .map(|matrix| [matrix[12], matrix[13], matrix[14]]);
            for item in surfaces {
                let Some(policy) = item.cache else {
                    continue;
                };

                // An unknown eye selects direct presentation.
                let distance = eye.map_or(f32::NAN, |eye| {
                    let d = [
                        item.anchor[0] - eye[0],
                        item.anchor[1] - eye[1],
                        item.anchor[2] - eye[2],
                    ];
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                });
                inputs.push(SurfaceCacheInput {
                    entity: item.entity,
                    policy,
                    clip_size: item.clip_size,
                    paint_revision: item.paint_revision,
                    resource_revision: item.resource_revision,
                    #[cfg(feature = "gui")]
                    interaction: item.interaction,
                    #[cfg(not(feature = "gui"))]
                    interaction: false,
                    missing_resident: self
                        .surface_cache
                        .missing(world.id(), item.entity)
                        .iter()
                        .any(|&key| surface_resource_resident(world, key)),
                    visible: world.geometry_visible(item.entity, &frustum),
                    distance,
                });
            }
        }

        let result = if inputs.is_empty() && self.surface_cache.is_empty() {
            Ok(())
        } else {
            let limit = if inputs.is_empty() {
                0
            } else {
                self.device.borrow().surface_cache_limit()
            };
            let mut device = self.device.borrow_mut();
            self.surface_cache.plan(
                world.id(),
                world.time(),
                limit,
                &inputs,
                &mut DeviceCacheTargets(&mut *device),
            )
        };

        self.surface_cache_inputs = inputs;
        result?;

        if self.surface_cache.composites_planned(world.id()) {
            self.surface_cache_program()?;
        }

        Ok(())
    }

    /// Whether this frame repaints any cache image, which moves glyph atlas
    /// population and the repaints ahead of `begin_frame`.
    pub(super) fn surface_repaints_planned(&self, world: ipp_core::WorldId) -> bool {
        self.surface_cache.repaints_planned(world)
    }

    /// Repaint every planned image before `begin_frame`, in item order.
    ///
    /// Recoverable failures release the image and present that Surface
    /// directly; context loss fails the frame. The returned work seeds the
    /// frame's stats.
    pub(super) fn repaint_surface_caches(
        &mut self,
        world: &WorldContext<'_>,
        surfaces: &[SurfaceRenderItem],
        instances: &mut Vec<super::super::device::SurfacePathInstance>,
    ) -> Result<RenderStats, RenderError> {
        let mut stats = RenderStats::default();
        let world_id = world.id();
        let time = world.time();
        for item in surfaces {
            if item.cache.is_none()
                || self.surface_cache.action(world_id, item.entity)
                    != Some(SurfaceCacheAction::Repaint)
            {
                continue;
            }

            self.surface_missing.clear();
            // Device order: double-sided state, begin, draw, and always end.
            let prepared = self
                .prepare_surface_program()
                .and_then(|()| self.device.borrow_mut().set_surface_double_sided(true));
            let begun = prepared.and_then(|()| {
                let (target, _) = self
                    .surface_cache
                    .image(world_id, item.entity)
                    .expect("planned repaint has an image");
                self.device.borrow_mut().begin_surface_cache_target(target)
            });
            let drawn = begun.clone().and_then(|()| {
                self.draw_surface_primitives(
                    world,
                    item,
                    &content_projection(item.clip_size),
                    &mut stats,
                    instances,
                )
            });
            // An end error means the image is incomplete.
            let ended = self.device.borrow_mut().end_surface_cache_target();
            let restored = self.device.borrow_mut().set_surface_double_sided(false);

            // Context loss anywhere outranks an earlier recoverable failure.
            let outcomes = [begun, drawn, ended, restored];
            let outcome = if outcomes.contains(&Err(RenderError::ContextLost)) {
                Err(RenderError::ContextLost)
            } else {
                outcomes.into_iter().collect::<Result<(), _>>()
            };
            match outcome {
                Ok(()) => {
                    // Skipped primitives leave the image incomplete until their
                    // resources are resident. Analytic text drawn while atlas
                    // population was deferred is refined on the next frame.
                    #[cfg(feature = "gui")]
                    let refine = self.surface_analytic_text && self.glyph_frame.population_capped();
                    #[cfg(not(feature = "gui"))]
                    let refine = false;
                    self.surface_cache.repainted(
                        world_id,
                        item.entity,
                        time,
                        &self.surface_missing,
                        refine,
                    );
                }
                Err(error) => {
                    let mut device = self.device.borrow_mut();
                    self.surface_cache.failed(
                        world_id,
                        item.entity,
                        time,
                        &mut DeviceCacheTargets(&mut *device),
                    );
                    if error == RenderError::ContextLost {
                        return Err(error);
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Composite `item`'s image at its current placement when the plan caches
    /// it. Returns `false` when the Surface must be drawn directly instead,
    /// including after a recoverable composite failure.
    pub(super) fn composite_surface_cache(
        &mut self,
        world: &WorldContext<'_>,
        item: &SurfaceRenderItem,
        view_projection: [f32; 16],
        stats: &mut RenderStats,
    ) -> Result<bool, RenderError> {
        let world_id = world.id();
        let action = item
            .cache
            .and_then(|_| self.surface_cache.action(world_id, item.entity));
        if !matches!(
            action,
            Some(SurfaceCacheAction::Reuse | SurfaceCacheAction::Repaint)
        ) {
            return Ok(false);
        }

        let started = self.device.borrow_mut().set_surface_double_sided(true);
        let drawn = started.and_then(|()| {
            let program = self
                .surface_cache_program
                .as_ref()
                .expect("planned composites create their program");
            let (target, _) = self
                .surface_cache
                .image(world_id, item.entity)
                .expect("planned composite has an image");
            self.device.borrow_mut().draw_surface_cache(
                program,
                target,
                &camera::multiply(view_projection, item.model),
                &item.clip_size,
            )
        });
        let restored = self.device.borrow_mut().set_surface_double_sided(false);

        let outcome = if restored == Err(RenderError::ContextLost) {
            restored
        } else {
            drawn.and(restored)
        };
        match outcome {
            Ok(()) => {
                stats.draw_calls += 1;
                stats.triangles += 2;
                self.surface_cache
                    .presented(world_id, item.entity, world.time());
                // A repainted Surface used its retained batches this frame; a
                // reused one keeps them like a culled Surface.
                if action == Some(SurfaceCacheAction::Repaint)
                    && let Some(submitted) = &mut self.submitted_surfaces
                {
                    submitted.insert(item.entity);
                }

                Ok(true)
            }
            Err(RenderError::ContextLost) => Err(RenderError::ContextLost),
            Err(_) => {
                let mut device = self.device.borrow_mut();
                self.surface_cache.failed(
                    world_id,
                    item.entity,
                    world.time(),
                    &mut DeviceCacheTargets(&mut *device),
                );
                Ok(false)
            }
        }
    }

    /// Finish the World's cache frame and publish context-wide residency.
    ///
    /// A completed planned frame releases entries of Surfaces no longer live
    /// or opted in and idle images, and publishes its counts. Failed and
    /// cameraless frames keep every entry, as retained batches do.
    pub(super) fn finish_surface_caches(
        &mut self,
        world: &WorldContext<'_>,
        stats: Option<&mut RenderStats>,
    ) {
        let counts = {
            let mut device = self.device.borrow_mut();
            self.surface_cache.finish_frame(
                world.id(),
                world.time(),
                stats.is_some(),
                &mut DeviceCacheTargets(&mut *device),
            )
        };

        let Some(stats) = stats else {
            return;
        };

        if let Some(counts) = counts {
            stats.surface_cache_repaints = counts.repaints;
            stats.surface_cache_reuses = counts.reuses;
            stats.surface_cache_direct = counts.direct;
            stats.surface_cache_fallbacks = counts.fallbacks;
            stats.surface_cache_animated = counts.animated;
            stats.surface_cache_allocations = counts.allocations;
        }

        (
            stats.surface_cache_entries,
            stats.surface_cache_resident_bytes,
        ) = self.surface_cache.resident();
    }

    /// Release every cache image of one World.
    pub(super) fn forget_surface_caches(&mut self, world: ipp_core::WorldId) {
        let mut device = self.device.borrow_mut();
        self.surface_cache
            .forget_world(world, &mut DeviceCacheTargets(&mut *device));
    }

    /// Release every cache image and the composite program after unload or
    /// context loss; the budget survives.
    pub(super) fn clear_surface_caches(&mut self) {
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
