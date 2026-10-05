//! Relative component priorities and complete overlay scopes, resolved from
//! the current core tree into physical planes. Logical priorities are
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
    fn same_scope(&self, other: &Self) -> bool {
        self.band == other.band && self.scope == other.scope
    }

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

/// Current physical plane IDs and destination priorities in shown tree order.
pub(super) struct CanvasLayers {
    layers: Vec<u32>,
    priorities: Vec<u32>,
    used: Vec<super::CanvasLayerPlane>,
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
                && world.components.canvas_layer_transition(index).is_none()
        }) {
            return None;
        }

        let mut path: Vec<(EntityId, usize)> = Vec::new();
        let mut keys: Vec<CanvasLayerKey> = Vec::with_capacity(order.len());
        let mut parents = Vec::with_capacity(order.len());
        let mut previous = Vec::with_capacity(order.len());
        let mut groups = Vec::with_capacity(order.len());
        for (position, &entity) in order.iter().enumerate() {
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
            let parent = path.last().map(|(_, position)| *position);
            let key = parent
                .map_or_else(CanvasLayerKey::default, |parent| keys[parent].clone())
                .child(world, entity, position);
            let transition = world
                .components
                .canvas_layer_transition(entity.index() as usize);
            let previous_key = transition.map(|transition| {
                let mut previous_key = key.clone();
                let destination = world
                    .components
                    .canvas_style(entity.index() as usize)
                    .map_or(0, |style| style.layer);
                previous_key.level =
                    key.level - u64::from(destination) + u64::from(transition.previous_layer);
                groups.push(previous_key.clone());
                (previous_key, f64::from(transition.progress))
            });
            groups.push(key.clone());
            keys.push(key);
            parents.push(parent);
            previous.push(previous_key);
            path.push((entity, position));
        }
        groups.sort_unstable();
        groups.dedup();

        // Endpoint ranks are local to an exact discrete band/scope. An ancestor's
        // previous endpoint never changes a child's authored destination key.
        let mut starts = Vec::with_capacity(groups.len());
        let mut scopes = Vec::with_capacity(groups.len());
        let mut start = 0;
        let mut scope = 0;
        for (position, key) in groups.iter().enumerate() {
            if position > 0 && !key.same_scope(&groups[position - 1]) {
                start = position;
                scope += 1;
            }
            starts.push(start);
            scopes.push(scope);
        }
        let priorities: Vec<_> = keys
            .iter()
            .map(|key| groups.binary_search(key).unwrap() as u32)
            .collect();
        let ranks: Vec<_> = priorities
            .iter()
            .map(|&priority| f64::from(priority) - starts[priority as usize] as f64)
            .collect();
        let mut offsets = vec![0.0; order.len()];
        let mut ranges = vec![[0.0; 2]; order.len()];
        let mut maxima = vec![0.0_f64; scope + 1];
        for position in 0..order.len() {
            let parent =
                parents[position].filter(|&parent| keys[parent].same_scope(&keys[position]));
            let (parent_offset, parent_range, parent_rank) = parent
                .map_or((0.0, [0.0; 2], 0.0), |parent| {
                    (offsets[parent], ranges[parent], ranks[parent])
                });
            let target = ranks[position];
            let source = previous[position].as_ref().map_or(target, |(key, _)| {
                let index = groups.binary_search(key).unwrap();
                (index - starts[index]) as f64
            });
            let progress = previous[position]
                .as_ref()
                .map_or(1.0, |(_, progress)| *progress);
            // Form exact integral local deltas before adding fractional inherited
            // placement. In particular, a zero-layer decoration must add exactly
            // zero rather than cancel ranks around its parent's fractional offset.
            let source_delta = source - parent_rank;
            let target_delta = target - parent_rank;
            let delta = source_delta + (target_delta - source_delta) * progress;
            offsets[position] = parent_offset + delta;
            ranges[position] = [
                parent_range[0] + source_delta.min(target_delta),
                parent_range[1] + source_delta.max(target_delta),
            ];
            let scope = scopes[priorities[position] as usize];
            maxima[scope] = maxima[scope].max(ranges[position][1]);
        }

        // Reserve every scope's conservative authored extent, even at progress1.
        // Complete later scopes remain above all independently timed descendants.
        let mut bases = vec![0.0; maxima.len()];
        for scope in 1..maxima.len() {
            bases[scope] = bases[scope - 1] + maxima[scope - 1] + 1.0;
        }
        let placements: Vec<_> = offsets
            .iter()
            .enumerate()
            .map(|(position, &offset)| {
                let scope = scopes[priorities[position] as usize];
                (scope, bases[scope] + offset)
            })
            .collect();
        let mut planes = placements.clone();
        planes.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        planes.dedup();
        let layers = placements
            .iter()
            .map(|placement| {
                planes
                    .binary_search_by(|plane| {
                        plane
                            .0
                            .cmp(&placement.0)
                            .then(plane.1.total_cmp(&placement.1))
                    })
                    .unwrap() as u32
            })
            .collect();
        let used = planes
            .iter()
            .enumerate()
            .map(|(id, &(_, offset))| super::CanvasLayerPlane {
                id: id as u32,
                offset,
            })
            .collect();
        Some(Self {
            layers,
            priorities,
            used,
            keys,
        })
    }

    pub fn layer(&self, position: usize) -> u32 {
        self.layers[position]
    }

    pub fn priority(&self, position: usize) -> u32 {
        self.priorities[position]
    }

    pub fn used(&self) -> &[super::CanvasLayerPlane] {
        &self.used
    }
}

/// Published index of each tree-order item at logical `layers` priority: by priority,
/// keeping tree order within each priority. Hits keep their tree-order
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
