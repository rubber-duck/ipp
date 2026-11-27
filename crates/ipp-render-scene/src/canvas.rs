//! Ordered Canvas publication access, placement, coverage parameters and projected triangles.
use crate::analytic_surface::AnalyticSurface;
use ipp_core::{
    HostRuntime, OutputRef, WorldAttachmentMode, WorldPublication, WorldPublicationId,
    services::asset_management::AssetKey,
    systems::{
        canvas::{
            CanvasAffine, CanvasAttachmentSlot, CanvasBoxShape, CanvasClip, CanvasPaintEntry,
            CanvasPaintInstance, CanvasPrimitiveStyle, CanvasPublication, CanvasShapeFill,
            CanvasSpace,
        },
        surface::SurfaceGeometry,
    },
};
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
/// Raster input shared by the native Canvas pipelines.
pub struct CanvasVertex {
    /// Homogeneous clip-space position.
    pub clip: [f32; 4],
    /// Local paint coordinates.
    pub local: [f32; 2],
    /// World-space position and basis validity, or homogeneous ray origin for analytic projection.
    pub world: [f32; 4],
    /// World-space horizontal tangent.
    pub right: [f32; 3],
    /// World-space downward tangent.
    pub down: [f32; 3],
    /// Oriented world-space Surface normal.
    pub normal: [f32; 3],
    /// Renderer-assigned paint record offset.
    pub slot: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Default)]
/// Linear paint and shape parameters consumed by the native Canvas fragment stages.
pub struct CanvasPaint {
    /// Linear color multiplied into the primitive.
    pub tint: [f32; 4],
    /// Local paint rectangle.
    pub bounds: [f32; 4],
    /// Linear endpoint colors.
    pub fill: [[f32; 4]; 2],
    /// Gradient axis and interpolation parameters.
    pub gradient: [f32; 4],
    /// Linear border color.
    pub border: [f32; 4],
    /// Shape selector, image mode, border width and corner or path radius.
    pub options: [f32; 4],
    /// Shape-specific geometry parameters.
    pub shape: [f32; 4],
    /// Endpoints or angular basis for segment and arc primitives.
    pub segment: [[f32; 4]; 2],
    /// Corner cut parameters.
    pub cut: [f32; 4],
    /// Linear accent color and coverage.
    pub accent: [f32; 4],
    /// Linear glow color and intensity.
    pub glow_color: [f32; 4],
    /// Glow radii and falloff.
    pub glow: [f32; 4],
    /// Two colors of a checker fill.
    pub checker: [[f32; 4]; 2],
    /// Axis and position for section clipping.
    pub section: [f32; 4],
    /// Local clipping rectangle.
    pub clip: [f32; 4],
    /// Origin and coordinate scale for a custom paint.
    pub paint_origin_scale: [f32; 4],
    /// Custom paint extent, opacity and enable flag.
    pub paint_size_opacity: [f32; 4],
    /// Per-primitive custom paint input.
    pub paint_data: [f32; 4],
}

#[derive(Clone, Copy)]
/// Canvas output paired with the publication providing its assets and attachments.
pub struct CanvasScene<'a> {
    /// Completed Canvas output.
    pub canvas: &'a CanvasPublication,
    /// Owning World publication.
    pub world: &'a WorldPublication,
}

impl<'a> CanvasScene<'a> {
    /// Resolve the exact Canvas selection at the given publication.
    pub fn new(host: &'a HostRuntime, output: OutputRef, id: WorldPublicationId) -> Option<Self> {
        Some(Self {
            canvas: host.output(id, output)?.data()?,
            world: host.publication(id)?,
        })
    }

    /// Resolve the attached output and its selected publication at this slot.
    pub fn child(
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

/// Immutable sources for Canvas and spatial Plot paints.
pub enum PaintSources<'a> {
    /// Paints declared by a Canvas output.
    Canvas(CanvasScene<'a>),
    /// Paints associated with a projected Plot plane.
    Plot {
        /// World publication holding the Plot resources.
        publication: WorldPublicationId,
        /// Resolved paint instances used by the plane.
        paints: [Option<&'a CanvasPaintInstance>; 2],
    },
}

impl PaintSources<'_> {
    /// World publication used to resolve paint assets.
    pub fn publication(&self) -> WorldPublicationId {
        match self {
            Self::Canvas(scene) => scene.world.id,
            Self::Plot {
                publication,
                ..
            } => *publication,
        }
    }

    /// Look up a paint instance in the selected source.
    pub fn paint(
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
/// Convex clipping polygon in Canvas coordinates.
pub struct CanvasPolygon {
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

/// Affine placement, inherited clip and opacity for a Canvas contribution.
pub struct CanvasPlacement {
    /// Transform from local to root content coordinates.
    pub affine: CanvasAffine,
    /// Visible region in root content coordinates.
    pub polygon: CanvasPolygon,
    /// Combined opacity inherited from containing slots.
    pub opacity: f32,
}

impl CanvasPlacement {
    /// Cover an entire Canvas of the supplied logical extent.
    pub fn root(extent: [f32; 2]) -> Self {
        Self::rectangle([0.0, 0.0, extent[0], extent[1]])
    }

    /// Start a placement clipped to the supplied bounds.
    pub fn rectangle(clip: CanvasClip) -> Self {
        Self {
            affine: CanvasAffine::IDENTITY,
            polygon: corners(clip).into_iter().collect(),
            opacity: 1.0,
        }
    }

    /// Intersect the current placement with the primitive's clip and clipping space.
    pub fn clipped(&self, space: Option<&CanvasSpace>, rect: CanvasClip) -> Option<Self> {
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

    /// Map a child Canvas through its attachment into the current placement.
    pub fn slot(&self, slot: &CanvasAttachmentSlot, extent: [f32; 2]) -> Option<Self> {
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

/// Mapping from root Canvas coordinates into the active render target.
pub enum CanvasProjection<'a> {
    /// Ray-intersected cylindrical or spherical Surface.
    Analytic(&'a AnalyticSurface<'a>),
    /// Affine Canvas or planar Surface.
    Plane {
        /// Model-to-clip transform.
        matrix: [f32; 16],
        /// Optional model-to-world transform for scene-aware paints.
        world: Option<[f32; 16]>,
    },
    /// Sampled nonlinear Surface mapping.
    Surface {
        /// Surface-to-clip transform.
        matrix: [f32; 16],
        /// Surface-to-world transform.
        world: [f32; 16],
        /// Completed mapping from chart coordinates to the Surface.
        geometry: &'a SurfaceGeometry,
        /// Physical units per logical unit on each axis.
        scale: [f64; 2],
        /// Layer displacement along the Surface normal.
        offset: f64,
        /// Optional flat target projection used when preparing a Surface image.
        image_projection: Option<[f32; 16]>,
    },
}

impl CanvasProjection<'_> {
    /// Create a top-left Canvas projection at the given extent.
    pub fn viewport(extent: [f32; 2]) -> Self {
        let matrix =
            CanvasAffine::axis_aligned([-1.0, 1.0], [2.0 / extent[0], -2.0 / extent[1]]).matrix();
        Self::Plane {
            matrix,
            world: None,
        }
    }

    /// Project a point and its local paint coordinates to a raster vertex.
    pub fn vertex(&self, root: [f32; 2], local: [f32; 2]) -> Option<CanvasVertex> {
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

    /// Whether this projection participates in scene depth testing.
    pub fn depth(&self) -> bool {
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

/// Resolve CPU-decoded asset data pinned by the World publication.
pub fn decoded<T: 'static>(
    host: &HostRuntime,
    publication: WorldPublicationId,
    key: AssetKey,
) -> Option<&T> {
    host.publication_resource(publication, key)?
        .data()?
        .decoded()
        .downcast_ref()
}

/// Convert an sRGB channel to linear intensity.
pub fn linear(srgb: f32) -> f32 {
    if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

/// Apply the style color and opacity in linear space.
pub fn tint(style: &CanvasPrimitiveStyle, color: [f32; 4]) -> [f32; 4] {
    [
        style.color[0] * color[0],
        style.color[1] * color[1],
        style.color[2] * color[2],
        style.color[3] * color[3] * style.opacity,
    ]
}

/// Pack a box, border, authored fill and shape for the fragment stage.
pub fn box_paint(
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

/// Return the four corners in perimeter order.
pub fn corners([l, t, r, b]: CanvasClip) -> [[f32; 2]; 4] {
    [[l, t], [r, t], [r, b], [l, b]]
}

/// Compute the axis-aligned bounds of a polygon.
pub fn bounds(poly: &[[f32; 2]]) -> CanvasClip {
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

/// Clip one convex polygon by the edges of another.
pub fn intersect(mut polygon: CanvasPolygon, clip: &[[f32; 2]]) -> CanvasPolygon {
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

/// Iterate over Camera attachments in Canvas painter order.
pub fn camera_slots(scene: CanvasScene<'_>) -> impl Iterator<Item = &CanvasAttachmentSlot> {
    scene
        .canvas
        .entries
        .iter()
        .filter_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Attachment(slot) if slot.opacity > 0.0 => Some(slot),
            _ => None,
        })
}
/// Choose an affine or curved projection from the completed Surface mapping.
pub fn surface_projection(
    view: [f32; 16],
    world: [f32; 16],
    geometry: &ipp_core::systems::surface::SurfaceGeometry,
    scale: [f64; 2],
    offset: f64,
) -> CanvasProjection<'_> {
    if let Some(local) = geometry.exact_affine(offset) {
        let local = ipp_core::math::multiply(
            local.map(|v| v as f32),
            ipp_core::systems::canvas::CanvasAffine::axis_aligned(
                [0.0; 2],
                scale.map(|v| v as f32),
            )
            .matrix(),
        );
        let pose = ipp_core::math::multiply(world, local);
        CanvasProjection::Plane {
            matrix: ipp_core::math::multiply(view, pose),
            world: Some(pose),
        }
    } else {
        let matrix = ipp_core::math::multiply(view, world);
        CanvasProjection::Surface {
            matrix,
            world,
            geometry,
            scale,
            offset,
            image_projection: None,
        }
    }
}

/// Order displaced layers from farthest to nearest for this view.
pub fn image_layer_order(
    scene: CanvasScene<'_>,
    geometry: &ipp_core::systems::surface::SurfaceGeometry,
    view: [f32; 16],
    model: [f32; 16],
    spacing: f32,
) -> impl Iterator<Item = (u32, f64)> {
    let clip = ipp_core::math::multiply(view, model);
    let center = geometry.physical_extent().map(|value| value * 0.5);
    let mut layers: Vec<_> = scene
        .canvas
        .layers
        .iter()
        .filter_map(|layer| {
            let offset = layer.offset * f64::from(spacing);
            let sample = geometry.sample(center, offset).ok()?;
            let z = f64::from(clip[14])
                + (0..3)
                    .map(|i| f64::from(clip[i * 4 + 2]) * sample.position[i])
                    .sum::<f64>();
            Some((layer.id, offset, z))
        })
        .collect();
    layers.sort_by(|a, b| b.2.total_cmp(&a.2));
    layers.into_iter().map(|(layer, offset, _)| (layer, offset))
}

impl CanvasProjection<'_> {
    /// Append projected triangles, subdividing curved Surfaces for the requested placement.
    pub fn triangulate(
        &self,
        placement: &CanvasPlacement,
        inverse: CanvasAffine,
        vertices: &mut Vec<CanvasVertex>,
    ) {
        if let CanvasProjection::Surface {
            geometry,
            scale,
            offset,
            ..
        } = self
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
                    self.triangles(&poly, inverse, vertices);
                }
            }
        } else {
            self.triangles(&placement.polygon, inverse, vertices);
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
                points.map(|point| self.vertex(point, inverse.point(point)))
            {
                vertices.extend([a, b, c]);
            }
        }
    }
}

/// Project the Surface bounds and quantize the target size under its authored cache policy.
pub fn projected_size(
    geometry: &ipp_core::systems::surface::SurfaceGeometry,
    matrix: [f32; 16],
    viewport: [u32; 2],
    policy: ipp_core::systems::surface::SurfaceCachePolicy,
) -> Option<[u32; 2]> {
    let ipp_core::systems::geometry::GeometryShape::Box {
        min,
        max,
    } = geometry.bounds([0.0; 2]).ok()?
    else {
        return None;
    };
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    let mut in_front = false;
    let mut behind = false;
    for corner in 0..8 {
        let p: [f32; 3] = std::array::from_fn(|i| {
            if corner & (1 << i) == 0 {
                min[i] as f32
            } else {
                max[i] as f32
            }
        });
        let clip: [f32; 4] = std::array::from_fn(|i| {
            matrix[12 + i] + matrix[i] * p[0] + matrix[4 + i] * p[1] + matrix[8 + i] * p[2]
        });
        if clip[3] <= 1e-6 {
            behind = true;
            continue;
        }
        in_front = true;
        for i in 0..2 {
            let ndc = clip[i] / clip[3];
            low[i] = low[i].min(ndc);
            high[i] = high[i].max(ndc);
        }
    }
    if !in_front || (0..2).any(|i| low[i] > 1.0 || high[i] < -1.0) {
        return None;
    }

    let limit = policy.texture_limit(2048);
    Some(std::array::from_fn(|i| {
        let span = if behind {
            2.0
        } else {
            high[i] - low[i]
        };
        let pixels = (viewport[i] as f32 * span * 0.5 * policy.resolution_scale)
            .ceil()
            .clamp(1.0, limit as f32) as u32;
        // A modest allocation step absorbs small changes in projected support.
        pixels.next_multiple_of(32).min(limit)
    }))
}

/// Local extent of a framebuffer pixel on a parallel-projected world plane.
/// Perspective and curved surfaces have a position-dependent footprint.
pub fn plane_pixel_extent(
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
