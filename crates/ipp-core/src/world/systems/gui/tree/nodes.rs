use super::controls::GuiControls;
use crate::components::schema::{ContractSink, FieldError, FieldKind, FieldValue, SchemaField};
use crate::services::asset_management::AssetSource;

const CODEC_VERSION: u8 = 1;
pub(in crate::world::systems::gui) const MAX_NODES: usize = 65_536;
/// Maximum UTF-8 bytes in one authored, committed, provisional or restored
/// GUI text value.
pub const MAX_GUI_TEXT_BYTES: usize = 65_536;
pub(in crate::world::systems::gui) const MAX_TEXT_BYTES: usize = MAX_GUI_TEXT_BYTES;

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

/// Structural node content.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiNodeContent {
    /// Layout container holding child nodes.
    Container(GuiContainerKind),
    /// Text leaf.
    Text(String),
    /// Vector drawing asset leaf.
    Drawing,
    /// Bitmap image leaf.
    Image {
        /// Display size in local metres.
        size: [f32; 2],
    },
    /// Clickable button.
    Button {
        /// Button label.
        label: String,
    },
    /// Checkbox control.
    Checkbox {
        /// Initial or authored check state.
        checked: bool,
    },
    /// Continuous or stepped range slider.
    Slider {
        /// Current value.
        value: f32,
        /// Minimum value.
        min: f32,
        /// Maximum value.
        max: f32,
        /// Step increment, or 0.0 for continuous.
        step: f32,
    },
    /// Single-line text input field.
    TextInput {
        /// Current text.
        text: String,
        /// Placeholder when empty.
        placeholder: String,
    },
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

/// Authoritative node layout and presentation properties.
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
        }
    }
}

/// Partial node edit patch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiNodePatch {
    /// Replacement content.
    pub content: Option<GuiNodeContent>,
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
    /// Structural node content.
    pub content: GuiNodeContent,
    /// Node lifetime / generation counter.
    pub lifetime: u32,
}

/// Fully fenced handle to a GUI node.
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
    /// Node lifetime / generation.
    pub node_lifetime: u32,
}

impl GuiNodeHandle {
    /// Construct a fenced node handle.
    pub const fn new(
        session: u64,
        entity: crate::EntityId,
        root_incarnation: u64,
        node_id: GuiNodeId,
        node_lifetime: u32,
    ) -> Self {
        Self {
            session,
            entity,
            root_incarnation,
            node_id,
            node_lifetime,
        }
    }
}

/// Authoritative typed storage for root-local GUI nodes and their committed control values.
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

    /// Committed control values of this tree's control nodes.
    pub fn controls(&self) -> &GuiControls {
        &self.controls
    }

    /// Validate identities, one acyclic reciprocal tree and one committed value per control node.
    pub fn validate(&self) -> Result<(), FieldError> {
        self.validate_tree()?;
        self.controls.validate_for(self)
    }

    fn validate_tree(&self) -> Result<(), FieldError> {
        if self.values.len() > MAX_NODES || self.next_id == 0 {
            return Err(FieldError::WrongType);
        }
        if self.values.is_empty() {
            if self.root_node.is_some() {
                return Err(FieldError::WrongType);
            }
            return Ok(());
        }

        let root_id = self.root_node.ok_or(FieldError::WrongType)?;
        let mut ids = std::collections::BTreeSet::new();
        for node in &self.values {
            if node.id.0 == 0 || node.id.0 >= self.next_id || !ids.insert(node.id) {
                return Err(FieldError::WrongType);
            }
            validate_node_content(&node.content)?;
        }

        if !ids.contains(&root_id) {
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
                if parent_id == node.id || !ids.contains(&parent_id) {
                    return Err(FieldError::WrongType);
                }
            }

            let mut child_set = std::collections::BTreeSet::new();
            for &child_id in &node.children {
                if !child_set.insert(child_id) || child_id == node.id || !ids.contains(&child_id) {
                    return Err(FieldError::WrongType);
                }
                let child_node = self.node(child_id).ok_or(FieldError::WrongType)?;
                if child_node.parent != Some(node.id) {
                    return Err(FieldError::WrongType);
                }
            }

            if let Some(parent_id) = node.parent {
                let parent_node = self.node(parent_id).ok_or(FieldError::WrongType)?;
                if !parent_node.children.contains(&node.id) {
                    return Err(FieldError::WrongType);
                }
            }
        }

        // Cycle and reachability check: BFS from root_id must reach every node exactly once
        let mut visited = std::collections::BTreeSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(root_id);
        visited.insert(root_id);
        while let Some(curr) = queue.pop_front() {
            let curr_node = self.node(curr).ok_or(FieldError::WrongType)?;
            for &child in &curr_node.children {
                if !visited.insert(child) {
                    return Err(FieldError::WrongType);
                }
                queue.push_back(child);
            }
        }

        if visited.len() != self.values.len() {
            return Err(FieldError::WrongType);
        }

        Ok(())
    }

    /// Insert one typed node, establishing its parent and child linkage.
    pub fn insert_node(
        &mut self,
        id: GuiNodeId,
        parent: Option<GuiNodeId>,
        index: usize,
        content: GuiNodeContent,
    ) -> Result<GuiNodeId, FieldError> {
        if self.values.len() >= MAX_NODES || id.0 != self.next_id {
            return Err(FieldError::WrongType);
        }
        validate_node_content(&content)?;

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

        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|&v| v != 0)
            .ok_or(FieldError::WrongType)?;

        self.values.push(GuiNode {
            id,
            parent,
            children: Vec::new(),
            content,
            lifetime: 1,
        });

        Ok(id)
    }

    /// Replace one node's structural content. Style lives in the root's named properties.
    pub fn replace_content(
        &mut self,
        id: GuiNodeId,
        content: GuiNodeContent,
    ) -> Result<(), FieldError> {
        validate_node_content(&content)?;
        self.node_mut(id).ok_or(FieldError::WrongType)?.content = content;
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
            put_u32(&mut output, node.lifetime);
            put_u32(&mut output, node.children.len() as u32);
            for child in &node.children {
                put_u32(&mut output, child.0);
            }

            encode_node_content(&mut output, &node.content);
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
            let lifetime = read_u32(&mut input)?;
            let children_count = read_u32(&mut input)? as usize;
            if children_count > MAX_NODES {
                return Err(FieldError::WrongType);
            }
            let mut children = Vec::with_capacity(children_count);
            for _ in 0..children_count {
                children.push(GuiNodeId(read_u32(&mut input)?));
            }

            let content = decode_node_content(&mut input)?;

            values.push(GuiNode {
                id,
                parent,
                children,
                content,
                lifetime,
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

pub(crate) fn initial_control_value_for(content: &GuiNodeContent) -> GuiControlValue {
    match content {
        GuiNodeContent::Checkbox {
            checked,
        } => GuiControlValue::Bool(*checked),
        GuiNodeContent::Slider {
            value,
            ..
        } => GuiControlValue::Scalar(*value),
        GuiNodeContent::TextInput {
            text,
            ..
        } => GuiControlValue::Text(text.clone()),
        _ => GuiControlValue::None,
    }
}

pub(in crate::world::systems::gui) fn validate_node_content(
    content: &GuiNodeContent,
) -> Result<(), FieldError> {
    match content {
        GuiNodeContent::Container(_) => Ok(()),
        GuiNodeContent::Text(text) if text.len() <= MAX_TEXT_BYTES => Ok(()),
        GuiNodeContent::Drawing => Ok(()),
        GuiNodeContent::Image {
            size,
        } if size.iter().all(|v| v.is_finite() && *v > 0.0) => Ok(()),
        GuiNodeContent::Button {
            label,
        } if label.len() <= MAX_TEXT_BYTES => Ok(()),
        GuiNodeContent::Checkbox {
            ..
        } => Ok(()),
        GuiNodeContent::Slider {
            value,
            min,
            max,
            step,
        } => {
            if value.is_finite()
                && min.is_finite()
                && max.is_finite()
                && step.is_finite()
                && min <= max
                && *step >= 0.0
            {
                Ok(())
            } else {
                Err(FieldError::NonFinite)
            }
        }
        GuiNodeContent::TextInput {
            text,
            placeholder,
        } => {
            if text.len() <= MAX_TEXT_BYTES && placeholder.len() <= MAX_TEXT_BYTES {
                Ok(())
            } else {
                Err(FieldError::WrongType)
            }
        }
        _ => Err(FieldError::WrongType),
    }
}

pub(in crate::world::systems::gui) fn validate_node_style(
    style: &GuiNodeStyle,
) -> Result<(), FieldError> {
    for dim in [
        style.width,
        style.height,
        style.min_width,
        style.min_height,
        style.max_width,
        style.max_height,
        style.flex,
        style.align_x,
        style.align_y,
    ]
    .into_iter()
    .flatten()
    {
        if !dim.is_finite() {
            return Err(FieldError::NonFinite);
        }
    }
    if let Some(padding) = &style.padding
        && !padding.iter().all(|v| v.is_finite() && *v >= 0.0)
    {
        return Err(FieldError::NonFinite);
    }
    if let Some(margin) = &style.margin
        && !margin.iter().all(|v| v.is_finite())
    {
        return Err(FieldError::NonFinite);
    }
    if !style
        .color
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    {
        return Err(FieldError::NonFinite);
    }
    if let Some(bg) = &style.background_color
        && !bg.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    {
        return Err(FieldError::NonFinite);
    }
    if !style.opacity.is_finite() || !(0.0..=1.0).contains(&style.opacity) {
        return Err(FieldError::NonFinite);
    }
    if !style.font_size.is_finite() || style.font_size <= 0.0 {
        return Err(FieldError::NonFinite);
    }
    Ok(())
}

pub(in crate::world::systems::gui) fn validate_control_value(
    content: &GuiNodeContent,
    value: &GuiControlValue,
) -> Result<(), FieldError> {
    match (content, value) {
        (
            GuiNodeContent::Checkbox {
                ..
            },
            GuiControlValue::Bool(_),
        ) => Ok(()),
        (
            GuiNodeContent::Slider {
                min,
                max,
                ..
            },
            GuiControlValue::Scalar(val),
        ) => {
            if val.is_finite() && *val >= *min && *val <= *max {
                Ok(())
            } else {
                Err(FieldError::NonFinite)
            }
        }
        (
            GuiNodeContent::TextInput {
                ..
            },
            GuiControlValue::Text(text),
        ) => {
            if text.len() <= MAX_TEXT_BYTES {
                Ok(())
            } else {
                Err(FieldError::WrongType)
            }
        }
        (
            GuiNodeContent::Container(_)
            | GuiNodeContent::Text(_)
            | GuiNodeContent::Drawing
            | GuiNodeContent::Image {
                ..
            }
            | GuiNodeContent::Button {
                ..
            },
            GuiControlValue::None,
        ) => Ok(()),
        _ => Err(FieldError::WrongType),
    }
}

pub(in crate::world::systems::gui) fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend(value.to_le_bytes());
}

fn put_f32s<const N: usize>(output: &mut Vec<u8>, values: &[f32; N]) {
    for value in values {
        output.extend(value.to_le_bytes());
    }
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

pub(in crate::world::systems::gui) fn read_f32(input: &mut &[u8]) -> Result<f32, FieldError> {
    let value = f32::from_le_bytes(take(input, 4)?.try_into().unwrap());
    if !value.is_finite() {
        return Err(FieldError::NonFinite);
    }
    Ok(value)
}

fn read_f32s<const N: usize>(input: &mut &[u8]) -> Result<[f32; N], FieldError> {
    let mut output = [0.0; N];
    for value in &mut output {
        *value = read_f32(input)?;
    }
    Ok(output)
}

fn encode_node_content(output: &mut Vec<u8>, content: &GuiNodeContent) {
    match content {
        GuiNodeContent::Container(kind) => {
            output.push(1);
            output.push(*kind as u8);
        }
        GuiNodeContent::Text(text) => {
            output.push(2);
            put_u32(output, text.len() as u32);
            output.extend(text.as_bytes());
        }
        GuiNodeContent::Drawing => {
            output.push(3);
        }
        GuiNodeContent::Image {
            size,
        } => {
            output.push(4);
            put_f32s(output, size);
        }
        GuiNodeContent::Button {
            label,
        } => {
            output.push(5);
            put_u32(output, label.len() as u32);
            output.extend(label.as_bytes());
        }
        GuiNodeContent::Checkbox {
            checked,
        } => {
            output.push(6);
            output.push(u8::from(*checked));
        }
        GuiNodeContent::Slider {
            value,
            min,
            max,
            step,
        } => {
            output.push(7);
            output.extend(value.to_le_bytes());
            output.extend(min.to_le_bytes());
            output.extend(max.to_le_bytes());
            output.extend(step.to_le_bytes());
        }
        GuiNodeContent::TextInput {
            text,
            placeholder,
        } => {
            output.push(8);
            put_u32(output, text.len() as u32);
            output.extend(text.as_bytes());
            put_u32(output, placeholder.len() as u32);
            output.extend(placeholder.as_bytes());
        }
    }
}

fn decode_node_content(input: &mut &[u8]) -> Result<GuiNodeContent, FieldError> {
    match take(input, 1)?[0] {
        1 => Ok(GuiNodeContent::Container(GuiContainerKind::from_u8(
            take(input, 1)?[0],
        )?)),
        2 => {
            let len = read_u32(input)? as usize;
            if len > MAX_TEXT_BYTES {
                return Err(FieldError::WrongType);
            }
            let bytes = take(input, len)?;
            let text = std::str::from_utf8(bytes).map_err(|_| FieldError::WrongType)?;
            Ok(GuiNodeContent::Text(text.into()))
        }
        3 => Ok(GuiNodeContent::Drawing),
        4 => Ok(GuiNodeContent::Image {
            size: read_f32s(input)?,
        }),
        5 => {
            let len = read_u32(input)? as usize;
            if len > MAX_TEXT_BYTES {
                return Err(FieldError::WrongType);
            }
            let bytes = take(input, len)?;
            let label = std::str::from_utf8(bytes).map_err(|_| FieldError::WrongType)?;
            Ok(GuiNodeContent::Button {
                label: label.into(),
            })
        }
        6 => Ok(GuiNodeContent::Checkbox {
            checked: take(input, 1)?[0] != 0,
        }),
        7 => {
            let value = read_f32(input)?;
            let min = read_f32(input)?;
            let max = read_f32(input)?;
            let step = read_f32(input)?;
            Ok(GuiNodeContent::Slider {
                value,
                min,
                max,
                step,
            })
        }
        8 => {
            let text_len = read_u32(input)? as usize;
            if text_len > MAX_TEXT_BYTES {
                return Err(FieldError::WrongType);
            }
            let text_bytes = take(input, text_len)?;
            let text = std::str::from_utf8(text_bytes).map_err(|_| FieldError::WrongType)?;

            let ph_len = read_u32(input)? as usize;
            if ph_len > MAX_TEXT_BYTES {
                return Err(FieldError::WrongType);
            }
            let ph_bytes = take(input, ph_len)?;
            let placeholder = std::str::from_utf8(ph_bytes).map_err(|_| FieldError::WrongType)?;

            Ok(GuiNodeContent::TextInput {
                text: text.into(),
                placeholder: placeholder.into(),
            })
        }
        _ => Err(FieldError::WrongType),
    }
}

#[cfg(test)]
#[path = "nodes_tests.rs"]
mod tests;
