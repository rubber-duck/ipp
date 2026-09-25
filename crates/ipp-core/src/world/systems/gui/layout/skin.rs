//! GUI skin resolution and retained Surface paint.
//!
//! Skinning observes evaluated layout and input cursors without changing control
//! behaviour, geometry, clipping, or painter order. Each part property resolves
//! independently: a node's part-row override wins, then its theme's (part,
//! state, variant), (part, state) and (part) rows in that order. Motion
//! resolves through the same theme chain. AnimationSystem is the sole numeric
//! sampler; this module resolves authored destinations and paints effective
//! values only.
//!
//! Colour, opacity, scale and the checkbox indicator's `align_x` are the
//! animated properties: a state's motion clip supplies colour, opacity and
//! scale tracks from its base track, plus an `align_x` track after them when
//! the part's base resolves that property, so a switch knob glides between its
//! unchecked and checked positions. Transitions write the node's part-row
//! channels, which replace those destinations only while a transition owns
//! them. The remaining shape, gradient and glow properties are static
//! material values of the resolved state.

use std::collections::{BTreeMap, BTreeSet};

use crate::EntityId;
use crate::services::asset_management::AssetSource;
use crate::systems::animation::AnimationTransitionEasing;
use crate::systems::gui::{
    GuiEvaluatedContent, GuiEvaluatedNode, GuiEvaluatedView, GuiInputFocus, GuiInputTarget,
    GuiNodeId, GuiPartId, GuiPartRow, GuiPartVariant, GuiResourceResolver, GuiRoot,
    GuiThemePartRow, MAX_LAYOUT_DEPTH,
};
use crate::systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, GuiShapeFill, GuiShapeGlow, SurfacePrimitiveIdentity,
    SurfacePrimitiveStyle, SurfaceRenderPrimitive, gui_logical_to_surface_content,
};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// Whether the focus source earns the focus ring: set only for keyboard
    /// and programmatic focus, never for pointer-press focus. Text inputs
    /// additionally keep the ring on pointer focus; see [`Self::ring_visible`].
    pub focus_visible: bool,
}

impl GuiInteractionState {
    /// Construct the enabled, unfocused idle state.
    pub fn idle() -> Self {
        Self {
            disabled: false,
            hovered: false,
            pressed: false,
            focused: false,
            focus_visible: false,
        }
    }

    /// Whether this target paints the focus ring: keyboard and programmatic
    /// focus everywhere, pointer-press focus only on text inputs where
    /// keyboard input lands and the caret target must stay visible.
    pub fn ring_visible(&self, is_text_input: bool) -> bool {
        self.focus_visible || (self.focused && is_text_input)
    }

    /// Resolve the mutually exclusive appearance state.
    pub fn state(&self) -> GuiSkinState {
        GuiSkinState::resolve(self.disabled, self.pressed, self.hovered)
    }
}

/// Input cursors fenced by entity, root incarnation and node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiSkinCursors {
    /// Full-fenced hovered targets.
    pub hovered: BTreeSet<GuiInputTarget>,
    /// Full-fenced pressed targets.
    pub pressed: BTreeSet<GuiInputTarget>,
    /// Full-fenced keyboard focus.
    pub focus: Option<GuiInputFocus>,
    /// Whether the focus source earns the focus ring: keyboard and
    /// programmatic focus show it, pointer-press focus does not. Text inputs
    /// additionally keep the ring on pointer focus at paint time; see
    /// [`GuiInteractionState::ring_visible`].
    pub focus_visible: bool,
    /// Committed offsets and bar interaction of ScrollViews that scrolled or
    /// whose scroll bars a pointer hovers or presses.
    pub scroll_bars: BTreeMap<GuiInputTarget, GuiScrollBarCursor>,
}

/// Scroll bar paint inputs of one ScrollView.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GuiScrollBarCursor {
    /// Committed scroll offset in local logical units.
    pub offset: [f32; 2],
    /// Scroll bar part a pointer hovers.
    pub hovered: Option<GuiPrimitivePart>,
    /// Scroll bar part a pointer presses or drags.
    pub pressed: Option<GuiPrimitivePart>,
}

impl GuiSkinCursors {
    /// Project transient cursors onto one exact live target.
    pub fn interaction_for(&self, target: GuiInputTarget, enabled: bool) -> GuiInteractionState {
        let focused = self.focus.is_some_and(|focus| focus.target == target);
        GuiInteractionState {
            disabled: !enabled,
            hovered: self.hovered.contains(&target),
            pressed: self.pressed.contains(&target),
            focused,
            focus_visible: focused && self.focus_visible,
        }
    }

    /// Project transient cursors onto one part of one exact live target:
    /// scroll bar parts follow their own hover and press and disable while
    /// they cannot scroll; every other part shares the node's interaction.
    pub fn interaction_for_part(
        &self,
        target: GuiInputTarget,
        part: GuiPrimitivePart,
        node: &GuiEvaluatedNode,
    ) -> GuiInteractionState {
        if super::scroll_bars::scroll_bar_axis(part).is_some() {
            return super::scroll_bars::scroll_bar_interaction(self, target, part, node);
        }
        self.interaction_for(target, node.enabled)
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
    /// Checked variant qualifying skin parts, when this variant has one.
    pub fn part_variant(self) -> Option<GuiPartVariant> {
        match self {
            Self::Checked(true) => Some(GuiPartVariant::Checked),
            Self::Checked(false) => Some(GuiPartVariant::Unchecked),
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

/// Appearance properties resolved for one part; absent properties fall back
/// to control defaults when painted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiPartStyle {
    /// Optional linear RGBA override.
    pub color: Option<[f32; 4]>,
    /// Optional opacity multiplier.
    pub opacity: Option<f32>,
    /// Optional per-axis scale.
    pub scale: Option<[f32; 2]>,
    /// Optional horizontal alignment along the control, clamped to -1..=1 when
    /// painted. Only the checkbox indicator reads it: -1 and +1 centre it in
    /// the left- and right-most height-square cells of a wider control.
    pub align_x: Option<f32>,
    /// Optional drawing or bitmap source.
    pub asset: Option<AssetSource>,
    /// Optional per-axis corner radii `[rx, ry]` in local Surface metres.
    pub corner_radius: Option<[f32; 2]>,
    /// Optional border width in local Surface metres.
    pub border_width: Option<f32>,
    /// Optional straight linear RGBA border color.
    pub border_color: Option<[f32; 4]>,
    /// Optional fill mode: 0.0 = solid, 1.0 = linear gradient, 2.0 = radial gradient.
    ///
    /// Solid paints the colour. Like every property, the mode resolves
    /// independently through the candidate chain, so a state that paints its
    /// own colour over an inherited gradient declares mode 0 explicitly.
    pub fill_mode: Option<f32>,
    /// Optional gradient start point (or radial center) in local shape metres.
    pub gradient_start: Option<[f32; 2]>,
    /// Optional gradient end point in local shape metres.
    pub gradient_end: Option<[f32; 2]>,
    /// Optional gradient stop 0 straight linear RGBA color.
    pub gradient_color0: Option<[f32; 4]>,
    /// Optional gradient stop 1 straight linear RGBA color.
    pub gradient_color1: Option<[f32; 4]>,
    /// Optional radial gradient radius in local shape metres.
    pub gradient_radius: Option<f32>,
    /// Optional straight linear RGBA glow color.
    pub glow_color: Option<[f32; 4]>,
    /// Optional glow intensity multiplier (>= 0.0).
    pub glow_intensity: Option<f32>,
    /// Optional outward glow radius in local Surface metres (>= 0.0).
    pub glow_radius: Option<f32>,
    /// Optional glow falloff exponent (>= 0.0).
    pub glow_falloff: Option<f32>,
}

/// Appearance properties of a theme or part row, which share names.
macro_rules! row_part_style {
    ($row:expr) => {
        GuiPartStyle {
            color: $row.color,
            opacity: $row.opacity,
            scale: $row.scale,
            align_x: $row.align_x,
            asset: $row.asset.clone(),
            corner_radius: $row.corner_radius,
            border_width: $row.border_width,
            border_color: $row.border_color,
            fill_mode: $row.fill_mode,
            gradient_start: $row.gradient_start,
            gradient_end: $row.gradient_end,
            gradient_color0: $row.gradient_color0,
            gradient_color1: $row.gradient_color1,
            gradient_radius: $row.gradient_radius,
            glow_color: $row.glow_color,
            glow_intensity: $row.glow_intensity,
            glow_radius: $row.glow_radius,
            glow_falloff: $row.glow_falloff,
        }
    };
}

impl From<&GuiThemePartRow> for GuiPartStyle {
    fn from(row: &GuiThemePartRow) -> Self {
        row_part_style!(row)
    }
}

impl GuiPartStyle {
    /// Appearance overrides of a part row, without its live channels.
    pub fn overrides(row: &GuiPartRow) -> Self {
        row_part_style!(row)
    }

    /// Return whether no property is present.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Fill every absent property from a less specific source.
    fn inherit(&mut self, next: &Self) {
        fn or<T: Clone>(target: &mut Option<T>, next: &Option<T>) {
            if target.is_none() {
                target.clone_from(next);
            }
        }

        or(&mut self.color, &next.color);
        or(&mut self.opacity, &next.opacity);
        or(&mut self.scale, &next.scale);
        or(&mut self.align_x, &next.align_x);
        or(&mut self.asset, &next.asset);
        or(&mut self.corner_radius, &next.corner_radius);
        or(&mut self.border_width, &next.border_width);
        or(&mut self.border_color, &next.border_color);
        or(&mut self.fill_mode, &next.fill_mode);
        or(&mut self.gradient_start, &next.gradient_start);
        or(&mut self.gradient_end, &next.gradient_end);
        or(&mut self.gradient_color0, &next.gradient_color0);
        or(&mut self.gradient_color1, &next.gradient_color1);
        or(&mut self.gradient_radius, &next.gradient_radius);
        or(&mut self.glow_color, &next.glow_color);
        or(&mut self.glow_intensity, &next.glow_intensity);
        or(&mut self.glow_radius, &next.glow_radius);
        or(&mut self.glow_falloff, &next.glow_falloff);
    }
}

/// Appearance of one exact part identity of a node's theme; empty without a
/// live theme or row.
pub fn theme_part_style(root: &GuiRoot, node: GuiNodeId, part: GuiPartId) -> GuiPartStyle {
    root.node_theme_slot(node)
        .and_then(|slot| root.theme_row(slot, part))
        .map(GuiPartStyle::from)
        .unwrap_or_default()
}

/// Appearance overrides one node declares for a base part.
pub fn part_overrides(root: &GuiRoot, node: GuiNodeId, part: GuiPrimitivePart) -> GuiPartStyle {
    root.part_row(node, part)
        .map(|(_, row)| GuiPartStyle::overrides(row))
        .unwrap_or_default()
}

/// Resolve every appearance property independently: the node's override,
/// then its theme's (part, state, variant), (part, state) and (part) rows.
pub fn resolve_state_part_style(
    root: &GuiRoot,
    node: GuiNodeId,
    base: GuiPrimitivePart,
    state: GuiSkinState,
    variant: GuiControlVariant,
) -> GuiPartStyle {
    let mut merged = part_overrides(root, node, base);
    if let Some(theme) = root.node_theme_slot(node) {
        for candidate in GuiPartId::candidates(base, state, variant.part_variant()) {
            if let Some(row) = root.theme_row(theme, candidate) {
                merged.inherit(&GuiPartStyle::from(row));
            }
        }
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

/// Find the geometry-producing properties (colour or asset) a node's theme
/// and overrides declare for the synthesized parts. Opacity and scale alone
/// never synthesize geometry. Icon sources from the base or a state without a
/// variant, and every override, belong to the plain (non-checked) chain.
fn control_visual_sources(root: &GuiRoot, node: GuiNodeId) -> ControlVisualSources {
    let theme = root.node_theme_slot(node);
    let mut sources = ControlVisualSources::default();
    for part in [
        GuiPrimitivePart::Background,
        GuiPrimitivePart::Fill,
        GuiPrimitivePart::Icon,
    ] {
        let overridden = root
            .part_row(node, part)
            .is_some_and(|(_, row)| row.color.is_some() || row.asset.is_some());
        let mut plain = overridden;
        let mut any = overridden;
        if let Some(theme) = theme {
            let first =
                super::super::tree::part_rows::base_part_index(part) * GuiPartId::QUALIFIERS;
            for index in first..first + GuiPartId::QUALIFIERS {
                let Some(id) = GuiPartId::from_index(index) else {
                    continue;
                };
                if root
                    .theme_row(theme, id)
                    .is_some_and(|row| row.color.is_some() || row.asset.is_some())
                {
                    any = true;
                    plain |= id.variant.is_none();
                }
            }
        }
        match part {
            GuiPrimitivePart::Background => sources.background = any,
            GuiPrimitivePart::Fill => sources.fill = any,
            _ => {
                sources.icon = any;
                sources.plain_icon = plain;
            }
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
    /// Optional colour.
    pub color: Option<[f32; 4]>,
    /// Optional opacity.
    pub opacity: Option<f32>,
    /// Optional scale.
    pub scale: Option<[f32; 2]>,
    /// Optional horizontal alignment.
    pub align_x: Option<f32>,
    /// Optional drawing or bitmap source.
    pub asset: Option<AssetSource>,
    /// Optional per-axis corner radii `[rx, ry]` in local Surface metres.
    pub corner_radius: Option<[f32; 2]>,
    /// Optional border width in local Surface metres.
    pub border_width: Option<f32>,
    /// Optional straight linear RGBA border color.
    pub border_color: Option<[f32; 4]>,
    /// Optional linear or radial gradient fill.
    ///
    /// Solid fills are not duplicated here: a box without a gradient paints
    /// the `color` property, and a focus ring without a border colour strokes it.
    /// Both therefore observe the colour AnimationSystem samples during a
    /// transition. Gradient stops are separate, unanimated material properties; a
    /// missing stop takes the destination colour when the gradient resolves.
    pub fill: Option<GuiShapeFill>,
    /// Optional local glow.
    pub glow: Option<GuiShapeGlow>,
}

/// Resolve one stable primitive part without coupling it to sibling parts.
pub fn resolve_appearance(
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    interaction: &GuiInteractionState,
    base_part: GuiPrimitivePart,
) -> Option<GuiSkinnedAppearance> {
    let state = interaction.state();
    let variant = variant_for_content(&node.content);
    let resolved = resolve_state_part_style(root, node.node, base_part, state, variant);
    let fill = match resolved.fill_mode {
        Some(1.0) => {
            let start = resolved.gradient_start.unwrap_or([0.0, 0.0]);
            let end = resolved.gradient_end.unwrap_or([1.0, 1.0]);
            let start_color = resolved
                .gradient_color0
                .or(resolved.color)
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let end_color = resolved.gradient_color1.unwrap_or(start_color);
            Some(GuiShapeFill::LinearGradient {
                start,
                end,
                start_color,
                end_color,
            })
        }
        Some(2.0) => {
            let center = resolved.gradient_start.unwrap_or([0.5, 0.5]);
            let radius = resolved.gradient_radius.unwrap_or(0.5);
            let start_color = resolved
                .gradient_color0
                .or(resolved.color)
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let end_color = resolved.gradient_color1.unwrap_or(start_color);
            Some(GuiShapeFill::RadialGradient {
                center,
                radius,
                start_color,
                end_color,
            })
        }
        // Solid mode paints the colour when the primitive is styled.
        _ => None,
    };

    let glow = if resolved.glow_intensity.is_some_and(|i| i > 0.0)
        && resolved.glow_radius.is_some_and(|r| r > 0.0)
    {
        Some(GuiShapeGlow {
            color: resolved.glow_color.unwrap_or([1.0, 1.0, 1.0, 1.0]),
            intensity: resolved.glow_intensity.unwrap_or(1.0),
            radius: resolved.glow_radius.unwrap_or(0.0),
            falloff: resolved.glow_falloff.unwrap_or(1.0),
        })
    } else {
        None
    };

    Some(GuiSkinnedAppearance {
        state,
        focused: interaction.focused,
        variant,
        color: resolved.color,
        opacity: resolved.opacity,
        scale: resolved.scale,
        align_x: resolved.align_x,
        asset: resolved.asset,
        corner_radius: resolved.corner_radius,
        border_width: resolved.border_width,
        border_color: resolved.border_color,
        fill,
        glow,
    })
}

/// Apply control-owned defaults after ordinary named-part resolution.
/// Checkbox `icon` base properties describe the checked indicator; unchecked stays
/// hidden unless its exact variant declares color, opacity, or an asset.
/// This rule is shared by paint and AnimationSystem destination ownership.
pub(crate) fn resolve_paint_appearance(
    root: &GuiRoot,
    node: &GuiEvaluatedNode,
    interaction: &GuiInteractionState,
    part: GuiPrimitivePart,
) -> Option<GuiSkinnedAppearance> {
    let mut appearance = resolve_appearance(root, node, interaction, part)?;
    if part != GuiPrimitivePart::Icon {
        return Some(appearance);
    }

    match &node.content {
        GuiEvaluatedContent::Checkbox {
            checked,
            ..
        } => {
            let unchecked = theme_part_style(
                root,
                node.node,
                GuiPartId::variant(
                    GuiPrimitivePart::Icon,
                    interaction.state(),
                    GuiPartVariant::Unchecked,
                ),
            );
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
    /// Whether the clip also animates `align_x` from `base_track + 3`.
    ///
    /// Set when the part's base resolves `align_x`, so every state resolves
    /// an alignment destination.
    pub animates_align: bool,
    /// Clip time that represents this state's authored destination.
    pub sample_time: f64,
    /// `part_state` slot whose live channels the transition writes.
    pub channels: u32,
}

/// Resolve every motion property independently through the node theme's
/// candidate chain. None without motion or without the node's live channels.
pub fn resolve_state_part_motion(
    root: &GuiRoot,
    node: GuiNodeId,
    base: GuiPrimitivePart,
    state: GuiSkinState,
    variant: GuiControlVariant,
) -> Option<GuiPartMotion> {
    let theme = root.node_theme_slot(node)?;
    let (channels, _) = root
        .part_row(node, base)
        .filter(|(_, row)| row.has_channels())?;
    let rows: Vec<&GuiThemePartRow> = GuiPartId::candidates(base, state, variant.part_variant())
        .filter_map(|candidate| root.theme_row(theme, candidate))
        .collect();
    let property =
        |read: fn(&GuiThemePartRow) -> Option<f32>| rows.iter().find_map(|row| read(row));
    let source = rows.iter().find_map(|row| row.motion.clone())?;
    if source.kind != crate::systems::animation::ANIMATION_TYPE {
        return None;
    }
    let duration = property(|row| row.duration)?;
    let easing = match property(|row| row.easing)? {
        0.0 => AnimationTransitionEasing::Linear,
        1.0 => AnimationTransitionEasing::Smoothstep,
        _ => return None,
    };
    let track = super::super::tree::part_rows::skin_motion_base_track(property(|row| row.track)?)?;
    let animates_align = part_overrides(root, node, base).align_x.is_some()
        || theme_part_style(root, node, GuiPartId::base(base))
            .align_x
            .is_some();
    if animates_align {
        track.checked_add(3)?;
    }
    let time = property(|row| row.time)?;
    if !duration.is_finite() || duration < 0.0 || !time.is_finite() || time < 0.0 {
        return None;
    }
    Some(GuiPartMotion {
        source,
        duration_secs: duration as f64,
        easing,
        base_track: track,
        animates_align,
        sample_time: time as f64,
        channels,
    })
}

/// Whether a node's colour, opacity and scale channels for a part are
/// present, so a transition can bind them.
pub(crate) fn skin_channels_complete(
    root: &GuiRoot,
    node: GuiNodeId,
    part: GuiPrimitivePart,
) -> bool {
    root.part_row(node, part).is_some_and(|(_, row)| {
        row.live_color.is_some() && row.live_opacity.is_some() && row.live_scale.is_some()
    })
}

/// Replace destination numeric properties with the live channels an owning
/// transition samples; alignment only when its motion animates it.
///
/// Solid fills and focus-ring strokes read the colour when applied, so
/// replacing `color` here is sufficient for them to paint the sampled value.
pub(crate) fn appearance_with_effective_numeric(
    mut appearance: GuiSkinnedAppearance,
    effective_root: &GuiRoot,
    node: GuiNodeId,
    part: GuiPrimitivePart,
    animates_align: bool,
) -> GuiSkinnedAppearance {
    if let Some((_, row)) = effective_root.part_row(node, part) {
        appearance.color = row.live_color.or(appearance.color);
        appearance.opacity = row.live_opacity.or(appearance.opacity);
        appearance.scale = row.live_scale.or(appearance.scale);
        if animates_align {
            appearance.align_x = row.live_align_x.or(appearance.align_x);
        }
    }
    appearance
}

/// Apply numeric skin properties while preserving primitive identity and geometry.
pub fn apply_appearance_to_primitive(
    primitive: &SurfaceRenderPrimitive,
    appearance: &GuiSkinnedAppearance,
) -> SurfaceRenderPrimitive {
    let mut next = crate::systems::surface::surface_primitive_with_skin_style(
        primitive,
        appearance.color,
        appearance.opacity,
        appearance.scale,
    );
    #[cfg(feature = "gui")]
    if let SurfaceRenderPrimitive::Box {
        corner_radius,
        border_width,
        border_color,
        fill,
        glow,
        ..
    } = &mut next
    {
        if let Some(r) = appearance
            .corner_radius
            .filter(|r| r.iter().all(|v| v.is_finite() && *v >= 0.0))
        {
            *corner_radius = r;
        }
        if let Some(w) = appearance
            .border_width
            .filter(|w| w.is_finite() && *w >= 0.0)
        {
            *border_width = w;
        }
        if let Some(c) = appearance
            .border_color
            .filter(|c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
        {
            *border_color = c;
        }
        // A gradient replaces the fill; otherwise the colour is the
        // solid fill, including its sampled value mid-transition.
        if let Some(f) = appearance.fill.filter(|f| f.is_valid()) {
            *fill = f;
        } else if let Some(c) = appearance
            .color
            .filter(|c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
        {
            *fill = GuiShapeFill::Solid(c);
        }
        if let Some(g) = appearance.glow.filter(|g| g.is_valid()) {
            *glow = Some(g);
        }
    }
    next
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
    let mut by_node: BTreeMap<GuiNodeId, Vec<SurfaceRenderPrimitive>> = BTreeMap::new();
    let mut unexpected = Vec::new();
    for primitive in base {
        match primitive_gui_id(&primitive) {
            Some(id) if id.root_incarnation == view.root_incarnation => {
                by_node.entry(id.node).or_default().push(primitive);
            }
            _ => unexpected.push(primitive),
        }
    }

    let mut painted = Vec::new();
    // Scroll bars paint above their ScrollView's whole subtree: each waits
    // here, innermost last, until painter order leaves that subtree.
    let mut bars: Vec<(u32, Vec<SurfaceRenderPrimitive>)> = Vec::new();
    for (index, node) in view.nodes.iter().enumerate() {
        while bars.last().is_some_and(|(depth, _)| *depth >= node.depth) {
            painted.extend(
                bars.pop()
                    .map(|(_, primitives)| primitives)
                    .unwrap_or_default(),
            );
        }
        let eligible = index < MAX_SKIN_NODES
            && node.depth as usize <= MAX_SKIN_DEPTH
            && node.available
            && !node.paint_suppressed;
        let interaction = cursors.interaction_for(
            GuiInputTarget {
                entity: view.entity,
                root_incarnation: view.root_incarnation,
                node: node.node,
            },
            node.enabled,
        );
        let mut node_primitives = by_node.remove(&node.node).unwrap_or_default();
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
        let ring = interaction.ring_visible(matches!(
            node.content,
            GuiEvaluatedContent::TextInput { .. }
        ));
        if eligible && ring {
            let id = GuiPrimitiveId {
                root_incarnation: view.root_incarnation,
                node: node.node,
                part: GuiPrimitivePart::FocusRing,
            };
            if let Some(focus) =
                focus_ring_primitive(view, root, node, &interaction, overrides.get(&id))
            {
                painted.push(focus);
            }
        }
        if eligible && node.viewport.is_some() {
            bars.push((
                node.depth,
                super::scroll_bars::scroll_bar_primitives(view, root, node, cursors, overrides),
            ));
        }
    }
    while let Some((_, primitives)) = bars.pop() {
        painted.extend(primitives);
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
                GuiPrimitivePart::Label
                | GuiPrimitivePart::FocusRing
                | GuiPrimitivePart::ScrollTrackX
                | GuiPrimitivePart::ScrollThumbX
                | GuiPrimitivePart::ScrollTrackY
                | GuiPrimitivePart::ScrollThumbY => false,
                // Text-input overlays paint outside skin synthesis.
                GuiPrimitivePart::Caret
                | GuiPrimitivePart::Selection
                | GuiPrimitivePart::Composition => false,
            };
            if !present && synthesis.part(part).is_some() {
                parts.push(GuiSkinnedPart {
                    id: GuiPrimitiveId {
                        root_incarnation: view.root_incarnation,
                        node: node.node,
                        part,
                    },
                    node,
                });
            }
        }
        let ring = interaction.ring_visible(matches!(
            node.content,
            GuiEvaluatedContent::TextInput { .. }
        ));
        if ring {
            parts.push(GuiSkinnedPart {
                id: GuiPrimitiveId {
                    root_incarnation: view.root_incarnation,
                    node: node.node,
                    part: GuiPrimitivePart::FocusRing,
                },
                node,
            });
        }
        for axis in 0..2 {
            if !super::scroll_bars::scroll_bar_shown(root, node, axis) {
                continue;
            }
            let (track, thumb) = super::scroll_bars::scroll_bar_parts(axis);
            for part in [track, thumb] {
                parts.push(GuiSkinnedPart {
                    id: GuiPrimitiveId {
                        root_incarnation: view.root_incarnation,
                        node: node.node,
                        part,
                    },
                    node,
                });
            }
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
            GuiPrimitivePart::Label
            | GuiPrimitivePart::FocusRing
            | GuiPrimitivePart::ScrollTrackX
            | GuiPrimitivePart::ScrollThumbX
            | GuiPrimitivePart::ScrollTrackY
            | GuiPrimitivePart::ScrollThumbY => None,
            GuiPrimitivePart::Caret
            | GuiPrimitivePart::Selection
            | GuiPrimitivePart::Composition => None,
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
                GuiPrimitivePart::Icon,
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
            // Not inset by the track border by decision (ipp-jtst.7): the
            // flush fill and the track read as one shape; see fill_rect.
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
        part,
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
            fill: GuiShapeFill::Solid(color),
            glow: None,
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
    let styled = match &node.content {
        GuiEvaluatedContent::Checkbox {
            ..
        } if id.part == GuiPrimitivePart::Icon => {
            place_checkbox_indicator(styled, node.rect, view.units_per_metre, &appearance)
        }
        _ => styled,
    };
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

/// Move a checkbox indicator along its control and scale it about its centre.
///
/// The indicator is synthesized as a square centred in the control rectangle.
/// For a control wider than tall, alignment `t` (clamped to -1..=1) places the
/// indicator centre at `x + h/2 + (t + 1)/2 * (w - h)`: 0 keeps the centred
/// square, while -1 and +1 centre it in the left- and right-most `h`-square
/// cells, so a switch knob keeps the same inset from both track ends. A
/// control no wider than tall ignores alignment. The scale then resizes
/// the indicator about that centre. Neutral values leave geometry untouched.
fn place_checkbox_indicator(
    mut primitive: SurfaceRenderPrimitive,
    rect: [f32; 4],
    units: f32,
    appearance: &GuiSkinnedAppearance,
) -> SurfaceRenderPrimitive {
    if let SurfaceRenderPrimitive::Box {
        style,
        size,
        ..
    } = &mut primitive
    {
        let align = appearance
            .align_x
            .filter(|align| align.is_finite())
            .map_or(0.0, |align| align.clamp(-1.0, 1.0));
        let travel = if rect[2] > rect[3] {
            align * 0.5 * (rect[2] - rect[3]) / units
        } else {
            0.0
        };
        let offset = [
            travel + size[0] * (1.0 - style.scale[0]) * 0.5,
            size[1] * (1.0 - style.scale[1]) * 0.5,
        ];
        if offset != [0.0, 0.0] && offset.iter().all(|value| value.is_finite()) {
            style.position = [style.position[0] + offset[0], style.position[1] + offset[1]];
        }
    }
    primitive
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
    let appearance = override_appearance
        .cloned()
        .or_else(|| resolve_appearance(root, node, interaction, GuiPrimitivePart::FocusRing))?;
    let color = appearance
        .border_color
        .or(appearance.color)
        .unwrap_or(FOCUS_BORDER_COLOR);
    Some(SurfaceRenderPrimitive::Box {
        style: SurfacePrimitiveStyle {
            identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                root_incarnation: view.root_incarnation,
                node: node.node,
                part: GuiPrimitivePart::FocusRing,
            }),
            position,
            scale: appearance.scale.unwrap_or([1.0, 1.0]),
            color: [0.0, 0.0, 0.0, 0.0],
            opacity: appearance.opacity.unwrap_or(node.opacity),
            clip,
        },
        size: [node.rect[2] / units, node.rect[3] / units],
        corner_radius: appearance.corner_radius.unwrap_or([0.0, 0.0]),
        border_width: appearance.border_width.unwrap_or(FOCUS_BORDER_WIDTH),
        border_color: color,
        fill: appearance.fill.unwrap_or(GuiShapeFill::Solid([0.0; 4])),
        glow: appearance.glow,
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
