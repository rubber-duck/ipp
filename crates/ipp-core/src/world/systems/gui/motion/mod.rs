//! Skin transitions between interaction states, prepared by GUI and sampled
//! only by AnimationSystem.

mod component;
mod runtime_state;
mod statistics;
mod timing;
mod update;

#[cfg(test)]
mod lifetime_tests;

pub use component::{GuiMotionEasing, GuiMotionPart, GuiThemeMotion};
pub use runtime_state::GuiMotionRuntime;
pub(in crate::world::systems::gui) use runtime_state::focus_part_channel;
pub(in crate::world) use runtime_state::{GuiMotionOwner, notify_sample};
pub use statistics::{GuiMotionPreparationWork, GuiMotionSamplingWork};
pub(in crate::world::systems::gui) use update::GuiMotionState;
