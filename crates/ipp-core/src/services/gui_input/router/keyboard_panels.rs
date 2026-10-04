//! Camera-relative panel placement for keyboard traversal order.
//!
//! Keys carry no position, so the panel order comes from the camera pose:
//! panels whose front face looks at the camera order first by World-space view
//! distance, then back-facing panels by the same distance, so they stay
//! reachable by continued traversal but are never preferred.
//!
//! Affine providers use their exact content mapping. Curved providers use a
//! bounded triangle approximation of the actual surface. Keys have no hit
//! position, so facing uses the sampled centre front normal; pointer facing
//! instead uses the exact intersection normal.

use crate::systems::{
    geometry::{GeometryRay, GeometryShapeTransform},
    surface::Surface,
};

/// Camera pose the keyboard panel order reads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GuiKeyboardView {
    /// World-space camera position.
    eye: [f64; 3],
    /// World-space view direction (the camera's local -Z).
    forward: [f64; 3],
    /// Orthographic projection: distance is depth along `forward`.
    orthographic: bool,
}

impl GuiKeyboardView {
    /// View from a camera's World-space eye and view axis, or None for a
    /// non-finite eye or a degenerate axis.
    pub(crate) fn from_pose(eye: [f64; 3], forward: [f64; 3], orthographic: bool) -> Option<Self> {
        let length = dot(forward, forward).sqrt();
        if !length.is_finite() || length <= 0.0 || !eye.iter().all(|value| value.is_finite()) {
            return None;
        }
        Some(Self {
            eye,
            forward: scale(forward, 1.0 / length),
            orthographic,
        })
    }

    pub(crate) fn of_publication(
        camera: &crate::systems::camera::CameraPublication,
    ) -> Option<Self> {
        Self::from_pose(
            camera.pose.point([0.0; 3]),
            camera.pose.vector([0.0, 0.0, -1.0]),
            camera.projection.projection != 0,
        )
    }

    /// Whether a panel with this World transform and completed geometry faces the
    /// view, with its view distance.
    pub(crate) fn placement(
        &self,
        affine: &crate::systems::geometry::GeometryShapeTransform,
        surface: &dyn Surface,
    ) -> Option<(bool, f64)> {
        placement(self, affine, surface)
    }
}

fn placement(
    view: &GuiKeyboardView,
    affine: &GeometryShapeTransform,
    surface: &dyn Surface,
) -> Option<(bool, f64)> {
    let extent = surface.physical_extent();
    if !extent.iter().all(|value| value.is_finite() && *value > 0.0) {
        return None;
    }
    surface.validate_offsets([0.0, 0.0]).ok()?;
    let half = extent.map(|value| value * 0.5);
    let centre = surface.sample(half, 0.0).ok()?;
    let local = affine.inverse_ray(&GeometryRay {
        origin: view.eye,
        direction: view.forward,
    });
    let front = if view.orthographic {
        dot(centre.front_normal, local.direction) < 0.0
    } else {
        dot(centre.front_normal, sub(local.origin, centre.position)) > 0.0
    };
    let distance = if let Some(mapping) = surface.exact_affine(0.0) {
        // This is a 2D affine embedding: its unused third column need not
        // make an invertible 3D transform. Compose only origin and XY axes.
        let origin = affine.point(std::array::from_fn(|axis| {
            mapping[12 + axis] + mapping[axis] * half[0] + mapping[4 + axis] * half[1]
        }));
        let axes = [
            affine.vector([mapping[0], mapping[1], mapping[2]]),
            affine.vector([mapping[4], mapping[5], mapping[6]]),
        ];
        if view.orthographic {
            [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
                .into_iter()
                .map(|(u, v)| {
                    let corner = add(
                        origin,
                        add(scale(axes[0], u * half[0]), scale(axes[1], v * half[1])),
                    );
                    dot(sub(corner, view.eye), view.forward)
                })
                .fold(f64::INFINITY, f64::min)
        } else {
            rectangle_distance(origin, axes, half, view.eye)
        }
    } else {
        sampled_distance(view, affine, surface, extent)?
    };
    distance.is_finite().then_some((front, distance))
}

/// Approximate distance to the surface itself, never its conservative enclosure.
/// Subdivide deterministically toward a 1cm World-space Euclidean error bound.
/// Depth six caps work at 4096 cells/8192 triangles per panel; exceptionally
/// large/curved/scaled panels can retain more error at that cap. Keys have no
/// geometric hit position, so this is a bounded ordering approximation.
fn sampled_distance(
    view: &GuiKeyboardView,
    affine: &GeometryShapeTransform,
    surface: &dyn Surface,
    extent: [f64; 2],
) -> Option<f64> {
    const TOLERANCE: f64 = 0.01;
    const MAX_DEPTH: u32 = 6;
    // Frobenius norm bounds the placement's operator norm, including shear.
    let scale_bound = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .into_iter()
        .map(|axis| {
            let axis = affine.vector(axis);
            dot(axis, axis)
        })
        .sum::<f64>()
        .sqrt();
    if !scale_bound.is_finite() {
        return None;
    }
    let mut pending = vec![([0.0, 0.0, extent[0], extent[1]], 0)];
    let mut best = f64::INFINITY;
    while let Some((patch, depth)) = pending.pop() {
        let error = surface.approximation_error(patch, 0.0).ok()? * scale_bound;
        if !error.is_finite() || error < 0.0 {
            return None;
        }
        if error > TOLERANCE && depth < MAX_DEPTH {
            let x = (patch[0] + patch[2]) * 0.5;
            let y = (patch[1] + patch[3]) * 0.5;
            for cell in [
                [patch[0], patch[1], x, y],
                [x, patch[1], patch[2], y],
                [patch[0], y, x, patch[3]],
                [x, y, patch[2], patch[3]],
            ] {
                pending.push((cell, depth + 1));
            }
            continue;
        }
        let coordinates = [
            [patch[0], patch[1]],
            [patch[2], patch[1]],
            [patch[0], patch[3]],
            [patch[2], patch[3]],
        ];
        let mut points = [[0.0; 3]; 4];
        for (point, content) in points.iter_mut().zip(coordinates) {
            *point = affine.point(surface.sample(content, 0.0).ok()?.position);
        }
        if view.orthographic {
            for point in points {
                best = best.min(dot(sub(point, view.eye), view.forward));
            }
        } else {
            best = best.min(triangle_distance(
                view.eye,
                [points[0], points[1], points[3]],
            ));
            best = best.min(triangle_distance(
                view.eye,
                [points[0], points[3], points[2]],
            ));
        }
    }
    best.is_finite().then_some(best)
}

fn triangle_distance(point: [f64; 3], triangle: [[f64; 3]; 3]) -> f64 {
    let a = sub(triangle[1], triangle[0]);
    let b = sub(triangle[2], triangle[0]);
    let offset = sub(point, triangle[0]);
    let aa = dot(a, a);
    let bb = dot(b, b);
    let ab = dot(a, b);
    let ad = dot(a, offset);
    let bd = dot(b, offset);
    let determinant = aa * bb - ab * ab;
    if determinant > 0.0 {
        let u = (ad * bb - bd * ab) / determinant;
        let v = (bd * aa - ad * ab) / determinant;
        if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
            let away = sub(offset, add(scale(a, u), scale(b, v)));
            return dot(away, away).sqrt();
        }
    }
    [[0, 1], [1, 2], [2, 0]]
        .into_iter()
        .map(|[start, end]| {
            let axis = sub(triangle[end], triangle[start]);
            let length = dot(axis, axis);
            let t = if length > 0.0 {
                (dot(sub(point, triangle[start]), axis) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let away = sub(point, add(triangle[start], scale(axis, t)));
            dot(away, away).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

/// Distance from `point` to the World-space parallelogram
/// `origin + u * axes[0] + v * axes[1]` with `|u| <= half[0]` and
/// `|v| <= half[1]`. The squared distance is a convex quadratic in `(u, v)`,
/// so its minimum is the unconstrained minimizer when that lies inside,
/// otherwise the best clamped minimizer along one of the four edges.
/// Degenerate rectangles return infinity.
fn rectangle_distance(
    origin: [f64; 3],
    axes: [[f64; 3]; 2],
    half: [f64; 2],
    point: [f64; 3],
) -> f64 {
    let offset = sub(point, origin);
    let [a, b] = axes;
    let (aa, bb, ab) = (dot(a, a), dot(b, b), dot(a, b));
    let (ad, bd) = (dot(a, offset), dot(b, offset));
    let at = |u: f64, v: f64| {
        let away = sub(offset, add(scale(a, u), scale(b, v)));
        dot(away, away).sqrt()
    };

    let determinant = aa * bb - ab * ab;
    if determinant > 0.0 {
        let u = (ad * bb - bd * ab) / determinant;
        let v = (bd * aa - ad * ab) / determinant;
        if u.abs() <= half[0] && v.abs() <= half[1] {
            return at(u, v);
        }
    }

    let mut best = f64::INFINITY;
    if aa > 0.0 {
        for v in [-half[1], half[1]] {
            let u = ((ad - v * ab) / aa).clamp(-half[0], half[0]);
            best = best.min(at(u, v));
        }
    }
    if bb > 0.0 {
        for u in [-half[0], half[0]] {
            let v = ((bd - u * ab) / bb).clamp(-half[1], half[1]);
            best = best.min(at(u, v));
        }
    }
    best
}

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn sub(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(vector: [f64; 3], factor: f64) -> [f64; 3] {
    [vector[0] * factor, vector[1] * factor, vector[2] * factor]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

#[cfg(test)]
mod tests {
    use super::rectangle_distance;

    const AXES: [[f64; 3]; 2] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    #[test]
    fn rectangle_distance_reaches_the_face_edges_and_corners() {
        let near = |point: [f64; 3], expected: f64| {
            let distance = rectangle_distance([0.0; 3], AXES, [2.0, 1.0], point);
            assert!((distance - expected).abs() < 1e-9, "{point:?}: {distance}");
        };

        // In front of the face, beside an edge and beyond a corner.
        near([0.5, -0.5, 3.0], 3.0);
        near([5.0, 0.0, 4.0], 5.0);
        near([5.0, 5.0, 0.0], 5.0);
    }

    #[test]
    fn rectangle_distance_follows_sheared_and_degenerate_axes() {
        // A sheared parallelogram: the point above its slanted top edge.
        let sheared = [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
        let distance = rectangle_distance([0.0; 3], sheared, [1.0, 1.0], [2.0, 3.0, 0.0]);
        assert!((distance - 2.0).abs() < 1e-9, "{distance}");

        let flat = [[0.0; 3], [0.0; 3]];
        assert_eq!(
            rectangle_distance([0.0; 3], flat, [1.0, 1.0], [1.0, 0.0, 0.0]),
            f64::INFINITY
        );
    }
}
