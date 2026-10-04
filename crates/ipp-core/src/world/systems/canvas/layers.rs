//! Relative component priorities and complete overlay scopes, resolved from
//! the current core tree into compact physical ranks. Logical priorities are
//! independent of Surface spacing and never allocate historical depth slots.
//! Shown entities occupy groups even when they are structural/layout roots
//! without paint. This keeps depth placement independent of resource readiness.
//! Closed overlay subtrees are excluded by the caller.

use crate::EntityId;
use crate::world::WorldSimulationState;

/// An ordinary relative level within a semantic band and overlay scope.
/// Lexicographic scope order puts every owner's level below its nested scopes,
/// and an earlier complete scope below the next sibling scope.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CanvasLayerKey {
    band: u32,
    scope: Vec<usize>,
    level: u64,
}

impl CanvasLayerKey {
    pub fn child(&self, world: &WorldSimulationState, entity: EntityId, position: usize) -> Self {
        let index = entity.index() as usize;
        let mut key = self.clone();
        if let Some(overlay) = world.components.gui_overlay(index) {
            let band = key.band.max(overlay.band);
            // Promotion starts a scope in the higher band. Tree position still
            // identifies its ownership deterministically; ordinary ancestry
            // remains intact for layout, focus and interaction.
            if band != key.band {
                key.scope.clear();
            }
            key.band = band;
            key.scope.push(position);
            key.level = 0;
        }
        let offset = world
            .components
            .canvas_style(index)
            .map_or(0, |style| style.layer);
        // A tree can contain at most u32::MAX indexed entities. Summing their
        // u32 offsets fits u64 even when an authored priority exceeds u32.
        key.level = key
            .level
            .checked_add(u64::from(offset))
            .expect("Canvas relative layer sum exhausted");
        key
    }
}

/// Resolved physical ranks and logical keys aligned with the shown tree order.
pub(super) struct CanvasLayers {
    layers: Vec<u32>,
    used: Vec<u32>,
    pub keys: Vec<CanvasLayerKey>,
}

impl CanvasLayers {
    /// None for all-zero ordinary content, preserving the tree-order fast path.
    pub fn resolve(world: &WorldSimulationState, order: &[EntityId]) -> Option<Self> {
        if order.iter().all(|entity| {
            let index = entity.index() as usize;
            world
                .components
                .canvas_style(index)
                .is_none_or(|style| style.layer == 0)
                && world.components.gui_overlay(index).is_none()
        }) {
            return None;
        }

        let mut path: Vec<(EntityId, CanvasLayerKey)> = Vec::new();
        let keys: Vec<_> = order
            .iter()
            .enumerate()
            .map(|(position, &entity)| {
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
                let key = path
                    .last()
                    .map_or_else(CanvasLayerKey::default, |(_, key)| key.clone())
                    .child(world, entity, position);
                path.push((entity, key.clone()));
                key
            })
            .collect();
        let mut groups = keys.clone();
        groups.sort_unstable();
        groups.dedup();
        let layers = keys
            .iter()
            .map(|key| groups.binary_search(key).unwrap() as u32)
            .collect();
        let used = (0..groups.len() as u32).collect();
        Some(Self {
            layers,
            used,
            keys,
        })
    }

    pub fn layer(&self, position: usize) -> u32 {
        self.layers[position]
    }

    pub fn used(&self) -> &[u32] {
        &self.used
    }
}

/// Published index of each tree-order item on rank `layers`: by rank,
/// keeping tree order within each rank. Hits keep their tree-order
/// `paint_order`.
pub(super) fn layer_order(layers: impl Iterator<Item = u32>) -> Vec<u32> {
    let layers: Vec<u32> = layers.collect();
    let mut published: Vec<u32> = (0..layers.len() as u32).collect();
    published.sort_by_key(|&index| layers[index as usize]);
    let mut order = vec![0; layers.len()];
    for (at, index) in published.into_iter().enumerate() {
        order[index as usize] = at as u32;
    }
    order
}

/// Tree-order `items` at their published indices `order`.
pub(super) fn in_layer_order<T>(items: Vec<T>, order: &[u32]) -> Vec<T> {
    let mut published: Vec<Option<T>> = std::iter::repeat_with(|| None).take(items.len()).collect();
    for (item, &at) in items.into_iter().zip(order) {
        published[at as usize] = Some(item);
    }
    published.into_iter().map(Option::unwrap).collect()
}
