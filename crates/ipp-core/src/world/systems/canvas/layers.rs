//! Canvas layers: resolution of the authored `CanvasStyle.layer` plane ids and
//! the painter and hit order they select.
//!
//! A layer is a plane id within the canvas. An entity whose layer is zero is on
//! its parent's plane, the base plane 0 at the top level; a nonzero layer puts
//! the entity on that plane, or one above its parent's plane when the parent's
//! is not below it, so content never sits below its parent. Every entity that
//! resolves to plane `n` shares that plane wherever it is declared. Paint and
//! hits are stable-sorted by plane, keeping tree order within each plane. The
//! publication lists the planes in use, ascending, and each primitive, hit and
//! slot carries its plane id, so a plane keeps its id, and an exploded Surface
//! its depth, whatever other planes come into or go out of use. A canvas whose
//! layers are all zero resolves nothing and keeps its tree order unchanged.

use super::{CanvasHit, CanvasPaintEntry};
use crate::EntityId;
use crate::world::WorldSimulationState;
use std::sync::Arc;

/// Resolved layers of one canvas walk, aligned with its tree order.
pub(super) struct CanvasLayers {
    /// Plane id of the entity at each position of the tree order.
    layers: Vec<u32>,
    /// Distinct plane ids in use, ascending.
    used: Vec<u32>,
}

impl CanvasLayers {
    /// Resolve the plane of every entity in `order`, a depth-first tree order.
    /// `None` when every layer is zero, which leaves the whole canvas on the
    /// base plane.
    pub fn resolve(world: &WorldSimulationState, order: &[EntityId]) -> Option<Self> {
        let authored = |entity: EntityId| {
            world
                .components
                .canvas_style(entity.index() as usize)
                .map_or(0, |style| style.layer)
        };
        if order.iter().all(|&entity| authored(entity) == 0) {
            return None;
        }

        // Entities from the top level to the current one with their planes.
        let mut path: Vec<(EntityId, u32)> = Vec::new();
        let layers: Vec<u32> = order
            .iter()
            .map(|&entity| {
                let parent = world
                    .state
                    .links
                    .effective(entity)
                    .and_then(|link| link.parent);
                while path
                    .last()
                    .is_some_and(|(ancestor, _)| Some(*ancestor) != parent)
                {
                    path.pop();
                }
                let layer = resolve(path.last().map_or(0, |(_, layer)| *layer), authored(entity));
                path.push((entity, layer));
                layer
            })
            .collect();

        let mut used = layers.clone();
        used.sort_unstable();
        used.dedup();
        Some(Self {
            layers,
            used,
        })
    }

    /// Plane id of the entity at `position` in the resolved tree order.
    pub fn layer(&self, position: usize) -> u32 {
        self.layers[position]
    }

    /// Distinct plane ids in use, ascending.
    pub fn used(&self) -> &[u32] {
        &self.used
    }
}

/// Plane of an entity authoring `layer` under a parent on plane `parent`: the
/// parent's for zero, otherwise `layer` but never below one above the parent.
/// An entity under the highest plane id stays on its parent's plane, after it
/// in tree order.
fn resolve(parent: u32, layer: u32) -> u32 {
    if layer == 0 {
        parent
    } else {
        layer.max(parent.saturating_add(1))
    }
}

/// Order paint and hits by plane, keeping the tree order of the walk within
/// each plane. Hits keep their tree-order `paint_order`.
pub(super) fn order_by_layer(entries: &mut [Arc<CanvasPaintEntry>], hits: &mut [CanvasHit]) {
    entries.sort_by_key(|entry| entry.layer());
    hits.sort_by_key(|hit| hit.layer);
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn zero_inherits_and_a_nonzero_layer_is_a_plane_id_above_its_parent() {
        // Zero keeps the parent's plane, the base at the top level.
        assert_eq!(resolve(0, 0), 0);
        assert_eq!(resolve(2, 0), 2);
        // The same id names the same plane from any depth below it.
        assert_eq!(resolve(0, 3), 3);
        assert_eq!(resolve(2, 3), 3);
        // A raise never lands on or below its parent's plane.
        assert_eq!(resolve(1, 1), 2);
        assert_eq!(resolve(2, 1), 3);
        assert_eq!(resolve(3, 3), 4);
        // The highest plane id cannot rise further.
        assert_eq!(resolve(u32::MAX, 1), u32::MAX);
    }
}
