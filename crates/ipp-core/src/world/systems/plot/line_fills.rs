//! Areas and bands are the union of source intervals, in original row order.
//!
//! Both boundaries use the line's monotone interpolation. Missing lower values
//! split only the fill. Repeated X has zero area; reversing X stays valid. Crossing
//! boundaries split at their intersection before triangulation, so a baseline
//! crossing cannot fill the exterior of a bow tie. All same-colour triangles in a
//! run blend once, including overlaps. Different colours/series keep painter order.
//! Preparation and clipping happen only when Plot inputs change.

use super::{
    plots_2d::{Sample, chord_distance, monotone_tangents, split_cubic},
    *,
};
use crate::{ErrorReason, services::asset_management::formats::drawing::FillRule};

type Triangle = [[f32; 2]; 3];

pub(super) fn prepare(
    geometry: &mut PlotPreparedGeometry,
    run: &[Sample],
    chart: &PlotLine2d,
    mapping: PlotFrameMapping2d,
) -> Result<(), ErrorReason> {
    if run.len() < 2 {
        return Ok(());
    }

    let upper: Vec<_> = run.iter().map(|s| mapping.map(s.point)).collect();
    let lower: Vec<_> = run
        .iter()
        .map(|s| s.lower.map(|y| mapping.map([s.point[0], y])))
        .collect();
    if upper
        .iter()
        .flatten()
        .chain(lower.iter().flatten().flatten())
        .any(|v| !v.is_finite())
    {
        return Err(ErrorReason::InvalidValue);
    }

    let upper_tangents = monotone_tangents(&upper);
    let mut lower_tangents = vec![None; run.len()];
    let mut start = 0;
    while start < lower.len() {
        if lower[start].is_none() {
            start += 1;
            continue;
        }
        let end = lower[start..]
            .iter()
            .position(Option::is_none)
            .map_or(lower.len(), |n| start + n);
        let points: Vec<_> = lower[start..end].iter().flatten().copied().collect();
        lower_tangents[start..end].copy_from_slice(&monotone_tangents(&points));
        start = end;
    }

    let mut triangles = Vec::new();
    let mut color = run[0].color;
    for i in 0..run.len() - 1 {
        if run[i].color != color || lower[i].is_none() || lower[i + 1].is_none() {
            emit(
                geometry,
                std::mem::take(&mut triangles),
                color,
                chart.fill_opacity,
            );
            color = run[i].color;
        }
        let (Some(a), Some(b)) = (lower[i], lower[i + 1]) else {
            continue;
        };
        let upper_curve = cubic(
            upper[i],
            upper[i + 1],
            upper_tangents[i],
            upper_tangents[i + 1],
            chart.interpolation,
        );
        let lower_curve = cubic(
            a,
            b,
            lower_tangents[i],
            lower_tangents[i + 1],
            chart.interpolation,
        );
        subdivide(
            upper_curve,
            lower_curve,
            mapping.rect,
            &mut triangles,
            &mut geometry.hits,
            run[i].row,
            0,
        );
    }

    emit(geometry, triangles, color, chart.fill_opacity);
    Ok(())
}

fn cubic(
    a: [f32; 2],
    b: [f32; 2],
    ma: Option<f32>,
    mb: Option<f32>,
    interpolation: u32,
) -> [[f32; 2]; 4] {
    if interpolation == 1
        && b[0] > a[0]
        && let (Some(ma), Some(mb)) = (ma, mb)
    {
        let dx = (b[0] - a[0]) / 3.0;
        [
            a,
            [a[0] + dx, a[1] + ma * dx],
            [b[0] - dx, b[1] - mb * dx],
            b,
        ]
    } else {
        // Keep X linear in the subdivision parameter even when only one
        // boundary has valid smooth tangents (for example beside a lower gap).
        [
            a,
            std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) / 3.0),
            std::array::from_fn(|axis| b[axis] - (b[axis] - a[axis]) / 3.0),
            b,
        ]
    }
}

#[allow(clippy::too_many_arguments)]
fn subdivide(
    upper: [[f32; 2]; 4],
    lower: [[f32; 2]; 4],
    rect: [f32; 4],
    triangles: &mut Vec<Triangle>,
    hits: &mut Vec<PlotHit>,
    row: PlotRowIdentity,
    depth: u32,
) {
    if (0..2).any(|axis| {
        upper.iter().chain(&lower).all(|p| p[axis] < rect[axis])
            || upper.iter().chain(&lower).all(|p| p[axis] > rect[axis + 2])
    }) {
        return;
    }

    let error = [upper, lower]
        .iter()
        .map(|p| chord_distance(p[1], p[0], p[3]).max(chord_distance(p[2], p[0], p[3])))
        .fold(0.0_f32, f32::max);
    if error > 0.2 && depth < 16 {
        for (u, l) in split_cubic(upper).into_iter().zip(split_cubic(lower)) {
            subdivide(u, l, rect, triangles, hits, row, depth + 1);
        }
        return;
    }

    if upper[0][0] == upper[3][0] {
        return;
    }

    let before = triangles.len();
    let da = upper[0][1] - lower[0][1];
    let db = upper[3][1] - lower[3][1];
    if da * db < 0.0 {
        let t = da / (da - db);
        let crossing =
            std::array::from_fn(|axis| upper[0][axis] + t * (upper[3][axis] - upper[0][axis]));
        clipped_triangle([upper[0], crossing, lower[0]], rect, triangles);
        clipped_triangle([crossing, upper[3], lower[3]], rect, triangles);
    } else {
        clipped_triangle([upper[0], upper[3], lower[3]], rect, triangles);
        clipped_triangle([upper[0], lower[3], lower[0]], rect, triangles);
    }
    if triangles.len() > before {
        hits.push(PlotHit {
            row,
            shape: PlotHitShape::Band {
                ends: [
                    [upper[0][0], upper[0][1], lower[0][1]],
                    [upper[3][0], upper[3][1], lower[3][1]],
                ],
                clip: rect,
            },
        });
    }
}

fn clipped_triangle(triangle: Triangle, rect: [f32; 4], result: &mut Vec<Triangle>) {
    let mut polygon = triangle.to_vec();
    for (edge, &boundary) in rect.iter().enumerate() {
        let axis = edge % 2;
        let inside = |p: [f32; 2]| {
            if edge < 2 {
                p[axis] >= boundary
            } else {
                p[axis] <= boundary
            }
        };
        let input = std::mem::take(&mut polygon);
        if input.is_empty() {
            return;
        }
        let mut previous = *input.last().expect("nonempty polygon");
        for point in input {
            if inside(previous) != inside(point) {
                let t = (boundary - previous[axis]) / (point[axis] - previous[axis]);
                polygon.push(std::array::from_fn(|a| {
                    previous[a] + t * (point[a] - previous[a])
                }));
            }
            if inside(point) {
                polygon.push(point);
            }
            previous = point;
        }
    }
    for i in 1..polygon.len().saturating_sub(1) {
        let mut p = [polygon[0], polygon[i], polygon[i + 1]];
        let cross = (f64::from(p[1][0]) - f64::from(p[0][0]))
            * (f64::from(p[2][1]) - f64::from(p[0][1]))
            - (f64::from(p[1][1]) - f64::from(p[0][1])) * (f64::from(p[2][0]) - f64::from(p[0][0]));
        if cross == 0.0 {
            continue;
        }
        if cross < 0.0 {
            p.swap(1, 2);
        }
        result.push(p);
    }
}

fn emit(
    geometry: &mut PlotPreparedGeometry,
    triangles: Vec<Triangle>,
    mut color: [f32; 4],
    opacity: f32,
) {
    if triangles.is_empty() {
        return;
    }

    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for p in triangles.iter().flatten() {
        for axis in 0..2 {
            bounds[axis] = bounds[axis].min(p[axis]);
            bounds[axis + 2] = bounds[axis + 2].max(p[axis]);
        }
    }
    color[3] *= opacity;
    geometry.canvas.push(
        PlotPrimitive::path(
            geometry.canvas.len() as u32,
            PlotPath {
                bounds,
                geometry: PlotPathGeometry::Fill(triangles.into()),
                fill_rule: FillRule::NonZero,
            },
            color,
        )
        .with_data_paint(),
    );
}

#[cfg(test)]
#[path = "line_fills_tests.rs"]
mod tests;
