//! Ordinary skin requests and private channels sampled only by AnimationSystem.

mod component;
#[cfg(feature = "diagnostics")]
mod diagnostics;
mod runtime;
mod update;

pub use component::{GuiMotionPart, GuiThemeMotion};
#[cfg(feature = "diagnostics")]
pub use diagnostics::{GuiMotionPreparationWork, GuiMotionSamplingWork};
pub(in crate::world) use runtime::{
    GuiMotionChannels, GuiMotionDestination, GuiMotionOwner, GuiMotionRequest, GuiMotionSkinTarget,
    theme_live,
};
pub use runtime::{GuiSkinMotionStatus, GuiSkinRuntime};
pub(in crate::world::systems::gui) use update::GuiMotionState;
