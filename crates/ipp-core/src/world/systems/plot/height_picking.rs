//! Height fields use their valid sample connectivity for both coverage and row ownership.

use super::{PlotHeightPicking, PlotRowIdentity};
use crate::systems::geometry::{GeometryRay, GeometryShapeTransform, dot, scale, subtract};

struct Candidate {
    site: usize,
    distance_squared: f64,
    depth: f64,
    sightline: GeometryRay,
}

impl PlotHeightPicking {
    pub(super) fn pick(
        &self,
        local_ray: &GeometryRay,
        near: f64,
        far: f64,
        to_view: &GeometryShapeTransform,
        orthographic: Option<bool>,
    ) -> Option<(usize, PlotRowIdentity, f64)> {
        // Work relative to the pointer-ray origin. Camera space supplies the pixel metric
        // and the same front winding as rendering, including reflected chart transforms.
        let direction = to_view.vector(local_ray.direction);
        let points: Vec<_> = self
            .sites
            .iter()
            .map(|(p, _)| to_view.vector(subtract(p.map(f64::from), local_ray.origin)))
            .collect();
        let ray = GeometryRay {
            origin: [0.; 3],
            direction,
        };
        let mut nearest = None;
        let mut limit = far;
        for &face in &self.faces {
            if let Some(t) = intersection(&ray, face.map(|i| points[i]), near, limit)
                && (nearest.is_none() || t < limit)
            {
                limit = t;
                nearest = Some(face);
            }
        }
        let face = nearest?;
        // Camera clip planes are constant view Z. A standalone geometric query
        // uses planes perpendicular to its direction.
        let forward = if orthographic.is_some() {
            [0., 0., -1.]
        } else {
            direction
        };
        let forward_step = dot(direction, forward);
        let parallel = orthographic == Some(true);
        let rank = |i: usize| {
            let depth = dot(points[i], forward) / forward_step;
            if depth < near || depth > far || depth <= 0. {
                return None;
            }
            let offset = subtract(points[i], scale(direction, depth));
            let screen = if parallel {
                offset
            } else {
                scale(offset, 1. / depth)
            };
            let sightline = if parallel {
                GeometryRay {
                    origin: offset,
                    direction,
                }
            } else {
                GeometryRay {
                    origin: [0.; 3],
                    direction: scale(points[i], 1. / depth),
                }
            };
            Some(Candidate {
                site: i,
                distance_squared: dot(screen, screen),
                depth,
                sightline,
            })
        };
        let closest_visible = |mut candidates: Vec<Candidate>| {
            candidates.sort_unstable_by(|a, b| {
                let row_key = |i: usize| (self.sites[i].1.series, self.sites[i].1.row_id);
                a.distance_squared
                    .total_cmp(&b.distance_squared)
                    .then_with(|| row_key(a.site).cmp(&row_key(b.site)))
            });
            candidates.into_iter().find(|candidate| {
                // The first front face on this sample's own sightline must reach the sample.
                // Adjacent faces can agree on that endpoint to within rounding error.
                let tolerance = 64. * f64::EPSILON * candidate.depth;
                let mut reaches_sample = false;
                for face in &self.faces {
                    if let Some(t) = intersection(
                        &candidate.sightline,
                        face.map(|i| points[i]),
                        near,
                        candidate.depth + tolerance,
                    ) {
                        if t < candidate.depth - tolerance {
                            return false;
                        }
                        reaches_sample = true;
                    }
                }
                reaches_sample
            })
        };
        let picked =
            closest_visible(face.into_iter().filter_map(rank).collect()).or_else(|| {
                // A sliver can be visible through a gap even when all its corners are hidden.
                closest_visible((0..points.len()).filter_map(rank).collect())
            })?;
        Some((picked.site, self.sites[picked.site].1, limit))
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Intersect the front of an oriented sample face, preserving the input ray parameter.
fn intersection(ray: &GeometryRay, [a, b, c]: [[f64; 3]; 3], near: f64, far: f64) -> Option<f64> {
    let ab = subtract(b, a);
    let ac = subtract(c, a);
    let perpendicular = cross(ray.direction, ac);
    let determinant = dot(ab, perpendicular);
    if determinant <= 0. {
        return None;
    }
    let from_a = subtract(ray.origin, a);
    let u = dot(from_a, perpendicular);
    let across = cross(from_a, ab);
    let v = dot(ray.direction, across);
    let tolerance = 32. * f64::EPSILON * determinant;
    if u < -tolerance || v < -tolerance || u + v > determinant + tolerance {
        return None;
    }
    let t = dot(ac, across) / determinant;
    (near..=far).contains(&t).then_some(t)
}
