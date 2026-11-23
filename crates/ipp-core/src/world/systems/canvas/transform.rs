//! Coordinate spaces introduced by a rotating Canvas subtree. Axis-aligned
//! descendants keep using their ordinary placement and rectangular part clips.

use super::CanvasClip;
use std::sync::Arc;

/// Affine mapping with two basis columns and a translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasAffine {
    /// Images of the unit X and Y vectors.
    pub axes: [[f32; 2]; 2],
    /// Image of the origin.
    pub origin: [f32; 2],
}

impl CanvasAffine {
    /// The canvas coordinate system.
    pub const IDENTITY: Self = Self {
        axes: [[1., 0.], [0., 1.]],
        origin: [0.; 2],
    };

    /// Axis-aligned mapping used by layout and rectilinear descendants.
    pub fn axis_aligned(origin: [f32; 2], scale: [f32; 2]) -> Self {
        Self {
            origin,
            axes: [[scale[0], 0.], [0., scale[1]]],
        }
    }

    /// Apply the evaluated Canvas mapping to a physical-coordinate query.
    pub fn point_f64(self, p: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|i| {
            f64::from(self.origin[i])
                + f64::from(self.axes[0][i]) * p[0]
                + f64::from(self.axes[1][i]) * p[1]
        })
    }

    /// Transform a direction without translating it.
    pub fn vector(self, p: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|i| self.axes[0][i] * p[0] + self.axes[1][i] * p[1])
    }

    /// Transform a point into the enclosing coordinates.
    pub fn point(self, p: [f32; 2]) -> [f32; 2] {
        let d = self.vector(p);
        [self.origin[0] + d[0], self.origin[1] + d[1]]
    }

    /// Apply the inner mapping first.
    pub fn compose(self, inner: Self) -> Self {
        Self {
            axes: inner.axes.map(|v| self.vector(v)),
            origin: self.point(inner.origin),
        }
    }

    /// Inverse mapping if both axes span a plane.
    pub fn inverse(self) -> Option<Self> {
        let [[a, b], [c, d]] = self.axes;
        let det = a * d - b * c;
        if det == 0. || !det.is_finite() {
            return None;
        }
        let mut result = Self {
            axes: [[d / det, -b / det], [-c / det, a / det]],
            origin: [0.; 2],
        };
        result.origin = result.vector(self.origin.map(|v| -v));
        Some(result)
    }

    /// Bounds of all four transformed corners.
    pub fn bounds(self, rect: CanvasClip) -> CanvasClip {
        let corners = corners(rect).map(|p| self.point(p));
        envelope(&corners)
    }

    /// Column-major matrix for placing painted geometry.
    pub fn matrix(self) -> [f32; 16] {
        let [[a, b], [c, d]] = self.axes;
        [
            a,
            b,
            0.,
            0.,
            c,
            d,
            0.,
            0.,
            0.,
            0.,
            1.,
            0.,
            self.origin[0],
            self.origin[1],
            0.,
            1.,
        ]
    }
}

fn corners([l, t, r, b]: CanvasClip) -> [[f32; 2]; 4] {
    [[l, t], [r, t], [r, b], [l, b]]
}

fn envelope(points: &[[f32; 2]]) -> CanvasClip {
    let Some(&[x, y]) = points.first() else {
        return [0.; 4];
    };
    points.iter().fold([x, y, x, y], |[l, t, r, b], &[x, y]| {
        [l.min(x), t.min(y), r.max(x), b.max(y)]
    })
}

/// A rotated drawing space, shared by all its axis-aligned descendants.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasSpace {
    /// Mapping from drawing coordinates to canvas coordinates.
    pub affine: CanvasAffine,
    /// Mapping from canvas coordinates to drawing coordinates.
    pub inverse: CanvasAffine,
    /// Inherited convex clipping polygon in drawing coordinates.
    pub polygon: Vec<[f32; 2]>,
}

impl CanvasSpace {
    pub(super) fn child(
        parent: Option<&Self>,
        local: CanvasAffine,
        clip: CanvasClip,
    ) -> Option<Arc<Self>> {
        let inverse_local = local.inverse()?;
        let affine = parent.map_or(local, |p| p.affine.compose(local));
        let polygon = match parent {
            Some(p) => clip_polygon(&p.polygon, clip),
            None if clip[0] < clip[2] && clip[1] < clip[3] => corners(clip).to_vec(),
            None => Vec::new(),
        }
        .into_iter()
        .map(|p| inverse_local.point(p))
        .collect();
        Some(Arc::new(Self {
            affine,
            inverse: affine.inverse()?,
            polygon,
        }))
    }

    /// Conservative rectangle used to cull work inside the exact polygon.
    pub fn clip_bounds(&self) -> CanvasClip {
        envelope(&self.polygon)
    }

    /// Whether a drawing-space point is inside every inherited edge.
    pub fn contains(&self, point: [f32; 2]) -> bool {
        if self.polygon.len() < 3 {
            return false;
        }
        let mut positive = false;
        let mut negative = false;
        for (&a, &b) in self.polygon.iter().zip(self.polygon.iter().cycle().skip(1)) {
            let side = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
            positive |= side > 0.;
            negative |= side < 0.;
            if positive && negative {
                return false;
            }
        }
        positive || negative
    }

    /// Intersect the inherited polygon with a presentation's canvas-space clip.
    pub fn visible_polygon(&self, clip: CanvasClip) -> Vec<[f32; 2]> {
        let canvas: Vec<_> = self.polygon.iter().map(|&p| self.affine.point(p)).collect();
        clip_polygon(&canvas, clip)
            .into_iter()
            .map(|p| self.inverse.point(p))
            .collect()
    }
}

/// Sutherland-Hodgman clipping against the four sides of a rectangle.
fn clip_polygon(points: &[[f32; 2]], rect: CanvasClip) -> Vec<[f32; 2]> {
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return Vec::new();
    }
    let mut input = points.to_vec();
    let mut output = Vec::new();
    for (axis, edge, sign) in [
        (0, rect[0], 1.),
        (1, rect[1], 1.),
        (0, rect[2], -1.),
        (1, rect[3], -1.),
    ] {
        let Some(mut start) = input.last().copied() else {
            break;
        };
        for &end in &input {
            let a = (start[axis] - edge) * sign;
            let b = (end[axis] - edge) * sign;
            if (a < 0. && b > 0.) || (b < 0. && a > 0.) {
                let fraction = a / (a - b);
                let mut cross = std::array::from_fn(|i| start[i] + fraction * (end[i] - start[i]));
                cross[axis] = edge;
                output.push(cross);
            }
            if b >= 0. {
                output.push(end);
            }
            start = end;
        }
        std::mem::swap(&mut input, &mut output);
        output.clear();
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::canvas::CanvasStyle;

    #[test]
    fn reflected_subtree_keeps_its_pivot_and_ancestor_clip() {
        let rect = [0., 0., 20., 10.];
        let style = CanvasStyle {
            x: 20.,
            y: 3.,
            rotation: 0.7,
            scale_x: -2.,
            scale_y: 0.5,
            pivot_x: 0.5,
            pivot_y: 0.5,
            ..Default::default()
        };
        let affine = style.affine([20., 10.]);
        let pivot = affine.point([10., 5.]);
        assert!(pivot[0].abs() < 1e-5 && (pivot[1] - 5.5).abs() < 1e-5);
        let outer = CanvasSpace::child(None, affine, rect).unwrap();
        let inner = CanvasSpace::child(
            Some(&outer),
            CanvasStyle {
                rotation: -0.3,
                ..Default::default()
            }
            .affine([0.; 2]),
            [-10., -10., 30., 30.],
        )
        .unwrap();
        for point in [[1., 5.], [3., 6.], [-1., 5.], [8., 12.]] {
            let local = inner.inverse.point(point);
            let back = inner.affine.point(local);
            assert!((back[0] - point[0]).abs() < 1e-4 && (back[1] - point[1]).abs() < 1e-4);
            let parent = outer.inverse.point(point);
            let visible = point[0] >= 0.
                && point[0] < 20.
                && point[1] >= 0.
                && point[1] < 10.
                && parent.iter().all(|v| (-10.0..30.0).contains(v));
            assert_eq!(inner.contains(local), visible);
        }
    }
}
