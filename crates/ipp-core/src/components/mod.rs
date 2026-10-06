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

pub use dynamic_properties::{
    DynamicProperties, DynamicPropertyDescriptor, DynamicPropertyKind, DynamicValue,
};
pub use primitives::{Scalar, Transform};
pub use registry::ComponentValue;

#[cfg(test)]
pub use rows::fixture::{RowsFixture, RowsFixtureItem, RowsFixtureTag};

// System-owned definitions, one block per owning System in registry order.

pub use crate::systems::constraints::{ExpressionDriver, LinearDriver};

pub use crate::systems::render::{
    BaseColorTexture, CustomMaterial, Light, MeshInstance, MeshPose, PbrMaterial, UnlitMaterial,
    UnlitTexture,
};

pub use crate::systems::camera::Camera;

pub use crate::systems::skeleton::{JointOverrideRow, Skeleton};

pub use crate::systems::skinning::Skin;

pub use crate::systems::geometry::{BoundingGeometry, PickingGeometry};

pub use crate::systems::hierarchy::ParentJoint;

pub use crate::systems::look_at::LookAt;

pub use crate::systems::particles::{
    ParticleEmitter, ParticleMesh, ParticlePlayback, ParticleSprite,
};

pub use crate::systems::surface::{CylinderSurface, FlatSurface, SphereSurface, SurfaceCache};

pub use crate::systems::world_attachment::WorldAttachment;

pub use crate::systems::canvas::{
    CanvasBitmap, CanvasBounds, CanvasBox, CanvasDrawing, CanvasGlyphRun, CanvasLayerTransition,
    CanvasPaint, CanvasStyle, CanvasText,
};

pub use crate::systems::gui::layout::{GuiLayout, GuiOverlay};
pub use crate::systems::gui::local::{
    GuiBehavior, GuiButton, GuiCheckbox, GuiColor, GuiGroup, GuiScrollView, GuiSlider,
    GuiTextInput, GuiVirtualItem, GuiVirtualList,
};
pub use crate::systems::gui::motion::GuiThemeMotion;
pub use crate::systems::gui::presentation::{GuiFont, GuiSkin, GuiTheme};

pub use crate::systems::data_bindings::{BufferDataSourceBinding, StreamingDataSourceBinding};

pub use crate::systems::plot::{
    PlotBars2d, PlotFrame2d, PlotFrame3d, PlotGridBars3d, PlotHeightSurface3d, PlotLine2d,
    PlotPie2d, PlotPie3d, PlotPoints3d,
};
