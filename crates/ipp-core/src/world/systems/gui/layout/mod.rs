//! GUI constraint evaluation and the retained layout pass.
//!
//! Ordinary [`GuiLayout`] components read core ordered entity links directly.
//! Retained entity geometry and shared text measurements feed Canvas production;
//! Canvas applies the same layout mapping to paint, hits and attachment placement.
//! Neither derived cache owns authoring values or another structural tree.
//! A [`GuiOverlay`] leaves its parent's flow and is laid out afterwards as its
//! own root, placed against its parent's box or the canvas (`overlay_placement`).
//! VirtualList item placement and range math live in [`virtual_list`], and
//! ScrollView bar geometry in [`scroll_bars`]; both are shared with input routing.
//! The pass also scrolls a newly focused control into view (`reveal`).

mod component;
mod entity_evaluation;
mod entity_layout;
mod geometry;
mod overlay_placement;
mod reveal;
pub use component::{GuiLayout, GuiOverlay};
pub use entity_layout::{GuiEntityLayout, GuiEntityLayoutDiagnostic};
pub use entity_layout::{GuiEntityLayoutStatistics, GuiEntityLayoutWork};
pub(crate) mod scroll_bars;
mod scroll_layout;
pub(super) mod system;
pub(crate) mod virtual_list;

pub use entity_evaluation::MAX_LAYOUT_DEPTH;
pub use system::{GuiLayoutSystem, GuiLayoutSystemFactory};
