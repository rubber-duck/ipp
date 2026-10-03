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
pub use crate::systems::constraints::{ExpressionDriver, LinearDriver};
pub use crate::systems::geometry::{BoundingGeometry, PickingGeometry};
pub use crate::systems::hierarchy::ParentJoint;
pub use crate::systems::look_at::{LookAt, LookAtRuntimeState};
pub use crate::systems::render::{
    BaseColorTexture, CustomMaterial, Light, MeshInstance, PbrMaterial, UnlitMaterial, UnlitTexture,
};

pub use crate::systems::surface::{Surface, SurfaceCache};

pub use crate::systems::canvas::{
    CanvasBitmap, CanvasBox, CanvasDrawing, CanvasGlyphRun, CanvasPaint, CanvasStyle, CanvasText,
};

pub use crate::systems::canvas::CanvasBounds;
pub use crate::systems::gui::layout::{GuiLayout, GuiOverlay};
pub use crate::systems::gui::motion::GuiThemeMotion;
pub use crate::systems::gui::presentation::{GuiFont, GuiSkin, GuiTheme};

pub use crate::systems::gui::local::{
    GuiBehavior, GuiButton, GuiCheckbox, GuiColor, GuiGroup, GuiScrollView, GuiSlider,
    GuiTextInput, GuiVirtualItem, GuiVirtualList,
};

pub use crate::systems::skeleton::{JointOverrideRow, Skeleton, SkeletonRuntimeState};

pub use crate::systems::skinning::{Skin, SkinRuntimeState};

pub use crate::systems::render::MeshPose;

#[cfg(test)]
mod rows_fixture;

#[cfg(test)]
pub use rows_fixture::{RowsFixture, RowsFixtureItem, RowsFixtureTag};

pub use crate::systems::particles::{
    ParticleEmitter, ParticleMesh, ParticlePlayback, ParticleSprite,
};

pub use crate::systems::data_bindings::{BufferDataSourceBinding, StreamingDataSourceBinding};

pub use crate::systems::plot::{
    PlotBars2d, PlotFrame2d, PlotFrame3d, PlotGridBars3d, PlotHeightSurface3d, PlotLine2d,
    PlotPie2d, PlotPie3d, PlotPoints3d,
};
