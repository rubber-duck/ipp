//! Authoritative GUI tree: node storage, root component and control state.
//!
//! The tree is the root's `node_tree` rows table: per node its parent,
//! sparse sibling order key, kind, bounded authored strings, committed text
//! and control revision. Node style and kind-specific scalars, including
//! committed checkbox and slider values, are rows of the root keyed by the
//! same node identity. Child order is derived by the GUI System into a
//! [`GuiTreeIndex`]. Skins are root-owned theme part rows referenced by node,
//! with per-node part rows for overrides and live transition channels. Other
//! GUI groups read the tree through these boundary types; authoring writes go
//! through [`GuiCommand`](super::system::GuiCommand).

pub(super) mod component;
pub(super) mod controls;
pub(super) mod node_rows;
pub(super) mod node_tree;
pub(super) mod node_writes;
pub(super) mod nodes;
pub(super) mod part_rows;
pub(super) mod tree_index;

pub use component::{GuiRoot, GuiRootRowProperty};
pub use controls::GuiControlState;
pub use node_rows::{
    GuiNodeDataProperty, GuiNodeDataRow, GuiNodePropertyRef, GuiNodeRowProperty,
    GuiNodeStyleChange, GuiNodeStyleProperty, GuiNodeStyleRow,
};
pub use node_tree::{GuiNode, GuiNodeTree, GuiNodeTreeProperty, GuiNodeTreeRow, GuiNodes};
pub use nodes::{
    GuiContainerKind, GuiControlValue, GuiNodeData, GuiNodeHandle, GuiNodeId, GuiNodeKind,
    GuiNodePatch, GuiNodeStyle, MAX_GUI_NODE_ID, MAX_GUI_TEXT_BYTES,
};
pub use part_rows::{
    GUI_BASE_PARTS, GuiPartChannel, GuiPartId, GuiPartPatch, GuiPartProperty, GuiPartRow,
    GuiPartRowProperty, GuiPartVariant, GuiThemePartRow,
};
pub use tree_index::GuiTreeIndex;
