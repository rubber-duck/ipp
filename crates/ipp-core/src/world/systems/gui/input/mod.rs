//! Ordered GUI input routing with focus, capture and control actions.
//!
//! The input system routes pointer, key, scroll, text and semantic-action
//! commands against retained layout views. Single-line text editing,
//! provisional IME composition and target liveness policy live in the
//! child modules below; control commits apply through the tree.

pub(super) mod composition;
pub(super) mod system;
pub(super) mod target_policy;
pub(super) mod text_edit;

pub use system::{
    GuiInputCancelReason, GuiInputCancellation, GuiInputCommand, GuiInputConflict,
    GuiInputConflictReason, GuiInputEffect, GuiInputEffectKind, GuiInputFocus, GuiInputSystem,
    GuiInputSystemFactory, GuiInputTarget, GuiKey, GuiPointerButton, GuiTextCompositionState,
    GuiTextFocusState, GuiTextFocusUpdate, GuiUnhandledInput, GuiUnhandledReason,
};
