//! Surface primitive submission for text, drawings, bitmaps and GUI boxes.

use super::super::assets::GlTextureData;
use super::{RenderError, RenderService, RenderStats};
use crate::RenderDevice;
use ipp_core::WorldContext;

impl<D: RenderDevice> RenderService<D> {
    /// Draw a Surface's primitives with `mvp`: the scene projection times the
    /// Surface model in the main pass, or a content-space projection into its
    /// cache image during a repaint.
    pub(super) fn draw_surface(
        &mut self,
        world: &WorldContext<'_>,
        item: &ipp_core::SurfaceRenderItem,
        mvp: [f32; 16],
        stats: &mut RenderStats,
        instances: &mut Vec<super::super::device::SurfacePathInstance>,
    ) -> Result<(), RenderError> {
        self.prepare_surface_program()?;
        let started = { self.device.borrow_mut().set_surface_double_sided(true) };
        if let Err(error) = started {
            let _ = self.device.borrow_mut().set_surface_double_sided(false);
            return Err(error);
        }
        let result = self.draw_surface_primitives(world, item, &mvp, stats, instances);
        // Surface draw errors must not leak double-sided state into later mesh
        // submissions. Preserve the draw error when restoring state also fails.
        let restored = self.device.borrow_mut().set_surface_double_sided(false);
        result.and(restored)
    }

    /// Create the shared Surface path program on first use.
    pub(super) fn prepare_surface_program(&mut self) -> Result<(), RenderError> {
        if self.surface_program.is_none() {
            self.surface_program = Some(self.device.borrow_mut().create_program(
                crate::services::render::embedded_shader!("shaders/surface.vert"),
                crate::services::render::embedded_shader!("shaders/surface.frag"),
            )?);
        }

        Ok(())
    }

    /// Submit a Surface's primitives in painter order with a caller-selected
    /// projection. Antialiasing and glyph coverage size from the device's
    /// current Surface viewport: the host target in the main pass, the image
    /// during a cache repaint.
    ///
    /// Primitives whose resource or GPU data is not resident are skipped and
    /// their resources listed in `surface_missing`, which this call resets
    /// together with `surface_analytic_text`.
    pub(super) fn draw_surface_primitives(
        &mut self,
        world: &WorldContext<'_>,
        item: &ipp_core::SurfaceRenderItem,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
        instances: &mut Vec<super::super::device::SurfacePathInstance>,
    ) -> Result<(), RenderError> {
        use ipp_core::services::asset_management::Asset as _;

        self.surface_missing.clear();
        #[cfg(feature = "gui")]
        {
            self.surface_analytic_text = false;
        }

        // Unchanged paint lets retained work skip hashing its inputs.
        let paint = self.surface_paint.get(&world.id()).map_or(
            super::super::retained_surfaces::SurfacePaint::UNKNOWN,
            |tracker| tracker.paint(item),
        );

        // Clip of the contiguous box run being collected.
        #[cfg(feature = "gui")]
        let mut current_box_batch: Option<ipp_core::systems::surface::SurfaceClipRect> = None;

        #[cfg(feature = "gui")]
        let mut pending_boxes: Vec<&ipp_core::SurfaceRenderPrimitive> = Vec::new();

        for primitive in &item.primitives {
            // Intersect the per-primitive clip with the root content rectangle
            // on every path. An empty intersection suppresses the primitive:
            // no draw call, no triangles, no effect on painter order or depth.
            let Some(clip) = ipp_core::primitive_effective_clip(primitive.style(), item.clip_size)
            else {
                continue;
            };

            #[cfg(feature = "gui")]
            if let ipp_core::SurfaceRenderPrimitive::Box {
                ..
            } = primitive
            {
                // The clip above is already non-empty; this re-check
                // guards the device against invalid box dimensions in
                // hand-built submissions with NaN uniforms.
                if !ipp_core::surface_primitive_visible(primitive, item.clip_size) {
                    continue;
                }

                // Boxes batch across part classes; the retained cache isolates
                // recently changed boxes by volatility instead.
                if let Some(batch_clip) = current_box_batch
                    && batch_clip != clip
                {
                    self.flush_gui_boxes(
                        world.id(),
                        item.entity,
                        batch_clip,
                        paint,
                        &mut pending_boxes,
                        mvp,
                        stats,
                    )?;
                }

                current_box_batch = Some(clip);
                pending_boxes.push(primitive);

                continue;
            }

            #[cfg(feature = "gui")]
            if let Some(batch_clip) = current_box_batch.take() {
                self.flush_gui_boxes(
                    world.id(),
                    item.entity,
                    batch_clip,
                    paint,
                    &mut pending_boxes,
                    mvp,
                    stats,
                )?;
            }

            match primitive {
                ipp_core::SurfaceRenderPrimitive::Glyphs {
                    style,
                    font,
                    font_size,
                    glyphs,
                } => {
                    let Some(data) = world
                        .asset_resources()
                        .get(font.key)
                        .and_then(|resource| resource.data())
                        .and_then(|asset| {
                            asset
                                .as_any()
                                .downcast_ref::<super::super::surface_assets::GlFontData<D>>()
                        })
                    else {
                        stats.failed_draw_calls += 1;
                        self.surface_missing.push(font.key);
                        continue;
                    };

                    #[cfg(feature = "gui")]
                    {
                        let run = super::super::glyph_atlas::TextRun {
                            entity: item.entity,
                            style,
                            clip,
                            font_key: font.key,
                            font_size: *font_size,
                            units_per_em: data.font.units_per_em(),
                            glyphs,
                        };
                        if self.draw_glyphs_via_atlas(world.id(), &run, mvp, stats)? {
                            continue;
                        }

                        self.surface_analytic_text = true;
                    }

                    let unit = *font_size / data.font.units_per_em() as f32;
                    let Some(path) = data.path.as_ref() else {
                        // A font without curves has no path; one whose GPU
                        // data was released is not resident yet.
                        if data.graphics_ready() == Some(false) {
                            self.surface_missing.push(font.key);
                        }
                        continue;
                    };
                    if self.surface_instance_program.is_none() {
                        self.surface_instance_program =
                            Some(self.device.borrow_mut().create_program(
                                crate::services::render::embedded_shader!(
                                    "shaders/surface_instanced.vert"
                                ),
                                crate::services::render::embedded_shader!("shaders/surface.frag"),
                            )?);
                    }

                    let run = super::super::analytic_glyphs::AnalyticGlyphRun {
                        entity: item.entity,
                        style,
                        clip,
                        font_key: font.key,
                        font_size: *font_size,
                        glyphs,
                    };
                    let build = |instances: &mut Vec<super::super::device::SurfacePathInstance>| {
                        for glyph in glyphs {
                            let Some(&range) = data.ranges.get(glyph.glyph_id as usize) else {
                                continue;
                            };
                            if range.curve_range[1] == 0 {
                                continue;
                            }
                            let bounds = data.glyph_bounds[glyph.glyph_id as usize];
                            #[cfg(feature = "gui")]
                            if !super::super::glyph_atlas::glyph_intersects_clip(
                                style, glyph, bounds, unit, clip,
                            ) {
                                continue;
                            }
                            let tint = glyph.color.unwrap_or(style.color);
                            let color = [tint[0], tint[1], tint[2], tint[3] * style.opacity];
                            let placement = [
                                style.position[0] + glyph.position[0] * style.scale[0],
                                style.position[1] + glyph.position[1] * style.scale[1],
                                style.scale[0] * unit,
                                style.scale[1] * unit,
                            ];
                            instances.push(super::super::device::SurfacePathInstance {
                                bounds,
                                placement,
                                color,
                                descriptor: range,
                            });
                        }
                    };
                    let program = self.surface_instance_program.as_ref().unwrap();
                    let device = &self.device;
                    self.analytic_glyphs
                        .entry(world.id())
                        .or_insert_with(|| {
                            super::super::analytic_glyphs::AnalyticGlyphCache::new(device.clone())
                        })
                        .draw_run(program, path, &run, paint, mvp, instances, build, stats)?;
                }
                ipp_core::SurfaceRenderPrimitive::Drawing {
                    style,
                    drawing,
                } => {
                    let Some(data) = world
                        .asset_resources()
                        .get(drawing.key)
                        .and_then(|resource| resource.data())
                        .and_then(|asset| {
                            asset
                                .as_any()
                                .downcast_ref::<super::super::surface_assets::GlDrawingData<D>>()
                        })
                    else {
                        stats.failed_draw_calls += 1;
                        self.surface_missing.push(drawing.key);
                        continue;
                    };
                    let placement = [
                        style.position[0],
                        style.position[1],
                        style.scale[0],
                        style.scale[1],
                    ];
                    let Some(path) = data.path.as_ref() else {
                        if data.graphics_ready() == Some(false) {
                            self.surface_missing.push(drawing.key);
                        }
                        continue;
                    };
                    for (i, (layer, &range)) in
                        data.drawing.layers().iter().zip(&data.ranges).enumerate()
                    {
                        if range.curve_range[1] == 0 {
                            continue;
                        }
                        let layer_rgb = [
                            srgb(layer.color[0]),
                            srgb(layer.color[1]),
                            srgb(layer.color[2]),
                        ];
                        let alpha = f32::from(layer.color[3]) / 255.0;
                        let color = [
                            style.color[0] * layer_rgb[0],
                            style.color[1] * layer_rgb[1],
                            style.color[2] * layer_rgb[2],
                            style.color[3] * style.opacity * alpha,
                        ];
                        let fill_rule = u32::from(matches!(
                            layer.fill_rule,
                            ipp_core::services::asset_management::drawing::FillRule::EvenOdd
                        ));
                        let bounds = data
                            .layer_bounds
                            .get(i)
                            .copied()
                            .unwrap_or_else(|| data.drawing.bounds());
                        let program = self.surface_program.as_ref().unwrap();
                        self.device.borrow_mut().draw_surface_path(
                            program, path, &bounds, range, mvp, &placement, &clip, &color,
                            fill_rule,
                        )?;
                        stats.draw_calls += 1;
                        stats.triangles += 2;
                    }
                }
                ipp_core::SurfaceRenderPrimitive::Bitmap {
                    style,
                    bitmap,
                    size,
                } => {
                    let Some(texture) = world
                        .asset_resources()
                        .get(bitmap.key)
                        .and_then(|resource| resource.data())
                        .and_then(|asset| asset.as_any().downcast_ref::<GlTextureData<D>>())
                        .and_then(|data| data.gpu.as_ref())
                    else {
                        stats.failed_draw_calls += 1;
                        self.surface_missing.push(bitmap.key);
                        continue;
                    };
                    if self.surface_bitmap_program.is_none() {
                        self.surface_bitmap_program =
                            Some(self.device.borrow_mut().create_program(
                                crate::services::render::embedded_shader!(
                                    "shaders/surface_bitmap.vert"
                                ),
                                crate::services::render::embedded_shader!(
                                    "shaders/surface_bitmap.frag"
                                ),
                            )?);
                    }
                    let program = self.surface_bitmap_program.as_ref().unwrap();
                    let placement = [
                        style.position[0],
                        style.position[1],
                        size[0] * style.scale[0],
                        size[1] * style.scale[1],
                    ];
                    let color = [
                        style.color[0],
                        style.color[1],
                        style.color[2],
                        style.color[3] * style.opacity,
                    ];
                    self.device
                        .borrow_mut()
                        .draw_surface_bitmap(program, texture, mvp, &placement, &clip, &color)?;
                    stats.draw_calls += 1;
                    stats.triangles += 2;
                }
                #[cfg(feature = "gui")]
                ipp_core::SurfaceRenderPrimitive::Box {
                    ..
                } => unreachable!(),
                // A GUI box reaching a renderer built without the gui
                // capability cannot draw: core and renderer features compose
                // independently, so this fallback keeps every combination
                // compiling and reports the omission like a missing resource
                // instead of breaking the frame.
                #[allow(unreachable_patterns)]
                _ => {
                    stats.failed_draw_calls += 1;
                }
            }
        }

        #[cfg(feature = "gui")]
        if let Some(batch_clip) = current_box_batch.take() {
            self.flush_gui_boxes(
                world.id(),
                item.entity,
                batch_clip,
                paint,
                &mut pending_boxes,
                mvp,
                stats,
            )?;
        }

        self.surface_paint
            .entry(world.id())
            .or_default()
            .drawn(item, paint);

        Ok(())
    }

    /// Publish this World's glyph demand before drawing, using the frame's culling
    /// and Surface cache decisions.
    ///
    /// Runs drawn this frame update their bands and demand and queue missing entries;
    /// a repainted cache image selects bands from its own projection and size, so
    /// camera movement within one cache resolution never changes glyph quality.
    /// Culled Surfaces and reused images keep theirs. Unchanged runs only hash their
    /// inputs.
    #[cfg(feature = "gui")]
    pub(super) fn prepare_glyph_demand(
        &mut self,
        world: &WorldContext<'_>,
        items: &[ipp_core::SurfaceRenderItem],
        view_projection: [f32; 16],
        viewport: (u32, u32),
    ) {
        use super::super::glyph_atlas::{GlyphBatchRenderCache, TextRun, projected_glyph_height};

        let frustum = ipp_core::systems::geometry::frustum_planes(view_projection);
        let atlas = &mut self.glyph_atlas;
        let work = &mut self.glyph_frame;
        let device = &self.device;
        let tracker = self.surface_paint.entry(world.id()).or_default();
        let cache = self
            .glyph_batch_cache
            .entry(world.id())
            .or_insert_with(|| GlyphBatchRenderCache::new(device.clone()));
        atlas.begin_publication();
        cache.begin_publication();

        for item in items {
            // Culled Surfaces and reused images keep their retained runs, so they
            // keep their entries too.
            let (mvp, viewport) = match super::surface_cache::surface_raster(
                &self.surface_cache,
                world,
                item,
                view_projection,
                viewport,
                &frustum,
            ) {
                super::surface_cache::SurfaceRaster::Skip => {
                    cache.keep_surface(item.entity);
                    continue;
                }
                super::surface_cache::SurfaceRaster::Draw(mvp, viewport) => (mvp, viewport),
            };
            let paint = tracker.paint(item);

            for primitive in &item.primitives {
                let ipp_core::SurfaceRenderPrimitive::Glyphs {
                    style,
                    font,
                    font_size,
                    glyphs,
                } = primitive
                else {
                    continue;
                };
                let Some(clip) = ipp_core::primitive_effective_clip(style, item.clip_size) else {
                    continue;
                };
                let Some(data) = world
                    .asset_resources()
                    .get(font.key)
                    .and_then(|r| r.data())
                    .and_then(|a| {
                        a.as_any()
                            .downcast_ref::<super::super::surface_assets::GlFontData<D>>()
                    })
                else {
                    continue;
                };

                let run = TextRun {
                    entity: item.entity,
                    style,
                    clip,
                    font_key: font.key,
                    font_size: *font_size,
                    units_per_em: data.font.units_per_em(),
                    glyphs,
                };
                let height = projected_glyph_height(
                    &mvp,
                    style.position,
                    *font_size * style.scale[1],
                    viewport,
                );
                // Glyphs without curves, such as spaces, need no coverage entry.
                let bounds = |glyph_id: u32| {
                    data.ranges
                        .get(glyph_id as usize)
                        .filter(|range| range.curve_range[1] != 0)
                        .and_then(|_| data.glyph_bounds.get(glyph_id as usize).copied())
                };
                cache.publish_run(atlas, &run, paint, height, bounds, work);
            }
        }

        cache.end_publication(atlas);
        atlas.release_if_unused();
    }

    /// Rasterize this frame's queued glyph misses before the main pass.
    ///
    /// Every slot is allocated first, then each atlas page is bound once and the host
    /// target restored once. Recoverable allocation or rasterization failures discard
    /// the entry, back the glyph off and keep its text analytic. Context loss, and any
    /// failure to restore the host target, fail the frame so recovery or the
    /// draw-failure path runs.
    #[cfg(feature = "gui")]
    pub(super) fn populate_glyph_misses(
        &mut self,
        world: &WorldContext<'_>,
    ) -> Result<(), RenderError> {
        let queue = self
            .glyph_frame
            .take_queue(self.glyph_population.allowance());
        if queue.is_empty() {
            self.glyph_frame.restore_queue(queue);
            return Ok(());
        }

        // Native passes refine the per-glyph cost; WebGL keeps its estimate because
        // its draws execute in another process.
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();
        let result = self.populate_glyphs(world, &queue);
        #[cfg(not(target_arch = "wasm32"))]
        if result.is_ok() {
            self.glyph_population
                .record(queue.len(), started.elapsed().as_secs_f64() * 1e3);
        }

        self.glyph_frame.restore_queue(queue);
        result
    }

    #[cfg(feature = "gui")]
    fn populate_glyphs(
        &mut self,
        world: &WorldContext<'_>,
        queue: &[super::super::glyph_atlas::GlyphKey],
    ) -> Result<(), RenderError> {
        struct PlannedGlyph<'a, P> {
            key: super::super::glyph_atlas::GlyphKey,
            page: usize,
            path: &'a P,
            range: super::super::device::SurfacePathDescriptor,
            bounds: [f32; 4],
            placement: [f32; 4],
            clip: [f32; 4],
        }

        if self.surface_program.is_none() {
            self.surface_program = Some(self.device.borrow_mut().create_program(
                crate::services::render::embedded_shader!("shaders/surface.vert"),
                crate::services::render::embedded_shader!("shaders/surface.frag"),
            )?);
        }

        // Allocate every slot before binding any page, so each page binds once.
        let mut planned = Vec::with_capacity(queue.len());
        for &key in queue {
            let Some(data) = world
                .asset_resources()
                .get(key.font_key)
                .and_then(|resource| resource.data())
                .and_then(|asset| {
                    asset
                        .as_any()
                        .downcast_ref::<super::super::surface_assets::GlFontData<D>>()
                })
            else {
                continue;
            };
            let (Some(path), Some(&range), Some(&bounds)) = (
                data.path.as_ref(),
                data.ranges.get(key.glyph_id as usize),
                data.glyph_bounds.get(key.glyph_id as usize),
            ) else {
                continue;
            };

            // Align to full texels and include an antialias texel inside the slot. UV
            // bounds and placed geometry then describe the same area.
            let scale = f32::from(key.resolution_band) / data.font.units_per_em() as f32;
            let raster_bounds = [
                ((bounds[0] * scale).floor() - 1.0) / scale,
                ((bounds[1] * scale).floor() - 1.0) / scale,
                ((bounds[2] * scale).ceil() + 1.0) / scale,
                ((bounds[3] * scale).ceil() + 1.0) / scale,
            ];
            let px_w = ((raster_bounds[2] - raster_bounds[0]) * scale).round() as u32;
            let px_h = ((raster_bounds[3] - raster_bounds[1]) * scale).round() as u32;

            match self
                .glyph_atlas
                .allocate_slot(key, px_w, px_h, raster_bounds)
            {
                Ok(([slot_x, slot_y], page, _)) => planned.push(PlannedGlyph {
                    key,
                    page,
                    path,
                    range,
                    bounds,
                    placement: [
                        slot_x as f32 - raster_bounds[0] * scale,
                        slot_y as f32 - raster_bounds[1] * scale,
                        scale,
                        scale,
                    ],
                    clip: [
                        slot_x as f32,
                        slot_y as f32,
                        (slot_x + px_w) as f32,
                        (slot_y + px_h) as f32,
                    ],
                }),
                Err(error) => {
                    if let Err(lost) = self.glyph_population_failed(key, error) {
                        for glyph in &planned {
                            self.glyph_atlas.discard_population(glyph.key);
                        }
                        return Err(lost);
                    }
                }
            }
        }

        if planned.is_empty() {
            return Ok(());
        }

        // Stable: glyphs keep their queue order within each page.
        planned.sort_by_key(|glyph| glyph.page);

        let started = self.device.borrow_mut().set_surface_double_sided(true);
        if let Err(error) = started {
            let _ = self.device.borrow_mut().set_surface_double_sided(false);
            for glyph in &planned {
                self.glyph_atlas.discard_population(glyph.key);
            }
            return Err(error);
        }

        let page_dim = super::super::glyph_atlas::ATLAS_PAGE_SIZE as f32;
        let atlas_mvp = [
            2.0 / page_dim,
            0.0,
            0.0,
            0.0,
            0.0,
            -2.0 / page_dim,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            -1.0,
            1.0,
            0.0,
            1.0,
        ];
        let color = [1.0, 1.0, 1.0, 1.0];
        let program = self.surface_program.as_ref().unwrap();
        let mut outcomes = Vec::with_capacity(planned.len());
        let mut lost = false;
        let mut bound_page = None;

        for glyph in &planned {
            // Consecutive pages switch targets; the host target is saved only once.
            if bound_page
                .as_ref()
                .is_none_or(|(page, _)| *page != glyph.page)
            {
                let begun = match self.glyph_atlas.page_handle(glyph.page) {
                    Some(handle) => self.device.borrow_mut().begin_glyph_atlas_page(handle),
                    None => Err(RenderError::RenderDevice("glyph atlas page missing".into())),
                };
                bound_page = Some((glyph.page, begun));
            }

            let begun = bound_page.as_ref().map(|(_, begun)| begun.clone());
            let drawn = begun.unwrap_or(Ok(())).and_then(|()| {
                self.device.borrow_mut().draw_surface_path(
                    program,
                    glyph.path,
                    &glyph.bounds,
                    glyph.range,
                    &atlas_mvp,
                    &glyph.placement,
                    &glyph.clip,
                    &color,
                    0,
                )
            });
            lost |= drawn == Err(RenderError::ContextLost);
            outcomes.push(drawn);
            if lost {
                break;
            }
        }

        // Restore the target even when beginning or drawing fails. GLES also reports
        // errors of earlier unchecked draws here, so a restore failure fails the frame.
        let restored = self.device.borrow_mut().end_glyph_atlas_page();
        let unset = self.device.borrow_mut().set_surface_double_sided(false);

        if lost
            || restored == Err(RenderError::ContextLost)
            || unset == Err(RenderError::ContextLost)
        {
            // Recovery clears the atlas; no unconfirmed entry may be sampled meanwhile.
            for glyph in &planned {
                self.glyph_atlas.discard_population(glyph.key);
            }
            return Err(RenderError::ContextLost);
        }

        if let Err(error) = restored {
            // A failed entry must never be sampled as populated coverage.
            for glyph in &planned {
                self.glyph_atlas.abandon_population(glyph.key);
            }
            return Err(error);
        }

        for (glyph, outcome) in planned.iter().zip(outcomes) {
            match outcome {
                Ok(()) => self.glyph_frame.populates += 1,
                Err(error) => self.glyph_population_failed(glyph.key, error)?,
            }
        }

        unset
    }

    /// Discard a failed glyph entry, then propagate context loss or count the failure.
    #[cfg(feature = "gui")]
    fn glyph_population_failed(
        &mut self,
        key: super::super::glyph_atlas::GlyphKey,
        error: RenderError,
    ) -> Result<(), RenderError> {
        self.glyph_atlas.abandon_population(key);
        if error == RenderError::ContextLost {
            return Err(error);
        }

        self.glyph_frame.failures += 1;
        Ok(())
    }

    /// Publish context-wide retained Surface residency and this frame's glyph work.
    ///
    /// Only a successful submission that reached its Surfaces can distinguish stale GUI
    /// batches and analytic text streams from those of culled Surfaces. Failed,
    /// cameraless and invalid-camera frames keep everything, so a transient failure or
    /// pan never forces re-uploads. Atlas text runs follow the demand published before
    /// drawing.
    pub(super) fn finish_retained_surfaces(
        &mut self,
        world: ipp_core::WorldId,
        items: &[ipp_core::SurfaceRenderItem],
        stats: Option<&mut RenderStats>,
    ) {
        let submitted = self.submitted_surfaces.take().filter(|_| stats.is_some());
        let live: std::collections::BTreeSet<_> = items.iter().map(|item| item.entity).collect();
        let surfaces = submitted.as_ref().map(|submitted| {
            super::super::retained_surfaces::RetainedSurfaceSubmission {
                live: &live,
                submitted,
            }
        });

        #[cfg(feature = "gui")]
        if let Some(cache) = self.gui_batch_cache.get_mut(&world) {
            cache.finish_frame(surfaces.as_ref());
        }

        if let Some(cache) = self.analytic_glyphs.get_mut(&world) {
            cache.finish_frame(surfaces.as_ref());
        }

        if let (Some(tracker), Some(_)) = (self.surface_paint.get_mut(&world), &surfaces) {
            tracker.retain(&live);
        }

        let Some(stats) = stats else {
            return;
        };
        stats.analytic_glyph_resident_bytes = self
            .analytic_glyphs
            .values()
            .map(|cache| cache.resident_bytes())
            .sum::<usize>() as u32;
        #[cfg(feature = "gui")]
        self.publish_glyph_work(stats);
    }

    /// Publish retained GUI and glyph residency and this frame's atlas work.
    #[cfg(feature = "gui")]
    fn publish_glyph_work(&mut self, stats: &mut RenderStats) {
        stats.gui_resident_bytes = self
            .gui_batch_cache
            .values()
            .map(|cache| cache.resident_bytes())
            .chain(
                self.glyph_batch_cache
                    .values()
                    .map(|cache| cache.resident_bytes()),
            )
            .sum::<usize>() as u32;
        self.glyph_frame.publish(stats);
        stats.glyph_page_retirements = self.glyph_atlas.take_retired_pages();
        stats.glyph_pages = self.glyph_atlas.page_count();
        stats.glyph_resident_bytes = self.glyph_atlas.resident_bytes();
    }

    #[cfg(feature = "gui")]
    #[allow(clippy::too_many_arguments)]
    fn flush_gui_boxes(
        &mut self,
        world: ipp_core::WorldId,
        entity: ipp_core::EntityId,
        batch_clip: ipp_core::systems::surface::SurfaceClipRect,
        paint: super::super::retained_surfaces::SurfacePaint,
        boxes: &mut Vec<&ipp_core::SurfaceRenderPrimitive>,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        if boxes.is_empty() {
            return Ok(());
        }

        if self.surface_box_program.is_none() {
            self.surface_box_program = Some(self.device.borrow_mut().create_program(
                crate::services::render::embedded_shader!("shaders/surface_box.vert"),
                crate::services::render::embedded_shader!("shaders/surface_box.frag"),
            )?);
        }

        let program = self.surface_box_program.as_ref().unwrap();

        let cache = self.gui_batch_cache.entry(world).or_insert_with(|| {
            super::super::gui_batch::GuiBatchRenderCache::new(self.device.clone())
        });
        // The cache splits the run into bounded batches with stable boundaries.
        cache.draw_box_batch(program, entity, batch_clip, paint, boxes, mvp, stats)?;

        boxes.clear();

        Ok(())
    }

    /// Draw a published text run from its retained atlas batches.
    ///
    /// Returns `false` when the run has no band or a demanded glyph is not resident;
    /// the caller then draws analytic glyphs.
    #[cfg(feature = "gui")]
    fn draw_glyphs_via_atlas(
        &mut self,
        world: ipp_core::WorldId,
        run: &super::super::glyph_atlas::TextRun<'_>,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<bool, RenderError> {
        if !self.glyph_batch_cache.contains_key(&world) {
            return Ok(false);
        }

        if self.surface_text_program.is_none() {
            self.surface_text_program = Some(self.device.borrow_mut().create_program(
                crate::services::render::embedded_shader!("shaders/surface_text.vert"),
                crate::services::render::embedded_shader!("shaders/surface_text.frag"),
            )?);
        }

        let program = self.surface_text_program.as_ref().unwrap();
        let Some(cache) = self.glyph_batch_cache.get_mut(&world) else {
            return Ok(false);
        };
        cache.draw_text_run(program, &self.glyph_atlas, run, mvp, stats)
    }
}

fn srgb(value: u8) -> f32 {
    let encoded = f32::from(value) / 255.0;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}
