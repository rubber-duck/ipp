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
        let program = self.surface_program.as_ref().unwrap();
        let mvp = camera::multiply(view_projection, item.model);
        for primitive in &item.primitives {
            // Intersect the per-primitive clip with the root content rectangle
            // on every path. An empty intersection suppresses the primitive:
            // no draw call, no triangles, no effect on painter order or depth.
            let Some(clip) = ipp_core::primitive_effective_clip(primitive.style(), item.clip_size)
            else {
                continue;
            };
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
                    for (layer, &range) in data.drawing.layers().iter().zip(&data.ranges) {
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
                        self.device.borrow_mut().draw_surface_path(
                            program,
                            path,
                            &data.drawing.bounds(),
                            range,
                            &mvp,
                            &placement,
                            &clip,
                            &color,
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
                    style,
                    size,
                    corner_radius,
                    border_width,
                    border_color,
                    ..
                } => {
                    // The clip above is already non-empty; this re-check
                    // guards the device against invalid box dimensions in
                    // hand-built submissions with NaN uniforms.
                    if !ipp_core::surface_primitive_visible(primitive, item.clip_size) {
                        continue;
                    }
                    if self.surface_box_program.is_none() {
                        self.surface_box_program = Some(self.device.borrow_mut().create_program(
                            include_str!("../shaders/surface_box.vert"),
                            include_str!("../shaders/surface_box.frag"),
                        )?);
                    }
                    let program = self.surface_box_program.as_ref().unwrap();
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
                    let border = [
                        border_color[0],
                        border_color[1],
                        border_color[2],
                        border_color[3] * style.opacity,
                    ];
                    let shape = super::super::device::SurfaceBoxShape {
                        corner: *corner_radius,
                        border: *border_width,
                    };
                    self.device.borrow_mut().draw_surface_box(
                        program, &mvp, &placement, &clip, &color, &border, shape,
                    )?;
                    stats.draw_calls += 1;
                    stats.triangles += 2;
                }
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
        Ok(())
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
