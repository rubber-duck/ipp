//! Curved Canvas scheduling bounds and precise GPU ray-intersection uniforms.

use ipp_core::systems::{
    canvas::{CanvasAffine, CanvasClip},
    surface::{CylinderSurface, SphereSurface, SurfaceGeometry},
};

const MAX_CLIP_EDGES: usize = 32;

/// The view-dependent ray mapping and conservative primitive covers for one curved Surface.
pub struct AnalyticSurface<'a> {
    /// Published geometry used by drawing and hit testing.
    pub geometry: &'a SurfaceGeometry,
    scale: [f64; 2],
    extent: [f64; 2],
    mapping: AnalyticMapping,
    viewport: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::NoUninit)]
/// Transforms and coefficients shared by every primitive on this Surface.
pub struct AnalyticMapping {
    // Shape, signed curvature, root index, domain radius.
    parameters: [f32; 4],
    projection: [f32; 16],
    inverse: [f32; 16],
    world: [f32; 16],
    chart_to_root: [f32; 4],
    // Column offset, maximum column index, orthonormal positive world frame.
    options: [u32; 4],
}

impl AnalyticMapping {
    /// Pack the ray equation, homogeneous transforms and content-space conversion into texture rows.
    pub fn records(&self) -> impl Iterator<Item = [f32; 4]> {
        std::iter::once(self.parameters)
            .chain(self.projection.as_chunks::<4>().0.iter().copied())
            .chain(self.inverse.as_chunks::<4>().0.iter().copied())
            .chain(self.world.as_chunks::<4>().0.iter().copied())
            .chain(std::iter::once(self.chart_to_root))
    }

    /// Homogeneous near point for a vertex of the conservative screen-space cover.
    pub fn ray_origin(&self, point: [f32; 2]) -> [f32; 4] {
        let m = &self.inverse;
        std::array::from_fn(|i| m[i] * point[0] + m[i + 4] * point[1] - m[i + 8] + m[i + 12])
    }

    /// Whether this mapping selects the nearer ray intersection.
    pub fn is_near_root(&self) -> bool {
        self.parameters[2] != 0.0
    }

    /// Whether the Surface has cylindrical curvature.
    pub fn is_cylinder(&self) -> bool {
        self.parameters[0] == 1.0
    }

    /// Whether this mapping references a prepared column table.
    pub fn uses_columns(&self) -> bool {
        self.options[0] != 0
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone, bytemuck::NoUninit)]
/// Paint placement and clip planes for a single conservative draw.
pub struct AnalyticDraw {
    local_x: [f32; 4],
    local_y: [f32; 4],
    clip_rectangle: [f32; 4],
    cover: [[f32; 2]; 4],
    clip_edges: [f32; 4],
}

impl AnalyticDraw {
    /// Counterclockwise strip corners in normalized screen coordinates.
    pub fn cover(&self) -> [[f32; 2]; 4] {
        self.cover
    }

    /// Per-primitive inverse placement, root clip and count of additional clipping planes.
    pub fn records(&self) -> [[f32; 4]; 4] {
        [
            self.local_x,
            self.local_y,
            self.clip_rectangle,
            self.clip_edges,
        ]
    }
}

/// Projected paint primitive with its Surface mapping and bounded clip polygon.
pub struct AnalyticPrimitive<'a> {
    /// Ray and chart conversion shared with neighboring paints.
    pub mapping: &'a AnalyticMapping,
    /// Per-primitive inverse and coverage bounds.
    pub draw: AnalyticDraw,
    /// Additional half-planes for a nonrectangular clip.
    pub edges: Vec<[f32; 4]>,
}

impl<'a> AnalyticSurface<'a> {
    /// Build a ray mapping when the geometry and transforms admit analytic projection.
    pub fn prepare(
        geometry: &'a SurfaceGeometry,
        view: [f32; 16],
        world: [f32; 16],
        scale: [f64; 2],
        viewport: [u32; 2],
    ) -> Option<Self> {
        let provider = geometry.as_any();
        let (shape, curve) = if let Some(cylinder) = provider.downcast_ref::<CylinderSurface>() {
            (1.0, cylinder.curvature)
        } else {
            (2.0, provider.downcast_ref::<SphereSurface>()?.curvature)
        };
        if curve == 0.0 {
            return None;
        }
        let projection = ipp_core::math::multiply(view, world);
        let inverse = inverse(projection)?;
        let extent = geometry.physical_extent();
        let mapping = AnalyticMapping {
            parameters: [shape, curve, 0.0, (extent[0].hypot(extent[1]) * 0.5) as f32],
            projection,
            inverse,
            world,
            chart_to_root: [
                extent[0] as f32 * 0.5,
                extent[1] as f32 * 0.5,
                scale[0].recip() as f32,
                -scale[1].recip() as f32,
            ],
            options: [0, 0, u32::from(orthonormal_frame(world)), 0],
        };
        Some(Self {
            geometry,
            scale,
            extent,
            mapping,
            viewport,
        })
    }

    /// Return the common transform, chart extent and content scale for batched draws.
    pub fn batch_mapping(&self) -> (AnalyticMapping, [f64; 2], [f64; 2]) {
        (self.mapping, self.scale, self.extent)
    }

    /// Set physical units per Canvas unit.
    pub fn set_scale(&mut self, scale: [f64; 2]) {
        self.scale = scale;
        self.mapping.chart_to_root[2] = scale[0].recip() as f32;
        self.mapping.chart_to_root[3] = -scale[1].recip() as f32;
    }

    /// Select which valid ray intersection contributes to this pass.
    pub fn select_root(&mut self, near: bool) {
        self.mapping.parameters[2] = f32::from(near);
    }

    /// Whether both roots can contribute within the visible chart.
    pub fn needs_near_root(&self) -> bool {
        let inverse = self.mapping.inverse.map(f64::from);
        let eye_w = inverse[11];
        let lateral_w = inverse[3].abs() + inverse[7].abs();
        if eye_w >= 0.0 || inverse[15] <= lateral_w || inverse[15] - eye_w <= lateral_w {
            return true;
        }
        let curvature = f64::from(self.mapping.parameters[1]);
        let x = curvature * inverse[8] / eye_w;
        let y = if self.mapping.parameters[0] < 1.5 {
            0.0
        } else {
            curvature * inverse[9] / eye_w
        };
        let z = 1.0 + curvature * inverse[10] / eye_w;
        x * x + y * y + z * z >= 1.0 - 64.0 * f64::from(f32::EPSILON)
    }

    /// Return a column-separable mapping when the same root applies throughout each screen column.
    pub fn column_mapping(&self) -> Option<AnalyticMapping> {
        let [shape, curvature, _, _] = self.mapping.parameters;
        let m = self.mapping.inverse;
        let projection = self.mapping.projection;
        // Align the cylinder axis with the vertical screen axis. Require a
        // perspective view with a near plane entirely inside the cylinder, so
        // every column has exactly one forward root throughout the viewport.
        let aligned = m[3] == 0.0
            && m[7] == 0.0
            && m[4].abs().max(m[6].abs()) <= m[5].abs() * 1e-6
            && projection[6].abs().max(projection[7].abs()) <= projection[5].abs() * 1e-6;
        if shape != 1.0 || curvature.abs() < 1e-4 || !aligned || self.needs_near_root() {
            return None;
        }

        let w = m[15] - m[11];
        for x in [-1.0, 1.0] {
            let x_near = (m[0] * x - m[8] + m[12]) / w;
            let z_near = (m[2] * x - m[10] + m[14]) / w;
            let radius = (curvature * x_near).hypot(1.0 + curvature * z_near);
            if radius >= 0.95 || !radius.is_finite() {
                return None;
            }
        }

        Some(self.mapping)
    }

    /// Bind the first record of a device-prepared column table.
    pub fn use_columns(&mut self, start: u32) {
        self.mapping.options[0] = start + 1;
        self.mapping.options[1] = self.viewport[0] + 1;
    }

    /// Build the conservative cover and inverse placement for a clipped primitive.
    pub fn draw(
        &self,
        polygon: &[[f32; 2]],
        bounds: CanvasClip,
        inverse: CanvasAffine,
    ) -> Option<AnalyticPrimitive<'_>> {
        if polygon.len() < 3 || polygon.len() > MAX_CLIP_EDGES {
            return None;
        }
        let [l, t, r, b] = bounds;
        let k = f64::from(self.mapping.parameters[1]);
        let cover = if self.mapping.is_cylinder() {
            let center = [
                f64::from(l.midpoint(r)) * self.scale[0],
                f64::from(t.midpoint(b)) * self.scale[1],
            ];
            let half = [
                f64::from(r - l) * self.scale[0] * 0.5,
                f64::from(b - t) * self.scale[1] * 0.5,
            ];
            let midpoint = self.geometry.sample(center, 0.0).ok()?.position;
            let chart_x = center[0] - self.extent[0] * 0.5;
            if (k * half[0]).abs() < 1.0 {
                self.cylinder_cover(midpoint, half, k * chart_x, k * half[0])?
            } else {
                let slope = (k.abs() * (chart_x.abs() + half[0])).min(1.0);
                let radius = [half[0], half[1], half[0] * slope];
                self.box_cover(std::array::from_fn(|i| {
                    [midpoint[i] - radius[i], midpoint[i] + radius[i]]
                }))?
            }
        } else {
            let x = [l, r].map(|v| f64::from(v) * self.scale[0] - self.extent[0] * 0.5);
            let y = [b, t].map(|v| self.extent[1] * 0.5 - f64::from(v) * self.scale[1]);
            self.box_cover(sphere_patch_bounds(x, y, k))?
        };

        let mut data = AnalyticDraw {
            local_x: [
                inverse.axes[0][0],
                inverse.axes[1][0],
                inverse.origin[0],
                0.0,
            ],
            local_y: [
                inverse.axes[0][1],
                inverse.axes[1][1],
                inverse.origin[1],
                0.0,
            ],
            cover,
            ..Default::default()
        };

        let mut edges = Vec::new();
        if polygon.len() == 4
            && polygon
                .iter()
                .all(|&[x, y]| (x == l || x == r) && (y == t || y == b))
        {
            data.clip_edges[0] = -1.0;
            data.clip_rectangle = bounds;
        } else {
            let signed_area = polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
                .sum::<f32>();
            let direction = signed_area.signum();
            data.clip_edges[0] = polygon.len() as f32;
            edges.reserve_exact(polygon.len());
            for (&[ax, ay], &[bx, by]) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
                let normal = [direction * (ay - by), direction * (bx - ax)];
                edges.push([normal[0], normal[1], -normal[0] * ax - normal[1] * ay, 0.0]);
            }
        }
        Some(AnalyticPrimitive {
            mapping: &self.mapping,
            draw: data,
            edges,
        })
    }

    fn project(&self, point: [f64; 3]) -> [f32; 4] {
        let matrix = &self.mapping.projection;
        let p = point.map(|v| v as f32);
        std::array::from_fn(|i| {
            matrix[12 + i] + matrix[i] * p[0] + matrix[i + 4] * p[1] + matrix[i + 8] * p[2]
        })
    }

    fn box_cover(&self, bounds: [[f64; 2]; 3]) -> Option<[[f32; 2]; 4]> {
        let corners = std::array::from_fn::<_, 8, _>(|corner| {
            self.project(std::array::from_fn(|i| bounds[i][(corner >> i) & 1]))
        });
        self.cover(&corners, false)
    }

    fn cylinder_cover(
        &self,
        center: [f64; 3],
        half: [f64; 2],
        angle: f64,
        half_angle: f64,
    ) -> Option<[[f32; 2]; 4]> {
        // The chord endpoints and intersection of their tangents enclose the
        // arc. Extruding this triangle encloses every point of the short patch.
        let sinc = |a: f64| {
            if a.abs() < 1e-4 {
                1.0 - a * a / 6.0
            } else {
                a.sin() / a
            }
        };
        let (s, c) = angle.sin_cos();
        let normal = [s, 0.0, c];
        let tangent = [c, 0.0, -s];
        let sagitta = half[0] * 0.5 * half_angle * sinc(half_angle * 0.5).powi(2);
        let chord: [f64; 3] = std::array::from_fn(|i| center[i] - normal[i] * sagitta);
        let half_chord = tangent.map(|axis| axis * half[0] * sinc(half_angle));
        let apex: [f64; 3] =
            std::array::from_fn(|i| center[i] + normal[i] * sagitta / half_angle.cos());
        let a = std::array::from_fn(|i| chord[i] - half_chord[i]);
        let b = std::array::from_fn(|i| chord[i] + half_chord[i]);
        let arc = [a, b, apex];
        let corners = std::array::from_fn::<_, 6, _>(|i| {
            let p = arc[i / 2];
            let dy = if i % 2 == 0 {
                -half[1]
            } else {
                half[1]
            };
            self.project([p[0], p[1] + dy, p[2]])
        });
        self.cover(&corners, true)
    }

    fn cover(&self, clip: &[[f32; 4]], sloped: bool) -> Option<[[f32; 2]; 4]> {
        let in_front = |point: &[f32; 4]| point[2] + point[3] >= 0.0 && point[3] > 1e-6;
        if clip.iter().all(|point| point[2] + point[3] < 0.0)
            || clip.iter().all(|point| point[2] > point[3])
        {
            return None;
        }
        let needs_clipping = clip.iter().any(|point| !in_front(point));
        let [width, height] = self.viewport.map(|dimension| dimension as f32);
        let xy = |p: &[f32; 4]| {
            let w = p[3].max(1e-6);
            [
                (p[0] / w + 1.0) * width * 0.5,
                (p[1] / w + 1.0) * height * 0.5,
            ]
        };
        let mut points = [[0.0; 2]; 40];
        let mut count = 0;
        for point in clip.iter().filter(|point| in_front(point)) {
            points[count] = xy(point);
            count += 1;
        }
        if needs_clipping {
            // Clip the convex covering volume against the camera's near plane.
            // The extrema lie on retained vertices or intersections along its edges.
            let connected = |i: usize, j: usize| {
                if sloped {
                    i / 2 == j / 2 || i % 2 == j % 2
                } else {
                    (i ^ j).is_power_of_two()
                }
            };
            for (i, a) in clip.iter().enumerate() {
                for (j, b) in clip.iter().enumerate().skip(i + 1) {
                    if !connected(i, j) {
                        continue;
                    }
                    let a_near = a[2] + a[3];
                    let b_near = b[2] + b[3];
                    if (a_near < 0.0) == (b_near < 0.0) {
                        continue;
                    }
                    let t = a_near / (a_near - b_near);
                    let point = std::array::from_fn(|k| a[k] + t * (b[k] - a[k]));
                    if point[3] > 1e-6 {
                        points[count] = xy(&point);
                        count += 1;
                    }
                }
            }
        }
        if count == 0 {
            return None;
        }
        let points = &points[..count];
        let sloped = sloped && !needs_clipping;
        let low_x = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) - 2.0;
        let high_x = points
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            + 2.0;
        let low_y = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) - 2.0;
        let high_y = points
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max)
            + 2.0;
        if high_x <= 0.0 || low_x >= width || high_y <= 0.0 || low_y >= height {
            return None;
        }

        let x = [low_x.max(0.0), high_x.min(width)];
        let bound = |upper| {
            let mut slope = 0.0;
            if sloped {
                let left = if (points[0][1] > points[1][1]) == upper {
                    points[0]
                } else {
                    points[1]
                };
                let right = if (points[2][1] > points[3][1]) == upper {
                    points[2]
                } else {
                    points[3]
                };
                let dx = right[0] - left[0];
                if dx.abs() > 1.0 {
                    slope = ((right[1] - left[1]) / dx).clamp(-2.0, 2.0);
                }
            }
            let sign = if upper {
                1.0
            } else {
                -1.0
            };
            let intercept = points
                .iter()
                .map(|p| sign * (p[1] - slope * p[0]))
                .fold(f32::NEG_INFINITY, f32::max);
            x.map(|x| {
                let y = slope * x + sign * (intercept + 2.0 * (1.0 + slope.abs()));
                [x / width * 2.0 - 1.0, y / height * 2.0 - 1.0]
            })
        };
        let [lower_left, lower_right] = bound(false);
        let [upper_left, upper_right] = bound(true);
        Some([lower_left, lower_right, upper_left, upper_right])
    }
}

// The equidistant sphere chart maps (x, y) to (x*sinc(a), y*sinc(a),
// (cos(a)-1)/k). On its valid domain [0, pi], sinc decreases from one
// to zero and the height is monotone for either curvature sign. Bound
// these factors over the patch before projecting its convex enclosure.
fn sphere_patch_bounds(x: [f64; 2], y: [f64; 2], k: f64) -> [[f64; 2]; 3] {
    let nearest = |[lo, hi]: [f64; 2]| lo.max(-hi).max(0.0);
    let farthest = |[lo, hi]: [f64; 2]| lo.abs().max(hi.abs());
    let radii = [nearest(x).hypot(nearest(y)), farthest(x).hypot(farthest(y))];
    let [[high_scale, near_z], [low_scale, far_z]] = radii.map(|radius| {
        let half = (k.abs() * radius).min(std::f64::consts::PI) * 0.5;
        let (sin, cos) = half.sin_cos();
        let scale = if half == 0.0 {
            1.0
        } else {
            (sin / half) * cos
        };
        [scale, -2.0 * sin * sin / k]
    });

    let scaled = |[lo, hi]: [f64; 2]| {
        [
            lo * if lo < 0.0 {
                high_scale
            } else {
                low_scale
            },
            hi * if hi < 0.0 {
                low_scale
            } else {
                high_scale
            },
        ]
    };
    [scaled(x), scaled(y), [near_z.min(far_z), near_z.max(far_z)]]
}

fn orthonormal_frame(world: [f32; 16]) -> bool {
    let x = [world[0], world[1], world[2]];
    let y = [world[4], world[5], world[6]];
    let z = [world[8], world[9], world[10]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];

    [
        dot(x, x) - 1.0,
        dot(y, y) - 1.0,
        dot(z, z) - 1.0,
        dot(x, y),
        dot(x, z),
        dot(y, z),
        dot(cross, z) - 1.0,
    ]
    .iter()
    .all(|error| error.abs() <= 2e-6)
}

fn inverse(matrix: [f32; 16]) -> Option<[f32; 16]> {
    let mut rows = [[0.0_f64; 8]; 4];
    for row in 0..4 {
        for column in 0..4 {
            rows[row][column] = f64::from(matrix[column * 4 + row]);
        }
        rows[row][row + 4] = 1.0;
    }
    for column in 0..4 {
        let pivot =
            (column..4).max_by(|&a, &b| rows[a][column].abs().total_cmp(&rows[b][column].abs()))?;
        rows.swap(column, pivot);
        let divisor = rows[column][column];
        if divisor == 0.0 || !divisor.is_finite() {
            return None;
        }
        for value in &mut rows[column] {
            *value /= divisor;
        }
        let pivot = rows[column];
        for (row, values) in rows.iter_mut().enumerate() {
            if row != column {
                let factor = values[column];
                for (value, pivot_value) in values.iter_mut().zip(pivot) {
                    *value -= factor * pivot_value;
                }
            }
        }
    }
    let result = std::array::from_fn(|i| rows[i % 4][4 + i / 4] as f32);
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

#[cfg(test)]
#[path = "analytic_surface_tests.rs"]
mod tests;
