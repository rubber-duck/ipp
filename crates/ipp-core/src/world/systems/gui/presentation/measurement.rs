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
use crate::systems::gui::local::GuiControlKind;
use crate::systems::gui::local::GuiEntityTarget;
use crate::systems::gui::local::control::entity_control;
use crate::systems::surface::{
    TextFont, TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome, measure_text,
};
use std::sync::Arc;

#[derive(Clone)]
pub(in crate::world::systems) struct GuiControlLabel {
    target: GuiEntityTarget,
    text: Arc<str>,
    font: Option<AssetKey>,
    pub font_size: f32,
    pub geometry: Option<CanvasGeometry>,
    pub layout: Option<Arc<crate::systems::surface::TextLayout>>,
    pub intrinsic: [f32; 2],
}

impl GuiControlLabel {
    /// Whether this measurement is of `text`: the same reference or content.
    pub(in crate::world::systems::gui) fn measures(&self, text: &Arc<str>) -> bool {
        same_text(&self.text, text)
    }
}

/// The text a control's label displays: its native record while one is
/// active, otherwise its label, text or placeholder field.
pub(in crate::world::systems::gui) fn label_text(
    context: &SystemRuntimeAccess<'_>,
    gui: &GuiSystem,
    entity: EntityId,
) -> Option<Arc<str>> {
    let world = &*context.world;
    let control = entity_control(world, &world.state, entity)?;
    Some(control_text(world, gui, control.target, control.kind))
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
            if control.text.is_empty() {
                control.placeholder.clone()
            } else {
                control.text.clone()
            }
        }),
        GuiControlKind::Slider | GuiControlKind::ScrollView | GuiControlKind::VirtualList => None,
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
    let font_size = typography.map_or(14.0, |font| font.font_size);
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
            && same_text(&previous.text, &text)
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
            text_size[1].max(1.4 * font_size),
        ],
        GuiControlKind::Checkbox => [1.4 * font_size, 1.4 * font_size],
        GuiControlKind::Slider => [8.0 * font_size, 1.4 * font_size],
        GuiControlKind::ScrollView | GuiControlKind::VirtualList => [0.0; 2],
        GuiControlKind::TextInput => [
            text_size[0].max(8.0 * font_size),
            text_size[1].max(1.4 * font_size),
        ],
    };
    let measured = geometry.is_some();
    Some((
        GuiControlLabel {
            target,
            text,
            font,
            font_size,
            geometry,
            layout,
            intrinsic,
        },
        measured,
    ))
}
