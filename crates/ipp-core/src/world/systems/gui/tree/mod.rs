//! Authoritative GUI tree: node storage, root component and control state.
//!
//! The tree owns structure plus one control record per control node; node
//! style and kind-specific scalars, including committed checkbox and slider
//! values, are rows of the root keyed by node identity. Other GUI groups read
//! the tree through these boundary types; authoring writes go through
//! [`GuiCommand`](super::system::GuiCommand).

pub(super) mod component;
pub(super) mod controls;
pub(super) mod node_rows;
pub(super) mod nodes;

pub use component::GuiRoot;
pub use controls::{GuiControlEntry, GuiControlState, GuiControls};
pub use node_rows::{
    GuiNodeDataProperty, GuiNodeDataRow, GuiNodePropertyRef, GuiNodeRowProperty,
    GuiNodeStyleChange, GuiNodeStyleProperty, GuiNodeStyleRow,
};
pub use nodes::{
    GuiContainerKind, GuiControlValue, GuiNode, GuiNodeData, GuiNodeHandle, GuiNodeId,
    GuiNodePatch, GuiNodeStyle, GuiNodes, MAX_GUI_NODE_ID, MAX_GUI_TEXT_BYTES,
};
