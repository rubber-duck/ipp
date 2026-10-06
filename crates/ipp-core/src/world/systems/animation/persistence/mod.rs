//! Animation-owned persistent controller state: World save and restore hooks,
//! entity remapping, and the versioned binary codec.

mod capture;
mod codec;

pub use capture::{
    AnimationFrozenTransitionValue, AnimationPersistentContribution, AnimationPersistentState,
    AnimationPersistentTransition, AnimationPersistentTransitionSource,
};
