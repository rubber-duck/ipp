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
    PositionedGlyph, SEGMENTATION_SCOPE, Surface, SurfaceClipRect, SurfaceCommand, SurfaceGlyph,
    SurfaceItem, SurfaceItemContent, SurfaceItemId, SurfaceItemPatch, SurfaceItemStyle,
    SurfacePrimitiveIdentity, SurfacePrimitiveStyle, SurfaceRenderItem, SurfaceRenderPrimitive,
    SurfaceRenderResource, TextCacheKey, TextCaret, TextFont, TextGlyph, TextLayout, TextLine,
    TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome, TextRequestError, TextUnits,
    UNICODE_VERSION, grapheme_boundaries, intersect_surface_clips, is_grapheme_boundary,
    measure_text, primitive_effective_clip, surface_clip_is_empty, surface_content_clip,
    surface_primitive_visible, utf8_to_utf16_offset, utf16_to_utf8_offset,
};

#[cfg(feature = "gui")]
pub use systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, gui_logical_to_surface_content,
    surface_content_to_gui_logical,
};

#[cfg(feature = "gui")]
pub use systems::gui::{
    DEFAULT_UNITS_PER_METRE, GuiBlockerHit, GuiCommand, GuiConstraintError, GuiContainerKind,
    GuiControlState, GuiControlValue, GuiEvaluatedContent, GuiEvaluatedNode, GuiEvaluatedView,
    GuiFontResolution, GuiHit, GuiInputCancelReason, GuiInputCancellation, GuiInputCommand,
    GuiInputConflict, GuiInputConflictReason, GuiInputEffect, GuiInputEffectKind, GuiInputFocus,
    GuiInputSystem, GuiInputSystemFactory, GuiInputTarget, GuiInspectQuery, GuiInspectResponse,
    GuiInspectedNode, GuiKey, GuiLayoutCache, GuiLayoutDiagnostic, GuiLayoutRequest,
    GuiLayoutSystem, GuiLayoutSystemFactory, GuiNode, GuiNodeContent, GuiNodeHandle, GuiNodeId,
    GuiNodePatch, GuiNodeStyle, GuiNodes, GuiPanelResolution, GuiPointerButton,
    GuiResourceResolver, GuiRoot, GuiSystem, GuiSystemFactory, GuiTextCompositionState,
    GuiTextFocusState, GuiTextFocusUpdate, GuiUnhandledInput, GuiUnhandledReason,
    MAX_GUI_TEXT_BYTES, MAX_LAYOUT_DEPTH, resolve_panel_hit,
};

#[cfg(feature = "gui")]
pub use systems::gui::semantics::{
    GuiSemanticAction, GuiSemanticActionCommand, GuiSemanticActionError, GuiSemanticActionKind,
    GuiSemanticActionRequest, GuiSemanticFocus, GuiSemanticNode, GuiSemanticRole,
    GuiSemanticSnapshotQuery, GuiSemanticTree, action_command,
};

pub use systems::camera::{CameraMotion, CameraStateChange, CameraStatePatch, PreparedCamera};

pub use systems::geometry::queries::{
    CameraProjectOutcome, CameraProjectQuery, GeometryPickHit, GeometryPickOutcome,
    GeometryPickQuery, WorldPlane,
};

pub use services::asset_management::mesh::{MESH_TYPE, MeshAsset, MeshKey, MeshStats, MeshUpload};

pub use world::{DebugRenderItem, RenderDiagnostic, RenderItem};

pub use services::asset_management::texture::{
    TEXTURE_TYPE, TextureAsset, TextureDecoder, TextureHeader, TextureKey, TextureStats,
    TextureUpload,
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

/// Production allocation reductions.
#[doc(hidden)]
#[inline]
pub fn allocation_optimizations_enabled() -> bool {
    true
}

/// Reusable joint, property and response storage.
#[doc(hidden)]
#[inline]
pub fn allocation_followup_enabled() -> bool {
    true
}

/// Indexed animation mutation callbacks.
#[doc(hidden)]
#[inline]
pub fn stress_optimizations_enabled() -> bool {
    true
}

/// Retained lighting preparation.
#[doc(hidden)]
#[inline]
pub fn lighting_reuse_enabled() -> bool {
    true
}

/// Retained mesh-demand and particle evaluation scratch.
#[doc(hidden)]
#[inline]
pub fn evaluation_scratch_reuse_enabled() -> bool {
    true
}

/// Retained draw preparation buffers.
#[doc(hidden)]
#[inline]
pub fn render_buffer_reuse_enabled() -> bool {
    true
}

/// Direct lookup of bound drivers affected by a component mutation.
#[doc(hidden)]
#[inline]
pub fn animation_binding_index_enabled() -> bool {
    true
}

/// Invalidate skin palettes once between skinning evaluations.
#[doc(hidden)]
#[inline]
pub fn skinning_invalidation_reuse_enabled() -> bool {
    true
}

/// Reuse validated animation metadata and coherent key intervals.
#[doc(hidden)]
#[inline]
pub fn animation_update_reuse_enabled() -> bool {
    true
}

/// Bind typed animation curves once, with lifecycle-driven preparation.
#[doc(hidden)]
#[inline]
pub fn compiled_animation_enabled() -> bool {
    true
}

/// Driver tracks borrow shared immutable key storage.
#[doc(hidden)]
#[inline]
pub fn animation_track_copy_enabled() -> bool {
    false
}

/// Driver-local current-segment working set.
#[doc(hidden)]
#[inline]
pub fn animation_segment_copy_enabled() -> bool {
    true
}

#[inline]
pub(crate) fn direct_numeric_updates_enabled() -> bool {
    true
}

#[inline]
pub(crate) fn compiled_hierarchy_enabled() -> bool {
    true
}

#[inline]
pub(crate) fn compiled_property_bindings_enabled() -> bool {
    true
}
