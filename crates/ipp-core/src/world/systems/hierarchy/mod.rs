//! Explicit object relationships and affine propagation. See README.md for contracts.

mod component;
pub use component::{Hierarchy, HierarchyRuntimeState};

use crate::systems::{self, camera::CameraAffineTransform, geometry::GeometryShapeTransform};
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, EntityId, ErrorReason, components::Transform};
use std::collections::{BTreeMap, BTreeSet};

mod system;
pub use system::{
    FinalPropagationSystem, FinalPropagationSystemFactory, HierarchySystem, HierarchySystemFactory,
};

mod system_state;
pub(crate) use system_state::HierarchyGraph;

mod update;
pub(crate) use update::affine;
pub(in crate::world) use update::{evaluated_affine, local_affine, parent_affine};

#[cfg(test)]
mod scaling_tests;

mod propagation;
mod transform_binding;
pub(in crate::world) use transform_binding::ObjectTransformBinding;
pub(crate) use transform_binding::ObjectTransformRuntime;
