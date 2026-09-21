use super::{
    GeometryBounds, GeometryPlane, GeometryRay, GeometryRayInterval, GeometryShapeTransform, dot,
    finite3, scale, subtract,
};
use crate::ErrorReason;

/// Closed local-space solids. A pill is a segment swept by a sphere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GeometryShape {
    /// Axis-aligned local box; zero extent axes are allowed.
    Box {
        /// Minimum corner.
        min: [f64; 3],
        /// Maximum corner.
        max: [f64; 3],
    },
    /// Solid sphere.
    Sphere {
        /// Sphere center.
        center: [f64; 3],
        /// Positive radius.
        radius: f64,
    },
    /// Closed segment swept by a sphere; coincident endpoints form a sphere.
    Pill {
        /// First segment endpoint.
        start: [f64; 3],
        /// Second segment endpoint.
        end: [f64; 3],
        /// Positive radius.
        radius: f64,
    },
}

impl Default for GeometryShape {
    fn default() -> Self {
        Self::Box {
            min: [-0.5; 3],
            max: [0.5; 3],
        }
    }
}

impl GeometryShape {
    /// Whether a point lies within this closed primitive.
    pub fn contains_point(&self, point: [f64; 3]) -> bool {
        match *self {
            Self::Box {
                min,
                max,
            } => {
                for axis in 0..3 {
                    if point[axis] < min[axis] || point[axis] > max[axis] {
                        return false;
                    }
                }
                true
            }
            Self::Sphere {
                center,
                radius,
            } => {
                let v = subtract(point, center);
                dot(v, v) <= radius * radius
            }
            Self::Pill {
                start,
                end,
                radius,
            } => {
                let axis = subtract(end, start);
                let relative = subtract(point, start);
                let length = dot(axis, axis);
                let t = if length == 0.0 {
                    0.0
                } else {
                    (dot(relative, axis) / length).clamp(0.0, 1.0)
                };
                let v = subtract(relative, scale(axis, t));
                dot(v, v) <= radius * radius
            }
        }
    }

    /// Reject nonfinite coordinates, inverted boxes and nonpositive radii.
    pub fn validate(&self) -> Result<(), ErrorReason> {
        let valid = match self {
            Self::Box {
                min,
                max,
            } => {
                finite3(*min)
                    && finite3(*max)
                    && min[0] <= max[0]
                    && min[1] <= max[1]
                    && min[2] <= max[2]
            }
            Self::Sphere {
                center,
                radius,
            } => finite3(*center) && radius.is_finite() && *radius > 0.0,
            Self::Pill {
                start,
                end,
                radius,
            } => finite3(*start) && finite3(*end) && radius.is_finite() && *radius > 0.0,
        };
        if valid {
            Ok(())
        } else {
            Err(ErrorReason::InvalidGeometry)
        }
    }
}

impl GeometryBounds for GeometryShape {
    fn ray_intervals(&self, ray: &GeometryRay, intervals: &mut Vec<GeometryRayInterval>) {
        let result = match *self {
            Self::Box {
                min,
                max,
            } => box_interval(ray, min, max),
            Self::Sphere {
                center,
                radius,
            } => sphere_interval(ray, center, radius),
            Self::Pill {
                start,
                end,
                radius,
            } => pill_interval(ray, start, end, radius),
        };
        if let Some([enter, exit]) = result {
            intervals.push(GeometryRayInterval {
                enter,
                exit,
                part: 0,
            });
        }
    }

    #[inline]
    fn plane_interval(&self, plane: &GeometryPlane) -> Option<[f64; 2]> {
        Some(match *self {
            Self::Box {
                min,
                max,
            } => {
                let mut range = [plane.offset; 2];
                for i in 0..3 {
                    let a = min[i] * plane.normal[i];
                    let b = max[i] * plane.normal[i];
                    range[0] += a.min(b);
                    range[1] += a.max(b);
                }
                range
            }
            Self::Sphere {
                center,
                radius,
            } => {
                let center = dot(plane.normal, center) + plane.offset;
                let extent = radius * dot(plane.normal, plane.normal).sqrt();
                [center - extent, center + extent]
            }
            Self::Pill {
                start,
                end,
                radius,
            } => {
                let a = dot(plane.normal, start) + plane.offset;
                let b = dot(plane.normal, end) + plane.offset;
                let extent = radius * dot(plane.normal, plane.normal).sqrt();
                [a.min(b) - extent, a.max(b) + extent]
            }
        })
    }
}

/// Exact affine image of a primitive; spheres may become ellipsoids.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformedGeometryShape {
    /// Local primitive.
    pub shape: GeometryShape,
    /// Complete local-to-query-space affine transform.
    pub transform: GeometryShapeTransform,
}

impl GeometryBounds for TransformedGeometryShape {
    fn bounds(&self) -> Option<[[f64; 3]; 2]> {
        // Axis-aligned support needs only one matrix row per axis. Avoid pulling
        // three general planes through the affine matrix for every enclosure.
        let m = self.transform.matrix();
        let mut bounds = [[0.0; 3]; 2];
        for axis in 0..3 {
            let interval = self.shape.plane_interval(&GeometryPlane {
                normal: [m[axis], m[4 + axis], m[8 + axis]],
                offset: m[12 + axis],
            })?;
            bounds[0][axis] = interval[0];
            bounds[1][axis] = interval[1];
        }
        Some(bounds)
    }

    fn ray_intervals(&self, ray: &GeometryRay, intervals: &mut Vec<GeometryRayInterval>) {
        self.shape
            .ray_intervals(&self.transform.inverse_ray(ray), intervals);
    }

    #[inline]
    fn plane_interval(&self, plane: &GeometryPlane) -> Option<[f64; 2]> {
        self.shape
            .plane_interval(&self.transform.local_plane(plane))
    }
}

/// Union of transformed primitives in stable authored order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompoundGeometryShape {
    /// Primitives in stable authored order.
    pub parts: Vec<TransformedGeometryShape>,
}

impl CompoundGeometryShape {
    /// Sufficient enclosure proof: one convex part contains every box corner.
    /// Returning false means unproven; callers must preserve visibility.
    pub fn encloses_box(&self, bounds: [[f64; 3]; 2]) -> bool {
        for part in &self.parts {
            let mut encloses = true;
            for corner in 0..8 {
                let point = [
                    bounds[corner & 1][0],
                    bounds[(corner >> 1) & 1][1],
                    bounds[(corner >> 2) & 1][2],
                ];
                if !part
                    .shape
                    .contains_point(part.transform.inverse_point(point))
                {
                    encloses = false;
                    break;
                }
            }
            if encloses {
                return true;
            }
        }
        false
    }
}

impl GeometryBounds for CompoundGeometryShape {
    fn bounds(&self) -> Option<[[f64; 3]; 2]> {
        let mut bounds = self.parts.first()?.bounds()?;
        for part in &self.parts[1..] {
            let next = part.bounds()?;
            for axis in 0..3 {
                bounds[0][axis] = bounds[0][axis].min(next[0][axis]);
                bounds[1][axis] = bounds[1][axis].max(next[1][axis]);
            }
        }
        Some(bounds)
    }

    fn ray_intervals(&self, ray: &GeometryRay, intervals: &mut Vec<GeometryRayInterval>) {
        for (part, shape) in self.parts.iter().enumerate() {
            let first = intervals.len();
            shape.ray_intervals(ray, intervals);
            for interval in &mut intervals[first..] {
                interval.part = part as u32;
            }
        }
    }

    #[inline]
    fn plane_interval(&self, plane: &GeometryPlane) -> Option<[f64; 2]> {
        let first = self.parts.first()?;
        let mut range = first.plane_interval(plane)?;
        for part in &self.parts[1..] {
            let next = part.plane_interval(plane)?;
            range[0] = range[0].min(next[0]);
            range[1] = range[1].max(next[1]);
        }
        Some(range)
    }
}

fn box_interval(ray: &GeometryRay, min: [f64; 3], max: [f64; 3]) -> Option<[f64; 2]> {
    let mut range = [f64::NEG_INFINITY, f64::INFINITY];
    for i in 0..3 {
        if ray.direction[i] == 0.0 {
            if ray.origin[i] < min[i] || ray.origin[i] > max[i] {
                return None;
            }
        } else {
            let a = (min[i] - ray.origin[i]) / ray.direction[i];
            let b = (max[i] - ray.origin[i]) / ray.direction[i];
            range[0] = range[0].max(a.min(b));
            range[1] = range[1].min(a.max(b));
        }
    }
    (range[0] <= range[1]).then_some(range)
}

fn quadratic_interval(a: f64, half_b: f64, c: f64) -> Option<[f64; 2]> {
    if a == 0.0 {
        return (c <= 0.0).then_some([f64::NEG_INFINITY, f64::INFINITY]);
    }
    let discriminant = half_b.mul_add(half_b, -a * c);
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let q = -half_b - root.copysign(half_b);
    if q == 0.0 {
        return Some([-half_b / a; 2]);
    }
    let (a, b) = (q / a, c / q);
    Some([a.min(b), a.max(b)])
}

fn sphere_interval(ray: &GeometryRay, center: [f64; 3], radius: f64) -> Option<[f64; 2]> {
    let relative = subtract(ray.origin, center);
    quadratic_interval(
        dot(ray.direction, ray.direction),
        dot(relative, ray.direction),
        dot(relative, relative) - radius * radius,
    )
}

fn pill_interval(
    ray: &GeometryRay,
    start: [f64; 3],
    end: [f64; 3],
    radius: f64,
) -> Option<[f64; 2]> {
    let axis = subtract(end, start);
    let length = dot(axis, axis).sqrt();
    if length == 0.0 {
        return sphere_interval(ray, start, radius);
    }
    let axis = scale(axis, 1.0 / length);
    let relative = subtract(ray.origin, start);
    let origin_axis = dot(relative, axis);
    let direction_axis = dot(ray.direction, axis);
    let radial_origin = subtract(relative, scale(axis, origin_axis));
    let radial_direction = subtract(ray.direction, scale(axis, direction_axis));
    let mut cylinder = quadratic_interval(
        dot(radial_direction, radial_direction),
        dot(radial_origin, radial_direction),
        dot(radial_origin, radial_origin) - radius * radius,
    );
    if let Some(range) = &mut cylinder {
        if direction_axis == 0.0 {
            if origin_axis < 0.0 || origin_axis > length {
                cylinder = None;
            }
        } else {
            let a = -origin_axis / direction_axis;
            let b = (length - origin_axis) / direction_axis;
            range[0] = range[0].max(a.min(b));
            range[1] = range[1].min(a.max(b));
            if range[0] > range[1] {
                cylinder = None;
            }
        }
    }
    let mut result = cylinder;
    for center in [start, end] {
        if let Some(cap) = sphere_interval(ray, center, radius) {
            if let Some(range) = &mut result {
                range[0] = range[0].min(cap[0]);
                range[1] = range[1].max(cap[1]);
            } else {
                result = Some(cap);
            }
        }
    }
    result
}
