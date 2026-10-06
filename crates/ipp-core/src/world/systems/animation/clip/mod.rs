//! Immutable, bounded property and pose keyframes and time/value Bézier segments,
//! and the IPPA clip asset format that stores them.

mod format;
mod track;

pub use format::ANIMATION_TYPE;
pub(crate) use format::animation_asset_loader;
pub(in crate::world::systems::animation) use track::AnimationSampleSegment;
pub use track::{
    AnimationClip, AnimationEntityPlacementKey, AnimationInterpolation, AnimationKeyframe,
    AnimationProperty, AnimationSample, AnimationTrack, AnimationTrackData, AnimationTrackTarget,
    AnimationValue, IntoAnimationTrack,
};
