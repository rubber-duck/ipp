//! Derived child order of one root's `node_tree` rows.
//!
//! The [`GuiSystem`](super::super::GuiSystem) keeps one index per live root in
//! its runtime state, rebuilt whole when a root incarnation or its whole tree
//! table is installed and updated in place for the nodes whose `parent` or
//! `order` an operation wrote. It is reconstructible from the rows at any
//! time and never authoritative.

use super::component::GuiRoot;
use super::nodes::GuiNodeId;
use std::collections::BTreeMap;

/// Ordered children of every node of one root: siblings by `(order, id)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiTreeIndex {
    /// Root component incarnation the index was derived from.
    incarnation: u64,
    /// The node whose parent is 0.
    root: Option<GuiNodeId>,
    /// Ordered children of every node with at least one child.
    children: BTreeMap<GuiNodeId, Vec<GuiNodeId>>,
    /// Indexed `(parent, order)` of every live node.
    placement: BTreeMap<GuiNodeId, (u32, u32)>,
}

impl GuiTreeIndex {
    /// Derive the index of a root incarnation from its rows.
    pub fn new(root: &GuiRoot, incarnation: u64) -> Self {
        let mut links: Vec<(u32, u32, u32)> = root
            .node_tree()
            .rows()
            .iter()
            .map(|(slot, row)| (row.parent, row.order, slot))
            .collect();
        links.sort_unstable();

        let mut index = Self {
            incarnation,
            ..Self::default()
        };
        for (parent, order, slot) in links {
            let id = GuiNodeId(slot);
            index.placement.insert(id, (parent, order));
            if parent == 0 {
                index.root.get_or_insert(id);
            } else {
                index
                    .children
                    .entry(GuiNodeId(parent))
                    .or_default()
                    .push(id);
            }
        }
        index
    }

    /// Root component incarnation the index describes.
    pub fn incarnation(&self) -> u64 {
        self.incarnation
    }

    /// The root node.
    pub fn root(&self) -> Option<GuiNodeId> {
        self.root
    }

    /// Ordered children of one node; empty for leaves and absent nodes.
    pub fn children(&self, id: GuiNodeId) -> &[GuiNodeId] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Number of indexed nodes.
    pub fn len(&self) -> usize {
        self.placement.len()
    }

    /// Whether no node is indexed.
    pub fn is_empty(&self) -> bool {
        self.placement.is_empty()
    }

    /// Whether a node is indexed.
    pub fn contains(&self, id: GuiNodeId) -> bool {
        self.placement.contains_key(&id)
    }

    /// Nodes of the subtree of `id` in depth-first tree order, `id` first;
    /// empty for an absent node.
    pub fn subtree(&self, id: GuiNodeId) -> Vec<GuiNodeId> {
        if !self.contains(id) {
            return Vec::new();
        }

        let mut nodes = Vec::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            nodes.push(current);
            stack.extend(self.children(current).iter().rev());
        }
        nodes
    }

    /// Re-derive the placement of one node from `root`'s rows: move it to its
    /// written parent and order, index it when it was allocated, or drop its
    /// indexed subtree when it retired. Other nodes keep their placement.
    pub fn update(&mut self, root: &GuiRoot, id: GuiNodeId) {
        let written = root.node_tree().get(id).map(|row| (row.parent, row.order));
        let indexed = self.placement.get(&id).copied();
        if written == indexed {
            return;
        }

        if let Some(placement) = indexed {
            self.unlink(id, placement);
        }
        if written.is_none() {
            // Descendants leave with their ancestors' child lists.
            for node in self.subtree(id) {
                self.placement.remove(&node);
                self.children.remove(&node);
            }
        }

        if let Some(placement) = written {
            self.link(id, placement);
        }
    }

    /// Remove one node from its indexed parent's children.
    fn unlink(&mut self, id: GuiNodeId, (parent, order): (u32, u32)) {
        if parent == 0 {
            if self.root == Some(id) {
                self.root = None;
            }
            return;
        }

        let parent = GuiNodeId(parent);
        let position = self.position(parent, (order, id));
        if let Some(siblings) = self.children.get_mut(&parent) {
            if siblings.get(position) == Some(&id) {
                siblings.remove(position);
            }
            if siblings.is_empty() {
                self.children.remove(&parent);
            }
        }
    }

    /// First position among `parent`'s indexed children not ordered before
    /// `key`.
    fn position(&self, parent: GuiNodeId, key: (u32, GuiNodeId)) -> usize {
        self.children(parent).partition_point(|sibling| {
            let order = self.placement.get(sibling).map_or(0, |&(_, order)| order);
            (order, *sibling) < key
        })
    }

    /// Place one node under its parent at its `(order, id)` position.
    fn link(&mut self, id: GuiNodeId, placement: (u32, u32)) {
        self.placement.insert(id, placement);
        let (parent, order) = placement;
        if parent == 0 {
            self.root = Some(id);
            return;
        }

        let parent = GuiNodeId(parent);
        let position = self.position(parent, (order, id));
        self.children
            .entry(parent)
            .or_default()
            .insert(position, id);
    }
}

#[cfg(test)]
#[path = "tree_index_tests.rs"]
mod tests;
