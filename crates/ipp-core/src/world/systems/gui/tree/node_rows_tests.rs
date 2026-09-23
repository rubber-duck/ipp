use super::*;
use crate::components::rows::{Rows, row_region_base};
use crate::components::schema::{ComponentLifecycle, FieldKind, FieldValue, SchemaComponent};
use crate::services::asset_management::font::FONT_TYPE;
use crate::world::systems::gui::{GuiContainerKind, GuiNodeId, GuiRoot};

const NAMES: [&str; 19] = [
    "enabled",
    "width",
    "height",
    "min_width",
    "min_height",
    "max_width",
    "max_height",
    "flex",
    "align_x",
    "align_y",
    "color",
    "background_color",
    "opacity",
    "font_size",
    "asset",
    "position",
    "scale",
    "padding",
    "margin",
];

fn font() -> AssetSource {
    AssetSource {
        kind: FONT_TYPE,
        uri: "fonts/inter".into(),
        variant: 0,
    }
}

fn styled() -> GuiNodeStyle {
    GuiNodeStyle {
        enabled: false,
        width: Some(1.0),
        height: Some(2.0),
        min_width: Some(0.5),
        min_height: None,
        max_width: Some(4.0),
        max_height: None,
        padding: Some([0.1, 0.2, 0.3, 0.4]),
        margin: Some([-0.1, 0.0, 0.1, 0.0]),
        flex: Some(1.0),
        align_x: Some(-1.0),
        align_y: None,
        color: [0.2, 0.4, 0.6, 1.0],
        background_color: Some([0.0, 0.0, 0.0, 0.5]),
        opacity: 0.75,
        font_size: 0.05,
        asset: Some(font()),
    }
}

#[test]
fn layout_order_matches_property_index() {
    let layout = GuiNodeStyleRow::LAYOUT;
    assert_eq!(layout.property_count(), GuiNodeStyleProperty::COUNT);
    assert_eq!(
        layout.properties.iter().map(|p| p.name).collect::<Vec<_>>(),
        NAMES
    );

    for (index, property) in GuiNodeStyleProperty::ALL.into_iter().enumerate() {
        assert_eq!(property.index(), index as u32);
        assert_eq!(
            GuiNodeStyleProperty::from_index(index as u32),
            Some(property)
        );
        assert_eq!(property.name(), NAMES[index]);
    }
    assert_eq!(
        GuiNodeStyleProperty::from_index(GuiNodeStyleProperty::COUNT),
        None
    );

    let required = [
        GuiNodeStyleProperty::Enabled,
        GuiNodeStyleProperty::Color,
        GuiNodeStyleProperty::Opacity,
        GuiNodeStyleProperty::FontSize,
        GuiNodeStyleProperty::Position,
        GuiNodeStyleProperty::Scale,
    ];
    for property in GuiNodeStyleProperty::ALL {
        assert_eq!(
            property.optional(),
            !required.contains(&property),
            "{property:?}"
        );
    }
    assert_eq!(
        GuiNodeStyleProperty::Asset.kind(),
        DynamicPropertyKind::Asset
    );
    assert_eq!(
        GuiNodeStyleProperty::Position.kind(),
        DynamicPropertyKind::Vec2
    );
    assert_eq!(
        GuiNodeStyleProperty::Margin.kind(),
        DynamicPropertyKind::Vec4
    );
}

#[test]
fn node_property_offsets_address_slot_by_node_id() {
    let base = row_region_base(GuiRoot::NODE_STYLE_FIELD);
    assert_eq!(base, 0x1000_0000);
    assert_eq!(
        GuiRoot::node_style_offset(GuiNodeId(1), GuiNodeStyleProperty::Enabled),
        Some(base + 19)
    );
    assert_eq!(
        GuiRoot::node_style_offset(GuiNodeId(2), GuiNodeStyleProperty::Margin),
        Some(base + 2 * 19 + 18)
    );

    for id in [1, 7, 65_536] {
        for property in GuiNodeStyleProperty::ALL {
            let offset = GuiRoot::node_style_offset(GuiNodeId(id), property).unwrap();
            assert_eq!(
                GuiRoot::node_property(offset),
                Some(GuiNodePropertyRef {
                    node: GuiNodeId(id),
                    property: GuiNodeRowProperty::Style(property),
                })
            );
        }
    }

    let last = Rows::<GuiNodeStyleRow>::MAX_SLOTS - 1;
    assert!(GuiRoot::node_style_offset(GuiNodeId(last), GuiNodeStyleProperty::Margin).is_some());
    assert_eq!(
        GuiRoot::node_style_offset(GuiNodeId(last + 1), GuiNodeStyleProperty::Enabled),
        None
    );
    assert_eq!(GuiRoot::node_property(base - 1), None);
    assert_eq!(
        GuiRoot::node_property(base),
        None,
        "node id zero is never valid"
    );
}

#[test]
fn default_row_is_the_default_node_style() {
    let row = GuiNodeStyleRow::default();
    assert!(row.enabled);
    assert_eq!(row.position, [0.0, 0.0]);
    assert_eq!(row.scale, [1.0, 1.0]);
    assert_eq!(GuiNodeStyle::from(&row), GuiNodeStyle::default());
    assert_eq!(row, GuiNodeStyleRow::from(&GuiNodeStyle::default()));
    assert_eq!(row.validate(), Ok(()));
}

#[test]
fn style_and_row_conversions_round_trip() {
    let style = styled();
    let mut row = GuiNodeStyleRow::from(&style);
    assert_eq!(GuiNodeStyle::from(&row), style);

    row.position = [0.3, -0.2];
    row.scale = [2.0, 0.5];
    assert_eq!(GuiNodeStyle::from(&row), style);

    let mut table = Rows::<GuiNodeStyleRow>::new();
    table.insert(3, row.clone()).unwrap();
    assert_eq!(Rows::<GuiNodeStyleRow>::decode(&table.encode()), Ok(table));
}

#[test]
fn patch_preserves_omitted_and_clears_optional_members() {
    let mut row = GuiNodeStyleRow::from(&styled());
    row.position = [1.0, 1.0];
    row.apply(&GuiNodePatch {
        width: Some(None),
        height: Some(Some(3.0)),
        color: Some([1.0, 0.0, 0.0, 1.0]),
        asset: Some(None),
        ..GuiNodePatch::default()
    });

    let mut expected = GuiNodeStyleRow::from(&styled());
    expected.position = [1.0, 1.0];
    expected.width = None;
    expected.height = Some(3.0);
    expected.color = [1.0, 0.0, 0.0, 1.0];
    expected.asset = None;
    assert_eq!(row, expected);
}

#[test]
fn node_property_ranges_follow_the_property_index() {
    use GuiNodeStyleProperty as P;

    let ok = |property, value| {
        let offset = GuiRoot::node_style_offset(GuiNodeId(1), property).unwrap();
        GuiRoot::validate_node_property(offset, &value)
    };
    assert_eq!(ok(P::Opacity, DynamicValue::F32(1.0)), Ok(()));
    assert_eq!(
        ok(P::Opacity, DynamicValue::F32(1.5)),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        ok(P::FontSize, DynamicValue::F32(0.0)),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        ok(P::Width, DynamicValue::F32(-1.0)),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(ok(P::AlignX, DynamicValue::F32(-1.0)), Ok(()));
    assert_eq!(
        ok(P::Color, DynamicValue::Vec4([0.0, 0.0, 0.0, 2.0])),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        ok(P::Padding, DynamicValue::Vec4([0.0, -0.1, 0.0, 0.0])),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        ok(P::Margin, DynamicValue::Vec4([0.0, -0.1, 0.0, 0.0])),
        Ok(())
    );
    assert_eq!(ok(P::Position, DynamicValue::Vec2([-5.0, 5.0])), Ok(()));
    assert_eq!(
        ok(P::Scale, DynamicValue::Vec2([f32::NAN, 1.0])),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        ok(P::Enabled, DynamicValue::F32(1.0)),
        Err(ErrorReason::InvalidField)
    );
    assert_eq!(ok(P::Asset, DynamicValue::Asset(font())), Ok(()));

    let row = GuiNodeStyleRow {
        opacity: 2.0,
        ..GuiNodeStyleRow::default()
    };
    assert_eq!(row.validate(), Err(ErrorReason::InvalidValue));
}

#[test]
fn gui_root_exposes_node_style_rows_by_offset() {
    let rows = rows_fields();
    let node_style = rows[0];

    let mut table = Rows::<GuiNodeStyleRow>::new();
    table
        .insert(
            2,
            GuiNodeStyleRow {
                asset: Some(font()),
                ..GuiNodeStyleRow::default()
            },
        )
        .unwrap();

    let mut root = GuiRoot::default();
    root.set_field(node_style, FieldValue::Rows(table.encode()))
        .unwrap();
    assert_eq!(root.node_style(), &table);

    let opacity = GuiRoot::node_style_offset(GuiNodeId(2), GuiNodeStyleProperty::Opacity).unwrap();
    let width = GuiRoot::node_style_offset(GuiNodeId(2), GuiNodeStyleProperty::Width).unwrap();
    assert_eq!(
        root.field(opacity),
        Ok(FieldValue::Dynamic(DynamicValue::F32(1.0)))
    );
    assert_eq!(root.field(width), Ok(FieldValue::Unset));
    root.set_field(opacity, FieldValue::Dynamic(DynamicValue::F32(0.5)))
        .unwrap();
    assert_eq!(root.node_style().get(2).unwrap().opacity, 0.5);

    let dead = GuiRoot::node_style_offset(GuiNodeId(3), GuiNodeStyleProperty::Opacity).unwrap();
    assert!(root.field(dead).is_err());

    let mut demand = std::collections::BTreeSet::new();
    root.resource_demand(&mut demand);
    assert_eq!(
        demand
            .into_iter()
            .map(|selection| selection.descriptor())
            .collect::<Vec<_>>(),
        [font()]
    );
}

const DATA_NAMES: [&str; 6] = ["image_size", "checked", "value", "min", "max", "step"];

/// Real offsets of the exposed rows fields: `node_style`, then `node_data`.
fn rows_fields() -> Vec<u32> {
    let rows: Vec<u32> = GuiRoot::default()
        .fields()
        .into_iter()
        .filter(|(_, value)| value.kind() == FieldKind::Rows)
        .map(|(offset, _)| offset)
        .collect();
    assert_eq!(rows.len(), 2);
    rows
}

fn slider() -> GuiNodeContent {
    GuiNodeContent::Slider {
        value: 0.25,
        min: 0.0,
        max: 1.0,
        step: 0.05,
    }
}

fn contents() -> [GuiNodeContent; 8] {
    [
        GuiNodeContent::Container(GuiContainerKind::Column),
        GuiNodeContent::Text("label".into()),
        GuiNodeContent::Drawing,
        GuiNodeContent::Image {
            size: [0.2, 0.1],
        },
        GuiNodeContent::Button {
            label: "go".into(),
        },
        GuiNodeContent::Checkbox {
            checked: true,
        },
        slider(),
        GuiNodeContent::TextInput {
            text: "t".into(),
            placeholder: "p".into(),
        },
    ]
}

#[test]
fn data_layout_order_matches_property_index() {
    let layout = GuiNodeDataRow::LAYOUT;
    assert_eq!(layout.property_count(), GuiNodeDataProperty::COUNT);
    assert_eq!(
        layout.properties.iter().map(|p| p.name).collect::<Vec<_>>(),
        DATA_NAMES
    );
    assert!(layout.properties.iter().all(|p| p.optional));

    for (index, property) in GuiNodeDataProperty::ALL.into_iter().enumerate() {
        assert_eq!(property.index(), index as u32);
        assert_eq!(
            GuiNodeDataProperty::from_index(index as u32),
            Some(property)
        );
        assert_eq!(property.name(), DATA_NAMES[index]);
    }
    assert_eq!(
        GuiNodeDataProperty::from_index(GuiNodeDataProperty::COUNT),
        None
    );
    assert_eq!(
        GuiNodeDataProperty::ImageSize.kind(),
        DynamicPropertyKind::Vec2
    );
    assert_eq!(
        GuiNodeDataProperty::Checked.kind(),
        DynamicPropertyKind::Bool
    );
    assert_eq!(GuiNodeDataProperty::Step.kind(), DynamicPropertyKind::F32);
    assert_eq!(
        GuiNodeDataRow::default(),
        GuiNodeDataRow::from_content(&GuiNodeContent::Drawing)
    );
}

#[test]
fn node_data_offsets_use_the_second_region() {
    let base = row_region_base(GuiRoot::NODE_DATA_FIELD);
    assert_eq!(base, 0x2000_0000);
    assert_eq!(
        GuiRoot::node_data_offset(GuiNodeId(1), GuiNodeDataProperty::ImageSize),
        Some(base + 6)
    );
    assert_eq!(
        GuiRoot::node_data_offset(GuiNodeId(3), GuiNodeDataProperty::Step),
        Some(base + 3 * 6 + 5)
    );

    for id in [1, 9, 65_536] {
        for property in GuiNodeDataProperty::ALL {
            let offset = GuiRoot::node_data_offset(GuiNodeId(id), property).unwrap();
            assert_eq!(
                GuiRoot::node_property(offset),
                Some(GuiNodePropertyRef {
                    node: GuiNodeId(id),
                    property: GuiNodeRowProperty::Data(property),
                })
            );
        }
    }

    let last = Rows::<GuiNodeDataRow>::MAX_SLOTS - 1;
    assert!(GuiRoot::node_data_offset(GuiNodeId(last), GuiNodeDataProperty::Step).is_some());
    assert_eq!(
        GuiRoot::node_data_offset(GuiNodeId(last + 1), GuiNodeDataProperty::ImageSize),
        None
    );
    assert_eq!(GuiRoot::node_property(base), None);
    assert_eq!(GuiRoot::node_property(base + 0x1000_0000), None);
}

#[test]
fn numeric_animation_targets_only_numeric_style_and_image_size() {
    use GuiNodeDataProperty as D;
    use GuiNodeStyleProperty as S;

    for property in GuiNodeStyleProperty::ALL {
        let offset = GuiRoot::node_style_offset(GuiNodeId(4), property).unwrap();
        let expected = !matches!(property, S::Enabled | S::Asset);
        assert_eq!(
            GuiRoot::numeric_animatable(offset),
            expected,
            "{property:?}"
        );
    }
    for property in GuiNodeDataProperty::ALL {
        let offset = GuiRoot::node_data_offset(GuiNodeId(4), property).unwrap();
        assert_eq!(
            GuiRoot::numeric_animatable(offset),
            property == D::ImageSize,
            "{property:?}"
        );
    }
    assert!(!GuiRoot::numeric_animatable(0));
    assert!(!GuiRoot::numeric_animatable(0x3000_0000 + 20));
    assert!(!GuiRoot::numeric_animatable(0x8000_0001));
}

#[test]
fn data_presence_follows_the_node_kind() {
    let kinds = contents();
    for (index, content) in kinds.iter().enumerate() {
        let row = GuiNodeDataRow::from_content(content);
        assert_eq!(row.validate_for(content), Ok(()), "{content:?}");
        assert_eq!(row.to_content(content).as_ref(), Some(content));
        for (other_index, other) in kinds.iter().enumerate() {
            let same_presence = GuiNodeDataRow::from_content(other) == GuiNodeDataRow::default()
                && row == GuiNodeDataRow::default();
            if other_index != index && !same_presence {
                assert!(!row.matches_kind(other), "{content:?} as {other:?}");
                assert_eq!(row.validate_for(other), Err(ErrorReason::InvalidField));
                assert_eq!(row.to_content(other), None);
            }
        }
    }

    let partial = GuiNodeDataRow {
        max: None,
        ..GuiNodeDataRow::from_content(&slider())
    };
    assert_eq!(
        partial.validate_for(&slider()),
        Err(ErrorReason::InvalidField)
    );
}

#[test]
fn data_row_values_carry_committed_state_back_into_content() {
    let committed = GuiNodeDataRow {
        value: Some(0.75),
        ..GuiNodeDataRow::from_content(&slider())
    };
    assert_eq!(
        committed.to_content(&slider()),
        Some(GuiNodeContent::Slider {
            value: 0.75,
            min: 0.0,
            max: 1.0,
            step: 0.05,
        })
    );

    let checkbox = GuiNodeContent::Checkbox {
        checked: true,
    };
    let unchecked = GuiNodeDataRow {
        checked: Some(false),
        ..GuiNodeDataRow::default()
    };
    assert_eq!(
        unchecked.to_content(&checkbox),
        Some(GuiNodeContent::Checkbox {
            checked: false,
        })
    );
}

#[test]
fn data_ranges_follow_today_s_content_and_control_rules() {
    use GuiNodeDataProperty as D;

    let check = |property, value| {
        let offset = GuiRoot::node_data_offset(GuiNodeId(1), property).unwrap();
        GuiRoot::validate_node_property(offset, &value)
    };
    assert_eq!(check(D::ImageSize, DynamicValue::Vec2([0.1, 0.1])), Ok(()));
    assert_eq!(
        check(D::ImageSize, DynamicValue::Vec2([0.0, 0.1])),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        check(D::Step, DynamicValue::F32(-0.1)),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(check(D::Min, DynamicValue::F32(-5.0)), Ok(()));
    assert_eq!(
        check(D::Value, DynamicValue::F32(f32::INFINITY)),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        check(D::Checked, DynamicValue::F32(1.0)),
        Err(ErrorReason::InvalidField)
    );
    assert_eq!(
        GuiRoot::validate_node_property(0x3000_0000, &DynamicValue::F32(1.0)),
        Err(ErrorReason::InvalidField)
    );

    let base = GuiNodeDataRow::from_content(&slider());
    let out_of_range = GuiNodeDataRow {
        value: Some(1.5),
        ..base.clone()
    };
    assert_eq!(
        out_of_range.validate_for(&slider()),
        Err(ErrorReason::InvalidValue)
    );
    let inverted = GuiNodeDataRow {
        min: Some(2.0),
        max: Some(1.0),
        value: Some(1.5),
        ..base
    };
    assert_eq!(
        inverted.validate_for(&slider()),
        Err(ErrorReason::InvalidValue)
    );
}

#[test]
fn gui_root_validates_node_data_against_live_nodes() {
    let node_data = rows_fields()[1];
    let mut root = GuiRoot::default();
    let id = root
        .nodes_mut()
        .insert_node(GuiNodeId(1), None, 0, slider())
        .unwrap();
    root.controls_mut()
        .reconcile_content(id, &slider())
        .unwrap();
    assert_eq!(root.validate(), Ok(()));

    let mut table = Rows::<GuiNodeDataRow>::new();
    table
        .insert(id.0, GuiNodeDataRow::from_content(&slider()))
        .unwrap();
    root.set_field(node_data, FieldValue::Rows(table.encode()))
        .unwrap();
    assert_eq!(root.node_data(), &table);
    assert_eq!(root.validate(), Ok(()));

    let value = GuiRoot::node_data_offset(id, GuiNodeDataProperty::Value).unwrap();
    assert_eq!(
        root.field(value),
        Ok(FieldValue::Dynamic(DynamicValue::F32(0.25)))
    );
    root.set_field(value, FieldValue::Dynamic(DynamicValue::F32(4.0)))
        .unwrap();
    assert_eq!(root.validate(), Err(ErrorReason::InvalidValue));

    let mut orphan = Rows::<GuiNodeDataRow>::new();
    orphan.insert(7, GuiNodeDataRow::default()).unwrap();
    root.set_field(node_data, FieldValue::Rows(orphan.encode()))
        .unwrap();
    assert_eq!(root.validate(), Err(ErrorReason::InvalidField));
}
