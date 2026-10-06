use super::component::em_scale;
use super::looks::{GuiSkinLook, control_look, default_look};
use super::measurement::GuiControlLabel;
use super::{
    GUI_DEFAULT_FONT_SIZE, GuiPaintPart, GuiPartId, GuiPartStyle, GuiPartVariant, GuiPrimitivePart,
    GuiSkinState,
};
use super::{GuiSkin, GuiTheme};
use crate::EntityId;
use crate::services::asset_management::{
    AssetKey, AssetSource,
    formats::drawing::{DRAWING_TYPE, DrawingAsset},
};
use crate::systems::SystemRuntimeAccess;
use crate::systems::canvas::*;
use crate::systems::gui::layout::scroll_bars::GUI_SCROLL_BAR_EMS;
use crate::systems::gui::local::controls::color::{GuiColorLayout, hsv_to_linear};
use crate::systems::gui::local::controls::identity::{GuiControl, GuiEligibility};
use crate::systems::gui::local::controls::slider::{GuiSliderDial, slider_dial};
use crate::systems::gui::local::{GuiColor, GuiControlKind, GuiInteractionFlags};
use crate::systems::gui::motion::focus_part_channel;
use crate::world::WorldSimulationState;

/// Thickness of the marks drawn on a text line in ems: the caret's width and
/// the composition underline along the bottom of the line.
const TEXT_MARK: f32 = 1.0 / 16.0;

/// Side of a numeric input's step marks in ems, centred in their parts.
const STEP_MARK: f32 = 0.75;

/// Linear RGBA of the checker's absent first colour, sRGB `#CCCCCC`.
const CHECKER_LIGHT: [f32; 4] = [0.603_827_36, 0.603_827_36, 0.603_827_36, 1.0];

/// Linear RGBA of the checker's absent second colour, sRGB `#999999`.
const CHECKER_DARK: [f32; 4] = [0.318_546_77, 0.318_546_77, 0.318_546_77, 1.0];

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

/// A control part's appearance in `state` and `variant` for an inherited font
/// of `font_size`: the skin's override row, then the theme's rows and then the
/// control's default `look`, each most specific first, every source filling
/// only the properties still absent.
pub(in crate::world::systems::gui) fn appearance(
    skin: Option<&GuiSkin>,
    theme: Option<&GuiTheme>,
    look: &GuiSkinLook,
    font_size: f32,
    part: GuiPrimitivePart,
    state: GuiSkinState,
    variant: Option<GuiPartVariant>,
) -> GuiPartStyle {
    resolve_rows(
        skin,
        theme,
        Some(look),
        font_size,
        part,
        GuiPartId::candidates(part, state, variant),
    )
}

/// The skin's unqualified override row of `part`, then each theme row and each
/// `look` row of `candidates`, most specific first, filling only properties
/// still absent. Override rows are absolute; theme and look rows draw their
/// lengths at `font_size` against their `em`.
fn resolve_rows(
    skin: Option<&GuiSkin>,
    theme: Option<&GuiTheme>,
    look: Option<&GuiSkinLook>,
    font_size: f32,
    part: GuiPrimitivePart,
    candidates: impl Iterator<Item = GuiPartId> + Clone,
) -> GuiPartStyle {
    let base = GuiPartId::base(part);
    let mut style = skin
        .and_then(|skin| part_row(skin.parts.iter().map(|(_, row)| row), base))
        .map(GuiPaintPart::style)
        .unwrap_or_default();
    if let Some(theme) = theme {
        let scale = theme.length_scale(font_size);
        for identity in candidates.clone() {
            if let Some(row) = part_row(theme.parts.iter().map(|(_, row)| row), identity) {
                style.inherit(&row.scaled_style(scale));
            }
        }
    }
    if let Some(look) = look {
        let scale = em_scale(look.em, font_size);
        for identity in candidates {
            if let Some(row) = look.row(identity) {
                style.inherit(&row.scaled_style(scale));
            }
        }
    }
    style
}

/// The label font size `entity` inherits: the nearest [`GuiFont`] at or above
/// it, or the default.
///
/// [`GuiFont`]: super::GuiFont
pub(in crate::world::systems::gui) fn inherited_font_size(
    world: &WorldSimulationState,
    entity: EntityId,
) -> f32 {
    let mut current = Some(entity);
    while let Some(entity) = current {
        if let Some(font) = world.components.gui_font(entity.index() as usize) {
            return font.font_size;
        }
        current = world.state.links.parent(entity);
    }
    GUI_DEFAULT_FONT_SIZE
}

/// The row of one part identity among `rows`.
fn part_row<'a>(
    rows: impl IntoIterator<Item = &'a GuiPaintPart>,
    identity: GuiPartId,
) -> Option<&'a GuiPaintPart> {
    let index = identity.index()?;
    rows.into_iter().find(|row| row.part == index)
}

/// The checked variant a control paints through: a checkbox's value or a
/// button's selection. Other controls have none.
pub(in crate::world::systems::gui) fn control_variant(
    world: &WorldSimulationState,
    control: GuiControl,
) -> Option<GuiPartVariant> {
    let index = control.target.entity.index() as usize;
    let checked = match control.kind {
        GuiControlKind::Checkbox => world.components.gui_checkbox(index)?.checked,
        GuiControlKind::Button => world.components.gui_button(index)?.selected,
        _ => return None,
    };
    Some(if checked {
        GuiPartVariant::Checked
    } else {
        GuiPartVariant::Unchecked
    })
}

/// Whether the theme or the default look of `kind` styles an unchecked
/// indicator in `state`, which then paints while the checkbox is unchecked.
pub(in crate::world::systems::gui) fn explicit_unchecked(
    theme: Option<&GuiTheme>,
    kind: GuiControlKind,
    state: GuiSkinState,
) -> bool {
    let identity = GuiPartId::variant(GuiPrimitivePart::Icon, state, GuiPartVariant::Unchecked);
    let styled =
        |row: &GuiPaintPart| row.color.is_some() || row.opacity.is_some() || row.asset.is_some();
    theme
        .and_then(|theme| part_row(theme.parts.iter().map(|(_, row)| row), identity))
        .is_some_and(styled)
        || default_look(kind).row(identity).is_some_and(styled)
}

/// A control's primitives. `focus` is its focused part while it shows the
/// focus ring, which paints on that part; `part_interaction` is the pointer
/// feedback of its scroll bar parts, its focus parts and a numeric input's
/// step parts.
#[allow(clippy::too_many_arguments)]
pub(in crate::world::systems) fn control_paint(
    context: &SystemRuntimeAccess<'_>,
    painted: &GuiPaintedControl,
    focus: Option<u32>,
    native: Option<&super::super::local::GuiNativeTextState>,
    style: CanvasPrimitiveStyle,
    layout: &super::super::layout::GuiEntityLayout,
    label: Option<&GuiControlLabel>,
    scroll_bars: &[super::super::layout::scroll_bars::GuiScrollBar],
    part_interaction: super::super::local::GuiPartInteraction,
    retained: impl Fn(CanvasPrimitiveId) -> Option<AssetKey>,
) -> Option<Vec<CanvasPrimitive>> {
    let size = layout.size;
    let entity = painted.control.target.entity;
    let index = entity.index() as usize;
    let kind = painted.control.kind;
    let enabled = painted.eligibility.enabled;
    let (skin, theme) = skin(context.world, entity).ok()?;
    if !painted.eligibility.available || !painted.eligibility.visible {
        return Some(Vec::new());
    }
    // A numeric input's step parts take the pointers over them, and its field
    // only the pointers over the rest of it.
    let number = (kind == GuiControlKind::TextInput)
        .then(|| context.world.components.gui_text_input(index))
        .flatten()
        .filter(|input| input.shows_step_parts());
    let body = match number {
        Some(_) => part_interaction.body,
        None => painted.interaction,
    };
    let state = GuiSkinState::resolve(!enabled, body.pressed, body.hovered);
    let variant = control_variant(context.world, painted.control);
    let look = control_look(context.world, painted.control);
    let font_size = label.map_or(GUI_DEFAULT_FONT_SIZE, |label| label.font_size);
    // A part in transition paints its sample instead of its resolved appearance.
    let motion = context
        .world
        .components
        .gui_behavior(index)
        .map(|behavior| &behavior.motion);
    let resolve = |part| {
        let mut style = appearance(skin, theme, look, font_size, part, state, variant);
        if let Some(motion) = motion {
            motion.appearance(part as u32, &mut style);
        }
        style
    };
    let paint_shape = |part, rect, appearance: &GuiPartStyle| {
        part_primitive(context, style, part, rect, appearance, &retained)
    };
    let whole = [0.0, 0.0, size[0], size[1]];
    let slider = (kind == GuiControlKind::Slider)
        .then(|| context.world.components.gui_slider(index))
        .flatten();
    let dial = slider
        .filter(|slider| slider.is_dial())
        .and_then(|_| slider_dial(whole));
    let rail = slider
        .filter(|slider| !slider.is_dial())
        .and_then(|slider| super::super::slider_rail(whole, slider.rail_axis()));
    // The rail is one scroll bar thick: every track shares a thickness.
    let rail_thickness = GUI_SCROLL_BAR_EMS * font_size;
    let ratio = slider.map_or(0.0, |slider| slider.fraction(slider.value));
    let origin = slider.map_or(0.0, |slider| slider.fraction(slider.origin));
    // Where each thumb lies: the value's, and a range's upper one.
    let thumb_ratio =
        |part: u32| slider.map_or(0.0, |slider| slider.fraction(slider.thumb_value(part)));
    let focus_part = focus.unwrap_or(0);
    let color = (kind == GuiControlKind::Color)
        .then(|| context.world.components.gui_color(index))
        .flatten();
    let color_layout = color.map(|color| GuiColorLayout::new(size, font_size, color.alpha_rail));

    // The Background is the whole control, a slider's rail or a labelled
    // checkbox's box; the focus ring follows a checkbox's box and a slider's
    // focused thumb. A dial's Background is its housing, the whole control,
    // which takes the ring.
    let (background_rect, focus_rect) = match kind {
        GuiControlKind::Slider => (
            rail.map_or(whole, |rail| rail.rail_rect(rail_thickness)),
            rail.and_then(|rail| rail.thumb_rect(thumb_ratio(focus_part)))
                .unwrap_or(whole),
        ),
        GuiControlKind::Checkbox => {
            let indicator = label.map_or(whole, |label| label.checkbox_box(size));
            (indicator, indicator)
        }
        GuiControlKind::Color => (
            whole,
            color_layout
                .and_then(|layout| layout.surface(focus_part))
                .unwrap_or(whole),
        ),
        _ => (whole, whole),
    };
    let mut primitives = Vec::new();
    let background = resolve(GuiPrimitivePart::Background);
    primitives.extend(paint_shape(
        CanvasPart::Background,
        background_rect,
        &background,
    ));
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
                // A bar that cannot scroll resolves the disabled state.
                for (part, id, rect, flags) in [
                    (
                        track_part,
                        track_id,
                        bar.track,
                        part_interaction.track[bar.axis],
                    ),
                    (
                        thumb_part,
                        thumb_id,
                        bar.thumb,
                        part_interaction.thumb[bar.axis],
                    ),
                ] {
                    let bar_state = GuiSkinState::resolve(
                        !enabled || !bar.enabled(),
                        flags.pressed,
                        flags.hovered,
                    );
                    let style = appearance(skin, theme, look, font_size, part, bar_state, variant);
                    primitives.extend(paint_shape(id, rect, &style));
                }
            }
        }
        GuiControlKind::Checkbox => {
            let icon = resolve(GuiPrimitivePart::Icon);
            if variant == Some(GuiPartVariant::Checked)
                || explicit_unchecked(theme, kind, state)
                || motion.is_some_and(|motion| motion.visible_part(GuiPrimitivePart::Icon))
            {
                let [x, y, width, height] = background_rect;
                let edge = width.min(height) * 0.5;
                let align = icon.align_x.unwrap_or(0.0).clamp(-1.0, 1.0);
                let center = x + width * 0.5 + (width - height).max(0.0) * align * 0.5;
                let scale = icon.scale.unwrap_or([1.0; 2]);
                primitives.extend(paint_shape(
                    CanvasPart::Icon,
                    [
                        center - edge * scale[0] * 0.5,
                        y + height * 0.5 - edge * scale[1] * 0.5,
                        edge,
                        edge,
                    ],
                    &icon,
                ));
            }
        }
        GuiControlKind::Slider => {
            let fill = resolve(GuiPrimitivePart::Fill);
            let icon = resolve(GuiPrimitivePart::Icon);
            let range = slider.is_some_and(|slider| slider.range);
            if let Some(rail) = rail {
                // A range fills between its thumbs and ignores the origin.
                let from = if range {
                    ratio
                } else {
                    origin
                };
                let to = if range {
                    thumb_ratio(1)
                } else {
                    ratio
                };
                if let Some(rect) = rail.fill_rect(from, to, rail_thickness) {
                    primitives.extend(paint_shape(CanvasPart::Fill, rect, &fill));
                }
                if range {
                    // Each thumb shows its own pointer feedback; the focused
                    // one paints last, above the other where they overlap.
                    let other = 1 - focus_part.min(1);
                    for part in [other, focus_part.min(1)] {
                        let flags = part_interaction.parts[part as usize];
                        let state = GuiSkinState::resolve(!enabled, flags.pressed, flags.hovered);
                        let mut thumb = appearance(
                            skin,
                            theme,
                            look,
                            font_size,
                            GuiPrimitivePart::Icon,
                            state,
                            variant,
                        );
                        if let Some(motion) = motion {
                            motion.appearance(
                                focus_part_channel(part, GuiPrimitivePart::Icon),
                                &mut thumb,
                            );
                        }
                        let id = match part {
                            0 => CanvasPart::Icon,
                            _ => CanvasPart::PartIcon(part as u8),
                        };
                        if let Some(rect) = rail.thumb_rect(thumb_ratio(part)) {
                            primitives.extend(paint_shape(id, rect, &thumb));
                        }
                    }
                } else if let Some(rect) = rail.thumb_rect(ratio) {
                    primitives.extend(paint_shape(CanvasPart::Icon, rect, &icon));
                }
            }
            if let Some(dial) = dial {
                let ticks = resolve(GuiPrimitivePart::Ticks);
                let track = resolve(GuiPrimitivePart::Track);
                for (part, rect, style) in
                    dial_parts(dial, font_size, origin, ratio, ticks, track, fill, icon)
                {
                    primitives.extend(paint_shape(part, rect, &style));
                }
            }
        }
        GuiControlKind::Color => {
            if let (Some(color), Some(layout)) = (color, color_layout) {
                // Each surface and its marker or thumb resolve the pointer
                // state of their focus part and move on its channels.
                let part_style = |part: u32, primitive| {
                    let flags = part_interaction.parts[part as usize];
                    let state = GuiSkinState::resolve(!enabled, flags.pressed, flags.hovered);
                    let mut style =
                        appearance(skin, theme, look, font_size, primitive, state, variant);
                    if let Some(motion) = motion {
                        motion.appearance(focus_part_channel(part, primitive), &mut style);
                    }
                    style
                };
                let swatch = resolve(GuiPrimitivePart::Fill);
                for (part, rect, style) in color_parts(color, &layout, part_style, swatch) {
                    primitives.extend(paint_shape(part, rect, &style));
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

            // Each step part is a division of the field with its centred
            // mark, both in the part's own state: disabled while its
            // direction is at its bound.
            if let Some(input) = number {
                let stepping = input.step_enabled();
                let rects = super::super::local::controls::number::number_step_rects(size);
                for (index, (cell, mark, cell_id, mark_id)) in [
                    (
                        GuiPrimitivePart::Decrement,
                        GuiPrimitivePart::DecrementMark,
                        CanvasPart::Decrement,
                        CanvasPart::DecrementMark,
                    ),
                    (
                        GuiPrimitivePart::Increment,
                        GuiPrimitivePart::IncrementMark,
                        CanvasPart::Increment,
                        CanvasPart::IncrementMark,
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let flags = part_interaction.steps[index];
                    let state = GuiSkinState::resolve(
                        !enabled || !stepping[index],
                        flags.pressed,
                        flags.hovered,
                    );
                    let resolve = |part: GuiPrimitivePart| {
                        let mut style =
                            appearance(skin, theme, look, font_size, part, state, variant);
                        if let Some(motion) = motion {
                            motion.appearance(part as u32, &mut style);
                        }
                        style
                    };
                    let rect = rects[index];
                    primitives.extend(paint_shape(cell_id, rect, &resolve(cell)));
                    let side = (STEP_MARK * font_size).min(rect[2]).min(rect[3]);
                    primitives.extend(paint_shape(
                        mark_id,
                        [
                            rect[0] + (rect[2] - side) * 0.5,
                            rect[1] + (rect[3] - side) * 0.5,
                            side,
                            side,
                        ],
                        &resolve(mark),
                    ));
                }
            }
        }
    }
    if let Some(native) = native
        && let Some(label) = label
        && let Some(layout) = &label.layout
    {
        let origin = label.origin();
        let selection = native.display_selection();
        let appearance = resolve(GuiPrimitivePart::Selection);
        for rect in layout.selection_rects(selection[0], selection[1]) {
            let mut rect = rect.map(|value| value * label.font_size);
            rect[0] += origin[0];
            rect[1] += origin[1];
            primitives.extend(paint_shape(CanvasPart::Selection, rect, &appearance));
        }
        if let Some(caret) = layout.caret_position(selection[1]) {
            let appearance = resolve(GuiPrimitivePart::Caret);
            primitives.extend(paint_shape(
                CanvasPart::Caret,
                [
                    origin[0] + caret.position[0] * label.font_size,
                    origin[1] + caret.position[1] * label.font_size,
                    label.font_size * TEXT_MARK,
                    caret.height * label.font_size,
                ],
                &appearance,
            ));
        }
        if let Some(composition) = &native.composition {
            let start = native.selection[0].min(native.selection[1]);
            let appearance = resolve(GuiPrimitivePart::Composition);
            for rect in layout.selection_rects(start, start + composition.text.len() as u32) {
                primitives.extend(paint_shape(
                    CanvasPart::Composition,
                    [
                        origin[0] + rect[0] * label.font_size,
                        origin[1] + (rect[1] + rect[3] - TEXT_MARK) * label.font_size,
                        rect[2] * label.font_size,
                        TEXT_MARK * label.font_size,
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
        let mut label_style = part_style(style, CanvasPart::Label, label.origin(), &appearance);
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
        && (focus.is_some()
            || motion.is_some_and(|motion| motion.visible_part(GuiPrimitivePart::FocusRing)))
    {
        // The ring is an outline: its colour styles the border unless a border
        // colour does, and its interior stays clear unless an asset replaces it.
        let mut focus = resolve(GuiPrimitivePart::FocusRing);
        focus.border_color = focus.border_color.or(focus.color);
        if focus.asset.is_none() {
            focus.color = Some([0.0; 4]);
        }
        primitives.extend(paint_shape(CanvasPart::FocusRing, focus_rect, &focus));
    }
    Some(primitives)
}

/// A colour control's parts in paint order, each with its rectangle and its
/// appearance with the fill its value gives: the saturation-value field of the
/// hue, the hue rail and the alpha rail from transparent to the opaque colour,
/// rising from their bottom edges, the swatch, then the marker and the thumbs
/// above every surface.
///
/// `part_style` resolves a focus part's surface (Track), marker or thumb in
/// that part's pointer state; `swatch` is the resolved Fill. The alpha rail
/// shows its translucent colours over the surfaces' checker and the swatch
/// shows the colour at its alpha over its own while the alpha rail is shown,
/// and opaque otherwise. The field and the hue rail are opaque and paint no
/// checker, so a dimmed surface shows what lies beneath the control rather
/// than a checker that no colour of theirs covers. The surfaces' and the swatch's colours come from
/// the HSV model on sRGB-encoded values ([`hsv_to_linear`]), which the colour
/// fields evaluate per fragment, so the swatch's pixel is the colour under the
/// marker and the colour a client reads from the fields. An inherited tint or
/// opacity still multiplies them, as it does every fill.
fn color_parts(
    color: &GuiColor,
    layout: &GuiColorLayout,
    part_style: impl Fn(u32, GuiPrimitivePart) -> GuiPartStyle,
    mut swatch: GuiPartStyle,
) -> Vec<(CanvasPart, [f32; 4], GuiPartStyle)> {
    use GuiPrimitivePart::{Icon, Marker, Track};

    let [red, green, blue] = hsv_to_linear(color.hue, color.saturation, color.value);
    let rail_axis = |style: &mut GuiPartStyle, rect: [f32; 4]| {
        style.gradient_start = Some([0.0, rect[3]]);
        style.gradient_end = Some([0.0, 0.0]);
    };
    let mut parts = Vec::with_capacity(8);

    let mut field = part_style(0, Track);
    field.fill_mode = Some(4.0);
    field.fill_hue = Some(color.hue);
    field.checker_size = None;
    parts.push((CanvasPart::Track, layout.field, field));

    let mut hue = part_style(1, Track);
    hue.fill_mode = Some(3.0);
    hue.checker_size = None;
    rail_axis(&mut hue, layout.hue);
    parts.push((CanvasPart::PartTrack(1), layout.hue, hue));

    if let Some(rect) = layout.alpha {
        let mut alpha = part_style(2, Track);
        alpha.fill_mode = Some(1.0);
        rail_axis(&mut alpha, rect);
        alpha.gradient_color0 = Some([red, green, blue, 0.0]);
        alpha.gradient_color1 = Some([red, green, blue, 1.0]);
        parts.push((CanvasPart::PartTrack(2), rect, alpha));
    }

    let opacity = if layout.alpha.is_some() {
        color.alpha
    } else {
        1.0
    };
    swatch.fill_mode = Some(0.0);
    swatch.color = Some([red, green, blue, opacity]);
    parts.push((CanvasPart::Fill, layout.swatch, swatch));

    parts.push((
        CanvasPart::Marker,
        layout.marker_rect(color.saturation, color.value),
        part_style(0, Marker),
    ));
    parts.push((
        CanvasPart::PartIcon(1),
        layout.thumb_rect(layout.hue, color.hue),
        part_style(1, Icon),
    ));
    if let Some(rect) = layout.alpha {
        parts.push((
            CanvasPart::PartIcon(2),
            layout.thumb_rect(rect, color.alpha),
            part_style(2, Icon),
        ));
    }
    parts
}

/// A dial's parts inside its housing in paint order, each with its rectangle
/// and its resolved appearance placed on the dial: the tick ring, the track
/// and value arcs and the pointer.
///
/// Geometry follows the value and the parts' thicknesses
/// ([`GuiSliderDial`]); the appearance supplies everything else. The arcs take
/// their start and sweep from the dial and the value, and the pointer's
/// strokes, authored at twelve o'clock in the square of the value ring, turn
/// about its centre to the value's angle. The tick ring starts half a dash
/// cell before the sweep and ends half a cell after it, so a whole number of
/// cells over the sweep puts a tick on each end.
#[allow(clippy::too_many_arguments)]
fn dial_parts(
    dial: GuiSliderDial,
    font_size: f32,
    origin: f32,
    fraction: f32,
    mut ticks: GuiPartStyle,
    mut track: GuiPartStyle,
    mut fill: GuiPartStyle,
    mut pointer: GuiPartStyle,
) -> Vec<(CanvasPart, [f32; 4], GuiPartStyle)> {
    let width = |style: &GuiPartStyle| style.border_width.unwrap_or(0.0).max(0.0);
    let ring = dial.ring_radius(font_size, width(&ticks), width(&fill));
    let [start, sweep] = GuiSliderDial::sweep();
    let cell = match ticks.arc_dashes {
        Some([cells, _]) if cells > 0.0 => 1.0 / cells,
        _ => 0.0,
    };
    let mut parts = Vec::with_capacity(4);
    ticks.arc_start = Some(start - cell * 0.5);
    ticks.arc_sweep = Some(sweep + cell);
    parts.push((
        CanvasPart::Ticks,
        dial.square(dial.ticks_radius(font_size)),
        ticks,
    ));
    track.arc_start = Some(start);
    track.arc_sweep = Some(sweep);
    parts.push((
        CanvasPart::Track,
        dial.square(ring + width(&track) * 0.5),
        track,
    ));
    if let Some([start, sweep]) = GuiSliderDial::value_arc(origin, fraction) {
        fill.arc_start = Some(start);
        fill.arc_sweep = Some(sweep);
        parts.push((
            CanvasPart::Fill,
            dial.square(ring + width(&fill) * 0.5),
            fill,
        ));
    }
    let turn = GuiSliderDial::angle(fraction) * std::f32::consts::TAU;
    let (sin, cos) = turn.sin_cos();
    let rotate = |[x, y]: [f32; 2]| {
        let [dx, dy] = [x - 0.5, y - 0.5];
        [
            (0.5 + dx * cos - dy * sin).clamp(0.0, 1.0),
            (0.5 + dx * sin + dy * cos).clamp(0.0, 1.0),
        ]
    };
    for stroke in [&mut pointer.stroke_a, &mut pointer.stroke_b] {
        if let Some([x0, y0, x1, y1]) = *stroke {
            let ([x0, y0], [x1, y1]) = (rotate([x0, y0]), rotate([x1, y1]));
            *stroke = Some([x0, y0, x1, y1]);
        }
    }
    parts.push((CanvasPart::Icon, dial.square(ring), pointer));
    parts
}

/// Background of a skinned entity that is not a control, over its box at `style`.
///
/// The part resolves from the skin's own override row, then the theme's
/// unqualified base row; with no theme the skin rows alone style the entity. The
/// entity has no interaction state, so no state- or variant-qualified row applies,
/// and no control default fills in: an absent property keeps the box primitive's
/// neutral value, such as a white fill under the inherited tint. A ready drawing or
/// bitmap asset replaces the box as it does for a control. Other parts are not
/// painted. None while the theme reference is broken or the asset is not ready.
pub(in crate::world::systems) fn skinned_background(
    context: &SystemRuntimeAccess<'_>,
    entity: EntityId,
    style: CanvasPrimitiveStyle,
    size: [f32; 2],
    retained: impl Fn(CanvasPrimitiveId) -> Option<AssetKey>,
) -> Option<CanvasPrimitive> {
    let (skin, theme) = skin(context.world, entity).ok()?;
    let part = GuiPrimitivePart::Background;
    let font_size = theme
        .filter(|theme| theme.em > 0.0)
        .map_or(GUI_DEFAULT_FONT_SIZE, |_| {
            inherited_font_size(context.world, entity)
        });
    let appearance = resolve_rows(
        skin,
        theme,
        None,
        font_size,
        part,
        std::iter::once(GuiPartId::base(part)),
    );

    part_primitive(
        context,
        style,
        CanvasPart::Background,
        [0.0, 0.0, size[0], size[1]],
        &appearance,
        &retained,
    )
}

/// One part's box over `rect`, or the ready skin asset that replaces it; a pending
/// replacement keeps the asset `retained` under the same primitive identity.
fn part_primitive(
    context: &SystemRuntimeAccess<'_>,
    style: CanvasPrimitiveStyle,
    part: CanvasPart,
    rect: [f32; 4],
    appearance: &GuiPartStyle,
    retained: &impl Fn(CanvasPrimitiveId) -> Option<AssetKey>,
) -> Option<CanvasPrimitive> {
    let primitive = shape(style, part, rect, appearance);
    let previous = retained(primitive.style().identity);
    apply_asset(context, primitive, appearance, previous)
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
        Some(3.0) => CanvasShapeFill::Hue {
            start: appearance.gradient_start.unwrap_or([0.0; 2]),
            end: appearance.gradient_end.unwrap_or([1.0; 2]),
        },
        Some(4.0) => CanvasShapeFill::SaturationValue {
            hue: appearance.fill_hue.unwrap_or(0.0),
        },
        _ => CanvasShapeFill::Solid(color),
    };
    // Either glow side alone makes the glow; an absent side has no reach.
    let radius = appearance.glow_radius.unwrap_or(0.0);
    let inner_radius = appearance.glow_inner_radius.unwrap_or(0.0);
    let glow = (appearance.glow_intensity.is_some_and(|value| value > 0.0)
        && (radius > 0.0 || inner_radius > 0.0))
        .then(|| CanvasShapeGlow {
            color: appearance.glow_color.unwrap_or([1.0; 4]),
            intensity: appearance.glow_intensity.unwrap(),
            radius,
            inner_radius,
            falloff: appearance.glow_falloff.unwrap_or(1.0),
        });
    // Like the fill mode, only an exact stroke or arc value selects that shape, so a
    // shape lane between values paints the box. An arc without a sweep is the whole
    // ring and without dashes solid. Spans without their own width keep the
    // ordinary border width; without spans the width has nothing to apply to.
    let border_width = appearance.border_width.unwrap_or(0.0);
    let shape = match appearance.shape {
        Some(1.0) => CanvasBoxShape::Stroke {
            segments: [
                appearance.stroke_a.unwrap_or([0.0; 4]),
                appearance.stroke_b.unwrap_or([0.0; 4]),
            ],
        },
        Some(2.0) => {
            let [dashes, dash_duty] = appearance.arc_dashes.unwrap_or([0.0, 1.0]);
            CanvasBoxShape::Arc {
                start: appearance.arc_start.unwrap_or(0.0),
                sweep: appearance.arc_sweep.unwrap_or(1.0),
                dashes,
                dash_duty,
            }
        }
        _ => {
            let corner_accent = appearance.corner_accent.unwrap_or([0.0; 4]);
            let accented = corner_accent.iter().any(|span| *span > 0.0);
            CanvasBoxShape::Rect {
                corner_cut: appearance.corner_cut.unwrap_or([0.0; 4]),
                corner_accent,
                corner_accent_width: if accented {
                    appearance.corner_accent_width.unwrap_or(border_width)
                } else {
                    0.0
                },
                checker: appearance
                    .checker_size
                    .filter(|size| *size > 0.0)
                    .map(|size| CanvasShapeChecker {
                        size,
                        colors: [
                            appearance.checker_color0.unwrap_or(CHECKER_LIGHT),
                            appearance.checker_color1.unwrap_or(CHECKER_DARK),
                        ],
                    }),
            }
        }
    };
    CanvasPrimitive::Box {
        style: part_style(style, part, [rect[0], rect[1]], appearance),
        size: [rect[2], rect[3]],
        corner_radius: appearance.corner_radius.unwrap_or([0.0; 2]),
        border_width,
        border_color: appearance.border_color.unwrap_or([0.0; 4]),
        fill,
        glow,
        shape,
    }
}
