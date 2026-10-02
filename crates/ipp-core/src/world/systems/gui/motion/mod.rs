//! Skin transitions between interaction states, prepared by GUI and sampled
//! only by AnimationSystem.

mod component;
mod diagnostics;
mod runtime;
mod timing;
mod update;

pub use component::{GuiMotionEasing, GuiMotionPart, GuiThemeMotion};
pub use diagnostics::{GuiMotionPreparationWork, GuiMotionSamplingWork};
pub use runtime::GuiMotionRuntime;
pub(in crate::world::systems::gui) use runtime::focus_part_channel;
pub(in crate::world) use runtime::{GuiMotionOwner, notify_sample};
pub(in crate::world::systems::gui) use update::GuiMotionState;
