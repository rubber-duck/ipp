//! GUI constraint evaluation and the retained layout pass.
//!
//! Ordinary [`GuiLayout`] components read core ordered entity links directly.
//! Retained entity geometry and shared text measurements feed Canvas production;
//! Canvas applies the same layout mapping to paint, hits and attachment placement.
//! Neither derived cache owns authoring values or another structural tree.
//! VirtualList item placement and range math live in [`virtual_list`], and
//! ScrollView bar geometry in [`scroll_bars`]; both are shared with input routing.

mod component;
mod entity_evaluation;
mod entity_layout;
mod geometry;
pub use component::GuiLayout;
pub use entity_layout::{GuiEntityLayout, GuiEntityLayoutDiagnostic};
pub use entity_layout::{GuiEntityLayoutStatistics, GuiEntityLayoutWork};
pub(crate) mod scroll_bars;
mod scroll_layout;
pub(super) mod system;
pub(crate) mod virtual_list;

pub use entity_evaluation::MAX_LAYOUT_DEPTH;
pub use system::{GuiLayoutSystem, GuiLayoutSystemFactory};
