//! Typed immutable animation curves and world-owned multi-entity controllers.

mod binding;
mod clip;
mod component_values;
mod controller_commands;
mod driver;
mod lifecycle;
mod math;
mod numeric_binding;
mod numeric_fields;
mod numeric_output;
mod persistence;
mod transition;
mod update;
mod world_api;
pub(in crate::world) use update::{AnimationAccess, AnimationReadAccess};
#[cfg(feature = "gui")]
pub(in crate::world) use world_api::{
    AnimationCommand, AnimationInternalCommand, GuiSkinAnimationOwner, GuiSkinAnimationSample,
};

#[cfg(feature = "profiling")]
mod sampling_profile;

#[cfg(feature = "skeletal-animation")]
mod pose;

pub(crate) use clip::animation_asset_loader;
pub use clip::{
    ANIMATION_TYPE, AnimationClip, AnimationInterpolation, AnimationKeyframe, AnimationProperty,
    AnimationSample, AnimationTrack, AnimationTrackData, AnimationTrackTarget, AnimationValue,
    IntoAnimationTrack,
};
pub use driver::AnimationDriver;
pub(crate) use math::normalize;
pub(crate) use math::{additive, mix};

use crate::{EntityId, ErrorReason};

/// Absolute controller-count guard, independent of reservation hints.
pub const MAX_CONTROLLERS: usize = 16_384;

/// World-local controller identity, never recycled during a World's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationControllerId(u64);

impl AnimationControllerId {
    /// Recover a serialized identity. Zero is rejected by controller operations.
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Persistent world-local identity.
    pub const fn to_bits(self) -> u64 {
        self.0
    }
}

/// Serializable binding of one immutable source track to one entity property.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationDriverDescription {
    /// Clip source URI.
    pub source: String,
    /// Immutable source variant.
    pub variant: u32,
    /// Stable track index in the clip.
    pub track: u32,
    /// Exact target entity generation.
    pub target: EntityId,
    /// Exact component property coverage or joint ordinals.
    pub property: AnimationTrackTarget,
    /// Contribution weight in 0..=1.
    pub weight: f32,
    /// Apply a sampled delta from the reference value.
    pub additive: bool,
    /// Reference time for an additive driver.
    pub reference_time: f32,
    /// Repeat this clip on the controller clock instead of holding its endpoint.
    pub repeat: bool,
}

/// Authored controller parameters. Drivers may span arbitrary entities and clips.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationControllerDescription {
    /// Ordered independent target bindings sampled at one shared time.
    pub drivers: Vec<AnimationDriverDescription>,
    /// Finite signed clock multiplier.
    pub speed: f32,
    /// Wrap at the longest selected clip's duration.
    pub looping: bool,
}

/// Curve applied to crossfade progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum AnimationTransitionEasing {
    /// Constant-rate crossfade.
    #[default]
    Linear = 0,
    /// Cubic smoothstep with zero slope at both endpoints.
    Smoothstep = 1,
}

/// Initial local time selected for a transition destination.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AnimationTransitionStartTime {
    /// Start at the endpoint selected by the destination speed direction.
    #[default]
    Restart,
    /// Preserve the outgoing controller's local time, clamped to the destination.
    Preserve,
    /// Preserve the outgoing controller's normalized phase.
    MatchPhase,
    /// Start at an exact destination local time.
    Seek(f64),
}

/// Destination and timing policy for an interruptible controller crossfade.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationControllerTransition {
    /// Replacement controller description sampled as the incoming side.
    pub description: AnimationControllerDescription,
    /// Finite nonnegative crossfade duration in Host seconds.
    pub duration: f64,
    /// Crossfade progress curve.
    pub easing: AnimationTransitionEasing,
    /// Destination clock initialization policy.
    pub start_time: AnimationTransitionStartTime,
}

/// Compact observable state for an active controller transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationControllerTransitionState {
    /// Total crossfade duration in Host seconds.
    pub duration: f64,
    /// Host time accumulated while the transition was ready and playing.
    pub elapsed: f64,
    /// Crossfade progress curve.
    pub easing: AnimationTransitionEasing,
    /// Whether source readiness currently freezes both clocks and progress.
    pub pending: bool,
}

impl Default for AnimationControllerDescription {
    fn default() -> Self {
        Self {
            drivers: Vec::new(),
            speed: 1.0,
            looping: false,
        }
    }
}

/// Shared clock status of a controller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum AnimationPlaybackStatus {
    /// No sampled contribution; local position is retained.
    #[default]
    Stopped = 0,
    /// Advance when all source bindings are ready.
    Playing = 1,
    /// Hold local time and sampled contribution.
    Paused = 2,
    /// Hold the final sample of a nonlooping controller.
    Completed = 3,
}

/// Ordered controller operation. Only a Host advances World time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationPlaybackControl {
    /// Resume, or replay a completed controller from its directional start.
    Play,
    /// Set a finite signed speed and resume without rewinding an endpoint.
    PlayAtSpeed(f32),
    /// Hold current time and contribution.
    Pause,
    /// Withdraw contributions and retain local time.
    Stop,
    /// Sample the exact destination without further advancement in this frame.
    Seek(f64),
    /// Start again from the directional start.
    Restart,
}

/// Readable and serializable controller state; no runtime bindings or clip ownership.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationControllerSnapshot {
    /// Persistent world-local identity.
    pub id: AnimationControllerId,
    /// Driver descriptions and clock settings.
    pub description: AnimationControllerDescription,
    /// Current controller status.
    pub state: AnimationPlaybackStatus,
    /// Local time, frozen while sources are pending.
    pub time: f64,
    /// Active crossfade state, when this controller is transitioning.
    pub transition: Option<AnimationControllerTransitionState>,
}

/// Complete persistent controller state, including deleted identity high-water.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationPersistentState {
    /// Next fresh identity; never less than any existing or previously used ID.
    pub next_id: u64,
    /// Controllers in identity order.
    pub controllers: Vec<AnimationControllerSnapshot>,
    /// Semantic state for active crossfades, keyed by controller identity.
    pub transitions: Vec<AnimationPersistentTransition>,
    /// Controllers awaiting a negative-speed directional start after clip readiness.
    pub directional_starts: Vec<AnimationControllerId>,
}

impl Default for AnimationPersistentState {
    fn default() -> Self {
        Self {
            next_id: 1,
            controllers: Vec::new(),
            transitions: Vec::new(),
            directional_starts: Vec::new(),
        }
    }
}

/// Sparse durable origin captured when a crossfade is interrupted.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationFrozenTransitionValue {
    /// Entity whose property was captured.
    pub target: EntityId,
    /// Property or single joint represented by this sparse value.
    pub property: AnimationTrackTarget,
    /// Frozen outgoing contribution at the interruption boundary.
    pub value: AnimationValue,
    /// Live producer value to restore or fade toward.
    pub baseline: AnimationValue,
}

/// Durable outgoing side of an active crossfade.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationPersistentTransitionSource {
    /// Independently advancing outgoing controller.
    Live(AnimationControllerSnapshot),
    /// Sparse composite captured from an interrupted transition.
    Frozen {
        /// Captured values and their restoration baselines.
        values: Vec<AnimationFrozenTransitionValue>,
        /// Declaration metadata used only to rebuild stable output bindings.
        bindings: AnimationControllerSnapshot,
        /// Prior destination time used by deferred preserve and phase matching.
        reference_time: f64,
        /// Prior destination duration used by deferred phase matching.
        reference_duration: f64,
    },
}

/// Persistent crossfade state; the destination remains the ordinary controller snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationPersistentTransition {
    /// Controller identity shared with the destination snapshot.
    pub id: AnimationControllerId,
    /// Durable outgoing side.
    pub source: AnimationPersistentTransitionSource,
    /// Deferred destination clock policy.
    pub start_time: AnimationTransitionStartTime,
}

/// Observable transition emitted once per actual state or failure change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AnimationPlaybackEventKind {
    /// Entered playing state.
    Started = 0,
    /// Entered paused state.
    Paused = 1,
    /// Contribution withdrawn.
    Stopped = 2,
    /// Nonlooping endpoint reached.
    Completed = 3,
    /// A bound target or source lifetime ended.
    Invalidated = 4,
    /// A sampled contribution could not activate.
    Failed = 5,
}

/// Compact transition observation without cloning driver descriptions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationControllerState {
    /// Controller identity.
    pub id: AnimationControllerId,
    /// Current clock status.
    pub state: AnimationPlaybackStatus,
    /// Current local time.
    pub time: f64,
}

/// Runtime transition, separate from correlated command outcomes.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationPlaybackEvent {
    /// Controller and resulting state.
    pub controller: AnimationControllerState,
    /// Actual transition.
    pub kind: AnimationPlaybackEventKind,
    /// Failure or invalidation diagnostic.
    pub reason: Option<ErrorReason>,
}

/// Correlated, FIFO controller mutation at the World mutation boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationControllerCommand {
    /// Create a stopped controller.
    Create(AnimationControllerDescription),
    /// Atomically replace driver descriptions and settings, retaining status/time.
    Update {
        /// Existing controller identity.
        id: AnimationControllerId,
        /// Replacement settings and drivers.
        description: AnimationControllerDescription,
    },
    /// Crossfade to a replacement description with independent source and destination clocks.
    Transition {
        /// Existing controller identity.
        id: AnimationControllerId,
        /// Destination and crossfade policy.
        transition: AnimationControllerTransition,
    },
    /// Restore contributions and delete a controller.
    Delete {
        /// Existing controller identity.
        id: AnimationControllerId,
    },
    /// Control one shared clock.
    Control {
        /// Existing controller identity.
        id: AnimationControllerId,
        /// Shared-clock operation.
        control: AnimationPlaybackControl,
    },
}

/// Completion of an ordered controller command.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationControllerOutcome {
    /// Caller correlation value.
    pub request_id: u64,
    /// A fresh identity for Create; None for the other successful commands.
    pub result: Result<Option<AnimationControllerId>, ErrorReason>,
}

mod system_state;
pub use system_state::{AnimationController, AnimationSystemState};

mod system;
pub use system::{AnimationSystem, AnimationSystemFactory};

mod codec;

pub(crate) use codec::decode_legacy as decode_legacy_persistent_state;
