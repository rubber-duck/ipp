//! Component-owned ordered two-dimensional presentation content.

mod cache_policy;
mod component;
mod items;
pub(in crate::world) mod rendering;
mod system;
pub mod text;

pub use cache_policy::{
    SURFACE_CACHE_BAND_HYSTERESIS, SURFACE_CACHE_MAX_BANDS, SURFACE_CACHE_MAX_DIRECT_DISTANCE,
    SURFACE_CACHE_MAX_REFRESH_HZ, SURFACE_CACHE_MAX_TEXELS_PER_METRE, SurfaceCachePolicy,
};
pub(crate) use component::validate_property_value as validate_animation_property;
pub use component::{Surface, SurfaceCache};
pub use items::{
    PositionedGlyph, SurfaceItem, SurfaceItemContent, SurfaceItemId, SurfaceItemPatch,
    SurfaceItemStyle,
};
#[cfg(feature = "gui")]
pub use rendering::{
    GuiPrimitiveId, GuiPrimitivePart, GuiShapeFill, GuiShapeGlow, gui_logical_to_surface_content,
    surface_content_to_gui_logical,
};
pub use rendering::{
    SurfaceClipRect, SurfaceGlyph, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
    SurfaceRenderItem, SurfaceRenderPrimitive, SurfaceRenderResource, intersect_surface_clips,
    primitive_effective_clip, surface_clip_is_empty, surface_content_clip,
    surface_primitive_visible, surface_primitive_with_skin_style,
};
pub use system::{SurfaceCommand, SurfaceSystem, SurfaceSystemFactory};
pub use text::{
    SEGMENTATION_SCOPE, TextCacheKey, TextCaret, TextFont, TextGlyph, TextLayout, TextLine,
    TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome, TextRequestError, TextUnits,
    UNICODE_VERSION, grapheme_boundaries, is_grapheme_boundary, measure_text, utf8_to_utf16_offset,
    utf16_to_utf8_offset,
};
