//! GUI named-part resolution and retained Surface paint.
//!
//! Skinning observes evaluated layout and input cursors without changing control
//! behaviour, geometry, clipping, or painter order. Appearance and motion lanes
//! resolve independently through the same variant/state/base candidate chain.
//! AnimationSystem is the sole numeric sampler; this module resolves authored
//! destinations and paints effective values only.

use std::collections::{BTreeMap, BTreeSet};

use crate::services::asset_management::AssetSource;
use crate::systems::animation::AnimationTransitionEasing;
use crate::systems::gui::{
    GuiEvaluatedContent, GuiEvaluatedNode, GuiEvaluatedView, GuiInputFocus, GuiInputTarget,
    GuiNodeId, GuiResourceResolver, GuiRoot, MAX_LAYOUT_DEPTH,
};
use crate::systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
    SurfaceRenderPrimitive, gui_logical_to_surface_content,
};
use crate::{DynamicValue, EntityId};

/// Nodes per tree mirrored from the evaluator cap.
pub const MAX_SKIN_NODES: usize = 65_536;

/// Evaluation depth mirrored from [`MAX_LAYOUT_DEPTH`].
pub const MAX_SKIN_DEPTH: usize = MAX_LAYOUT_DEPTH;

/// Focus-ring border width in Surface metres.
pub const FOCUS_BORDER_WIDTH: f32 = 0.005;

/// Default focus-ring border colour.
pub const FOCUS_BORDER_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Slider-track height as a fraction of the retained control rectangle.
const SLIDER_TRACK_HEIGHT: f32 = 0.25;

/// Checkbox-indicator edge as a fraction of the retained control rectangle.
const CHECKBOX_INDICATOR_EDGE: f32 = 0.5;

/// Generic themed-icon edge as a fraction of the retained control height.
const CONTROL_ICON_EDGE: f32 = 0.55;

/// Resolved interaction state with fixed precedence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiSkinState {
    /// The control cannot interact.
    Disabled,
    /// A pointer currently presses the control.
    Pressed,
    /// A pointer currently hovers the control.
    Hovered,
    /// The enabled control has no pointer interaction.
    Idle,
}

impl GuiSkinState {
    /// Resolve disabled over pressed over hovered over idle.
    pub fn resolve(disabled: bool, pressed: bool, hovered: bool) -> Self {
        if disabled {
            Self::Disabled
        } else if pressed {
            Self::Pressed
        } else if hovered {
            Self::Hovered
        } else {
            Self::Idle
        }
    }

    /// Stable state suffix used by named parts.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Pressed => "pressed",
            Self::Hovered => "hovered",
            Self::Idle => "idle",
        }
    }
}

/// Per-node interaction with focus kept as an independent channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiInteractionState {
    /// Whether interaction is disabled.
    pub disabled: bool,
    /// Whether a pointer hovers this exact target.
    pub hovered: bool,
    /// Whether a pointer presses this exact target.
    pub pressed: bool,
    /// Whether keyboard focus names this exact target.
    pub focused: bool,
}

impl GuiInteractionState {
    /// Construct the enabled, unfocused idle state.
    pub fn idle() -> Self {
        Self {
            disabled: false,
            hovered: false,
            pressed: false,
            focused: false,
        }
    }

    /// Resolve the mutually exclusive appearance state.
    pub fn state(&self) -> GuiSkinState {
        GuiSkinState::resolve(self.disabled, self.pressed, self.hovered)
    }
}

/// Input cursors fenced by entity, root incarnation, node and lifetime.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiSkinCursors {
    /// Full-fenced hovered targets.
    pub hovered: BTreeSet<GuiInputTarget>,
    /// Full-fenced pressed targets.
    pub pressed: BTreeSet<GuiInputTarget>,
    /// Full-fenced keyboard focus.
    pub focus: Option<GuiInputFocus>,
}

impl GuiSkinCursors {
    /// Project transient cursors onto one exact live target.
    pub fn interaction_for(&self, target: GuiInputTarget, enabled: bool) -> GuiInteractionState {
        GuiInteractionState {
            disabled: !enabled,
            hovered: self.hovered.contains(&target),
            pressed: self.pressed.contains(&target),
            focused: self.focus.is_some_and(|focus| focus.target == target),
        }
    }
}

/// Checked/value variant carried by evaluated content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GuiControlVariant {
    /// Content without a skin suffix.
    Plain,
    /// Checkbox state, resolved as `checked` or `unchecked`.
    Checked(bool),
    /// Normalized slider value, retained for future value-qualified themes.
    Slider01(Option<f32>),
    /// Text-input metadata, retained without exposing the text to paint lookup.
    Text {
        /// Whether the input is empty.
        empty: bool,
        /// Committed text revision.
        revision: u32,
    },
}

impl GuiControlVariant {
    /// Return the stable authoring suffix, when this variant has one.
    pub fn suffix(self) -> Option<&'static str> {
        match self {
            Self::Checked(true) => Some("checked"),
            Self::Checked(false) => Some("unchecked"),
            Self::Plain
            | Self::Slider01(_)
            | Self::Text {
                ..
            } => None,
        }
    }
}

/// Variant for one evaluated content payload.
pub fn variant_for_content(content: &GuiEvaluatedContent) -> GuiControlVariant {
    match content {
        GuiEvaluatedContent::Checkbox {
            checked,
            ..
        } => GuiControlVariant::Checked(*checked),
        GuiEvaluatedContent::Slider {
            value,
            min,
            max,
            ..
        } => {
            let span = *max - *min;
            let ratio = if span.is_finite() && span != 0.0 {
                Some(((value - min) / span).clamp(0.0, 1.0))
            } else {
                None
            };
            GuiControlVariant::Slider01(ratio.map(|value| {
                if value.is_finite() {
                    value
                } else {
                    0.0
                }
            }))
        }
        GuiEvaluatedContent::TextInput {
            text,
            revision,
            ..
        } => GuiControlVariant::Text {
            empty: text.is_empty(),
            revision: *revision,
        },
        GuiEvaluatedContent::Container
        | GuiEvaluatedContent::Text {
            ..
        }
        | GuiEvaluatedContent::Drawing {
            ..
        }
        | GuiEvaluatedContent::Image {
            ..
        }
        | GuiEvaluatedContent::Button {
            ..
        } => GuiControlVariant::Plain,
    }
}

/// Return whether a named part is safe for dynamic-property lookup.
pub fn is_valid_skin_part(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Construct a validated node/part key.
pub fn skin_part_key(node: GuiNodeId, part: &str) -> Option<(GuiNodeId, String)> {
    is_valid_skin_part(part).then(|| (node, part.to_owned()))
}

/// Qualified part names in per-lane lookup order.
pub fn state_part_candidates(
    base: &str,
    state: GuiSkinState,
    variant: GuiControlVariant,
) -> Vec<String> {
    let mut names = Vec::with_capacity(3);
    if let Some(suffix) = variant.suffix() {
        names.push(format!("{base}_{}_{suffix}", state.suffix()));
    }
    names.push(format!("{base}_{}", state.suffix()));
    names.push(base.to_owned());
    names
}

/// Retained for source compatibility; focus-ring authoring uses `focusRing`.
pub fn focus_part_name(base: &str) -> String {
    format!("{base}_focus")
}

/// Resolved appearance lanes for one exact named part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiPartStyle {
    /// Optional linear RGBA override.
    pub color: Option<[f32; 4]>,
    /// Optional opacity multiplier.
    pub opacity: Option<f32>,
    /// Optional per-axis scale.
    pub scale: Option<[f32; 2]>,
    /// Optional drawing or bitmap source.
    pub asset: Option<AssetSource>,
}

impl GuiPartStyle {
    /// Return whether no valid lane was authored.
    pub fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.opacity.is_none()
            && self.scale.is_none()
            && self.asset.is_none()
    }
}

fn valid_color(value: [f32; 4]) -> bool {
    value
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}

fn valid_opacity(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn valid_scale(value: [f32; 2]) -> bool {
    value.iter().all(|v| v.is_finite())
}

/// Read valid lanes for one exact named part.
pub fn part_style(root: &GuiRoot, node: GuiNodeId, part: &str) -> GuiPartStyle {
    let mut style = GuiPartStyle::default();
    let Some(_) = skin_part_key(node, part) else {
        return style;
    };
    if let Some(name) = GuiRoot::part_property_name(node, part, "color")
        && let Some(DynamicValue::Vec4(color)) = root.properties.get(&name)
        && valid_color(color)
    {
        style.color = Some(color);
    }
    if let Some(name) = GuiRoot::part_property_name(node, part, "opacity")
        && let Some(DynamicValue::F32(opacity)) = root.properties.get(&name)
        && valid_opacity(opacity)
    {
        style.opacity = Some(opacity);
    }
    if let Some(name) = GuiRoot::part_property_name(node, part, "scale")
        && let Some(DynamicValue::Vec2(scale)) = root.properties.get(&name)
        && valid_scale(scale)
    {
        style.scale = Some(scale);
    }
    if let Some(name) = GuiRoot::part_property_name(node, part, "asset")
        && let Some(source) = root.properties.asset(&name).cloned()
    {
        style.asset = Some(source);
    }
    style
}

/// Resolve appearance lanes independently through variant, state and base names.
pub fn resolve_state_part_style(
    root: &GuiRoot,
    node: GuiNodeId,
    base: &str,
    state: GuiSkinState,
    variant: GuiControlVariant,
) -> GuiPartStyle {
    let mut merged = GuiPartStyle::default();
    if skin_part_key(node, base).is_none() {
        return merged;
    }
    for candidate in state_part_candidates(base, state, variant) {
        let next = part_style(root, node, &candidate);
        merged.color = merged.color.or(next.color);
        merged.opacity = merged.opacity.or(next.opacity);
        merged.scale = merged.scale.or(next.scale);
        merged.asset = merged.asset.or(next.asset);
    }
    merged
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ControlVisualSources {
    background: bool,
    fill: bool,
    icon: bool,
    plain_icon: bool,
}

/// Whether one canonical candidate can contribute a visual source for `base`,
/// and whether it belongs to the plain (non-checked) lookup chain.
fn visual_candidate(candidate: &str, base: &str) -> Option<bool> {
    if candidate == base {
        return Some(true);
    }
    let suffix = candidate.strip_prefix(base)?.strip_prefix('_')?;
    let mut parts = suffix.split('_');
    if !matches!(
        parts.next(),
        Some("disabled" | "pressed" | "hovered" | "idle")
    ) {
        return None;
    }
    match (parts.next(), parts.next()) {
        (None, None) => Some(true),
        (Some("checked" | "unchecked"), None) => Some(false),
        _ => None,
    }
}

/// Scan one node's canonical descriptors once for geometry-producing lanes.
/// Opacity and scale alone never synthesize geometry.
fn control_visual_sources(root: &GuiRoot, node: GuiNodeId) -> ControlVisualSources {
    let prefix = format!("node_{}_part_", node.0);
    let mut sources = ControlVisualSources::default();
    for (name, descriptor) in root
        .properties
        .descriptors()
        .range(prefix.clone()..)
        .take_while(|(name, _)| name.starts_with(&prefix))
    {
        if !matches!(
            descriptor.kind,
            crate::DynamicPropertyKind::Vec4 | crate::DynamicPropertyKind::Asset
        ) {
            continue;
        }
        let Some(candidate) = name.strip_prefix(&prefix).and_then(|name| {
            name.strip_suffix("_color")
                .or_else(|| name.strip_suffix("_asset"))
        }) else {
            continue;
        };
        if visual_candidate(candidate, GuiPrimitivePart::Background.as_str()).is_some() {
            sources.background = true;
        }
        if visual_candidate(candidate, GuiPrimitivePart::Fill.as_str()).is_some() {
            sources.fill = true;
        }
        if let Some(plain) = visual_candidate(candidate, GuiPrimitivePart::Icon.as_str()) {
            sources.icon = true;
            sources.plain_icon |= plain;
        }
    }
    sources
}

/// Appearance resolved for one stable part.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiSkinnedAppearance {
    /// Resolved interaction state.
    pub state: GuiSkinState,
    /// Independent focus channel.
    pub focused: bool,
    /// Resolved content variant.
    pub variant: GuiControlVariant,
    /// Optional color lane.
    pub color: Option<[f32; 4]>,
    /// Optional opacity lane.
    pub opacity: Option<f32>,
    /// Optional scale lane.
    pub scale: Option<[f32; 2]>,
    /// Optional drawing or bitmap source.
    pub asset: Option<AssetSource>,
    /// Focus-ring color when this is the focus-ring part.
    pub focus_border_color: Option<[f32; 4]>,
}

/// Resolve one stable primitive part without coupling it to sibling parts.
pub fn resolve_appearance(
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    interaction: &GuiInteractionState,
    base_part: &str,
) -> Option<GuiSkinnedAppearance> {
    skin_part_key(node.node, base_part)?;
    let state = interaction.state();
    let variant = variant_for_content(&node.content);
    let lanes = resolve_state_part_style(root, node.node, base_part, state, variant);
    Some(GuiSkinnedAppearance {
        state,
        focused: interaction.focused,
        variant,
        color: lanes.color,
        opacity: lanes.opacity,
        scale: lanes.scale,
        asset: lanes.asset,
        focus_border_color: (base_part == GuiPrimitivePart::FocusRing.as_str())
            .then_some(lanes.color)
            .flatten(),
    })
}

/// Apply control-owned defaults after ordinary named-part resolution.
/// Checkbox `icon` base lanes describe the checked indicator; unchecked stays
/// hidden unless its exact variant declares color, opacity, or an asset.
/// This rule is shared by paint and AnimationSystem destination ownership.
pub(crate) fn resolve_paint_appearance(
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    interaction: &GuiInteractionState,
    part: GuiPrimitivePart,
) -> Option<GuiSkinnedAppearance> {
    let mut appearance = resolve_appearance(root, node, interaction, part.as_str())?;
    if part != GuiPrimitivePart::Icon {
        return Some(appearance);
    }

    match &node.content {
        GuiEvaluatedContent::Checkbox {
            checked,
            ..
        } => {
            let exact = format!("icon_{}_unchecked", interaction.state().suffix());
            let unchecked = part_style(root, node.node, &exact);
            let explicit_unchecked = unchecked.color.is_some()
                || unchecked.opacity.is_some()
                || unchecked.asset.is_some();
            if !checked && !explicit_unchecked {
                appearance.opacity = Some(0.0);
            } else if appearance.color.is_none() && appearance.asset.is_none() {
                appearance.color = Some(node.color);
            }
        }
        GuiEvaluatedContent::Slider {
            ..
        } if appearance.color.is_none() && appearance.asset.is_none() => {
            appearance.color = Some(node.color);
        }
        _ => {}
    }
    Some(appearance)
}

/// Animation clip and crossfade configuration for one resolved state.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiPartMotion {
    /// Animation clip source.
    pub source: AssetSource,
    /// Crossfade duration in seconds.
    pub duration_secs: f64,
    /// Crossfade easing.
    pub easing: AnimationTransitionEasing,
    /// First of the color, opacity and scale tracks.
    pub base_track: u32,
    /// Clip time that represents this state's authored destination.
    pub sample_time: f64,
}

/// Resolve every motion lane independently through the appearance candidate chain.
pub fn resolve_state_part_motion(
    root: &GuiRoot,
    node: GuiNodeId,
    base: &str,
    state: GuiSkinState,
    variant: GuiControlVariant,
) -> Option<GuiPartMotion> {
    let candidates = state_part_candidates(base, state, variant);
    let asset_lane = |suffix: &str| {
        candidates.iter().find_map(|candidate| {
            let name = GuiRoot::part_property_name(node, candidate, suffix)?;
            root.properties.asset(&name).cloned()
        })
    };
    let f32_lane = |suffix: &str| {
        candidates.iter().find_map(|candidate| {
            let name = GuiRoot::part_property_name(node, candidate, suffix)?;
            match root.properties.get(&name) {
                Some(DynamicValue::F32(value)) => Some(value),
                _ => None,
            }
        })
    };
    let source = asset_lane("motion")?;
    if source.kind != crate::systems::animation::ANIMATION_TYPE {
        return None;
    }
    let duration = f32_lane("duration")?;
    let easing = match f32_lane("easing")? {
        0.0 => AnimationTransitionEasing::Linear,
        1.0 => AnimationTransitionEasing::Smoothstep,
        _ => return None,
    };
    let track = super::super::tree::component::skin_motion_base_track(f32_lane("track")?)?;
    let time = f32_lane("time")?;
    if !duration.is_finite() || duration < 0.0 || !time.is_finite() || time < 0.0 {
        return None;
    }
    Some(GuiPartMotion {
        source,
        duration_secs: duration as f64,
        easing,
        base_track: track,
        sample_time: time as f64,
    })
}

/// Replace destination numeric lanes with effective animated base lanes.
pub(crate) fn appearance_with_effective_numeric(
    mut appearance: GuiSkinnedAppearance,
    effective_root: &GuiRoot,
    node: GuiNodeId,
    part: GuiPrimitivePart,
) -> GuiSkinnedAppearance {
    let sampled = part_style(effective_root, node, part.as_str());
    appearance.color = sampled.color.or(appearance.color);
    appearance.opacity = sampled.opacity.or(appearance.opacity);
    appearance.scale = sampled.scale.or(appearance.scale);
    appearance
}

/// Apply numeric skin lanes while preserving primitive identity and geometry.
pub fn apply_appearance_to_primitive(
    primitive: &SurfaceRenderPrimitive,
    appearance: &GuiSkinnedAppearance,
) -> SurfaceRenderPrimitive {
    crate::systems::surface::surface_primitive_with_skin_style(
        primitive,
        appearance.color,
        appearance.opacity,
        appearance.scale,
    )
}

/// Swap drawing/bitmap resources only; measured glyph fonts never change here.
pub fn apply_asset_to_primitive(
    primitive: SurfaceRenderPrimitive,
    appearance: &GuiSkinnedAppearance,
    resolver: &dyn GuiResourceResolver,
) -> SurfaceRenderPrimitive {
    let resource = appearance
        .asset
        .as_ref()
        .and_then(|source| resolver.surface_resource(source));
    let fallback = primitive.clone();
    apply_resolved_asset_to_primitive(primitive, resource, resolver).unwrap_or(fallback)
}

/// Paint one evaluated view with independently resolved named parts.
pub fn skinned_primitives_for_view(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    cursors: &GuiSkinCursors,
    resolver: &dyn GuiResourceResolver,
) -> Vec<SurfaceRenderPrimitive> {
    let mut retained = BTreeMap::new();
    skinned_primitives_for_view_with_retained_resources(
        view,
        root,
        cursors,
        resolver,
        &mut retained,
    )
}

pub(crate) fn skinned_primitives_for_view_with_retained_resources(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    cursors: &GuiSkinCursors,
    resolver: &dyn GuiResourceResolver,
    retained_resources: &mut BTreeMap<
        (EntityId, GuiPrimitiveId),
        crate::systems::surface::SurfaceRenderResource,
    >,
) -> Vec<SurfaceRenderPrimitive> {
    skinned_primitives_for_view_with_overrides(
        view,
        root,
        cursors,
        resolver,
        retained_resources,
        &BTreeMap::new(),
    )
}

pub(crate) fn skinned_primitives_for_view_with_overrides(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    cursors: &GuiSkinCursors,
    resolver: &dyn GuiResourceResolver,
    retained_resources: &mut BTreeMap<
        (EntityId, GuiPrimitiveId),
        crate::systems::surface::SurfaceRenderResource,
    >,
    overrides: &BTreeMap<GuiPrimitiveId, GuiSkinnedAppearance>,
) -> Vec<SurfaceRenderPrimitive> {
    let base = view.surface_primitives();
    if !view.available {
        return base;
    }
    let mut by_node: BTreeMap<(GuiNodeId, u32), Vec<SurfaceRenderPrimitive>> = BTreeMap::new();
    let mut unexpected = Vec::new();
    for primitive in base {
        match primitive_gui_id(&primitive) {
            Some(id) if id.root_incarnation == view.root_incarnation => {
                by_node
                    .entry((id.node, id.lifetime))
                    .or_default()
                    .push(primitive);
            }
            _ => unexpected.push(primitive),
        }
    }

    let mut painted = Vec::new();
    for (index, node) in view.nodes.iter().enumerate() {
        let eligible = index < MAX_SKIN_NODES
            && node.depth as usize <= MAX_SKIN_DEPTH
            && node.available
            && !node.paint_suppressed;
        let interaction = cursors.interaction_for(
            GuiInputTarget {
                entity: view.entity,
                root_incarnation: view.root_incarnation,
                node: node.node,
                lifetime: node.lifetime,
            },
            node.enabled,
        );
        let mut node_primitives = by_node
            .remove(&(node.node, node.lifetime))
            .unwrap_or_default();
        let synthesis = if eligible {
            synthetic_control_plan(view, root, node, &interaction)
        } else {
            SyntheticControlPlan::default()
        };
        if eligible {
            for part in [
                GuiPrimitivePart::Background,
                GuiPrimitivePart::Fill,
                GuiPrimitivePart::Icon,
            ] {
                if node_primitives
                    .iter()
                    .any(|primitive| primitive_gui_id(primitive).is_some_and(|id| id.part == part))
                {
                    continue;
                }
                if let Some(synthetic) = synthesis.part(part) {
                    let insertion = node_primitives
                        .iter()
                        .position(|primitive| {
                            primitive_gui_id(primitive).is_some_and(|id| {
                                part == GuiPrimitivePart::Background
                                    || (part == GuiPrimitivePart::Fill
                                        && id.part == GuiPrimitivePart::Icon)
                                    || id.part == GuiPrimitivePart::Label
                            })
                        })
                        .unwrap_or(node_primitives.len());
                    node_primitives.insert(insertion, synthetic.primitive.clone());
                }
            }
        }
        for primitive in node_primitives {
            if eligible {
                let id = primitive_gui_id(&primitive);
                let resource_required = id
                    .and_then(|id| synthesis.part(id.part))
                    .is_some_and(|synthetic| synthetic.resource_required);
                if let Some(primitive) = skin_primitive(
                    view,
                    root,
                    node,
                    &interaction,
                    resolver,
                    retained_resources,
                    overrides,
                    primitive,
                    resource_required,
                ) {
                    painted.push(primitive);
                }
            } else {
                painted.push(primitive);
            }
        }
        if eligible && interaction.focused {
            let id = GuiPrimitiveId {
                root_incarnation: view.root_incarnation,
                node: node.node,
                lifetime: node.lifetime,
                part: GuiPrimitivePart::FocusRing,
            };
            if let Some(focus) =
                focus_ring_primitive(view, root, node, &interaction, overrides.get(&id))
            {
                painted.push(focus);
            }
        }
    }
    for primitives in by_node.into_values() {
        painted.extend(primitives);
    }
    painted.extend(unexpected);
    painted
}

/// One stable paint identity paired with its already-resolved evaluated node.
pub(crate) struct GuiSkinnedPart<'a> {
    pub(crate) id: GuiPrimitiveId,
    pub(crate) node: &'a GuiEvaluatedNode,
}

/// Stable paint inventory that skin composition can emit for this view.
///
/// RenderSystem consumes the carried node directly, avoiding a part-to-node
/// search for every presentation. Synthesized parts share the same inventory
/// and AnimationSystem ownership as retained layout parts.
pub(crate) fn skinned_parts_for_view<'a>(
    view: &'a GuiEvaluatedView,
    root: &GuiRoot,
    cursors: &GuiSkinCursors,
) -> Vec<GuiSkinnedPart<'a>> {
    let mut parts = Vec::new();
    for entry in view.surface_part_inventory() {
        let node = entry.node;
        if entry.index >= MAX_SKIN_NODES || node.depth as usize > MAX_SKIN_DEPTH {
            continue;
        }
        let target = GuiInputTarget {
            entity: view.entity,
            root_incarnation: view.root_incarnation,
            node: node.node,
            lifetime: node.lifetime,
        };
        let interaction = cursors.interaction_for(target, node.enabled);
        let synthesis = synthetic_control_plan(view, root, node, &interaction);
        let mut has_background = false;
        let mut has_fill = false;
        let mut has_icon = false;
        for part in entry.parts.into_iter().flatten() {
            has_background |= part == GuiPrimitivePart::Background;
            has_fill |= part == GuiPrimitivePart::Fill;
            has_icon |= part == GuiPrimitivePart::Icon;
            parts.push(GuiSkinnedPart {
                id: GuiPrimitiveId {
                    root_incarnation: view.root_incarnation,
                    node: node.node,
                    lifetime: node.lifetime,
                    part,
                },
                node,
            });
        }
        for part in [
            GuiPrimitivePart::Background,
            GuiPrimitivePart::Fill,
            GuiPrimitivePart::Icon,
        ] {
            let present = match part {
                GuiPrimitivePart::Background => has_background,
                GuiPrimitivePart::Fill => has_fill,
                GuiPrimitivePart::Icon => has_icon,
                GuiPrimitivePart::Label | GuiPrimitivePart::FocusRing => false,
            };
            if !present && synthesis.part(part).is_some() {
                parts.push(GuiSkinnedPart {
                    id: GuiPrimitiveId {
                        root_incarnation: view.root_incarnation,
                        node: node.node,
                        lifetime: node.lifetime,
                        part,
                    },
                    node,
                });
            }
        }
        if interaction.focused {
            parts.push(GuiSkinnedPart {
                id: GuiPrimitiveId {
                    root_incarnation: view.root_incarnation,
                    node: node.node,
                    lifetime: node.lifetime,
                    part: GuiPrimitivePart::FocusRing,
                },
                node,
            });
        }
    }
    parts
}

#[derive(Default)]
struct SyntheticControlPlan {
    background: Option<SyntheticControlPart>,
    fill: Option<SyntheticControlPart>,
    icon: Option<SyntheticControlPart>,
}

impl SyntheticControlPlan {
    fn part(&self, part: GuiPrimitivePart) -> Option<&SyntheticControlPart> {
        match part {
            GuiPrimitivePart::Background => self.background.as_ref(),
            GuiPrimitivePart::Fill => self.fill.as_ref(),
            GuiPrimitivePart::Icon => self.icon.as_ref(),
            GuiPrimitivePart::Label | GuiPrimitivePart::FocusRing => None,
        }
    }
}

struct SyntheticControlPart {
    primitive: SurfaceRenderPrimitive,
    resource_required: bool,
}

#[cfg(test)]
std::thread_local! {
    static SYNTHESIS_PLAN_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn synthesis_plan_builds(reset: bool) -> usize {
    SYNTHESIS_PLAN_BUILDS.with(|builds| {
        let value = builds.get();
        if reset {
            builds.set(0);
        }
        value
    })
}

fn synthetic_control_plan(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    interaction: &GuiInteractionState,
) -> SyntheticControlPlan {
    #[cfg(test)]
    SYNTHESIS_PLAN_BUILDS.with(|builds| builds.set(builds.get() + 1));

    let sources = control_visual_sources(root, node.node);
    SyntheticControlPlan {
        background: synthetic_control_part(
            view,
            root,
            node,
            interaction,
            GuiPrimitivePart::Background,
            sources,
        ),
        fill: synthetic_control_part(
            view,
            root,
            node,
            interaction,
            GuiPrimitivePart::Fill,
            sources,
        ),
        icon: synthetic_control_part(
            view,
            root,
            node,
            interaction,
            GuiPrimitivePart::Icon,
            sources,
        ),
    }
}

fn synthetic_control_part(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    interaction: &GuiInteractionState,
    part: GuiPrimitivePart,
    sources: ControlVisualSources,
) -> Option<SyntheticControlPart> {
    let units = view.units_per_metre;
    if !(units.is_finite()
        && units > 0.0
        && node.rect[2].is_finite()
        && node.rect[2] > 0.0
        && node.rect[3].is_finite()
        && node.rect[3] > 0.0)
    {
        return None;
    }

    let has_affordance = node.background.is_some() || sources.background || sources.icon;
    let (logical, color, opacity, corner_radius) = match (&node.content, part) {
        (
            GuiEvaluatedContent::Button {
                ..
            }
            | GuiEvaluatedContent::Checkbox {
                ..
            }
            | GuiEvaluatedContent::TextInput {
                ..
            },
            GuiPrimitivePart::Background,
        ) if node.background.is_none() && sources.background => {
            let radius = node.rect[2].min(node.rect[3]) * 0.08 / units;
            (node.rect, [0.0, 0.0, 0.0, 0.0], node.opacity, radius)
        }
        (
            GuiEvaluatedContent::Slider {
                ..
            },
            GuiPrimitivePart::Background,
        ) if node.background.is_none() && sources.background => {
            let height = node.rect[3] * SLIDER_TRACK_HEIGHT;
            (
                [
                    node.rect[0],
                    node.rect[1] + (node.rect[3] - height) * 0.5,
                    node.rect[2],
                    height,
                ],
                [0.0, 0.0, 0.0, 0.0],
                node.opacity,
                height * 0.5 / units,
            )
        }
        (
            GuiEvaluatedContent::Checkbox {
                checked,
                ..
            },
            GuiPrimitivePart::Icon,
        ) if has_affordance => {
            let edge = node.rect[2].min(node.rect[3]) * CHECKBOX_INDICATOR_EDGE;
            let current = resolve_state_part_style(
                root,
                node.node,
                GuiPrimitivePart::Icon.as_str(),
                interaction.state(),
                variant_for_content(&node.content),
            );
            let default_visible = current.color.is_none() && current.asset.is_none() && *checked;
            (
                [
                    node.rect[0] + (node.rect[2] - edge) * 0.5,
                    node.rect[1] + (node.rect[3] - edge) * 0.5,
                    edge,
                    edge,
                ],
                if default_visible {
                    node.color
                } else {
                    [0.0, 0.0, 0.0, 0.0]
                },
                node.opacity,
                edge * 0.12 / units,
            )
        }
        (
            GuiEvaluatedContent::Slider {
                ..
            },
            GuiPrimitivePart::Fill,
        ) if sources.fill => {
            let ratio = match variant_for_content(&node.content) {
                GuiControlVariant::Slider01(Some(value)) => value,
                _ => 0.0,
            };
            let height = node.rect[3] * SLIDER_TRACK_HEIGHT;
            let logical = super::super::slider_rail(node.rect)?.fill_rect(ratio, height)?;
            (logical, node.color, node.opacity, height * 0.5 / units)
        }
        (
            GuiEvaluatedContent::Slider {
                ..
            },
            GuiPrimitivePart::Icon,
        ) if has_affordance => {
            let ratio = match variant_for_content(&node.content) {
                GuiControlVariant::Slider01(Some(value)) => value,
                _ => 0.0,
            };
            let logical = super::super::slider_rail(node.rect)?.thumb_rect(ratio)?;
            let edge = logical[2];
            (logical, node.color, node.opacity, edge * 0.5 / units)
        }
        (
            GuiEvaluatedContent::Button {
                ..
            }
            | GuiEvaluatedContent::TextInput {
                ..
            },
            GuiPrimitivePart::Icon,
        ) if sources.plain_icon => {
            let edge = (node.rect[3] * CONTROL_ICON_EDGE).min(node.rect[2]);
            (
                [
                    node.rect[0] + (node.rect[3] - edge) * 0.5,
                    node.rect[1] + (node.rect[3] - edge) * 0.5,
                    edge,
                    edge,
                ],
                [0.0, 0.0, 0.0, 0.0],
                node.opacity,
                edge * 0.12 / units,
            )
        }
        _ => return None,
    };
    let position = gui_logical_to_surface_content([logical[0], logical[1]], units)?;
    let clip = node.clip.and_then(|clip| {
        let min = gui_logical_to_surface_content([clip[0], clip[1]], units)?;
        let max = gui_logical_to_surface_content([clip[2], clip[3]], units)?;
        Some([min[0], min[1], max[0], max[1]])
    });
    let appearance = resolve_state_part_style(
        root,
        node.node,
        part.as_str(),
        interaction.state(),
        variant_for_content(&node.content),
    );
    let resource_required = appearance.asset.is_some() && appearance.color.is_none();
    let color = if resource_required {
        [1.0, 1.0, 1.0, 1.0]
    } else {
        color
    };
    Some(SyntheticControlPart {
        primitive: SurfaceRenderPrimitive::Box {
            style: SurfacePrimitiveStyle {
                identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                    root_incarnation: view.root_incarnation,
                    node: node.node,
                    lifetime: node.lifetime,
                    part,
                }),
                position,
                scale: [1.0, 1.0],
                color,
                opacity,
                clip,
            },
            size: [logical[2] / units, logical[3] / units],
            corner_radius: [corner_radius, corner_radius],
            border_width: 0.0,
            border_color: [0.0, 0.0, 0.0, 0.0],
        },
        resource_required,
    })
}

fn primitive_gui_id(primitive: &SurfaceRenderPrimitive) -> Option<GuiPrimitiveId> {
    match primitive.style().identity {
        SurfacePrimitiveIdentity::Gui(id) => Some(id),
        SurfacePrimitiveIdentity::Authored(_) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn skin_primitive(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    interaction: &GuiInteractionState,
    resolver: &dyn GuiResourceResolver,
    retained_resources: &mut BTreeMap<
        (EntityId, GuiPrimitiveId),
        crate::systems::surface::SurfaceRenderResource,
    >,
    overrides: &BTreeMap<GuiPrimitiveId, GuiSkinnedAppearance>,
    primitive: SurfaceRenderPrimitive,
    resource_required: bool,
) -> Option<SurfaceRenderPrimitive> {
    let Some(id) = primitive_gui_id(&primitive) else {
        return Some(primitive);
    };
    let retained_key = (view.entity, id);
    let appearance = overrides
        .get(&id)
        .cloned()
        .or_else(|| resolve_paint_appearance(root, node, interaction, id.part));
    let Some(appearance) = appearance else {
        retained_resources.remove(&retained_key);
        return Some(primitive);
    };
    let styled = apply_appearance_to_primitive(&primitive, &appearance);
    let supports_resource = !matches!(styled, SurfaceRenderPrimitive::Glyphs { .. });
    let resource = if supports_resource {
        match appearance.asset.as_ref() {
            Some(source) if primitive_accepts_skin_asset(&styled, source) => {
                match resolver.surface_resource(source) {
                    Some(resource) if primitive_accepts_skin_asset(&styled, &resource.source) => {
                        retained_resources.insert(retained_key, resource.clone());
                        Some(resource)
                    }
                    Some(_) | None => retained_resources.get(&retained_key).cloned(),
                }
            }
            None => {
                retained_resources.remove(&retained_key);
                None
            }
            Some(_) => {
                retained_resources.remove(&retained_key);
                None
            }
        }
    } else {
        retained_resources.remove(&retained_key);
        None
    };
    if resource_required && resource.is_none() {
        return None;
    }
    let fallback = styled.clone();
    apply_resolved_asset_to_primitive(styled, resource, resolver)
        .or_else(|| (!resource_required).then_some(fallback))
}

fn is_supported_skin_asset(source: &AssetSource) -> bool {
    source.kind == crate::services::asset_management::drawing::DRAWING_TYPE
        || source.kind == crate::TEXTURE_TYPE
}

fn primitive_accepts_skin_asset(primitive: &SurfaceRenderPrimitive, source: &AssetSource) -> bool {
    match primitive {
        SurfaceRenderPrimitive::Drawing {
            ..
        } => source.kind == crate::services::asset_management::drawing::DRAWING_TYPE,
        SurfaceRenderPrimitive::Bitmap {
            ..
        } => source.kind == crate::TEXTURE_TYPE,
        SurfaceRenderPrimitive::Box {
            ..
        } => is_supported_skin_asset(source),
        SurfaceRenderPrimitive::Glyphs {
            ..
        } => false,
    }
}

fn focus_ring_primitive(
    view: &GuiEvaluatedView,
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    interaction: &GuiInteractionState,
    override_appearance: Option<&GuiSkinnedAppearance>,
) -> Option<SurfaceRenderPrimitive> {
    let units = view.units_per_metre;
    let position = gui_logical_to_surface_content([node.rect[0], node.rect[1]], units)?;
    let clip = node.clip.and_then(|clip| {
        let min = gui_logical_to_surface_content([clip[0], clip[1]], units)?;
        let max = gui_logical_to_surface_content([clip[2], clip[3]], units)?;
        Some([min[0], min[1], max[0], max[1]])
    });
    let appearance = override_appearance.cloned().or_else(|| {
        resolve_appearance(
            root,
            node,
            interaction,
            GuiPrimitivePart::FocusRing.as_str(),
        )
    })?;
    let color = appearance
        .focus_border_color
        .or(appearance.color)
        .unwrap_or(FOCUS_BORDER_COLOR);
    Some(SurfaceRenderPrimitive::Box {
        style: SurfacePrimitiveStyle {
            identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                root_incarnation: view.root_incarnation,
                node: node.node,
                lifetime: node.lifetime,
                part: GuiPrimitivePart::FocusRing,
            }),
            position,
            scale: appearance.scale.unwrap_or([1.0, 1.0]),
            color: [0.0, 0.0, 0.0, 0.0],
            opacity: appearance.opacity.unwrap_or(node.opacity),
            clip,
        },
        size: [node.rect[2] / units, node.rect[3] / units],
        corner_radius: [0.0, 0.0],
        border_width: FOCUS_BORDER_WIDTH,
        border_color: color,
    })
}

fn apply_resolved_asset_to_primitive(
    primitive: SurfaceRenderPrimitive,
    resource: Option<crate::systems::surface::SurfaceRenderResource>,
    resolver: &dyn GuiResourceResolver,
) -> Option<SurfaceRenderPrimitive> {
    let Some(resource) = resource else {
        return Some(primitive);
    };
    match (primitive, resource.source.kind) {
        (
            SurfaceRenderPrimitive::Box {
                mut style,
                size,
                ..
            },
            kind,
        ) if kind == crate::services::asset_management::drawing::DRAWING_TYPE => {
            let view_box = resolver.drawing_view_box(&resource.source)?;
            let extent = [view_box[2] - view_box[0], view_box[3] - view_box[1]];
            if !view_box.iter().all(|value| value.is_finite())
                || !extent.iter().all(|value| *value > 0.0)
                || !size.iter().all(|value| value.is_finite() && *value > 0.0)
            {
                return None;
            }
            let fit = [size[0] / extent[0], size[1] / extent[1]];
            style.scale = [style.scale[0] * fit[0], style.scale[1] * fit[1]];
            style.position = [
                style.position[0] - view_box[0] * style.scale[0],
                style.position[1] - view_box[1] * style.scale[1],
            ];
            if !style.position.iter().all(|value| value.is_finite())
                || !style.scale.iter().all(|value| value.is_finite())
            {
                return None;
            }
            Some(SurfaceRenderPrimitive::Drawing {
                style,
                drawing: resource,
            })
        }
        (
            SurfaceRenderPrimitive::Box {
                style,
                size,
                ..
            }
            | SurfaceRenderPrimitive::Bitmap {
                style,
                size,
                ..
            },
            kind,
        ) if kind == crate::TEXTURE_TYPE => Some(SurfaceRenderPrimitive::Bitmap {
            style,
            bitmap: resource,
            size,
        }),
        (
            SurfaceRenderPrimitive::Drawing {
                style,
                ..
            },
            kind,
        ) if kind == crate::services::asset_management::drawing::DRAWING_TYPE => {
            Some(SurfaceRenderPrimitive::Drawing {
                style,
                drawing: resource,
            })
        }
        (
            SurfaceRenderPrimitive::Bitmap {
                style,
                size,
                ..
            },
            kind,
        ) if kind == crate::TEXTURE_TYPE => Some(SurfaceRenderPrimitive::Bitmap {
            style,
            bitmap: resource,
            size,
        }),
        (other, _) => Some(other),
    }
}

#[cfg(test)]
#[path = "skin_tests.rs"]
mod tests;
