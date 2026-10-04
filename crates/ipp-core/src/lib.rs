//! Headless ordered world mutation, stable storage and fixed-order evaluation.
//! Hosts own transports and supply simulation time. Optional capabilities beyond
//! the selected scalar, declaration and unlit scene capabilities remain omitted.

pub mod diagnostics;

/// Filter before constructing arguments; a disabled level evaluates no argument.
#[macro_export]
macro_rules! diagnostic {
    ($level:ident, $($args:tt)*) => {{
        if $crate::diagnostics::enabled($crate::diagnostics::Level::$level) {
            $crate::diagnostics::emit($crate::diagnostics::Level::$level, format_args!($($args)*));
        }
    }};
}

extern crate self as ipp_core;

pub mod commands;

mod batch_symbol_reports;
pub use batch_symbol_reports::BatchSymbolReports;

pub mod components;
pub use components::{
    ComponentValue, DynamicProperties, DynamicPropertyDescriptor, DynamicPropertyKind, DynamicValue,
};

pub mod identity;

pub mod expressions;

pub mod services;

mod world;

pub use world::systems;

pub use commands::*;
pub use identity::EntityId;
pub use world::{
    EntityLink, EntityOrder, EntityPersistentId, EntityPlacement, EntityPlacementRef,
    EntitySnapshot, RECYCLED_COMMAND_BUFFER_COMMANDS, World, WorldCapacityHints,
    WorldCapacityHintsPatch, WorldConstructionError, WorldContext, WorldCreateOptions,
    WorldDescriptor, WorldLimits, WorldMetadata, WorldPersistentId, WorldSelector,
    WorldSystemCapacityHints, WorldUpdateReport,
};

pub use systems::canvas::canvas_state::{
    CanvasEvaluatedExtent, CanvasState, CanvasStateRecord, CanvasStateUpdate,
};
pub use systems::render::render_state::{RenderState, RenderStateChange, RenderStatePatch};

pub use systems::surface::{
    SEGMENTATION_SCOPE, Surface, SurfaceCache, SurfaceCachePolicy, TextCacheKey, TextCaret,
    TextFont, TextGlyph, TextLayout, TextLine, TextLinePolicy, TextMaxWidth, TextMeasureRequest,
    TextOutcome, TextRequestError, TextUnits, UNICODE_VERSION, grapheme_boundaries,
    is_grapheme_boundary, measure_text, utf8_to_utf16_offset, utf16_to_utf8_offset,
};

pub use systems::gui::{
    GuiLayoutSystem, GuiLayoutSystemFactory, GuiPrimitivePart, GuiSystem, GuiSystemFactory,
    MAX_GUI_TEXT_BYTES, MAX_LAYOUT_DEPTH,
};

pub use systems::camera::{CameraMotion, CameraStateChange, CameraStatePatch, PreparedCamera};

pub use systems::geometry::queries::{
    CameraProjectOutcome, CameraProjectQuery, GeometryPickHit, GeometryPickOutcome,
    GeometryPickQuery, WorldPlane,
};

pub use services::asset_management::mesh::{
    MAX_MESH_VERTICES, MESH_TYPE, MeshAsset, MeshKey, MeshUpload,
};

pub use world::{DebugRenderItem, RenderDiagnostic, RenderItem};

pub use services::asset_management::texture::{
    TEXTURE_TYPE, TextureAsset, TextureDecoder, TextureHeader, TextureKey, TextureUpload,
};

pub use services::asset_management::service::{
    AssetAcquisitionRequest, AssetResourceKind, AssetResourceSnapshot, AssetResourceStatus,
};

pub use services::asset_management::skeleton::{
    MAX_JOINTS, POSE_TYPE, PoseAsset, SKELETON_TYPE, SkeletonAsset,
};

pub use services::asset_management::skin_binding::{SKIN_TYPE, SkinAsset};

mod host;
pub use host::{
    AttachmentPlacement, HostRuntime, OutputKind, OutputRef, OutputTarget, WorldAttachment,
    WorldAttachmentMode, WorldFrameContext, WorldId, WorldRef, WorldViewport,
};
pub use host::{
    HostFrameReport, OutputPublicationObservation, PublishedWorldAttachment, WorldDerivedChunk,
    WorldOutputBuilder, WorldPublication, WorldPublicationId,
};
pub use host::{HostIngressView, RootBindingGeneration, RootOutputBinding};
pub use host::{OutputReferenceToken, WorldReferenceToken};
pub use host::{PublishedSceneContribution, PublishedSceneHit};
pub use host::{ViewDescriptor, ViewPickHit, ViewQueryTarget};
pub use host::{WorldAttachmentEffect, WorldAttachmentRetirement, WorldAttachmentToken};

/// Opt-in stage timing and allocation counters; never enabled by default.
#[cfg(feature = "instrumentation")]
pub mod profiling;
#[cfg(feature = "instrumentation")]
pub mod profiling_trace;

#[cfg(test)]
#[allow(dead_code)]
mod test_task_scheduler {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/task_scheduler.rs"
    ));
}
