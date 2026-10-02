/// Actual transition-preparation work in the latest evaluated frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiMotionPreparationWork {
    /// Changed controls whose interaction key was inspected.
    pub snapshots: usize,
    /// Part destinations resolved on those controls whose key changed or
    /// whose transition was retargeted.
    pub parts: usize,
}

/// Actual AnimationSystem skin work in the latest evaluated frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiMotionSamplingWork {
    /// Transitioning parts visited.
    pub owners: usize,
    /// Transitions started, whose clock began this frame.
    pub bindings: usize,
    /// Samples written by the sole sampler.
    pub samples: usize,
}

impl crate::WorldContext<'_> {
    /// Latest evaluated preparation and sampler counts; observing never
    /// evaluates a World.
    pub fn gui_motion_work(&self) -> Option<(GuiMotionPreparationWork, GuiMotionSamplingWork)> {
        use crate::systems::{animation::AnimationSystem, gui::GuiSystem};
        Some((
            self.system::<GuiSystem>(GuiSystem::ID)?.motion_work(),
            self.system::<AnimationSystem>(AnimationSystem::ID)?
                .motion_work(),
        ))
    }
}
