//! Authoritative GUI tree: node storage, root component and control state.
//!
//! The tree owns structure plus one committed value and revision per
//! control node. Other GUI groups read the tree through these boundary
//! types; authoring writes go through [`GuiCommand`](super::system::GuiCommand).

pub(super) mod component;
pub(super) mod controls;
pub(super) mod nodes;

pub use component::GuiRoot;
pub use controls::{GuiControlState, GuiControls};
pub use nodes::{
    GuiContainerKind, GuiControlValue, GuiNode, GuiNodeContent, GuiNodeHandle, GuiNodeId,
    GuiNodePatch, GuiNodeStyle, GuiNodes, MAX_GUI_TEXT_BYTES,
};
