//! Camera-dependent index ordering for retained transparent Plot meshes.
//!
//! Centroid depth is a stable tie-breaker, not visibility: a long wall may extend
//! in front of a cap whose centre is nearer. Projected overlap establishes the
//! actual far-to-near constraints, without changing vertices or source geometry.

use std::{cmp::Reverse, collections::BinaryHeap};

#[derive(Default)]
pub(super) struct TriangleOrder {
    projection: Option<[f32; 16]>,
    projected: Vec<ProjectedTriangle>,
    sweep: Vec<usize>,
    active: Vec<usize>,
    ranked: Vec<usize>,
    rank: Vec<usize>,
    edges: Vec<Vec<usize>>,
    incoming: Vec<usize>,
    emitted: Vec<bool>,
    ready: BinaryHeap<Reverse<(usize, usize)>>,
    pub indices: Vec<u16>,
    next: Vec<u16>,
}

#[derive(Default)]
struct ProjectedTriangle {
    points: [[f64; 3]; 3],
    bounds: [f64; 4],
    depth_plane: [f64; 3],
    depth: f64,
    area: f64,
}

impl ProjectedTriangle {
    fn new(positions: &[[f32; 3]], indices: &[u16], matrix: [f32; 16]) -> Self {
        let mut triangle = Self::default();
        for (point, &index) in triangle.points.iter_mut().zip(indices) {
            let position = positions[usize::from(index)];
            let clip: [f64; 4] = std::array::from_fn(|axis| {
                f64::from(matrix[12 + axis])
                    + (0..3)
                        .map(|i| f64::from(matrix[i * 4 + axis]) * f64::from(position[i]))
                        .sum::<f64>()
            });
            // Near-plane crossing needs clipped triangles. Retain deterministic
            // depth order there instead of admitting invalid projected bounds.
            if clip[3] <= 1e-8 || !clip.iter().all(|value| value.is_finite()) {
                return Self::default();
            }
            *point = std::array::from_fn(|i| clip[i] / clip[3]);
        }
        let [a, b, c] = triangle.points;
        triangle.depth = (a[2] + b[2] + c[2]) / 3.0;
        triangle.area = cross(a, b, c);
        if triangle.area.abs() < 1e-14 {
            triangle.area = 0.0;
            return triangle;
        }
        let dz_dx = ((b[2] - a[2]) * (c[1] - a[1]) - (c[2] - a[2]) * (b[1] - a[1])) / triangle.area;
        let dz_dy = ((b[0] - a[0]) * (c[2] - a[2]) - (c[0] - a[0]) * (b[2] - a[2])) / triangle.area;
        triangle.depth_plane = [dz_dx, dz_dy, a[2] - dz_dx * a[0] - dz_dy * a[1]];
        triangle.bounds = [
            a[0].min(b[0]).min(c[0]),
            a[1].min(b[1]).min(c[1]),
            a[0].max(b[0]).max(c[0]),
            a[1].max(b[1]).max(c[1]),
        ];
        triangle
    }
}

impl TriangleOrder {
    pub fn update(
        &mut self,
        positions: &[[f32; 3]],
        original: &[u16],
        projection: Option<[f32; 16]>,
    ) -> bool {
        if self.projection == projection {
            return false;
        }
        self.projection = projection;
        self.next.clear();
        if let Some(matrix) = projection {
            self.projected.clear();
            self.projected.extend(
                original
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|indices| ProjectedTriangle::new(positions, indices, matrix)),
            );
            self.sort(original);
        } else {
            self.next.extend_from_slice(original);
        }
        let previous = if self.indices.is_empty() {
            original
        } else {
            &self.indices
        };
        let changed = previous != self.next;
        std::mem::swap(&mut self.indices, &mut self.next);
        changed
    }

    fn sort(&mut self, original: &[u16]) {
        let count = self.projected.len();
        self.ranked.clear();
        self.ranked.extend(0..count);
        self.ranked.sort_unstable_by(|&a, &b| {
            self.projected[b]
                .depth
                .total_cmp(&self.projected[a].depth)
                .then(a.cmp(&b))
        });
        self.rank.resize(count, 0);
        self.edges.resize_with(count, Vec::new);
        self.incoming.clear();
        self.incoming.resize(count, 0);
        self.emitted.clear();
        self.emitted.resize(count, false);
        for (rank, &index) in self.ranked.iter().enumerate() {
            self.rank[index] = rank;
            self.edges[index].clear();
        }
        self.sweep.clear();
        self.sweep
            .extend((0..count).filter(|&i| self.projected[i].area != 0.0));
        self.sweep.sort_unstable_by(|&a, &b| {
            self.projected[a].bounds[0]
                .total_cmp(&self.projected[b].bounds[0])
                .then(a.cmp(&b))
        });
        self.active.clear();
        for &current in &self.sweep {
            let a = &self.projected[current];
            self.active
                .retain(|&other| self.projected[other].bounds[2] > a.bounds[0]);
            for &other in &self.active {
                let b = &self.projected[other];
                if a.bounds[1] >= b.bounds[3] || b.bounds[1] >= a.bounds[3] {
                    continue;
                }
                if let Some(a_farther) = overlap_order(a, b) {
                    let (far, near) = if a_farther {
                        (current, other)
                    } else {
                        (other, current)
                    };
                    self.edges[far].push(near);
                    self.incoming[near] += 1;
                }
            }
            self.active.push(current);
        }

        self.ready.clear();
        for index in 0..count {
            if self.incoming[index] == 0 {
                self.ready.push(Reverse((self.rank[index], index)));
            }
        }
        let mut fallback = 0;
        for _ in 0..count {
            let index = if let Some(Reverse((_, index))) = self.ready.pop() {
                index
            } else {
                // Intersecting/cyclic surfaces cannot have a single triangle
                // order. Break only that cycle in stable centroid order.
                while self.emitted[self.ranked[fallback]] {
                    fallback += 1;
                }
                self.ranked[fallback]
            };
            self.emitted[index] = true;
            self.next
                .extend_from_slice(&original[index * 3..index * 3 + 3]);
            for &near in &self.edges[index] {
                self.incoming[near] -= 1;
                if self.incoming[near] == 0 && !self.emitted[near] {
                    self.ready.push(Reverse((self.rank[near], near)));
                }
            }
        }
    }
}

fn cross(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Clip in screen space; NDC depth is affine over each projected triangle.
/// Shared edges and coplanar faces impose no constraint. A genuine crossing
/// remains on the centroid fallback because its geometry needs splitting.
fn overlap_order(a: &ProjectedTriangle, b: &ProjectedTriangle) -> Option<bool> {
    let mut polygon = [[0.0; 3]; 8];
    polygon[..3].copy_from_slice(&a.points);
    let mut length = 3;
    let sign = b.area.signum();
    for edge in 0..3 {
        let first = b.points[edge];
        let second = b.points[(edge + 1) % 3];
        let input = polygon;
        let mut output_length = 0;
        let mut previous = input[length - 1];
        let mut previous_side = cross(first, second, previous) * sign;
        for &point in &input[..length] {
            let side = cross(first, second, point) * sign;
            if (side >= 0.0) != (previous_side >= 0.0) {
                let t = previous_side / (previous_side - side);
                polygon[output_length] =
                    std::array::from_fn(|i| previous[i] + t * (point[i] - previous[i]));
                output_length += 1;
            }
            if side >= 0.0 {
                polygon[output_length] = point;
                output_length += 1;
            }
            previous = point;
            previous_side = side;
        }
        length = output_length;
        if length < 3 {
            return None;
        }
    }
    let area: f64 = (1..length - 1)
        .map(|i| cross(polygon[0], polygon[i], polygon[i + 1]))
        .sum();
    if area.abs() < 1e-14 {
        return None;
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for point in &polygon[..length] {
        let delta = point[2]
            - (b.depth_plane[0] * point[0] + b.depth_plane[1] * point[1] + b.depth_plane[2]);
        min = min.min(delta);
        max = max.max(delta);
    }
    const TOLERANCE: f64 = 1e-7;
    if min >= -TOLERANCE && max > TOLERANCE {
        Some(true)
    } else if max <= TOLERANCE && min < -TOLERANCE {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "triangle_order_tests.rs"]
mod tests;
