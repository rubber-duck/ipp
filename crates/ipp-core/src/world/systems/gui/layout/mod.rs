//! GUI constraint evaluation, retained layout pass and skin paint.
//!
//! Evaluation turns [`GuiRoot`](super::tree::GuiRoot) trees into retained
//! [`GuiEvaluatedView`] geometry; the layout system retains one view per
//! root entity; skinning resolves theme and part-row appearance over those
//! views.

pub(super) mod evaluation;
pub(crate) mod scroll_bars;
pub(super) mod skin;
pub(super) mod system;

#[cfg(test)]
pub(crate) use evaluation::GuiLayoutDiagnostic;
pub use evaluation::{
    DEFAULT_UNITS_PER_METRE, GuiBlockerHit, GuiEvaluatedContent, GuiEvaluatedNode,
    GuiEvaluatedView, GuiFontResolution, GuiHit, GuiLayoutCache, GuiLayoutRequest,
    GuiPanelResolution, GuiResourceResolver, MAX_LAYOUT_DEPTH, resolve_panel_hit,
};
pub use skin::{
    FOCUS_BORDER_COLOR, FOCUS_BORDER_WIDTH, GuiControlVariant, GuiInteractionState, GuiPartMotion,
    GuiPartStyle, GuiScrollBarCursor, GuiSkinCursors, GuiSkinState, GuiSkinnedAppearance,
    MAX_SKIN_DEPTH, MAX_SKIN_NODES, apply_appearance_to_primitive, apply_asset_to_primitive,
    part_overrides, resolve_appearance, resolve_state_part_motion, resolve_state_part_style,
    skinned_primitives_for_view, theme_part_style, variant_for_content,
};
pub(crate) use skin::{
    appearance_with_effective_numeric, resolve_paint_appearance, skin_channels_complete,
    skinned_parts_for_view, skinned_primitives_for_view_with_overrides,
};
pub use system::{GuiLayoutSystem, GuiLayoutSystemFactory};
