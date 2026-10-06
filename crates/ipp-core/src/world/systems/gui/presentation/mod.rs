//! Ordinary control paint and immutable observations joined to completed Canvas output.
//! Compact theme/part rows remain typed authoring values, never a second entity tree.
//! Each control part resolves property by property from its skin's override row, its
//! theme's rows and finally the built-in default look of its kind ([`looks`]).
//!
//! A skinned entity that is not a control paints only its Background part, from the
//! same rows, during the same Canvas walk; it has no states, focus or hit, and is
//! absent from these observations.
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
pub mod looks;
pub(in crate::world::systems::gui) mod measurement;
pub(in crate::world::systems::gui) mod paint;
mod part_style;
pub(in crate::world::systems::gui) mod parts;
mod publication;
mod retained;

pub use component::{GUI_DEFAULT_FONT_SIZE, GuiFont, GuiPaintPart, GuiSkin, GuiTheme};
pub(crate) use contract::write_paint_contract;
pub use looks::{GuiSkinLook, gui_skin_looks};
pub(in crate::world::systems) use paint::{GuiPaintedControl, control_paint, skinned_background};
pub use part_style::{GuiPartStyle, GuiSkinState};
pub use parts::{GUI_BASE_PARTS, GuiPartId, GuiPartProperty, GuiPartVariant, GuiPrimitivePart};
pub use publication::{
    GuiCanvasPublication, GuiCanvasSemanticView, GuiControlObservation, GuiControlRecord,
    GuiGroupItem, GuiNumberGeometry, GuiOverlayObservation, GuiRoutingValue, GuiSemanticActionKind,
    GuiSliderGeometry, GuiSliderRange,
};
pub(in crate::world::systems) use retained::GuiCanvasState;
