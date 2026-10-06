//! Control identity and the controls with their own geometry, value rules or
//! editing: every control's identity, eligibility and focus parts
//! ([`identity`]), the colour control, numeric text input and slider, and
//! native single-line text editing.

pub(crate) mod color;
pub(in crate::world::systems) mod identity;
pub(in crate::world::systems::gui) mod native_text;
pub(crate) mod number;
pub(crate) mod slider;
pub(crate) mod text_edit;

#[cfg(test)]
mod native_text_tests;
#[cfg(test)]
mod slider_tests;
#[cfg(test)]
mod submit_tests;

pub use native_text::{GuiNativeTextState, GuiTextComposition, GuiTextEdit, GuiTextFence};
pub use number::{GuiNumberStep, format_number, parse_number};
