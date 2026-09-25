use super::controls::GuiControls;
use super::node_rows::{GuiNodeDataRow, GuiNodeStyleRow};
use crate::components::rows::Rows;
use crate::components::schema::{ContractSink, FieldError, FieldKind, FieldValue, SchemaField};
use crate::services::asset_management::AssetSource;

const CODEC_VERSION: u8 = 2;
pub(in crate::world::systems::gui) const MAX_NODES: usize = 65_536;
/// Exclusive bound of node identities: one past the last slot both node row
/// tables can address. Identities are never reused within a root
/// incarnation, so a long-lived root that exhausts them fails `InsertNode`
/// until a new incarnation (for example a restore) compacts its identities.
pub const MAX_GUI_NODE_ID: u32 = min_u32(
    Rows::<GuiNodeStyleRow>::MAX_SLOTS,
    Rows::<GuiNodeDataRow>::MAX_SLOTS,
);
pub(in crate::world::systems::gui) const MAX_NODE_ID: u32 = MAX_GUI_NODE_ID;
/// Maximum UTF-8 bytes in one authored, committed, provisional or restored
/// GUI text value.
pub const MAX_GUI_TEXT_BYTES: usize = 65_536;
pub(in crate::world::systems::gui) const MAX_TEXT_BYTES: usize = MAX_GUI_TEXT_BYTES;

const fn min_u32(a: u32, b: u32) -> u32 {
    if a < b {
        a
    } else {
        b
    }
}

/// Stable root-local node identity. Zero is never valid and identities are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuiNodeId(pub u32);

/// Container layout behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GuiContainerKind {
    /// Horizontal child layout.
    Row = 0,
    /// Vertical child layout.
    Column = 1,
    /// Layered child layout.
    Stack = 2,
    /// Inner margin constraint.
    Padding = 3,
    /// Alignment within available space.
    Align = 4,
    /// Explicit dimensional constraint.
    SizedBox = 5,
    /// Scrollable viewport.
    ScrollView = 6,
}

impl GuiContainerKind {
    fn from_u8(value: u8) -> Result<Self, FieldError> {
        match value {
            0 => Ok(Self::Row),
            1 => Ok(Self::Column),
            2 => Ok(Self::Stack),
            3 => Ok(Self::Padding),
            4 => Ok(Self::Align),
            5 => Ok(Self::SizedBox),
            6 => Ok(Self::ScrollView),
            _ => Err(FieldError::WrongType),
        }
    }
}

/// Node kind with its authored strings. Kind-specific scalars (image size,
/// checkbox and slider values and slider range) live in the root's
/// `node_data` rows; see [`GuiNodeDataRow`].
#[derive(Clone, Debug, PartialEq)]
pub enum GuiNodeData {
    /// Layout container holding child nodes.
    Container(GuiContainerKind),
    /// Text leaf.
    Text(String),
    /// Vector drawing asset leaf.
    Drawing,
    /// Bitmap image leaf; its display size is the `image_size` row property.
    Image,
    /// Clickable button.
    Button {
        /// Button label.
        label: String,
    },
    /// Checkbox control; its committed state is the `checked` row property.
    Checkbox,
    /// Range slider; `value`, `min`, `max` and `step` are row properties.
    Slider,
    /// Single-line text input field.
    TextInput {
        /// Authored initial text; the committed text is a control record.
        text: String,
        /// Placeholder when empty.
        placeholder: String,
    },
}

impl GuiNodeData {
    /// Whether this kind carries a committed control value.
    pub fn is_control(&self) -> bool {
        matches!(self, Self::Checkbox | Self::Slider | Self::TextInput { .. })
    }

    /// Whether both values are the same node kind, ignoring authored strings.
    pub fn same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// Authoritative control value.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiControlValue {
    /// Non-control node.
    None,
    /// Checkbox state.
    Bool(bool),
    /// Numeric value for sliders.
    Scalar(f32),
    /// Text input value.
    Text(String),
}

/// Complete authored node style, including the visual transform. The
/// authoritative copy is the node's [`GuiNodeStyleRow`].
#[derive(Clone, Debug, PartialEq)]
pub struct GuiNodeStyle {
    /// Effective interactivity; false skips hit testing and activation.
    pub enabled: bool,
    /// Explicit width in local metres.
    pub width: Option<f32>,
    /// Explicit height in local metres.
    pub height: Option<f32>,
    /// Minimum width in local metres.
    pub min_width: Option<f32>,
    /// Minimum height in local metres.
    pub min_height: Option<f32>,
    /// Maximum width in local metres.
    pub max_width: Option<f32>,
    /// Maximum height in local metres.
    pub max_height: Option<f32>,
    /// Content padding [top, right, bottom, left].
    pub padding: Option<[f32; 4]>,
    /// Outer margin [top, right, bottom, left].
    pub margin: Option<[f32; 4]>,
    /// Share of the main-axis space a parent Row or Column leaves after its
    /// fixed children; the node keeps its tree-order slot.
    pub flex: Option<f32>,
    /// Alignment X factor from -1.0 (start) to 1.0 (end). Stack and Column
    /// place this node by it; an Align node also places its content by it.
    pub align_x: Option<f32>,
    /// Alignment Y factor from -1.0 (start) to 1.0 (end). Stack and Row
    /// place this node by it; an Align node also places its content by it.
    pub align_y: Option<f32>,
    /// Foreground / text colour (RGBA 0.0..=1.0).
    pub color: [f32; 4],
    /// Background fill colour (RGBA 0.0..=1.0).
    pub background_color: Option<[f32; 4]>,
    /// Content opacity (0.0..=1.0).
    pub opacity: f32,
    /// Font size in local metres per em.
    pub font_size: f32,
    /// Bound asset reference (font, drawing, or image).
    pub asset: Option<AssetSource>,
    /// Visual translation in local metres; moves paint and hit regions
    /// together without reflow.
    pub position: [f32; 2],
    /// Visual axis-aligned scale; moves paint and hit regions together
    /// without reflow. Layout rejects singular scales with a diagnostic.
    pub scale: [f32; 2],
    /// Handle of the root theme skinning this node, if any.
    pub theme: Option<u32>,
    /// Whether this node bounds keyboard traversal of its descendants.
    pub focus_scope: bool,
}

impl Default for GuiNodeStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            width: None,
            height: None,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            padding: None,
            margin: None,
            flex: None,
            align_x: None,
            align_y: None,
            color: [1.0; 4],
            background_color: None,
            opacity: 1.0,
            font_size: 0.1,
            asset: None,
            position: [0.0, 0.0],
            scale: [1.0, 1.0],
            theme: None,
            focus_scope: false,
        }
    }
}

/// Partial node edit. Style members replace one row property each; `None`
/// preserves it and `Some(None)` clears an optional one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiNodePatch {
    /// Replacement kind and authored strings.
    pub data: Option<GuiNodeData>,
    /// Replacement authored kind-specific scalars. Omitted with a kind change,
    /// the new kind starts from placeholders (unchecked, a unit image, a zero
    /// slider); omitted otherwise, the current row is kept. Committed control
    /// values survive compatible edits.
    pub values: Option<GuiNodeDataRow>,
    /// Replacement interactivity; None preserves the current value.
    pub enabled: Option<bool>,
    /// Replacement width.
    pub width: Option<Option<f32>>,
    /// Replacement height.
    pub height: Option<Option<f32>>,
    /// Replacement min width.
    pub min_width: Option<Option<f32>>,
    /// Replacement min height.
    pub min_height: Option<Option<f32>>,
    /// Replacement max width.
    pub max_width: Option<Option<f32>>,
    /// Replacement max height.
    pub max_height: Option<Option<f32>>,
    /// Replacement padding.
    pub padding: Option<Option<[f32; 4]>>,
    /// Replacement margin.
    pub margin: Option<Option<[f32; 4]>>,
    /// Replacement flex factor.
    pub flex: Option<Option<f32>>,
    /// Replacement align X.
    pub align_x: Option<Option<f32>>,
    /// Replacement align Y.
    pub align_y: Option<Option<f32>>,
    /// Replacement foreground colour.
    pub color: Option<[f32; 4]>,
    /// Replacement background colour.
    pub background_color: Option<Option<[f32; 4]>>,
    /// Replacement opacity.
    pub opacity: Option<f32>,
    /// Replacement font size.
    pub font_size: Option<f32>,
    /// Replacement asset.
    pub asset: Option<Option<AssetSource>>,
    /// Replacement visual translation.
    pub position: Option<[f32; 2]>,
    /// Replacement visual scale.
    pub scale: Option<[f32; 2]>,
    /// Replacement theme reference.
    pub theme: Option<Option<u32>>,
    /// Replacement focus-scope flag.
    pub focus_scope: Option<bool>,
}

/// One authoritative node in the GUI tree.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiNode {
    /// Never-reused root-local node identity.
    pub id: GuiNodeId,
    /// Logical parent node, or None if root.
    pub parent: Option<GuiNodeId>,
    /// Ordered children.
    pub children: Vec<GuiNodeId>,
    /// Node kind and authored strings.
    pub data: GuiNodeData,
}

/// Fully fenced handle to a GUI node. Node identities are never reused within
/// a root incarnation, so the identity and incarnation fence the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuiNodeHandle {
    /// World session fence.
    pub session: u64,
    /// Root entity identity.
    pub entity: crate::EntityId,
    /// Root entity component incarnation.
    pub root_incarnation: u64,
    /// Root-local node identity.
    pub node_id: GuiNodeId,
}

impl GuiNodeHandle {
    /// Construct a fenced node handle.
    pub const fn new(
        session: u64,
        entity: crate::EntityId,
        root_incarnation: u64,
        node_id: GuiNodeId,
    ) -> Self {
        Self {
            session,
            entity,
            root_incarnation,
            node_id,
        }
    }
}

/// Authoritative root-local node tree and per-control records.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiNodes {
    pub(in crate::world::systems::gui) values: Vec<GuiNode>,
    pub(in crate::world::systems::gui) next_id: u32,
    pub(in crate::world::systems::gui) root_node: Option<GuiNodeId>,
    pub(in crate::world::systems::gui) controls: GuiControls,
}

impl Default for GuiNodes {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            next_id: 1,
            root_node: None,
            controls: GuiControls::default(),
        }
    }
}

impl GuiNodes {
    /// Nodes in internal storage order.
    pub fn as_slice(&self) -> &[GuiNode] {
        &self.values
    }

    /// Number of live nodes in this root.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the GUI tree has no live nodes.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Primary root node identity, if established.
    pub fn root_node(&self) -> Option<GuiNodeId> {
        self.root_node
    }

    /// Next allocated node identity.
    pub fn next_node_id(&self) -> u32 {
        self.next_id
    }

    /// Inspect one node by identity.
    pub fn node(&self, id: GuiNodeId) -> Option<&GuiNode> {
        self.values.iter().find(|n| n.id == id)
    }

    /// Mutably inspect one node by identity.
    pub fn node_mut(&mut self, id: GuiNodeId) -> Option<&mut GuiNode> {
        self.values.iter_mut().find(|n| n.id == id)
    }

    /// Per-control revisions and committed text of this tree's control nodes.
    pub fn controls(&self) -> &GuiControls {
        &self.controls
    }

    /// Validate identities, one acyclic reciprocal tree and one control
    /// record per control node.
    pub fn validate(&self) -> Result<(), FieldError> {
        self.validate_tree()?;
        self.controls.validate_for(self)
    }

    fn validate_tree(&self) -> Result<(), FieldError> {
        if self.values.len() > MAX_NODES || self.next_id == 0 || self.next_id > MAX_NODE_ID {
            return Err(FieldError::WrongType);
        }
        if self.values.is_empty() {
            if self.root_node.is_some() {
                return Err(FieldError::WrongType);
            }
            return Ok(());
        }

        let root_id = self.root_node.ok_or(FieldError::WrongType)?;
        // One sorted identity index keeps the reciprocal-link checks
        // O(n log n) without per-node allocation.
        let mut index: Vec<(GuiNodeId, usize)> = self
            .values
            .iter()
            .enumerate()
            .map(|(position, node)| (node.id, position))
            .collect();
        index.sort_unstable_by_key(|&(id, _)| id);
        if index.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(FieldError::WrongType);
        }
        let position = |id: GuiNodeId| {
            index
                .binary_search_by_key(&id, |&(id, _)| id)
                .map(|found| index[found].1)
                .ok()
        };
        for node in &self.values {
            if node.id.0 == 0 || node.id.0 >= self.next_id {
                return Err(FieldError::WrongType);
            }
            validate_node_data(&node.data)?;
        }
        if position(root_id).is_none() {
            return Err(FieldError::WrongType);
        }

        for node in &self.values {
            if node.id == root_id {
                if node.parent.is_some() {
                    return Err(FieldError::WrongType);
                }
            } else {
                let Some(parent_id) = node.parent else {
                    return Err(FieldError::WrongType);
                };
                let parent = position(parent_id).ok_or(FieldError::WrongType)?;
                if parent_id == node.id || !self.values[parent].children.contains(&node.id) {
                    return Err(FieldError::WrongType);
                }
            }

            for &child_id in &node.children {
                let child = position(child_id).ok_or(FieldError::WrongType)?;
                if child_id == node.id || self.values[child].parent != Some(node.id) {
                    return Err(FieldError::WrongType);
                }
            }
        }

        // Cycle and reachability check: a walk from the root must reach every
        // node exactly once, which also rejects duplicate children.
        let mut visited = vec![false; self.values.len()];
        let mut reached = 1;
        let mut stack = vec![position(root_id).ok_or(FieldError::WrongType)?];
        visited[stack[0]] = true;
        while let Some(current) = stack.pop() {
            for &child in &self.values[current].children {
                let child = position(child).ok_or(FieldError::WrongType)?;
                if std::mem::replace(&mut visited[child], true) {
                    return Err(FieldError::WrongType);
                }
                reached += 1;
                stack.push(child);
            }
        }

        if reached != self.values.len() {
            return Err(FieldError::WrongType);
        }

        Ok(())
    }

    /// Insert one typed node, establishing its parent and child linkage.
    /// Fails when the node identity would leave the addressable row slots.
    pub fn insert_node(
        &mut self,
        id: GuiNodeId,
        parent: Option<GuiNodeId>,
        index: usize,
        data: GuiNodeData,
    ) -> Result<GuiNodeId, FieldError> {
        if self.values.len() >= MAX_NODES || id.0 != self.next_id || id.0 >= MAX_NODE_ID {
            return Err(FieldError::WrongType);
        }
        validate_node_data(&data)?;

        if let Some(parent_id) = parent {
            let parent_node = self
                .values
                .iter_mut()
                .find(|n| n.id == parent_id)
                .ok_or(FieldError::WrongType)?;
            let insert_idx = index.min(parent_node.children.len());
            parent_node.children.insert(insert_idx, id);
        } else if self.root_node.is_none() {
            self.root_node = Some(id);
        } else {
            return Err(FieldError::WrongType);
        }

        self.next_id += 1;
        self.values.push(GuiNode {
            id,
            parent,
            children: Vec::new(),
            data,
        });

        Ok(id)
    }

    /// Replace one node's kind and authored strings. Style and kind-specific
    /// scalars live in the root's rows.
    pub fn replace_data(&mut self, id: GuiNodeId, data: GuiNodeData) -> Result<(), FieldError> {
        validate_node_data(&data)?;
        self.node_mut(id).ok_or(FieldError::WrongType)?.data = data;
        Ok(())
    }

    /// Move a node to a new parent or reorder within children without changing identity.
    pub fn move_node(
        &mut self,
        id: GuiNodeId,
        new_parent: Option<GuiNodeId>,
        index: usize,
    ) -> Result<(), FieldError> {
        let current_parent = self
            .values
            .iter()
            .find(|n| n.id == id)
            .ok_or(FieldError::WrongType)?
            .parent;

        if let Some(new_p) = new_parent {
            if new_p == id || self.is_descendant_of(new_p, id) {
                return Err(FieldError::WrongType);
            }
            if !self.values.iter().any(|n| n.id == new_p) {
                return Err(FieldError::WrongType);
            }
        } else if self.root_node.is_some() && self.root_node != Some(id) {
            return Err(FieldError::WrongType);
        }

        if let Some(old_p) = current_parent {
            if let Some(old_parent_node) = self.values.iter_mut().find(|n| n.id == old_p) {
                old_parent_node.children.retain(|&c| c != id);
            }
        } else if self.root_node == Some(id) && new_parent.is_some() {
            self.root_node = None;
        }

        if let Some(new_p) = new_parent {
            let new_parent_node = self
                .values
                .iter_mut()
                .find(|n| n.id == new_p)
                .ok_or(FieldError::WrongType)?;
            let insert_idx = index.min(new_parent_node.children.len());
            new_parent_node.children.insert(insert_idx, id);
        } else {
            self.root_node = Some(id);
        }

        let node = self
            .values
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or(FieldError::WrongType)?;
        node.parent = new_parent;

        Ok(())
    }

    fn is_descendant_of(&self, candidate: GuiNodeId, ancestor: GuiNodeId) -> bool {
        let mut curr = candidate;
        let max_hops = self.values.len();
        for _ in 0..max_hops {
            if let Some(node) = self.node(curr) {
                if let Some(p) = node.parent {
                    if p == ancestor {
                        return true;
                    }
                    curr = p;
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        false
    }

    /// Remove a node and all of its recursive subtree, returning all removed node IDs.
    pub fn remove_node(&mut self, id: GuiNodeId) -> Result<Vec<GuiNodeId>, FieldError> {
        let parent = self
            .values
            .iter()
            .find(|n| n.id == id)
            .ok_or(FieldError::WrongType)?
            .parent;

        if let Some(parent_id) = parent {
            if let Some(parent_node) = self.values.iter_mut().find(|n| n.id == parent_id) {
                parent_node.children.retain(|&c| c != id);
            }
        } else if self.root_node == Some(id) {
            self.root_node = None;
        }

        let mut to_remove = vec![id];
        let mut queue = vec![id];
        while let Some(next) = queue.pop() {
            if let Some(node) = self.node(next) {
                for &child in &node.children {
                    to_remove.push(child);
                    queue.push(child);
                }
            }
        }

        self.values.retain(|n| !to_remove.contains(&n.id));
        Ok(to_remove)
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut output = vec![CODEC_VERSION];
        put_u32(&mut output, self.next_id);
        put_u32(&mut output, self.root_node.map(|r| r.0).unwrap_or(0));
        put_u32(&mut output, self.values.len() as u32);

        for node in &self.values {
            put_u32(&mut output, node.id.0);
            put_u32(&mut output, node.parent.map(|p| p.0).unwrap_or(0));
            put_u32(&mut output, node.children.len() as u32);
            for child in &node.children {
                put_u32(&mut output, child.0);
            }

            encode_node_data(&mut output, &node.data);
        }
        self.controls.encode(&mut output);
        output
    }

    pub(crate) fn decode(mut input: &[u8]) -> Result<Self, FieldError> {
        if take(&mut input, 1)?[0] != CODEC_VERSION {
            return Err(FieldError::WrongType);
        }
        let next_id = read_u32(&mut input)?;
        let root_id_val = read_u32(&mut input)?;
        let root_node = if root_id_val == 0 {
            None
        } else {
            Some(GuiNodeId(root_id_val))
        };
        let count = read_u32(&mut input)? as usize;
        if count > MAX_NODES {
            return Err(FieldError::WrongType);
        }

        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let id = GuiNodeId(read_u32(&mut input)?);
            let parent_val = read_u32(&mut input)?;
            let parent = if parent_val == 0 {
                None
            } else {
                Some(GuiNodeId(parent_val))
            };
            let children_count = read_u32(&mut input)? as usize;
            if children_count > MAX_NODES {
                return Err(FieldError::WrongType);
            }
            let mut children = Vec::with_capacity(children_count);
            for _ in 0..children_count {
                children.push(GuiNodeId(read_u32(&mut input)?));
            }

            let data = decode_node_data(&mut input)?;

            values.push(GuiNode {
                id,
                parent,
                children,
                data,
            });
        }

        let controls = GuiControls::decode(&mut input)?;
        if !input.is_empty() {
            return Err(FieldError::WrongType);
        }

        let result = Self {
            values,
            next_id,
            root_node,
            controls,
        };
        result.validate()?;
        Ok(result)
    }
}

impl SchemaField for GuiNodes {
    const KIND: FieldKind = FieldKind::Bytes;

    fn to_value(&self) -> FieldValue {
        FieldValue::Bytes(self.encode())
    }

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        match value {
            FieldValue::Bytes(bytes) => Self::decode(&bytes),
            _ => Err(FieldError::WrongType),
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        let bytes = self.encode();
        sink.write(&(bytes.len() as u32).to_le_bytes());
        sink.write(&bytes);
    }

    fn retained_bytes(&self) -> Option<usize> {
        Some(self.values.capacity() * std::mem::size_of::<GuiNode>())
    }
}

/// Authored strings stay within the GUI text budget.
pub(in crate::world::systems::gui) fn validate_node_data(
    data: &GuiNodeData,
) -> Result<(), FieldError> {
    let valid = match data {
        GuiNodeData::Text(text) => text.len() <= MAX_TEXT_BYTES,
        GuiNodeData::Button {
            label,
        } => label.len() <= MAX_TEXT_BYTES,
        GuiNodeData::TextInput {
            text,
            placeholder,
        } => text.len() <= MAX_TEXT_BYTES && placeholder.len() <= MAX_TEXT_BYTES,
        GuiNodeData::Container(_)
        | GuiNodeData::Drawing
        | GuiNodeData::Image
        | GuiNodeData::Checkbox
        | GuiNodeData::Slider => true,
    };
    if valid {
        Ok(())
    } else {
        Err(FieldError::WrongType)
    }
}

pub(in crate::world::systems::gui) fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend(value.to_le_bytes());
}

pub(in crate::world::systems::gui) fn take<'a>(
    input: &mut &'a [u8],
    length: usize,
) -> Result<&'a [u8], FieldError> {
    let value = input.get(..length).ok_or(FieldError::WrongType)?;
    *input = &input[length..];
    Ok(value)
}

pub(in crate::world::systems::gui) fn read_u32(input: &mut &[u8]) -> Result<u32, FieldError> {
    Ok(u32::from_le_bytes(take(input, 4)?.try_into().unwrap()))
}

fn encode_node_data(output: &mut Vec<u8>, data: &GuiNodeData) {
    match data {
        GuiNodeData::Container(kind) => {
            output.push(1);
            output.push(*kind as u8);
        }
        GuiNodeData::Text(text) => {
            output.push(2);
            put_string(output, text);
        }
        GuiNodeData::Drawing => output.push(3),
        GuiNodeData::Image => output.push(4),
        GuiNodeData::Button {
            label,
        } => {
            output.push(5);
            put_string(output, label);
        }
        GuiNodeData::Checkbox => output.push(6),
        GuiNodeData::Slider => output.push(7),
        GuiNodeData::TextInput {
            text,
            placeholder,
        } => {
            output.push(8);
            put_string(output, text);
            put_string(output, placeholder);
        }
    }
}

pub(in crate::world::systems::gui) fn put_string(output: &mut Vec<u8>, text: &str) {
    put_u32(output, text.len() as u32);
    output.extend(text.as_bytes());
}

pub(in crate::world::systems::gui) fn read_string(input: &mut &[u8]) -> Result<String, FieldError> {
    let len = read_u32(input)? as usize;
    if len > MAX_TEXT_BYTES {
        return Err(FieldError::WrongType);
    }
    let text = std::str::from_utf8(take(input, len)?).map_err(|_| FieldError::WrongType)?;
    Ok(text.into())
}

fn decode_node_data(input: &mut &[u8]) -> Result<GuiNodeData, FieldError> {
    Ok(match take(input, 1)?[0] {
        1 => GuiNodeData::Container(GuiContainerKind::from_u8(take(input, 1)?[0])?),
        2 => GuiNodeData::Text(read_string(input)?),
        3 => GuiNodeData::Drawing,
        4 => GuiNodeData::Image,
        5 => GuiNodeData::Button {
            label: read_string(input)?,
        },
        6 => GuiNodeData::Checkbox,
        7 => GuiNodeData::Slider,
        8 => GuiNodeData::TextInput {
            text: read_string(input)?,
            placeholder: read_string(input)?,
        },
        _ => return Err(FieldError::WrongType),
    })
}

#[cfg(test)]
#[path = "nodes_tests.rs"]
mod tests;
