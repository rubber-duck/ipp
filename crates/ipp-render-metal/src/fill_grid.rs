//! Local-space coverage of a retained triangle union. The grid uses the same
//! range records as contour winding. A constant cell carries no triangle IDs.
use std::collections::HashSet;

type Point = [f32; 2];
type Triangle = [Point; 3];

fn key(p: Point) -> [u32; 2] {
    p.map(|v| {
        if v == 0.0 {
            0
        } else {
            v.to_bits()
        }
    })
}

fn cross(a: Point, b: Point, p: [f64; 2]) -> f64 {
    let [a, b] = [a, b].map(|v| v.map(f64::from));
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

pub(super) fn fill_grid(triangles: &[Triangle], bounds: [f32; 4]) -> Option<Vec<[f32; 4]>> {
    if !(16..=65_536).contains(&triangles.len()) {
        return None;
    }

    let side = triangles.len().isqrt().clamp(4, 64);
    let scale = [0, 1].map(|d| side as f32 / (bounds[d + 2] - bounds[d]));
    // Include rounding from the shader’s cell lookup and the inverse CPU mapping.
    let pad =
        [0, 1].map(|d| 16.0 * f32::EPSILON * (1.0 + bounds[d].abs().max(bounds[d + 2].abs())));
    if !(0..2).all(|d| scale[d].is_finite() && scale[d] > 0.0 && pad[d] * scale[d] < 0.25) {
        return None;
    }

    let cell = |v: f32, d: usize| (((v - bounds[d]) * scale[d]) as usize).min(side - 1);
    let mut bins = vec![Vec::new(); side * side];
    let mut directed = HashSet::with_capacity(triangles.len() * 3);
    // Limit both duplication per triangle and total temporary index storage.
    let reference_limit = (triangles.len() * 96).min(1 << 20);
    let mut references = 0;
    for (i, &[a, b, c]) in triangles.iter().enumerate() {
        // Plot publishes counterclockwise, nondegenerate fills. Preserve the
        // general shader path if another producer supplies a different winding.
        if cross(a, b, c.map(f64::from)) <= 0.0 {
            return None;
        }

        let first = [0, 1].map(|d| cell(a[d].min(b[d]).min(c[d]) - pad[d], d));
        let last = [0, 1].map(|d| cell(a[d].max(b[d]).max(c[d]) + pad[d], d));
        references += (last[0] - first[0] + 1) * (last[1] - first[1] + 1);
        if references > reference_limit {
            return None;
        }

        for y in first[1]..=last[1] {
            for x in first[0]..=last[0] {
                bins[y * side + x].push(i);
            }
        }

        for (start, end) in [(a, b), (b, c), (c, a)] {
            directed.insert((key(start), key(end)));
        }
    }

    // Matching reversed edges cover both sides. An unmatched edge can bound
    // the union, even when contours overlap or a fill has holes.
    let exterior: Vec<_> = triangles
        .iter()
        .map(|v| [0, 1, 2].map(|k| !directed.contains(&(key(v[(k + 1) % 3]), key(v[k])))))
        .collect();

    let mut data = vec![[0.0; 4]; 2 + side * side];
    let mut ids = Vec::new();
    for (cell_index, candidates) in bins.iter().enumerate() {
        let ij = [cell_index % side, cell_index / side];
        let low = [0, 1].map(|d| bounds[d] + ij[d] as f32 / scale[d] - pad[d]);
        let high = [0, 1].map(|d| bounds[d] + (ij[d] + 1) as f32 / scale[d] + pad[d]);
        let center = [0, 1].map(|d| (f64::from(low[d]) + f64::from(high[d])) * 0.5);
        // Any exterior edge overlapping the expanded cell keeps the exact
        // candidate tests. Else one interior point classifies the whole cell.
        let boundary = candidates.iter().any(|&i| {
            let v = triangles[i];
            (0..3).any(|k| {
                let (a, b) = (v[k], v[(k + 1) % 3]);
                exterior[i][k]
                    && (0..2).all(|d| a[d].min(b[d]) <= high[d] && a[d].max(b[d]) >= low[d])
            })
        });

        let offset = ids.len();
        let mut inside = false;
        if boundary {
            ids.extend(candidates.iter().map(|&i| i as f32));
        } else {
            inside = candidates.iter().any(|&i| {
                let [a, b, c] = triangles[i];
                cross(a, b, center) >= 0.0
                    && cross(b, c, center) >= 0.0
                    && cross(c, a, center) >= 0.0
            });
        }

        data[2 + cell_index] = [
            offset as f32,
            (ids.len() - offset) as f32,
            u8::from(inside) as f32,
            0.0,
        ];
    }

    for chunk in ids.chunks(4) {
        data.push(std::array::from_fn(|i| {
            chunk.get(i).copied().unwrap_or(0.0)
        }));
    }

    data[0] = [-(side as f32), bounds[1], scale[1], data.len() as f32];
    data[1] = [bounds[0], scale[0], bounds[2], bounds[3]];
    Some(data)
}

#[cfg(test)]
#[path = "fill_grid_tests.rs"]
mod tests;
