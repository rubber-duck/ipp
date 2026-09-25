//! Row writes applying one node command to a root.
//!
//! Every node command becomes a bounded number of ordinary field writes at
//! row addresses, planned from the producer root and the GUI System's derived
//! [`GuiTreeIndex`]. Each write leaves a root that its own
//! [`validate_field`](crate::components::schema::ComponentLifecycle::validate_field)
//! accepts, and the root's write hooks derive what follows from it: an
//! allocated node's default rows, a retired node's subtree, a kind's shape.
//! Only renumbering one parent's sibling keys, when an insertion or move
//! finds no gap, grows with the sibling count.

use super::component::{GuiControlCommit, GuiControlCommitValue, GuiNodeDataEdit, GuiRoot};
use super::node_rows::{
    GuiNodeDataProperty, GuiNodeDataRow, GuiNodeStyleProperty, GuiNodeStyleRow,
};
use super::node_tree::{GuiNodeTreeProperty, GuiNodeTreeRow, place_among};
use super::nodes::{
    GuiControlValue, GuiNodeData, GuiNodeId, GuiNodeKind, GuiNodePatch, GuiNodeStyle, MAX_NODE_ID,
    MAX_NODES, valid_node_data,
};
use super::tree_index::GuiTreeIndex;
use crate::components::rows::SchemaRow;
use crate::{DynamicValue, ErrorReason, FieldValue, FieldWrite};

/// Write of one tree property of one node.
fn tree_write(
    node: GuiNodeId,
    property: GuiNodeTreeProperty,
    value: FieldValue,
) -> Result<FieldWrite, ErrorReason> {
    Ok(FieldWrite {
        offset: GuiRoot::node_tree_offset(node, property).ok_or(ErrorReason::Capacity)?,
        value,
    })
}

/// Write of one U32 tree property.
fn tree_u32(
    node: GuiNodeId,
    property: GuiNodeTreeProperty,
    value: u32,
) -> Result<FieldWrite, ErrorReason> {
    tree_write(
        node,
        property,
        FieldValue::Dynamic(DynamicValue::U32(value)),
    )
}

/// Write of one optional text tree property.
fn tree_text(
    node: GuiNodeId,
    property: GuiNodeTreeProperty,
    value: Option<&str>,
) -> Result<FieldWrite, ErrorReason> {
    tree_write(
        node,
        property,
        value.map_or(FieldValue::Unset, |text| {
            FieldValue::String(text.to_owned())
        }),
    )
}

/// Writes turning one node's tree row `from` into `to` for its strings and
/// revision, in layout order; unchanged properties are skipped.
fn tree_row_writes(
    node: GuiNodeId,
    from: &GuiNodeTreeRow,
    to: &GuiNodeTreeRow,
    writes: &mut Vec<FieldWrite>,
) -> Result<(), ErrorReason> {
    for (property, before, after) in [
        (GuiNodeTreeProperty::Text, &from.text, &to.text),
        (
            GuiNodeTreeProperty::Placeholder,
            &from.placeholder,
            &to.placeholder,
        ),
        (
            GuiNodeTreeProperty::CommittedText,
            &from.committed_text,
            &to.committed_text,
        ),
    ] {
        if before != after {
            writes.push(tree_text(node, property, after.as_deref())?);
        }
    }
    if from.revision != to.revision {
        writes.push(tree_u32(node, GuiNodeTreeProperty::Revision, to.revision)?);
    }
    Ok(())
}

/// Writes turning one node's style row `from` into `to`, in layout order.
fn style_writes(
    node: GuiNodeId,
    from: &GuiNodeStyleRow,
    to: &GuiNodeStyleRow,
    writes: &mut Vec<FieldWrite>,
) {
    for property in GuiNodeStyleProperty::ALL {
        let value = to.property(property.index()).ok().flatten();
        if value != from.property(property.index()).ok().flatten()
            && let Some(offset) = GuiRoot::node_style_offset(node, property)
        {
            writes.push(FieldWrite {
                offset,
                value: value.map_or(FieldValue::Unset, FieldValue::Dynamic),
            });
        }
    }
}

/// Writes turning one node's data row `from` into `to` in an order that
/// keeps the slider range valid after every write.
fn data_writes(
    node: GuiNodeId,
    from: &GuiNodeDataRow,
    to: &GuiNodeDataRow,
    writes: &mut Vec<FieldWrite>,
) {
    for property in from.write_order(to) {
        let value = to.property(property.index()).ok().flatten();
        if let Some(offset) = GuiRoot::node_data_offset(node, property) {
            writes.push(FieldWrite {
                offset,
                value: value.map_or(FieldValue::Unset, FieldValue::Dynamic),
            });
        }
    }
}

impl GuiRoot {
    /// Ordered `(id, order)` of the indexed children of `parent`, excluding
    /// `except`.
    fn indexed_siblings(
        &self,
        tree: &GuiTreeIndex,
        parent: GuiNodeId,
        except: Option<GuiNodeId>,
    ) -> Vec<(GuiNodeId, u32)> {
        tree.children(parent)
            .iter()
            .filter(|&&child| Some(child) != except)
            .map(|&child| (child, self.tree_row(child).map_or(0, |row| row.order)))
            .collect()
    }

    /// Order-key writes placing `node` at `index` among the indexed children
    /// of `parent`: renumbered siblings first, then the node's key. A
    /// VirtualList child's key is its item index, so `index` names the item
    /// and no sibling changes.
    fn placement_writes(
        &self,
        tree: &GuiTreeIndex,
        node: GuiNodeId,
        parent: GuiNodeId,
        index: usize,
        writes: &mut Vec<FieldWrite>,
    ) -> Result<u32, ErrorReason> {
        if self.tree_row(parent).and_then(GuiNodeTreeRow::node_kind)
            == Some(GuiNodeKind::VirtualList)
        {
            return u32::try_from(index).map_err(|_| ErrorReason::InvalidValue);
        }
        let siblings = self.indexed_siblings(tree, parent, Some(node));
        let (key, renumbered) = place_among(&siblings, index.min(siblings.len()));
        for (sibling, order) in renumbered {
            writes.push(tree_u32(sibling, GuiNodeTreeProperty::Order, order)?);
        }
        Ok(key)
    }

    /// Writes inserting one node with its complete style and kind-specific
    /// data: sibling keys when renumbered, the allocating `parent` write
    /// (which adds default style and data rows), its order key, its kind
    /// (which conforms its strings, revision and data row), its strings and
    /// the style and data properties that differ from those defaults.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::world::systems::gui) fn insert_node_writes(
        &self,
        tree: &GuiTreeIndex,
        id: GuiNodeId,
        parent: Option<GuiNodeId>,
        index: usize,
        data: &GuiNodeData,
        values: &GuiNodeDataRow,
        style: &GuiNodeStyle,
    ) -> Result<Vec<FieldWrite>, ErrorReason> {
        if id.0 >= MAX_NODE_ID {
            return Err(ErrorReason::Capacity);
        }
        if self.node_tree().len() >= MAX_NODES
            || id.0 != self.next_node_id()
            || !valid_node_data(data)
        {
            return Err(ErrorReason::InvalidValue);
        }
        let style = GuiNodeStyleRow::from(style);
        style.validate()?;
        let kind = data.kind();
        values.validate_for(kind)?;

        let mut writes = Vec::new();
        let order = match parent {
            None if !self.node_tree().is_empty() => return Err(ErrorReason::InvalidValue),
            None => 0,
            Some(parent) => {
                self.node_tree().validate_placement(id, parent.0)?;
                self.placement_writes(tree, id, parent, index, &mut writes)?
            }
        };

        writes.push(tree_u32(
            id,
            GuiNodeTreeProperty::Parent,
            parent.map_or(0, |parent| parent.0),
        )?);
        let mut allocated = GuiNodeTreeRow {
            parent: parent.map_or(0, |parent| parent.0),
            ..GuiNodeTreeRow::default()
        };
        if order != allocated.order {
            writes.push(tree_u32(id, GuiNodeTreeProperty::Order, order)?);
        }
        if kind.code() != allocated.kind {
            writes.push(tree_u32(id, GuiNodeTreeProperty::Kind, kind.code())?);
        }
        allocated.conform(kind);
        let authored = GuiNodeTreeRow::authored(parent, order, data);
        tree_row_writes(id, &allocated, &authored, &mut writes)?;

        style_writes(id, &GuiNodeStyleRow::default(), &style, &mut writes);
        let mut defaults = GuiNodeDataRow::default();
        defaults.conform(kind);
        data_writes(id, &defaults, values, &mut writes);
        Ok(writes)
    }

    /// Writes applying a node patch: the kind when it changes (which
    /// conforms strings, revision and data row), then strings, committed
    /// text and revision, the data row, and the changed style properties.
    pub(in crate::world::systems::gui) fn update_node_writes(
        &self,
        id: GuiNodeId,
        patch: &GuiNodePatch,
    ) -> Result<Vec<FieldWrite>, ErrorReason> {
        let row = self.tree_row(id).ok_or(ErrorReason::InvalidValue)?;
        let mut writes = Vec::new();
        if patch.data.is_some() || patch.values.is_some() {
            let current = self.data_row(id).cloned().unwrap_or_default();
            let edit = GuiNodeDataEdit::new(row, &current, patch)?;
            let mut conformed = row.clone();
            let mut base = current;
            if edit.row.kind != row.kind {
                writes.push(tree_u32(id, GuiNodeTreeProperty::Kind, edit.row.kind)?);
                let kind = edit.row.node_kind().ok_or(ErrorReason::InvalidValue)?;
                conformed.conform(kind);
                base.conform(kind);
            }
            tree_row_writes(id, &conformed, &edit.row, &mut writes)?;
            data_writes(id, &base, &edit.values, &mut writes);
        }

        let current = self.style_row(id).ok_or(ErrorReason::InvalidValue)?;
        let mut style = current.clone();
        style.apply(patch);
        style.validate()?;
        style_writes(id, current, &style, &mut writes);
        Ok(writes)
    }

    /// Writes moving a node to `index` among the children of `parent`:
    /// sibling keys when renumbered, then its `parent` and `order` where they
    /// change. The root may only stay the root.
    pub(in crate::world::systems::gui) fn move_node_writes(
        &self,
        tree: &GuiTreeIndex,
        id: GuiNodeId,
        parent: Option<GuiNodeId>,
        index: usize,
    ) -> Result<Vec<FieldWrite>, ErrorReason> {
        let row = self.tree_row(id).ok_or(ErrorReason::InvalidValue)?;
        let Some(parent) = parent else {
            return if row.parent == 0 {
                Ok(Vec::new())
            } else {
                Err(ErrorReason::InvalidValue)
            };
        };
        self.node_tree().validate_placement(id, parent.0)?;

        let mut writes = Vec::new();
        let order = self.placement_writes(tree, id, parent, index, &mut writes)?;
        if row.parent != parent.0 {
            writes.push(tree_u32(id, GuiNodeTreeProperty::Parent, parent.0)?);
        }
        if row.order != order {
            writes.push(tree_u32(id, GuiNodeTreeProperty::Order, order)?);
        }
        Ok(writes)
    }

    /// The single write retiring a node: clearing its `parent` removes its
    /// subtree's rows from every table, one batch removal per table.
    pub(in crate::world::systems::gui) fn remove_node_write(
        &self,
        id: GuiNodeId,
    ) -> Result<FieldWrite, ErrorReason> {
        if !self.node_tree().is_live(id) {
            return Err(ErrorReason::InvalidValue);
        }
        tree_write(id, GuiNodeTreeProperty::Parent, FieldValue::Unset)
    }

    /// Writes committing a control value when the caller observed the
    /// current revision: the value, then the next revision.
    pub(in crate::world::systems::gui) fn control_value_writes(
        &self,
        id: GuiNodeId,
        expected_revision: u32,
        value: &GuiControlValue,
    ) -> Result<Vec<FieldWrite>, ErrorReason> {
        let commit = GuiControlCommit::new(self, id, expected_revision, value)?;
        let value = match commit.value {
            GuiControlCommitValue::Checked(checked) => FieldWrite {
                offset: GuiRoot::node_data_offset(id, GuiNodeDataProperty::Checked)
                    .ok_or(ErrorReason::InvalidValue)?,
                value: FieldValue::Dynamic(DynamicValue::Bool(checked)),
            },
            GuiControlCommitValue::Value(value) => FieldWrite {
                offset: GuiRoot::node_data_offset(id, GuiNodeDataProperty::Value)
                    .ok_or(ErrorReason::InvalidValue)?,
                value: FieldValue::Dynamic(DynamicValue::F32(value)),
            },
            GuiControlCommitValue::Text(text) => tree_write(
                id,
                GuiNodeTreeProperty::CommittedText,
                FieldValue::String(text),
            )?,
        };
        Ok(vec![
            value,
            tree_u32(id, GuiNodeTreeProperty::Revision, commit.revision)?,
        ])
    }
}

impl GuiRoot {
    /// Writes anchoring a VirtualList at `index`, clamped to its last item,
    /// `offset` logical units into it.
    pub(in crate::world::systems::gui) fn scroll_to_index_writes(
        &self,
        id: GuiNodeId,
        index: u32,
        offset: f32,
    ) -> Result<Vec<FieldWrite>, ErrorReason> {
        let row = self.tree_row(id).ok_or(ErrorReason::InvalidValue)?;
        let values = self.data_row(id).ok_or(ErrorReason::InvalidValue)?;
        if row.node_kind() != Some(GuiNodeKind::VirtualList) || !offset.is_finite() || offset < 0.0
        {
            return Err(ErrorReason::InvalidValue);
        }

        let index = index.min(values.item_count.unwrap_or(0).saturating_sub(1));
        let mut target = values.clone();
        target.anchor_index = Some(index);
        target.anchor_offset = Some(offset);
        let mut writes = Vec::new();
        data_writes(id, values, &target, &mut writes);
        Ok(writes)
    }
}
