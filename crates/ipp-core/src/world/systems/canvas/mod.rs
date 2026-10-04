//! Ordinary entity content evaluated into the retained, immutable output of the
//! World's one canvas.
//!
//! A World selecting this System is a canvas: every top-level entity and its
//! descendants are canvas content, painted depth-first in core sibling order
//! within each layer. [`layers`] sums relative `CanvasStyle.layer` offsets and
//! orders complete GUI overlay scopes within semantic priority bands. Every
//! primitive, hit and slot carries a compact physical rank. Ordinary raised
//! content retains ancestor clips; an overlay resets clipping at the canvas.
//! Paint and hits sort by rank, while hits retain tree ordinals for keyboard
//! traversal. All-zero ordinary content keeps the tree-order fast path. Closed
//! overlay subtrees have no output or occupied group. Open overlays publish in
//! stacking order through GUI observations. The
//! canvas's extent and density are this System's [`CanvasState`], set at World
//! creation and changed by [`CanvasStateUpdate`] at the mutation boundary. A
//! Surface presenting the canvas scales its physical size by the density, a root
//! viewport supplies CSS pixels and ignores the density, and the stored extent
//! applies otherwise. Leaf geometry is prepared only after relevant input/resource
//! changes, and unchanged output, primitive and glyph chunks retain their Arcs.
//!
//! The [`walk`] retains what it derived for each entity. A change that affects
//! neither structure, GUI layout nor layers, such as an animated style, a leaf
//! value or a sampled skin transition, is [patched](patch): only the changed
//! entities' subtrees are walked again, their entries, hits and observations are
//! replaced in place, and the publication's `paint_changes` names the replaced
//! entries so the renderer looks only at them. Every other change walks the
//! whole canvas. [`CanvasWork`] counts each evaluation's work.
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
//! boundary; it does not write a parallel Surface item list. A skinned entity that
//! is not a control paints its GUI Background part in its own place in the walk,
//! replacing a CanvasBox's plain paint, without a hit record.
//!
//! A CanvasPaint turns its entity's own box fill, a CanvasBox or the Background
//! part of its skin or control, into a paint fill naming the component, and the
//! publication carries each named paint instance with its shader and property
//! values beside the entries, so a property write republishes only the
//! instances and their own revision.
//!
//! [`canvas_state`] declares the canvas state for every World, so World creation and
//! the protocol always name it; only a World selecting this System accepts it, and
//! the System saves it through its persistence hook.

pub mod canvas_state;

pub use canvas_state::{CanvasEvaluatedExtent, CanvasState, CanvasStateRecord, CanvasStateUpdate};
mod component;
mod diagnostics;
mod layers;
mod patch;
mod publication;
mod system;
mod system_state;
#[cfg(test)]
pub(crate) mod test_support;
mod update;
mod walk;

pub use component::CanvasBounds;
pub use component::{
    CanvasBitmap, CanvasBox, CanvasDrawing, CanvasGlyphRow, CanvasGlyphRun, CanvasPaint,
    CanvasStyle, CanvasText,
};
pub use diagnostics::CanvasWork;
pub use publication::{
    CanvasAttachmentSlot, CanvasAxis, CanvasBoxShape, CanvasClip, CanvasGlyph, CanvasHit,
    CanvasHitKind, CanvasInteractionPriority, CanvasPaintChanges, CanvasPaintEntry,
    CanvasPaintInstance, CanvasPart, CanvasPrimitive, CanvasPrimitiveId, CanvasPrimitiveStyle,
    CanvasPublication, CanvasShapeChecker, CanvasShapeFill, CanvasShapeGlow, CanvasTarget,
};
pub use system::{CanvasSystem, CanvasSystemFactory};
pub(in crate::world::systems) use system_state::CanvasGeometry;
pub(in crate::world::systems) use update::{logical_extent, prepare_constrained_geometry};
