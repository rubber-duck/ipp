use super::GuiFont;
use crate::EntityId;
use crate::components::schema::same_text;
use crate::services::asset_management::{
    AssetKey,
    font::{FONT_TYPE, FontAsset},
};
use crate::systems::SystemRuntimeAccess;
use crate::systems::canvas::{CanvasGeometry, CanvasGlyph};
use crate::systems::gui::GuiSystem;
use crate::systems::gui::local::GuiEntityTarget;
use crate::systems::gui::local::control::entity_control;
use crate::systems::gui::local::{GUI_SLIDER_DIAL, GuiControlKind};
use crate::systems::surface::{
    TextFont, TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome, measure_text,
};
use std::sync::Arc;

/// Heights of unsized controls in ems of their font, from the design
/// language's sizes at its 16-unit body type: full-size controls (buttons and
/// text fields) 40, small controls (the checkbox box) 32, and a slider deep
/// enough across its rail for its 16-unit thumb, which is three quarters of
/// that depth.
pub(in crate::world::systems::gui) const CONTROL_HEIGHT: f32 = 2.5;
pub(in crate::world::systems::gui) const SMALL_HEIGHT: f32 = 2.0;
const SLIDER_DEPTH: f32 = 4.0 / 3.0;

/// Length of an unsized slider along its rail, in ems.
const SLIDER_LENGTH: f32 = 8.0;

/// Side of an unsized dial, which is square, in ems: 80 at the language's
/// 16-unit body type, room for its tick ring half an em inside the housing
/// around a value ring 44 across.
pub(in crate::world::systems::gui) const DIAL_SIDE: f32 = 5.0;

/// Gap between a checkbox's box and its label, in ems.
const CHECKBOX_LABEL_GAP: f32 = 0.5;

/// Inset of a text input's line from the left of its content box, in ems.
pub(in crate::world::systems::gui) const TEXT_INPUT_INSET: f32 = 1.0;

/// A control's measured label line and where it sits in the control.
///
/// Layout measures the label and then records the control's content box, the
/// laid-out size within its padding. The label line sits in that box by the
/// control's role: a button centres it on both axes, a text input insets it one
/// em from the left and centres it vertically, or with step parts centres it
/// between them, and a labelled checkbox places it half an em right of its
/// box, centred vertically. Paint, the caret, the
/// selection and pointer text hits all use the same origin.
#[derive(Clone)]
pub(in crate::world::systems) struct GuiControlLabel {
    target: GuiEntityTarget,
    kind: GuiControlKind,
    text: Arc<str>,
    font: Option<AssetKey>,
    pub font_size: f32,
    pub geometry: Option<CanvasGeometry>,
    pub layout: Option<Arc<crate::systems::surface::TextLayout>>,
    pub intrinsic: [f32; 2],
    /// A control's presentation, which sets its intrinsic size: a slider's
    /// `axis` field, 1 for a colour control with its alpha rail; 0 for other
    /// controls.
    axis: usize,
    /// Whether a numeric text input shows its step parts, between which its
    /// line is centred.
    steps: bool,
    /// Content box `[x, y, width, height]` in control-local logical units.
    pub content: [f32; 4],
    /// The control's laid-out size.
    size: [f32; 2],
}

impl GuiControlLabel {
    /// Whether this measurement is of `text` (the same reference or content),
    /// of a slider presented as `axis` and of a text input with or without
    /// `steps`.
    pub(in crate::world::systems::gui) fn measures(
        &self,
        text: &Arc<str>,
        axis: usize,
        steps: bool,
    ) -> bool {
        same_text(&self.text, text) && self.axis == axis && self.steps == steps
    }

    /// Record the content box of a control of `size` with `padding`
    /// `[top, right, bottom, left]`.
    pub(in crate::world::systems::gui) fn place(&mut self, size: [f32; 2], padding: [f32; 4]) {
        self.size = size;
        self.content = [
            padding[3],
            padding[0],
            (size[0] - padding[1] - padding[3]).max(0.0),
            (size[1] - padding[0] - padding[2]).max(0.0),
        ];
    }

    /// Measured line size in logical units.
    fn line(&self) -> [f32; 2] {
        self.layout.as_ref().map_or([0.0; 2], |layout| {
            layout.size.map(|value| value * self.font_size)
        })
    }

    /// Control-local origin of the label line's top-left corner.
    pub(in crate::world::systems) fn origin(&self) -> [f32; 2] {
        let [x, y, width, height] = self.content;
        let line = self.line();
        let centred = y + (height - line[1]) * 0.5;
        match self.kind {
            GuiControlKind::Button => [x + (width - line[0]) * 0.5, centred],
            // Between a numeric input's step parts, the line is centred.
            GuiControlKind::TextInput if self.steps => {
                let [decrement, increment] =
                    super::super::local::number::number_step_rects(self.size);
                let start = decrement[0] + decrement[2];
                [start + (increment[0] - start - line[0]) * 0.5, centred]
            }
            GuiControlKind::TextInput => [x + TEXT_INPUT_INSET * self.font_size, centred],
            GuiControlKind::Checkbox if self.labelled() => {
                [x + height + CHECKBOX_LABEL_GAP * self.font_size, centred]
            }
            _ => [x, y],
        }
    }

    /// Whether a checkbox shows a label beside its box.
    fn labelled(&self) -> bool {
        !self.text.is_empty()
    }

    /// A checkbox's box in a control of `size`: the square of its content
    /// height at the left of its content box while it shows a label, and the
    /// whole control otherwise.
    pub(in crate::world::systems::gui) fn checkbox_box(&self, size: [f32; 2]) -> [f32; 4] {
        let [x, y, _, height] = self.content;
        if self.labelled() {
            [x, y, height, height]
        } else {
            [0.0, 0.0, size[0], size[1]]
        }
    }
}

/// What a control's measurement reads besides its typography: the text its
/// label displays, which is its native record while one is active and
/// otherwise its label, text or placeholder field or its formatted number, its
/// presentation and whether a text input shows step parts.
pub(in crate::world::systems::gui) fn label_inputs(
    context: &SystemRuntimeAccess<'_>,
    gui: &GuiSystem,
    entity: EntityId,
) -> Option<(Arc<str>, usize, bool)> {
    let world = &*context.world;
    let control = entity_control(world, &world.state, entity)?;
    Some((
        control_text(world, gui, control.target, control.kind),
        control_axis(world, entity, control.kind),
        control_steps(world, entity, control.kind),
    ))
}

/// Whether a numeric text input shows its step parts.
fn control_steps(
    world: &crate::world::WorldSimulationState,
    entity: EntityId,
    kind: GuiControlKind,
) -> bool {
    kind == GuiControlKind::TextInput
        && world
            .components
            .gui_text_input(entity.index() as usize)
            .is_some_and(super::super::local::GuiTextInput::shows_step_parts)
}

/// A control's presentation: a slider's `axis` field, 1 for a colour control
/// with its alpha rail; 0 for other controls.
fn control_axis(
    world: &crate::world::WorldSimulationState,
    entity: EntityId,
    kind: GuiControlKind,
) -> usize {
    let index = entity.index() as usize;
    match kind {
        GuiControlKind::Slider => world
            .components
            .gui_slider(index)
            .map_or(0, |slider| slider.axis as usize),
        GuiControlKind::Color => world
            .components
            .gui_color(index)
            .map_or(0, |color| usize::from(color.alpha_rail)),
        _ => 0,
    }
}

fn control_text(
    world: &crate::world::WorldSimulationState,
    gui: &GuiSystem,
    target: GuiEntityTarget,
    kind: GuiControlKind,
) -> Arc<str> {
    if let Some(state) = gui.native_text_state(target) {
        return state.display_text();
    }
    let index = target.entity.index() as usize;
    match kind {
        GuiControlKind::Button => world
            .components
            .gui_button(index)
            .map(|control| control.label.clone()),
        GuiControlKind::Checkbox => world
            .components
            .gui_checkbox(index)
            .map(|control| control.label.clone()),
        GuiControlKind::TextInput => world.components.gui_text_input(index).map(|control| {
            if control.numeric {
                control.formatted()
            } else if control.text.is_empty() {
                control.placeholder.clone()
            } else {
                control.text.clone()
            }
        }),
        GuiControlKind::Slider
        | GuiControlKind::ScrollView
        | GuiControlKind::VirtualList
        | GuiControlKind::Color => None,
    }
    .unwrap_or_default()
}

/// Measure a control's label from its fields and the typography it inherits,
/// reusing the previous measurement when text, font and size are unchanged.
pub(in crate::world::systems::gui) fn measure_control(
    context: &SystemRuntimeAccess<'_>,
    gui: &GuiSystem,
    entity: EntityId,
    typography: Option<&GuiFont>,
    previous: Option<&GuiControlLabel>,
) -> Option<(GuiControlLabel, bool)> {
    let world = &*context.world;
    let control = entity_control(world, &world.state, entity)?;
    let (target, kind) = (control.target, control.kind);
    let text = control_text(world, gui, target, kind);
    let axis = control_axis(world, entity, kind);
    let steps = control_steps(world, entity, kind);
    let font_size = typography.map_or(super::GUI_DEFAULT_FONT_SIZE, |font| font.font_size);
    let assets = context.asset_resources();
    let font = typography
        .filter(|font| !font.source.is_empty())
        .and_then(|font| {
            assets
                .find_source(world.id, FONT_TYPE, &font.source, font.variant)
                .filter(|key| assets.get_typed::<FontAsset>(*key).is_some())
                .or_else(|| {
                    previous
                        .filter(|previous| previous.target == target)
                        .and_then(|previous| previous.font)
                        .filter(|key| assets.get_typed::<FontAsset>(*key).is_some())
                })
        });
    if let Some(previous) = previous.filter(|previous| {
        previous.target == target
            && previous.measures(&text, axis, steps)
            && previous.font == font
            && previous.font_size == font_size
    }) {
        return Some((previous.clone(), false));
    }
    let layout = font.and_then(|key| {
        let request = TextMeasureRequest::new(
            &text,
            TextFont::Ready {
                key,
                font: assets.get_typed::<FontAsset>(key)?,
            },
            font_size,
            TextLinePolicy::SingleLine,
            TextMaxWidth::Unbounded,
        )
        .ok()?;
        let TextOutcome::Measured(layout) = measure_text(&request) else {
            return None;
        };
        Some(Arc::new(layout))
    });
    let geometry = layout.as_ref().and_then(|layout| {
        Some(CanvasGeometry::Glyphs {
            font: font?,
            font_size,
            size: layout.size.map(|value| value * font_size),
            glyphs: layout
                .glyphs
                .iter()
                .map(|glyph| CanvasGlyph {
                    glyph_id: glyph.glyph_id,
                    position: glyph.position.map(|value| value * font_size),
                    color: None,
                })
                .collect::<Vec<_>>()
                .into(),
        })
    });
    let text_size = match &geometry {
        Some(CanvasGeometry::Glyphs {
            size,
            ..
        }) => *size,
        _ => [0.0; 2],
    };
    let intrinsic = match kind {
        GuiControlKind::Button => [
            text_size[0].max(2.0 * font_size),
            text_size[1].max(CONTROL_HEIGHT * font_size),
        ],
        GuiControlKind::Checkbox if !text.is_empty() => [
            (SMALL_HEIGHT + CHECKBOX_LABEL_GAP) * font_size + text_size[0],
            text_size[1].max(SMALL_HEIGHT * font_size),
        ],
        GuiControlKind::Checkbox => [SMALL_HEIGHT * font_size, SMALL_HEIGHT * font_size],
        GuiControlKind::Slider if axis == GUI_SLIDER_DIAL as usize => [DIAL_SIDE * font_size; 2],
        GuiControlKind::Slider => {
            let mut size = [SLIDER_LENGTH * font_size; 2];
            size[1 - axis] = SLIDER_DEPTH * font_size;
            size
        }
        GuiControlKind::Color => {
            crate::systems::gui::local::color::intrinsic_size(font_size, axis == 1)
        }
        GuiControlKind::ScrollView | GuiControlKind::VirtualList => [0.0; 2],
        // Step parts add a square of the control's height at each end.
        GuiControlKind::TextInput => [
            text_size[0].max(8.0 * font_size)
                + if steps {
                    2.0 * CONTROL_HEIGHT * font_size
                } else {
                    0.0
                },
            text_size[1].max(CONTROL_HEIGHT * font_size),
        ],
    };
    let measured = geometry.is_some();
    Some((
        GuiControlLabel {
            target,
            kind,
            text,
            font,
            font_size,
            geometry,
            layout,
            intrinsic,
            axis,
            steps,
            content: [0.0; 4],
            size: [0.0; 2],
        },
        measured,
    ))
}
