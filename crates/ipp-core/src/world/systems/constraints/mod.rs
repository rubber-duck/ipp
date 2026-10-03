//! Component constraint declarations, compiled properties and dependency-ordered evaluation.
//!
//! [`LinearDriver`] binds one source Scalar; [`ExpressionDriver`] maps fields
//! on one typed source entity into a shared immutable expression plan. Both write
//! properties on their own entity. Successful absolute writes land
//! before animation, so animation contributions to a target apply
//! on top of the constraint's value. The target keeps no memory of its value from
//! before the binding: a binding that departs leaves the last written value.
//! Invalid expressions retain the target and expose [`ExpressionDriverStatus`].
//! Drivers evaluate by resolved property overlap, with sources before targets.
//! A self-dependency or
//! cycle makes its members invalid: they do not evaluate, their targets keep
//! their values, the batch still succeeds, and a Warn diagnostic names each
//! member. Correcting any member's declaration or binding restores evaluation.

mod component;
pub use component::{ExpressionDriver, LinearDriver};

mod input_mapping;
pub use input_mapping::{
    DriverProperty, EXPRESSION_DRIVER_MAX_INPUTS, ExpressionDriverInput,
    decode_expression_driver_inputs, encode_expression_driver_inputs,
};

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

mod dependency_graph;
mod expression_binding;
mod status;
pub use status::{
    ExpressionDriverAvailability, ExpressionDriverReason, ExpressionDriverState,
    ExpressionDriverStatus,
};

mod update;

#[cfg(test)]
mod expression_tests;
