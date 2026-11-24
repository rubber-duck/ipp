//! Exact row identity on compact analytic chart geometry, independent of mesh triangles.

use super::*;
use crate::systems::canvas::{CanvasClip, CanvasTarget};
use crate::systems::geometry::{
    GeometryBounds, GeometryRay, GeometryRayHit, GeometryShape, GeometryShapeTransform,
};
use std::sync::Arc;

/// Scene hit tied to the exact producing chart lifetime.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotRayHit {
    /// Entity/component lifetime within the producing World.
    pub target: CanvasTarget,
    /// Exact original source row and authored series slot.
    pub row: PlotRowIdentity,
    /// Caller-space ray parameter and local hit ordinal.
    pub hit: GeometryRayHit,
}

/// Canvas placement of a retained chart hit list; shares the immutable local result.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotCanvasHit {
    /// Rotated drawing space, when inherited.
    pub space: Option<Arc<crate::systems::canvas::CanvasSpace>>,
    /// Producing chart lifetime.
    pub target: CanvasTarget,
    /// Shared local shape and row identity data.
    pub geometry: Arc<PlotPreparedGeometry>,
    /// Final logical origin and scale, [tx,ty,sx,sy].
    pub placement: [f32; 4],
    /// Already intersected Canvas clip.
    pub clip: CanvasClip,
    /// Final resolved layer for painter ordering.
    pub layer: u32,
    /// Tree painter ordinal in the containing output.
    pub order: u32,
}

impl PlotCanvasHit {
    /// First local authored mark under the point; no numeric row conversion.
    pub fn pick(&self, point: [f32; 2]) -> Option<PlotRowIdentity> {
        let point = self
            .space
            .as_ref()
            .map_or(point, |s| s.inverse.point(point));
        if self.space.as_ref().is_some_and(|s| !s.contains(point))
            || point.iter().any(|value| !value.is_finite())
            || point[0] < self.clip[0]
            || point[1] < self.clip[1]
            || point[0] >= self.clip[2]
            || point[1] >= self.clip[3]
            || self.placement[2] == 0.0
            || self.placement[3] == 0.0
        {
            return None;
        }
        let local = [
            (point[0] - self.placement[0]) / self.placement[2],
            (point[1] - self.placement[1]) / self.placement[3],
        ];
        self.geometry
            .hits
            .iter()
            .rev()
            .find(|hit| hit.shape.contains_canvas_point(local))
            .map(|hit| hit.row)
    }
}

impl PlotPublication {
    /// Nearest scene mark, preserving the ray parameter through entity transforms.
    pub fn pick(&self, ray: &GeometryRay, near: f64, far: f64) -> Option<PlotRayHit> {
        self.pick_in_view(ray, near, far, None)
    }

    /// The completed camera supplies projection and the containing World's view transform.
    pub(crate) fn pick_in_view(
        &self,
        ray: &GeometryRay,
        near: f64,
        far: f64,
        view: Option<(&GeometryShapeTransform, bool)>,
    ) -> Option<PlotRayHit> {
        if ray
            .origin
            .iter()
            .chain(&ray.direction)
            .any(|value| !value.is_finite())
            || !near.is_finite()
            || !far.is_finite()
            || near < 0.0
            || far < near
            || ray.direction == [0.0; 3]
        {
            return None;
        }
        let mut nearest: Option<PlotRayHit> = None;
        for chart in &self.charts {
            let Ok(transform) = GeometryShapeTransform::from_matrix(chart.model) else {
                continue;
            };
            let local = transform.inverse_ray(ray);
            let limit = nearest.map_or(far, |hit| hit.hit.distance);
            let surface = &chart.geometry.height_surface;
            let surface_hit = if surface.faces.is_empty() {
                None
            } else {
                let chart_to_view = match view {
                    Some((world_to_view, _)) => {
                        let Ok(placement) = transform.then(world_to_view) else {
                            continue;
                        };
                        placement
                    }
                    None => transform,
                };
                surface.pick(
                    &local,
                    near,
                    far,
                    &chart_to_view,
                    view.map(|(_, parallel)| parallel),
                )
            };
            let marks = chart
                .geometry
                .hits
                .iter()
                .enumerate()
                .filter_map(|(part, mark)| {
                    Some((
                        part,
                        mark.row,
                        mark.shape.ray_intersection(&local, near, limit)?,
                    ))
                });
            for (part, row, distance) in marks.chain(surface_hit) {
                let candidate = PlotRayHit {
                    target: chart.target,
                    row,
                    hit: GeometryRayHit {
                        distance,
                        part: part as u32,
                    },
                };
                if nearest.is_none_or(|previous| {
                    distance < previous.hit.distance
                        || distance == previous.hit.distance
                            && (candidate.target, part)
                                < (previous.target, previous.hit.part as usize)
                }) {
                    nearest = Some(candidate);
                }
            }
        }
        nearest
    }
}

impl PlotHitShape {
    /// Test compact 2D marks in top-left/Y-down local coordinates.
    pub fn contains_canvas_point(&self, point: [f32; 2]) -> bool {
        match *self {
            Self::Band {
                ends: [a, b],
                clip,
            } => {
                if point[0] < clip[0]
                    || point[0] > clip[2]
                    || point[1] < clip[1]
                    || point[1] > clip[3]
                    || a[0] == b[0]
                {
                    return false;
                }
                let t = (point[0] - a[0]) / (b[0] - a[0]);
                let upper = a[1] + t * (b[1] - a[1]);
                let lower = a[2] + t * (b[2] - a[2]);
                (0.0..=1.0).contains(&t)
                    && point[1] >= upper.min(lower)
                    && point[1] <= upper.max(lower)
            }
            Self::Rect([left, top, right, bottom]) => {
                point[0] >= left && point[0] <= right && point[1] >= top && point[1] <= bottom
            }
            Self::SlantedBar(corners) => corners.iter().enumerate().all(|(index, a)| {
                let b = corners[(index + 1) % 4];
                (b[0] - a[0]) * (point[1] - a[1]) >= (b[1] - a[1]) * (point[0] - a[0])
            }),
            Self::Circle {
                center,
                radius,
            } => (point[0] - center[0]).hypot(point[1] - center[1]) <= radius,
            Self::Sector {
                center,
                inner_radius,
                radius,
                start,
                sweep,
            } => {
                let x = f64::from(point[0] - center[0]);
                let y = f64::from(center[1] - point[1]);
                let distance = x.hypot(y);
                distance >= f64::from(inner_radius)
                    && distance <= f64::from(radius)
                    && in_sector(x, y, f64::from(start), f64::from(sweep))
            }
            _ => false,
        }
    }

    /// First closed-solid boundary in the local ray interval. Box/sphere reuse Geometry.
    pub fn ray_intersection(&self, ray: &GeometryRay, near: f64, far: f64) -> Option<f64> {
        match *self {
            Self::Box {
                min,
                max,
            } => GeometryShape::Box {
                min: min.map(f64::from),
                max: max.map(f64::from),
            }
            .ray_intersection(ray, near, far)
            .map(|hit| hit.distance),
            Self::Bar(solid) => {
                let [cx, cz] = solid.center_xz().map(f64::from);
                let (sin, cos) = f64::from(solid.yaw).sin_cos();
                let unturn = |[x, y, z]: [f64; 3]| [cos * x - sin * z, y, sin * x + cos * z];
                let local = GeometryRay {
                    origin: unturn([ray.origin[0] - cx, ray.origin[1], ray.origin[2] - cz]),
                    direction: unturn(ray.direction),
                };
                let relative =
                    |p: [f32; 3]| [f64::from(p[0]) - cx, f64::from(p[1]), f64::from(p[2]) - cz];
                let min = relative(solid.min);
                let max = relative(solid.max);
                if solid.bevel == 0.0 {
                    GeometryShape::Box {
                        min,
                        max,
                    }
                    .ray_intersection(&local, near, far)
                    .map(|hit| hit.distance)
                } else {
                    beveled_bar(&local, near, far, min, max, solid.bevel)
                }
            }
            Self::Sphere {
                center,
                radius,
            } => GeometryShape::Sphere {
                center: center.map(f64::from),
                radius: f64::from(radius),
            }
            .ray_intersection(ray, near, far)
            .map(|hit| hit.distance),
            Self::RadialPrism {
                center,
                radius,
                start,
                sweep,
                min_y,
                max_y,
            } => radial_prism(
                ray,
                near,
                far,
                center.map(f64::from),
                f64::from(radius),
                f64::from(start),
                f64::from(sweep),
                [f64::from(min_y), f64::from(max_y)],
            ),
            _ => None,
        }
    }
}

/// Intersect the six box half-spaces and the eight 45-degree cap planes.
fn beveled_bar(
    ray: &GeometryRay,
    near: f64,
    far: f64,
    min: [f64; 3],
    max: [f64; 3],
    bevel: f32,
) -> Option<f64> {
    let mut entry = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    let mut clip = |distance: f64, direction: f64| {
        if direction == 0.0 {
            return distance >= 0.0;
        }

        let crossing = distance / direction;
        if direction > 0.0 {
            exit = exit.min(crossing);
        } else {
            entry = entry.max(crossing);
        }
        entry <= exit
    };

    for axis in 0..3 {
        if !clip(max[axis] - ray.origin[axis], ray.direction[axis])
            || !clip(ray.origin[axis] - min[axis], -ray.direction[axis])
        {
            return None;
        }
    }
    for axis in [0, 2] {
        for (side, wall) in [(-1.0, min[axis]), (1.0, max[axis])] {
            for (vertical, cap) in [(-1.0, min[1]), (1.0, max[1])] {
                let distance = side * (wall - ray.origin[axis]) + vertical * (cap - ray.origin[1])
                    - f64::from(bevel);
                if !clip(
                    distance,
                    side * ray.direction[axis] + vertical * ray.direction[1],
                ) {
                    return None;
                }
            }
        }
    }

    [entry, exit].into_iter().find(|t| *t >= near && *t <= far)
}

fn in_sector(x: f64, z: f64, start: f64, sweep: f64) -> bool {
    if x == 0.0 && z == 0.0 {
        return true;
    }
    let span = sweep.abs();
    let epsilon = 64.0 * f64::EPSILON;
    span >= std::f64::consts::TAU - epsilon
        || if sweep >= 0.0 {
            (x.atan2(z) - start).rem_euclid(std::f64::consts::TAU) <= span + epsilon
        } else {
            (start - x.atan2(z)).rem_euclid(std::f64::consts::TAU) <= span + epsilon
        }
}

#[allow(clippy::too_many_arguments)]
fn radial_prism(
    ray: &GeometryRay,
    near: f64,
    far: f64,
    center: [f64; 3],
    radius: f64,
    start: f64,
    sweep: f64,
    y: [f64; 2],
) -> Option<f64> {
    if radius <= 0.0 || sweep == 0.0 || y[0] > y[1] {
        return None;
    }
    let x = ray.origin[0] - center[0];
    let z = ray.origin[2] - center[2];
    let dx = ray.direction[0];
    let dz = ray.direction[2];
    let tolerance = 64.0 * f64::EPSILON * (radius + y[0].abs() + y[1].abs() + 1.0);
    let mut nearest: Option<f64> = None;
    let mut consider = |t: f64| {
        if !t.is_finite() || t < near || t > far || nearest.is_some_and(|previous| t >= previous) {
            return;
        }
        let px = x + t * dx;
        let pz = z + t * dz;
        let py = ray.origin[1] + t * ray.direction[1];
        if py >= y[0] - tolerance
            && py <= y[1] + tolerance
            && px.hypot(pz) <= radius + tolerance
            && in_sector(px, pz, start, sweep)
        {
            nearest = Some(t);
        }
    };
    let a = dx * dx + dz * dz;
    let b = x * dx + z * dz;
    let c = x * x + z * z - radius * radius;
    let discriminant = b * b - a * c;
    if a > 0.0 && discriminant >= 0.0 {
        let root = discriminant.sqrt();
        consider((-b - root) / a);
        consider((-b + root) / a);
    }
    if ray.direction[1] != 0.0 {
        consider((y[0] - ray.origin[1]) / ray.direction[1]);
        consider((y[1] - ray.origin[1]) / ray.direction[1]);
    }
    if sweep.abs() < std::f64::consts::TAU {
        for angle in [start, start + sweep] {
            let (sine, cosine) = angle.sin_cos();
            let denominator = cosine * dx - sine * dz;
            if denominator != 0.0 {
                let t = -(cosine * x - sine * z) / denominator;
                // A radial side is a half-plane from the centre, not its opposite ray.
                if (x + t * dx) * sine + (z + t * dz) * cosine >= -tolerance {
                    consider(t);
                }
            }
        }
    }
    nearest
}

#[cfg(test)]
#[path = "picking_tests.rs"]
mod tests;
