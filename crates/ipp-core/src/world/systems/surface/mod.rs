//! Component-owned two-dimensional presentation planes, their cache policy and shared text metrics.

mod cache_policy;
mod component;
mod system;
pub mod text;

pub use cache_policy::{
    SURFACE_CACHE_BAND_HYSTERESIS, SURFACE_CACHE_MAX_BANDS, SURFACE_CACHE_MAX_DIRECT_DISTANCE,
    SURFACE_CACHE_MAX_REFRESH_HZ, SURFACE_CACHE_MAX_TEXELS_PER_METRE, SurfaceCachePolicy,
};
pub use component::{Surface, SurfaceCache};
pub use system::{SurfaceSystem, SurfaceSystemFactory};
pub use text::{
    SEGMENTATION_SCOPE, TextCacheKey, TextCaret, TextFont, TextGlyph, TextLayout, TextLine,
    TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome, TextRequestError, TextUnits,
    UNICODE_VERSION, grapheme_boundaries, is_grapheme_boundary, measure_text, utf8_to_utf16_offset,
    utf16_to_utf8_offset,
};
