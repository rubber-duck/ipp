//! Packed binary BVH: median construction, bottom-up refits and quality-triggered rebuilds.

use super::{BoxBounds, area, index::GeometryPreparedBounds, union};

#[derive(Clone, Copy)]
pub(super) struct GeometryBvhNode {
    pub bounds: BoxBounds,
    pub left: usize,
    pub right: usize,
    pub slot: Option<usize>,
}

#[derive(Default)]
pub(super) struct GeometryBvh {
    pub nodes: Vec<GeometryBvhNode>,
    order: Vec<usize>,
    quality: f64,
    refits: u32,
}

fn bounds(rows: &[Option<GeometryPreparedBounds>], slot: usize) -> BoxBounds {
    rows[slot]
        .expect("indexed row")
        .culling
        .expect("bounded row")
        .bounds
}

impl GeometryBvh {
    pub fn rebuild(&mut self, rows: &[Option<GeometryPreparedBounds>], slots: &[usize]) {
        self.nodes.clear();
        self.order.clear();
        self.order.extend_from_slice(slots);
        self.nodes.reserve(slots.len().saturating_mul(2));
        if !slots.is_empty() {
            Self::build(&mut self.nodes, rows, &mut self.order);
        }
        self.quality = self.quality();
        self.refits = 0;
    }

    fn build(
        nodes: &mut Vec<GeometryBvhNode>,
        rows: &[Option<GeometryPreparedBounds>],
        order: &mut [usize],
    ) -> usize {
        let node = nodes.len();
        let enclosure = order
            .iter()
            .skip(1)
            .fold(bounds(rows, order[0]), |b, &slot| {
                union(b, bounds(rows, slot))
            });
        nodes.push(GeometryBvhNode {
            bounds: enclosure,
            left: 0,
            right: 0,
            slot: (order.len() == 1).then_some(order[0]),
        });
        if order.len() > 1 {
            let axis = (0..3)
                .max_by(|&a, &b| {
                    (enclosure[1][a] - enclosure[0][a])
                        .total_cmp(&(enclosure[1][b] - enclosure[0][b]))
                })
                .expect("three axes");
            let middle = order.len() / 2;
            order.select_nth_unstable_by(middle, |&a, &b| {
                let a_bounds = bounds(rows, a);
                let b_bounds = bounds(rows, b);
                (a_bounds[0][axis] + a_bounds[1][axis])
                    .total_cmp(&(b_bounds[0][axis] + b_bounds[1][axis]))
                    .then(a.cmp(&b))
            });
            let (a, b) = order.split_at_mut(middle);
            nodes[node].left = Self::build(nodes, rows, a);
            nodes[node].right = Self::build(nodes, rows, b);
        }
        node
    }

    fn quality(&self) -> f64 {
        self.nodes.first().map_or(0.0, |root| {
            self.nodes.iter().map(|node| area(node.bounds)).sum::<f64>()
                / area(root.bounds).max(f64::MIN_POSITIVE)
        })
    }

    pub fn refit(&mut self, rows: &[Option<GeometryPreparedBounds>], slots: &[usize]) {
        for i in (0..self.nodes.len()).rev() {
            let node = self.nodes[i];
            self.nodes[i].bounds = node.slot.map_or_else(
                || union(self.nodes[node.left].bounds, self.nodes[node.right].bounds),
                |slot| bounds(rows, slot),
            );
        }
        self.refits = self.refits.saturating_add(1);
        if self.refits >= 32 && self.quality() > self.quality * 1.5 {
            self.rebuild(rows, slots);
        }
    }
}
