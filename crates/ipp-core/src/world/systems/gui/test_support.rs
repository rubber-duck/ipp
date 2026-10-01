//! Font fixture and control reads shared by ordinary GUI lane tests.

use super::local::control::{ancestry, eligibility, entity_control};
use super::local::{GuiControlKind, GuiEntityTarget, GuiInteractionFlags};
use crate::EntityId;
use crate::services::asset_management::font::FontAsset;
use std::sync::Arc;

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Encoded layout/input fixture font: 1000 units per em with stable Latin
/// advances used by the independent geometry expectations in each test suite.
pub(super) fn font_fixture_bytes() -> Vec<u8> {
    let mut bytes = b"IPPF".to_vec();
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 1000);
    push_f32(&mut bytes, 800.0);
    push_f32(&mut bytes, -200.0);
    push_f32(&mut bytes, 200.0);

    let advances = [500.0_f32, 600.0, 650.0, 550.0, 300.0, 550.0];
    push_u32(&mut bytes, advances.len() as u32);
    push_u32(&mut bytes, 5);
    push_u32(&mut bytes, 1);

    for advance in advances {
        push_f32(&mut bytes, advance);
        push_f32(&mut bytes, 0.0);

        for bound in [0.0_f32, 0.0, 0.0, 0.0] {
            push_f32(&mut bytes, bound);
        }

        push_u32(&mut bytes, 0);
    }

    for (codepoint, glyph_id) in [
        (0x20_u32, 4_u32),
        (0x41, 1),
        (0x56, 2),
        (0x61, 3),
        (0x65, 5),
    ] {
        push_u32(&mut bytes, codepoint);
        push_u32(&mut bytes, glyph_id);
    }

    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 2);
    push_f32(&mut bytes, -50.0);

    bytes
}

pub(super) fn test_font() -> FontAsset {
    FontAsset::decode(&font_fixture_bytes()).expect("GUI test font must decode")
}

/// A control's value field, read from the component store.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GuiTestValue {
    /// A button has no value.
    None,
    /// A checkbox's `checked`.
    Bool(bool),
    /// A slider's `value`.
    Scalar(f32),
    /// A text input's `text`.
    Text(Arc<str>),
    /// A scroll view's or virtual list's `offset_x` and `offset_y`.
    Scroll([f32; 2]),
}

/// One control read the way a client reads it: its value and eligibility from
/// component fields, focus and pointer feedback from the GUI System queries.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GuiControlRead {
    /// Exact control lifetime.
    pub target: GuiEntityTarget,
    /// Control role.
    pub kind: GuiControlKind,
    /// `GuiBehavior.effective_enabled`.
    pub enabled: bool,
    /// `GuiBehavior.effective_visible`.
    pub visible: bool,
    /// `GuiBehavior.available`.
    pub available: bool,
    /// Whether the `GuiFocus` query names this control.
    pub focused: bool,
    /// Union of this control's `GuiPointers` records.
    pub interaction: GuiInteractionFlags,
    /// The control's value field.
    pub value: GuiTestValue,
    /// `CanvasBounds` as `[x, y, width, height]`.
    pub bounds: [f32; 4],
    /// Root-first core ancestry, including the control.
    pub ancestry: Arc<[EntityId]>,
}

/// Read one control's fields and System query records, if `entity` is a control.
pub(crate) fn read_control(
    world: &crate::WorldContext<'_>,
    entity: EntityId,
) -> Option<GuiControlRead> {
    let simulation = &*world.world;
    let control = entity_control(simulation, &simulation.state, entity)?;
    let components = &simulation.components;
    let index = entity.index() as usize;
    let value = match control.kind {
        GuiControlKind::Button => GuiTestValue::None,
        GuiControlKind::Checkbox => GuiTestValue::Bool(components.gui_checkbox(index)?.checked),
        GuiControlKind::Slider => GuiTestValue::Scalar(components.gui_slider(index)?.value),
        GuiControlKind::TextInput => {
            GuiTestValue::Text(components.gui_text_input(index)?.text.clone())
        }
        GuiControlKind::ScrollView => {
            let scroll = components.gui_scroll_view(index)?;
            GuiTestValue::Scroll([scroll.offset_x, scroll.offset_y])
        }
        GuiControlKind::VirtualList => {
            let list = components.gui_virtual_list(index)?;
            GuiTestValue::Scroll([list.offset_x, list.offset_y])
        }
    };
    let bounds = components.canvas_bounds(index).map_or([0.0; 4], |bounds| {
        [bounds.x, bounds.y, bounds.width, bounds.height]
    });
    let eligibility = eligibility(simulation, entity);
    let focused = world
        .gui_focus_page(0, entity.to_bits(), 1)
        .first()
        .is_some_and(|record| record.target == control.target);
    let interaction = world
        .gui_pointer_page(0, entity.to_bits(), usize::MAX)
        .into_iter()
        .filter(|record| record.target == control.target)
        .fold(GuiInteractionFlags::default(), |flags, record| {
            GuiInteractionFlags {
                hovered: flags.hovered || record.state.hovered,
                pressed: flags.pressed || record.state.pressed,
                captured: flags.captured || record.state.captured,
            }
        });
    Some(GuiControlRead {
        target: control.target,
        kind: control.kind,
        enabled: eligibility.enabled,
        visible: eligibility.visible,
        available: eligibility.available,
        focused,
        interaction,
        value,
        bounds,
        ancestry: ancestry(&simulation.state, entity),
    })
}
