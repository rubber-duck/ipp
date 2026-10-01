//! Scalar constraint declarations, strict bindings and dependency-ordered evaluation.
//!
//! Each [`LinearDriver`] binds one source Scalar and writes its target's value
//! every frame, before animation, so animation contributions to a target apply
//! on top of the constraint's value. The target keeps no memory of its value from
//! before the binding: a binding that departs leaves the last written value.
//! Drivers evaluate with every source before its target. A self-dependency or
//! cycle makes its members invalid: they do not evaluate, their targets keep
//! their values, the batch still succeeds, and a Warn diagnostic names each
//! member. Correcting any member's declaration or binding restores evaluation.

mod component;
pub use component::LinearDriver;

use crate::{
    ComponentValue, EntityId, ErrorReason,
    components::registry::ComponentStorage,
    world::{WorldEntityState, WorldMutationState, WorldSimulationState},
};
use std::collections::{BTreeMap, BTreeSet};

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
