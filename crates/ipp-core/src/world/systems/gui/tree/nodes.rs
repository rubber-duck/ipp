//! Node identity, kinds, authored data and style values of the GUI tree.
//!
//! The authoritative tree is the root's `node_tree` rows table (see
//! [`super::node_tree`]); the types here describe one node's identity, kind
//! and authored strings as commands, inspection and readers exchange them.

use super::node_rows::{GuiNodeDataRow, GuiNodeStyleRow};
use super::node_tree::GuiNodeTreeRow;
use crate::components::rows::Rows;
use crate::services::asset_management::AssetSource;

/// Maximum live nodes in one root.
pub(in crate::world::systems::gui) const MAX_NODES: usize = 65_536;

/// Exclusive bound of node identities: one past the last slot every node row
/// table can address. Identities are never reused within a root incarnation,
/// so a long-lived root that exhausts them fails `InsertNode` until a new
/// incarnation (for example a restore) compacts its identities.
pub const MAX_GUI_NODE_ID: u32 = min_u32(
    min_u32(
        Rows::<GuiNodeStyleRow>::MAX_SLOTS,
        Rows::<GuiNodeDataRow>::MAX_SLOTS,
    ),
    Rows::<GuiNodeTreeRow>::MAX_SLOTS,
);
pub(in crate::world::systems::gui) const MAX_NODE_ID: u32 = MAX_GUI_NODE_ID;

/// Maximum UTF-8 bytes in one authored, committed, provisional or restored
/// GUI text value; the byte bound of every `node_tree` text property.
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
    /// Scrollable viewport over an authored item count, placing each child
    /// at its item index.
    VirtualList = 7,
}

impl GuiContainerKind {
    /// Whether this container scrolls its content through input-owned
    /// offsets, scroll bars and viewport clips: ScrollView and VirtualList.
    pub const fn is_scrollable(self) -> bool {
        matches!(self, Self::ScrollView | Self::VirtualList)
    }
}

/// Node kind: one code space covering every [`GuiNodeData`] variant, container
/// kinds included. The code is the `kind` property of a node's tree row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum GuiNodeKind {
    /// Horizontal container.
    Row = 0,
    /// Vertical container.
    Column = 1,
    /// Layered container.
    Stack = 2,
    /// Inner margin container.
    Padding = 3,
    /// Alignment container.
    Align = 4,
    /// Explicit dimensional container.
    SizedBox = 5,
    /// Scrollable viewport container.
    ScrollView = 6,
    /// Text leaf; the row's `text` is its content.
    Text = 7,
    /// Vector drawing leaf.
    Drawing = 8,
    /// Bitmap image leaf.
    Image = 9,
    /// Button; the row's `text` is its label.
    Button = 10,
    /// Checkbox control.
    Checkbox = 11,
    /// Range slider control.
    Slider = 12,
    /// Single-line text input; the row's `text` is its authored text and
    /// `committed_text` its committed value.
    TextInput = 13,
    /// Virtualized scrollable list; its children's order keys are their
    /// item indices.
    VirtualList = 14,
}

impl GuiNodeKind {
    /// Number of kind codes.
    pub const COUNT: u32 = 15;

    /// Every kind in code order; `ALL[i] as u32 == i`.
    pub const ALL: [Self; Self::COUNT as usize] = [
        Self::Row,
        Self::Column,
        Self::Stack,
        Self::Padding,
        Self::Align,
        Self::SizedBox,
        Self::ScrollView,
        Self::Text,
        Self::Drawing,
        Self::Image,
        Self::Button,
        Self::Checkbox,
        Self::Slider,
        Self::TextInput,
        Self::VirtualList,
    ];

    /// Kind of a row code, or None for an unknown code.
    pub const fn from_code(code: u32) -> Option<Self> {
        if code < Self::COUNT {
            Some(Self::ALL[code as usize])
        } else {
            None
        }
    }

    /// Row code of this kind.
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// Container kind, for container codes.
    pub const fn container(self) -> Option<GuiContainerKind> {
        Some(match self {
            Self::Row => GuiContainerKind::Row,
            Self::Column => GuiContainerKind::Column,
            Self::Stack => GuiContainerKind::Stack,
            Self::Padding => GuiContainerKind::Padding,
            Self::Align => GuiContainerKind::Align,
            Self::SizedBox => GuiContainerKind::SizedBox,
            Self::ScrollView => GuiContainerKind::ScrollView,
            Self::VirtualList => GuiContainerKind::VirtualList,
            _ => return None,
        })
    }

    /// Whether this kind carries a committed control value and a revision.
    pub const fn is_control(self) -> bool {
        matches!(self, Self::Checkbox | Self::Slider | Self::TextInput)
    }

    /// Whether the row's `text` is present: Text content, Button label or
    /// TextInput authored text.
    pub const fn has_text(self) -> bool {
        matches!(self, Self::Text | Self::Button | Self::TextInput)
    }

    /// Whether the row's `placeholder` and `committed_text` are present.
    pub const fn is_text_input(self) -> bool {
        matches!(self, Self::TextInput)
    }
}

impl From<GuiContainerKind> for GuiNodeKind {
    fn from(kind: GuiContainerKind) -> Self {
        match kind {
            GuiContainerKind::Row => Self::Row,
            GuiContainerKind::Column => Self::Column,
            GuiContainerKind::Stack => Self::Stack,
            GuiContainerKind::Padding => Self::Padding,
            GuiContainerKind::Align => Self::Align,
            GuiContainerKind::SizedBox => Self::SizedBox,
            GuiContainerKind::ScrollView => Self::ScrollView,
            GuiContainerKind::VirtualList => Self::VirtualList,
        }
    }
}

/// Node kind with its authored strings. Kind-specific scalars (image size,
/// checkbox and slider values, slider range and VirtualList items and
/// anchor) live in the root's `node_data` rows; see [`GuiNodeDataRow`].
///
/// Commands and inspection own their strings (`S = String`); readers borrow
/// them from the root's tree rows (`S = &str`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GuiNodeData<S = String> {
    /// Layout container holding child nodes.
    Container(GuiContainerKind),
    /// Text leaf.
    Text(S),
    /// Vector drawing asset leaf.
    Drawing,
    /// Bitmap image leaf; its display size is the `image_size` row property.
    Image,
    /// Clickable button.
    Button {
        /// Button label.
        label: S,
    },
    /// Checkbox control; its committed state is the `checked` row property.
    Checkbox,
    /// Range slider; `value`, `min`, `max` and `step` are row properties.
    Slider,
    /// Single-line text input field.
    TextInput {
        /// Authored initial text; the committed text is the row's
        /// `committed_text`.
        text: S,
        /// Placeholder when empty.
        placeholder: S,
    },
}

impl<S> GuiNodeData<S> {
    /// Node kind of this data.
    pub fn kind(&self) -> GuiNodeKind {
        match self {
            Self::Container(kind) => GuiNodeKind::from(*kind),
            Self::Text(_) => GuiNodeKind::Text,
            Self::Drawing => GuiNodeKind::Drawing,
            Self::Image => GuiNodeKind::Image,
            Self::Button {
                ..
            } => GuiNodeKind::Button,
            Self::Checkbox => GuiNodeKind::Checkbox,
            Self::Slider => GuiNodeKind::Slider,
            Self::TextInput {
                ..
            } => GuiNodeKind::TextInput,
        }
    }

    /// Whether this kind carries a committed control value.
    pub fn is_control(&self) -> bool {
        self.kind().is_control()
    }

    /// Whether both values are the same node kind, ignoring authored strings.
    pub fn same_kind<T>(&self, other: &GuiNodeData<T>) -> bool {
        self.kind() == other.kind()
    }
}

impl<S: AsRef<str>> GuiNodeData<S> {
    /// Borrow the authored strings.
    pub fn as_str(&self) -> GuiNodeData<&str> {
        match self {
            Self::Container(kind) => GuiNodeData::Container(*kind),
            Self::Text(text) => GuiNodeData::Text(text.as_ref()),
            Self::Drawing => GuiNodeData::Drawing,
            Self::Image => GuiNodeData::Image,
            Self::Button {
                label,
            } => GuiNodeData::Button {
                label: label.as_ref(),
            },
            Self::Checkbox => GuiNodeData::Checkbox,
            Self::Slider => GuiNodeData::Slider,
            Self::TextInput {
                text,
                placeholder,
            } => GuiNodeData::TextInput {
                text: text.as_ref(),
                placeholder: placeholder.as_ref(),
            },
        }
    }

    /// Copy the authored strings.
    pub fn to_owned_data(&self) -> GuiNodeData {
        match self.as_str() {
            GuiNodeData::Container(kind) => GuiNodeData::Container(kind),
            GuiNodeData::Text(text) => GuiNodeData::Text(text.to_owned()),
            GuiNodeData::Drawing => GuiNodeData::Drawing,
            GuiNodeData::Image => GuiNodeData::Image,
            GuiNodeData::Button {
                label,
            } => GuiNodeData::Button {
                label: label.to_owned(),
            },
            GuiNodeData::Checkbox => GuiNodeData::Checkbox,
            GuiNodeData::Slider => GuiNodeData::Slider,
            GuiNodeData::TextInput {
                text,
                placeholder,
            } => GuiNodeData::TextInput {
                text: text.to_owned(),
                placeholder: placeholder.to_owned(),
            },
        }
    }

    /// The row `text` this data authors: Text content, Button label or
    /// TextInput authored text.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text)
            | Self::Button {
                label: text,
            }
            | Self::TextInput {
                text,
                ..
            } => Some(text.as_ref()),
            _ => None,
        }
    }

    /// The row `placeholder` this data authors, for a text input.
    pub fn placeholder(&self) -> Option<&str> {
        match self {
            Self::TextInput {
                placeholder,
                ..
            } => Some(placeholder.as_ref()),
            _ => None,
        }
    }
}

impl<'a> GuiNodeData<&'a str> {
    /// Data of one tree row, or None for an unknown kind code. Absent strings
    /// read as empty.
    pub fn from_row(row: &'a GuiNodeTreeRow) -> Option<Self> {
        let kind = GuiNodeKind::from_code(row.kind)?;
        let text = row.text.as_deref().unwrap_or_default();
        Some(match kind {
            GuiNodeKind::Text => Self::Text(text),
            GuiNodeKind::Drawing => Self::Drawing,
            GuiNodeKind::Image => Self::Image,
            GuiNodeKind::Button => Self::Button {
                label: text,
            },
            GuiNodeKind::Checkbox => Self::Checkbox,
            GuiNodeKind::Slider => Self::Slider,
            GuiNodeKind::TextInput => Self::TextInput {
                text,
                placeholder: row.placeholder.as_deref().unwrap_or_default(),
            },
            container => Self::Container(container.container()?),
        })
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

/// Authored strings stay within the GUI text budget.
pub(in crate::world::systems::gui) fn valid_node_data<S: AsRef<str>>(
    data: &GuiNodeData<S>,
) -> bool {
    data.text().is_none_or(|text| text.len() <= MAX_TEXT_BYTES)
        && data
            .placeholder()
            .is_none_or(|text| text.len() <= MAX_TEXT_BYTES)
}
