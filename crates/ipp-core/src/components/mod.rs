//! Component catalogue: umbrella-owned primitives, schema, lifecycle, registry,
//! storage and dynamic properties, plus re-exports of subsystem-owned definitions.
//! Effective instances live in the World's stable typed storage.

pub mod dynamic_properties;
pub mod lifecycle;
pub mod primitives;
pub mod registry;
pub mod rows;
pub mod schema;
pub(crate) mod storage;

pub use crate::WorldAttachment;
pub use dynamic_properties::{
    DynamicProperties, DynamicPropertyDescriptor, DynamicPropertyKind, DynamicValue,
};
pub use primitives::{Scalar, Transform};
pub use registry::ComponentValue;

pub use crate::systems::camera::Camera;
pub use crate::systems::constraints::LinearDriver;
pub use crate::systems::geometry::{BoundingGeometry, PickingGeometry};
#[cfg(feature = "skeletal-animation")]
pub use crate::systems::hierarchy::ParentJoint;
pub use crate::systems::look_at::{LookAt, LookAtRuntimeState};
pub use crate::systems::render::{
    BaseColorTexture, CustomMaterial, Light, MeshInstance, PbrMaterial, UnlitMaterial, UnlitTexture,
};

#[cfg(feature = "surfaces")]
pub use crate::systems::surface::{Surface, SurfaceCache};

#[cfg(feature = "surfaces")]
pub use crate::systems::canvas::{
    CanvasBitmap, CanvasBox, CanvasDrawing, CanvasGlyphRun, CanvasStyle, CanvasText,
};

#[cfg(feature = "gui")]
pub use crate::systems::canvas::CanvasBounds;
#[cfg(feature = "gui")]
pub use crate::systems::gui::layout::GuiLayout;
#[cfg(feature = "gui")]
pub use crate::systems::gui::motion::GuiThemeMotion;
#[cfg(feature = "gui")]
pub use crate::systems::gui::presentation::{GuiFont, GuiSkin, GuiTheme};

#[cfg(feature = "gui")]
pub use crate::systems::gui::local::{
    GuiBehavior, GuiButton, GuiCheckbox, GuiScrollView, GuiSlider, GuiTextInput, GuiVirtualItem,
    GuiVirtualList,
};

#[cfg(feature = "skeletal-animation")]
pub use crate::systems::skeleton::{JointOverrideRow, Skeleton, SkeletonRuntimeState};

#[cfg(feature = "skeletal-animation")]
pub use crate::systems::skinning::{Skin, SkinRuntimeState};

#[cfg(feature = "mesh-poses")]
pub use crate::systems::render::MeshPose;

#[cfg(test)]
mod rows_fixture;

#[cfg(test)]
pub use rows_fixture::{RowsFixture, RowsFixtureItem, RowsFixtureTag};

#[cfg(feature = "particles")]
pub use crate::systems::particles::{
    ParticleEmitter, ParticleMesh, ParticlePlayback, ParticleSprite,
};
