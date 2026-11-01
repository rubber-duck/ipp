//! Retained filled-triangle unions with a stackless spatial hierarchy.
//!
//! Each node is two RGBA32I texels: bit-exact float bounds, then integer
//! [escape, first_triangle, triangle_count, 0]. Preorder traversal descends by
//! incrementing the node index and skips a subtree with its escape index. Leaves
//! hold at most four spatially sorted triangles, each in two bit-exact texels.
//! The second texel also carries an exterior edge mask; exactly shared opposite edges disappear from boundary queries.
//! Only overlapping pixel footprints visit leaves, independent of source order.
//! A packed two-bit grid certifies uniform interiors/exteriors with a one-cell
//! guard. It is an exact rejection index, not a raster replacement or sample LOD.
//! Storage and build work are bounded O(N) and O(N log N); no per-frame packing.

use super::paths::{
    BAND_HEADER_TEXELS, SurfaceBandTexels, SurfaceCurveTexels, SurfacePathAtlas, SurfacePathTexels,
};
use crate::SurfacePathDescriptor;

type Triangle = [[f32; 2]; 3];

pub(super) fn pack_fill(triangles: &[Triangle], limit: u32) -> SurfacePathAtlas {
    let mut order: Vec<_> = (0..triangles.len()).collect();
    let mut nodes = Vec::new();
    let mut data = Vec::new();
    let boundaries = boundary_masks(triangles);
    build(triangles, &boundaries, &mut order, &mut nodes, &mut data);
    let node_count = nodes.len() / 2;
    for node in nodes.as_chunks_mut::<2>().0 {
        node[1][1] += nodes_data_offset(node_count);
    }

    let mut bands = candidate_index(&nodes, triangles.len(), limit);
    let mut points = vec![[node_count as i32, triangles.len() as i32, 0, 0]];
    points.extend(nodes);
    points.extend(data);
    uniform_grid(&mut points, &mut bands, limit);

    let bands = if bands.iter().all(|&v| v <= u16::MAX as u32) {
        SurfaceBandTexels::Uint16(bands.into_iter().map(|v| v as u16).collect())
    } else {
        SurfaceBandTexels::Uint32(bands)
    };

    SurfacePathAtlas {
        descriptors: vec![SurfacePathDescriptor::new([0, points.len() as u32], 0)],
        texels: SurfacePathTexels {
            curves: SurfaceCurveTexels::Int32(points),
            curve_scale: 1.0,
            bands,
        },
    }
}

// A direct index skips interior tree nodes for narrow local queries. Wide lists
// still use the spatial tree, so disconnected 2D geometry never degenerates to
// scanning all candidates on one axis. Each level includes a full adjacent-bin
// margin; choosing width >= footprint preserves minified and painted boundaries.
fn candidate_index(nodes: &[[i32; 4]], triangles: usize, limit: u32) -> Vec<u32> {
    let bounds = nodes[0].map(|v| f32::from_bits(v as u32));
    let leaves: Vec<_> = nodes
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, node)| node[1][2] > 0)
        .map(|(i, node)| (i as u32, node[0].map(|v| f32::from_bits(v as u32))))
        .collect();
    let overlap = |axis: usize| {
        leaves
            .iter()
            .map(|(_, b)| {
                f64::from(b[axis + 2] - b[axis])
                    / f64::from(bounds[axis + 2] - bounds[axis]).max(f64::MIN_POSITIVE)
            })
            .sum::<f64>()
    };
    let axis = usize::from(overlap(1) < overlap(0));
    let budget = (triangles.saturating_mul(32) + BAND_HEADER_TEXELS as usize)
        .min((limit as usize).saturating_mul(limit as usize));
    let mut bins = leaves
        .len()
        .next_power_of_two()
        .saturating_mul(2)
        .clamp(1, 4096);
    loop {
        if let Some(values) = index_levels(&leaves, bounds, axis, bins, budget) {
            break values;
        }
        if bins == 1 {
            break index_levels(&leaves, bounds, axis, 1, usize::MAX).expect("complete leaf list");
        }
        bins /= 2;
    }
}

// Classify cells only when their entire three-by-three neighbourhood is free
// of possible union boundaries. A pixel footprint up to one cell in each axis
// then has the same membership as the centre, including anisotropic views.
// Unknown cells always use the exact triangle query; buried/partial edges only
// reduce the number of certified cells. Eight cells fit in one Uint16 texel.
fn uniform_grid(points: &mut [[i32; 4]], bands: &mut Vec<u32>, limit: u32) {
    let count = points[0][1] as usize;
    let budget = (count.saturating_mul(32) + BAND_HEADER_TEXELS as usize)
        .min((limit as usize).saturating_mul(limit as usize));
    let mut size = 128;
    while size >= 8
        && (size * size / 8 > budget.saturating_sub(bands.len())
            || size * size > count.saturating_mul(8).max(64))
    {
        size /= 2;
    }
    let bounds = float_data(points[1]);
    let width = [bounds[2] - bounds[0], bounds[3] - bounds[1]];
    if size < 8 || width.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return;
    }

    let mut unknown = vec![false; size * size];
    let first = nodes_data_offset(points[0][0] as usize) as usize;
    for triangle in points[first..].as_chunks::<2>().0 {
        let ab = float_data(triangle[0]);
        let c = float_data(triangle[1]);
        let vertices = [[ab[0], ab[1]], [ab[2], ab[3]], [c[0], c[1]]];
        for edge in 0..3 {
            if triangle[1][2] & (1 << edge) == 0 {
                continue;
            }
            let map = |p: [f64; 2]| {
                std::array::from_fn(|axis| (p[axis] - bounds[axis]) / width[axis] * size as f64)
            };
            mark_boundary_cells(
                &mut unknown,
                size,
                map(vertices[edge]),
                map(vertices[(edge + 1) % 3]),
            );
        }
    }

    let offset = bands.len();
    bands.resize(offset + size * size / 8, 0);
    for y in 1..size - 1 {
        for x in 1..size - 1 {
            let index = y * size + x;
            if unknown[index] {
                continue;
            }
            let p = [
                bounds[0] + (x as f64 + 0.5) * width[0] / size as f64,
                bounds[1] + (y as f64 + 0.5) * width[1] / size as f64,
            ];
            let value = if contains_point(points, p) {
                1
            } else {
                2
            };
            bands[offset + index / 8] |= value << (index % 8 * 2);
        }
    }
    points[0][2] = size as i32;
    points[0][3] = offset as i32;
}

fn float_data(data: [i32; 4]) -> [f64; 4] {
    data.map(|v| f64::from(f32::from_bits(v as u32)))
}

fn mark_boundary_cells(unknown: &mut [bool], size: usize, a: [f64; 2], b: [f64; 2]) {
    // Expand roundoff at closed cell boundaries before floor. Work per crossed
    // row, not the edge's entire rectangle, then include all neighbouring cells.
    const EPSILON: f64 = 1e-5;
    let cell = |v: f64| (v.floor() as usize).min(size - 1);
    let first = cell(a[1].min(b[1]) - EPSILON);
    let last = cell(a[1].max(b[1]) + EPSILON);
    for y in first..=last {
        let (low, high) = if (a[1] - b[1]).abs() <= f64::EPSILON {
            (a[0].min(b[0]), a[0].max(b[0]))
        } else {
            let at = |v: f64| {
                let t = ((v - a[1]) / (b[1] - a[1])).clamp(0.0, 1.0);
                a[0] + t * (b[0] - a[0])
            };
            let start = at(y as f64 - EPSILON);
            let end = at(y as f64 + 1.0 + EPSILON);
            (start.min(end), start.max(end))
        };
        let x0 = cell(low - EPSILON).saturating_sub(1);
        let x1 = (cell(high + EPSILON) + 1).min(size - 1);
        for row in y.saturating_sub(1)..=(y + 1).min(size - 1) {
            unknown[row * size + x0..=row * size + x1].fill(true);
        }
    }
}

fn contains_point(points: &[[i32; 4]], p: [f64; 2]) -> bool {
    let mut node = 0;
    while node < points[0][0] as usize {
        let bounds = float_data(points[1 + node * 2]);
        let meta = points[2 + node * 2];
        if (0..2).any(|axis| p[axis] < bounds[axis] || p[axis] > bounds[axis + 2]) {
            node = meta[0] as usize;
            continue;
        }
        for i in 0..meta[2] as usize {
            let ab = float_data(points[meta[1] as usize + i * 2]);
            let c = float_data(points[meta[1] as usize + i * 2 + 1]);
            let vertices = [[ab[0], ab[1]], [ab[2], ab[3]], [c[0], c[1]]];
            let [a, b, c] = vertices;
            if (b[0] - a[0]) * (c[1] - a[1]) <= (b[1] - a[1]) * (c[0] - a[0]) {
                continue;
            }
            if (0..3).all(|edge| {
                let a = vertices[edge];
                let b = vertices[(edge + 1) % 3];
                (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= 0.0
            }) {
                return true;
            }
        }
        node += 1;
    }
    false
}

fn index_levels(
    leaves: &[(u32, [f32; 4])],
    bounds: [f32; 4],
    axis: usize,
    mut bins: usize,
    budget: usize,
) -> Option<Vec<u32>> {
    let mut values = vec![0; BAND_HEADER_TEXELS as usize];
    values[0] = axis as u32;
    let low = f64::from(bounds[axis]);
    let extent = (f64::from(bounds[axis + 2]) - low).max(f64::MIN_POSITIVE);
    let mut level = 0;
    loop {
        let header = values.len();
        if header.checked_add(bins * 2)? > budget {
            return None;
        }
        values[2 + level * 2] = bins as u32;
        values[3 + level * 2] = header as u32;
        values.resize(header + bins * 2, 0);
        let width = extent / bins as f64;
        let range = |b: [f32; 4]| {
            let a = (((f64::from(b[axis]) - low) / width).ceil() as i64 - 2)
                .clamp(0, bins as i64 - 1) as usize;
            let b = (((f64::from(b[axis + 2]) - low) / width).floor() as i64 + 1)
                .clamp(0, bins as i64 - 1) as usize;
            a..=b
        };
        for &(_, b) in leaves {
            for bin in range(b) {
                values[header + bin * 2 + 1] += 1;
            }
        }
        let mut end = values.len();
        for bin in 0..bins {
            values[header + bin * 2] = end as u32;
            end = end.checked_add(values[header + bin * 2 + 1] as usize)?;
            if end > budget {
                return None;
            }
        }
        values.resize(end, 0);
        let mut offsets = vec![0; bins];
        for &(index, b) in leaves {
            for bin in range(b) {
                let start = values[header + bin * 2] as usize;
                values[start + offsets[bin]] = index;
                offsets[bin] += 1;
            }
        }
        level += 1;
        if bins == 1 {
            break;
        }
        bins /= 2;
    }

    values[1] = level as u32;
    Some(values)
}

// An oppositely directed shared edge has coverage on both sides. Removing it
// does not depend on row order or manifold input. Partial/coincident edges with
// different endpoints remain conservative candidates; the shader checks union
// membership before exposing them to authored paint.
fn boundary_masks(triangles: &[Triangle]) -> Vec<u8> {
    let key = |p: [f32; 2]| {
        p.map(|v| {
            if v == 0.0 {
                0
            } else {
                v.to_bits()
            }
        })
    };
    let mut edges = Vec::with_capacity(triangles.len() * 3);
    for (triangle, vertices) in triangles.iter().enumerate() {
        for edge in 0..3 {
            let a = key(vertices[edge]);
            let b = key(vertices[(edge + 1) % 3]);
            edges.push(((a.min(b), a.max(b)), a < b, triangle, edge));
        }
    }
    edges.sort_unstable_by_key(|edge| edge.0);
    let mut masks = vec![7; triangles.len()];
    let mut start = 0;
    while start < edges.len() {
        let end = start + edges[start..].partition_point(|edge| edge.0 == edges[start].0);
        let shared = edges[start..end]
            .iter()
            .any(|edge| edge.1 != edges[start].1);
        if shared {
            for &(_, _, triangle, edge) in &edges[start..end] {
                masks[triangle] &= !(1 << edge);
            }
        }

        start = end;
    }

    masks
}

fn nodes_data_offset(count: usize) -> i32 {
    (1 + count * 2) as i32
}

fn build(
    triangles: &[Triangle],
    boundaries: &[u8],
    order: &mut [usize],
    nodes: &mut Vec<[i32; 4]>,
    data: &mut Vec<[i32; 4]>,
) {
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for &i in order.iter() {
        for p in triangles[i] {
            for axis in 0..2 {
                bounds[axis] = bounds[axis].min(p[axis]);
                bounds[axis + 2] = bounds[axis + 2].max(p[axis]);
            }
        }
    }

    let node = nodes.len();
    nodes.push(bounds.map(|v| v.to_bits() as i32));
    nodes.push([0; 4]);
    if order.len() <= 4 {
        nodes[node + 1][1] = data.len() as i32;
        nodes[node + 1][2] = order.len() as i32;
        for &i in order.iter() {
            let p = triangles[i];
            data.push([p[0][0], p[0][1], p[1][0], p[1][1]].map(|v| v.to_bits() as i32));
            data.push([
                p[2][0].to_bits() as i32,
                p[2][1].to_bits() as i32,
                i32::from(boundaries[i]),
                0,
            ]);
        }
    } else {
        let middle = order.len() / 2;
        // Bounding-box extent chooses Y repeatedly for tall area triangles,
        // interleaving distant X intervals and making most nodes overlap.
        // Evaluate both balanced partitions by their actual covered area.
        let partition = |order: &mut [usize], axis: usize| {
            let center = |index: usize| {
                triangles[index]
                    .iter()
                    .map(|p| f64::from(p[axis]))
                    .sum::<f64>()
            };
            order.select_nth_unstable_by(middle, |&a, &b| {
                center(a).total_cmp(&center(b)).then(a.cmp(&b))
            });
            [&order[..middle], &order[middle..]]
                .into_iter()
                .map(|items| {
                    let mut b = [
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ];
                    for &i in items {
                        for p in triangles[i] {
                            for axis in 0..2 {
                                b[axis] = b[axis].min(f64::from(p[axis]));
                                b[axis + 2] = b[axis + 2].max(f64::from(p[axis]));
                            }
                        }
                    }
                    (b[2] - b[0]) * (b[3] - b[1]) * items.len() as f64
                })
                .sum::<f64>()
        };
        let x_cost = partition(order, 0);
        let y_cost = partition(order, 1);
        if x_cost <= y_cost {
            partition(order, 0);
        }
        let (left, right) = order.split_at_mut(middle);
        build(triangles, boundaries, left, nodes, data);
        build(triangles, boundaries, right, nodes, data);
    }

    nodes[node + 1][0] = (nodes.len() / 2) as i32;
}

#[cfg(test)]
#[path = "fills_tests.rs"]
mod tests;
