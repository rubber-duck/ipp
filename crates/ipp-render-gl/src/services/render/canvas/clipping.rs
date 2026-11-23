//! A convex drawing-space clip normally needs one submission. A deeply
//! nested set of rotated rectangles can have arbitrarily many edges. Partition
//! that polygon into disjoint fans that fit the fragment uniform block.

pub(super) const MAX_PLANES: usize = 16;

pub(super) fn batches(points: &[[f32; 2]]) -> Vec<Vec<[f32; 4]>> {
    if points.len() < 3 {
        return Vec::new();
    }
    let origin = points[0];
    let area: f32 = points[1..]
        .windows(2)
        .map(|w| {
            (w[0][0] - origin[0]) * (w[1][1] - origin[1])
                - (w[0][1] - origin[1]) * (w[1][0] - origin[0])
        })
        .sum();
    if area == 0.0 {
        return Vec::new();
    }
    let winding = area.signum();
    let mut result = Vec::new();
    let mut first = 1;
    while first + 1 < points.len() {
        let last = (first + MAX_PLANES - 2).min(points.len() - 1);
        let vertices: Vec<_> = std::iter::once(&origin)
            .chain(&points[first..=last])
            .collect();
        let mut planes = Vec::new();
        for (i, (&a, &b)) in vertices
            .iter()
            .zip(vertices.iter().cycle().skip(1))
            .enumerate()
        {
            let n = [(a[1] - b[1]) * winding, (b[0] - a[0]) * winding];
            if n == [0.; 2] {
                continue;
            }
            // Adjacent fans own a shared edge once. The reversed edge has
            // the same coefficients with opposite signs, including offset.
            planes.push([
                n[0],
                n[1],
                (b[0] * a[1] - a[0] * b[1]) * -winding,
                if first > 1 && i == 0 {
                    -1.
                } else {
                    1.
                },
            ]);
        }
        if planes.len() >= 3 {
            result.push(planes);
        }
        first = last;
    }
    result
}

pub(super) fn child_polygon(
    polygon: Option<Vec<[f32; 2]>>,
    mapping: &super::scene::CanvasMapping,
) -> Option<Vec<[f32; 2]>> {
    polygon.map(|mut points| {
        for p in &mut points {
            for (i, v) in p.iter_mut().enumerate() {
                *v = ((f64::from(*v) - mapping.origin[i]) / mapping.scale[i]) as f32;
            }
        }
        points
    })
}

/// Intersect a transformed scope with the polygon inherited through a Surface slot.
pub(super) fn in_space(
    space: Option<&ipp_core::systems::canvas::CanvasSpace>,
    rect: [f32; 4],
    inherited: Option<&[[f32; 2]]>,
) -> Option<Vec<[f32; 2]>> {
    let Some(space) = space else {
        return inherited.map(|p| p.to_vec());
    };
    let mut points = space.visible_polygon(rect);
    let Some(inherited) = inherited else {
        return Some(points);
    };
    let outer: Vec<_> = inherited.iter().map(|&p| space.inverse.point(p)).collect();
    let area: f32 = outer
        .iter()
        .zip(outer.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum();
    if area == 0.0 {
        return Some(Vec::new());
    }
    for (a, b) in outer.iter().zip(outer.iter().cycle().skip(1)) {
        let side = |p: [f32; 2]| {
            ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])) * area.signum()
        };
        let Some(mut start) = points.last().copied() else {
            break;
        };
        let mut clipped = Vec::new();
        for &end in &points {
            let (s, e) = (side(start), side(end));
            if (s < 0. && e > 0.) || (s > 0. && e < 0.) {
                clipped.push(std::array::from_fn(|i| {
                    start[i] + (end[i] - start[i]) * s / (s - e)
                }));
            }
            if e >= 0. {
                clipped.push(end)
            }
            start = end;
        }
        points = clipped;
    }
    Some(points)
}
