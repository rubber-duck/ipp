//! Authoritative GUI tree, stable node handles and committed control values on Surfaces.
//!
//! `GuiRoot.nodes` stores structure plus one committed value and revision per
//! control node. While a root incarnation is live it changes only through
//! [`GuiCommand`]; a new incarnation (insertion or restore) may supply it whole.
//! Node style and named skin parts are ordinary dynamic properties named
//! `node_<id>_<lane>` and `node_<id>_part_<part>_<lane>`, removed with their node.
//! Operation guards reject raw Surface items on GUI-owned Surfaces, GUI roots on
//! populated Surfaces and overlays that would supply either structure.
//!
//! ## Controls and actions
//!
//! Button, Checkbox (toggle), Slider and TextInput nodes keep ordinary
//! authored state plus effective values. Authored content carries the initial
//! value only; the committed revision always wins and authored edits never
//! replay over it. Retained [`GuiEvaluatedView`] output measures and reports
//! the effective value with its revision. Text edits reflow because they can
//! change measurement; checkbox/slider commits and enabled changes refresh
//! retained payload/paint state without reflow.
//!
//! Actions originate only from [`GuiInputSystem`] routing followed by
//! liveness revalidation at the next mutation boundary. Committed outcomes
//! report as revision-keyed [`GuiInputEffect`] records (`ButtonPressed`,
//! `ControlCommitted`) with source and effect ticks; removals, hiding and
//! session replacement cancel as [`GuiInputCancellation`], arbitration and
//! admission losses conflict as [`GuiInputConflict`]. The authored
//! `SetControlValue` path stays the explicit revision-aware external reset,
//! never an input action.
//!
//! Boundaries: hidden, unavailable or disabled targets are ineligible for
//! routing and application; retained interactions are synchronously cancelled
//! when those states or their full identities change. The authored `enabled`
//! style lane defaults true and gates hit testing, activation and skin state.
//! Sessions fence every handle, focus, capture
//! and queued envelope; replacement cancels in-flight work. Clipboard,
//! IME composition and soft keyboards belong to platform adapters: this
//! system exposes no clipboard verbs and accepts text only through routed
//! `Text` edits. Touch arbitration reuses routing state (one press per
//! control, capture retention, click-cancel on miss); scroll offsets stay
//! input-owned and never reflow layout. Frames with no ingress still validate
//! retained focus/capture/hover/caret targets proportionally to active cursors;
//! when no cursor changes, publication and retained views stay untouched.
//!
//! Caps: `MAX_NODES` nodes per tree, [`MAX_LAYOUT_DEPTH`] evaluation depth,
//! 1024 pending input envelopes, [`MAX_GUI_TEXT_BYTES`] text bytes, and bounded
//! 32-deep / 256-node inspection.
//! No extra passes or systems exist for controls; animation reaches nodes
//! only through sparse state overlays sampled by `AnimationSystem`, and hit
//! testing uses retained views, never widget entities.
//!
//! Downstream interface (skinning, semantic readers): effective control
//! payloads on [`GuiEvaluatedContent`],
//! revision-keyed input effects, bounded `inspect_gui` snapshots with
//! committed values and revisions, and the focus/scroll cursors on
//! `WorldContext`.
//!
//! [`semantics`] builds machine-client snapshots and translates actions over
//! these read interfaces. `RenderSystem` keeps
//! GUI presentation bookkeeping in `render::gui_presentation` at its
//! existing schedule points. Components are re-exported through
//! [`crate::components`]; the [GUI architecture](../../../../../../docs/architecture/gui.md)
//! owns the design.

pub mod input;
pub mod layout;
pub mod semantics;
mod system;
mod system_state;
#[cfg(test)]
mod test_support;
pub mod tree;

pub use input::{
    GuiInputCancelReason, GuiInputCancellation, GuiInputCommand, GuiInputConflict,
    GuiInputConflictReason, GuiInputEffect, GuiInputEffectKind, GuiInputFocus, GuiInputSystem,
    GuiInputSystemFactory, GuiInputTarget, GuiKey, GuiPointerButton, GuiTextCompositionState,
    GuiTextFocusState, GuiTextFocusUpdate, GuiUnhandledInput, GuiUnhandledReason,
};
pub use layout::{
    DEFAULT_UNITS_PER_METRE, FOCUS_BORDER_COLOR, FOCUS_BORDER_WIDTH, GuiBlockerHit,
    GuiConstraintError, GuiControlVariant, GuiEvaluatedContent, GuiEvaluatedNode, GuiEvaluatedView,
    GuiFontResolution, GuiHit, GuiInteractionState, GuiLayoutCache, GuiLayoutDiagnostic,
    GuiLayoutRequest, GuiLayoutSystem, GuiLayoutSystemFactory, GuiPanelResolution, GuiPartMotion,
    GuiPartStyle, GuiResourceResolver, GuiSkinCursors, GuiSkinState, GuiSkinnedAppearance,
    MAX_LAYOUT_DEPTH, MAX_SKIN_DEPTH, MAX_SKIN_NODES, apply_appearance_to_primitive,
    apply_asset_to_primitive, focus_part_name, is_valid_skin_part, part_style, resolve_appearance,
    resolve_panel_hit, resolve_state_part_motion, resolve_state_part_style, skin_part_key,
    skinned_primitives_for_view, state_part_candidates, variant_for_content,
};
pub(crate) use layout::{
    appearance_with_effective_numeric, resolve_paint_appearance, skinned_parts_for_view,
    skinned_primitives_for_view_with_overrides,
};
pub(in crate::world) use system::validate_overlay_declaration;
pub use system::{
    GuiCommand, GuiInspectQuery, GuiInspectResponse, GuiInspectedNode, GuiSystem, GuiSystemFactory,
};
pub(crate) use tree::component::validate_property_value as validate_gui_property_value;
pub(crate) use tree::controls::slider_rail;
pub use tree::{
    GuiContainerKind, GuiControlState, GuiControlValue, GuiControls, GuiNode, GuiNodeContent,
    GuiNodeHandle, GuiNodeId, GuiNodePatch, GuiNodeStyle, GuiNodes, GuiRoot, MAX_GUI_TEXT_BYTES,
};
