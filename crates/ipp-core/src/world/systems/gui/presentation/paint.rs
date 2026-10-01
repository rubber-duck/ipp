use super::measurement::GuiControlLabel;
use super::{
    FOCUS_BORDER_COLOR, FOCUS_BORDER_WIDTH, GuiPartId, GuiPartStyle, GuiPartVariant,
    GuiPrimitivePart, GuiSkinState,
};
use super::{GuiSkin, GuiTheme};
use crate::EntityId;
use crate::services::asset_management::{
    AssetKey, AssetSource,
    drawing::{DRAWING_TYPE, DrawingAsset},
};
use crate::systems::SystemRuntimeAccess;
use crate::systems::canvas::*;
use crate::systems::gui::local::control::{GuiControl, GuiEligibility};
use crate::systems::gui::local::{GuiControlKind, GuiInteractionFlags};
use crate::world::WorldSimulationState;

/// A control's identity, eligibility fields and pointer feedback as its paint
/// reads them; values are read from the control's own fields.
#[derive(Clone, Copy, Debug)]
pub(in crate::world::systems) struct GuiPaintedControl {
    /// Exact control lifetime and role.
    pub control: GuiControl,
    /// Evaluated eligibility fields.
    pub eligibility: GuiEligibility,
    /// Aggregate live pointer feedback.
    pub interaction: GuiInteractionFlags,
}

fn skin(
    world: &WorldSimulationState,
    entity: EntityId,
) -> Result<(Option<&GuiSkin>, Option<&GuiTheme>), ()> {
    let skin = world.components.gui_skin(entity.index() as usize);
    let theme = match skin.filter(|skin| skin.theme.to_bits() != 0) {
        Some(skin) if world.state.entities.contains_key(&skin.theme) => Some(
            world
                .components
                .gui_theme(skin.theme.index() as usize)
                .ok_or(())?,
        ),
        Some(_) => return Err(()),
        None => None,
    };
    Ok((skin, theme))
}

pub(in crate::world::systems::gui) fn appearance(
    skin: Option<&GuiSkin>,
    theme: Option<&GuiTheme>,
    part: GuiPrimitivePart,
    state: GuiSkinState,
    variant: Option<GuiPartVariant>,
) -> GuiPartStyle {
    let base = GuiPartId::base(part).index().unwrap();
    let mut style = skin
        .and_then(|skin| {
            skin.parts
                .iter()
                .find(|(_, row)| row.part == base)
                .map(|(_, row)| row.style())
        })
        .unwrap_or_default();
    if let Some(theme) = theme {
        for identity in GuiPartId::candidates(part, state, variant) {
            if let Some((_, row)) = theme
                .parts
                .iter()
                .find(|(_, row)| Some(row.part) == identity.index())
            {
                style.inherit(&row.style());
            }
        }
    }
    style
}

pub(in crate::world::systems::gui) fn explicit_unchecked(
    theme: Option<&GuiTheme>,
    state: GuiSkinState,
) -> bool {
    theme.is_some_and(|theme| {
        let identity =
            GuiPartId::variant(GuiPrimitivePart::Icon, state, GuiPartVariant::Unchecked).index();
        theme.parts.iter().any(|(_, row)| {
            Some(row.part) == identity
                && (row.color.is_some() || row.opacity.is_some() || row.asset.is_some())
        })
    })
}

/// Axes whose theme styles the disabled track, asking for bars that stay
/// visible while content fits.
pub(in crate::world::systems) fn scroll_bars_kept(
    world: &WorldSimulationState,
    entity: EntityId,
) -> [bool; 2] {
    let Ok((_, Some(theme))) = skin(world, entity) else {
        return [false; 2];
    };
    std::array::from_fn(|axis| {
        let (track, _) = super::super::layout::scroll_bars::scroll_bar_parts(axis);
        let identity = GuiPartId::state(track, GuiSkinState::Disabled).index();
        theme.parts.iter().any(|(_, row)| {
            Some(row.part) == identity && (row.color.is_some() || row.opacity.is_some())
        })
    })
}

#[allow(clippy::too_many_arguments)]
pub(in crate::world::systems) fn control_paint(
    context: &SystemRuntimeAccess<'_>,
    painted: &GuiPaintedControl,
    focus_visible: bool,
    native: Option<&super::super::local::GuiNativeTextState>,
    style: CanvasPrimitiveStyle,
    layout: &super::super::layout::GuiEntityLayout,
    label: Option<&GuiControlLabel>,
    scroll_bars: &[super::super::layout::scroll_bars::GuiScrollBar],
    bar_interaction: super::super::local::GuiPartInteraction,
    retained: impl Fn(CanvasPrimitiveId) -> Option<AssetKey>,
) -> Option<Vec<CanvasPrimitive>> {
    let size = layout.size;
    let content_offset = layout.content_offset;
    let entity = painted.control.target.entity;
    let index = entity.index() as usize;
    let kind = painted.control.kind;
    let enabled = painted.eligibility.enabled;
    let (skin, theme) = skin(context.world, entity).ok()?;
    if !painted.eligibility.available || !painted.eligibility.visible {
        return Some(Vec::new());
    }
    let state = GuiSkinState::resolve(
        !enabled,
        painted.interaction.pressed,
        painted.interaction.hovered,
    );
    let checked = (kind == GuiControlKind::Checkbox)
        .then(|| context.world.components.gui_checkbox(index))
        .flatten()
        .map(|checkbox| checkbox.checked);
    let variant = checked.map(|checked| {
        if checked {
            GuiPartVariant::Checked
        } else {
            GuiPartVariant::Unchecked
        }
    });
    let resolve = |part| {
        let mut style = appearance(skin, theme, part, state, variant);
        if let Some(skin) = skin {
            skin.runtime.appearance(part as u32, &mut style);
        }
        style
    };
    let paint_shape = |part, rect, appearance: &GuiPartStyle| {
        let primitive = shape(style, part, rect, appearance);
        let previous = retained(primitive.style().identity);
        apply_asset(context, primitive, appearance, previous)
    };
    let mut primitives = Vec::new();
    let mut background = resolve(GuiPrimitivePart::Background);
    if background.color.is_none() && background.asset.is_none() {
        background.color = Some([0.16, 0.16, 0.16, 1.0]);
    }
    let track = if kind == GuiControlKind::Slider {
        [0.0, size[1] * 0.375, size[0], size[1] * 0.25]
    } else {
        [0.0, 0.0, size[0], size[1]]
    };
    primitives.extend(paint_shape(CanvasPart::Background, track, &background));
    match kind {
        GuiControlKind::ScrollView | GuiControlKind::VirtualList => {
            for bar in scroll_bars {
                let (track_part, thumb_part) =
                    super::super::layout::scroll_bars::scroll_bar_parts(bar.axis);
                let (track_id, thumb_id) = if bar.axis == 0 {
                    (CanvasPart::ScrollTrackX, CanvasPart::ScrollThumbX)
                } else {
                    (CanvasPart::ScrollTrackY, CanvasPart::ScrollThumbY)
                };
                // Each bar part resolves the pointers over or pressing that part.
                // A bar that cannot scroll resolves the disabled state and paints
                // only the parts its theme styles.
                for (part, id, rect, fallback, flags) in [
                    (
                        track_part,
                        track_id,
                        bar.track,
                        [0.12, 0.12, 0.12, 0.8],
                        bar_interaction.track[bar.axis],
                    ),
                    (
                        thumb_part,
                        thumb_id,
                        bar.thumb,
                        [0.65, 0.65, 0.65, 1.0],
                        bar_interaction.thumb[bar.axis],
                    ),
                ] {
                    let bar_state = GuiSkinState::resolve(
                        !enabled || !bar.enabled(),
                        flags.pressed,
                        flags.hovered,
                    );
                    let mut style = appearance(skin, theme, part, bar_state, variant);
                    if let Some(skin) = skin {
                        skin.runtime.appearance(part as u32, &mut style);
                    }
                    if !bar.enabled() && style.color.is_none() && style.opacity.is_none() {
                        continue;
                    }
                    style.color.get_or_insert(fallback);
                    primitives.extend(paint_shape(id, rect, &style));
                }
            }
        }
        GuiControlKind::Checkbox => {
            let mut icon = resolve(GuiPrimitivePart::Icon);
            if checked == Some(true)
                || explicit_unchecked(theme, state)
                || skin.is_some_and(|skin| skin.runtime.visible_icon())
            {
                icon.color.get_or_insert([1.0; 4]);
                let edge = size[0].min(size[1]) * 0.5;
                let align = icon.align_x.unwrap_or(0.0).clamp(-1.0, 1.0);
                let center = size[0] * 0.5 + (size[0] - size[1]).max(0.0) * align * 0.5;
                let scale = icon.scale.unwrap_or([1.0; 2]);
                primitives.extend(paint_shape(
                    CanvasPart::Icon,
                    [
                        center - edge * scale[0] * 0.5,
                        size[1] * 0.5 - edge * scale[1] * 0.5,
                        edge,
                        edge,
                    ],
                    &icon,
                ));
            }
        }
        GuiControlKind::Slider => {
            let slider = context.world.components.gui_slider(index)?;
            let ratio = if slider.max > slider.min {
                ((slider.value - slider.min) / (slider.max - slider.min)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut fill = resolve(GuiPrimitivePart::Fill);
            fill.color.get_or_insert([0.5, 0.7, 1.0, 1.0]);
            let mut icon = resolve(GuiPrimitivePart::Icon);
            icon.color.get_or_insert([1.0; 4]);
            if let Some(rail) = super::super::slider_rail([0.0, 0.0, size[0], size[1]]) {
                if let Some(rect) = rail.fill_rect(ratio, track[3]) {
                    primitives.extend(paint_shape(CanvasPart::Fill, rect, &fill));
                }
                if let Some(rect) = rail.thumb_rect(ratio) {
                    primitives.extend(paint_shape(CanvasPart::Icon, rect, &icon));
                }
            }
        }
        GuiControlKind::Button | GuiControlKind::TextInput => {
            let icon = resolve(GuiPrimitivePart::Icon);
            if icon.color.is_some() || icon.asset.is_some() {
                let edge = (size[1] * 0.55).min(size[0]);
                primitives.extend(paint_shape(
                    CanvasPart::Icon,
                    [(size[1] - edge) * 0.5, (size[1] - edge) * 0.5, edge, edge],
                    &icon,
                ));
            }
        }
    }
    if let Some(native) = native
        && let Some(label) = label
        && let Some(layout) = &label.layout
    {
        let selection = native.display_selection();
        let mut appearance = resolve(GuiPrimitivePart::Selection);
        appearance.color.get_or_insert([0.2, 0.45, 0.85, 0.5]);
        for rect in layout.selection_rects(selection[0], selection[1]) {
            let mut rect = rect.map(|value| value * label.font_size);
            rect[0] += content_offset[0];
            rect[1] += content_offset[1];
            primitives.extend(paint_shape(CanvasPart::Selection, rect, &appearance));
        }
        if let Some(caret) = layout.caret_position(selection[1]) {
            let mut appearance = resolve(GuiPrimitivePart::Caret);
            appearance.color.get_or_insert([1.0; 4]);
            primitives.extend(paint_shape(
                CanvasPart::Caret,
                [
                    content_offset[0] + caret.position[0] * label.font_size,
                    content_offset[1] + caret.position[1] * label.font_size,
                    (label.font_size * 0.06).max(0.5),
                    caret.height * label.font_size,
                ],
                &appearance,
            ));
        }
        if let Some(composition) = &native.composition {
            let start = native.selection[0].min(native.selection[1]);
            let mut appearance = resolve(GuiPrimitivePart::Composition);
            appearance.color.get_or_insert([1.0; 4]);
            for rect in layout.selection_rects(start, start + composition.text.len() as u32) {
                primitives.extend(paint_shape(
                    CanvasPart::Composition,
                    [
                        content_offset[0] + rect[0] * label.font_size,
                        content_offset[1] + (rect[1] + rect[3]) * label.font_size - 1.0,
                        rect[2] * label.font_size,
                        1.0,
                    ],
                    &appearance,
                ));
            }
        }
    }
    if let Some(label) = label
        && let Some(CanvasGeometry::Glyphs {
            font,
            font_size,
            glyphs,
            ..
        }) = &label.geometry
    {
        let appearance = resolve(GuiPrimitivePart::Label);
        let mut label_style = part_style(style, CanvasPart::Label, content_offset, &appearance);
        let color = appearance.color.unwrap_or([1.0; 4]);
        for (channel, tint) in label_style.color.iter_mut().zip(color) {
            *channel *= tint;
        }
        primitives.push(CanvasPrimitive::Glyphs {
            style: label_style,
            font: *font,
            font_size: *font_size,
            glyphs: glyphs.clone(),
        });
    }
    if enabled
        && (focus_visible
            || skin.is_some_and(|skin| skin.runtime.visible_part(GuiPrimitivePart::FocusRing)))
    {
        let mut focus = resolve(GuiPrimitivePart::FocusRing);
        focus.border_color = Some(
            focus
                .border_color
                .or(focus.color)
                .unwrap_or(FOCUS_BORDER_COLOR),
        );
        if focus.asset.is_none() {
            focus.color = Some([0.0; 4]);
        }
        focus.border_width.get_or_insert(FOCUS_BORDER_WIDTH);
        primitives.extend(paint_shape(
            CanvasPart::FocusRing,
            [0.0, 0.0, size[0], size[1]],
            &focus,
        ));
    }
    Some(primitives)
}

fn apply_asset(
    context: &SystemRuntimeAccess<'_>,
    primitive: CanvasPrimitive,
    appearance: &GuiPartStyle,
    retained: Option<AssetKey>,
) -> Option<CanvasPrimitive> {
    let Some(source) = appearance.asset.as_ref() else {
        return Some(primitive);
    };
    let assets = context.asset_resources();
    let ready = |key| {
        assets
            .get(key)
            .filter(|provider| provider.data().is_some())
            .map(|_| key)
    };
    let key = assets
        .find_source(context.world.id, source.kind, &source.uri, source.variant)
        .and_then(ready)
        .or_else(|| retained.and_then(ready))?;
    let AssetSource {
        kind,
        ..
    } = assets.get(key)?.source();
    let CanvasPrimitive::Box {
        mut style,
        size,
        ..
    } = primitive
    else {
        return Some(primitive);
    };
    for (channel, tint) in style
        .color
        .iter_mut()
        .zip(appearance.color.unwrap_or([1.0; 4]))
    {
        *channel *= tint;
    }
    if *kind == DRAWING_TYPE {
        let bounds = assets.get_typed::<DrawingAsset>(key)?.view_box();
        let extent = [bounds[2] - bounds[0], bounds[3] - bounds[1]];
        if extent
            .iter()
            .any(|extent| !extent.is_finite() || *extent <= 0.0)
        {
            return None;
        }
        style.scale[0] *= size[0] / extent[0];
        style.scale[1] *= size[1] / extent[1];
        style.position[0] -= bounds[0] * style.scale[0];
        style.position[1] -= bounds[1] * style.scale[1];
        Some(CanvasPrimitive::Drawing {
            style,
            drawing: key,
        })
    } else if *kind == crate::TEXTURE_TYPE {
        Some(CanvasPrimitive::Bitmap {
            style,
            bitmap: key,
            size,
        })
    } else {
        None
    }
}

fn part_style(
    mut style: CanvasPrimitiveStyle,
    part: CanvasPart,
    origin: [f32; 2],
    appearance: &GuiPartStyle,
) -> CanvasPrimitiveStyle {
    style.identity.part = part;
    style.position[0] += style.scale[0] * origin[0];
    style.position[1] += style.scale[1] * origin[1];
    let scale = appearance.scale.unwrap_or([1.0; 2]);
    style.scale[0] *= scale[0];
    style.scale[1] *= scale[1];
    style.opacity *= appearance.opacity.unwrap_or(1.0);
    style
}

fn shape(
    style: CanvasPrimitiveStyle,
    part: CanvasPart,
    rect: [f32; 4],
    appearance: &GuiPartStyle,
) -> CanvasPrimitive {
    let color = appearance.color.unwrap_or([1.0; 4]);
    let fill = match appearance.fill_mode {
        Some(1.0) => CanvasShapeFill::LinearGradient {
            start: appearance.gradient_start.unwrap_or([0.0; 2]),
            end: appearance.gradient_end.unwrap_or([1.0; 2]),
            start_color: appearance.gradient_color0.unwrap_or(color),
            end_color: appearance
                .gradient_color1
                .or(appearance.gradient_color0)
                .unwrap_or(color),
        },
        Some(2.0) => CanvasShapeFill::RadialGradient {
            center: appearance.gradient_start.unwrap_or([0.5; 2]),
            radius: appearance.gradient_radius.unwrap_or(0.5),
            start_color: appearance.gradient_color0.unwrap_or(color),
            end_color: appearance
                .gradient_color1
                .or(appearance.gradient_color0)
                .unwrap_or(color),
        },
        _ => CanvasShapeFill::Solid(color),
    };
    let glow = (appearance.glow_intensity.is_some_and(|value| value > 0.0)
        && appearance.glow_radius.is_some_and(|value| value > 0.0))
    .then(|| CanvasShapeGlow {
        color: appearance.glow_color.unwrap_or([1.0; 4]),
        intensity: appearance.glow_intensity.unwrap(),
        radius: appearance.glow_radius.unwrap(),
        falloff: appearance.glow_falloff.unwrap_or(1.0),
    });
    CanvasPrimitive::Box {
        style: part_style(style, part, [rect[0], rect[1]], appearance),
        size: [rect[2], rect[3]],
        corner_radius: appearance.corner_radius.unwrap_or([0.0; 2]),
        border_width: appearance.border_width.unwrap_or(0.0),
        border_color: appearance.border_color.unwrap_or([0.0; 4]),
        fill,
        glow,
    }
}
