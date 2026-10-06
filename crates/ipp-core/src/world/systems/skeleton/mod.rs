//! Component-owned local poses and skeleton-space propagation from shared assets.

mod component;
pub(crate) use component::SkeletonPoseState;
pub use component::{JointOverrideRow, Skeleton, SkeletonRuntimeState};

use crate::{ComponentValue, world::WorldMutationState};

use crate::{
    EntityId, ErrorReason, PoseAsset, SkeletonAsset, WorldContext, WorldId,
    components::Transform,
    math,
    services::asset_management::{AssetKey, AssetManagementService},
    world::{WorldEntityState, WorldSimulationState},
};

mod system_state;
pub use system_state::SkeletonSystemState;

mod system;
pub use system::{SkeletonSystem, SkeletonSystemFactory};

mod update;
pub(in crate::world) use update::{pose, rebase_sampled_inputs, validate_changes};
