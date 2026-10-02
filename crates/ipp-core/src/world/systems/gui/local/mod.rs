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
//! Focus, pointer feedback, groups' active items, overlays' invokers, hint
//! delays, held step repeats and native text composition, including a
//! numeric text input's edit ([`number`]), are GUI System state; focus and
//! pointer feedback name a focus part of a control with several, such as a
//! range slider's thumb or a colour control's field or rail
//! ([`control::focus_parts`]), and pointer feedback a
//! numeric input's step part. The first three
//! read through the `GuiFocus`, `GuiPointers` and `GuiActiveItems` System
//! queries, and none is persisted. Momentary effects use
//! the World-owned publisher; values reach clients through field observation.
//! Groups keep their selection in their items' `selected` fields.

mod actions;
pub(crate) mod color;
mod command;
mod component;
pub(in crate::world::systems) mod control;
mod eligibility;
pub(in crate::world::systems) mod group;
mod hint;
mod interaction;
pub(crate) mod number;
mod overlay;
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
    GUI_GROUP_BOTH, GUI_GROUP_HORIZONTAL, GUI_GROUP_SELECT_FOLLOW, GUI_GROUP_SELECT_NONE,
    GUI_GROUP_SELECT_SINGLE, GUI_GROUP_VERTICAL, GUI_SLIDER_DIAL, GuiBehavior, GuiButton,
    GuiCheckbox, GuiColor, GuiGroup, GuiSlider, GuiTextInput, MAX_GUI_NUMBER_PRECISION,
    MAX_GUI_TEXT_BYTES,
};
pub(in crate::world::systems) use interaction::{GUI_MAX_FOCUS_PARTS, GuiPartInteraction};
pub use interaction::{
    GuiInteractionEffect, GuiInteractionFlags, GuiInteractionPart, GuiInteractionUpdate,
};
pub use number::{GuiNumberStep, format_number, parse_number};
pub use overlay::GuiOverlayCommand;
pub use scroll_chain::GuiScrollChain;
pub use scroll_component::{GuiScrollView, GuiVirtualItem, GuiVirtualList};
pub use system_queries::{GuiActiveItemRecord, GuiFocusRecord, GuiPointerRecord};
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
mod slider_tests;
#[cfg(test)]
mod submit_tests;
#[cfg(test)]
mod text_tests;
#[cfg(test)]
mod validation_tests;
