//! VirtualList scrolling: persisted anchors and the wanted item range.
//!
//! A VirtualList scrolls through the ordinary ScrollView path: routing,
//! wheel and drag consumption, scroll bars and hit testing use its
//! input-owned offset and evaluated extents unchanged. Its authoritative
//! scroll position is the persisted anchor in its data row, the first
//! visible item and the offset into it, so this module keeps offset and
//! anchor in step at two points:
//!
//! - After scroll envelopes apply, each moved list's new offset converts to
//!   an anchor against the retained placement the routing used and is
//!   staged into its producer root once per frame.
//! - After each layout, before routing, every evaluated list's offset is
//!   recomputed from its anchor against the fresh placement and clamped to
//!   its capacity. A measurement or declaration change above the viewport
//!   therefore shifts the offset by the same amount and visible content
//!   stays put; restore and scroll-to-index reach the offset the same way.
//!
//! The same step computes the wanted range from the offset, viewport and
//! overscan and publishes [`GuiInputEffectKind::VirtualRangeChanged`] when
//! it differs from the last range published for that list, including the
//! first evaluation of a list or of a restored incarnation. Cost follows
//! evaluated lists and their declared children, never the item count.

use super::super::layout::scroll_bars::scroll_capacity;
use super::super::{GuiInputEffect, GuiInputEffectKind, GuiInputTarget, GuiLayoutSystem};
use super::system::{GuiInputSystem, ScrollCursor, stage_producer_root};
use super::target_policy::producer_root;
use crate::systems::SystemRuntimeAccess;
use crate::world::WorldSimulationState;
use std::collections::{BTreeMap, BTreeSet};

/// Last range published for one VirtualList.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PublishedRange {
    first: u32,
    last: u32,
    revision: u32,
}

/// VirtualList bookkeeping owned by the input system: lists whose offset
/// moved since anchors were last persisted, and the range last published
/// per list of a live root incarnation.
#[derive(Debug, Default)]
pub(in crate::world::systems::gui) struct GuiVirtualScroll {
    moved: BTreeSet<GuiInputTarget>,
    published: BTreeMap<GuiInputTarget, PublishedRange>,
}

impl GuiVirtualScroll {
    /// Note an applied scroll that moved one target's offset; only
    /// VirtualLists persist it.
    pub(super) fn offset_moved(&mut self, target: GuiInputTarget) {
        self.moved.insert(target);
    }
}

impl GuiInputSystem {
    /// Persist the anchors of VirtualLists whose offsets moved this
    /// mutation boundary, one staged producer root per changed list.
    pub(super) fn persist_virtual_anchors(&mut self, access: &mut SystemRuntimeAccess<'_>) {
        let moved = std::mem::take(&mut self.virtual_scroll.moved);
        if moved.is_empty() {
            return;
        }
        let Ok(layout) = self.layout(&*access) else {
            return;
        };

        let mut anchors = Vec::new();
        for target in moved {
            let Some(list) = layout
                .view(target.entity)
                .filter(|view| view.root_incarnation == target.root_incarnation)
                .and_then(|view| view.virtual_lists.get(&target.node))
            else {
                continue;
            };
            let offset = self
                .scroll_offsets
                .get(&target)
                .map_or(0.0, |cursor| cursor.offset[list.axis]);
            let (index, within) = list.anchor(offset);
            anchors.push((target, index, within));
        }

        for (target, index, within) in anchors {
            let Some((incarnation, root)) = producer_root(access.world, target.entity) else {
                continue;
            };
            let current = root
                .data_row(target.node)
                .map(|row| (row.anchor_index, row.anchor_offset));
            if incarnation != target.root_incarnation
                || current == Some((Some(index), Some(within)))
            {
                continue;
            }

            let mut root = root.into_owned();
            if root.set_virtual_anchor(target.node, index, within).is_ok() {
                // A refused staging keeps the previous anchor; the next sync
                // then restores the offset it names.
                let _ = stage_producer_root(access, target.entity, root);
            }
        }
    }

    /// Recompute every evaluated VirtualList's offset from its persisted
    /// anchor against current layout, and publish changed wanted ranges.
    /// Runs after layout and before routing.
    pub(super) fn sync_virtual_lists(
        &mut self,
        sim: &WorldSimulationState,
        layout: &GuiLayoutSystem,
        tick: u64,
    ) {
        for entity in layout.evaluated_entities() {
            let Some(view) = layout
                .view(entity)
                .filter(|view| view.available && !view.virtual_lists.is_empty())
            else {
                continue;
            };
            let Some((incarnation, root)) = producer_root(sim, entity) else {
                continue;
            };
            if incarnation != view.root_incarnation {
                continue;
            }

            for (&node, list) in &view.virtual_lists {
                let target = GuiInputTarget {
                    entity,
                    root_incarnation: view.root_incarnation,
                    node,
                };

                let values = root.data_row(node);
                let anchor_index = values.and_then(|row| row.anchor_index).unwrap_or(0);
                let anchor_offset = values.and_then(|row| row.anchor_offset).unwrap_or(0.0);
                let capacity = view
                    .nodes
                    .iter()
                    .find(|record| record.node == node)
                    .map_or([0.0, 0.0], scroll_capacity)[list.axis];
                let main = list
                    .anchored_offset(anchor_index, anchor_offset)
                    .clamp(0.0, capacity);
                let mut offset = [0.0, 0.0];
                offset[list.axis] = main;

                let current = self.scroll_offsets.get(&target).copied();
                if current.map(|cursor| cursor.offset) != Some(offset) {
                    self.scroll_offsets.insert(
                        target,
                        ScrollCursor {
                            offset,
                            session: current.map_or(0, |cursor| cursor.session),
                            source_tick: tick,
                        },
                    );
                    if current.map_or([0.0, 0.0], |cursor| cursor.offset) != offset {
                        self.scroll_revision = self.scroll_revision.saturating_add(1);
                    }
                }

                let (first, last) = list.wanted_range(main);
                let previous = self.virtual_scroll.published.get(&target).copied();
                if previous.is_some_and(|range| (range.first, range.last) == (first, last)) {
                    continue;
                }
                let revision = previous.map_or(1, |range| range.revision.saturating_add(1));
                self.virtual_scroll.published.insert(
                    target,
                    PublishedRange {
                        first,
                        last,
                        revision,
                    },
                );
                self.pending_effects.push(GuiInputEffect {
                    session: 0,
                    source_tick: tick,
                    effect_tick: tick,
                    kind: GuiInputEffectKind::VirtualRangeChanged {
                        entity,
                        node,
                        first,
                        last,
                        revision,
                    },
                });
            }
        }
        // Ranges of removed lists and replaced incarnations drop; a list
        // that only paused evaluation keeps its revision sequence.
        self.virtual_scroll.published.retain(|target, _| {
            producer_root(sim, target.entity).is_some_and(|(incarnation, root)| {
                incarnation == target.root_incarnation && root.nodes().node(target.node).is_some()
            })
        });
    }
}
