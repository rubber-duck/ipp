//! Ordinary skin requests and private channels sampled only by AnimationSystem.

mod component;
mod diagnostics;
mod runtime;
mod update;

pub use component::{GuiMotionPart, GuiThemeMotion};
pub use diagnostics::{GuiMotionPreparationWork, GuiMotionSamplingWork};
pub(in crate::world) use runtime::{
    GuiMotionChannels, GuiMotionDestination, GuiMotionOwner, GuiMotionRequest, GuiMotionSkinTarget,
    theme_live,
};
pub use runtime::{GuiSkinMotionStatus, GuiSkinRuntime};
pub(in crate::world::systems::gui) use update::GuiMotionState;
