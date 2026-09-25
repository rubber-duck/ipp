//! Ordered GUI input routing with focus, capture and control actions.
//!
//! The input system routes pointer, key, scroll, text and semantic-action
//! commands against retained layout views. Single-line text editing,
//! provisional IME composition, keyboard panel order, target liveness
//! policy and VirtualList anchoring live in the child modules below; control commits apply through
//! the tree.

pub(super) mod composition;
pub(super) mod keyboard_panels;
pub(super) mod system;
pub(super) mod target_policy;
pub(super) mod text_edit;
pub(super) mod virtual_scroll;

pub use system::{
    GuiCommitSource, GuiInputCancelReason, GuiInputCancellation, GuiInputCommand, GuiInputConflict,
    GuiInputConflictReason, GuiInputEffect, GuiInputEffectKind, GuiInputFocus, GuiInputSystem,
    GuiInputSystemFactory, GuiInputTarget, GuiKey, GuiPointerButton, GuiTextCompositionState,
    GuiTextFence, GuiTextFocusState, GuiTextFocusUpdate, GuiUnhandledInput, GuiUnhandledReason,
};
