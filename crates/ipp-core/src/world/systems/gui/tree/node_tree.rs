//! The root's `node_tree` rows table: one row per live node at slot = node
//! id holding its parent, sparse sibling order key, kind, bounded authored
//! strings and control revision.
//!
//! Child order is derived: siblings order by `(order, id)`. The table stores
//! no child lists, so reordering or reparenting a node writes only its own
//! row; readers use the GUI System's derived
//! [`GuiTreeIndex`](super::GuiTreeIndex) for children.
//!
//! The table is command-owned on a live root. Besides ordinary property
//! writes, it accepts the two structural writes GUI commands need through
//! ordinary field writes: writing `parent` of an absent slot allocates the
//! node's row there, and clearing `parent` of a live node retires it; the
//! root then retires the node's subtree with its other rows (see
//! [`GuiRoot`](super::GuiRoot)). A retired slot is dead and never reused
//! within the root incarnation.

use super::nodes::{GuiNodeData, GuiNodeId, GuiNodeKind, MAX_NODE_ID, MAX_NODES, MAX_TEXT_BYTES};
use crate::components::rows::{Rows, RowsLayout, SchemaRow, SchemaRowsField, row_address};
use crate::components::schema::{ContractSink, FieldError, FieldKind, FieldValue, SchemaField};
use crate::services::asset_management::AssetSource;
use crate::systems::gui::MAX_LAYOUT_DEPTH;
use crate::{DynamicPropertyKind, DynamicValue, ErrorReason};

/// Structure, kind, authored strings and control revision of one node.
///
/// Text properties are bounded by
/// [`MAX_GUI_TEXT_BYTES`](super::MAX_GUI_TEXT_BYTES), so a whole-table write
/// or inspection of a root whose rows carry long strings can exceed the
/// maximum message size and fails explicitly; ordinary roots stay far below
/// it.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct GuiNodeTreeRow {
    /// Parent node identity, or 0 for the root.
    pub parent: u32,
    /// Sparse sibling order key; siblings order by `(order, id)`.
    pub order: u32,
    /// [`GuiNodeKind`] code.
    pub kind: u32,
    /// Text content, button label or text-input authored text; present
    /// exactly for those kinds.
    #[schema(text = 65536)]
    pub text: Option<String>,
    /// Text-input placeholder; present exactly for text inputs.
    #[schema(text = 65536)]
    pub placeholder: Option<String>,
    /// Committed text-input value; present exactly for text inputs.
    #[schema(text = 65536)]
    pub committed_text: Option<String>,
    /// Control revision: zero until the node first becomes a control, then
    /// monotonic for the node lifetime, including while it is not a control,
    /// so a stale write never applies to a later control on the same node.
    pub revision: u32,
}

const _: () = assert!(super::nodes::MAX_GUI_TEXT_BYTES == 65_536);

/// Typed index of one [`GuiNodeTreeRow`] property, in layout order.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiNodeTreeProperty {
    /// `parent`: U32 node identity, 0 for the root.
    Parent = 0,
    /// `order`: U32 sparse sibling key.
    Order,
    /// `kind`: U32 [`GuiNodeKind`] code.
    Kind,
    /// `text`: optional bounded text.
    Text,
    /// `placeholder`: optional bounded text.
    Placeholder,
    /// `committed_text`: optional bounded text; committed control value.
    CommittedText,
    /// `revision`: U32 control revision.
    Revision,
}

impl GuiNodeTreeProperty {
    /// Number of node tree properties.
    pub const COUNT: u32 = 7;

    /// Every property in layout order; `ALL[i] as u32 == i`.
    pub const ALL: [Self; Self::COUNT as usize] = [
        Self::Parent,
        Self::Order,
        Self::Kind,
        Self::Text,
        Self::Placeholder,
        Self::CommittedText,
        Self::Revision,
    ];

    /// Property at a layout index.
    pub const fn from_index(index: u32) -> Option<Self> {
        if index < Self::COUNT {
            Some(Self::ALL[index as usize])
        } else {
            None
        }
    }

    /// Layout index of this property.
    pub const fn index(self) -> u32 {
        self as u32
    }

    /// Row layout name, as generated clients see it.
    pub const fn name(self) -> &'static str {
        GuiNodeTreeRow::LAYOUT.properties[self as usize].name
    }

    /// Exact storage type.
    pub const fn kind(self) -> DynamicPropertyKind {
        GuiNodeTreeRow::LAYOUT.properties[self as usize].kind
    }

    /// Whether a write can change child order: `parent` and `order`.
    pub const fn structural(self) -> bool {
        matches!(self, Self::Parent | Self::Order)
    }
}

impl GuiNodeTreeRow {
    /// Row of a new node with its kind and authored strings. A control starts
    /// at revision 1 with the authored text committed.
    pub fn authored<S: AsRef<str>>(
        parent: Option<GuiNodeId>,
        order: u32,
        data: &GuiNodeData<S>,
    ) -> Self {
        let mut row = Self {
            parent: parent.map_or(0, |parent| parent.0),
            order,
            ..Self::default()
        };
        row.conform(data.kind());
        row.text = data.text().map(str::to_owned);
        row.placeholder = data.placeholder().map(str::to_owned);
        if data.kind().is_text_input() {
            row.committed_text = row.text.clone();
        }
        row
    }

    /// Node kind, or None for an unknown code.
    pub fn node_kind(&self) -> Option<GuiNodeKind> {
        GuiNodeKind::from_code(self.kind)
    }

    /// Kind and authored strings, or None for an unknown kind code.
    pub fn data(&self) -> Option<GuiNodeData<&str>> {
        GuiNodeData::from_row(self)
    }

    /// Parent identity; None for the root.
    pub fn parent(&self) -> Option<GuiNodeId> {
        (self.parent != 0).then_some(GuiNodeId(self.parent))
    }

    /// Make the row's shape match `kind`: the kind code, strings present
    /// exactly for the kinds that carry them (newly present ones empty), and
    /// revision 1 for a node becoming a control for the first time. Kept
    /// strings and revisions are unchanged.
    pub fn conform(&mut self, kind: GuiNodeKind) {
        fn keep(value: &mut Option<String>, present: bool) {
            match (present, value.is_some()) {
                (true, false) => *value = Some(String::new()),
                (false, true) => *value = None,
                _ => {}
            }
        }

        self.kind = kind.code();
        keep(&mut self.text, kind.has_text());
        keep(&mut self.placeholder, kind.is_text_input());
        keep(&mut self.committed_text, kind.is_text_input());
        if kind.is_control() && self.revision == 0 {
            self.revision = 1;
        }
    }

    /// Check the row's own rules: a known kind, strings present exactly for
    /// the kinds that carry them and within the text bound, and a control
    /// revision of at least 1.
    pub fn validate(&self) -> Result<GuiNodeKind, ErrorReason> {
        let kind = self.node_kind().ok_or(ErrorReason::InvalidValue)?;
        let shaped = self.text.is_some() == kind.has_text()
            && self.placeholder.is_some() == kind.is_text_input()
            && self.committed_text.is_some() == kind.is_text_input()
            && (!kind.is_control() || self.revision > 0);
        if !shaped {
            return Err(ErrorReason::InvalidField);
        }

        let bounded = [&self.text, &self.placeholder, &self.committed_text]
            .into_iter()
            .flatten()
            .all(|text| text.len() <= MAX_TEXT_BYTES);
        if bounded {
            Ok(kind)
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// Default gap between consecutive sibling order keys, leaving room for
/// many insertions between neighbours before a renumbering.
pub(in crate::world::systems::gui) const ORDER_SPACING: u32 = 1 << 16;

/// Order keys placing a node at `position` among `siblings`, which are the
/// other children of its parent as `(id, order)` in `(order, id)` order: the
/// node's key, and the siblings whose keys must change. The key lies strictly
/// between the neighbours around `position` when they leave a gap; otherwise
/// every sibling is renumbered evenly with a gap at `position`, the only edit
/// whose size follows the sibling count.
pub(in crate::world::systems::gui) fn place_among(
    siblings: &[(GuiNodeId, u32)],
    position: usize,
) -> (u32, Vec<(GuiNodeId, u32)>) {
    let before = position
        .checked_sub(1)
        .and_then(|index| siblings.get(index))
        .map(|&(_, order)| order);
    let after = siblings.get(position).map(|&(_, order)| order);
    let between = match (before, after) {
        (None, None) => Some(ORDER_SPACING),
        (None, Some(after)) => (after > 0).then_some(after / 2),
        (Some(before), None) => before
            .checked_add(ORDER_SPACING)
            .or_else(|| (before < u32::MAX).then(|| before + (u32::MAX - before).div_ceil(2))),
        (Some(before), Some(after)) => {
            (after > before && after - before >= 2).then(|| before + (after - before) / 2)
        }
    };
    if let Some(key) = between {
        return (key, Vec::new());
    }

    let count = siblings.len() as u32 + 1;
    let spacing = (u32::MAX / (count + 1)).clamp(1, ORDER_SPACING);
    let key_at = |slot: usize| (slot as u32 + 1) * spacing;
    let renumbered = siblings
        .iter()
        .enumerate()
        .filter_map(|(index, &(id, order))| {
            let key = key_at(index + usize::from(index >= position));
            (key != order).then_some((id, key))
        })
        .collect();
    (key_at(position), renumbered)
}

/// Accept a value for one node tree property written as a dynamic value:
/// `parent`, `order`, `kind` and `revision` are U32 and `kind` a known code.
/// Text properties are written as strings, never dynamic values.
pub(crate) fn validate_node_tree_property(
    property: GuiNodeTreeProperty,
    value: &DynamicValue,
) -> Result<(), ErrorReason> {
    if value.kind() != property.kind() || property.kind() == DynamicPropertyKind::Text {
        return Err(ErrorReason::InvalidField);
    }
    match (property, value) {
        (GuiNodeTreeProperty::Kind, DynamicValue::U32(code))
            if GuiNodeKind::from_code(*code).is_none() =>
        {
            Err(ErrorReason::InvalidValue)
        }
        _ => Ok(()),
    }
}

/// The `node_tree` rows table. See the module documentation for its
/// structural writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiNodeTree {
    rows: Rows<GuiNodeTreeRow>,
}

impl GuiNodeTree {
    /// Live rows keyed by node id.
    pub fn rows(&self) -> &Rows<GuiNodeTreeRow> {
        &self.rows
    }

    pub(in crate::world::systems::gui) fn rows_mut(&mut self) -> &mut Rows<GuiNodeTreeRow> {
        &mut self.rows
    }

    /// Row of one live node.
    pub fn get(&self, id: GuiNodeId) -> Option<&GuiNodeTreeRow> {
        self.rows.get(id.0)
    }

    /// Whether a node is live.
    pub fn is_live(&self, id: GuiNodeId) -> bool {
        self.rows.is_live(id.0)
    }

    /// Number of live nodes.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the tree has no live nodes.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Identity the next inserted node must use: the table's next slot,
    /// starting at 1.
    pub fn next_node_id(&self) -> u32 {
        self.rows.next_slot().max(1)
    }

    /// Check where one node's parent write leaves it: the parent is live and
    /// not the node, walking up from it reaches the root within
    /// [`MAX_LAYOUT_DEPTH`] steps without meeting the node, so the node is
    /// neither in a cycle nor deeper than layout evaluates; or the node is
    /// the single root. The walk is conservative: a placement it cannot
    /// confirm within the depth is rejected.
    pub fn validate_parent(&self, id: GuiNodeId) -> Result<(), ErrorReason> {
        let row = self.get(id).ok_or(ErrorReason::InvalidValue)?;
        if row.parent == 0 {
            return self.validate_single_root(id);
        }
        self.validate_placement(id, row.parent)
    }

    /// Check placing `id` under the live node `parent`, which need not be
    /// its current parent: the walk of [`Self::validate_parent`] from
    /// `parent`.
    pub fn validate_placement(&self, id: GuiNodeId, parent: u32) -> Result<(), ErrorReason> {
        let mut current = parent;
        for _ in 0..MAX_LAYOUT_DEPTH {
            if current == id.0 {
                return Err(ErrorReason::InvalidValue);
            }
            let ancestor = self.rows.get(current).ok_or(ErrorReason::InvalidValue)?;
            if ancestor.parent == 0 {
                return Ok(());
            }
            current = ancestor.parent;
        }
        Err(ErrorReason::InvalidValue)
    }

    /// Whether `id` is the only root: alone, or reached from another node.
    fn validate_single_root(&self, id: GuiNodeId) -> Result<(), ErrorReason> {
        let Some((mut current, _)) = self.rows.iter().find(|(slot, _)| *slot != id.0) else {
            return Ok(());
        };
        for _ in 0..=MAX_LAYOUT_DEPTH {
            if current == id.0 {
                return Ok(());
            }
            let row = self.rows.get(current).ok_or(ErrorReason::InvalidValue)?;
            if row.parent == 0 {
                return Err(ErrorReason::InvalidValue);
            }
            current = row.parent;
        }
        Err(ErrorReason::InvalidValue)
    }

    /// Check the complete table in one pass: identity bounds, every row's own
    /// rules, one root, live parents and one acyclic tree reaching every
    /// node. Depth is unbounded here; per-write checks bound only the
    /// placements they write.
    pub fn validate(&self) -> Result<(), ErrorReason> {
        if self.rows.len() > MAX_NODES
            || self.rows.next_slot() > MAX_NODE_ID
            || self.rows.is_live(0)
        {
            return Err(ErrorReason::InvalidValue);
        }

        let mut links = Vec::with_capacity(self.rows.len());
        let mut root = None;
        for (slot, row) in self.rows.iter() {
            row.validate()?;
            if row.parent == 0 {
                if root.replace(slot).is_some() {
                    return Err(ErrorReason::InvalidValue);
                }
            } else if row.parent == slot || !self.rows.is_live(row.parent) {
                return Err(ErrorReason::InvalidValue);
            } else {
                links.push((row.parent, slot));
            }
        }
        let Some(root) = root else {
            return if self.rows.is_empty() {
                Ok(())
            } else {
                Err(ErrorReason::InvalidValue)
            };
        };

        // Every node has one live parent, so a walk from the root reaching
        // every node proves the links form one acyclic tree.
        links.sort_unstable();
        let mut reached = 1;
        let mut stack = vec![root];
        while let Some(parent) = stack.pop() {
            let start = links.partition_point(|&(link, _)| link < parent);
            let end = links.partition_point(|&(link, _)| link <= parent);
            reached += end - start;
            stack.extend(links[start..end].iter().map(|&(_, child)| child));
        }
        if reached == self.rows.len() {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }

    /// Live nodes in the subtree of `id`, excluding `id`, found from the
    /// parent links in one sorted pass. Used after `id` retired, when the
    /// table no longer holds its row.
    pub(in crate::world::systems::gui) fn descendants(&self, id: GuiNodeId) -> Vec<u32> {
        let mut links: Vec<(u32, u32)> = self
            .rows
            .iter()
            .filter(|(_, row)| row.parent != 0)
            .map(|(slot, row)| (row.parent, slot))
            .collect();
        links.sort_unstable();

        let mut descendants = Vec::new();
        let mut stack = vec![id.0];
        while let Some(parent) = stack.pop() {
            let start = links.partition_point(|&(link, _)| link < parent);
            let end = links.partition_point(|&(link, _)| link <= parent);
            for &(_, child) in &links[start..end] {
                descendants.push(child);
                stack.push(child);
            }
        }
        descendants
    }

    /// Read-only node view.
    pub fn nodes(&self) -> GuiNodes<'_> {
        GuiNodes {
            tree: self,
        }
    }
}

impl SchemaField for GuiNodeTree {
    const KIND: FieldKind = FieldKind::Rows;

    fn to_value(&self) -> FieldValue {
        self.rows.to_value()
    }

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        Ok(Self {
            rows: Rows::from_value(value)?,
        })
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        self.rows.write_default(sink);
    }

    fn retained_bytes(&self) -> Option<usize> {
        Some(self.rows.retained_bytes())
    }
}

impl SchemaRowsField for GuiNodeTree {
    const LAYOUT: RowsLayout = GuiNodeTreeRow::LAYOUT;

    fn has_row_field(relative: u32) -> bool {
        Rows::<GuiNodeTreeRow>::has_row_field(relative)
    }

    /// An absent node reads its `parent` as unset, so an allocating write
    /// reports the change it makes.
    fn row_field(&self, relative: u32) -> Result<FieldValue, FieldError> {
        let address =
            row_address(relative, GuiNodeTreeProperty::COUNT).ok_or(FieldError::UnknownField)?;
        if address.property == GuiNodeTreeProperty::Parent.index()
            && !self.rows.is_live(address.slot)
        {
            return Ok(FieldValue::Unset);
        }
        self.rows.row_field(relative)
    }

    /// Writing `parent` of an absent slot allocates its row with every other
    /// property defaulted; clearing `parent` of a live node retires its row.
    /// Dead slots reject both, so identities are never reused.
    fn set_row_field(&mut self, relative: u32, value: FieldValue) -> Result<(), FieldError> {
        let address =
            row_address(relative, GuiNodeTreeProperty::COUNT).ok_or(FieldError::UnknownField)?;
        if address.property == GuiNodeTreeProperty::Parent.index() {
            let live = self.rows.is_live(address.slot);
            match (live, value) {
                (false, FieldValue::Dynamic(DynamicValue::U32(parent))) if address.slot != 0 => {
                    return self.rows.insert(
                        address.slot,
                        GuiNodeTreeRow {
                            parent,
                            ..GuiNodeTreeRow::default()
                        },
                    );
                }
                (true, FieldValue::Unset) => {
                    self.rows.remove(address.slot);
                    return Ok(());
                }
                (false, _) => return Err(FieldError::UnknownField),
                (true, value) => return self.rows.set_row_field(relative, value),
            }
        }
        self.rows.set_row_field(relative, value)
    }

    fn validate_row_field(relative: u32, kind: FieldKind) -> Result<(), FieldError> {
        let address =
            row_address(relative, GuiNodeTreeProperty::COUNT).ok_or(FieldError::UnknownField)?;
        if address.property == GuiNodeTreeProperty::Parent.index() && kind == FieldKind::Unset {
            return Ok(());
        }
        Rows::<GuiNodeTreeRow>::validate_row_field(relative, kind)
    }

    fn visit_assets(&self, _visit: &mut dyn FnMut(&AssetSource)) {}
}

/// Read-only view of a root's live nodes.
#[derive(Clone, Copy, Debug)]
pub struct GuiNodes<'a> {
    tree: &'a GuiNodeTree,
}

impl<'a> GuiNodes<'a> {
    /// One live node by identity.
    pub fn node(&self, id: GuiNodeId) -> Option<GuiNode<'a>> {
        let row = self.tree.get(id)?;
        Some(GuiNode {
            id,
            parent: row.parent(),
            order: row.order,
            data: row.data()?,
        })
    }

    /// Number of live nodes.
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    /// Whether the tree has no live nodes.
    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    /// Identity the next inserted node must use.
    pub fn next_node_id(&self) -> u32 {
        self.tree.next_node_id()
    }

    /// Live nodes in identity order.
    pub fn iter(&self) -> impl Iterator<Item = GuiNode<'a>> + 'a {
        self.tree.rows.iter().filter_map(|(slot, row)| {
            Some(GuiNode {
                id: GuiNodeId(slot),
                parent: row.parent(),
                order: row.order,
                data: row.data()?,
            })
        })
    }
}

/// Borrowed view of one live node. Children come from the derived
/// [`GuiTreeIndex`](super::GuiTreeIndex).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiNode<'a> {
    /// Never-reused root-local node identity.
    pub id: GuiNodeId,
    /// Logical parent node, or None for the root.
    pub parent: Option<GuiNodeId>,
    /// Sparse sibling order key.
    pub order: u32,
    /// Node kind and authored strings.
    pub data: GuiNodeData<&'a str>,
}

#[cfg(test)]
#[path = "node_tree_tests.rs"]
mod tests;
