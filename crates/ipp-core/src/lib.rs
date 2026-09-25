//! Headless ordered world mutation, stable storage and fixed-order evaluation.
//! Hosts own transports and supply simulation time. Optional capabilities beyond
//! the selected scalar, declaration and unlit scene capabilities remain omitted.

#[cfg(feature = "diagnostics")]
pub mod diagnostics;

/// Filter before constructing arguments; lean builds erase every input token.
#[cfg(feature = "diagnostics")]
#[macro_export]
macro_rules! diagnostic {
    ($level:ident, $($args:tt)*) => {{
        if $crate::diagnostics::enabled($crate::diagnostics::Level::$level) {
            $crate::diagnostics::emit($crate::diagnostics::Level::$level, format_args!($($args)*));
        }
    }};
}

/// Erase diagnostics, including argument evaluation and format strings.
#[cfg(not(feature = "diagnostics"))]
#[macro_export]
macro_rules! diagnostic {
    ($($args:tt)*) => {{}};
}

extern crate self as ipp_core;

pub mod commands;

pub mod components;
pub use components::{
    ComponentValue, DynamicProperties, DynamicPropertyDescriptor, DynamicPropertyKind, DynamicValue,
};

pub mod identity;

pub mod services;

mod world;

pub use world::systems;

pub use commands::*;
pub use identity::EntityId;
pub use world::{
    EntityPersistentId, EntitySnapshot, World, WorldCapacityHints, WorldCapacityHintsPatch,
    WorldConstructionError, WorldContext, WorldCreateOptions, WorldDescriptor, WorldLimits,
    WorldMetadata, WorldPersistentId, WorldSelector, WorldSystemCapacityHints, WorldUpdateReport,
};

pub use systems::state_overlay::{
    ComponentOverlayMode, EntityOverlayMode, MAX_STATE_OVERLAY_DIAGNOSTICS, StateOverlayAlias,
    StateOverlayHandleKind, StateOverlayLifecycleDiagnostic, StateOverlayLifecycleReason,
    StateOverlayRef,
};

pub use systems::render::render_state::{RenderState, RenderStateChange, RenderStatePatch};

#[cfg(feature = "surfaces")]
pub use systems::surface::{
    PositionedGlyph, SEGMENTATION_SCOPE, Surface, SurfaceCache, SurfaceCachePolicy,
    SurfaceClipRect, SurfaceCommand, SurfaceGlyph, SurfaceItem, SurfaceItemContent, SurfaceItemId,
    SurfaceItemPatch, SurfaceItemStyle, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
    SurfaceRenderItem, SurfaceRenderPrimitive, SurfaceRenderResource, TextCacheKey, TextCaret,
    TextFont, TextGlyph, TextLayout, TextLine, TextLinePolicy, TextMaxWidth, TextMeasureRequest,
    TextOutcome, TextRequestError, TextUnits, UNICODE_VERSION, grapheme_boundaries,
    intersect_surface_clips, is_grapheme_boundary, measure_text, primitive_effective_clip,
    surface_clip_is_empty, surface_content_clip, surface_primitive_visible, utf8_to_utf16_offset,
    utf16_to_utf8_offset,
};

#[cfg(feature = "gui")]
pub use systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, GuiShapeFill, GuiShapeGlow, gui_logical_to_surface_content,
    surface_content_to_gui_logical,
};

#[cfg(feature = "gui")]
pub use systems::gui::{
    DEFAULT_UNITS_PER_METRE, GuiBlockerHit, GuiCommand, GuiCommitSource, GuiContainerKind,
    GuiControlState, GuiControlValue, GuiEvaluatedContent, GuiEvaluatedNode, GuiEvaluatedView,
    GuiFontResolution, GuiHit, GuiInputCancelReason, GuiInputCancellation, GuiInputCommand,
    GuiInputConflict, GuiInputConflictReason, GuiInputEffect, GuiInputEffectKind, GuiInputFocus,
    GuiInputSystem, GuiInputSystemFactory, GuiInputTarget, GuiInspectQuery, GuiInspectResponse,
    GuiInspectedNode, GuiKey, GuiLayoutCache, GuiLayoutRequest, GuiLayoutSystem,
    GuiLayoutSystemFactory, GuiNode, GuiNodeData, GuiNodeDataProperty, GuiNodeDataRow,
    GuiNodeHandle, GuiNodeId, GuiNodePatch, GuiNodePropertyRef, GuiNodeRowProperty, GuiNodeStyle,
    GuiNodeStyleProperty, GuiNodeStyleRow, GuiNodes, GuiPanelResolution, GuiPointerButton,
    GuiResourceResolver, GuiRoot, GuiSystem, GuiSystemFactory, GuiTextCompositionState,
    GuiTextFence, GuiTextFocusState, GuiTextFocusUpdate, GuiUnhandledInput, GuiUnhandledReason,
    MAX_GUI_NODE_ID, MAX_GUI_TEXT_BYTES, MAX_LAYOUT_DEPTH, resolve_panel_hit,
};

#[cfg(feature = "gui")]
pub use systems::gui::semantics::{
    GuiSemanticAction, GuiSemanticActionCommand, GuiSemanticActionError, GuiSemanticActionKind,
    GuiSemanticActionRequest, GuiSemanticFocus, GuiSemanticNode, GuiSemanticRole,
    GuiSemanticScroll, GuiSemanticSnapshotQuery, GuiSemanticTree, action_command,
};

pub use systems::camera::{CameraMotion, CameraStateChange, CameraStatePatch, PreparedCamera};

pub use systems::geometry::queries::{
    CameraProjectOutcome, CameraProjectQuery, GeometryPickHit, GeometryPickOutcome,
    GeometryPickQuery, WorldPlane,
};

pub use services::asset_management::mesh::{MESH_TYPE, MeshAsset, MeshKey, MeshUpload};

pub use world::{DebugRenderItem, RenderDiagnostic, RenderItem};

pub use services::asset_management::texture::{
    TEXTURE_TYPE, TextureAsset, TextureDecoder, TextureHeader, TextureKey, TextureUpload,
};

pub use services::asset_management::service::{
    AssetAcquisitionRequest, AssetResourceKind, AssetResourceSnapshot, AssetResourceStatus,
};

#[cfg(feature = "skeletal-animation")]
pub use services::asset_management::skeleton::{
    MAX_JOINTS, POSE_TYPE, PoseAsset, SKELETON_TYPE, SkeletonAsset,
};

#[cfg(feature = "skeletal-animation")]
pub use services::asset_management::skin_binding::{SKIN_TYPE, SkinAsset};

mod host;
pub use host::{HostRuntime, WorldId};

/// Opt-in stage timing and allocation counters; never enabled by default.
#[cfg(feature = "profiling")]
pub mod profiling;
