//! State owned directly by one GUI world system instance.

use crate::EntityId;

/// Retained [`GuiSystem`](super::system::GuiSystem) state: the root whose
/// structural fields the system is currently committing, if any.
#[derive(Default)]
pub struct GuiSystemState {
    /// Root whose structural fields this System is currently committing.
    pub(super) committing: Option<EntityId>,
}
