use ipp_core::{
    HostRuntime, OutputRef, WorldAttachmentMode, WorldPublication, WorldPublicationId,
    services::asset_management::{
        AssetKey,
        formats::{drawing::DrawingAsset, font::FontAsset},
    },
    systems::{
        canvas::{
            CanvasAffine, CanvasAttachmentSlot, CanvasBoxShape, CanvasClip, CanvasPaintEntry,
            CanvasPaintInstance, CanvasPrimitive, CanvasPrimitiveStyle, CanvasPublication,
            CanvasShapeFill, CanvasSpace,
        },
        surface::SurfaceGeometry,
    },
};

use crate::{
    analytic_surface::{AnalyticPrimitive, AnalyticSurface},
    assets::MetalTexture,
    device::{
        CanvasBinding, CanvasCoverage, CanvasPaint, CanvasVertex, Device, Frame, PaintParameters,
    },
    paths::{Path, PathCache},
    service::{MetalFrameSummary, asset},
    shaders::AuthoredPaint,
    web_images::WebImages,
};

#[derive(Clone, Copy)]
pub(super) struct CanvasScene<'a> {
    pub(super) canvas: &'a CanvasPublication,
    pub(super) world: &'a WorldPublication,
}

impl<'a> CanvasScene<'a> {
    pub(super) fn new(
        host: &'a HostRuntime,
        output: OutputRef,
        id: WorldPublicationId,
    ) -> Option<Self> {
        Some(Self {
            canvas: host.output(id, output)?.data()?,
            world: host.publication(id)?,
        })
    }

    pub(super) fn child(
        self,
        host: &'a HostRuntime,
        slot: &CanvasAttachmentSlot,
    ) -> Option<(OutputRef, WorldPublicationId)> {
        let edge = self.world.attachments.iter().find(|edge| {
            edge.anchor == slot.anchor
                && edge.token == slot.token
                && edge.placement_output == Some(self.canvas.selection)
                && edge.surface_extent == Some(slot.physical_extent)
                && edge.surface_incarnation == Some(slot.surface_incarnation)
                && matches!(
                    edge.mode,
                    WorldAttachmentMode::SurfaceCanvas | WorldAttachmentMode::SurfaceCamera
                )
        })?;
        Some((edge.output?, host.attached_publication(edge)?.id))
    }
}

pub(super) enum PaintSources<'a> {
    Canvas(CanvasScene<'a>),
    Plot {
        publication: WorldPublicationId,
        paints: [Option<&'a CanvasPaintInstance>; 2],
    },
}

impl PaintSources<'_> {
    fn publication(&self) -> WorldPublicationId {
        match self {
            Self::Canvas(scene) => scene.world.id,
            Self::Plot {
                publication,
                ..
            } => *publication,
        }
    }

    fn paint(
        &self,
        target: ipp_core::systems::canvas::CanvasTarget,
    ) -> Option<&CanvasPaintInstance> {
        match self {
            Self::Canvas(scene) => scene.canvas.paint(target),
            Self::Plot {
                paints,
                ..
            } => paints
                .iter()
                .flatten()
                .copied()
                .find(|paint| paint.target == target),
        }
    }
}

#[derive(Clone, Default)]
struct CanvasPolygon {
    vertices: [[f32; 2]; 8],
    len: usize,
    overflow: Vec<[f32; 2]>,
}

impl CanvasPolygon {
    fn push(&mut self, point: [f32; 2]) {
        if self.len < self.vertices.len() {
            self.vertices[self.len] = point;
        } else {
            if self.len == self.vertices.len() {
                self.overflow.extend_from_slice(&self.vertices);
            }
            self.overflow.push(point);
        }
        self.len += 1;
    }

    fn clear(&mut self) {
        self.len = 0;
        self.overflow.clear();
    }
}

impl std::ops::Deref for CanvasPolygon {
    type Target = [[f32; 2]];

    fn deref(&self) -> &Self::Target {
        if self.len <= self.vertices.len() {
            &self.vertices[..self.len]
        } else {
            &self.overflow
        }
    }
}

impl FromIterator<[f32; 2]> for CanvasPolygon {
    fn from_iter<T: IntoIterator<Item = [f32; 2]>>(points: T) -> Self {
        let mut polygon = Self::default();
        for point in points {
            polygon.push(point);
        }
        polygon
    }
}

pub(super) struct CanvasPlacement {
    affine: CanvasAffine,
    polygon: CanvasPolygon,
    opacity: f32,
}

impl CanvasPlacement {
    pub(super) fn root(extent: [f32; 2]) -> Self {
        Self::rectangle([0.0, 0.0, extent[0], extent[1]])
    }

    pub(super) fn rectangle(clip: CanvasClip) -> Self {
        Self {
            affine: CanvasAffine::IDENTITY,
            polygon: corners(clip).into_iter().collect(),
            opacity: 1.0,
        }
    }

    fn clipped(&self, space: Option<&CanvasSpace>, rect: CanvasClip) -> Option<Self> {
        let affine = space.map_or(CanvasAffine::IDENTITY, |space| space.affine);
        let to_root = self.affine.compose(affine);
        let inverse = to_root.inverse()?;
        let mut polygon = self.polygon.iter().map(|&p| inverse.point(p)).collect();
        if let Some(space) = space {
            polygon = intersect(polygon, &space.polygon);
        }
        polygon = intersect(polygon, &corners(rect));
        (polygon.len() >= 3).then_some(Self {
            affine: to_root,
            polygon: polygon.iter().map(|&p| to_root.point(p)).collect(),
            opacity: self.opacity,
        })
    }

    pub(super) fn slot(&self, slot: &CanvasAttachmentSlot, extent: [f32; 2]) -> Option<Self> {
        let opacity = self.opacity * slot.opacity;
        if opacity <= 0.0 {
            return None;
        }
        let clipped = self.clipped(slot.space.as_deref(), slot.clip)?;
        let [w, h] = slot.physical_extent;
        let origin = slot
            .to_drawing([-w * 0.5, h * 0.5])
            .map(|value| value as f32);
        let scale = [
            (slot.scale[0] * w) as f32 / extent[0],
            (-slot.scale[1] * h) as f32 / extent[1],
        ];
        let placement = CanvasAffine::axis_aligned(origin, scale);
        let affine = clipped.affine.compose(placement);
        let content = corners([0.0, 0.0, extent[0], extent[1]]).map(|point| affine.point(point));
        let polygon = intersect(clipped.polygon, &content);
        (polygon.len() >= 3).then_some(Self {
            affine,
            polygon,
            opacity,
        })
    }
}

pub(super) enum CanvasProjection<'a> {
    Analytic(&'a AnalyticSurface<'a>),
    Plane {
        matrix: [f32; 16],
        world: Option<[f32; 16]>,
    },
    Surface {
        matrix: [f32; 16],
        world: [f32; 16],
        geometry: &'a SurfaceGeometry,
        scale: [f64; 2],
        offset: f64,
        image_projection: Option<[f32; 16]>,
    },
}

impl CanvasProjection<'_> {
    pub(super) fn viewport(extent: [f32; 2]) -> Self {
        let matrix =
            CanvasAffine::axis_aligned([-1.0, 1.0], [2.0 / extent[0], -2.0 / extent[1]]).matrix();
        Self::Plane {
            matrix,
            world: None,
        }
    }

    fn vertex(&self, root: [f32; 2], local: [f32; 2]) -> Option<CanvasVertex> {
        let (clip, point, basis) = match self {
            Self::Analytic(_) => return None,
            Self::Plane {
                matrix,
                world,
            } => {
                let point = [root[0], root[1], 0.0];
                let basis = world.as_ref().map(|world| {
                    let right = [world[0], world[1], world[2]];
                    let down = [world[4], world[5], world[6]];
                    let front = [world[8], world[9], world[10]];
                    SurfaceBasis::new(world, point, right, down, front)
                });
                (matrix, point, basis)
            }
            Self::Surface {
                matrix,
                world,
                geometry,
                scale,
                offset,
                ..
            } => {
                let content = [f64::from(root[0]) * scale[0], f64::from(root[1]) * scale[1]];
                let sample = geometry.sample(content, *offset).ok()?;
                let physical = geometry.physical_extent();
                let dx = physical[0] * 1e-6;
                let dy = physical[1] * 1e-6;
                let point = sample.position.map(|v| v as f32);
                let derivative = |axis, delta| -> Option<[f32; 3]> {
                    let mut nearby = content;
                    nearby[axis] += delta;
                    // Backward differences at a provider's domain edge keep
                    // the forward tangent's orientation.
                    if let Ok(neighbor) = geometry.sample(nearby, *offset) {
                        Some(std::array::from_fn(|i| {
                            ((neighbor.position[i] - sample.position[i]) / delta) as f32
                        }))
                    } else {
                        nearby[axis] = content[axis] - delta;
                        let neighbor = geometry.sample(nearby, *offset).ok()?;
                        Some(std::array::from_fn(|i| {
                            ((sample.position[i] - neighbor.position[i]) / delta) as f32
                        }))
                    }
                };
                let right = transform_vector(world, derivative(0, dx)?);
                let down = transform_vector(world, derivative(1, dy)?);
                let local_normal = sample.front_normal.map(|v| v as f32);
                let front = transform_vector(world, local_normal);
                (
                    matrix,
                    point,
                    Some(SurfaceBasis::new(world, point, right, down, front)),
                )
            }
        };
        let clip = match self {
            Self::Surface {
                image_projection: Some(image),
                ..
            } => transform_point(image, [root[0], root[1], 0.0]),
            _ => transform_point(clip, point),
        };
        let (world, right, down, normal) = basis
            .map_or(([0.0; 4], [0.0; 3], [0.0; 3], [0.0; 3]), |basis| {
                (basis.point, basis.right, basis.down, basis.normal)
            });
        clip.iter().all(|v| v.is_finite()).then_some(CanvasVertex {
            clip,
            local,
            world,
            right,
            down,
            normal,
            slot: 0,
        })
    }

    fn depth(&self) -> bool {
        !matches!(
            self,
            Self::Plane {
                world: None,
                ..
            }
        )
    }
}

struct SurfaceBasis {
    point: [f32; 4],
    right: [f32; 3],
    down: [f32; 3],
    normal: [f32; 3],
}

impl SurfaceBasis {
    fn new(
        world: &[f32; 16],
        position: [f32; 3],
        right: [f32; 3],
        down: [f32; 3],
        front: [f32; 3],
    ) -> Self {
        let normal = [
            down[1] * right[2] - down[2] * right[1],
            down[2] * right[0] - down[0] * right[2],
            down[0] * right[1] - down[1] * right[0],
        ];
        let sign = normal
            .iter()
            .zip(front)
            .map(|(axis, forward)| axis * forward)
            .sum::<f32>()
            .signum();
        Self {
            point: transform_point(world, position),
            right,
            down,
            normal: normal.map(|axis| axis * sign),
        }
    }
}

fn transform_point(matrix: &[f32; 16], p: [f32; 3]) -> [f32; 4] {
    std::array::from_fn(|i| {
        matrix[12 + i] + matrix[i] * p[0] + matrix[4 + i] * p[1] + matrix[8 + i] * p[2]
    })
}

fn transform_vector(matrix: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| matrix[i] * v[0] + matrix[4 + i] * v[1] + matrix[8 + i] * v[2])
}

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
}

/// Local extent of a framebuffer pixel on a parallel-projected world plane.
/// Perspective and curved surfaces have a position-dependent footprint.
fn plane_pixel_extent(
    projection: &CanvasProjection<'_>,
    affine: CanvasAffine,
    target: [u32; 2],
) -> Option<[f32; 2]> {
    let CanvasProjection::Plane {
        matrix,
        world: Some(_),
    } = projection
    else {
        return None;
    };
    if matrix[3] != 0.0 || matrix[7] != 0.0 || matrix[15] <= 0.0 {
        return None;
    }
    let [sx, sy] = target.map(|n| n as f32 * 0.5 / matrix[15]);
    let screen = CanvasAffine {
        axes: [
            [sx * matrix[0], sy * matrix[1]],
            [sx * matrix[4], sy * matrix[5]],
        ],
        origin: [0.0; 2],
    };
    let inverse = screen.compose(affine).inverse()?;
    Some(std::array::from_fn(|i| {
        inverse.axes[0][i].abs() + inverse.axes[1][i].abs()
    }))
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
        if let CanvasProjection::Surface {
            geometry,
            scale,
            offset,
            ..
        } = self.projection
        {
            let bounds = bounds(&placement.polygon);
            let physical = [
                f64::from(bounds[0]) * scale[0],
                f64::from(bounds[1]) * scale[1],
                f64::from(bounds[2]) * scale[0],
                f64::from(bounds[3]) * scale[1],
            ];
            let extent = geometry.physical_extent();
            let [left, top, right, bottom] = physical;
            let error_x = geometry
                .approximation_error([left, top, right, top], *offset)
                .unwrap_or(f64::INFINITY);
            let error_y = geometry
                .approximation_error([left, top, left, bottom], *offset)
                .unwrap_or(f64::INFINITY);
            // Budget the projected approximation independently along both
            // axes. A cylindrical patch does not subdivide its straight axis.
            let tolerance = extent[0].min(extent[1]) / 2048.0;
            let subdivisions = |error: f64, other| {
                (error
                    / if other > 0.0 {
                        tolerance * 0.5
                    } else {
                        tolerance
                    })
                .sqrt()
                .ceil()
                .clamp(1.0, 32.0) as u32
            };
            let columns = subdivisions(error_x, error_y);
            let rows = subdivisions(error_y, error_x);
            let w = (bounds[2] - bounds[0]) / columns as f32;
            let h = (bounds[3] - bounds[1]) / rows as f32;
            for y in 0..rows {
                for x in 0..columns {
                    let patch = corners([
                        bounds[0] + x as f32 * w,
                        bounds[1] + y as f32 * h,
                        bounds[0] + (x + 1) as f32 * w,
                        bounds[1] + (y + 1) as f32 * h,
                    ]);
                    let poly = intersect(placement.polygon.clone(), &patch);
                    self.triangles(&poly, inverse, &mut vertices);
                }
            }
        } else {
            self.triangles(&placement.polygon, inverse, &mut vertices);
        }

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

    fn triangles(
        &self,
        polygon: &[[f32; 2]],
        inverse: CanvasAffine,
        vertices: &mut Vec<CanvasVertex>,
    ) {
        for pair in polygon.get(1..).unwrap_or_default().windows(2) {
            let points = [polygon[0], pair[0], pair[1]];
            if let [Some(a), Some(b), Some(c)] =
                points.map(|point| self.projection.vertex(point, inverse.point(point)))
            {
                vertices.extend([a, b, c]);
            }
        }
    }
}

pub(super) fn decoded<T: 'static>(
    host: &HostRuntime,
    publication: WorldPublicationId,
    key: AssetKey,
) -> Option<&T> {
    host.publication_resource(publication, key)?
        .data()?
        .decoded()
        .downcast_ref()
}

fn linear(srgb: f32) -> f32 {
    if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

fn tint(style: &CanvasPrimitiveStyle, color: [f32; 4]) -> [f32; 4] {
    [
        style.color[0] * color[0],
        style.color[1] * color[1],
        style.color[2] * color[2],
        style.color[3] * color[3] * style.opacity,
    ]
}

fn box_paint(
    style: &CanvasPrimitiveStyle,
    [w, h]: [f32; 2],
    radii: [f32; 2],
    border: f32,
    border_color: [f32; 4],
    fill: CanvasShapeFill,
    shape: CanvasBoxShape,
) -> CanvasPaint {
    let [sx, sy] = style.scale.map(f32::abs);
    let length = sx.max(sy);
    let mut paint = CanvasPaint {
        bounds: [0.0, 0.0, w, h],
        border: tint(style, border_color),
        shape: [radii[0] * sx, radii[1] * sy, border * length, 0.0],
        ..Default::default()
    };
    let axis = |[x, y]: [f32; 2], [u, v]: [f32; 2]| [x * sx, y * sy, u * sx, v * sy];
    let (kind, colors, gradient) = match fill {
        CanvasShapeFill::Solid(color)
        | CanvasShapeFill::Paint {
            color,
            ..
        } => (0.0, [tint(style, color); 2], [0.0; 4]),
        CanvasShapeFill::LinearGradient {
            start,
            end,
            start_color,
            end_color,
        } => (
            1.0,
            [tint(style, start_color), tint(style, end_color)],
            axis(start, end),
        ),
        CanvasShapeFill::RadialGradient {
            center,
            radius,
            start_color,
            end_color,
        } => (
            2.0,
            [tint(style, start_color), tint(style, end_color)],
            [center[0] * sx, center[1] * sy, radius * length, 0.0],
        ),
        CanvasShapeFill::Hue {
            start,
            end,
        } => (3.0, [tint(style, [1.0; 4]), [0.0; 4]], axis(start, end)),
        CanvasShapeFill::SaturationValue {
            hue,
        } => (
            4.0,
            [tint(style, [1.0; 4]), [hue.rem_euclid(1.0), 0.0, 0.0, 0.0]],
            [0.0, 0.0, w, h],
        ),
    };
    paint.options[0] = kind;
    paint.fill = colors;
    paint.gradient = gradient;

    match shape {
        CanvasBoxShape::Rect {
            section,
            corner_cut,
            corner_accent,
            corner_accent_width,
            checker,
        } => {
            paint.cut = corner_cut.map(|v| (v * length).clamp(0.0, w.min(h)));
            paint.accent = corner_accent.map(|v| v * length);
            paint.shape[3] = corner_accent_width * length;
            if let Some(section) = section {
                let edge = section.edge as u8;
                let scale = if edge.is_multiple_of(2) {
                    sx
                } else {
                    sy
                };
                paint.section = [f32::from(edge + 1), section.position * scale, 0.0, 0.0];
            }
            if let Some(checker) = checker.filter(|checker| checker.size > 0.0) {
                paint.options[0] += 8.0;
                paint.options[2] = checker.size * length;
                paint.checker = checker.colors.map(|color| tint(style, color));
            }
        }
        CanvasBoxShape::Stroke {
            segments,
        } => {
            paint.options[0] += 16.0;
            paint.segment = segments.map(|[a, b, c, d]| {
                [
                    (c + a) * w * 0.5,
                    (d + b) * h * 0.5,
                    (c - a) * w * 0.5,
                    (d - b) * h * 0.5,
                ]
            });
        }
        CanvasBoxShape::Arc {
            start,
            sweep,
            dashes,
            dash_duty,
        } => {
            paint.options[0] += 32.0;
            let outer = w.min(h) * 0.5;
            let width = paint.shape[2].clamp(0.0, outer);
            let half_sweep = sweep.abs().min(1.0) * std::f32::consts::PI;
            let mid = std::f32::consts::TAU * (start + sweep.signum() * sweep.abs().min(1.0) * 0.5);
            let (sin, cos) = mid.sin_cos();
            let (cap_sin, cap_cos) = half_sweep.sin_cos();
            paint.shape = [
                outer - width * 0.5,
                half_sweep,
                width,
                dash_duty.clamp(0.0, 1.0),
            ];
            paint.segment = [
                [w * 0.5, h * 0.5, sin, -cos],
                [cap_sin, cap_cos, dashes * sweep.signum(), 0.0],
            ];
        }
    }
    paint
}

fn corners([l, t, r, b]: CanvasClip) -> [[f32; 2]; 4] {
    [[l, t], [r, t], [r, b], [l, b]]
}

fn bounds(poly: &[[f32; 2]]) -> CanvasClip {
    poly.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |[l, t, r, b], &[x, y]| [l.min(x), t.min(y), r.max(x), b.max(y)],
    )
}

fn cross(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn clip_rectangle(polygon: &[[f32; 2]]) -> Option<CanvasClip> {
    let [a, b, c, d] = polygon else {
        return None;
    };
    if a[0] == c[0] || a[1] == c[1] {
        return None;
    }
    let horizontal_first = a[1] == b[1] && b[0] == c[0] && c[1] == d[1] && d[0] == a[0];
    let vertical_first = a[0] == b[0] && b[1] == c[1] && c[0] == d[0] && d[1] == a[1];
    (horizontal_first || vertical_first).then(|| {
        [
            a[0].min(c[0]),
            a[1].min(c[1]),
            a[0].max(c[0]),
            a[1].max(c[1]),
        ]
    })
}

fn intersect(mut polygon: CanvasPolygon, clip: &[[f32; 2]]) -> CanvasPolygon {
    if clip.len() < 3 || polygon.len() < 3 {
        return CanvasPolygon::default();
    }
    if let Some([l, t, r, b]) = clip_rectangle(clip) {
        if polygon
            .iter()
            .all(|p| p[0] >= l && p[0] <= r && p[1] >= t && p[1] <= b)
        {
            return polygon;
        }
        if let Some([x0, y0, x1, y1]) = clip_rectangle(&polygon) {
            let clipped = [l.max(x0), t.max(y0), r.min(x1), b.min(y1)];
            polygon.clear();
            if clipped[0] < clipped[2] && clipped[1] < clipped[3] {
                polygon = corners(clipped).into_iter().collect();
            }
            return polygon;
        }
    }

    let area: f32 = (1..clip.len() - 1)
        .map(|i| cross(clip[0], clip[i], clip[i + 1]))
        .sum();
    if area == 0.0 {
        return CanvasPolygon::default();
    }
    let winding = area.signum();
    let mut output = CanvasPolygon::default();
    for (&a, &b) in clip.iter().zip(clip.iter().cycle().skip(1)) {
        let Some(&mut_last) = polygon.last() else {
            break;
        };
        let mut previous = mut_last;
        let mut distance = cross(a, b, previous) * winding;
        output.clear();
        for &current in polygon.iter() {
            let next = cross(a, b, current) * winding;
            if (distance < 0.0) != (next < 0.0) {
                let t = distance / (distance - next);
                output.push(std::array::from_fn(|i| {
                    previous[i] + (current[i] - previous[i]) * t
                }));
            }
            if next >= 0.0 {
                output.push(current);
            }
            previous = current;
            distance = next;
        }
        std::mem::swap(&mut polygon, &mut output);
    }
    polygon
}

pub(super) fn camera_slots(scene: CanvasScene<'_>) -> impl Iterator<Item = &CanvasAttachmentSlot> {
    scene
        .canvas
        .entries
        .iter()
        .filter_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Attachment(slot) if slot.opacity > 0.0 => Some(slot),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_pixel_accounts_for_both_axes_and_target_dimensions() {
        let mut matrix = CanvasAffine::IDENTITY.matrix();
        let affine = CanvasAffine {
            axes: [[1.0, 1.0], [-1.0, 1.0]],
            origin: [12.0, 80.0],
        };
        let extent = |matrix| {
            plane_pixel_extent(
                &CanvasProjection::Plane {
                    matrix,
                    world: Some(matrix),
                },
                affine,
                [100, 200],
            )
        };
        // Half a local unit along either screen axis before the viewport scale.
        for axis in extent(matrix).unwrap() {
            assert!((axis - 0.015).abs() < 1e-6);
        }
        matrix[3] = 0.2;
        assert!(extent(matrix).is_none());
        matrix[3] = 0.0;
        matrix[0] = 0.0;
        assert!(extent(matrix).is_none());
    }
}
