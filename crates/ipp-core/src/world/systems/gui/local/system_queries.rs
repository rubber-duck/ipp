//! Records of the GUI System queries: focus and live pointer feedback.
//!
//! Focus and pointer interaction are GUI System state, not component fields.
//! Clients read them through the `GuiFocus` and `GuiPointers` inspection
//! collections, paged like the other inspection collections by the target
//! entity identity.

use super::{GuiEntityTarget, GuiInteractionFlags};

/// Logical focus of one World, read through the `GuiFocus` System query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiFocusRecord {
    /// Focused control lifetime.
    pub target: GuiEntityTarget,
    /// Whether focus is indicated, as after keyboard traversal.
    pub visible: bool,
}

/// One live pointer's feedback on one control, read through the `GuiPointers`
/// System query. Records are ordered by target entity, then pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiPointerRecord {
    /// Control receiving this pointer's feedback.
    pub target: GuiEntityTarget,
    /// Pointer number, scoped by its GUI input session and context.
    pub pointer: u64,
    /// This pointer's flags on the control, not the control-wide aggregate.
    pub state: GuiInteractionFlags,
}
