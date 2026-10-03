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
        if point.iter().any(|value| !value.is_finite())
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
            for (part, mark) in chart.geometry.hits.iter().enumerate() {
                let limit = nearest.map_or(far, |hit| hit.hit.distance);
                let Some(distance) = mark.shape.ray_intersection(&local, near, limit) else {
                    continue;
                };
                let candidate = PlotRayHit {
                    target: chart.target,
                    row: mark.row,
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
            Self::Rect([left, top, right, bottom]) => {
                point[0] >= left && point[0] <= right && point[1] >= top && point[1] <= bottom
            }
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
