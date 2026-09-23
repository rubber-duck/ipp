//! World render projection and settings.

mod custom_material;
pub use custom_material::CustomMaterial;

mod components;
pub use components::{BaseColorTexture, MeshInstance, UnlitMaterial, UnlitTexture};

mod lighting;
pub use lighting::{Light, PbrMaterial};

#[cfg(feature = "mesh-poses")]
mod mesh_pose;

#[cfg(feature = "mesh-poses")]
pub use mesh_pose::MeshPose;

#[cfg(feature = "mesh-poses")]
pub(in crate::world) use mesh_pose::mesh_pose;

mod settings;

pub mod render_state;

use crate::services::asset_management::AssetManagementService;
use crate::world::WorldSimulationState;
use crate::world::systems::asset_dependencies::{cached_source_key, resolve_asset_key};
use crate::{EntityId, ErrorReason, MeshAsset, MeshKey, MeshUpload, components::Transform};

use crate::{TextureAsset, TextureKey, TextureUpload};

use crate::services::asset_management::service::AssetResourceKind;

/// One complete renderable, copied from final effective storage in entity order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderItem {
    /// Per-particle presentation inputs; absent for ordinary geometry.
    #[cfg(feature = "particles")]
    pub particle: Option<crate::systems::particles::ParticleRenderData>,
    /// Last-resort material ignores authored vertex colors and texture inputs.
    pub solid_fallback: bool,
    /// A custom material declaration participates in this prepared input.
    pub custom_material: bool,
    /// Normal and texture-weight availability, compiled from selected mesh endpoints.
    pub normals: bool,
    /// The base mesh supplies texture blending weights.
    pub texture_weights: bool,
    /// This input has a valid evaluated skin palette; rigid draws need no palette lookup.
    #[cfg(feature = "skeletal-animation")]
    pub skinned: bool,
    /// Live entity identity.
    pub entity: EntityId,
    /// Effective authored local TRS (before terminal LookAt).
    pub transform: Transform,
    /// Complete final object-to-World matrix used by every render pass.
    pub model: [f32; 16],
    /// Rescaled inverse transpose of the final model, shared by render passes.
    pub normal: Result<[f32; 16], ErrorReason>,
    /// Final effective linear RGB factor.
    pub material: UnlitMaterial,
    /// Optional direct-light material; takes precedence over the unlit factor.
    pub pbr: Option<PbrMaterial>,
    /// Final effective immutable mesh selection.
    pub mesh: MeshKey,
    /// Optional corresponding target mesh and final blend weight.
    #[cfg(feature = "mesh-poses")]
    pub pose: Option<(MeshKey, f32)>,
    /// Optional exact immutable texture selection.
    pub texture: Option<crate::TextureKey>,
}

/// A diagnostic declaration whose private asset belongs to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugRenderItem {
    /// Live entity identity.
    pub entity: EntityId,
    /// Evaluated shape placement, including local and skeletal transforms.
    pub model: [f32; 16],
    /// Private mesh parameters for this evaluated primitive.
    pub geometry: crate::systems::geometry::GeometryPrimitiveVisual,
    /// Resolved uniform linear RGB color.
    pub color: [f32; 3],
}

/// A data-dependent incompatibility for a single renderable use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderDiagnostic {
    /// Entity whose required payload cannot be used together.
    pub entity: EntityId,
    /// Compatibility failure, independent of shared resource readiness.
    pub reason: ErrorReason,
}

mod system_state;
pub use system_state::RenderSystemState;

mod system;
pub use system::{RenderSystem, RenderSystemFactory};

#[cfg(feature = "gui")]
mod gui_presentation;

#[cfg(feature = "surfaces")]
mod surface_cache_inputs;

pub(in crate::world) struct RenderReadAccess<'a> {
    world: &'a WorldSimulationState,
    assets: &'a AssetManagementService,
    render: &'a RenderSystemState,
}

mod preparation;
mod read;
