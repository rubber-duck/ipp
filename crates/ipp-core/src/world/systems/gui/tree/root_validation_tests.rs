//! GuiRoot validates each written field for exactly what the write changed
//! instead of walking the whole root after every operation. Every write the
//! registry accepts must leave a root that passes complete validation, and
//! every write it rejects must be one complete validation would reject, so
//! both paths enforce the same rules.

use super::*;
use crate::ComponentValue;
use crate::components::registry;
use crate::components::schema::FieldValue as SchemaValue;
use crate::services::asset_management::AssetSource;
use crate::systems::animation::ANIMATION_TYPE;
use crate::systems::gui::test_support::{set_node_theme, set_theme_part};
use crate::systems::gui::tree::nodes::GuiContainerKind;

fn motion() -> DynamicValue {
    DynamicValue::Asset(AssetSource {
        kind: ANIMATION_TYPE,
        uri: "fade.ippa".into(),
        variant: 0,
    })
}

/// A root with slider and checkbox data rows, two themes (the first
/// animating its background), theme references and part overrides.
fn fixture() -> GuiRoot {
    let mut root = GuiRoot::default();
    let nodes = [
        (
            GuiNodeData::Container(GuiContainerKind::Column),
            GuiNodeDataRow::default(),
        ),
        (
            GuiNodeData::Slider,
            GuiNodeDataRow::slider(0.5, 0.0, 1.0, 0.1),
        ),
        (GuiNodeData::Checkbox, GuiNodeDataRow::checkbox(true)),
        (GuiNodeData::Text("label".into()), GuiNodeDataRow::default()),
        (
            GuiNodeData::Container(GuiContainerKind::Row),
            GuiNodeDataRow::default(),
        ),
    ];
    for (index, (data, values)) in nodes.into_iter().enumerate() {
        let id = GuiNodeId(index as u32 + 1);
        let parent = (id.0 != 1).then_some(GuiNodeId(1));
        root.insert_node(
            id,
            parent,
            usize::MAX,
            data,
            values,
            &GuiNodeStyle::default(),
        )
        .unwrap();
    }

    set_theme_part(
        &mut root,
        1,
        "background",
        "color",
        Some(DynamicValue::Vec4([0.2, 0.4, 0.6, 1.0])),
    );
    set_theme_part(&mut root, 1, "background_hovered", "motion", Some(motion()));
    set_theme_part(
        &mut root,
        2,
        "icon",
        "opacity",
        Some(DynamicValue::F32(0.5)),
    );
    for node in [2, 3] {
        set_node_theme(&mut root, GuiNodeId(node), Some(1));
    }
    set_node_theme(&mut root, GuiNodeId(4), Some(2));

    let writes = root
        .part_override_writes(
            GuiNodeId(4),
            GuiPrimitivePart::Icon,
            &GuiPartPatch::default().set(GuiPartProperty::Opacity, DynamicValue::F32(0.25)),
        )
        .unwrap();
    crate::systems::gui::test_support::apply_writes(&mut root, writes);

    root.validate_complete().unwrap();
    assert!(!root.part_state().is_empty());
    root
}

fn schema_value(value: &FieldValue) -> SchemaValue {
    match value {
        FieldValue::Dynamic(value) => SchemaValue::Dynamic(value.clone()),
        FieldValue::Bytes(bytes) => SchemaValue::Bytes(bytes.clone()),
        FieldValue::Rows(bytes) => SchemaValue::Rows(bytes.clone()),
        FieldValue::Unset => SchemaValue::Unset,
        other => panic!("unexpected GUI write {other:?}"),
    }
}

/// Write one field as staging does and check that the per-field verdict
/// matches complete validation of the unchecked result. A rejected write
/// restores the written field and leaves a valid root; channels a rejected
/// theme table re-derived may keep their new part row slots.
fn write(root: &GuiRoot, offset: u32, value: FieldValue) -> Result<(), ErrorReason> {
    let mut unchecked = ComponentValue::GuiRoot(root.clone());
    let whole = match unchecked.set_field(offset, schema_value(&value)) {
        Ok(()) => unchecked.validate_lifecycle(),
        Err(_) => Err(ErrorReason::InvalidField),
    };

    let mut staged = ComponentValue::GuiRoot(root.clone());
    let field = FieldWrite {
        offset,
        value,
    };
    let per_field = registry::write(&mut staged, &field);
    assert_eq!(
        per_field.is_ok(),
        whole.is_ok(),
        "per-field {per_field:?} and complete {whole:?} validation disagree on {field:?}"
    );
    if per_field.is_err() {
        assert_eq!(
            staged.field(offset),
            ComponentValue::GuiRoot(root.clone()).field(offset)
        );
        assert_eq!(staged.validate_lifecycle(), Ok(()));
    }
    per_field
}

fn tree_write(nodes: &GuiNodes) -> FieldValue {
    FieldValue::Bytes(nodes.encode())
}

#[test]
fn operations_rely_on_per_field_checks() {
    assert!(!<GuiRoot as ComponentLifecycle>::validates_after_operation());
}

#[test]
fn tree_writes_reject_bad_links_and_nodes_without_rows() {
    let root = fixture();

    let mut dangling = root.nodes().clone();
    dangling.node_mut(GuiNodeId(3)).unwrap().parent = Some(GuiNodeId(99));
    assert!(write(&root, GuiRoot::nodes_field(), tree_write(&dangling)).is_err());

    let mut cycle = root.nodes().clone();
    cycle
        .node_mut(GuiNodeId(1))
        .unwrap()
        .children
        .push(GuiNodeId(1));
    assert!(write(&root, GuiRoot::nodes_field(), tree_write(&cycle)).is_err());

    // Restoring a removed node's identity finds its row slots dead.
    let before = root.nodes().clone();
    let mut removed = root.clone();
    removed.remove_node(GuiNodeId(5)).unwrap();
    assert_eq!(
        write(&removed, GuiRoot::nodes_field(), tree_write(&before)),
        Err(ErrorReason::InvalidField)
    );

    // A valid move, kind change and removal are accepted.
    let mut moved = root.nodes().clone();
    moved
        .move_node(GuiNodeId(3), Some(GuiNodeId(5)), 0)
        .unwrap();
    assert_eq!(
        write(&root, GuiRoot::nodes_field(), tree_write(&moved)),
        Ok(())
    );
    let mut retyped = root.nodes().clone();
    retyped
        .replace_data(GuiNodeId(2), GuiNodeData::Checkbox)
        .unwrap();
    retyped
        .controls
        .restart(GuiNodeId(2), &GuiNodeData::Checkbox)
        .unwrap();
    assert_eq!(
        write(&root, GuiRoot::nodes_field(), tree_write(&retyped)),
        Ok(())
    );
    let mut pruned = root.nodes().clone();
    pruned.remove_node(GuiNodeId(2)).unwrap();
    pruned.controls.remove(GuiNodeId(2));
    assert_eq!(
        write(&root, GuiRoot::nodes_field(), tree_write(&pruned)),
        Ok(())
    );
}

#[test]
fn row_property_writes_follow_every_row_rule() {
    let root = fixture();
    let values = [
        None,
        Some(DynamicValue::F32(0.5)),
        Some(DynamicValue::F32(-1.0)),
        Some(DynamicValue::F32(2.0)),
        Some(DynamicValue::F32(f32::NAN)),
        Some(DynamicValue::Vec2([1.0, 1.0])),
        Some(DynamicValue::Vec2([-1.0, 1.0])),
        Some(DynamicValue::Vec4([0.5; 4])),
        Some(DynamicValue::Vec4([2.0; 4])),
        Some(DynamicValue::Bool(false)),
        Some(DynamicValue::U32(1)),
        Some(DynamicValue::U32(2)),
        Some(DynamicValue::U32(9)),
        Some(motion()),
    ];
    let value =
        |value: &Option<DynamicValue>| value.clone().map_or(FieldValue::Unset, FieldValue::Dynamic);

    let mut offsets = Vec::new();
    for node in root.nodes().as_slice() {
        offsets.extend(
            GuiNodeStyleProperty::ALL
                .into_iter()
                .filter_map(|property| GuiRoot::node_style_offset(node.id, property)),
        );
        offsets.extend(
            GuiNodeDataProperty::ALL
                .into_iter()
                .filter_map(|property| GuiRoot::node_data_offset(node.id, property)),
        );
    }
    for (slot, _) in root.theme_parts().iter() {
        offsets.extend(
            GuiPartProperty::ALL
                .into_iter()
                .filter_map(|property| GuiRoot::theme_part_offset(slot, property)),
        );
        offsets.extend(GuiRoot::theme_part_offset(slot, GuiPartProperty::Motion));
    }
    for (slot, _) in root.part_state().iter() {
        offsets.extend(
            (0..GuiPartRow::NODE).filter_map(|index| GuiRoot::part_row_offset(slot, index)),
        );
    }

    let mut rejected = 0;
    for offset in offsets {
        for candidate in &values {
            rejected += usize::from(write(&root, offset, value(candidate)).is_err());
        }
    }
    assert!(rejected > 0);

    // Named cases: an out-of-range style value, a slider value outside its
    // range and a checkbox value on a slider.
    let opacity = GuiRoot::node_style_offset(GuiNodeId(2), GuiNodeStyleProperty::Opacity).unwrap();
    assert_eq!(
        write(&root, opacity, FieldValue::Dynamic(DynamicValue::F32(1.5))),
        Err(ErrorReason::InvalidValue)
    );
    let slider = GuiRoot::node_data_offset(GuiNodeId(2), GuiNodeDataProperty::Value).unwrap();
    assert_eq!(
        write(&root, slider, FieldValue::Dynamic(DynamicValue::F32(3.0))),
        Err(ErrorReason::InvalidValue)
    );
    let checked = GuiRoot::node_data_offset(GuiNodeId(2), GuiNodeDataProperty::Checked).unwrap();
    assert!(
        write(
            &root,
            checked,
            FieldValue::Dynamic(DynamicValue::Bool(true))
        )
        .is_err()
    );

    // Row keys are never writable, even with their current value, which
    // complete validation alone would accept.
    let (slot, row) = root.part_row(GuiNodeId(4), GuiPrimitivePart::Icon).unwrap();
    for (index, key) in [
        (GuiPartRow::NODE, row.node),
        (GuiPartRow::NODE + 1, row.part),
    ] {
        let offset = GuiRoot::part_row_offset(slot, index).unwrap();
        let mut value = ComponentValue::GuiRoot(root.clone());
        let write = FieldWrite {
            offset,
            value: FieldValue::Dynamic(DynamicValue::U32(key)),
        };
        assert_eq!(
            registry::write(&mut value, &write),
            Err(ErrorReason::InvalidField)
        );
    }
    let theme_key = Rows::<GuiThemePartRow>::offset(
        GuiRoot::THEME_PARTS_FIELD,
        root.theme_slot(1).unwrap() * GuiPartId::COUNT,
        GuiThemePartRow::THEME,
    )
    .unwrap();
    let mut value = ComponentValue::GuiRoot(root.clone());
    let write = FieldWrite {
        offset: theme_key,
        value: FieldValue::Dynamic(DynamicValue::U32(1)),
    };
    assert_eq!(
        registry::write(&mut value, &write),
        Err(ErrorReason::InvalidField)
    );
}

#[test]
fn theme_references_and_motion_re_derive_checked_part_rows() {
    let root = fixture();
    let theme = GuiRoot::node_style_offset(GuiNodeId(5), GuiNodeStyleProperty::Theme).unwrap();
    assert_eq!(
        write(&root, theme, FieldValue::Dynamic(DynamicValue::U32(1))),
        Ok(())
    );
    assert_eq!(
        write(&root, theme, FieldValue::Dynamic(DynamicValue::U32(7))),
        Ok(())
    );
    let unthemed = GuiRoot::node_style_offset(GuiNodeId(2), GuiNodeStyleProperty::Theme).unwrap();
    assert_eq!(write(&root, unthemed, FieldValue::Unset), Ok(()));

    let theme_slot = root.theme_slot(1).unwrap();
    let hovered = GuiRoot::theme_part_slot(
        theme_slot,
        GuiPartId::state(
            GuiPrimitivePart::Background,
            crate::systems::gui::GuiSkinState::Hovered,
        ),
    )
    .unwrap();
    let motion_offset = GuiRoot::theme_part_offset(hovered, GuiPartProperty::Motion).unwrap();
    assert_eq!(write(&root, motion_offset, FieldValue::Unset), Ok(()));
    let icon = GuiRoot::theme_part_slot(
        root.theme_slot(2).unwrap(),
        GuiPartId::base(GuiPrimitivePart::Icon),
    )
    .unwrap();
    let icon_motion = GuiRoot::theme_part_offset(icon, GuiPartProperty::Motion).unwrap();
    assert_eq!(
        write(&root, icon_motion, FieldValue::Dynamic(motion())),
        Ok(())
    );
}

#[test]
fn skin_table_writes_reject_dangling_keys_and_index_inconsistency() {
    let root = fixture();
    let part_table = |table: Rows<GuiPartRow>| FieldValue::Rows(table.encode());
    let theme_table = |table: Rows<GuiThemePartRow>| FieldValue::Rows(table.encode());

    // A part row keyed to a node the tree does not hold.
    let mut dangling = Rows::clone(root.part_state());
    let mut row = GuiPartRow::keyed(99, GuiPrimitivePart::Background);
    row.opacity = Some(0.5);
    dangling.push(row).unwrap();
    assert_eq!(
        write(&root, GuiRoot::part_state_field(), part_table(dangling)),
        Err(ErrorReason::InvalidField)
    );

    // Two rows under one key would leave one of them outside the index.
    let mut duplicate = Rows::clone(root.part_state());
    let (_, existing) = root.part_row(GuiNodeId(4), GuiPrimitivePart::Icon).unwrap();
    duplicate.push(existing.clone()).unwrap();
    assert_eq!(
        write(&root, GuiRoot::part_state_field(), part_table(duplicate)),
        Err(ErrorReason::InvalidField)
    );

    // A theme slot holding parts of two themes, and one theme at two slots.
    let first = root.theme_slot(1).unwrap();
    let second = root.theme_slot(2).unwrap();
    let mut mixed = Rows::clone(root.theme_parts());
    mixed
        .insert(
            GuiRoot::theme_part_slot(first, GuiPartId::base(GuiPrimitivePart::Icon)).unwrap(),
            GuiThemePartRow::for_theme(2),
        )
        .unwrap();
    assert_eq!(
        write(&root, GuiRoot::theme_parts_field(), theme_table(mixed)),
        Err(ErrorReason::InvalidField)
    );
    let mut split = Rows::clone(root.theme_parts());
    let spare = second.max(first) + 1;
    split
        .insert(
            GuiRoot::theme_part_slot(spare, GuiPartId::base(GuiPrimitivePart::Icon)).unwrap(),
            GuiThemePartRow::for_theme(1),
        )
        .unwrap();
    assert_eq!(
        write(&root, GuiRoot::theme_parts_field(), theme_table(split)),
        Err(ErrorReason::InvalidField)
    );

    // An out-of-range value inside a replaced table.
    let mut out_of_range = Rows::clone(root.part_state());
    let mut row = GuiPartRow::keyed(5, GuiPrimitivePart::Background);
    row.opacity = Some(4.0);
    out_of_range.push(row).unwrap();
    assert_eq!(
        write(&root, GuiRoot::part_state_field(), part_table(out_of_range)),
        Err(ErrorReason::InvalidValue)
    );

    // Replacing either table with valid content, which re-derives every
    // themed node's channels, is accepted.
    let mut added = Rows::clone(root.theme_parts());
    let mut row = GuiThemePartRow::for_theme(2);
    row.motion = Some(match motion() {
        DynamicValue::Asset(source) => source,
        _ => unreachable!("motion is an asset reference"),
    });
    added
        .insert(
            GuiRoot::theme_part_slot(second, GuiPartId::base(GuiPrimitivePart::Background))
                .unwrap(),
            row,
        )
        .unwrap();
    assert_eq!(
        write(&root, GuiRoot::theme_parts_field(), theme_table(added)),
        Ok(())
    );
    assert_eq!(
        write(
            &root,
            GuiRoot::part_state_field(),
            part_table(Rows::default())
        ),
        Ok(())
    );
}

#[test]
fn node_table_writes_check_rows_against_their_nodes() {
    let root = fixture();
    let style_table = |table: Rows<GuiNodeStyleRow>| FieldValue::Rows(table.encode());
    let data_table = |table: Rows<GuiNodeDataRow>| FieldValue::Rows(table.encode());

    let mut missing = Rows::clone(root.node_style());
    missing.remove(3);
    assert!(write(&root, GuiRoot::node_style_field(), style_table(missing)).is_err());

    let mut out_of_range = Rows::clone(root.node_style());
    out_of_range.get_mut(3).unwrap().opacity = 7.0;
    assert!(
        write(
            &root,
            GuiRoot::node_style_field(),
            style_table(out_of_range)
        )
        .is_err()
    );

    let mut wrong_kind = Rows::clone(root.node_data());
    *wrong_kind.get_mut(3).unwrap() = GuiNodeDataRow::slider(0.0, 0.0, 1.0, 0.0);
    assert!(write(&root, GuiRoot::node_data_field(), data_table(wrong_kind)).is_err());

    let mut edited = Rows::clone(root.node_data());
    edited.get_mut(2).unwrap().value = Some(0.75);
    assert_eq!(
        write(&root, GuiRoot::node_data_field(), data_table(edited)),
        Ok(())
    );
}
