//! Ordinary control paint and immutable observations joined to completed Canvas output.
//! Compact theme/part rows remain typed authoring values, never a second entity tree.
//!
//! Consumers join `GuiCanvasPublication` from CanvasSystem's System chunk with the
//! exact selected Canvas output in the same WorldPublication, then join each
//! `CanvasHitKind::Entity` by its control component incarnation. Input revisions
//! include semantic-only changes; these observations confer no action authority.
//! Geometry uses Canvas top-left logical space and already-intersected clips.
//! Slider rails remain control-local before the hit's signed position/scale map.
//! Decorative parts never replace the control's ordinary entity hit identity.
//!
//! Paint borrows shared themes and reuses immutable glyphs, entries and semantic
//! records. Pending asset replacement retains only a same-incarnation part's
//! previous ready resource, under completed publication leases, while recomputing
//! its placement from current layout. Headless local semantics do not depend on
//! this presentation chunk or a selected root.

mod component;
mod contract;
pub(in crate::world::systems::gui) mod measurement;
pub(in crate::world::systems::gui) mod paint;
mod part_style;
pub(in crate::world::systems::gui) mod parts;
mod publication;
mod system_state;

pub use component::{GuiFont, GuiPaintPart, GuiSkin, GuiTheme};
pub(crate) use contract::write_paint_contract;
pub(in crate::world::systems) use paint::{GuiPaintedControl, control_paint, scroll_bars_kept};
pub use part_style::{FOCUS_BORDER_COLOR, FOCUS_BORDER_WIDTH, GuiPartStyle, GuiSkinState};
pub use parts::{GUI_BASE_PARTS, GuiPartId, GuiPartProperty, GuiPartVariant, GuiPrimitivePart};
pub use publication::{
    GuiCanvasPublication, GuiCanvasSemanticView, GuiControlObservation, GuiControlRecord,
    GuiRoutingValue, GuiSemanticActionKind, GuiSliderGeometry,
};
pub(in crate::world::systems) use system_state::GuiCanvasState;
