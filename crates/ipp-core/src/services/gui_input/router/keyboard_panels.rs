//! Camera-relative panel placement for keyboard traversal order.
//!
//! Keys carry no position, so the panel order comes from the camera pose:
//! panels whose front face looks at the camera order first by World-space view
//! distance, then back-facing panels by the same distance, so they stay
//! reachable by continued traversal but are never preferred.
//!
//! A panel's view distance is the World-space distance from a perspective
//! camera's eye to the nearest point of its Surface rectangle, or the depth
//! of that rectangle's nearest corner along an orthographic camera's view
//! direction. A panel faces the camera when the eye (perspective) or the
//! view direction (orthographic) lies on the side of its +Z front face,
//! matching the front-face rule of the projected pointer path.

use crate::systems::geometry::GeometryRay;

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

    /// Whether a panel with this World transform and Surface extent faces the
    /// view, with its view distance.
    pub(crate) fn placement(
        &self,
        affine: &crate::systems::geometry::GeometryShapeTransform,
        extent: [f64; 2],
    ) -> Option<(bool, f64)> {
        placement(self, affine, extent)
    }
}

fn placement(
    view: &GuiKeyboardView,
    affine: &crate::systems::geometry::GeometryShapeTransform,
    extent: [f64; 2],
) -> Option<(bool, f64)> {
    if !extent.iter().all(|value| value.is_finite() && *value > 0.0) {
        return None;
    }
    let local = affine.inverse_ray(&GeometryRay {
        origin: view.eye,
        direction: view.forward,
    });
    let front = if view.orthographic {
        local.direction[2] < 0.0
    } else {
        local.origin[2] > 0.0
    };

    // The Surface rectangle is centred on the entity-local XY plane.
    let half = extent.map(|value| value * 0.5);
    let origin = affine.point([0.0; 3]);
    let axes = [
        affine.vector([1.0, 0.0, 0.0]),
        affine.vector([0.0, 1.0, 0.0]),
    ];
    let distance = if view.orthographic {
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
    };

    distance.is_finite().then_some((front, distance))
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
