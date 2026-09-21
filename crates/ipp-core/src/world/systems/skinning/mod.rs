//! Mesh-local palettes stored directly on each Skin component.

mod component;
pub use component::{Skin, SkinRuntimeState};

use crate::{
    EntityId, ErrorReason, WorldContext,
    systems::{camera, skeleton},
};

mod system_state;
pub use system_state::SkinningSystemState;

mod system;
pub use system::{SkinningSystem, SkinningSystemFactory};

mod update;
pub(in crate::world) use update::palette;
