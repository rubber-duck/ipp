//! World-owned controller operations: ordered commands, the contributions each
//! controller adds to its fields, and interruptible crossfades.

mod commands;
pub(in crate::world::systems::animation) mod contribution;
mod transition;

pub(in crate::world::systems::animation) use commands::{
    add_description_demand, description_bytes, directional_start, validate_control,
};
pub(in crate::world::systems::animation) use transition::{
    AnimationTransitionProgram, frozen_values_invalidated,
};
