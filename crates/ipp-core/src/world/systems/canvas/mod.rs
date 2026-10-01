//! Ordinary entity content evaluated into the retained, immutable output of the
//! World's one canvas.
//!
//! A World selecting this System is a canvas: every top-level entity and its
//! descendants are canvas content, painted depth-first in core sibling order. The
//! canvas's extent and density are this System's [`CanvasState`], set at World
//! creation and changed by [`CanvasStateUpdate`] at the mutation boundary. A
//! Surface presenting the canvas scales its physical size by the density, a root
//! viewport supplies CSS pixels and ignores the density, and the stored extent
//! applies otherwise. Leaf geometry is prepared only after relevant input/resource
//! changes, and unchanged output, primitive and glyph chunks retain their Arcs.
//!
//! Every Surface anchor is a canvas slot whose retained mapping is shared with the
//! generic attachment-placement hook before child evaluation; spatial attachments
//! keep their spatial placement. Completed edges must match the canvas output,
//! anchor and exact applied-write token; availability, gates and presentation-path
//! admission remain Host/router responsibilities. Slots without a usable Surface
//! mapping never fall back to unrelated spatial placement.
//!
//! Raw content has no control or semantic actions. GUI supplies its local control
//! state, semantics, compact parts and interaction priority through this same paint
//! boundary; it does not write a parallel Surface item list.
//!
//! [`canvas_state`] declares the canvas state in every build, so World creation and
//! the protocol name it without the `surfaces` feature; only a World selecting this
//! System accepts it, and the System saves it through its persistence hook.

pub mod canvas_state;

pub use canvas_state::{CanvasEvaluatedExtent, CanvasState, CanvasStateRecord, CanvasStateUpdate};
#[cfg(feature = "surfaces")]
mod component;
#[cfg(feature = "surfaces")]
mod publication;
#[cfg(feature = "surfaces")]
mod system;
#[cfg(feature = "surfaces")]
mod system_state;
#[cfg(all(test, feature = "gui"))]
pub(crate) mod test_support;
#[cfg(feature = "surfaces")]
mod update;

#[cfg(feature = "gui")]
pub use component::CanvasBounds;
#[cfg(feature = "surfaces")]
pub use component::{
    CanvasBitmap, CanvasBox, CanvasDrawing, CanvasGlyphRow, CanvasGlyphRun, CanvasStyle, CanvasText,
};
#[cfg(feature = "surfaces")]
pub use publication::{
    CanvasAttachmentSlot, CanvasAxis, CanvasClip, CanvasGlyph, CanvasHit, CanvasHitKind,
    CanvasInteractionPriority, CanvasPaintEntry, CanvasPart, CanvasPrimitive, CanvasPrimitiveId,
    CanvasPrimitiveStyle, CanvasPublication, CanvasShapeFill, CanvasShapeGlow, CanvasTarget,
};
#[cfg(feature = "surfaces")]
pub use system::{CanvasSystem, CanvasSystemFactory};
#[cfg(feature = "gui")]
pub(in crate::world::systems) use system_state::CanvasGeometry;
#[cfg(feature = "gui")]
pub(in crate::world::systems) use update::{logical_extent, prepare_constrained_geometry};
