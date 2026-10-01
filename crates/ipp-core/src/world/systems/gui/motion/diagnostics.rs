/// Actual request-preparation work in the latest evaluated frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiMotionPreparationWork {
    /// Motion-bearing controls whose eligibility/state was inspected.
    pub snapshots: usize,
    /// Part destinations considered on those affected controls.
    pub parts: usize,
}

/// Actual AnimationSystem skin work in the latest evaluated frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiMotionSamplingWork {
    /// Dirty or actively fading part owners visited.
    pub owners: usize,
    /// Binding attempts, including pending or invalid resources.
    pub bindings: usize,
    /// Composite samples installed by the sole sampler.
    pub samples: usize,
}

impl crate::WorldContext<'_> {
    /// Latest evaluated request and sampler counts; observing never evaluates a World.
    pub fn gui_motion_work(&self) -> Option<(GuiMotionPreparationWork, GuiMotionSamplingWork)> {
        use crate::systems::{animation::AnimationSystem, gui::GuiSystem};
        Some((
            self.system::<GuiSystem>(GuiSystem::ID)?.motion_work(),
            self.system::<AnimationSystem>(AnimationSystem::ID)?
                .motion_work(),
        ))
    }
}
