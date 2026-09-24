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

/// Part identity named like `background`, `icon_pressed` or
/// `icon_idle_unchecked`: a base part, an optional state and, under a
/// state, an optional variant.
pub(in crate::world::systems) fn part_id(name: &str) -> super::GuiPartId {
    use super::{GUI_BASE_PARTS, GuiPartId, GuiPartVariant, GuiSkinState};

    let mut words = name.split('_');
    let base = words.next().unwrap_or_default();
    let part = GUI_BASE_PARTS
        .into_iter()
        .find(|part| part.as_str() == base)
        .unwrap_or_else(|| panic!("unknown base part in {name}"));
    let state = words.next().map(|state| match state {
        "idle" => GuiSkinState::Idle,
        "hovered" => GuiSkinState::Hovered,
        "pressed" => GuiSkinState::Pressed,
        "disabled" => GuiSkinState::Disabled,
        other => panic!("unknown part state {other}"),
    });
    let variant = words.next().map(|variant| match variant {
        "checked" => GuiPartVariant::Checked,
        "unchecked" => GuiPartVariant::Unchecked,
        other => panic!("unknown part variant {other}"),
    });
    assert!(words.next().is_none(), "unknown part {name}");
    GuiPartId {
        part,
        state,
        variant,
    }
}

/// Part property by its row layout name.
pub(in crate::world::systems) fn part_property(name: &str) -> super::GuiPartProperty {
    super::GuiPartProperty::ALL
        .into_iter()
        .find(|property| property.name() == name)
        .unwrap_or_else(|| panic!("unknown part property {name}"))
}

/// Write one field of a root through the component registry, as staging
/// does, so the root re-derives its indexes and live channels.
fn write_field(
    root: &mut super::GuiRoot,
    offset: u32,
    value: crate::components::schema::FieldValue,
) {
    let mut component = crate::ComponentValue::GuiRoot(std::mem::take(root));
    component.set_field(offset, value).unwrap();
    let crate::ComponentValue::GuiRoot(written) = component else {
        unreachable!("GUI root writes keep the component type")
    };
    *root = written;
}

/// Apply GUI command writes to a root value as staging does.
pub(in crate::world::systems) fn apply_writes(
    root: &mut super::GuiRoot,
    writes: Vec<crate::FieldWrite>,
) {
    use crate::components::schema::FieldValue;

    for write in writes {
        let value = match write.value {
            crate::FieldValue::Dynamic(value) => FieldValue::Dynamic(value),
            crate::FieldValue::Unset => FieldValue::Unset,
            crate::FieldValue::Rows(bytes) => FieldValue::Rows(bytes),
            other => panic!("unexpected GUI write {other:?}"),
        };
        write_field(root, write.offset, value);
    }
}

/// Set or clear one property of one part of a root theme, creating the
/// theme on its first part.
pub(in crate::world::systems) fn set_theme_part(
    root: &mut super::GuiRoot,
    theme: u32,
    part: &str,
    property: &str,
    value: Option<crate::DynamicValue>,
) {
    let patch = match value {
        Some(value) => super::GuiPartPatch::default().set(part_property(property), value),
        None => super::GuiPartPatch::default().clear(part_property(property)),
    };
    let writes = root
        .theme_part_writes(theme, part_id(part), &patch)
        .unwrap();
    apply_writes(root, writes);
}

/// Point one live node at a theme handle, or clear its reference.
pub(in crate::world::systems) fn set_node_theme(
    root: &mut super::GuiRoot,
    node: super::GuiNodeId,
    theme: Option<u32>,
) {
    use crate::components::schema::FieldValue;

    let offset =
        super::GuiRoot::node_style_offset(node, super::GuiNodeStyleProperty::Theme).unwrap();
    let value = theme.map_or(FieldValue::Unset, |theme| {
        FieldValue::Dynamic(crate::DynamicValue::U32(theme))
    });
    write_field(root, offset, value);
}

/// Make nodes up to `node` exist, each a Stack child of node 1 that
/// references the theme with its own identity, so per-node fixtures author
/// their parts in a theme of their own.
pub(in crate::world::systems) fn ensure_themed_nodes(root: &mut super::GuiRoot, node: u32) {
    while root.next_node_id() <= node {
        let id = super::GuiNodeId(root.next_node_id());
        let parent = (id.0 != 1).then_some(super::GuiNodeId(1));
        root.insert_node(
            id,
            parent,
            usize::MAX,
            super::GuiNodeData::Container(super::GuiContainerKind::Stack),
            super::GuiNodeDataRow::default(),
            &super::GuiNodeStyle::default(),
        )
        .unwrap();
        set_node_theme(root, id, Some(id.0));
    }
}

/// Author one part property of `node` in the node's own theme, the
/// per-node fixture shape of named part properties.
pub(in crate::world::systems) fn author_part(
    root: &mut super::GuiRoot,
    node: u32,
    part: &str,
    property: &str,
    value: crate::DynamicValue,
) {
    ensure_themed_nodes(root, node);
    set_theme_part(root, node, part, property, Some(value));
}

/// Write one live channel of a node's part row, as a skin transition does.
/// The node's theme must declare motion for the part, which opens it.
pub(in crate::world::systems) fn set_part_channel(
    root: &mut super::GuiRoot,
    node: u32,
    part: crate::systems::surface::GuiPrimitivePart,
    channel: super::GuiPartChannel,
    value: crate::DynamicValue,
) {
    let (slot, _) = root
        .part_row(super::GuiNodeId(node), part)
        .expect("animated part has live channels");
    let offset = super::GuiRoot::part_row_offset(slot, channel.index()).unwrap();
    write_field(
        root,
        offset,
        crate::components::schema::FieldValue::Dynamic(value),
    );
}
