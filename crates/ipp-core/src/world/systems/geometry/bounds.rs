/// A ray in the caller's coordinate system. A unit direction makes t a distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryRay {
    /// GeometryRay starting point.
    pub origin: [f64; 3],
    /// GeometryRay direction; need not be normalized.
    pub direction: [f64; 3],
}

/// Signed plane equation dot(normal, position) + offset = 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryPlane {
    /// GeometryPlane normal; need not be normalized.
    pub normal: [f64; 3],
    /// Constant term in the signed plane equation.
    pub offset: f64,
}

/// Relationship to the plane's negative and positive half spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryPlaneIntersection {
    /// Wholly in the negative half space.
    Negative,
    /// Touches or crosses the plane.
    Intersecting,
    /// Wholly in the positive half space.
    Positive,
}

/// One closed interval occupied by a shape, before clipping to the query range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryRayInterval {
    /// Parameter at the first boundary.
    pub enter: f64,
    /// Parameter at the last boundary.
    pub exit: f64,
    /// Zero-based primitive ordinal in authored order.
    pub part: u32,
}

/// First union boundary in the requested interval; ties retain authored order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryRayHit {
    /// GeometryRay parameter at the selected boundary.
    pub distance: f64,
    /// Zero-based primitive ordinal in authored order.
    pub part: u32,
}

/// Common geometric contract, independent of assets, entities and rendering.
pub trait GeometryBounds {
    /// Append occupied ray intervals, preserving the input ray parameter.
    fn ray_intervals(&self, ray: &GeometryRay, intervals: &mut Vec<GeometryRayInterval>);

    /// Minimum and maximum signed plane values; None denotes empty geometry.
    fn plane_interval(&self, plane: &GeometryPlane) -> Option<[f64; 2]>;

    /// Find the nearest surface, including the exit when the query starts inside.
    fn ray_intersection(&self, ray: &GeometryRay, near: f64, far: f64) -> Option<GeometryRayHit> {
        let mut intervals = Vec::new();
        self.ray_intersection_with_scratch(ray, near, far, &mut intervals)
    }

    /// Reuse query-owned storage across candidates instead of allocating per shape.
    fn ray_intersection_with_scratch(
        &self,
        ray: &GeometryRay,
        near: f64,
        far: f64,
        intervals: &mut Vec<GeometryRayInterval>,
    ) -> Option<GeometryRayHit> {
        if !finite3(ray.origin)
            || !finite3(ray.direction)
            || dot(ray.direction, ray.direction) == 0.0
            || !near.is_finite()
            || !far.is_finite()
            || near < 0.0
            || far < near
        {
            return None;
        }
        intervals.clear();
        self.ray_intervals(ray, intervals);
        intervals.sort_unstable_by(|a, b| a.enter.total_cmp(&b.enter).then(a.part.cmp(&b.part)));
        let mut current = *intervals.first()?;
        let mut exit_part = current.part;
        for &next in &intervals[1..] {
            if next.enter <= current.exit {
                if next.exit > current.exit {
                    current.exit = next.exit;
                    exit_part = next.part;
                } else if next.exit == current.exit {
                    exit_part = exit_part.min(next.part);
                }
                continue;
            }
            if let Some(hit) = interval_hit(current, exit_part, near, far) {
                return Some(hit);
            }
            current = next;
            exit_part = next.part;
        }
        interval_hit(current, exit_part, near, far)
    }

    /// Classify against a plane. Empty shapes have no intersection.
    fn plane_intersection(&self, plane: &GeometryPlane) -> Option<GeometryPlaneIntersection> {
        let [min, max] = self.plane_interval(plane)?;
        Some(if min > 0.0 {
            GeometryPlaneIntersection::Positive
        } else if max < 0.0 {
            GeometryPlaneIntersection::Negative
        } else {
            GeometryPlaneIntersection::Intersecting
        })
    }

    /// Conservative half-space rejection, suitable for an inward-facing frustum.
    fn intersects_frustum(&self, planes: &[GeometryPlane; 6]) -> bool {
        for plane in planes {
            let Some([min, max]) = self.plane_interval(plane) else {
                return false;
            };
            // Keep boundaries visible across CPU f64 and renderer f32 math.
            let tolerance = 16.0
                * f64::from(f32::EPSILON)
                * (min.abs().max(max.abs()) + plane.offset.abs() + 1.0);
            if max < -tolerance {
                return false;
            }
        }
        true
    }

    /// Axis-aligned enclosure in the shape's evaluated coordinate system.
    fn bounds(&self) -> Option<[[f64; 3]; 2]> {
        let mut bounds = [[0.0; 3]; 2];
        for axis in 0..3 {
            let mut normal = [0.0; 3];
            normal[axis] = 1.0;
            let interval = self.plane_interval(&GeometryPlane {
                normal,
                offset: 0.0,
            })?;
            bounds[0][axis] = interval[0];
            bounds[1][axis] = interval[1];
        }
        Some(bounds)
    }
}

/// Extract inward-facing planes from a column-major GL view-projection matrix.
pub fn frustum_planes(matrix: [f32; 16]) -> [GeometryPlane; 6] {
    let mut planes = [GeometryPlane {
        normal: [0.0; 3],
        offset: 0.0,
    }; 6];
    for axis in 0..3 {
        for side in 0..2 {
            let sign = if side == 0 {
                1.0
            } else {
                -1.0
            };
            planes[axis * 2 + side] = GeometryPlane {
                normal: [
                    f64::from(matrix[3]) + sign * f64::from(matrix[axis]),
                    f64::from(matrix[7]) + sign * f64::from(matrix[4 + axis]),
                    f64::from(matrix[11]) + sign * f64::from(matrix[8 + axis]),
                ],
                offset: f64::from(matrix[15]) + sign * f64::from(matrix[12 + axis]),
            };
        }
    }
    planes
}

#[inline]
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub(crate) fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
pub(crate) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
pub(crate) fn finite3(a: [f64; 3]) -> bool {
    a[0].is_finite() && a[1].is_finite() && a[2].is_finite()
}

fn interval_hit(
    interval: GeometryRayInterval,
    exit_part: u32,
    near: f64,
    far: f64,
) -> Option<GeometryRayHit> {
    let hit = if interval.enter >= near {
        GeometryRayHit {
            distance: interval.enter,
            part: interval.part,
        }
    } else {
        GeometryRayHit {
            distance: interval.exit,
            part: exit_part,
        }
    };
    if hit.distance >= near && hit.distance <= far {
        Some(hit)
    } else {
        None
    }
}
