use super::{BoxBounds, GeometrySpatialBackend, GeometrySpatialIndex};
use crate::{EntityId, systems::geometry::GeometryPlane};

/// Reusable entity-slot masks, chunked into words for arbitrary query counts.
#[derive(Default)]
pub struct GeometryQueryResults {
    masks: Vec<u64>,
    identities: Vec<Option<EntityId>>,
    words: usize,
    queries: usize,
}

impl GeometryQueryResults {
    /// Missing geometry is an unknown candidate, never proof of exclusion.
    pub fn matches(&self, entity: EntityId, query: usize) -> bool {
        assert!(query < self.queries);
        let slot = entity.index() as usize;
        if self.identities.get(slot).copied().flatten() != Some(entity) {
            return true;
        }
        self.masks[slot * self.words + query / 64] & (1 << (query % 64)) != 0
    }
}

/// Caller-owned traversal stack; capacity is reused across batches and index backends.
#[derive(Default)]
pub struct GeometryQueryScratch {
    stack: Vec<(usize, u64)>,
}

fn intersects(bounds: BoxBounds, planes: &[GeometryPlane; 6]) -> bool {
    for plane in planes {
        let mut min = plane.offset;
        let mut max = plane.offset;
        for (axis, &normal) in plane.normal.iter().enumerate() {
            let a = bounds[0][axis] * normal;
            let b = bounds[1][axis] * normal;
            min += a.min(b);
            max += a.max(b);
        }
        let tolerance =
            16.0 * f64::from(f32::EPSILON) * (min.abs().max(max.abs()) + plane.offset.abs() + 1.0);
        if max < -tolerance {
            return false;
        }
    }
    true
}

fn filter(bounds: BoxBounds, frustums: &[[GeometryPlane; 6]], mut active: u64) -> u64 {
    let mut kept = 0;
    while active != 0 {
        let index = active.trailing_zeros() as usize;
        let bit = 1u64 << index;
        active &= active - 1;
        if intersects(bounds, &frustums[index]) {
            kept |= bit;
        }
    }
    kept
}

impl GeometrySpatialIndex {
    /// Batch inward-facing frustums using retained masks and traversal storage.
    pub fn query_frustums(
        &self,
        frustums: &[[GeometryPlane; 6]],
        results: &mut GeometryQueryResults,
        scratch: &mut GeometryQueryScratch,
    ) {
        scratch.stack.clear();
        let depth = self.active.len().max(1).next_power_of_two().ilog2() as usize + 1;
        if scratch.stack.capacity() < depth {
            scratch.stack.reserve(depth);
        }
        results.queries = frustums.len();
        results.words = frustums.len().div_ceil(64);
        results.masks.resize(self.rows.len() * results.words, 0);
        results.masks.fill(0);
        results.identities.clear();
        results
            .identities
            .extend(self.rows.iter().map(|row| row.map(|row| row.entity)));
        for (word, queries) in frustums.chunks(64).enumerate() {
            let all = u64::MAX >> (64 - queries.len());
            for &slot in &self.unknown {
                results.masks[slot * results.words + word] = all;
            }
            match self.backend {
                GeometrySpatialBackend::Flat => {
                    for &slot in &self.active {
                        results.masks[slot * results.words + word] =
                            filter(self.box_at(slot), queries, all);
                    }
                }
                GeometrySpatialBackend::Bvh => {
                    scratch.stack.clear();
                    if !self.tree.nodes.is_empty() {
                        scratch.stack.push((0, all));
                    }
                    while let Some((index, active)) = scratch.stack.pop() {
                        let node = self.tree.nodes[index];
                        let active = filter(node.bounds, queries, active);
                        if active == 0 {
                            continue;
                        }
                        if let Some(slot) = node.slot {
                            results.masks[slot * results.words + word] = active;
                        } else {
                            scratch.stack.push((node.right, active));
                            scratch.stack.push((node.left, active));
                        }
                    }
                }
            }
        }
    }
}
