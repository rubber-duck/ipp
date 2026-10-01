//! Explicit object relationships and affine propagation. See README.md for contracts.

#[cfg(feature = "skeletal-animation")]
mod component;
#[cfg(feature = "skeletal-animation")]
pub use component::ParentJoint;

use crate::systems::{self, camera::CameraAffineTransform, geometry::GeometryShapeTransform};
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{EntityId, ErrorReason, components::Transform};
use std::collections::BTreeSet;

mod system;
pub use system::{
    FinalPropagationSystem, FinalPropagationSystemFactory, HierarchySystem, HierarchySystemFactory,
};

mod system_state;
pub(crate) use system_state::HierarchyGraph;

mod update;
pub(crate) use update::affine;
pub(in crate::world) use update::{evaluated_affine, parent_affine};

#[cfg(test)]
mod scaling_tests;

mod propagation;
mod transform_binding;
pub(in crate::world) use transform_binding::ObjectTransformBinding;
pub(crate) use transform_binding::ObjectTransformRuntime;
