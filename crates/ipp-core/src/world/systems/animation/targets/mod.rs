//! Driver binding to animated targets: typed property, dynamic, row, numeric,
//! joint and structural destinations, and the component values they stage.

mod binding;
mod component_values;
mod driver;
mod numeric_binding;
mod numeric_output;
pub(in crate::world::systems::animation) mod pose;
mod row_property_destination;
mod structural;

pub(in crate::world::systems::animation) use binding::{
    present_field, removable_field, validate_target_support,
};
pub(in crate::world::systems::animation) use component_values::{
    AnimationComponentValues, ComponentScratch, PropertyScratch,
};
pub use driver::AnimationDriver;
pub(in crate::world::systems::animation) use driver::{
    AnimationDriverBinding, AnimationRuntimeTarget, AnimationTargetIdentity,
    AnimationTransitionOutput, bind_frozen_transition_output, frozen_transition_target_supported,
};
pub(in crate::world::systems::animation) use numeric_output::AnimationNumericOutput;
pub(in crate::world::systems::animation) use structural::AnimationStructuralDriver;
