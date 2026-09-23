use super::controls::{GuiControlState, GuiControls};
use super::node_rows::{
    GuiNodeDataProperty, GuiNodeDataRow, GuiNodePropertyRef, GuiNodeRowProperty,
    GuiNodeStyleProperty, GuiNodeStyleRow, validate_node_data_property,
    validate_node_style_property,
};
use super::nodes::{GuiControlValue, GuiNodeData, GuiNodeId, GuiNodePatch, GuiNodeStyle, GuiNodes};
use crate::components::rows::{RowAddress, Rows, SchemaRow, row_address, row_region_relative};
use crate::components::schema::ComponentLifecycle;
use crate::{DynamicProperties, DynamicValue, ErrorReason};
use ipp_schema_derive::SchemaComponent;

use crate::DynamicPropertyKind as Kind;

/// Named skin-part lanes and their exact storage types.
/// Ordered by descending suffix length so longer composite suffixes match first.
pub(crate) const GUI_PART_PROPERTIES: [(&str, Kind); 23] = [
    ("gradient_color0", Kind::Vec4),
    ("gradient_color1", Kind::Vec4),
    ("gradient_radius", Kind::F32),
    ("glow_intensity", Kind::F32),
    ("gradient_start", Kind::Vec2),
    ("corner_radius", Kind::Vec2),
    ("gradient_end", Kind::Vec2),
    ("border_color", Kind::Vec4),
    ("border_width", Kind::F32),
    ("glow_falloff", Kind::F32),
    ("glow_radius", Kind::F32),
    ("glow_color", Kind::Vec4),
    ("fill_mode", Kind::F32),
    ("duration", Kind::F32),
    ("opacity", Kind::F32),
    ("align_x", Kind::F32),
    ("easing", Kind::F32),
    ("motion", Kind::Asset),
    ("color", Kind::Vec4),
    ("scale", Kind::Vec2),
    ("asset", Kind::Asset),
    ("track", Kind::F32),
    ("time", Kind::F32),
];

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
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiRoot {
    /// Authoritative root-local node tree and control records.
    nodes: GuiNodes,
    /// Node style rows at slot = node id; rows field 0.
    #[schema(rows)]
    node_style: Rows<GuiNodeStyleRow>,
    /// Scalar kind-specific node data at slot = node id; rows field 1.
    #[schema(rows)]
    node_data: Rows<GuiNodeDataRow>,
    /// Named skin-part properties. Boxed: the set is rarely large and keeps
    /// the root, and so every `Command`, within its size budget.
    #[schema(ignore)]
    pub properties: Box<DynamicProperties>,
}

impl GuiRoot {
    /// Rows field index of `node_style`; its region starts at `0x1000_0000`.
    pub const NODE_STYLE_FIELD: usize = 0;

    /// Rows field index of `node_data`; its region starts at `0x2000_0000`.
    pub const NODE_DATA_FIELD: usize = 1;

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

    /// Whether numeric animation and overlays may target an offset: numeric
    /// style properties and `image_size`. False for `enabled`, `asset`, the
    /// command-owned control values and slider range, and any other offset.
    pub const fn numeric_animatable(offset: u32) -> bool {
        match Self::node_property(offset) {
            Some(reference) => reference.property.numeric_animatable(),
            None => false,
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

    /// Node style rows keyed by node id.
    pub fn node_style(&self) -> &Rows<GuiNodeStyleRow> {
        &self.node_style
    }

    /// Scalar node data rows keyed by node id, including committed checkbox
    /// and slider values.
    pub fn node_data(&self) -> &Rows<GuiNodeDataRow> {
        &self.node_data
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

    /// Whether only GUI commands may write `offset` on a live root: the tree,
    /// both whole tables, committed control values and the slider range.
    pub fn command_owned_field(offset: u32) -> bool {
        offset == Self::nodes_field()
            || offset == Self::node_style_field()
            || offset == Self::node_data_field()
            || matches!(
                Self::node_property(offset),
                Some(GuiNodePropertyRef {
                    property: GuiNodeRowProperty::Data(property),
                    ..
                }) if property.command_owned()
            )
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

    /// Every named-part lane of one node in name order, as the part-and-lane
    /// remainder after `node_<id>_part_` and its prepared descriptor.
    pub(crate) fn part_lanes<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a str, crate::DynamicPropertyDescriptor)> + 'a {
        self.properties.with_prefix(prefix)
    }

    /// Copy of this root for editing at most one node: the complete tree and
    /// control records with only `node`'s rows and part properties.
    /// Validating and diffing the copy covers exactly what one command can
    /// change.
    pub(in crate::world::systems::gui) fn edit_scope(
        &self,
        node: Option<GuiNodeId>,
    ) -> Result<Self, ErrorReason> {
        let mut scope = Self {
            nodes: self.nodes.clone(),
            node_style: Rows::new(),
            node_data: Rows::new(),
            properties: Box::default(),
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
            let prefix = node_property_prefix(id);
            for (name, descriptor) in self.properties.named_with_prefix(&prefix) {
                let value = self
                    .properties
                    .get_descriptor(descriptor)
                    .ok_or(ErrorReason::InvalidField)?;
                scope.properties.set(name, value).map_err(field_error)?;
            }
        }
        Ok(scope)
    }

    /// Full names and descriptors of every part property owned by `node`, in
    /// name order.
    pub(in crate::world::systems::gui) fn node_properties<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a str, crate::DynamicPropertyDescriptor)> + 'a {
        self.properties.named_with_prefix(prefix)
    }

    /// Insert one node with its complete style and kind-specific data.
    /// Control nodes start at revision 1 with the authored value committed.
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
        self.node_data.insert(id.0, values).map_err(field_error)
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
        row.validate()
    }

    /// Remove a node and its subtree with their rows, control records and
    /// part properties, returning the removed identities.
    pub(in crate::world::systems::gui) fn remove_node(
        &mut self,
        id: GuiNodeId,
    ) -> Result<Vec<GuiNodeId>, ErrorReason> {
        let removed = self.nodes.remove_node(id).map_err(field_error)?;
        for &id in &removed {
            self.nodes.controls.remove(id);
            self.node_style.remove(id.0);
            self.node_data.remove(id.0);
            self.remove_part_properties(id);
        }
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

    /// Remove every part property owned by a removed node.
    fn remove_part_properties(&mut self, id: GuiNodeId) {
        let prefix = node_property_prefix(id);
        let owned: Vec<String> = self
            .properties
            .descriptors()
            .range(prefix.clone()..)
            .take_while(|(name, _)| name.starts_with(&prefix))
            .map(|(name, _)| name.clone())
            .collect();
        for name in owned {
            self.properties.remove(&name);
        }
    }

    /// Give every node its rows, drop the rows of nodes no longer in the tree
    /// and conform each data row to its node's kind, after the tree field was
    /// replaced. New rows start from the default style; data rows keep their
    /// values for properties the kind still uses, gain placeholders for newly
    /// used ones and drop the rest, so every tree write leaves a valid root
    /// that the row writes following it complete. A slot that died earlier in
    /// this incarnation stays without a row, which readers treat as defaults.
    fn sync_rows(&mut self) {
        let mut live: Vec<u32> = self.nodes.as_slice().iter().map(|node| node.id.0).collect();
        live.sort_unstable();
        sync_table(&mut self.node_style, &live);
        sync_table(&mut self.node_data, &live);
        for node in self.nodes.as_slice() {
            if let Some(row) = self.node_data.get_mut(node.id.0) {
                row.conform(&node.data);
            }
        }
    }

    /// Produce a validated named-property address for a named skin part lane.
    pub fn part_property_name(id: GuiNodeId, part: &str, suffix: &str) -> Option<String> {
        if valid_part_name(part) && lane_kind(&GUI_PART_PROPERTIES, suffix).is_some() {
            Some(format!("node_{}_part_{}_{}", id.0, part, suffix))
        } else {
            None
        }
    }

    /// Whether a canonical named property belongs to a node removed from this root.
    pub(in crate::world) fn is_removed_node_property(&self, name: &str) -> bool {
        property_lane(name).is_some_and(|(id, _, _)| self.nodes.node(id).is_none())
    }

    /// Every named property is a GUI part lane with its declared type and range.
    pub(in crate::world::systems::gui) fn validate_properties(&self) -> Result<(), ErrorReason> {
        for name in self.properties.descriptors().keys() {
            let value = self.properties.get(name).ok_or(ErrorReason::InvalidField)?;
            validate_property_value(name, &value)?;
        }
        Ok(())
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

    fn supports_numeric_property(offset: u32) -> bool {
        crate::components::dynamic_properties::is_dynamic_field(offset)
            && offset != crate::components::dynamic_properties::DYNAMIC_METADATA
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), ErrorReason> {
        // Numeric writes cannot change structure, so only the written lanes need checks.
        for (offset, field) in fields {
            let crate::components::schema::FieldValue::Dynamic(value) = field else {
                return Err(ErrorReason::InvalidField);
            };
            if value.kind() == crate::DynamicPropertyKind::Asset
                || self
                    .properties
                    .get_key(*offset)
                    .is_none_or(|old| old.kind() != value.kind())
            {
                return Err(ErrorReason::InvalidField);
            }
            let name = self
                .properties
                .descriptors()
                .iter()
                .find_map(|(name, descriptor)| (descriptor.key == *offset).then_some(name.as_str()))
                .ok_or(ErrorReason::InvalidField)?;
            validate_property_value(name, value)?;
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
        self.validate_properties()
    }

    /// Each write is checked where it lands: a row property against its range
    /// and node, a named part lane against its declaration. The tree and both
    /// whole tables are structurally validated by their decoders; only GUI
    /// commands write them on a live root, and new incarnations are validated
    /// completely before admission.
    fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
        if offset == crate::components::dynamic_properties::DYNAMIC_METADATA {
            return self.validate_properties();
        }
        if crate::components::dynamic_properties::is_dynamic_field(offset) {
            let (name, _) = self
                .properties
                .descriptors()
                .iter()
                .find(|(_, descriptor)| descriptor.key == offset)
                .ok_or(ErrorReason::InvalidField)?;
            let value = self.properties.get(name).ok_or(ErrorReason::InvalidField)?;
            return validate_property_value(name, &value);
        }
        if let Some(reference) = Self::node_property(offset) {
            return self.validate_row_property(reference);
        }
        Ok(())
    }

    /// Rows follow the tree: replacing it inserts and removes node rows.
    fn after_field_write(&mut self, offset: u32) {
        if offset == Self::nodes_field() {
            self.sync_rows();
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
    }
}

/// Common prefix of every part lane owned by one node.
pub(in crate::world::systems::gui) fn node_property_prefix(id: GuiNodeId) -> String {
    format!("node_{}_", id.0)
}

/// Prefix of every lane owned by one named part of a node.
pub(crate) fn part_property_prefix(id: GuiNodeId, part: &str) -> String {
    format!("node_{}_part_{}_", id.0, part)
}

/// Prefix of every named-part lane owned by one node.
pub(crate) fn node_parts_prefix(id: GuiNodeId) -> String {
    format!("node_{}_part_", id.0)
}

fn field_error(_: crate::components::schema::FieldError) -> ErrorReason {
    ErrorReason::InvalidValue
}

/// Parse `node_<id>_part_<part>_<lane>` into its node, lane and type.
fn property_lane(name: &str) -> Option<(GuiNodeId, &str, Kind)> {
    let (id, lane) = name.strip_prefix("node_")?.split_once('_')?;
    if id.starts_with('0') {
        return None;
    }
    let id = GuiNodeId(id.parse().ok()?);
    let part_and_suffix = lane.strip_prefix("part_")?;
    for &(suffix, kind) in &GUI_PART_PROPERTIES {
        if let Some(part) = part_and_suffix.strip_suffix(suffix)
            && let Some(part) = part.strip_suffix('_')
            && valid_part_name(part)
        {
            return Some((id, suffix, kind));
        }
    }
    None
}

fn lane_kind(lanes: &[(&str, Kind)], suffix: &str) -> Option<Kind> {
    lanes
        .iter()
        .find_map(|(lane, kind)| (*lane == suffix).then_some(*kind))
}

fn valid_part_name(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Convert an authored F32 base track only when all three skin lanes fit.
pub(in crate::world::systems::gui) fn skin_motion_base_track(value: f32) -> Option<u32> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    let base = u32::try_from(value as u64).ok()?;
    base.checked_add(2)?;
    Some(base)
}

/// Accept only GUI part-addressed names with their exact type and value range.
pub(crate) fn validate_property_value(name: &str, value: &DynamicValue) -> Result<(), ErrorReason> {
    let (_, lane, kind) = property_lane(name).ok_or(ErrorReason::InvalidField)?;
    if value.kind() != kind {
        return Err(ErrorReason::InvalidField);
    }
    value.validate().map_err(field_error)?;
    let valid = match value {
        DynamicValue::F32(value) => {
            value.is_finite()
                && match lane {
                    "opacity" => (0.0..=1.0).contains(value),
                    "font_size" => *value > 0.0,
                    "duration" | "time" => *value >= 0.0,
                    "easing" => matches!(*value, 0.0 | 1.0),
                    "track" => skin_motion_base_track(*value).is_some(),
                    "border_width" | "gradient_radius" | "glow_intensity" | "glow_radius"
                    | "glow_falloff" => *value >= 0.0,
                    "fill_mode" => matches!(*value, 0.0 | 1.0 | 2.0),
                    _ => true,
                }
        }
        DynamicValue::Vec2(value) => {
            value.iter().all(|v| v.is_finite())
                && match lane {
                    "corner_radius" => value.iter().all(|v| *v >= 0.0),
                    _ => true,
                }
        }
        DynamicValue::Vec4(value) => {
            value.iter().all(|value| value.is_finite())
                && match lane {
                    "color" | "border_color" | "gradient_color0" | "gradient_color1"
                    | "glow_color" => value.iter().all(|value| (0.0..=1.0).contains(value)),
                    _ => true,
                }
        }
        DynamicValue::Bool(_) => true,
        DynamicValue::Asset(_) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}
