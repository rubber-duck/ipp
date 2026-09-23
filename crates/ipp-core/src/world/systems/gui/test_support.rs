use crate::services::asset_management::AssetSource;
use crate::services::asset_management::font::{FONT_TYPE, FontAsset};

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

pub(super) fn font_source() -> AssetSource {
    AssetSource {
        kind: FONT_TYPE,
        uri: "test-font".to_owned(),
        variant: 0,
    }
}

/// Authored node for fixtures: kind and strings plus the kind-specific
/// scalars an `InsertNode` seeds into the node's `node_data` row.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::world::systems) struct AuthoredNode {
    pub(in crate::world::systems) data: super::GuiNodeData,
    pub(in crate::world::systems) values: super::GuiNodeDataRow,
}

impl From<super::GuiNodeData> for AuthoredNode {
    fn from(data: super::GuiNodeData) -> Self {
        Self {
            data,
            values: super::GuiNodeDataRow::default(),
        }
    }
}

/// Slider node with its initial value and range.
pub(in crate::world::systems) fn slider_node(
    value: f32,
    min: f32,
    max: f32,
    step: f32,
) -> AuthoredNode {
    AuthoredNode {
        data: super::GuiNodeData::Slider,
        values: super::GuiNodeDataRow::slider(value, min, max, step),
    }
}

/// Checkbox node with its initial state.
pub(in crate::world::systems) fn checkbox_node(checked: bool) -> AuthoredNode {
    AuthoredNode {
        data: super::GuiNodeData::Checkbox,
        values: super::GuiNodeDataRow::checkbox(checked),
    }
}
