//! State preservation and color normalization shared by native GPU-image imports.

mod copy;
mod state;

pub(super) use copy::GlesExternalImageCopy;
pub(super) use state::State;
