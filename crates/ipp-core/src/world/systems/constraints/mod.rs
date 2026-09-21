//! Scalar constraint declarations, strict bindings and fixed entity-slot evaluation.

mod component;
pub use component::LinearDriver;

use crate::{
    ComponentValue, EntityId, ErrorReason,
    components::registry::ComponentStorage,
    world::{WorldEntityState, WorldMutationState, WorldSimulationState},
};
#[cfg(debug_assertions)]
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScalarConstraintBinding {
    pub(crate) source: EntityId,
    source_incarnation: u64,
    target_incarnation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::world) struct ConstraintDriverIdentity {
    incarnation: u64,
    source: EntityId,
}

mod system_state;
pub use system_state::ConstraintSystemState;

mod system;
pub use system::{ConstraintSystem, ConstraintSystemFactory};

mod update;
