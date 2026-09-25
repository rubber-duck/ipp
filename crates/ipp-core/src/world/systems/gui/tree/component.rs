use super::controls::GuiControlState;
use super::node_rows::{
    GuiNodeDataProperty, GuiNodeDataRow, GuiNodePropertyRef, GuiNodeRowProperty,
    GuiNodeStyleProperty, GuiNodeStyleRow, validate_node_data_property,
    validate_node_style_property,
};
#[cfg(test)]
use super::node_tree::place_among;
use super::node_tree::{
    GuiNodeTree, GuiNodeTreeProperty, GuiNodeTreeRow, GuiNodes, validate_node_tree_property,
};
#[cfg(test)]
use super::nodes::GuiNodeData;
use super::nodes::{
    GuiControlValue, GuiNodeId, GuiNodeKind, GuiNodePatch, GuiNodeStyle, MAX_NODE_ID, MAX_NODES,
    MAX_TEXT_BYTES, valid_node_data,
};
use super::part_rows::{
    GUI_BASE_PARTS, GuiPartId, GuiPartPatch, GuiPartProperty, GuiPartRow, GuiPartRowProperty,
    GuiThemePartRow, base_part_index, validate_part_property, validate_part_row_property,
};
use crate::components::rows::{RowAddress, Rows, SchemaRow, row_address, row_region_relative};
use crate::components::schema::ComponentLifecycle;
use crate::systems::gui::DEFAULT_UNITS_PER_METRE;
use crate::systems::surface::GuiPrimitivePart;
use crate::{DynamicProperties, DynamicValue, ErrorReason, FieldValue, FieldWrite};
use ipp_schema_derive::SchemaComponent;
use std::collections::{BTreeMap, BTreeSet};

/// Root GUI component owning the content of its entity's Surface.
///
/// The tree is the `node_tree` rows table: each live node owns one
/// `node_tree` row (parent, sparse sibling order, kind, bounded authored
/// strings, committed text and control revision), one `node_style` row and
/// one `node_data` row, all at slot = node id. Child order is derived from
/// `(order, id)` by the GUI System. While a root incarnation is live only
/// GuiCommand writes the tree, the committed checkbox and slider values and
/// the slider range; a new incarnation may supply them whole. Allocating a
/// node's tree row inserts its default style and data rows, retiring it
/// removes its subtree's rows from every table in one batch per table, and
/// writing its kind conforms its strings and data row, so row properties are
/// addressed by offset only while their node lives. Style properties and
/// `image_size` are ordinary numeric properties.
///
/// Skins are root-owned themes in `theme_parts`, referenced by each node's
/// `theme` style property, and per-node `part_state` rows holding appearance
/// overrides and the live channels skin transitions animate. The root keeps a
/// node's channels present exactly while its theme declares motion for the
/// part, so writing a theme reference, a theme's motion or either table
/// re-derives them. Theme updates never rewrite referencing nodes.
///
/// `units_per_metre` is the root's logical density: GUI logical units per
/// Surface metre, a finite positive ordinary property that clients write,
/// animate or overlay per root. Changing it reflows the root.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct GuiRoot {
    /// Node style rows at slot = node id; rows field 0.
    #[schema(rows)]
    node_style: Rows<GuiNodeStyleRow>,
    /// Scalar kind-specific node data at slot = node id; rows field 1.
    #[schema(rows)]
    node_data: Rows<GuiNodeDataRow>,
    /// Theme part rows at slot = theme slot * [`GuiPartId::COUNT`] + part;
    /// rows field 2.
    #[schema(rows)]
    theme_parts: Rows<GuiThemePartRow>,
    /// Per-node part rows keyed by their `node` and `part` properties at
    /// monotonically allocated slots; rows field 3.
    #[schema(rows)]
    part_state: Rows<GuiPartRow>,
    /// Node structure, kind and strings at slot = node id; rows field 4.
    #[schema(rows)]
    node_tree: GuiNodeTree,
    /// Logical GUI units per Surface metre; finite and positive, default
    /// [`DEFAULT_UNITS_PER_METRE`].
    pub units_per_metre: f32,
    /// Application extension values; the runtime writes no GUI names here.
    #[schema(ignore)]
    pub properties: DynamicProperties,
    /// Theme and part row lookup, rebuilt whenever either table is replaced.
    #[schema(ignore)]
    skin_index: GuiSkinIndex,
}

impl Default for GuiRoot {
    fn default() -> Self {
        Self {
            node_style: Default::default(),
            node_data: Default::default(),
            theme_parts: Default::default(),
            part_state: Default::default(),
            node_tree: Default::default(),
            units_per_metre: DEFAULT_UNITS_PER_METRE,
            properties: Default::default(),
            skin_index: Default::default(),
        }
    }
}

/// Lookup from theme handles and (node, base part) keys to row slots,
/// derived from the tables' key properties.
#[derive(Clone, Debug, Default, PartialEq)]
struct GuiSkinIndex {
    /// Theme handle to theme slot.
    themes: BTreeMap<u32, u32>,
    /// (node, base part index) to `part_state` slot.
    parts: BTreeMap<(u32, u32), u32>,
}

/// A GuiRoot row property addressed by a field offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiRootRowProperty {
    /// A `node_style`, `node_data` or `node_tree` property of one node.
    Node(GuiNodePropertyRef),
    /// A `theme_parts` property; None is the `theme` key.
    Theme {
        /// Row slot.
        slot: u32,
        /// Part property, or None for the key.
        property: Option<GuiPartProperty>,
    },
    /// A `part_state` property.
    Part {
        /// Row slot.
        slot: u32,
        /// Override, channel or key.
        property: GuiPartRowProperty,
    },
}

impl GuiRoot {
    /// Rows field index of `node_style`; its region starts at `0x1000_0000`.
    pub const NODE_STYLE_FIELD: usize = 0;

    /// Rows field index of `node_data`; its region starts at `0x2000_0000`.
    pub const NODE_DATA_FIELD: usize = 1;

    /// Rows field index of `theme_parts`; its region starts at `0x3000_0000`.
    pub const THEME_PARTS_FIELD: usize = 2;

    /// Rows field index of `part_state`; its region starts at `0x4000_0000`.
    pub const PART_STATE_FIELD: usize = 3;

    /// Rows field index of `node_tree`; its region starts at `0x5000_0000`.
    pub const NODE_TREE_FIELD: usize = 4;

    /// Field offset of one node style property, or None for a node id the
    /// region cannot address.
    pub const fn node_style_offset(node: GuiNodeId, property: GuiNodeStyleProperty) -> Option<u32> {
        Rows::<GuiNodeStyleRow>::offset(Self::NODE_STYLE_FIELD, node.0, property.index())
    }

    /// Field offset of one node data property, or None for a node id the
    /// region cannot address.
    pub const fn node_data_offset(node: GuiNodeId, property: GuiNodeDataProperty) -> Option<u32> {
        Rows::<GuiNodeDataRow>::offset(Self::NODE_DATA_FIELD, node.0, property.index())
    }

    /// Field offset of one node tree property, or None for a node id the
    /// region cannot address.
    pub const fn node_tree_offset(node: GuiNodeId, property: GuiNodeTreeProperty) -> Option<u32> {
        Rows::<GuiNodeTreeRow>::offset(Self::NODE_TREE_FIELD, node.0, property.index())
    }

    /// Row slot of one part of a theme slot.
    pub const fn theme_part_slot(theme_slot: u32, part: GuiPartId) -> Option<u32> {
        let Some(index) = part.index() else {
            return None;
        };
        let Some(base) = theme_slot.checked_mul(GuiPartId::COUNT) else {
            return None;
        };
        base.checked_add(index)
    }

    /// Field offset of one property of a theme part row slot.
    pub const fn theme_part_offset(slot: u32, property: GuiPartProperty) -> Option<u32> {
        Rows::<GuiThemePartRow>::offset(Self::THEME_PARTS_FIELD, slot, property.index())
    }

    /// Field offset of one property index of a `part_state` row slot.
    pub const fn part_row_offset(slot: u32, property: u32) -> Option<u32> {
        Rows::<GuiPartRow>::offset(Self::PART_STATE_FIELD, slot, property)
    }

    /// Node and row property addressed by a field offset; None outside the
    /// three node regions and for node id zero, which is never valid.
    pub const fn node_property(offset: u32) -> Option<GuiNodePropertyRef> {
        let (slot, property) =
            if let Some(relative) = row_region_relative(offset, Self::NODE_STYLE_FIELD) {
                let Some(RowAddress {
                    slot,
                    property,
                }) = row_address(relative, GuiNodeStyleProperty::COUNT)
                else {
                    return None;
                };
                let Some(property) = GuiNodeStyleProperty::from_index(property) else {
                    return None;
                };
                (slot, GuiNodeRowProperty::Style(property))
            } else if let Some(relative) = row_region_relative(offset, Self::NODE_DATA_FIELD) {
                let Some(RowAddress {
                    slot,
                    property,
                }) = row_address(relative, GuiNodeDataProperty::COUNT)
                else {
                    return None;
                };
                let Some(property) = GuiNodeDataProperty::from_index(property) else {
                    return None;
                };
                (slot, GuiNodeRowProperty::Data(property))
            } else if let Some(relative) = row_region_relative(offset, Self::NODE_TREE_FIELD) {
                let Some(RowAddress {
                    slot,
                    property,
                }) = row_address(relative, GuiNodeTreeProperty::COUNT)
                else {
                    return None;
                };
                let Some(property) = GuiNodeTreeProperty::from_index(property) else {
                    return None;
                };
                (slot, GuiNodeRowProperty::Tree(property))
            } else {
                return None;
            };

        if slot == 0 {
            return None;
        }

        Some(GuiNodePropertyRef {
            node: GuiNodeId(slot),
            property,
        })
    }

    /// Row property of any GuiRoot table addressed by a field offset.
    pub const fn row_property(offset: u32) -> Option<GuiRootRowProperty> {
        if let Some(reference) = Self::node_property(offset) {
            return Some(GuiRootRowProperty::Node(reference));
        }
        if let Some(relative) = row_region_relative(offset, Self::THEME_PARTS_FIELD) {
            let Some(RowAddress {
                slot,
                property,
            }) = row_address(relative, GuiThemePartRow::THEME + 1)
            else {
                return None;
            };
            return Some(GuiRootRowProperty::Theme {
                slot,
                property: GuiPartProperty::from_index(property),
            });
        }
        if let Some(relative) = row_region_relative(offset, Self::PART_STATE_FIELD) {
            let Some(RowAddress {
                slot,
                property,
            }) = row_address(relative, GuiPartRowProperty::COUNT)
            else {
                return None;
            };
            let Some(property) = GuiPartRowProperty::from_index(property) else {
                return None;
            };
            return Some(GuiRootRowProperty::Part {
                slot,
                property,
            });
        }
        None
    }

    /// Whether numeric animation and overlays may target an offset: numeric
    /// node style properties, `image_size`, numeric theme part properties and
    /// numeric part overrides and channels. False for `enabled`, `asset`,
    /// `theme`, the command-owned control values, slider range, tree rows and
    /// row keys, asset references and any other offset.
    pub const fn numeric_animatable(offset: u32) -> bool {
        match Self::row_property(offset) {
            Some(GuiRootRowProperty::Node(reference)) => reference.property.numeric_animatable(),
            Some(GuiRootRowProperty::Theme {
                property: Some(property),
                ..
            }) => property.numeric_animatable(),
            Some(GuiRootRowProperty::Part {
                property,
                ..
            }) => property.numeric_animatable(),
            Some(GuiRootRowProperty::Theme {
                property: None,
                ..
            })
            | None => false,
        }
    }

    /// Accept a value for the node property at `offset`: exact type, finite
    /// and the property's own range. Row-wide rules (presence by node kind,
    /// slider `min <= value <= max`, tree placement) are checked on the row.
    pub fn validate_node_property(offset: u32, value: &DynamicValue) -> Result<(), ErrorReason> {
        match Self::node_property(offset)
            .ok_or(ErrorReason::InvalidField)?
            .property
        {
            GuiNodeRowProperty::Style(property) => validate_node_style_property(property, value),
            GuiNodeRowProperty::Data(property) => validate_node_data_property(property, value),
            GuiNodeRowProperty::Tree(property) => validate_node_tree_property(property, value),
        }
    }

    /// Accept a value for any non-key row property at `offset`: exact type,
    /// finite and the property's own range.
    pub fn validate_row_value(offset: u32, value: &DynamicValue) -> Result<(), ErrorReason> {
        match Self::row_property(offset).ok_or(ErrorReason::InvalidField)? {
            GuiRootRowProperty::Node(_) => Self::validate_node_property(offset, value),
            GuiRootRowProperty::Theme {
                property: Some(property),
                ..
            } => validate_part_property(property, value),
            GuiRootRowProperty::Part {
                property: GuiPartRowProperty::Override(property),
                ..
            } => validate_part_property(property, value),
            GuiRootRowProperty::Part {
                property: GuiPartRowProperty::Channel(channel),
                ..
            } => validate_part_property(channel.property(), value),
            GuiRootRowProperty::Theme {
                property: None,
                ..
            }
            | GuiRootRowProperty::Part {
                property: GuiPartRowProperty::Key,
                ..
            } => Err(ErrorReason::InvalidField),
        }
    }

    /// Node style rows keyed by node id.
    pub fn node_style(&self) -> &Rows<GuiNodeStyleRow> {
        &self.node_style
    }

    /// Scalar node data rows keyed by node id, including committed checkbox
    /// and slider values.
    pub fn node_data(&self) -> &Rows<GuiNodeDataRow> {
        &self.node_data
    }

    /// Node structure, kind, strings and control revisions keyed by node id.
    pub fn node_tree(&self) -> &GuiNodeTree {
        &self.node_tree
    }

    /// Theme part rows keyed by theme slot and part.
    pub fn theme_parts(&self) -> &Rows<GuiThemePartRow> {
        &self.theme_parts
    }

    /// Per-node part rows.
    pub fn part_state(&self) -> &Rows<GuiPartRow> {
        &self.part_state
    }

    /// Slot of a live theme, the base of its part rows.
    pub fn theme_slot(&self, theme: u32) -> Option<u32> {
        self.skin_index.themes.get(&theme).copied()
    }

    /// Live theme handles in ascending order.
    pub fn themes(&self) -> impl Iterator<Item = u32> + '_ {
        self.skin_index.themes.keys().copied()
    }

    /// One part row of a theme slot.
    pub fn theme_row(&self, theme_slot: u32, part: GuiPartId) -> Option<&GuiThemePartRow> {
        self.theme_parts
            .get(Self::theme_part_slot(theme_slot, part)?)
    }

    /// Theme slot a live node references, or None without a live theme.
    pub fn node_theme_slot(&self, node: GuiNodeId) -> Option<u32> {
        self.theme_slot(self.node_style.get(node.0)?.theme?)
    }

    /// Slot and row of one node's base part.
    pub fn part_row(&self, node: GuiNodeId, part: GuiPrimitivePart) -> Option<(u32, &GuiPartRow)> {
        let slot = *self
            .skin_index
            .parts
            .get(&(node.0, base_part_index(part)))?;
        Some((slot, self.part_state.get(slot)?))
    }

    /// First theme slot no theme has used in this incarnation.
    fn next_theme_slot(&self) -> u32 {
        self.theme_parts.next_slot().div_ceil(GuiPartId::COUNT)
    }

    /// Real offset of the root density `units_per_metre`.
    pub(crate) const fn units_per_metre_field() -> u32 {
        std::mem::offset_of!(Self, units_per_metre) as u32
    }

    /// Whether `units` is an accepted root density: finite and positive.
    fn valid_units_per_metre(units: f32) -> bool {
        units.is_finite() && units > 0.0
    }

    /// Real offset of the whole `node_tree` table.
    pub(in crate::world::systems::gui) const fn node_tree_field() -> u32 {
        std::mem::offset_of!(Self, node_tree) as u32
    }

    /// Real offset of the whole `node_style` table.
    pub(in crate::world::systems::gui) const fn node_style_field() -> u32 {
        std::mem::offset_of!(Self, node_style) as u32
    }

    /// Real offset of the whole `node_data` table.
    pub(in crate::world::systems::gui) const fn node_data_field() -> u32 {
        std::mem::offset_of!(Self, node_data) as u32
    }

    /// Real offset of the whole `theme_parts` table.
    pub(in crate::world::systems::gui) const fn theme_parts_field() -> u32 {
        std::mem::offset_of!(Self, theme_parts) as u32
    }

    /// Real offset of the whole `part_state` table.
    pub(in crate::world::systems::gui) const fn part_state_field() -> u32 {
        std::mem::offset_of!(Self, part_state) as u32
    }

    /// Whether only GUI commands may write `offset` on a live root: every
    /// whole table, the tree rows, committed control values, the slider
    /// range, node theme references and row keys.
    pub fn command_owned_field(offset: u32) -> bool {
        offset == Self::node_tree_field()
            || offset == Self::node_style_field()
            || offset == Self::node_data_field()
            || offset == Self::theme_parts_field()
            || offset == Self::part_state_field()
            || match Self::row_property(offset) {
                Some(GuiRootRowProperty::Node(GuiNodePropertyRef {
                    property: GuiNodeRowProperty::Data(property),
                    ..
                })) => property.command_owned(),
                Some(GuiRootRowProperty::Node(GuiNodePropertyRef {
                    property: GuiNodeRowProperty::Style(property),
                    ..
                })) => property == GuiNodeStyleProperty::Theme,
                Some(GuiRootRowProperty::Node(GuiNodePropertyRef {
                    property: GuiNodeRowProperty::Tree(_),
                    ..
                })) => true,
                Some(GuiRootRowProperty::Theme {
                    property,
                    ..
                }) => property.is_none(),
                Some(GuiRootRowProperty::Part {
                    property,
                    ..
                }) => matches!(property, GuiPartRowProperty::Key),
                None => false,
            }
    }

    /// Read-only view of the live nodes.
    pub fn nodes(&self) -> GuiNodes<'_> {
        self.node_tree.nodes()
    }

    /// Tree row of one live node.
    pub fn tree_row(&self, id: GuiNodeId) -> Option<&GuiNodeTreeRow> {
        self.node_tree.get(id)
    }

    /// Identity a caller must use for the next node insertion.
    pub fn next_node_id(&self) -> u32 {
        self.node_tree.next_node_id()
    }

    /// Committed control value and revision of one node; None for nodes that
    /// were never controls.
    pub fn control_state(&self, id: GuiNodeId) -> Option<GuiControlState> {
        let revision = self.control_revision(id);
        (revision > 0).then(|| GuiControlState {
            value: self.control_value(id),
            revision,
        })
    }

    /// Control revision of one node; zero for nodes that were never controls.
    pub fn control_revision(&self, id: GuiNodeId) -> u32 {
        self.node_tree.get(id).map_or(0, |row| row.revision)
    }

    /// Committed text of a text input.
    pub fn committed_text(&self, id: GuiNodeId) -> Option<&str> {
        self.node_tree.get(id)?.committed_text.as_deref()
    }

    /// Committed control value of one node; None for non-control nodes.
    pub fn control_value(&self, id: GuiNodeId) -> GuiControlValue {
        let Some(row) = self.node_tree.get(id) else {
            return GuiControlValue::None;
        };
        let data = self.node_data.get(id.0);
        match row.node_kind() {
            Some(GuiNodeKind::Checkbox) => data
                .and_then(|row| row.checked)
                .map_or(GuiControlValue::None, GuiControlValue::Bool),
            Some(GuiNodeKind::Slider) => data
                .and_then(|row| row.value)
                .map_or(GuiControlValue::None, GuiControlValue::Scalar),
            Some(GuiNodeKind::TextInput) => row
                .committed_text
                .clone()
                .map_or(GuiControlValue::None, GuiControlValue::Text),
            _ => GuiControlValue::None,
        }
    }

    /// Style row of one node, read in place.
    pub fn style_row(&self, id: GuiNodeId) -> Option<&GuiNodeStyleRow> {
        self.node_style.get(id.0)
    }

    /// Kind-specific scalar row of one node, read in place.
    pub fn data_row(&self, id: GuiNodeId) -> Option<&GuiNodeDataRow> {
        self.node_data.get(id.0)
    }

    /// Visual translation and axis-aligned scale for one node. A missing row
    /// reads as identity; layout rejects singular (zero) scales with a
    /// diagnostic instead of flowing them into paint or hit testing. These
    /// properties never reflow: they move paint and hit regions together.
    pub fn visual_transform(&self, id: GuiNodeId) -> ([f32; 2], [f32; 2]) {
        self.node_style
            .get(id.0)
            .map_or(([0.0, 0.0], [1.0, 1.0]), |row| (row.position, row.scale))
    }

    /// Copy the authoritative style of one live node.
    pub fn style(&self, id: GuiNodeId) -> Option<GuiNodeStyle> {
        if !self.node_tree.is_live(id) {
            return None;
        }
        Some(
            self.node_style
                .get(id.0)
                .map(GuiNodeStyle::from)
                .unwrap_or_default(),
        )
    }

    /// Ordered `(id, order)` of the live children of `parent` (0 for the
    /// root position), excluding `except`, from one pass over the tree rows.
    /// Direct edits use it; GUI commands read the System's derived index.
    #[cfg(test)]
    fn scanned_siblings(&self, parent: u32, except: Option<GuiNodeId>) -> Vec<(GuiNodeId, u32)> {
        let mut siblings: Vec<(u32, GuiNodeId)> = self
            .node_tree
            .rows()
            .iter()
            .filter(|(slot, row)| row.parent == parent && Some(GuiNodeId(*slot)) != except)
            .map(|(slot, row)| (row.order, GuiNodeId(slot)))
            .collect();
        siblings.sort_unstable();
        siblings
            .into_iter()
            .map(|(order, id)| (id, order))
            .collect()
    }

    /// Place `id` at `index` among the scanned children of `parent`,
    /// renumbering them when their keys leave no gap, and return its key.
    #[cfg(test)]
    fn place_directly(&mut self, id: GuiNodeId, parent: u32, index: usize) -> u32 {
        let siblings = self.scanned_siblings(parent, Some(id));
        let (key, renumbered) = place_among(&siblings, index.min(siblings.len()));
        for (sibling, order) in renumbered {
            if let Some(row) = self.node_tree.rows_mut().get_mut(sibling.0) {
                row.order = order;
            }
        }
        key
    }

    /// Insert one node with its complete style and kind-specific data, and
    /// the live channels its theme reference needs, directly into the rows.
    /// Control nodes start at revision 1 with the authored value committed.
    /// Identities at or past [`MAX_GUI_NODE_ID`](super::nodes::MAX_GUI_NODE_ID)
    /// fail with `Capacity`; a new root incarnation starts identities anew.
    /// GUI commands reach the same result through row writes.
    #[cfg(test)]
    pub(in crate::world::systems::gui) fn insert_node(
        &mut self,
        id: GuiNodeId,
        parent: Option<GuiNodeId>,
        index: usize,
        data: GuiNodeData,
        values: GuiNodeDataRow,
        style: &GuiNodeStyle,
    ) -> Result<(), ErrorReason> {
        if id.0 >= MAX_NODE_ID {
            return Err(ErrorReason::Capacity);
        }
        if self.node_tree.len() >= MAX_NODES
            || id.0 != self.next_node_id()
            || !valid_node_data(&data)
        {
            return Err(ErrorReason::InvalidValue);
        }
        let style = GuiNodeStyleRow::from(style);
        style.validate()?;
        values.validate_for(data.kind())?;
        match parent {
            None if !self.node_tree.is_empty() => return Err(ErrorReason::InvalidValue),
            None => {}
            Some(parent) => self.node_tree.validate_placement(id, parent.0)?,
        }

        let parent_slot = parent.map_or(0, |parent| parent.0);
        let order = match parent {
            Some(_) => self.place_directly(id, parent_slot, index),
            None => 0,
        };
        self.node_tree
            .rows_mut()
            .insert(id.0, GuiNodeTreeRow::authored(parent, order, &data))
            .map_err(field_error)?;
        self.node_style.insert(id.0, style).map_err(field_error)?;
        self.node_data.insert(id.0, values).map_err(field_error)?;
        self.sync_node_channels(id.0);
        Ok(())
    }

    /// Insert one node at an explicit sibling order key, without scanning
    /// its siblings, so fixtures build large trees in linear time.
    #[cfg(test)]
    pub(in crate::world::systems::gui) fn insert_node_at(
        &mut self,
        id: GuiNodeId,
        parent: Option<GuiNodeId>,
        order: u32,
        data: GuiNodeData,
        values: GuiNodeDataRow,
        style: &GuiNodeStyle,
    ) -> Result<(), ErrorReason> {
        self.node_tree
            .rows_mut()
            .insert(id.0, GuiNodeTreeRow::authored(parent, order, &data))
            .map_err(field_error)?;
        self.node_style
            .insert(id.0, GuiNodeStyleRow::from(style))
            .map_err(field_error)?;
        self.node_data.insert(id.0, values).map_err(field_error)?;
        self.sync_node_channels(id.0);
        Ok(())
    }

    /// Move a node to a new parent or reorder it among its siblings,
    /// directly in the rows, without changing identity. The root may only
    /// stay the root.
    #[cfg(test)]
    pub(in crate::world::systems::gui) fn move_node(
        &mut self,
        id: GuiNodeId,
        parent: Option<GuiNodeId>,
        index: usize,
    ) -> Result<(), ErrorReason> {
        let row = self.node_tree.get(id).ok_or(ErrorReason::InvalidValue)?;
        let Some(parent) = parent else {
            return if row.parent == 0 {
                Ok(())
            } else {
                Err(ErrorReason::InvalidValue)
            };
        };
        self.node_tree.validate_placement(id, parent.0)?;

        let order = self.place_directly(id, parent.0, index);
        let row = self
            .node_tree
            .rows_mut()
            .get_mut(id.0)
            .ok_or(ErrorReason::InvalidValue)?;
        row.parent = parent.0;
        row.order = order;
        Ok(())
    }

    /// Apply a node patch directly to the rows. Style members replace row
    /// properties; a data or values change keeps a compatible committed
    /// value, rejects a same-kind edit that would invalidate it, and
    /// establishes the new kind's initial state at the next revision when the
    /// kind changes to or from a control.
    #[cfg(test)]
    pub(in crate::world::systems::gui) fn update_node(
        &mut self,
        id: GuiNodeId,
        patch: &GuiNodePatch,
    ) -> Result<(), ErrorReason> {
        let previous = self
            .node_tree
            .get(id)
            .ok_or(ErrorReason::InvalidValue)?
            .clone();
        if patch.data.is_some() || patch.values.is_some() {
            let current = self.node_data.get(id.0).cloned().unwrap_or_default();
            let edit = GuiNodeDataEdit::new(&previous, &current, patch)?;
            *self
                .node_tree
                .rows_mut()
                .get_mut(id.0)
                .ok_or(ErrorReason::InvalidValue)? = edit.row;
            *self
                .node_data
                .get_mut(id.0)
                .ok_or(ErrorReason::InvalidValue)? = edit.values;
        }

        let row = self
            .node_style
            .get_mut(id.0)
            .ok_or(ErrorReason::InvalidValue)?;
        row.apply(patch);
        row.validate()?;
        if patch.theme.is_some() {
            self.sync_node_channels(id.0);
        }
        Ok(())
    }

    /// Remove a node and its subtree with their node and part rows directly,
    /// one batch removal per table, returning the removed identities with
    /// `id` first.
    #[cfg(test)]
    pub(in crate::world::systems::gui) fn remove_node(
        &mut self,
        id: GuiNodeId,
    ) -> Result<Vec<GuiNodeId>, ErrorReason> {
        if !self.node_tree.is_live(id) {
            return Err(ErrorReason::InvalidValue);
        }
        let mut removed = vec![id.0];
        removed.extend(self.node_tree.descendants(id));
        self.retire_rows(&removed);
        Ok(removed.into_iter().map(GuiNodeId).collect())
    }

    /// Remove the rows of `nodes` from every node table and their part rows,
    /// one batch removal per table.
    fn retire_rows(&mut self, nodes: &[u32]) {
        self.node_tree.rows_mut().remove_slots(nodes);
        self.node_style.remove_slots(nodes);
        self.node_data.remove_slots(nodes);
        self.prune_node_part_rows(nodes);
    }

    /// Commit a control value directly when the caller observed the current
    /// revision.
    pub(in crate::world::systems::gui) fn set_control_value(
        &mut self,
        id: GuiNodeId,
        expected_revision: u32,
        value: &GuiControlValue,
    ) -> Result<(), ErrorReason> {
        let commit = GuiControlCommit::new(self, id, expected_revision, value)?;
        let row = self
            .node_tree
            .rows_mut()
            .get_mut(id.0)
            .ok_or(ErrorReason::InvalidValue)?;
        row.revision = commit.revision;
        if let GuiControlCommitValue::Text(text) = &commit.value {
            row.committed_text = Some(text.clone());
        }
        let data = self
            .node_data
            .get_mut(id.0)
            .ok_or(ErrorReason::InvalidValue)?;
        match commit.value {
            GuiControlCommitValue::Checked(checked) => data.checked = Some(checked),
            GuiControlCommitValue::Value(value) => data.value = Some(value),
            GuiControlCommitValue::Text(_) => {}
        }
        Ok(())
    }

    /// Give every node its rows, drop the rows of nodes no longer in the tree
    /// and conform each data row to its node's kind, after the whole tree
    /// table was replaced. New rows start from the default style; data rows
    /// keep their values for properties the kind still uses, gain
    /// placeholders for newly used ones and drop the rest. A slot that died
    /// earlier in this incarnation stays without a row, which validation
    /// rejects. Part rows of removed nodes go with them.
    fn sync_rows(&mut self) {
        let live: Vec<u32> = self.node_tree.rows().iter().map(|(slot, _)| slot).collect();
        sync_table(&mut self.node_style, &live);
        sync_table(&mut self.node_data, &live);
        for (slot, row) in self.node_tree.rows().iter() {
            if let (Some(kind), Some(data)) = (row.node_kind(), self.node_data.get_mut(slot)) {
                data.conform(kind);
            }
        }
        self.prune_part_rows();
    }

    /// Derive what one node's `parent` write implies: a newly allocated node
    /// gains default style and data rows; a retired node takes its subtree's
    /// rows from every table with it.
    fn sync_node_rows(&mut self, node: GuiNodeId) {
        if !self.node_tree.is_live(node) {
            let mut retired = vec![node.0];
            retired.extend(self.node_tree.descendants(node));
            self.retire_rows(&retired);
            return;
        }

        if !self.node_style.is_live(node.0) && !self.node_data.is_live(node.0) {
            // A dead slot rejects the insert; validation then rejects the node.
            let _ = self.node_style.insert(node.0, GuiNodeStyleRow::default());
            let mut data = GuiNodeDataRow::default();
            if let Some(kind) = self.node_tree.get(node).and_then(GuiNodeTreeRow::node_kind) {
                data.conform(kind);
            }
            let _ = self.node_data.insert(node.0, data);
        }
    }

    /// Conform one node's strings, revision and data row to its written kind.
    fn conform_node(&mut self, node: GuiNodeId) {
        let Some(row) = self.node_tree.rows_mut().get_mut(node.0) else {
            return;
        };
        let Some(kind) = row.node_kind() else {
            return;
        };
        row.conform(kind);
        if let Some(data) = self.node_data.get_mut(node.0) {
            data.conform(kind);
        }
    }

    /// Writes applying one theme part patch. An existing row changes property
    /// by property; a new part, or the first part of a new theme at the next
    /// unused theme slot, replaces the whole table.
    pub(in crate::world::systems::gui) fn theme_part_writes(
        &self,
        theme: u32,
        part: GuiPartId,
        patch: &GuiPartPatch,
    ) -> Result<Vec<FieldWrite>, ErrorReason> {
        patch.validate()?;
        let theme_slot = self
            .theme_slot(theme)
            .unwrap_or_else(|| self.next_theme_slot());
        let slot = Self::theme_part_slot(theme_slot, part).ok_or(ErrorReason::InvalidValue)?;
        if let Some(row) = self.theme_parts.get(slot) {
            return Ok(patch
                .changes
                .iter()
                .filter(|(property, value)| {
                    row.property(property.index()).ok().flatten() != **value
                })
                .filter_map(|(property, value)| {
                    Some(FieldWrite {
                        offset: Self::theme_part_offset(slot, *property)?,
                        value: value.clone().map_or(FieldValue::Unset, FieldValue::Dynamic),
                    })
                })
                .collect());
        }

        if slot >= Rows::<GuiThemePartRow>::MAX_SLOTS {
            return Err(ErrorReason::Capacity);
        }
        let mut row = GuiThemePartRow::for_theme(theme);
        patch.apply(&mut row).map_err(field_error)?;
        let mut table = Rows::clone(&self.theme_parts);
        table.insert(slot, row).map_err(field_error)?;
        Ok(vec![FieldWrite {
            offset: Self::theme_parts_field(),
            value: FieldValue::Rows(table.encode()),
        }])
    }

    /// Write removing every part row of one live theme. Referencing nodes
    /// keep the handle and resolve without a theme until it is defined again.
    pub(in crate::world::systems::gui) fn theme_removal_write(
        &self,
        theme: u32,
    ) -> Result<FieldWrite, ErrorReason> {
        let theme_slot = self.theme_slot(theme).ok_or(ErrorReason::InvalidValue)?;
        let mut table = Rows::clone(&self.theme_parts);
        let owned: Vec<u32> = table
            .iter()
            .map(|(slot, _)| slot)
            .filter(|slot| slot / GuiPartId::COUNT == theme_slot)
            .collect();
        for slot in owned {
            table.remove(slot);
        }
        Ok(FieldWrite {
            offset: Self::theme_parts_field(),
            value: FieldValue::Rows(table.encode()),
        })
    }

    /// Writes applying one node's part overrides. Motion is theme-only. An
    /// existing row changes property by property, or is dropped with the whole
    /// table once it holds neither overrides nor channels; a first override
    /// pushes a new row.
    pub(in crate::world::systems::gui) fn part_override_writes(
        &self,
        node: GuiNodeId,
        part: GuiPrimitivePart,
        patch: &GuiPartPatch,
    ) -> Result<Vec<FieldWrite>, ErrorReason> {
        patch.validate()?;
        if patch.changes.keys().any(|property| !property.appearance()) {
            return Err(ErrorReason::InvalidField);
        }
        if !self.node_tree.is_live(node) {
            return Err(ErrorReason::InvalidValue);
        }

        let table_write = |table: Rows<GuiPartRow>| FieldWrite {
            offset: Self::part_state_field(),
            value: FieldValue::Rows(table.encode()),
        };
        if let Some((slot, row)) = self.part_row(node, part) {
            let mut next = row.clone();
            patch.apply(&mut next).map_err(field_error)?;
            if !next.has_overrides() && !next.has_channels() {
                let mut table = Rows::clone(&self.part_state);
                table.remove(slot);
                return Ok(vec![table_write(table)]);
            }
            return Ok(patch
                .changes
                .iter()
                .filter(|(property, value)| {
                    row.property(property.index()).ok().flatten() != **value
                })
                .filter_map(|(property, value)| {
                    Some(FieldWrite {
                        offset: Self::part_row_offset(slot, property.index())?,
                        value: value.clone().map_or(FieldValue::Unset, FieldValue::Dynamic),
                    })
                })
                .collect());
        }

        let mut row = GuiPartRow::keyed(node.0, part);
        patch.apply(&mut row).map_err(field_error)?;
        if !row.has_overrides() {
            return Ok(Vec::new());
        }
        let mut table = Rows::clone(&self.part_state);
        table.push(row).map_err(|_| ErrorReason::Capacity)?;
        Ok(vec![table_write(table)])
    }

    /// Base parts the theme at `theme_slot` declares motion for, as a bit
    /// mask by base part index.
    fn animated_parts(&self, theme_slot: u32) -> u16 {
        let mut mask = 0;
        for index in 0..GuiPartId::COUNT {
            if let Some(id) = GuiPartId::from_index(index)
                && self
                    .theme_row(theme_slot, id)
                    .is_some_and(|row| row.motion.is_some())
            {
                mask |= 1 << base_part_index(id.part);
            }
        }
        mask
    }

    /// Make one node's live channels present exactly for the base parts in
    /// `animated` of the theme at `theme_slot`, pushing a row where one is
    /// needed and dropping rows left with neither overrides nor channels.
    /// Opened channels start from the part's base appearance.
    fn sync_part_channels(&mut self, node: u32, theme_slot: Option<u32>, animated: u16) {
        for part in GUI_BASE_PARTS {
            let key = (node, base_part_index(part));
            let needed = animated & (1 << key.1) != 0;
            let base = theme_slot
                .filter(|_| needed)
                .and_then(|slot| self.theme_row(slot, GuiPartId::base(part)))
                .cloned();
            match self.skin_index.parts.get(&key).copied() {
                Some(slot) => {
                    let Some(row) = self.part_state.get_mut(slot) else {
                        continue;
                    };
                    if needed {
                        row.open_channels(base.as_ref());
                    } else {
                        row.close_channels();
                        if !row.has_overrides() {
                            self.part_state.remove(slot);
                            self.skin_index.parts.remove(&key);
                        }
                    }
                }
                None if needed => {
                    let mut row = GuiPartRow::keyed(node, part);
                    row.open_channels(base.as_ref());
                    // An exhausted region leaves the part without channels,
                    // so its transitions paint destinations directly.
                    if let Ok(slot) = self.part_state.push(row) {
                        self.skin_index.parts.insert(key, slot);
                    }
                }
                None => {}
            }
        }
    }

    /// Re-derive the channels of one live node from its theme reference.
    fn sync_node_channels(&mut self, node: u32) {
        let theme_slot = self.node_theme_slot(GuiNodeId(node));
        let animated = theme_slot.map_or(0, |slot| self.animated_parts(slot));
        self.sync_part_channels(node, theme_slot, animated);
    }

    /// Re-derive the channels of every node referencing `theme`.
    fn sync_theme_channels(&mut self, theme: u32) {
        let theme_slot = self.theme_slot(theme);
        let animated = theme_slot.map_or(0, |slot| self.animated_parts(slot));
        let nodes: Vec<u32> = self
            .node_style
            .iter()
            .filter(|(_, row)| row.theme == Some(theme))
            .map(|(slot, _)| slot)
            .collect();
        for node in nodes {
            self.sync_part_channels(node, theme_slot, animated);
        }
    }

    /// Re-derive the channels of every node after a table was replaced.
    fn sync_all_channels(&mut self) {
        let mut masks = BTreeMap::new();
        let mut nodes: BTreeSet<u32> = self.skin_index.parts.keys().map(|key| key.0).collect();
        for (slot, row) in self.node_style.iter() {
            if row.theme.is_some() {
                nodes.insert(slot);
            }
        }
        for node in nodes {
            if !self.node_tree.is_live(GuiNodeId(node)) {
                continue;
            }
            let theme_slot = self.node_theme_slot(GuiNodeId(node));
            let animated = match theme_slot {
                Some(slot) => *masks
                    .entry(slot)
                    .or_insert_with(|| self.animated_parts(slot)),
                None => 0,
            };
            self.sync_part_channels(node, theme_slot, animated);
        }
    }

    /// Theme handle to theme slot, from the `theme` keys of a table.
    fn theme_index(table: &Rows<GuiThemePartRow>) -> BTreeMap<u32, u32> {
        table
            .iter()
            .map(|(slot, row)| (row.theme, slot / GuiPartId::COUNT))
            .collect()
    }

    /// (node, base part) to slot, from the `node` and `part` keys of a table.
    fn part_index(table: &Rows<GuiPartRow>) -> BTreeMap<(u32, u32), u32> {
        table
            .iter()
            .map(|(slot, row)| ((row.node, row.part), slot))
            .collect()
    }

    /// Rebuild the theme index after the table was replaced.
    fn rebuild_theme_index(&mut self) {
        self.skin_index.themes = Self::theme_index(&self.theme_parts);
    }

    /// Rebuild the part index after the table was replaced.
    fn rebuild_part_index(&mut self) {
        self.skin_index.parts = Self::part_index(&self.part_state);
    }

    /// Drop the part rows of nodes no longer in the tree, in one pass.
    fn prune_part_rows(&mut self) {
        let stale: Vec<(u32, u32)> = self
            .skin_index
            .parts
            .keys()
            .filter(|(node, _)| !self.node_tree.is_live(GuiNodeId(*node)))
            .copied()
            .collect();
        self.remove_part_rows(stale);
    }

    /// Drop the part rows of removed nodes, looked up by their keys.
    fn prune_node_part_rows(&mut self, nodes: &[u32]) {
        let stale: Vec<(u32, u32)> = nodes
            .iter()
            .flat_map(|&node| {
                self.skin_index
                    .parts
                    .range((node, 0)..=(node, u32::MAX))
                    .map(|(key, _)| *key)
            })
            .collect();
        self.remove_part_rows(stale);
    }

    /// Remove the part rows at `keys` with one batch removal.
    fn remove_part_rows(&mut self, keys: Vec<(u32, u32)>) {
        let slots: Vec<u32> = keys
            .iter()
            .filter_map(|key| self.skin_index.parts.remove(key))
            .collect();
        self.part_state.remove_slots(&slots);
    }

    /// Every theme slot holds rows of one theme and every theme one slot.
    fn validate_theme_rows(&self) -> Result<(), ErrorReason> {
        let mut slots: BTreeMap<u32, u32> = BTreeMap::new();
        let mut themes: BTreeSet<u32> = BTreeSet::new();
        for (slot, row) in self.theme_parts.iter() {
            row.validate()?;
            match slots.get(&(slot / GuiPartId::COUNT)) {
                Some(theme) if *theme != row.theme => return Err(ErrorReason::InvalidField),
                Some(_) => {}
                None => {
                    if !themes.insert(row.theme) {
                        return Err(ErrorReason::InvalidField);
                    }
                    slots.insert(slot / GuiPartId::COUNT, row.theme);
                }
            }
        }
        Ok(())
    }

    /// Every part row is valid, belongs to a live node and has a unique key.
    /// Node liveness reads the style table, whose slots are exactly the live
    /// nodes once [`Self::validate_rows`] holds.
    fn validate_part_rows(&self) -> Result<(), ErrorReason> {
        let mut keys = BTreeSet::new();
        for (_, row) in self.part_state.iter() {
            row.validate()?;
            if !self.node_style.is_live(row.node) || !keys.insert((row.node, row.part)) {
                return Err(ErrorReason::InvalidField);
            }
        }
        Ok(())
    }

    /// The part rows and index entries of one node, after a write re-derived
    /// its live channels: each indexed row is valid and carries its key, and
    /// the index still covers every row. Other nodes' rows are unchanged.
    fn validate_node_parts(&self, node: u32) -> Result<(), ErrorReason> {
        for part in GUI_BASE_PARTS {
            let key = (node, base_part_index(part));
            let Some(&slot) = self.skin_index.parts.get(&key) else {
                continue;
            };
            let row = self.part_state.get(slot).ok_or(ErrorReason::InvalidField)?;
            if (row.node, row.part) != key {
                return Err(ErrorReason::InvalidField);
            }
            row.validate()?;
        }

        if self.skin_index.parts.len() == self.part_state.len() {
            Ok(())
        } else {
            Err(ErrorReason::InvalidField)
        }
    }

    /// [`Self::validate_node_parts`] for every node referencing `theme`,
    /// after a motion write re-derived their channels.
    fn validate_theme_node_parts(&self, theme: u32) -> Result<(), ErrorReason> {
        for (node, row) in self.node_style.iter() {
            if row.theme == Some(theme) {
                self.validate_node_parts(node)?;
            }
        }
        Ok(())
    }

    /// The lookup index matches the tables' keys. Registry writes rebuild it;
    /// a value assembled around them is rejected rather than misresolved.
    fn validate_skin_index(&self) -> Result<(), ErrorReason> {
        if Self::theme_index(&self.theme_parts) == self.skin_index.themes
            && Self::part_index(&self.part_state) == self.skin_index.parts
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidField)
        }
    }

    /// Check one written theme or part row property against its range.
    fn validate_skin_property(&self, reference: GuiRootRowProperty) -> Result<(), ErrorReason> {
        match reference {
            GuiRootRowProperty::Theme {
                slot,
                property: Some(property),
            } => match self
                .theme_parts
                .property(slot, property.index())
                .map_err(|_| ErrorReason::InvalidField)?
            {
                Some(value) => validate_part_property(property, &value),
                None => Ok(()),
            },
            GuiRootRowProperty::Part {
                slot,
                property,
            } => {
                let index = property.index().ok_or(ErrorReason::InvalidField)?;
                match self
                    .part_state
                    .property(slot, index)
                    .map_err(|_| ErrorReason::InvalidField)?
                {
                    Some(value) => validate_part_row_property(index, &value),
                    None => Ok(()),
                }
            }
            GuiRootRowProperty::Theme {
                property: None,
                ..
            }
            | GuiRootRowProperty::Node(_) => Err(ErrorReason::InvalidField),
        }
    }

    /// Check one node's rows: its tree row's own rules, and style and data
    /// rows present with the data row matching its kind.
    pub(in crate::world::systems::gui) fn validate_node(
        &self,
        id: GuiNodeId,
    ) -> Result<GuiNodeKind, ErrorReason> {
        let kind = self
            .node_tree
            .get(id)
            .ok_or(ErrorReason::InvalidField)?
            .validate()?;
        self.node_style.get(id.0).ok_or(ErrorReason::InvalidField)?;
        self.node_data
            .get(id.0)
            .ok_or(ErrorReason::InvalidField)?
            .validate_for(kind)?;
        Ok(kind)
    }

    /// Whether each node table holds one row per live node. Tables only ever
    /// hold rows of live nodes after a tree write, so equal counts mean every
    /// live node has its rows.
    fn rows_cover_nodes(&self) -> bool {
        self.node_style.len() == self.node_tree.len()
            && self.node_data.len() == self.node_tree.len()
    }

    /// Every live node has exactly its rows, and every row matches its node.
    fn validate_rows(&self) -> Result<(), ErrorReason> {
        self.validate_style_rows()?;
        self.validate_data_rows()
    }

    /// Every live node has its rows, each style row within its ranges.
    fn validate_style_rows(&self) -> Result<(), ErrorReason> {
        if !self.rows_cover_nodes() {
            return Err(ErrorReason::InvalidField);
        }
        for (slot, _) in self.node_tree.rows().iter() {
            self.node_style
                .get(slot)
                .ok_or(ErrorReason::InvalidField)?
                .validate()?;
        }
        Ok(())
    }

    /// Every live node has its rows, each data row matching its node's kind.
    fn validate_data_rows(&self) -> Result<(), ErrorReason> {
        if !self.rows_cover_nodes() {
            return Err(ErrorReason::InvalidField);
        }
        for (slot, row) in self.node_tree.rows().iter() {
            let kind = row.node_kind().ok_or(ErrorReason::InvalidValue)?;
            self.node_data
                .get(slot)
                .ok_or(ErrorReason::InvalidField)?
                .validate_for(kind)?;
        }
        Ok(())
    }

    /// Everything a whole tree table write can break: the table itself, the
    /// node rows it re-derived and the part rows it pruned.
    fn validate_tree_table(&self) -> Result<(), ErrorReason> {
        self.node_tree.validate()?;
        if self.skin_index.parts.len() != self.part_state.len() {
            return Err(ErrorReason::InvalidField);
        }
        self.validate_data_rows()
    }

    /// Every rule a replaced theme table can break: its own rows and keys,
    /// the part rows whose channels the write re-derived for every themed
    /// node, and the lookup index.
    fn validate_skin_tables(&self) -> Result<(), ErrorReason> {
        self.validate_theme_rows()?;
        self.validate_part_table()
    }

    /// Every rule a replaced part table can break: its rows, their keys and
    /// the lookup index.
    fn validate_part_table(&self) -> Result<(), ErrorReason> {
        self.validate_part_rows()?;
        self.validate_skin_index()
    }

    /// Check one written row property against its range, its node's kind and
    /// the row-wide slider rule; a tree property also checks its node's rows
    /// and, for `parent`, the node's placement.
    fn validate_row_property(&self, reference: GuiNodePropertyRef) -> Result<(), ErrorReason> {
        let slot = reference.node.0;
        match reference.property {
            GuiNodeRowProperty::Style(property) => {
                let row = self.node_style.get(slot).ok_or(ErrorReason::InvalidField)?;
                match row
                    .property(property.index())
                    .map_err(|_| ErrorReason::InvalidField)?
                {
                    Some(value) => validate_node_style_property(property, &value),
                    None => Ok(()),
                }
            }
            GuiNodeRowProperty::Data(property) => {
                let row = self.node_data.get(slot).ok_or(ErrorReason::InvalidField)?;
                let kind = self
                    .node_tree
                    .get(reference.node)
                    .and_then(GuiNodeTreeRow::node_kind)
                    .ok_or(ErrorReason::InvalidField)?;
                if row.present(property) != property.used_by(kind) {
                    return Err(ErrorReason::InvalidField);
                }
                row.validate_values()
            }
            GuiNodeRowProperty::Tree(property) => {
                // A retiring write leaves no row; its subtree left with it.
                if !self.node_tree.is_live(reference.node) {
                    return Ok(());
                }
                self.validate_node(reference.node)?;
                if property == GuiNodeTreeProperty::Parent {
                    if self.node_tree.len() > MAX_NODES || slot >= MAX_NODE_ID {
                        return Err(ErrorReason::Capacity);
                    }
                    self.node_tree.validate_parent(reference.node)?;
                }
                Ok(())
            }
        }
    }

    /// Validate the complete root, as insertion and restoration do.
    #[cfg(test)]
    pub(in crate::world) fn validate_complete(&self) -> Result<(), ErrorReason> {
        <Self as ComponentLifecycle>::validate(self)
    }
}

/// How a node's data or values patch changes its tree and data rows: kind,
/// strings, committed text and revision, and kind-specific scalars. Direct
/// edits install the rows; GUI commands write the properties that differ.
pub(in crate::world::systems::gui) struct GuiNodeDataEdit {
    /// Tree row after the edit.
    pub row: GuiNodeTreeRow,
    /// Data row after the edit.
    pub values: GuiNodeDataRow,
}

impl GuiNodeDataEdit {
    /// Resolve a patch against a node's current tree and data rows.
    pub(in crate::world::systems::gui) fn new(
        previous: &GuiNodeTreeRow,
        current: &GuiNodeDataRow,
        patch: &GuiNodePatch,
    ) -> Result<Self, ErrorReason> {
        let previous_kind = previous.node_kind().ok_or(ErrorReason::InvalidValue)?;
        let data = match &patch.data {
            Some(data) => data.clone(),
            None => previous
                .data()
                .ok_or(ErrorReason::InvalidValue)?
                .to_owned_data(),
        };
        if !valid_node_data(&data) {
            return Err(ErrorReason::InvalidValue);
        }

        let kind = data.kind();
        let same_kind = kind == previous_kind;
        let mut values = match &patch.values {
            Some(values) => values.clone(),
            None if same_kind => current.clone(),
            None => {
                let mut placeholders = GuiNodeDataRow::default();
                placeholders.conform(kind);
                placeholders
            }
        };
        if same_kind {
            // Ordinary commits never replay authored values over newer
            // committed ones.
            values.checked = current.checked.or(values.checked);
            values.value = current.value.or(values.value);
        }
        values.validate_for(kind)?;

        let mut row = previous.clone();
        row.conform(kind);
        row.text = data.text().map(str::to_owned);
        row.placeholder = data.placeholder().map(str::to_owned);
        if !same_kind && (kind.is_control() || previous_kind.is_control()) {
            // The kind's initial state starts at the next revision; a node
            // becoming a control for the first time starts at 1.
            if previous.revision > 0 {
                row.revision = previous
                    .revision
                    .checked_add(1)
                    .ok_or(ErrorReason::InvalidValue)?;
            }
            row.committed_text = kind.is_text_input().then(|| row.text.clone()).flatten();
        }

        Ok(Self {
            row,
            values,
        })
    }
}

/// Committed value of one accepted control commit.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::world::systems::gui) enum GuiControlCommitValue {
    /// Checkbox `checked`.
    Checked(bool),
    /// Slider `value`.
    Value(f32),
    /// Text-input `committed_text`.
    Text(String),
}

/// One revision-gated control commit, checked against the current rows.
pub(in crate::world::systems::gui) struct GuiControlCommit {
    /// Revision the commit produces.
    pub revision: u32,
    /// Committed value.
    pub value: GuiControlCommitValue,
}

impl GuiControlCommit {
    /// Accept `value` for a live control when the caller observed its
    /// current revision: the value's type matches the kind and a slider value
    /// stays within its range.
    pub(in crate::world::systems::gui) fn new(
        root: &GuiRoot,
        id: GuiNodeId,
        expected_revision: u32,
        value: &GuiControlValue,
    ) -> Result<Self, ErrorReason> {
        let row = root.node_tree.get(id).ok_or(ErrorReason::InvalidValue)?;
        let data = root.node_data.get(id.0).ok_or(ErrorReason::InvalidValue)?;
        let value = match (row.node_kind(), value) {
            (Some(GuiNodeKind::Checkbox), GuiControlValue::Bool(checked)) => {
                GuiControlCommitValue::Checked(*checked)
            }
            (Some(kind @ GuiNodeKind::Slider), GuiControlValue::Scalar(scalar)) => {
                let mut next = data.clone();
                next.value = Some(*scalar);
                next.validate_for(kind)
                    .map_err(|_| ErrorReason::InvalidValue)?;
                GuiControlCommitValue::Value(*scalar)
            }
            (Some(GuiNodeKind::TextInput), GuiControlValue::Text(text))
                if text.len() <= MAX_TEXT_BYTES =>
            {
                GuiControlCommitValue::Text(text.clone())
            }
            _ => return Err(ErrorReason::InvalidValue),
        };
        if row.revision != expected_revision {
            return Err(ErrorReason::InvalidValue);
        }

        Ok(Self {
            revision: row
                .revision
                .checked_add(1)
                .ok_or(ErrorReason::InvalidValue)?,
            value,
        })
    }
}

/// Make a table's live slots equal `live` (sorted ascending), inserting
/// default rows and removing rows of absent slots in one batch.
fn sync_table<R: SchemaRow>(table: &mut Rows<R>, live: &[u32]) {
    if table.len() == live.len() && table.iter().map(|(slot, _)| slot).eq(live.iter().copied()) {
        return;
    }
    let stale: Vec<u32> = table
        .iter()
        .map(|(slot, _)| slot)
        .filter(|slot| live.binary_search(slot).is_err())
        .collect();
    table.remove_slots(&stale);
    for &slot in live {
        if !table.is_live(slot) {
            // A dead slot rejects the insert; validation then rejects the node.
            let _ = table.insert(slot, R::default());
        }
    }
}

impl ComponentLifecycle for GuiRoot {
    fn required_components() -> &'static [u16] {
        &[crate::ComponentValue::SURFACE]
    }

    // Animation, numeric animation and evaluated writes address every GUI
    // property by row offset; extension values are not animatable.
    fn animatable_field(offset: u32) -> bool {
        crate::components::rows::row_region(offset).is_none() || Self::numeric_animatable(offset)
    }

    fn supports_numeric_property(offset: u32) -> bool {
        offset == Self::units_per_metre_field() || Self::numeric_animatable(offset)
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), ErrorReason> {
        use crate::components::schema::{FieldValue, SchemaComponent};

        // Numeric writes cannot change structure, so only the written properties
        // need checks: the density must be finite and positive, and a row
        // property must be animatable, present and in range.
        for (offset, field) in fields {
            if *offset == Self::units_per_metre_field() {
                match field {
                    FieldValue::F32(units) if Self::valid_units_per_metre(*units) => continue,
                    FieldValue::F32(_) => return Err(ErrorReason::InvalidValue),
                    _ => return Err(ErrorReason::InvalidField),
                }
            }
            let FieldValue::Dynamic(value) = field else {
                return Err(ErrorReason::InvalidField);
            };
            if !Self::numeric_animatable(*offset)
                || !matches!(self.field(*offset), Ok(FieldValue::Dynamic(_)))
            {
                return Err(ErrorReason::InvalidField);
            }
            Self::validate_row_value(*offset, value)?;
        }
        Ok(())
    }

    fn supports_dynamic_properties() -> bool {
        true
    }

    fn dynamic_properties(&self) -> Option<&DynamicProperties> {
        Some(&self.properties)
    }

    fn dynamic_properties_mut(&mut self) -> Option<&mut DynamicProperties> {
        Some(&mut self.properties)
    }

    /// A root is validated whole only on insertion, restoration and evaluated
    /// replacement. Walking every node, row and skin entry after each GUI
    /// command costs O(nodes + part rows) per written field;
    /// [`Self::validate_field`] instead checks the same rules for exactly
    /// what each write changed.
    fn validates_after_operation() -> bool {
        false
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if !Self::valid_units_per_metre(self.units_per_metre) {
            return Err(ErrorReason::InvalidValue);
        }
        self.node_tree.validate()?;
        self.validate_rows()?;
        self.validate_skin_tables()
    }

    /// Each write is checked where it lands, after
    /// [`Self::after_field_write`] re-derived what depends on it, so that
    /// together the checks cover every rule of [`Self::validate`] for what
    /// the write changed:
    ///
    /// - the whole tree table: the table in one pass, then the node rows it
    ///   re-derived against their nodes;
    /// - a tree property: the node's tree row, style and data rows; a
    ///   `parent` write also the node's placement (a live parent reached
    ///   from the root within the layout depth without a cycle, or the single
    ///   root) and the identity bounds; a retiring write took its subtree's
    ///   rows with it;
    /// - a whole node table: each of its rows against its node;
    /// - a whole theme table: both skin tables and their index, since the
    ///   write re-derives every themed node's channels;
    /// - a whole part table: its rows, keys and the index;
    /// - a node row property: its range, its node's kind and the row-wide
    ///   slider rule; a theme reference also checks the node's part rows;
    /// - a theme or part row property: its range; theme motion also checks
    ///   the part rows of the nodes referencing the theme.
    ///
    /// The density is checked against its range. Row keys are never
    /// writable, and extension values carry no GUI rules.
    fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
        if offset == Self::units_per_metre_field() {
            return Self::valid_units_per_metre(self.units_per_metre)
                .then_some(())
                .ok_or(ErrorReason::InvalidValue);
        }
        if offset == Self::node_tree_field() {
            return self.validate_tree_table();
        }
        if offset == Self::node_style_field() {
            return self.validate_style_rows();
        }
        if offset == Self::node_data_field() {
            return self.validate_data_rows();
        }
        if offset == Self::theme_parts_field() {
            return self.validate_skin_tables();
        }
        if offset == Self::part_state_field() {
            return self.validate_part_table();
        }
        match Self::row_property(offset) {
            Some(GuiRootRowProperty::Node(reference)) => {
                self.validate_row_property(reference)?;
                if reference.property == GuiNodeRowProperty::Style(GuiNodeStyleProperty::Theme) {
                    self.validate_node_parts(reference.node.0)?;
                }
                Ok(())
            }
            Some(
                reference @ GuiRootRowProperty::Theme {
                    slot,
                    property: Some(GuiPartProperty::Motion),
                },
            ) => {
                self.validate_skin_property(reference)?;
                match self.theme_parts.get(slot) {
                    Some(row) => self.validate_theme_node_parts(row.theme),
                    None => Ok(()),
                }
            }
            Some(reference) => self.validate_skin_property(reference),
            None => Ok(()),
        }
    }

    /// Rows follow the tree and skins follow theme references: replacing the
    /// tree table re-derives the node rows; allocating or retiring a node
    /// inserts or removes its rows; writing a kind conforms the node's
    /// strings and data row; and writing a theme reference, a theme's motion
    /// or either skin table re-derives live channels.
    fn after_field_write(&mut self, offset: u32) {
        if offset == Self::node_tree_field() {
            self.sync_rows();
            return;
        }
        if offset == Self::theme_parts_field() {
            self.rebuild_theme_index();
            self.sync_all_channels();
            return;
        }
        if offset == Self::part_state_field() {
            self.rebuild_part_index();
            self.sync_all_channels();
            return;
        }
        match Self::row_property(offset) {
            Some(GuiRootRowProperty::Node(GuiNodePropertyRef {
                node,
                property: GuiNodeRowProperty::Tree(GuiNodeTreeProperty::Parent),
            })) => self.sync_node_rows(node),
            Some(GuiRootRowProperty::Node(GuiNodePropertyRef {
                node,
                property: GuiNodeRowProperty::Tree(GuiNodeTreeProperty::Kind),
            })) => self.conform_node(node),
            Some(GuiRootRowProperty::Node(GuiNodePropertyRef {
                node,
                property: GuiNodeRowProperty::Style(GuiNodeStyleProperty::Theme),
            })) => self.sync_node_channels(node.0),
            Some(GuiRootRowProperty::Theme {
                slot,
                property: Some(GuiPartProperty::Motion),
            }) => {
                if let Some(theme) = self.theme_parts.get(slot).map(|row| row.theme) {
                    self.sync_theme_channels(theme);
                }
            }
            _ => {}
        }
    }

    fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    ) {
        self.properties.resource_demand(demand);
        self.node_style.resource_demand(demand);
        self.theme_parts.resource_demand(demand);
        self.part_state.resource_demand(demand);
    }
}

fn field_error(_: crate::components::schema::FieldError) -> ErrorReason {
    ErrorReason::InvalidValue
}

#[cfg(test)]
#[path = "root_validation_tests.rs"]
mod tests;
