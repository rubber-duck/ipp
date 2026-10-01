//! Ordinary-entity control declarations, their value fields and local actions.
//!
//! Control values, scroll state and eligibility are component fields; the
//! component store is their only source of truth. The GUI System writes values
//! at the mutation boundary for actions, routed input and native text through
//! the ordinary write path; layout writes scroll geometry and the normalized
//! position in its pass; eligibility on `GuiBehavior` is refreshed after each
//! commit for the affected subtrees. Any client may write these fields
//! directly and may use compare-and-set for conditional writes.
//!
//! A client's semantic action is a [`Command::GuiAction`](crate::Command::GuiAction)
//! applied at its operation's mutation boundary: the control lifetime, its
//! stored eligibility, the control role and the resulting value are checked in
//! that order, and the batch outcome is its only result. Routed physical input
//! is a delivery-reserved [`GuiLocalCommand`] that also needs a presented path
//! and view/context fences, and settles through its ticket.
//!
//! Focus, pointer feedback and native text composition are GUI System state,
//! read through the `GuiFocus` and `GuiPointers` System queries and never
//! persisted. Momentary effects use the World-owned publisher; values reach
//! clients through field observation.

mod actions;
mod command;
mod component;
pub(in crate::world::systems) mod control;
mod eligibility;
mod interaction;
mod scroll;
mod scroll_chain;
mod scroll_component;
pub(crate) mod slider;
mod system_queries;
mod system_state;
mod text;
pub(crate) mod text_edit;
mod types;

pub use command::GuiLocalCommand;
pub use component::{
    GuiBehavior, GuiButton, GuiCheckbox, GuiSlider, GuiTextInput, MAX_GUI_TEXT_BYTES,
};
pub(in crate::world::systems) use interaction::GuiPartInteraction;
pub use interaction::{
    GuiInteractionEffect, GuiInteractionFlags, GuiInteractionPart, GuiInteractionUpdate,
};
pub use scroll_chain::GuiScrollChain;
pub use scroll_component::{GuiScrollView, GuiVirtualItem, GuiVirtualList};
pub use system_queries::{GuiFocusRecord, GuiPointerRecord};
pub(in crate::world::systems::gui) use system_state::GuiLocalState;
pub use text::{GuiNativeTextState, GuiTextComposition, GuiTextEdit, GuiTextFence};
pub use types::{
    GuiControlKind, GuiEntityTarget, GuiLocalAction, GuiLocalActionError, GuiLocalEffect,
    GuiLocalEffectKind, GuiLocalEffectSource,
};

#[cfg(test)]
mod input_test_support;
#[cfg(test)]
mod interaction_tests;
#[cfg(test)]
mod lifetime_tests;
#[cfg(test)]
mod local_tests;
#[cfg(test)]
mod observation_tests;
#[cfg(test)]
mod receiver_tests;
#[cfg(test)]
mod scroll_tests;
#[cfg(test)]
mod submit_tests;
#[cfg(test)]
mod text_tests;
#[cfg(test)]
mod validation_tests;
