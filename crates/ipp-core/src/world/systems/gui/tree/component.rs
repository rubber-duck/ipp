use super::controls::{GuiControlState, GuiControls};
use super::node_rows::{
    GuiNodeDataProperty, GuiNodeDataRow, GuiNodePropertyRef, GuiNodeRowProperty,
    GuiNodeStyleProperty, GuiNodeStyleRow, validate_node_data_property,
    validate_node_style_property,
};
use super::nodes::{GuiControlValue, GuiNodeData, GuiNodeId, GuiNodePatch, GuiNodeStyle, GuiNodes};
use super::part_rows::{
    GUI_BASE_PARTS, GuiPartId, GuiPartPatch, GuiPartProperty, GuiPartRow, GuiPartRowProperty,
    GuiThemePartRow, base_part_index, validate_part_property, validate_part_row_property,
};
use crate::components::rows::{RowAddress, Rows, SchemaRow, row_address, row_region_relative};
use crate::components::schema::ComponentLifecycle;
use crate::systems::surface::GuiPrimitivePart;
use crate::{DynamicProperties, DynamicValue, ErrorReason, FieldValue, FieldWrite};
use ipp_schema_derive::SchemaComponent;
use std::collections::{BTreeMap, BTreeSet};

/// Root GUI component owning the content of its entity's Surface.
///
/// The node tree and control records change only through GuiCommand while a
/// root incarnation is live; a new incarnation may supply them. Each node owns
/// one `node_style` row and one `node_data` row at slot = node id. Writing
/// the tree inserts default rows for new nodes and removes the rows of
/// removed nodes, so row properties are addressed by offset only while their
/// node lives. The committed checkbox and slider values and the slider range
/// are `node_data` properties that only GuiCommand changes; style properties
/// and `image_size` are ordinary numeric properties.
///
/// Skins are root-owned themes in `theme_parts`, referenced by each node's
/// `theme` style property, and per-node `part_state` rows holding appearance
/// overrides and the live channels skin transitions animate. The root keeps a
/// node's channels present exactly while its theme declares motion for the
/// part, so writing a theme reference, a theme's motion or either table
/// re-derives them. Theme updates never rewrite referencing nodes.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiRoot {
    /// Authoritative root-local node tree and control records.
    nodes: GuiNodes,
    /// Node style rows at slot = node id; rows field 0.
    #[schema(rows)]
    node_style: Rows<GuiNodeStyleRow>,
    /// Scalar kind-specific node data at slot = node id; rows field 1. Boxed,
    /// like both skin tables, to keep the root and so every `Command` small.
    #[schema(rows)]
    node_data: Box<Rows<GuiNodeDataRow>>,
    /// Theme part rows at slot = theme slot * [`GuiPartId::COUNT`] + part;
    /// rows field 2.
    #[schema(rows)]
    theme_parts: Box<Rows<GuiThemePartRow>>,
    /// Per-node part rows keyed by their `node` and `part` properties at
    /// monotonically allocated slots; rows field 3.
    #[schema(rows)]
    part_state: Box<Rows<GuiPartRow>>,
    /// Application extension values; the runtime writes no GUI names here.
    /// Boxed: the set is rarely large and keeps the root, and so every
    /// `Command`, within its size budget.
    #[schema(ignore)]
    pub properties: Box<DynamicProperties>,
    /// Theme and part row lookup, rebuilt whenever either table is replaced.
    #[schema(ignore)]
    skin_index: Box<GuiSkinIndex>,
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
    /// A `node_style` or `node_data` property of one node.
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
    /// two node regions and for node id zero, which is never valid.
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
    /// `theme`, the command-owned control values, slider range and row keys,
    /// asset references and any other offset.
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
    /// slider `min <= value <= max`) are checked on the row.
    pub fn validate_node_property(offset: u32, value: &DynamicValue) -> Result<(), ErrorReason> {
        match Self::node_property(offset)
            .ok_or(ErrorReason::InvalidField)?
            .property
        {
            GuiNodeRowProperty::Style(property) => validate_node_style_property(property, value),
            GuiNodeRowProperty::Data(property) => validate_node_data_property(property, value),
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

    pub(in crate::world::systems::gui) const fn nodes_field() -> u32 {
        std::mem::offset_of!(Self, nodes) as u32
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

    /// Whether only GUI commands may write `offset` on a live root: the tree,
    /// every whole table, committed control values, the slider range, node
    /// theme references and row keys.
    pub fn command_owned_field(offset: u32) -> bool {
        offset == Self::nodes_field()
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

    /// Read-only access to root-local nodes.
    pub fn nodes(&self) -> &GuiNodes {
        &self.nodes
    }

    pub(in crate::world::systems::gui) fn nodes_mut(&mut self) -> &mut GuiNodes {
        &mut self.nodes
    }

    /// Control revisions and committed text-input text.
    pub fn controls(&self) -> &GuiControls {
        &self.nodes.controls
    }

    /// Number of live nodes in this tree.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Identity a caller must use for the next node insertion.
    pub fn next_node_id(&self) -> u32 {
        self.nodes.next_node_id()
    }

    /// Committed control value and revision of one node; None for nodes that
    /// were never controls.
    pub fn control_state(&self, id: GuiNodeId) -> Option<GuiControlState> {
        let entry = self.nodes.controls.get(id)?;
        Some(GuiControlState {
            value: self.control_value(id),
            revision: entry.revision,
        })
    }

    /// Committed control value of one node; None for non-control nodes.
    pub fn control_value(&self, id: GuiNodeId) -> GuiControlValue {
        let Some(node) = self.nodes.node(id) else {
            return GuiControlValue::None;
        };
        let data = self.node_data.get(id.0);
        match &node.data {
            GuiNodeData::Checkbox => data
                .and_then(|row| row.checked)
                .map_or(GuiControlValue::None, GuiControlValue::Bool),
            GuiNodeData::Slider => data
                .and_then(|row| row.value)
                .map_or(GuiControlValue::None, GuiControlValue::Scalar),
            GuiNodeData::TextInput {
                ..
            } => self
                .nodes
                .controls
                .get(id)
                .and_then(|entry| entry.text.clone())
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
        self.nodes.node(id)?;
        Some(
            self.node_style
                .get(id.0)
                .map(GuiNodeStyle::from)
                .unwrap_or_default(),
        )
    }

    /// Copy of this root for editing at most one node: the complete tree and
    /// control records with only `node`'s style and data rows. Validating
    /// and diffing the copy covers exactly what one node command can change;
    /// part rows follow from the written tree and theme reference.
    pub(in crate::world::systems::gui) fn edit_scope(
        &self,
        node: Option<GuiNodeId>,
    ) -> Result<Self, ErrorReason> {
        let mut scope = Self {
            nodes: self.nodes.clone(),
            ..Self::default()
        };
        if let Some(id) = node {
            if let Some(row) = self.node_style.get(id.0) {
                scope
                    .node_style
                    .insert(id.0, row.clone())
                    .map_err(field_error)?;
            }
            if let Some(row) = self.node_data.get(id.0) {
                scope
                    .node_data
                    .insert(id.0, row.clone())
                    .map_err(field_error)?;
            }
        }
        Ok(scope)
    }

    /// Insert one node with its complete style and kind-specific data, and
    /// the live channels its theme reference needs. Control nodes start at
    /// revision 1 with the authored value committed.
    /// Identities at or past [`MAX_GUI_NODE_ID`](super::nodes::MAX_GUI_NODE_ID)
    /// fail with `Capacity`; a new root incarnation starts identities anew.
    pub(in crate::world::systems::gui) fn insert_node(
        &mut self,
        id: GuiNodeId,
        parent: Option<GuiNodeId>,
        index: usize,
        data: GuiNodeData,
        values: GuiNodeDataRow,
        style: &GuiNodeStyle,
    ) -> Result<(), ErrorReason> {
        if id.0 >= super::nodes::MAX_NODE_ID {
            return Err(ErrorReason::Capacity);
        }
        let style = GuiNodeStyleRow::from(style);
        style.validate()?;
        values.validate_for(&data)?;
        self.nodes
            .insert_node(id, parent, index, data.clone())
            .map_err(field_error)?;
        self.nodes.controls.insert_initial(id, &data);
        self.node_style.insert(id.0, style).map_err(field_error)?;
        self.node_data.insert(id.0, values).map_err(field_error)?;
        self.sync_node_channels(id.0);
        Ok(())
    }

    /// Apply a node patch. Style members replace row properties; a data or
    /// values change keeps a compatible committed value, rejects a same-kind
    /// edit that would invalidate it, and establishes the new kind's initial
    /// state at the next revision when the kind changes to or from a control.
    pub(in crate::world::systems::gui) fn update_node(
        &mut self,
        id: GuiNodeId,
        patch: &GuiNodePatch,
    ) -> Result<(), ErrorReason> {
        let previous = self
            .nodes
            .node(id)
            .ok_or(ErrorReason::InvalidValue)?
            .data
            .clone();
        if patch.data.is_some() || patch.values.is_some() {
            let data = patch.data.clone().unwrap_or_else(|| previous.clone());
            let current = self.node_data.get(id.0).cloned().unwrap_or_default();
            let same_kind = data.same_kind(&previous);
            let mut values = match &patch.values {
                Some(values) => values.clone(),
                None if same_kind => current.clone(),
                None => {
                    let mut placeholders = GuiNodeDataRow::default();
                    placeholders.conform(&data);
                    placeholders
                }
            };
            if same_kind {
                // Ordinary commits never replay authored values over newer
                // committed ones.
                values.checked = current.checked.or(values.checked);
                values.value = current.value.or(values.value);
            }
            values.validate_for(&data)?;

            self.nodes
                .replace_data(id, data.clone())
                .map_err(field_error)?;
            if !same_kind && (data.is_control() || previous.is_control()) {
                self.nodes
                    .controls
                    .restart(id, &data)
                    .map_err(field_error)?;
            }
            *self
                .node_data
                .get_mut(id.0)
                .ok_or(ErrorReason::InvalidValue)? = values;
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

    /// Remove a node and its subtree with their node and part rows and
    /// control records, returning the removed identities.
    pub(in crate::world::systems::gui) fn remove_node(
        &mut self,
        id: GuiNodeId,
    ) -> Result<Vec<GuiNodeId>, ErrorReason> {
        let removed = self.nodes.remove_node(id).map_err(field_error)?;
        for &id in &removed {
            self.nodes.controls.remove(id);
            self.node_style.remove(id.0);
            self.node_data.remove(id.0);
        }
        self.prune_part_rows();
        Ok(removed)
    }

    /// Commit a control value when the caller observed the current revision.
    pub(in crate::world::systems::gui) fn set_control_value(
        &mut self,
        id: GuiNodeId,
        expected_revision: u32,
        value: &GuiControlValue,
    ) -> Result<(), ErrorReason> {
        let data = &self.nodes.node(id).ok_or(ErrorReason::InvalidValue)?.data;
        let row = self.node_data.get(id.0).ok_or(ErrorReason::InvalidValue)?;
        let mut next = row.clone();
        let mut text = None;
        match (data, value) {
            (GuiNodeData::Checkbox, GuiControlValue::Bool(checked)) => {
                next.checked = Some(*checked)
            }
            (GuiNodeData::Slider, GuiControlValue::Scalar(scalar)) => next.value = Some(*scalar),
            (
                GuiNodeData::TextInput {
                    ..
                },
                GuiControlValue::Text(value),
            ) if value.len() <= super::nodes::MAX_TEXT_BYTES => text = Some(value.clone()),
            _ => return Err(ErrorReason::InvalidValue),
        }
        next.validate_for(data)
            .map_err(|_| ErrorReason::InvalidValue)?;

        let entry = self
            .nodes
            .controls
            .advance(id, expected_revision)
            .map_err(|_| ErrorReason::InvalidValue)?;
        if text.is_some() {
            entry.text = text;
        }
        *self
            .node_data
            .get_mut(id.0)
            .ok_or(ErrorReason::InvalidValue)? = next;
        Ok(())
    }

    /// Give every node its rows, drop the rows of nodes no longer in the tree
    /// and conform each data row to its node's kind, after the tree field was
    /// replaced. New rows start from the default style; data rows keep their
    /// values for properties the kind still uses, gain placeholders for newly
    /// used ones and drop the rest, so every tree write leaves a valid root
    /// that the row writes following it complete. A slot that died earlier in
    /// this incarnation stays without a row, which readers treat as defaults.
    /// Part rows of removed nodes go with them.
    fn sync_rows(&mut self) {
        let mut live: Vec<u32> = self.nodes.as_slice().iter().map(|node| node.id.0).collect();
        live.sort_unstable();
        sync_table(&mut self.node_style, &live);
        sync_table(&mut *self.node_data, &live);
        for node in self.nodes.as_slice() {
            if let Some(row) = self.node_data.get_mut(node.id.0) {
                row.conform(&node.data);
            }
        }
        self.prune_part_rows();
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
        self.nodes.node(node).ok_or(ErrorReason::InvalidValue)?;

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
    fn animated_parts(&self, theme_slot: u32) -> u8 {
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
    fn sync_part_channels(&mut self, node: u32, theme_slot: Option<u32>, animated: u8) {
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
            if self.nodes.node(GuiNodeId(node)).is_none() {
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

    /// Drop the part rows of nodes no longer in the tree.
    fn prune_part_rows(&mut self) {
        let stale: Vec<((u32, u32), u32)> = self
            .skin_index
            .parts
            .iter()
            .filter(|((node, _), _)| self.nodes.node(GuiNodeId(*node)).is_none())
            .map(|(key, slot)| (*key, *slot))
            .collect();
        for (key, slot) in stale {
            self.part_state.remove(slot);
            self.skin_index.parts.remove(&key);
        }
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
    fn validate_part_rows(&self) -> Result<(), ErrorReason> {
        let mut keys = BTreeSet::new();
        for (_, row) in self.part_state.iter() {
            row.validate()?;
            if self.nodes.node(GuiNodeId(row.node)).is_none() || !keys.insert((row.node, row.part))
            {
                return Err(ErrorReason::InvalidField);
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

    /// Validate the node tree and control records, which are all a control
    /// commit changes besides the committed value's row.
    pub(in crate::world::systems::gui) fn validate_tree(&self) -> Result<(), ErrorReason> {
        self.nodes.validate().map_err(field_error)
    }

    /// Every live node has exactly its rows, and every row matches its node.
    fn validate_rows(&self) -> Result<(), ErrorReason> {
        if self.node_style.len() != self.nodes.len() || self.node_data.len() != self.nodes.len() {
            return Err(ErrorReason::InvalidField);
        }
        for node in self.nodes.as_slice() {
            self.node_style
                .get(node.id.0)
                .ok_or(ErrorReason::InvalidField)?
                .validate()?;
            self.node_data
                .get(node.id.0)
                .ok_or(ErrorReason::InvalidField)?
                .validate_for(&node.data)?;
        }
        Ok(())
    }

    /// Check one written row property against its range, its node's kind and
    /// the row-wide slider rule.
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
                let node = self
                    .nodes
                    .node(reference.node)
                    .ok_or(ErrorReason::InvalidField)?;
                if row.present(property) != property.used_by(&node.data) {
                    return Err(ErrorReason::InvalidField);
                }
                row.validate_values()
            }
        }
    }

    pub(in crate::world) fn validate_complete(&self) -> Result<(), ErrorReason> {
        <Self as ComponentLifecycle>::validate(self)
    }
}

/// Make a table's live slots equal `live` (sorted ascending), inserting
/// default rows and removing rows of absent slots.
fn sync_table<R: crate::components::rows::SchemaRow>(table: &mut Rows<R>, live: &[u32]) {
    if table.len() == live.len() && table.iter().map(|(slot, _)| slot).eq(live.iter().copied()) {
        return;
    }
    let stale: Vec<u32> = table
        .iter()
        .map(|(slot, _)| slot)
        .filter(|slot| live.binary_search(slot).is_err())
        .collect();
    for slot in stale {
        table.remove(slot);
    }
    for &slot in live {
        if !table.is_live(slot) {
            // A dead slot rejects the insert; the node then reads defaults.
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
        Self::numeric_animatable(offset)
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), ErrorReason> {
        use crate::components::schema::{FieldValue, SchemaComponent};

        // Numeric writes cannot change structure, so only the written properties
        // need checks: a row property must be animatable, present and in range.
        for (offset, field) in fields {
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

    fn validate(&self) -> Result<(), ErrorReason> {
        self.nodes.validate().map_err(field_error)?;
        self.validate_rows()?;
        self.validate_theme_rows()?;
        self.validate_part_rows()?;
        self.validate_skin_index()
    }

    /// Each write is checked where it lands: a row property against its range
    /// and node. Whole theme and part tables are checked for consistent keys;
    /// the tree and node tables are structurally validated by their decoders.
    /// Only GUI commands write tables on a live root, and new incarnations are
    /// validated completely before admission. Extension values carry no GUI
    /// rules.
    fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
        if offset == Self::theme_parts_field() {
            return self.validate_theme_rows();
        }
        if offset == Self::part_state_field() {
            return self.validate_part_rows();
        }
        match Self::row_property(offset) {
            Some(GuiRootRowProperty::Node(reference)) => self.validate_row_property(reference),
            Some(reference) => self.validate_skin_property(reference),
            None => Ok(()),
        }
    }

    /// Rows follow the tree and skins follow theme references: replacing the
    /// tree inserts and removes node rows, and writing a theme reference, a
    /// theme's motion or either skin table re-derives live channels.
    fn after_field_write(&mut self, offset: u32) {
        if offset == Self::nodes_field() {
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
