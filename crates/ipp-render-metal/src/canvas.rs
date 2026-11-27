use ipp_core::{
    HostRuntime,
    services::asset_management::formats::{drawing::DrawingAsset, font::FontAsset},
    systems::canvas::{CanvasAffine, CanvasClip, CanvasPrimitive, CanvasShapeFill, CanvasSpace},
};

use crate::{
    analytic_surface::AnalyticPrimitive,
    assets::MetalTexture,
    device::{CanvasBinding, CanvasCoverage, Device, Frame, PaintParameters},
    paths::{Path, PathCache},
    service::{MetalFrameSummary, asset},
    shaders::AuthoredPaint,
    web_images::WebImages,
};

pub(crate) use ipp_render_scene::canvas::*;

pub(super) struct CanvasAssets<'a> {
    pub(super) host: &'a HostRuntime,
    pub(super) device: &'a Device,
    pub(super) paths: &'a mut PathCache,
    pub(super) web_images: &'a WebImages,
}

pub(super) struct CanvasPass<'a, 'b> {
    pub(super) frame: &'a mut Frame,
    pub(super) summary: &'a mut MetalFrameSummary,
    pub(super) projection: &'a CanvasProjection<'b>,
    pub(super) owner: u32,
    pub(super) plot: bool,
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
            web_images,
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

        let mut authored = None;
        let resolve =
            |target| AuthoredPaint::resolve(host, sources.publication(), sources.paint(target)?);
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
                let Some(texture) =
                    asset::<MetalTexture>(host, sources.publication(), bitmap.to_u64())
                        .and_then(|image| image.texture.as_ref())
                else {
                    self.summary.fail();
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
                (size, paint, Some(CanvasBinding::Bitmap(texture)))
            }
            CanvasPrimitive::Glyphs {
                font,
                font_size,
                glyphs,
                ..
            } => {
                let Some(data) = decoded::<FontAsset>(host, sources.publication(), *font) else {
                    self.summary.fail();
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
                    self.summary.fail();
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
                    self.summary.fail();
                }
                return;
            }
            CanvasPrimitive::Html {
                image,
                size,
                ..
            } => {
                let Some(id) = image else {
                    return;
                };
                let Some(texture) = web_images.resource(host, sources.publication(), *id) else {
                    return;
                };
                let size = [
                    size[0] * style.scale[0].abs(),
                    size[1] * style.scale[1].abs(),
                ];
                let paint = CanvasPaint {
                    tint: tint(&style, [1.0; 4]),
                    bounds: [0.0, 0.0, size[0], size[1]],
                    options: [0.0, 2.0, 0.0, 0.0],
                    ..Default::default()
                };
                (size, paint, Some(CanvasBinding::Bitmap(texture)))
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
            authored.as_ref().map(AuthoredPaint::parameters),
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
        // Each coverage tap is at most a quarter pixel along either screen axis.
        // Allow a full pixel for raster rounding, bounded by the existing path envelope.
        let [mx, my] = plane_pixel_extent(self.projection, affine, self.frame.target_size)
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

        if let CanvasProjection::Analytic(surface) = self.projection {
            let bounds = bounds(&placement.polygon);
            if let Some(analytic) = surface.draw(&placement.polygon, bounds, inverse) {
                self.submit(&[], paint, image, custom, Some(&analytic));
            }
            return;
        }
        // The strip uses the same diagonal as the triangle fan for all convex clipped quads.
        if matches!(self.projection, CanvasProjection::Plane { .. })
            && let [a, b, c, d] = placement.polygon.as_ref()
            && let [Some(b), Some(c), Some(a), Some(d)] =
                [*b, *c, *a, *d].map(|p| self.projection.vertex(p, inverse.point(p)))
        {
            self.submit(&[b, c, a, d], paint, image, custom, None);
            return;
        }
        let mut vertices = std::mem::take(&mut self.frame.canvas_vertices);
        vertices.clear();
        self.projection
            .triangulate(placement, inverse, &mut vertices);
        if !vertices.is_empty() {
            self.submit(&vertices, paint, image, custom, None);
        }
        self.frame.canvas_vertices = vertices;
    }

    fn submit(
        &mut self,
        vertices: &[CanvasVertex],
        paint: &CanvasPaint,
        image: Option<CanvasBinding<'_>>,
        custom: Option<PaintParameters<'_>>,
        analytic: Option<&AnalyticPrimitive<'_>>,
    ) {
        if self.frame.canvas(
            vertices,
            paint,
            image,
            custom,
            CanvasCoverage {
                analytic,
                depth_test: self.projection.depth(),
                plot: self.plot,
                layer_owner: self.owner,
            },
        ) {
            self.summary.draw_calls = self.summary.draw_calls.saturating_add(1);
            let triangles = if analytic.is_some() || vertices.len() == 4 {
                2
            } else {
                vertices.len() / 3
            };
            self.summary.triangles = self.summary.triangles.saturating_add(triangles as u32);
        } else {
            self.summary.fail();
        }
    }
}
