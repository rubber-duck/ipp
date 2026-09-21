//! GUI constraint evaluation, retained layout pass and skin paint.
//!
//! Evaluation turns [`GuiRoot`](super::tree::GuiRoot) trees into retained
//! [`GuiEvaluatedView`] geometry; the layout system retains one view per
//! root entity; skinning resolves named-part appearance over those views.

pub(super) mod evaluation;
pub(super) mod skin;
pub(super) mod system;

pub use evaluation::{
    DEFAULT_UNITS_PER_METRE, GuiBlockerHit, GuiConstraintError, GuiEvaluatedContent,
    GuiEvaluatedNode, GuiEvaluatedView, GuiFontResolution, GuiHit, GuiLayoutCache,
    GuiLayoutDiagnostic, GuiLayoutRequest, GuiPanelResolution, GuiResourceResolver,
    MAX_LAYOUT_DEPTH, resolve_panel_hit,
};
pub use skin::{
    FOCUS_BORDER_COLOR, FOCUS_BORDER_WIDTH, GuiControlVariant, GuiInteractionState, GuiPartMotion,
    GuiPartStyle, GuiSkinCursors, GuiSkinState, GuiSkinnedAppearance, MAX_SKIN_DEPTH,
    MAX_SKIN_NODES, apply_appearance_to_primitive, apply_asset_to_primitive, focus_part_name,
    is_valid_skin_part, part_style, resolve_appearance, resolve_state_part_motion,
    resolve_state_part_style, skin_part_key, skinned_primitives_for_view, state_part_candidates,
    variant_for_content,
};
pub(crate) use skin::{
    appearance_with_effective_numeric, resolve_paint_appearance, skinned_parts_for_view,
    skinned_primitives_for_view_with_overrides,
};
pub use system::{GuiLayoutSystem, GuiLayoutSystemFactory};
