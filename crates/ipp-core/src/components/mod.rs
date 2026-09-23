//! Component catalogue: umbrella-owned primitives, schema, lifecycle, registry,
//! storage and dynamic properties, plus re-exports of subsystem-owned definitions.
//! Effective instances live in the World's stable typed storage.

pub mod dynamic_properties;
pub mod lifecycle;
pub mod primitives;
pub mod registry;
pub mod schema;
pub(crate) mod storage;

pub use dynamic_properties::{
    DynamicProperties, DynamicPropertyDescriptor, DynamicPropertyKind, DynamicValue,
};
pub use primitives::{Scalar, Transform};
pub use registry::ComponentValue;

pub use crate::systems::camera::Camera;
pub use crate::systems::constraints::LinearDriver;
pub use crate::systems::geometry::{BoundingGeometry, PickingGeometry};
pub use crate::systems::hierarchy::{Hierarchy, HierarchyRuntimeState};
pub use crate::systems::look_at::{LookAt, LookAtRuntimeState};
pub use crate::systems::render::{
    BaseColorTexture, CustomMaterial, Light, MeshInstance, PbrMaterial, UnlitMaterial, UnlitTexture,
};

#[cfg(feature = "surfaces")]
pub use crate::systems::surface::{Surface, SurfaceCache};

#[cfg(feature = "gui")]
pub use crate::systems::gui::GuiRoot;

#[cfg(feature = "skeletal-animation")]
pub use crate::systems::skeleton::{Skeleton, SkeletonRuntimeState};

#[cfg(feature = "skeletal-animation")]
pub use crate::systems::skinning::{Skin, SkinRuntimeState};

#[cfg(feature = "mesh-poses")]
pub use crate::systems::render::MeshPose;

#[cfg(test)]
mod preparation_fixture;

#[cfg(test)]
pub use preparation_fixture::{BufferCounters, PreparedBuffer};

#[cfg(feature = "particles")]
pub use crate::systems::particles::{
    ParticleEmitter, ParticleMesh, ParticlePlayback, ParticleSprite,
};
