//! Scrolling controls: the ScrollView and VirtualList components, the fields
//! a scroll action reads and the chain a physical scroll delta crosses.

mod chain;
mod component;
mod fields;

#[cfg(test)]
mod scroll_tests;

pub use chain::GuiScrollChain;
pub use component::{GuiScrollView, GuiVirtualItem, GuiVirtualList};
pub(in crate::world::systems::gui::local) use fields::GuiScrollFields;
