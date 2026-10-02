//! Retained primitives from immutable Canvas publications.

use super::super::assets::GlTextureData;
use super::super::canvas_scene::{CanvasScene, effective_clip};
use super::super::frame_statistics::RenderFrameWork;
use super::super::retained_surfaces::{CANVAS_SURFACE, SurfacePaint};
use super::canvas_composition::{CanvasChild, CanvasLayering, canvas_attachment};
use super::{RenderError, RenderService};
use crate::RenderDevice;
use ipp_core::systems::canvas::{CanvasClip, CanvasPaintEntry, CanvasPrimitive};

/// One draw of a Canvas frame on the plane of its layer, the last field.
pub(super) enum SurfaceOp {
    /// Committed GUI batches; a range never spans two layers.
    Gui(std::ops::Range<usize>, u32),
    Primitive(usize, CanvasClip, u32),
    Attachment(usize, u32),
}

impl SurfaceOp {
    fn layer(&self) -> u32 {
        match self {
            Self::Gui(_, layer) | Self::Primitive(_, _, layer) | Self::Attachment(_, layer) => {
                *layer
            }
        }
    }
}

struct CanvasDrawFrame<'a> {
    scene: CanvasScene<'a>,
    mvp: [f32; 16],
    clip: CanvasClip,
    opacity: f32,
    layering: CanvasLayering,
    paint: SurfacePaint,
    ops: std::vec::IntoIter<SurfaceOp>,
    stale_parent: bool,
}

impl<D: RenderDevice> RenderService<D> {
    /// Draw a Canvas presentation directly, or its cached image when the cache
    /// selects one. `layering` separates its layers along content Z; nested
    /// Canvases draw on their slot's plane.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_canvas(
        &mut self,
        scene: &CanvasScene<'_>,
        mvp: [f32; 16],
        clip: CanvasClip,
        opacity: f32,
        layering: CanvasLayering,
        viewport: ipp_core::WorldViewport,
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        if self.draw_cached_canvas(scene, &mvp, clip, opacity, stats)? {
            return Ok(());
        }
        self.prepare_surface_program()?;
        self.surface_missing.clear();
        self.surface_analytic_text = false;
        self.surface_gui_unretained = false;
        let mut frames = Vec::new();
        let mut instances = Vec::new();
        let result = (|| {
            frames.push(
                self.prepare_canvas_frame(*scene, mvp, clip, opacity, layering, viewport, stats)?,
            );
            while let Some(frame) = frames.last_mut() {
                let Some(op) = frame.ops.next() else {
                    let frame = frames.pop().expect("completed Canvas frame");
                    self.surface_paint
                        .entry(frame.scene.canvas.selection)
                        .or_default()
                        .drawn(frame.scene.canvas.paint_revision, frame.clip, frame.opacity);
                    self.finish_canvas(frame.scene.canvas.selection);
                    self.inclusions
                        .record(frame.scene.canvas.selection, frame.scene.publication.id);
                    self.inclusions.active.stale_image |= frame.stale_parent;
                    self.surface_ops = frame.ops.collect();
                    continue;
                };
                self.device.borrow_mut().set_surface_double_sided(true)?;
                let mvp = frame.layering.layer_mvp(&frame.mvp, op.layer());
                match op {
                    SurfaceOp::Gui(range, _) => self.draw_gui_work(
                        frame.scene.canvas.selection,
                        range,
                        frame.clip,
                        &mvp,
                        stats,
                    )?,
                    SurfaceOp::Primitive(index, clip, _) => {
                        if let Some(primitive) = frame.scene.primitive(index) {
                            self.draw_surface_primitive(
                                &frame.scene,
                                primitive,
                                clip,
                                frame.paint.entry(frame.scene.replaced(index)),
                                &mvp,
                                stats,
                                &mut instances,
                            )?;
                        }
                    }
                    SurfaceOp::Attachment(index, _) => {
                        match canvas_attachment(
                            &frame.scene,
                            index,
                            mvp,
                            frame.clip,
                            frame.opacity,
                        )? {
                            Some(CanvasChild::Canvas {
                                scene,
                                mvp,
                                clip,
                                opacity,
                            }) => {
                                if self.draw_cached_canvas(&scene, &mvp, clip, opacity, stats)? {
                                    continue;
                                }
                                let frame = self.prepare_canvas_frame(
                                    scene,
                                    mvp,
                                    clip,
                                    opacity,
                                    CanvasLayering::FLAT,
                                    viewport,
                                    stats,
                                )?;
                                frames.push(frame);
                            }
                            Some(CanvasChild::Camera {
                                selection,
                                mvp,
                                extent,
                                clip,
                                opacity,
                            }) => {
                                self.draw_camera_image(
                                    selection, mvp, extent, clip, opacity, stats,
                                )?;
                            }
                            None => {}
                        }
                    }
                }
            }
            Ok(())
        })();
        let restored = self.device.borrow_mut().set_surface_double_sided(false);
        result.and(restored)
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_canvas_frame<'a>(
        &mut self,
        scene: CanvasScene<'a>,
        mvp: [f32; 16],
        clip: CanvasClip,
        opacity: f32,
        layering: CanvasLayering,
        viewport: ipp_core::WorldViewport,
        stats: &mut RenderFrameWork,
    ) -> Result<CanvasDrawFrame<'a>, RenderError> {
        let paint = self
            .surface_paint
            .entry(scene.canvas.selection)
            .or_default()
            .paint(
                scene.canvas.paint_revision,
                scene
                    .canvas
                    .paint_changes
                    .as_ref()
                    .map(|changes| changes.base),
                clip,
                opacity,
            );
        self.prepare_glyph_demand(&scene, mvp, layering, viewport, clip, paint);
        self.populate_glyph_misses(&scene)?;
        let mut ops = std::mem::take(&mut self.surface_ops);
        self.prepare_gui_work(&scene, paint, &mut ops, stats, clip)?;
        // Separated layer planes draw farthest first; each keeps painter
        // order. Positions index the ascending layers in use, not plane ids.
        let layers = &scene.canvas.layers;
        if layering.spacing != 0.0 && layers.len() > 1 {
            let mut position = vec![0; layers.len()];
            for (index, layer) in layering.draw_order(layers).into_iter().enumerate() {
                if let Ok(slot) = layers.binary_search(&layer) {
                    position[slot] = index;
                }
            }
            ops.sort_by_key(|op| {
                layers
                    .binary_search(&op.layer())
                    .map_or(usize::MAX, |slot| position[slot])
            });
        }
        Ok(CanvasDrawFrame {
            scene,
            mvp,
            clip,
            opacity,
            layering,
            paint,
            ops: ops.into_iter(),
            stale_parent: std::mem::take(&mut self.inclusions.active.stale_image),
        })
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

    /// Draw one text, drawing or bitmap primitive that is not retained GUI work.
    #[allow(clippy::too_many_arguments)]
    fn draw_surface_primitive(
        &mut self,
        scene: &CanvasScene<'_>,
        primitive: &CanvasPrimitive,
        clip: CanvasClip,
        paint: super::super::retained_surfaces::SurfacePaint,
        mvp: &[f32; 16],
        stats: &mut RenderFrameWork,
        instances: &mut Vec<super::super::device::SurfacePathInstance>,
    ) -> Result<(), RenderError> {
        use ipp_core::services::asset_management::Asset as _;

        let mut adjusted = *primitive.style();
        adjusted.opacity *= paint.opacity;
        let style = &adjusted;
        match primitive {
            CanvasPrimitive::Glyphs {
                style: _,
                font,
                font_size,
                glyphs,
            } => {
                let Some(data) = scene
                    .resource(*font)
                    .and_then(|resource| resource.data())
                    .and_then(|asset| {
                        asset
                            .as_any()
                            .downcast_ref::<super::super::surface_assets::GlFontData<D>>()
                    })
                else {
                    stats.failed_draw();
                    self.surface_missing.push(*font);
                    return Ok(());
                };

                // Atlas-drawn runs are GUI work; this run draws analytically.
                {
                    self.surface_analytic_text = true;
                }

                let unit = *font_size / data.font.units_per_em() as f32;
                let Some(path) = data.path.as_ref() else {
                    // A font without curves has no path; one whose GPU
                    // data was released is not resident yet.
                    if data.graphics_ready() == Some(false) {
                        self.surface_missing.push(*font);
                    }
                    return Ok(());
                };
                if self.surface_instance_program.is_none() {
                    self.surface_instance_program = Some(self.device.borrow_mut().create_program(
                        crate::services::render::embedded_shader!("shaders/surface_instanced.vert"),
                        crate::services::render::embedded_shader!("shaders/surface.frag"),
                    )?);
                }

                let run = super::super::analytic_glyphs::AnalyticGlyphRun {
                    entity: CANVAS_SURFACE,
                    style,
                    clip,
                    font_key: *font,
                    font_size: *font_size,
                    glyphs,
                };
                let build = |instances: &mut Vec<super::super::device::SurfacePathInstance>| {
                    for glyph in glyphs.iter() {
                        let Some(&range) = data.ranges.get(glyph.glyph_id as usize) else {
                            continue;
                        };
                        if range.curve_range[1] == 0 {
                            continue;
                        }
                        let bounds = data.glyph_bounds[glyph.glyph_id as usize];
                        if !super::super::glyph_atlas::glyph_intersects_clip(
                            style, glyph, bounds, unit, clip,
                        ) {
                            continue;
                        }
                        let tint = style.glyph_tint(glyph);
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
                    .entry(scene.canvas.selection)
                    .or_insert_with(|| {
                        super::super::analytic_glyphs::AnalyticGlyphCache::new(device.clone())
                    })
                    .draw_run(program, path, &run, paint, mvp, instances, build, stats)?;
            }
            CanvasPrimitive::Drawing {
                style: _,
                drawing,
            } => {
                let Some(data) = scene
                    .resource(*drawing)
                    .and_then(|resource| resource.data())
                    .and_then(|asset| {
                        asset
                            .as_any()
                            .downcast_ref::<super::super::surface_assets::GlDrawingData<D>>()
                    })
                else {
                    stats.failed_draw();
                    self.surface_missing.push(*drawing);
                    return Ok(());
                };
                let placement = [
                    style.position[0],
                    style.position[1],
                    style.scale[0],
                    style.scale[1],
                ];
                let Some(path) = data.path.as_ref() else {
                    if data.graphics_ready() == Some(false) {
                        self.surface_missing.push(*drawing);
                    }
                    return Ok(());
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
                        program, path, &bounds, range, mvp, &placement, &clip, &color, fill_rule,
                    )?;
                    stats.draw(2);
                }
            }
            CanvasPrimitive::Bitmap {
                style: _,
                bitmap,
                size,
            } => {
                let Some(texture) = scene
                    .resource(*bitmap)
                    .and_then(|resource| resource.data())
                    .and_then(|asset| asset.as_any().downcast_ref::<GlTextureData<D>>())
                    .and_then(|data| data.gpu.as_ref())
                else {
                    stats.failed_draw();
                    self.surface_missing.push(*bitmap);
                    return Ok(());
                };
                if self.surface_bitmap_program.is_none() {
                    self.surface_bitmap_program = Some(self.device.borrow_mut().create_program(
                        crate::services::render::embedded_shader!("shaders/surface_bitmap.vert"),
                        crate::services::render::embedded_shader!("shaders/surface_bitmap.frag"),
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
                stats.draw(2);
            }
            CanvasPrimitive::Box {
                ..
            } => {
                stats.failed_draw();
            }
        }

        Ok(())
    }

    /// Collect the Surface's retained GUI work in painter order and commit it to the
    /// Surface's storage.
    ///
    /// Boxes and text runs whose atlas entries are all resident become retained
    /// batches; consecutive batches of one layer form one [`SurfaceOp::Gui`]
    /// range whatever their clips, which draws its shapes and then its glyphs in the
    /// [GUI draw order](super::super::gui_draw_order). Painter order groups each
    /// layer, so a canvas with layers adds one batch and range boundary per layer,
    /// whatever the spacing, and a spacing change keeps every batch. Every other
    /// visible primitive becomes a [`SurfaceOp::Primitive`].
    ///
    /// When the Surface has no usable storage after a recoverable allocation or write
    /// failure, or while it backs off from one, its boxes are skipped for the frame and
    /// counted as one failed draw, its text draws analytically and
    /// `surface_gui_unretained` is set. Other Surfaces and the rest of the frame are
    /// unaffected.
    fn prepare_gui_work(
        &mut self,
        scene: &CanvasScene<'_>,
        mut paint: super::super::retained_surfaces::SurfacePaint,
        ops: &mut Vec<SurfaceOp>,
        stats: &mut RenderFrameWork,
        parent_clip: CanvasClip,
    ) -> Result<(), RenderError> {
        let device = &self.device;
        let cache = self
            .gui_batch_cache
            .entry(scene.canvas.selection)
            .or_insert_with(|| super::super::gui_batch::GuiBatchRenderCache::new(device.clone()));
        let mut glyphs = self.glyph_batch_cache.get_mut(&scene.canvas.selection);
        cache.begin_surface(CANVAS_SURFACE);

        // Custom paints take their slots and parameter blocks before the boxes that
        // name them are hashed; changed lanes rehash every box of the canvas.
        if !scene.canvas.paints.is_empty() || !cache.paints.is_empty() {
            let output = scene.canvas.selection;
            let programs = &mut self.canvas_paints;
            let fallbacks = &mut self.canvas_paint_fallbacks;
            cache.paints.prepare(
                &scene.canvas.paints,
                |instance| admit_paint::<D>(scene, programs, instance),
                |instance, reason| record_paint_fallback(fallbacks, output, instance, reason),
            );
            fallbacks.retain(|(fallback, entity), _| {
                *fallback != output
                    || scene
                        .canvas
                        .paints
                        .iter()
                        .any(|instance| instance.target.entity == *entity)
            });
            if cache.paints.lanes_changed() {
                paint.invalidate();
            }
        }

        let mut boxes = Vec::new();
        let mut gui_start = 0;
        let mut layer = 0;
        for (index, entry) in scene.canvas.entries.iter().enumerate() {
            if entry.layer() != layer {
                cache.push_boxes(&boxes, stats);
                boxes.clear();
                if cache.piece_count() > gui_start {
                    ops.push(SurfaceOp::Gui(gui_start..cache.piece_count(), layer));
                    gui_start = cache.piece_count();
                }
                layer = entry.layer();
            }
            let CanvasPaintEntry::Primitive {
                primitive,
                ..
            } = entry.as_ref()
            else {
                cache.push_boxes(&boxes, stats);
                boxes.clear();
                if cache.piece_count() > gui_start {
                    ops.push(SurfaceOp::Gui(gui_start..cache.piece_count(), layer));
                    gui_start = cache.piece_count();
                }
                ops.push(SurfaceOp::Attachment(index, layer));
                continue;
            };
            // Intersect the per-primitive clip with the root content rectangle
            // on every path. An empty intersection suppresses the primitive:
            // no draw call, no triangles, no effect on painter order or depth.
            let Some(clip) = effective_clip(primitive.style(), parent_clip) else {
                continue;
            };

            match primitive {
                CanvasPrimitive::Box {
                    size,
                    ..
                } => {
                    // The clip above is already non-empty; this re-check guards the
                    // device against invalid box dimensions in hand-built submissions.
                    if size.iter().all(|value| value.is_finite() && *value > 0.0) {
                        boxes.push((primitive, clip, paint.entry(scene.replaced(index))));
                    }
                    continue;
                }
                CanvasPrimitive::Glyphs {
                    style,
                    font,
                    font_size,
                    glyphs: run_glyphs,
                } => {
                    let data = scene
                        .resource(*font)
                        .and_then(|resource| resource.data())
                        .and_then(|asset| {
                            asset
                                .as_any()
                                .downcast_ref::<super::super::surface_assets::GlFontData<D>>()
                        });
                    if let (Some(data), Some(glyphs)) = (data, glyphs.as_deref_mut()) {
                        let mut adjusted = *style;
                        adjusted.opacity *= paint.opacity;
                        let style = &adjusted;
                        let run = super::super::glyph_atlas::TextRun {
                            entity: CANVAS_SURFACE,
                            style,
                            clip,
                            font_key: *font,
                            font_size: *font_size,
                            units_per_em: data.font.units_per_em(),
                            glyphs: run_glyphs,
                        };
                        if glyphs.prepare_text_run(&self.glyph_atlas, &run, stats) {
                            cache.push_boxes(&boxes, stats);
                            boxes.clear();
                            cache.push_glyphs(glyphs.run_pieces(CANVAS_SURFACE, style.identity));
                            continue;
                        }
                    }
                }
                _ => {}
            }

            // Any other primitive ends the current GUI range.
            cache.push_boxes(&boxes, stats);
            boxes.clear();
            if cache.piece_count() > gui_start {
                ops.push(SurfaceOp::Gui(gui_start..cache.piece_count(), layer));
                gui_start = cache.piece_count();
            }
            ops.push(SurfaceOp::Primitive(index, clip, layer));
        }

        cache.push_boxes(&boxes, stats);
        if cache.piece_count() > gui_start {
            ops.push(SurfaceOp::Gui(gui_start..cache.piece_count(), layer));
        }

        let glyphs = glyphs.as_deref();
        let committed = cache.commit_surface(
            |identity, batch| {
                glyphs.map_or(&[], |glyphs| {
                    glyphs.batch_records(CANVAS_SURFACE, identity, batch)
                })
            },
            stats,
        )?;
        if committed {
            return Ok(());
        }

        self.surface_gui_unretained = true;
        stats.failed_draw();
        ops.clear();
        for (index, entry) in scene.canvas.entries.iter().enumerate() {
            let CanvasPaintEntry::Primitive {
                primitive,
                ..
            } = entry.as_ref()
            else {
                ops.push(SurfaceOp::Attachment(index, entry.layer()));
                continue;
            };
            let Some(clip) = effective_clip(primitive.style(), parent_clip) else {
                continue;
            };

            if !matches!(primitive, CanvasPrimitive::Box { .. }) {
                ops.push(SurfaceOp::Primitive(index, clip, entry.layer()));
            }
        }

        Ok(())
    }

    /// Draw committed GUI batches `range` of the current Surface, within the canvas
    /// content rectangle `clip`: shapes through the canvas program, holding the
    /// Surface's paint parameter blocks, and glyphs through the glyph program.
    fn draw_gui_work(
        &mut self,
        output: ipp_core::OutputRef,
        range: std::ops::Range<usize>,
        clip: CanvasClip,
        mvp: &[f32; 16],
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        let Some(cache) = self.gui_batch_cache.get_mut(&output) else {
            return Ok(());
        };
        if self.gui_glyph_program.is_none() {
            self.gui_glyph_program = Some(self.device.borrow_mut().create_program(
                crate::services::render::embedded_shader!("shaders/surface_glyph.vert"),
                crate::services::render::embedded_shader!("shaders/surface_glyph.frag"),
            )?);
        }
        let shapes = self.canvas_paints.prepare_draw(
            &mut self.device.borrow_mut(),
            output,
            &cache.paints,
        )?;
        let glyphs = self
            .gui_glyph_program
            .as_ref()
            .expect("glyph program created above");
        let atlas = &self.glyph_atlas;
        cache.draw_pieces(
            shapes,
            glyphs,
            range,
            clip,
            |page| atlas.page_texture(page),
            mvp,
            stats,
        )
    }

    /// Canvas entities whose custom paint draws their colour, with the reason, by
    /// canvas output and entity, separate from semantic World outcomes.
    pub fn canvas_paint_diagnostics(
        &self,
    ) -> &std::collections::BTreeMap<
        (ipp_core::OutputRef, ipp_core::EntityId),
        super::super::canvas_paint::CanvasPaintFallback,
    > {
        &self.canvas_paint_fallbacks
    }

    /// Canvas programs built so far, each after a paint entered a slot or the
    /// context was replaced.
    pub fn canvas_program_builds(&self) -> u32 {
        self.canvas_paints.builds()
    }

    fn prepare_glyph_demand(
        &mut self,
        scene: &CanvasScene<'_>,
        mvp: [f32; 16],
        layering: CanvasLayering,
        viewport: ipp_core::WorldViewport,
        clip: CanvasClip,
        paint: SurfacePaint,
    ) {
        use super::super::glyph_atlas::{TextRun, projected_glyph_height};
        let atlas = &mut self.glyph_atlas;
        let work = &mut self.glyph_frame;
        let cache = self
            .glyph_batch_cache
            .entry(scene.canvas.selection)
            .or_default();
        atlas.begin_publication();
        cache.begin_publication();
        for (index, entry) in scene.canvas.entries.iter().enumerate() {
            let CanvasPaintEntry::Primitive {
                primitive:
                    CanvasPrimitive::Glyphs {
                        style,
                        font,
                        font_size,
                        glyphs,
                    },
                ..
            } = entry.as_ref()
            else {
                continue;
            };
            let Some(clip) = effective_clip(style, clip) else {
                continue;
            };
            let Some(data) = scene
                .resource(*font)
                .and_then(|resource| resource.data())
                .and_then(|asset| {
                    asset
                        .as_any()
                        .downcast_ref::<super::super::surface_assets::GlFontData<D>>()
                })
            else {
                continue;
            };
            let mut adjusted = *style;
            adjusted.opacity *= paint.opacity;
            let style = &adjusted;
            let run = TextRun {
                entity: CANVAS_SURFACE,
                style,
                clip,
                font_key: *font,
                font_size: *font_size,
                units_per_em: data.font.units_per_em(),
                glyphs,
            };
            let height = projected_glyph_height(
                &layering.layer_mvp(&mvp, style.layer),
                style.position,
                *font_size * style.scale[1],
                (viewport.width, viewport.height),
            );
            let bounds = |glyph_id: u32| {
                data.ranges
                    .get(glyph_id as usize)
                    .filter(|range| range.curve_range[1] != 0)
                    .and_then(|_| data.glyph_bounds.get(glyph_id as usize).copied())
            };
            cache.publish_run(
                atlas,
                &run,
                paint.entry(scene.replaced(index)),
                height,
                bounds,
                work,
            );
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
    pub(super) fn populate_glyph_misses(
        &mut self,
        scene: &CanvasScene<'_>,
    ) -> Result<(), RenderError> {
        let queue = self.glyph_frame.take_queue(
            self.glyph_population
                .allowance()
                .saturating_sub((self.glyph_frame.populates + self.glyph_frame.failures) as usize),
        );
        if queue.is_empty() {
            self.glyph_frame.restore_queue(queue);
            return Ok(());
        }

        // Native passes refine the per-glyph cost; WebGL keeps its estimate because
        // its draws execute in another process.
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();
        let result = self.populate_glyphs(scene, &queue);
        #[cfg(not(target_arch = "wasm32"))]
        if result.is_ok() {
            self.glyph_population
                .record(queue.len(), started.elapsed().as_secs_f64() * 1e3);
        }

        self.glyph_frame.restore_queue(queue);
        result
    }

    fn populate_glyphs(
        &mut self,
        scene: &CanvasScene<'_>,
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
            let Some(data) = scene
                .resource(key.font_key)
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

    fn finish_canvas(&mut self, output: ipp_core::OutputRef) {
        let live = std::collections::BTreeSet::from([CANVAS_SURFACE]);
        let submitted = super::super::retained_surfaces::RetainedSurfaceSubmission {
            live: &live,
            submitted: &live,
        };
        if let Some(cache) = self.gui_batch_cache.get_mut(&output) {
            cache.finish_frame(Some(&submitted));
        }
        if let Some(cache) = self.analytic_glyphs.get_mut(&output) {
            cache.finish_frame(Some(&submitted));
        }
    }

    pub(super) fn publish_retained_surface_statistics(
        &mut self,
        statistics: &mut crate::RenderStatistics,
    ) {
        statistics.analytic_glyph_resident_bytes = self
            .analytic_glyphs
            .values()
            .map(|cache| cache.resident_bytes())
            .sum::<usize>()
            .min(u32::MAX as usize) as u32;
        statistics.gui_resident_bytes = self
            .gui_batch_cache
            .values()
            .map(|cache| cache.resident_bytes())
            .sum::<usize>()
            .min(u32::MAX as usize) as u32;
        self.glyph_frame.publish(statistics);
        let counts = self.surface_cache.counts();
        statistics.surface_cache_repaints = counts.repaints;
        statistics.surface_cache_reuses = counts.reuses;
        statistics.surface_cache_direct = counts.direct;
        statistics.surface_cache_fallbacks = counts.fallbacks;
        statistics.surface_cache_animated = counts.animated;
        statistics.surface_cache_allocations = counts.allocations;
        let (entries, bytes) = self.surface_cache.resident();
        statistics.surface_cache_entries = entries;
        statistics.surface_cache_resident_bytes = bytes;
        statistics.glyph_page_retirements = self.glyph_atlas.take_retired_pages();
        statistics.glyph_pages = self.glyph_atlas.page_count();
        statistics.glyph_resident_bytes = self.glyph_atlas.resident_bytes();
    }
}

/// The slot and source of `instance`'s paint: its loaded, validated shader admitted
/// to the canvas program.
fn admit_paint<'a, D: RenderDevice>(
    scene: &CanvasScene<'a>,
    programs: &mut super::super::canvas_paint::CanvasPaintPrograms<D>,
    instance: &ipp_core::systems::canvas::CanvasPaintInstance,
) -> Result<
    (u32, &'a super::super::canvas_paint::CanvasPaintSource),
    super::super::canvas_paint::CanvasPaintFallbackReason,
> {
    use super::super::canvas_paint::CanvasPaintFallbackReason;

    let shader = instance
        .shader
        .ok_or(CanvasPaintFallbackReason::Unavailable)?;
    let data = scene
        .resource(shader)
        .and_then(|resource| resource.data())
        .and_then(|asset| {
            asset
                .as_any()
                .downcast_ref::<super::super::shader_asset::GlShaderData<D>>()
        })
        .ok_or(CanvasPaintFallbackReason::Unavailable)?;
    let source = data
        .paint
        .as_ref()
        .ok_or(CanvasPaintFallbackReason::NotAPaint)?;
    if !data.paint_ready {
        return Err(CanvasPaintFallbackReason::Unavailable);
    }
    Ok((programs.admit(shader, source)?, source))
}

/// Retain `instance`'s fallback `reason`, logging it only when it changes, or clear
/// it when the instance paints.
fn record_paint_fallback(
    fallbacks: &mut std::collections::BTreeMap<
        (ipp_core::OutputRef, ipp_core::EntityId),
        super::super::canvas_paint::CanvasPaintFallback,
    >,
    output: ipp_core::OutputRef,
    instance: &ipp_core::systems::canvas::CanvasPaintInstance,
    reason: Option<super::super::canvas_paint::CanvasPaintFallbackReason>,
) {
    let key = (output, instance.target.entity);
    let Some(reason) = reason else {
        fallbacks.remove(&key);
        return;
    };
    if fallbacks
        .get(&key)
        .is_some_and(|fallback| fallback.reason == reason && *fallback.source == *instance.source)
    {
        return;
    }

    ipp_core::diagnostic!(
        Warn,
        "canvas paint fallback output={output:?} entity={:?}: {}: {reason}",
        instance.target.entity,
        instance.source
    );
    fallbacks.insert(
        key,
        super::super::canvas_paint::CanvasPaintFallback {
            source: instance.source.to_string(),
            reason,
        },
    );
}

fn srgb(value: u8) -> f32 {
    let encoded = f32::from(value) / 255.0;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}
