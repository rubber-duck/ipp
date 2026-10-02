//! Records of the GUI System queries: focus, live pointer feedback and the
//! active items of groups.
//!
//! Focus, pointer interaction and active items are GUI System state, not
//! component fields. Clients read them through the `GuiFocus`, `GuiPointers`
//! and `GuiActiveItems` inspection collections, paged like the other
//! inspection collections by an entity identity: the target's, or the
//! group's for active items.

use super::{GuiEntityTarget, GuiInteractionFlags};
use crate::EntityId;

/// Logical focus of one World, read through the `GuiFocus` System query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiFocusRecord {
    /// Focused control lifetime.
    pub target: GuiEntityTarget,
    /// Whether focus is indicated, as after keyboard traversal.
    pub visible: bool,
    /// The focused part of the control, such as a range's upper thumb 1; 0
    /// for a control with one part.
    pub part: u32,
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

/// One group's active item, read through the `GuiActiveItems` System query.
/// Records are ordered by group entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiActiveItemRecord {
    /// The entity holding the `GuiGroup`.
    pub group: EntityId,
    /// The active item's control lifetime.
    pub target: GuiEntityTarget,
}
