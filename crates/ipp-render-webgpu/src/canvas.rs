//! Prepare and batch compatible adjacent Canvas draws in publication order.
use crate::{
    canvas_device::{CanvasPipeline, CanvasProgram},
    device::Graphics,
    paths::{Path, PathCache},
    shaders::PaintParameters,
};
use ipp_core::{
    HostRuntime,
    services::asset_management::formats::{drawing::DrawingAsset, font::FontAsset},
    systems::canvas::{CanvasAffine, CanvasClip, CanvasPrimitive, CanvasShapeFill, CanvasSpace},
};
use ipp_render_scene::canvas::*;
use std::rc::Rc;

pub(super) enum CanvasBinding<'a> {
    Layer(&'a wgpu::BindGroup, u32),
    Screen(&'a wgpu::BindGroup),
    Bitmap(&'a wgpu::BindGroup),
    Path {
        buffer: &'a wgpu::BindGroup,
        start: u32,
    },
}

pub(super) struct CanvasAssets<'a> {
    pub host: &'a HostRuntime,
    pub device: &'a Graphics,
    pub paths: &'a mut PathCache,
}

#[derive(Clone)]
pub(super) struct Run {
    pub start: u32,
    pub end: u32,
    pub pipeline: CanvasPipeline,
    pub masked: bool,
    pub program: Option<Rc<CanvasProgram>>,
    pub image: Option<wgpu::BindGroup>,
    pub path: Option<wgpu::BindGroup>,
}

#[derive(Default)]
pub(super) struct Draws {
    pub owner: Option<u32>,
    pub vertices: Vec<CanvasVertex>,
    pub records: Vec<[f32; 4]>,
    pub runs: Vec<Run>,
    pub failed: u32,
    pub mask_pixels: u32,
}

pub(super) struct CanvasPass<'a, 'b> {
    pub draws: &'a mut Draws,
    pub projection: &'a CanvasProjection<'b>,
    pub plot_target: Option<[u32; 2]>,
    pub light_offset: u32,
    pub mapping: u32,
}

impl CanvasPass<'_, '_> {
    pub(super) fn draw_image(
        &mut self,
        placement: &CanvasPlacement,
        image: CanvasBinding<'_>,
        extent: [f32; 2],
    ) {
        let paint = CanvasPaint {
            tint: [1.0, 1.0, 1.0, placement.opacity],
            bounds: [0.0, 0.0, extent[0], extent[1]],
            options: [0.0, 2.0, 0.0, 0.0],
            clip: [-f32::MAX, -f32::MAX, f32::MAX, f32::MAX],
            ..Default::default()
        };
        self.draw(placement, &paint, Some(image), None);
    }

    pub(super) fn primitive(
        &mut self,
        sources: &PaintSources<'_>,
        placement: &CanvasPlacement,
        space: Option<&CanvasSpace>,
        primitive: &CanvasPrimitive,
        assets: CanvasAssets<'_>,
    ) {
        let CanvasAssets {
            host,
            device,
            paths,
        } = assets;
        let style = primitive.style();
        let mut style = *style;
        style.opacity *= placement.opacity;
        if style.opacity <= 0.0 || style.scale.contains(&0.0) {
            return;
        }

        let Some(placement) = placement.clipped(space, style.clip) else {
            return;
        };

        #[cfg(target_arch = "wasm32")]
        let web_binding;
        let mut authored = None;
        let resolve = |target| {
            crate::shaders::AuthoredPaint::resolve(
                host,
                sources.publication(),
                sources.paint(target)?,
            )
        };
        let (size, mut paint, image) = match primitive {
            CanvasPrimitive::Box {
                size,
                corner_radius,
                border_width,
                border_color,
                fill,
                glow,
                shape,
                ..
            } => {
                let scale = style.scale.map(f32::abs);
                let size = [size[0] * scale[0], size[1] * scale[1]];
                let mut paint = box_paint(
                    &style,
                    size,
                    *corner_radius,
                    *border_width,
                    *border_color,
                    *fill,
                    *shape,
                );
                if let CanvasShapeFill::Paint {
                    color,
                    paint: target,
                    data,
                    bounds,
                } = fill
                {
                    authored = resolve(*target);
                    if authored.is_some() {
                        let [x, y, w, h] =
                            bounds.unwrap_or([0.0, 0.0, size[0] / scale[0], size[1] / scale[1]]);
                        let mut input = style;
                        input.opacity = 1.0;
                        paint.fill[0] = tint(&input, *color);
                        paint.options[0] += 5.0;
                        paint.paint_origin_scale = [x * scale[0], y * scale[1], scale[0], scale[1]];
                        paint.paint_size_opacity = [w, h, style.opacity, 1.0];
                        paint.paint_data = *data;
                    }
                }
                if let Some(glow) = glow {
                    let length = scale[0].max(scale[1]);
                    paint.glow = [
                        0.0,
                        glow.inner_radius * length,
                        glow.radius * length,
                        glow.falloff,
                    ];
                    paint.glow_color = tint(&style, glow.color);
                    paint.glow_color[3] *= glow.intensity;
                }
                (size, paint, None)
            }
            CanvasPrimitive::Bitmap {
                bitmap,
                size,
                ..
            } => {
                let Some(texture) = crate::scene::resource::<crate::assets::WebGpuTexture>(
                    host,
                    sources.publication(),
                    bitmap.to_u64(),
                )
                .and_then(|image| image.texture.as_ref()) else {
                    self.draws.failed += 1;
                    return;
                };
                let size = [
                    size[0] * style.scale[0].abs(),
                    size[1] * style.scale[1].abs(),
                ];
                let paint = CanvasPaint {
                    tint: tint(&style, [1.0; 4]),
                    bounds: [0.0, 0.0, size[0], size[1]],
                    options: [0.0, 1.0, 0.0, 0.0],
                    ..Default::default()
                };
                (size, paint, Some(CanvasBinding::Bitmap(&texture.1)))
            }
            CanvasPrimitive::Glyphs {
                font,
                font_size,
                glyphs,
                ..
            } => {
                let Some(data) = decoded::<FontAsset>(host, sources.publication(), *font) else {
                    self.draws.failed += 1;
                    return;
                };
                let unit = *font_size / data.units_per_em() as f32;
                for glyph in glyphs.iter() {
                    let Some(outline) = data.glyph(glyph.glyph_id) else {
                        continue;
                    };
                    if let Some(path) =
                        paths.glyph(device, *font, glyph.glyph_id, &outline.contours)
                    {
                        let origin = std::array::from_fn(|i| {
                            style.position[i] + style.scale[i] * glyph.position[i]
                        });
                        let scale = [unit * style.scale[0], -unit * style.scale[1]];
                        let local = CanvasAffine::axis_aligned(origin, scale);
                        let color = style.glyph_tint(glyph);
                        self.draw_path(
                            &placement,
                            style.clip,
                            local,
                            path,
                            [color[0], color[1], color[2], color[3] * style.opacity],
                            None,
                        );
                    }
                }
                return;
            }
            CanvasPrimitive::Drawing {
                drawing,
                ..
            } => {
                let Some(data) = decoded::<DrawingAsset>(host, sources.publication(), *drawing)
                else {
                    self.draws.failed += 1;
                    return;
                };
                let local =
                    CanvasAffine::axis_aligned(style.position, [style.scale[0], -style.scale[1]]);
                for (i, layer) in data.layers().iter().enumerate() {
                    if let Some(path) = paths.drawing(device, *drawing, i as u32, layer) {
                        let [r, g, b, a] = layer.color.map(|v| f32::from(v) / 255.0);
                        self.draw_path(
                            &placement,
                            style.clip,
                            local,
                            path,
                            tint(&style, [linear(r), linear(g), linear(b), a]),
                            None,
                        );
                    }
                }
                return;
            }
            CanvasPrimitive::Path {
                path,
                paint,
                ..
            } => {
                let custom = paint.and_then(resolve);
                if let Some(path) = paths.plot(device, path) {
                    let mut input = style;
                    if custom.is_some() {
                        input.opacity = 1.0;
                    }
                    let local = CanvasAffine::axis_aligned(style.position, style.scale);
                    self.draw_path(
                        &placement,
                        style.clip,
                        local,
                        path,
                        tint(&input, [1.0; 4]),
                        custom
                            .as_ref()
                            .map(|paint| (paint.parameters(), style.opacity)),
                    );
                } else if !path.is_empty() {
                    self.draws.failed += 1;
                }
                return;
            }
            #[cfg(target_arch = "wasm32")]
            CanvasPrimitive::Html {
                image,
                size,
                ..
            } => {
                let Some(id) = image else {
                    return;
                };
                if host
                    .publication_web_image(sources.publication(), *id)
                    .is_none()
                {
                    return;
                }
                let Some(binding) = device.web_images.borrow().binding(*id) else {
                    self.draws.failed += 1;
                    return;
                };
                web_binding = binding;
                let size = [
                    size[0] * style.scale[0].abs(),
                    size[1] * style.scale[1].abs(),
                ];
                let paint = CanvasPaint {
                    tint: tint(&style, [1.; 4]),
                    bounds: [0., 0., size[0], size[1]],
                    options: [0., 1., 0., 0.],
                    ..Default::default()
                };
                (size, paint, Some(CanvasBinding::Bitmap(&web_binding)))
            }
            #[cfg(not(target_arch = "wasm32"))]
            CanvasPrimitive::Html {
                image,
                ..
            } => {
                if image.is_some() {
                    self.draws.failed += 1;
                }
                return;
            }
        };
        if size.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return;
        }

        let local = CanvasAffine::axis_aligned(style.position, style.scale.map(f32::signum));
        let affine = placement.affine.compose(local);
        paint.clip = local
            .inverse()
            .expect("signs are nonzero")
            .bounds(style.clip);

        // Derivatives in the fragment cover the analytic contour and its glow.
        let margin = if image.is_some() {
            0.0
        } else {
            paint.glow[2].max(0.0) + 2.0
        };
        let content = corners([-margin, -margin, size[0] + margin, size[1] + margin])
            .map(|point| affine.point(point));
        let polygon = intersect(placement.polygon, &content);
        let placement = CanvasPlacement {
            affine,
            polygon,
            opacity: placement.opacity,
        };
        self.draw(
            &placement,
            &paint,
            image,
            authored
                .as_ref()
                .map(crate::shaders::AuthoredPaint::parameters),
        );
    }

    fn draw_path(
        &mut self,
        placement: &CanvasPlacement,
        clip: CanvasClip,
        local: CanvasAffine,
        path: &Path,
        color: [f32; 4],
        custom: Option<(PaintParameters<'_>, f32)>,
    ) {
        let Some(inverse) = local.inverse() else {
            return;
        };
        let affine = placement.affine.compose(local);
        let clip = inverse.bounds(clip);
        let [l, t, r, b] = path.bounds;
        let margin = ((r - l).max(b - t) / 16.0).max(1.0);
        let [mx, my] = self
            .plot_target
            .and_then(|size| plane_pixel_extent(self.projection, affine, size))
            .map_or([margin; 2], |pixel| pixel.map(|v| v.min(margin)));
        let content = corners([l - mx, t - my, r + mx, b + my]).map(|point| affine.point(point));
        let polygon = intersect(placement.polygon.clone(), &content);
        let placement = CanvasPlacement {
            affine,
            polygon,
            opacity: placement.opacity,
        };
        let mut paint = CanvasPaint {
            tint: color,
            clip,
            options: [0.0, path.mode, 0.0, path.radius],
            ..Default::default()
        };
        if let Some((_, opacity)) = &custom {
            paint.paint_origin_scale = [l, t, 1.0, 1.0];
            paint.paint_size_opacity = [r - l, b - t, *opacity, 1.0];
        }
        self.draw(
            &placement,
            &paint,
            Some(CanvasBinding::Path {
                buffer: &path.buffer,
                start: path.start,
            }),
            custom.map(|(parameters, _)| parameters),
        );
    }

    fn draw(
        &mut self,
        placement: &CanvasPlacement,
        paint: &CanvasPaint,
        image: Option<CanvasBinding<'_>>,
        custom: Option<PaintParameters<'_>>,
    ) {
        if placement.polygon.len() < 3 {
            return;
        }
        let Some(inverse) = placement.affine.inverse() else {
            return;
        };

        let screen = matches!(image, Some(CanvasBinding::Screen(_)));
        let start = self.draws.vertices.len();
        let analytic = if let CanvasProjection::Analytic(surface) = self.projection {
            let Some(primitive) =
                surface.draw(&placement.polygon, bounds(&placement.polygon), inverse)
            else {
                return;
            };
            let cover = primitive.draw.cover();
            for i in [0, 1, 2, 2, 1, 3] {
                let point = cover[i];
                self.draws.vertices.push(CanvasVertex {
                    clip: [point[0], point[1], 0.0, 1.0],
                    local: [(point[0] + 1.) * 0.5, (1. - point[1]) * 0.5],
                    world: primitive.mapping.ray_origin(point),
                    right: [0.; 3],
                    down: [0.; 3],
                    normal: [0.; 3],
                    slot: 0,
                });
            }
            Some(primitive)
        } else {
            self.projection
                .triangulate(placement, inverse, &mut self.draws.vertices);
            None
        };
        if self.draws.vertices.len() == start {
            return;
        }
        let slot = self.draws.records.len() as u32;
        for vertex in &mut self.draws.vertices[start..] {
            vertex.slot = slot;
        }
        self.draws
            .records
            .extend_from_slice(bytemuck::cast_slice(std::slice::from_ref(paint)));
        let selected = match image {
            Some(CanvasBinding::Layer(_, id)) => Some(id),
            _ => None,
        };
        let (image, path, offset) = match image {
            Some(CanvasBinding::Layer(bind, _)) => (Some(bind), None, 0),
            Some(CanvasBinding::Bitmap(bind) | CanvasBinding::Screen(bind)) => {
                (Some(bind), None, 0)
            }
            Some(CanvasBinding::Path {
                buffer,
                start,
            }) => (None, Some(buffer), start),
            None => (None, None, 0),
        };
        let address = self.draws.records.len();
        self.draws.records.push([
            offset as f32,
            self.light_offset as f32,
            0.0,
            self.mapping as f32,
        ]);
        // Split each ID into two exactly representable 16-bit floats in the floating-point paint records.
        let parts = |v: u32| [(v & 65535) as f32, (v >> 16) as f32];
        let owner = parts(self.draws.owner.unwrap_or(0));
        let layer = selected.map_or([-1.; 2], parts);
        self.draws
            .records
            .push([owner[0], owner[1], layer[0], layer[1]]);
        if let Some(custom) = &custom {
            self.draws.records.extend_from_slice(custom.values);
        }
        let pipeline = if let Some(analytic) = analytic {
            self.draws.records[address][2] = self.draws.records.len() as f32;
            self.draws.records.extend(analytic.draw.records());
            self.draws.records.extend(analytic.edges);
            if screen {
                CanvasPipeline::Screen
            } else {
                CanvasPipeline::Analytic
            }
        } else if self.projection.depth() {
            CanvasPipeline::DepthTested
        } else if paint.options[1] == 2. && selected.is_none() {
            CanvasPipeline::Panel
        } else {
            CanvasPipeline::Overlay
        };

        let rect = match (self.plot_target, pipeline, path, custom.as_ref()) {
            (Some(size), CanvasPipeline::DepthTested, Some(_), None) => {
                ipp_render_scene::path_masks::reserve(
                    &self.draws.vertices[start..],
                    size,
                    &mut self.draws.mask_pixels,
                )
            }
            _ => None,
        };
        if let Some(rect) = rect {
            // Unpainted paths use tint, leaving the first fill record free for sample addressing.
            self.draws.records[slot as usize + 2] = rect;
        }
        let masked = rect.is_some();
        let program = custom.map(|c| c.program);
        let end = self.draws.vertices.len() as u32;
        if let Some(run) = self.draws.runs.last_mut()
            && run.masked == masked
            && run.pipeline == pipeline
            && run.path.as_ref().zip(path).is_none_or(|(a, b)| a == b)
            && run.image.as_ref().zip(image).is_none_or(|(a, b)| a == b)
            && match (&run.program, program) {
                (None, None) => true,
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                _ => false,
            }
        {
            // Shape-only painters may share the image and path bindings of adjacent painters.
            if run.image.is_none() {
                run.image = image.cloned();
            }
            if run.path.is_none() {
                run.path = path.cloned();
            }
            run.end = end;
        } else {
            self.draws.runs.push(Run {
                start: start as u32,
                end,
                pipeline,
                masked,
                program: program.cloned(),
                image: image.cloned(),
                path: path.cloned(),
            });
        }
    }
}
