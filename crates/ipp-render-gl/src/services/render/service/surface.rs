//! Surface primitive submission for text, drawings, bitmaps and GUI boxes.

use super::super::assets::GlTextureData;
use super::{RenderError, RenderService, RenderStats};
use crate::RenderDevice;
use ipp_core::WorldContext;
use ipp_core::systems::camera;

impl<D: RenderDevice> RenderService<D> {
    pub(super) fn draw_surface(
        &mut self,
        world: &WorldContext<'_>,
        item: &ipp_core::SurfaceRenderItem,
        view_projection: [f32; 16],
        stats: &mut RenderStats,
        instances: &mut Vec<super::super::device::SurfacePathInstance>,
    ) -> Result<(), RenderError> {
        if self.surface_program.is_none() {
            self.surface_program = Some(self.device.borrow_mut().create_program(
                include_str!("../shaders/surface.vert"),
                include_str!("../shaders/surface.frag"),
            )?);
        }
        let started = { self.device.borrow_mut().set_surface_double_sided(true) };
        if let Err(error) = started {
            let _ = self.device.borrow_mut().set_surface_double_sided(false);
            return Err(error);
        }
        let result = self.draw_surface_primitives(world, item, view_projection, stats, instances);
        // Surface draw errors must not leak double-sided state into later mesh
        // submissions. Preserve the draw error when restoring state also fails.
        let restored = self.device.borrow_mut().set_surface_double_sided(false);
        result.and(restored)
    }

    fn draw_surface_primitives(
        &mut self,
        world: &WorldContext<'_>,
        item: &ipp_core::SurfaceRenderItem,
        view_projection: [f32; 16],
        stats: &mut RenderStats,
        instances: &mut Vec<super::super::device::SurfacePathInstance>,
    ) -> Result<(), RenderError> {
        let mvp = camera::multiply(view_projection, item.model);

        #[cfg(feature = "gui")]
        let mut current_box_batch: Option<(
            ipp_core::systems::surface::SurfaceClipRect,
            super::super::gui_batch::GuiPartClass,
        )> = None;

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

                let part_class = super::super::gui_batch::GuiPartClass::from_identity(
                    primitive.style().identity,
                );

                match current_box_batch {
                    Some((batch_clip, batch_class))
                        if batch_clip == clip && batch_class == part_class =>
                    {
                        pending_boxes.push(primitive);
                    }
                    Some((batch_clip, batch_class)) => {
                        self.flush_gui_boxes(
                            world.id(),
                            item.entity,
                            batch_clip,
                            batch_class,
                            &mut pending_boxes,
                            &mvp,
                            stats,
                        )?;
                        current_box_batch = Some((clip, part_class));
                        pending_boxes.push(primitive);
                    }
                    None => {
                        current_box_batch = Some((clip, part_class));
                        pending_boxes.push(primitive);
                    }
                }

                continue;
            }

            #[cfg(feature = "gui")]
            if let Some((batch_clip, batch_class)) = current_box_batch.take() {
                self.flush_gui_boxes(
                    world.id(),
                    item.entity,
                    batch_clip,
                    batch_class,
                    &mut pending_boxes,
                    &mvp,
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
                    #[cfg(feature = "gui")]
                    let drawn_via_atlas = {
                        let vp = world.render_viewport().unwrap_or((800, 600));
                        let nominal_px_height = super::super::glyph_atlas::projected_glyph_height(
                            &mvp,
                            style.position,
                            *font_size * style.scale[1],
                            vp,
                        );
                        let band_opt =
                            super::super::glyph_atlas::select_resolution_band(nominal_px_height);

                        if let Some(band) = band_opt {
                            self.draw_glyphs_via_atlas(
                                world,
                                item.entity,
                                style,
                                font.key,
                                *font_size,
                                glyphs,
                                band,
                                &mvp,
                                clip,
                                stats,
                            )?
                        } else {
                            false
                        }
                    };

                    #[cfg(not(feature = "gui"))]
                    let drawn_via_atlas = false;

                    if !drawn_via_atlas {
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
                            continue;
                        };
                        let unit = *font_size / data.font.units_per_em() as f32;
                        let Some(path) = data.path.as_ref() else {
                            continue;
                        };
                        instances.clear();
                        if !ipp_core::render_buffer_reuse_enabled() {
                            *instances = Vec::new();
                        }
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
                        if !instances.is_empty() {
                            if self.surface_instance_program.is_none() {
                                self.surface_instance_program =
                                    Some(self.device.borrow_mut().create_program(
                                        include_str!("../shaders/surface_instanced.vert"),
                                        include_str!("../shaders/surface.frag"),
                                    )?);
                            }
                            self.device.borrow_mut().draw_surface_path_instances(
                                self.surface_instance_program.as_ref().unwrap(),
                                path,
                                instances,
                                &mvp,
                                &clip,
                                0,
                            )?;
                            stats.draw_calls += 1;
                            stats.triangles += instances.len() as u32 * 2;
                            // The device packs sixteen f32 lanes per analytic
                            // instance and uploads that stream on every draw.
                            stats.uploaded_bytes = stats.uploaded_bytes.saturating_add(
                                (instances.len() * 16 * std::mem::size_of::<f32>()) as u32,
                            );
                        }
                    }
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
                        continue;
                    };
                    let placement = [
                        style.position[0],
                        style.position[1],
                        style.scale[0],
                        style.scale[1],
                    ];
                    let Some(path) = data.path.as_ref() else {
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
                            program, path, &bounds, range, &mvp, &placement, &clip, &color,
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
                        continue;
                    };
                    if self.surface_bitmap_program.is_none() {
                        self.surface_bitmap_program =
                            Some(self.device.borrow_mut().create_program(
                                include_str!("../shaders/surface_bitmap.vert"),
                                include_str!("../shaders/surface_bitmap.frag"),
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
                        .draw_surface_bitmap(program, texture, &mvp, &placement, &clip, &color)?;
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
        if let Some((batch_clip, batch_class)) = current_box_batch.take() {
            self.flush_gui_boxes(
                world.id(),
                item.entity,
                batch_clip,
                batch_class,
                &mut pending_boxes,
                &mvp,
                stats,
            )?;
        }

        Ok(())
    }

    /// Publish glyph atlas demand before drawing, using the frame's culling decisions.
    ///
    /// Without a usable camera no Surface is submitted, so the previous demand stays.
    #[cfg(feature = "gui")]
    pub(super) fn prepare_glyph_demand(
        &mut self,
        world: &WorldContext<'_>,
        items: &[ipp_core::SurfaceRenderItem],
        viewport: (u32, u32),
    ) {
        use super::super::glyph_atlas::{
            GlyphKey, GlyphSurfaceDemand, glyph_intersects_clip, projected_glyph_height,
            select_resolution_band,
        };

        let Ok(camera) = world.prepare_camera(viewport.0, viewport.1) else {
            return;
        };
        let frustum = ipp_core::systems::geometry::frustum_planes(camera.view_projection);
        let mut surfaces = std::collections::BTreeMap::new();

        for item in items {
            // Culled Surfaces keep their retained runs, so they keep their entries too.
            if !world.geometry_visible(item.entity, &frustum) {
                surfaces.insert(item.entity, GlyphSurfaceDemand::Culled);
                continue;
            }

            let mvp = camera::multiply(camera.view_projection, item.model);
            let mut keys = std::collections::BTreeSet::new();
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
                let Some(band) = select_resolution_band(projected_glyph_height(
                    &mvp,
                    style.position,
                    *font_size * style.scale[1],
                    viewport,
                )) else {
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

                for glyph in glyphs {
                    if data
                        .glyph_bounds
                        .get(glyph.glyph_id as usize)
                        .is_some_and(|bounds| {
                            glyph_intersects_clip(
                                style,
                                glyph,
                                *bounds,
                                *font_size / data.font.units_per_em() as f32,
                                clip,
                            )
                        })
                    {
                        keys.insert(GlyphKey {
                            font_key: font.key,
                            glyph_id: glyph.glyph_id,
                            resolution_band: band,
                        });
                    }
                }
            }
            surfaces.insert(item.entity, GlyphSurfaceDemand::Submitted(keys));
        }

        self.glyph_atlas.prepare_world(world.id(), surfaces);
    }

    /// Release retained Surface work a completed frame shows to be stale and publish
    /// context-wide residency.
    ///
    /// Only a successful submission that reached its Surfaces can distinguish stale work
    /// from work of culled Surfaces. Failed, cameraless and invalid-camera frames keep
    /// everything, so a transient failure or pan never forces re-uploads.
    #[cfg(feature = "gui")]
    pub(super) fn finish_retained_surfaces(
        &mut self,
        world: ipp_core::WorldId,
        items: &[ipp_core::SurfaceRenderItem],
        stats: Option<&mut RenderStats>,
    ) {
        let submitted = self.submitted_surfaces.take().filter(|_| stats.is_some());
        let live: std::collections::BTreeSet<_> = items.iter().map(|item| item.entity).collect();
        let surfaces = submitted.as_ref().map(|submitted| {
            super::super::gui_batch::RetainedSurfaceSubmission {
                live: &live,
                submitted,
            }
        });

        if let Some(cache) = self.gui_batch_cache.get_mut(&world) {
            cache.finish_frame(surfaces.as_ref());
        }
        if let Some(cache) = self.glyph_batch_cache.get_mut(&world) {
            cache.finish_frame(surfaces.as_ref());
        }

        let Some(stats) = stats else {
            return;
        };
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
        part_class: super::super::gui_batch::GuiPartClass,
        boxes: &mut Vec<&ipp_core::SurfaceRenderPrimitive>,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError> {
        if boxes.is_empty() {
            return Ok(());
        }

        if self.surface_box_program.is_none() {
            self.surface_box_program = Some(self.device.borrow_mut().create_program(
                include_str!("../shaders/surface_box.vert"),
                include_str!("../shaders/surface_box.frag"),
            )?);
        }

        let program = self.surface_box_program.as_ref().unwrap();

        let cache = self.gui_batch_cache.entry(world).or_insert_with(|| {
            super::super::gui_batch::GuiBatchRenderCache::new(self.device.clone())
        });
        for chunk in boxes.chunks(128) {
            cache.draw_box_batch(program, entity, batch_clip, part_class, chunk, mvp, stats)?;
        }

        boxes.clear();

        Ok(())
    }

    #[cfg(feature = "gui")]
    #[allow(clippy::too_many_arguments)]
    fn draw_glyphs_via_atlas(
        &mut self,
        world: &WorldContext<'_>,
        entity: ipp_core::EntityId,
        style: &ipp_core::systems::surface::SurfacePrimitiveStyle,
        font_key: ipp_core::services::asset_management::AssetKey,
        font_size: f32,
        glyphs: &[ipp_core::systems::surface::SurfaceGlyph],
        band: u16,
        mvp: &[f32; 16],
        clip: ipp_core::systems::surface::SurfaceClipRect,
        stats: &mut RenderStats,
    ) -> Result<bool, RenderError> {
        let Some(data) = world
            .asset_resources()
            .get(font_key)
            .and_then(|resource| resource.data())
            .and_then(|asset| {
                asset
                    .as_any()
                    .downcast_ref::<super::super::surface_assets::GlFontData<D>>()
            })
        else {
            return Ok(false);
        };

        let Some(path) = data.path.as_ref() else {
            return Ok(false);
        };

        let font_units_per_em = data.font.units_per_em();
        let scale = band as f32 / font_units_per_em as f32;

        let mut misses = std::collections::BTreeMap::new();
        for glyph in glyphs {
            if let Some(&range) = data.ranges.get(glyph.glyph_id as usize) {
                if range.curve_range[1] == 0
                    || !super::super::glyph_atlas::glyph_intersects_clip(
                        style,
                        glyph,
                        data.glyph_bounds[glyph.glyph_id as usize],
                        font_size / font_units_per_em as f32,
                        clip,
                    )
                {
                    continue;
                }
                let key = super::super::glyph_atlas::GlyphKey {
                    font_key,
                    glyph_id: glyph.glyph_id,
                    resolution_band: band,
                };
                if self.glyph_atlas.get(&key).is_none() {
                    misses.insert(glyph.glyph_id, range);
                }
            }
        }

        if !misses.is_empty() {
            stats.glyph_misses += misses.len() as u32;

            if self.surface_program.is_none() {
                self.surface_program = Some(self.device.borrow_mut().create_program(
                    include_str!("../shaders/surface.vert"),
                    include_str!("../shaders/surface.frag"),
                )?);
            }

            // Any glyph left unpopulated keeps the whole run on the analytic path.
            let mut complete = true;
            for (&glyph_id, &range) in &misses {
                let key = super::super::glyph_atlas::GlyphKey {
                    font_key,
                    glyph_id,
                    resolution_band: band,
                };
                if self.glyph_atlas.population_deferred(&key)
                    || stats.glyph_populates as usize
                        >= super::super::glyph_atlas::MAX_POPULATES_PER_FRAME
                {
                    complete = false;
                    continue;
                }

                let bounds = data.glyph_bounds[glyph_id as usize];
                if !self.populate_glyph(key, path, range, bounds, scale, stats)? {
                    complete = false;
                    break;
                }
            }

            if !complete {
                return Ok(false);
            }
        }

        if self.surface_text_program.is_none() {
            self.surface_text_program = Some(self.device.borrow_mut().create_program(
                include_str!("../shaders/surface_text.vert"),
                include_str!("../shaders/surface_text.frag"),
            )?);
        }

        let program = self.surface_text_program.as_ref().unwrap();
        self.glyph_batch_cache
            .entry(world.id())
            .or_insert_with(|| {
                super::super::glyph_atlas::GlyphBatchRenderCache::new(self.device.clone())
            })
            .draw_text_run(
                program,
                &self.glyph_atlas,
                entity,
                clip,
                style,
                font_key,
                font_size,
                font_units_per_em,
                glyphs,
                band,
                mvp,
                stats,
            )?;

        Ok(true)
    }

    /// Rasterize one glyph's coverage into a new atlas slot.
    ///
    /// Returns `Ok(false)` after a recoverable allocation or rasterization failure: the
    /// unpopulated entry is discarded, the glyph backs off and its text stays analytic
    /// for this frame. Context loss, and any failure to restore the host target, fail
    /// the frame so recovery or the draw-failure path runs.
    #[cfg(feature = "gui")]
    fn populate_glyph(
        &mut self,
        key: super::super::glyph_atlas::GlyphKey,
        path: &D::SurfacePath,
        range: super::super::device::SurfacePathDescriptor,
        bounds: [f32; 4],
        scale: f32,
        stats: &mut RenderStats,
    ) -> Result<bool, RenderError> {
        // Align to full texels and include an antialias texel inside the slot. UV
        // bounds and placed geometry then describe the same area.
        let raster_bounds = [
            ((bounds[0] * scale).floor() - 1.0) / scale,
            ((bounds[1] * scale).floor() - 1.0) / scale,
            ((bounds[2] * scale).ceil() + 1.0) / scale,
            ((bounds[3] * scale).ceil() + 1.0) / scale,
        ];
        let px_w = ((raster_bounds[2] - raster_bounds[0]) * scale).round() as u32;
        let px_h = ((raster_bounds[3] - raster_bounds[1]) * scale).round() as u32;

        let allocated = self
            .glyph_atlas
            .allocate_slot(key, px_w, px_h, raster_bounds);
        let ([slot_x, slot_y], page_idx, _) = match allocated {
            Ok(slot) => slot,
            Err(error) => return self.glyph_population_failed(key, error, stats),
        };
        let Some(page_handle) = self.glyph_atlas.page_handle(page_idx) else {
            let error = RenderError::RenderDevice("glyph atlas page missing".into());
            return self.glyph_population_failed(key, error, stats);
        };

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
        let placement = [
            slot_x as f32 - raster_bounds[0] * scale,
            slot_y as f32 - raster_bounds[1] * scale,
            scale,
            scale,
        ];
        let atlas_clip = [
            slot_x as f32,
            slot_y as f32,
            (slot_x + px_w) as f32,
            (slot_y + px_h) as f32,
        ];
        let color = [1.0, 1.0, 1.0, 1.0];

        let begun = self.device.borrow_mut().begin_glyph_atlas_page(page_handle);
        let drawn = begun.and_then(|()| {
            self.device.borrow_mut().draw_surface_path(
                self.surface_program.as_ref().unwrap(),
                path,
                &bounds,
                range,
                &atlas_mvp,
                &placement,
                &atlas_clip,
                &color,
                0,
            )
        });
        // Restore the target even when beginning or drawing fails. GLES also reports
        // errors of earlier unchecked draws here, so a restore failure fails the frame.
        let restored = self.device.borrow_mut().end_glyph_atlas_page();

        match (drawn, restored) {
            (Ok(()), Ok(())) => {
                stats.glyph_populates += 1;
                Ok(true)
            }
            (Err(RenderError::ContextLost), _) | (_, Err(RenderError::ContextLost)) => {
                self.glyph_population_failed(key, RenderError::ContextLost, stats)
            }
            (_, Err(error)) => {
                // A failed entry must never be sampled as populated coverage.
                self.glyph_atlas.abandon_population(key);
                Err(error)
            }
            (Err(error), Ok(())) => self.glyph_population_failed(key, error, stats),
        }
    }

    /// Discard a failed glyph entry, then propagate context loss or count the failure.
    #[cfg(feature = "gui")]
    fn glyph_population_failed(
        &mut self,
        key: super::super::glyph_atlas::GlyphKey,
        error: RenderError,
        stats: &mut RenderStats,
    ) -> Result<bool, RenderError> {
        self.glyph_atlas.abandon_population(key);
        if error == RenderError::ContextLost {
            return Err(error);
        }

        stats.glyph_population_failures += 1;
        Ok(false)
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
