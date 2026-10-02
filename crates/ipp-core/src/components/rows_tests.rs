//! Schema rows addressing, slot identity, derive dispatch and table codec.

use super::*;
use crate::components::schema::{ContractHash, SchemaComponent};
use crate::components::{RowsFixture, RowsFixtureItem, RowsFixtureTag};
use crate::{ComponentValue, DynamicValue};
use std::mem::offset_of;

const ITEMS: usize = 0;
const TAGS: usize = 1;
const WEIGHT: u32 = 0;
const COUNT: u32 = 1;
const OFFSET: u32 = 2;
const ROTATION: u32 = 3;
const TEXTURE: u32 = 5;
const LABEL: u32 = 9;

fn item(weight: f32) -> RowsFixtureItem {
    RowsFixtureItem {
        weight,
        count: 3,
        offset: Some([1.0, 2.0, 3.0]),
        ..RowsFixtureItem::default()
    }
}

fn item_offset(slot: u32, property: u32) -> u32 {
    Rows::<RowsFixtureItem>::offset(ITEMS, slot, property).unwrap()
}

fn fixture() -> RowsFixture {
    let mut fixture = RowsFixture::default();
    assert_eq!(fixture.items.push(item(1.0)), Ok(0));
    assert_eq!(fixture.items.push(item(2.0)), Ok(1));
    assert_eq!(
        fixture.tags.push(RowsFixtureTag {
            value: 9
        }),
        Ok(0)
    );
    fixture
}

#[test]
fn regions_lie_between_struct_offsets_and_the_dynamic_namespace() {
    assert_eq!(row_region_base(0), 0x1000_0000);
    assert_eq!(row_region_base(6), 0x7000_0000);
    assert_eq!(
        row_region_base(6) + ROW_REGION_SPAN,
        crate::components::dynamic_properties::DYNAMIC_METADATA
    );
    assert_eq!(row_region(0x0fff_ffff), None);
    assert_eq!(row_region(0x1000_0005), Some((0, 5)));
    assert_eq!(row_region(0x7fff_ffff), Some((6, ROW_REGION_SPAN - 1)));
    assert_eq!(row_region(0x8000_0000), None);
    assert_eq!(row_region_relative(0x2000_0001, 1), Some(1));
    assert_eq!(row_region_relative(0x2000_0001, 0), None);
    assert_eq!(row_region_relative(0x2000_0001, 7), None);

    // Nine properties per row: the last whole row of the region is addressable.
    let max = max_row_slots(9);
    assert_eq!(max, ROW_REGION_SPAN / 9);
    assert_eq!(
        row_address(9 * 4 + 2, 9),
        Some(RowAddress {
            slot: 4,
            property: 2
        })
    );
    assert_eq!(row_address(max * 9, 9), None);
    assert_eq!(
        row_property_offset(0, 9, max - 1, 8),
        Some(0x1000_0000 + max * 9 - 1)
    );
    assert_eq!(row_property_offset(0, 9, max, 0), None);
    assert_eq!(row_property_offset(0, 9, 0, 9), None);
    assert_eq!(row_property_offset(7, 9, 0, 0), None);
}

#[test]
fn derived_layout_follows_declaration_order_with_kinds_flags_and_hints() {
    let layout = RowsFixtureItem::LAYOUT;
    let names: Vec<_> = layout.properties.iter().map(|p| p.name).collect();
    assert_eq!(
        names,
        [
            "weight", "count", "offset", "rotation", "enabled", "texture", "delta", "size", "mark",
            "label"
        ]
    );
    assert_eq!(layout.property_count(), 10);
    assert_eq!(layout.mask_bytes(), 2);
    assert_eq!(
        layout.properties[3],
        RowProperty {
            name: "rotation",
            kind: DynamicPropertyKind::Vec4,
            optional: true,
            hint: RowPropertyHint::Rotation,
            max_bytes: 0,
        }
    );
    assert_eq!(
        layout.properties[LABEL as usize],
        RowProperty {
            name: "label",
            kind: DynamicPropertyKind::Text,
            optional: true,
            hint: RowPropertyHint::None,
            max_bytes: 16,
        }
    );
    assert_eq!(layout.properties[0].kind, DynamicPropertyKind::F32);
    assert!(!layout.properties[0].optional);
    assert_eq!(layout.properties[5].kind, DynamicPropertyKind::Asset);
    assert_eq!(layout.properties[6].kind, DynamicPropertyKind::I32);
    assert_eq!(RowsFixtureTag::LAYOUT.property_count(), 1);
}

#[test]
fn slots_are_never_reused_and_dead_or_unallocated_slots_reject_access() {
    let mut rows = Rows::<RowsFixtureItem>::new();
    assert_eq!(rows.slot_state(0), RowSlotState::Unallocated);
    assert_eq!(rows.push(item(1.0)), Ok(0));
    assert_eq!(rows.insert(4, item(4.0)), Ok(()));
    assert_eq!(rows.next_slot(), 5);

    // Never-used slots below next_slot stay unallocated and accept rows in any
    // order without moving next_slot; live slots reject.
    assert_eq!(rows.slot_state(2), RowSlotState::Unallocated);
    assert_eq!(rows.insert(2, item(2.0)), Ok(()));
    assert_eq!(rows.next_slot(), 5);
    assert_eq!(rows.slot_state(2), RowSlotState::Live);
    assert_eq!(rows.insert(2, item(9.0)), Err(FieldError::UnknownField));
    assert_eq!(rows.insert(4, item(9.0)), Err(FieldError::UnknownField));

    // Removed slots are dead and never reused within the incarnation.
    assert_eq!(rows.remove(0).map(|row| row.weight), Some(1.0));
    assert_eq!(rows.remove(0), None);
    assert_eq!(rows.remove(2).map(|row| row.weight), Some(2.0));
    for dead in [0, 2] {
        assert_eq!(rows.slot_state(dead), RowSlotState::Dead);
        assert_eq!(rows.insert(dead, item(0.0)), Err(FieldError::UnknownField));
    }
    assert_eq!(rows.push(item(5.0)), Ok(5));
    assert_eq!(rows.insert(3, item(3.0)), Ok(()));
    assert_eq!(
        rows.iter()
            .map(|(slot, row)| (slot, row.weight))
            .collect::<Vec<_>>(),
        [(3, 3.0), (4, 4.0), (5, 5.0)]
    );
    assert_eq!(rows.get(3).map(|row| row.weight), Some(3.0));
    rows.remove(3);

    for slot in [0, 2, 3, 6] {
        assert_eq!(rows.property(slot, WEIGHT), Err(FieldError::UnknownField));
        assert_eq!(
            rows.set_property(slot, WEIGHT, DynamicValue::F32(1.0)),
            Err(FieldError::UnknownField)
        );
        assert_eq!(
            rows.clear_property(slot, OFFSET),
            Err(FieldError::UnknownField)
        );
    }

    let last = Rows::<RowsFixtureItem>::MAX_SLOTS;
    assert_eq!(rows.insert(last, item(0.0)), Err(FieldError::UnknownField));
    assert_eq!(rows.insert(last - 1, item(0.0)), Ok(()));
    assert_eq!(rows.push(item(0.0)), Err(FieldError::UnknownField));
}

#[test]
fn decoding_starts_an_incarnation_without_dead_slots() {
    let mut rows = Rows::<RowsFixtureTag>::new();
    for value in 0..3 {
        rows.push(RowsFixtureTag {
            value,
        })
        .unwrap();
    }
    rows.remove(1);
    assert_eq!(rows.slot_state(1), RowSlotState::Dead);

    // Equality covers next_slot and live rows, not the in-memory dead record.
    let mut decoded = Rows::<RowsFixtureTag>::decode(&rows.encode()).unwrap();
    assert_eq!(decoded, rows);
    assert_eq!(decoded.next_slot(), 3);
    assert_eq!(decoded.slot_state(1), RowSlotState::Unallocated);
    assert_eq!(
        decoded.insert(
            1,
            RowsFixtureTag {
                value: 7
            }
        ),
        Ok(())
    );
    assert_eq!(decoded.next_slot(), 3);
    assert_eq!(
        decoded
            .iter()
            .map(|(slot, row)| (slot, row.value))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 7), (2, 2)]
    );
    assert_ne!(decoded, rows);
}

#[test]
fn component_offsets_read_and_write_row_properties_through_the_derive() {
    let mut fixture = fixture();
    let weight = item_offset(1, WEIGHT);
    let offset = item_offset(1, OFFSET);
    let rotation = item_offset(1, ROTATION);
    let tag = Rows::<RowsFixtureTag>::offset(TAGS, 0, 0).unwrap();
    assert_eq!(tag, 0x2000_0000);

    assert_eq!(
        fixture.field(weight),
        Ok(FieldValue::Dynamic(DynamicValue::F32(2.0)))
    );
    assert_eq!(fixture.field(rotation), Ok(FieldValue::Unset));
    assert_eq!(
        fixture.field(tag),
        Ok(FieldValue::Dynamic(DynamicValue::U32(9)))
    );

    assert_eq!(
        fixture.set_field(weight, FieldValue::Dynamic(DynamicValue::F32(-4.5))),
        Ok(())
    );
    assert_eq!(fixture.items.get(1).unwrap().weight, -4.5);
    assert_eq!(fixture.items.get(0).unwrap().weight, 1.0);
    assert_eq!(
        fixture.set_field(
            rotation,
            FieldValue::Dynamic(DynamicValue::Vec4([0.0, 0.0, 0.0, 1.0]))
        ),
        Ok(())
    );
    assert_eq!(
        fixture.items.get(1).unwrap().rotation,
        Some([0.0, 0.0, 0.0, 1.0])
    );

    // The layout supplies the expected kind; values are checked like dynamic ones.
    assert_eq!(
        fixture.set_field(weight, FieldValue::Dynamic(DynamicValue::U32(1))),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        fixture.set_field(weight, FieldValue::Dynamic(DynamicValue::F32(f32::NAN))),
        Err(FieldError::NonFinite)
    );
    assert_eq!(
        fixture.set_field(weight, FieldValue::F32(1.0)),
        Err(FieldError::WrongType)
    );

    // Unset clears optional properties only.
    assert_eq!(fixture.set_field(offset, FieldValue::Unset), Ok(()));
    assert_eq!(fixture.items.get(1).unwrap().offset, None);
    assert_eq!(fixture.field(offset), Ok(FieldValue::Unset));
    assert_eq!(
        fixture.set_field(weight, FieldValue::Unset),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        fixture.set_field(tag, FieldValue::Unset),
        Err(FieldError::WrongType)
    );

    // Dead, unallocated and undeclared regions reject reads and writes.
    fixture.items.remove(0);
    for dead in [item_offset(0, WEIGHT), item_offset(2, WEIGHT), 0x3000_0000] {
        assert_eq!(fixture.field(dead), Err(FieldError::UnknownField));
        assert_eq!(
            fixture.set_field(dead, FieldValue::Dynamic(DynamicValue::F32(1.0))),
            Err(FieldError::UnknownField)
        );
    }
    assert!(RowsFixture::has_field(item_offset(0, WEIGHT)));
    assert!(RowsFixture::has_field(tag + 1_000));
    assert!(!RowsFixture::has_field(0x3000_0000));
    assert!(!RowsFixture::has_field(
        0x1000_0000 + Rows::<RowsFixtureItem>::MAX_SLOTS * RowsFixtureItem::LAYOUT.property_count()
    ));
}

#[test]
fn static_validation_accepts_dynamic_and_optional_unset_at_row_addresses() {
    let weight = item_offset(7, WEIGHT);
    let offset = item_offset(7, OFFSET);
    let items = offset_of!(RowsFixture, items) as u32;
    assert_eq!(
        RowsFixture::validate_field(weight, FieldKind::Dynamic),
        Ok(())
    );
    assert_eq!(
        RowsFixture::validate_field(offset, FieldKind::Unset),
        Ok(())
    );
    assert_eq!(
        RowsFixture::validate_field(weight, FieldKind::Unset),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        RowsFixture::validate_field(weight, FieldKind::F32),
        Err(FieldError::WrongType)
    );
    assert_eq!(RowsFixture::validate_field(items, FieldKind::Rows), Ok(()));
    assert_eq!(
        RowsFixture::validate_field(items, FieldKind::Bytes),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        ComponentValue::validate_field(ComponentValue::ROWS_FIXTURE, offset, FieldKind::Unset),
        Ok(())
    );
    assert!(ComponentValue::has_field(
        ComponentValue::ROWS_FIXTURE,
        weight
    ));
}

#[test]
fn each_rows_field_appears_once_at_its_real_offset_and_round_trips() {
    let mut fixture = fixture();
    fixture.items.remove(0);
    fixture.items.get_mut(1).unwrap().texture = Some(AssetSource {
        kind: AssetTypeId(3),
        uri: "textures/a.png".into(),
        variant: 2,
    });
    fixture.items.get_mut(1).unwrap().mark = Some(0.5);

    let fields = fixture.fields();
    assert_eq!(fields.len(), RowsFixture::FIELD_COUNT);
    let offsets: Vec<_> = fields.iter().map(|(offset, _)| *offset).collect();
    assert_eq!(
        offsets,
        [
            offset_of!(RowsFixture, marker) as u32,
            offset_of!(RowsFixture, items) as u32,
            offset_of!(RowsFixture, tags) as u32,
        ]
    );
    assert!(
        offsets
            .iter()
            .all(|offset| *offset < std::mem::size_of::<RowsFixture>() as u32)
    );

    let mut restored = RowsFixture::default();
    for (offset, value) in fields {
        restored.set_field(offset, value).unwrap();
    }
    assert_eq!(restored, fixture);
    assert_eq!(restored.items.next_slot(), 2);
    assert_eq!(fixture.items.slot_state(0), RowSlotState::Dead);
    assert_eq!(restored.items.slot_state(0), RowSlotState::Unallocated);

    let mut sources = Vec::new();
    restored.visit_row_assets(&mut |asset| sources.push(asset.uri.to_string()));
    assert_eq!(sources, ["textures/a.png"]);
    assert!(fixture.retained_bytes().unwrap() >= "textures/a.png".len());
}

#[test]
fn table_encoding_matches_the_documented_layout() {
    let mut rows = Rows::<RowsFixtureItem>::new();
    rows.insert(2, item(1.5)).unwrap();

    let mut expected = Vec::new();
    expected.extend(3u32.to_le_bytes()); // next slot
    expected.extend(1u32.to_le_bytes()); // live rows
    expected.extend(2u32.to_le_bytes()); // slot
    expected.extend([0b0101_0111, 0b0000_0000]); // weight, count, offset, enabled, delta
    expected.extend(1.5f32.to_le_bytes());
    expected.extend(3u32.to_le_bytes());
    for lane in [1.0f32, 2.0, 3.0] {
        expected.extend(lane.to_le_bytes());
    }
    expected.extend(0u32.to_le_bytes()); // enabled = false
    expected.extend(0i32.to_le_bytes());
    assert_eq!(rows.encode(), expected);
    assert_eq!(Rows::<RowsFixtureItem>::decode(&expected), Ok(rows));
}

#[test]
fn field_asset_visitation_ignores_unrelated_tables() {
    let mut value = fixture();
    value.items.get_mut(0).unwrap().texture = Some(AssetSource {
        kind: crate::services::asset_management::AssetTypeId(2),
        uri: "asset://2/42".into(),
        variant: 0,
    });
    let mut sources = Vec::new();
    value.visit_row_field_assets(offset_of!(RowsFixture, tags) as u32, &mut |source| {
        sources.push(source.uri.to_string());
    });
    assert!(sources.is_empty());
    value.visit_row_field_assets(offset_of!(RowsFixture, items) as u32, &mut |source| {
        sources.push(source.uri.to_string());
    });
    assert_eq!(sources, ["asset://2/42"]);

    let mut value = ComponentValue::RowsFixture(value);
    let mut sources = Vec::new();
    value.visit_row_field_assets(offset_of!(RowsFixture, tags) as u32, &mut |source| {
        sources.push(source.uri.to_string());
    });
    assert!(sources.is_empty());
    value.visit_row_field_assets(offset_of!(RowsFixture, items) as u32, &mut |source| {
        sources.push(source.uri.to_string());
    });
    assert_eq!(sources, ["asset://2/42"]);

    if let ComponentValue::RowsFixture(value) = &mut value {
        value
            .items
            .get_mut(0)
            .unwrap()
            .texture
            .as_mut()
            .unwrap()
            .uri = "asset://malformed".into();
    }
    crate::components::registry::write(
        &mut value,
        &crate::FieldWrite {
            offset: offset_of!(RowsFixture, tags) as u32,
            value: crate::FieldValue::Rows(Rows::<RowsFixtureTag>::new().encode()),
        },
    )
    .unwrap();
    assert_eq!(
        crate::components::registry::validate_asset_references(&value),
        Err(crate::ErrorReason::InvalidAsset)
    );
}

#[test]
fn table_decoding_rejects_malformed_tables() {
    let mut rows = Rows::<RowsFixtureTag>::new();
    rows.push(RowsFixtureTag {
        value: 1,
    })
    .unwrap();
    rows.push(RowsFixtureTag {
        value: 2,
    })
    .unwrap();
    let valid = rows.encode();
    assert_eq!(Rows::<RowsFixtureTag>::decode(&valid), Ok(rows));

    let table = |next: u32, rows: &[(u32, u8, u32)]| {
        let mut bytes = Vec::new();
        bytes.extend(next.to_le_bytes());
        bytes.extend((rows.len() as u32).to_le_bytes());
        for (slot, mask, value) in rows {
            bytes.extend(slot.to_le_bytes());
            bytes.push(*mask);
            if mask & 1 != 0 {
                bytes.extend(value.to_le_bytes());
            }
        }
        bytes
    };
    let decode = |bytes: &[u8]| Rows::<RowsFixtureTag>::decode(bytes);

    assert!(decode(&table(2, &[(0, 1, 1), (1, 1, 2)])).is_ok());
    assert!(
        decode(&table(1, &[(0, 1, 1), (1, 1, 2)])).is_err(),
        "slot beyond next"
    );
    assert!(
        decode(&table(3, &[(1, 1, 1), (0, 1, 2)])).is_err(),
        "unordered slots"
    );
    assert!(
        decode(&table(3, &[(1, 1, 1), (1, 1, 2)])).is_err(),
        "duplicate slots"
    );
    assert!(
        decode(&table(1, &[(0, 0, 0)])).is_err(),
        "missing required property"
    );
    assert!(
        decode(&table(1, &[(0, 3, 1)])).is_err(),
        "undeclared mask bit"
    );
    assert!(
        decode(&table(u32::MAX, &[])).is_err(),
        "unaddressable next slot"
    );
    assert!(decode(&[0; 7]).is_err(), "truncated header");

    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode(&trailing).is_err(), "trailing bytes");

    let mut oversized = table(1, &[]);
    oversized[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode(&oversized).is_err(), "row count exceeds payload");

    let mut items = Rows::<RowsFixtureItem>::new();
    items.push(item(1.0)).unwrap();
    let mut nonfinite = items.encode();
    nonfinite[14..18].copy_from_slice(&f32::NAN.to_le_bytes());
    assert_eq!(
        Rows::<RowsFixtureItem>::decode(&nonfinite),
        Err(FieldError::NonFinite)
    );
    let mut boolean = items.encode();
    let enabled = boolean.len() - 8;
    boolean[enabled] = 2;
    assert!(Rows::<RowsFixtureItem>::decode(&boolean).is_err());
}

#[test]
fn contract_stream_covers_region_base_and_layout_order() {
    #[derive(Default, SchemaRow)]
    struct Forward {
        first: f32,
        second: Option<u32>,
    }

    #[derive(Default, SchemaRow)]
    struct Reversed {
        second: Option<u32>,
        first: f32,
    }

    let mut forward = Vec::new();
    Rows::<Forward>::write_row_contract(1, &mut forward);
    let mut expected = Vec::new();
    expected.extend(0x2000_0000u32.to_le_bytes());
    expected.extend(2u16.to_le_bytes());
    for (name, kind, optional) in [("first", 1u8, 0u8), ("second", 3, 1)] {
        expected.extend((name.len() as u32).to_le_bytes());
        expected.extend(name.as_bytes());
        expected.extend([kind, optional, 0]);
    }
    assert_eq!(forward, expected);

    let hash = |write: fn(&mut ContractHash)| {
        let mut hash = ContractHash::default();
        write(&mut hash);
        hash.0
    };
    assert_ne!(
        hash(|sink| Rows::<Forward>::write_row_contract(0, sink)),
        hash(|sink| Rows::<Reversed>::write_row_contract(0, sink)),
    );

    let mut contract = Vec::new();
    RowsFixture::write_contract(&mut contract);
    let mut items = Vec::new();
    Rows::<RowsFixtureItem>::write_row_contract(0, &mut items);
    assert!(contract.windows(items.len()).any(|window| window == items));
}

#[test]
fn registry_dispatch_reaches_rows_and_keeps_unknown_offsets_distinct() {
    let mut component = ComponentValue::RowsFixture(fixture());
    let weight = item_offset(0, WEIGHT);
    assert_eq!(
        component.field(weight),
        Ok(FieldValue::Dynamic(DynamicValue::F32(1.0)))
    );
    assert_eq!(
        component.set_field(weight, FieldValue::Dynamic(DynamicValue::F32(8.0))),
        Ok(())
    );
    assert_eq!(
        component.field(item_offset(5, COUNT)),
        Err(FieldError::UnknownField)
    );
    assert_eq!(component.fields().len(), 3);

    let texture = item_offset(0, TEXTURE);
    let asset = AssetSource {
        kind: AssetTypeId(1),
        uri: "a".into(),
        variant: 0,
    };
    assert_eq!(
        component.set_field(
            texture,
            FieldValue::Dynamic(DynamicValue::Asset(asset.clone()))
        ),
        Ok(())
    );
    let mut demand = std::collections::BTreeSet::new();
    component.resource_demand(&mut demand);
    assert_eq!(
        demand
            .into_iter()
            .map(|selection| selection.descriptor())
            .collect::<Vec<_>>(),
        [asset]
    );
}

#[test]
fn text_properties_travel_as_bounded_string_values() {
    let mut fixture = fixture();
    let label = item_offset(1, LABEL);
    let weight = item_offset(1, WEIGHT);
    assert_eq!(fixture.field(label), Ok(FieldValue::Unset));

    // Sixteen UTF-8 bytes fit the bound even though they are fewer characters.
    let full = "ünïcødé✓ab";
    assert_eq!(full.len(), 16);
    assert_eq!(
        fixture.set_field(label, FieldValue::String(full.into())),
        Ok(())
    );
    assert_eq!(fixture.field(label), Ok(FieldValue::String(full.into())));
    assert_eq!(fixture.items.get(1).unwrap().label.as_deref(), Some(full));
    assert_eq!(
        fixture.set_field(label, FieldValue::String(format!("{full}!").into())),
        Err(FieldError::TextTooLong)
    );
    assert_eq!(fixture.items.get(1).unwrap().label.as_deref(), Some(full));

    // Text uses the string value kind only, and only at text properties.
    assert_eq!(
        fixture.set_field(label, FieldValue::Dynamic(DynamicValue::Text("a".into()))),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        fixture.set_field(label, FieldValue::Dynamic(DynamicValue::F32(1.0))),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        fixture.set_field(weight, FieldValue::String("1".into())),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        RowsFixture::validate_field(label, FieldKind::String),
        Ok(())
    );
    assert_eq!(
        RowsFixture::validate_field(label, FieldKind::Dynamic),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        RowsFixture::validate_field(weight, FieldKind::String),
        Err(FieldError::WrongType)
    );

    // The row API carries text as a row-only dynamic value with the same bound.
    assert_eq!(
        fixture.items.property(1, LABEL),
        Ok(Some(DynamicValue::Text(full.into())))
    );
    assert_eq!(
        fixture
            .items
            .set_property(1, LABEL, DynamicValue::Text("x".repeat(17).into())),
        Err(FieldError::TextTooLong)
    );
    assert_eq!(fixture.set_field(label, FieldValue::Unset), Ok(()));
    assert_eq!(fixture.field(label), Ok(FieldValue::Unset));
}

#[test]
fn required_text_rejects_clearing_and_derives_its_bound() {
    #[derive(Debug, Default, PartialEq, SchemaRow)]
    struct Named {
        #[schema(text = 4)]
        name: Arc<str>,
    }

    assert_eq!(Named::LAYOUT.properties[0].kind, DynamicPropertyKind::Text);
    assert!(!Named::LAYOUT.properties[0].optional);
    assert_eq!(Named::LAYOUT.properties[0].max_bytes, 4);

    let mut rows = Rows::<Named>::new();
    rows.push(Named::default()).unwrap();
    assert_eq!(
        rows.set_row_field(0, FieldValue::String("four".into())),
        Ok(())
    );
    assert_eq!(&*rows.get(0).unwrap().name, "four");
    assert_eq!(
        rows.set_row_field(0, FieldValue::String("fives".into())),
        Err(FieldError::TextTooLong)
    );
    assert_eq!(
        rows.set_row_field(0, FieldValue::Unset),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        Rows::<Named>::validate_row_field(0, FieldKind::Unset),
        Err(FieldError::WrongType)
    );

    // A required text property is always present, so an empty string encodes.
    let mut empty = Rows::<Named>::new();
    empty.push(Named::default()).unwrap();
    assert_eq!(Rows::<Named>::decode(&empty.encode()), Ok(empty));
}

#[test]
fn text_encodes_as_length_and_utf8_and_decoding_checks_both() {
    let mut rows = Rows::<RowsFixtureItem>::new();
    rows.push(RowsFixtureItem {
        label: Some("hé".into()),
        ..RowsFixtureItem::default()
    })
    .unwrap();

    let mut expected = Vec::new();
    expected.extend(1u32.to_le_bytes()); // next slot
    expected.extend(1u32.to_le_bytes()); // live rows
    expected.extend(0u32.to_le_bytes()); // slot
    expected.extend([0b0101_0011, 0b0000_0010]); // weight, count, enabled, delta, label
    expected.extend(0f32.to_le_bytes());
    expected.extend(0u32.to_le_bytes());
    expected.extend(0u32.to_le_bytes());
    expected.extend(0i32.to_le_bytes());
    expected.extend(3u32.to_le_bytes());
    expected.extend("hé".as_bytes());
    assert_eq!(rows.encode(), expected);
    assert_eq!(Rows::<RowsFixtureItem>::decode(&expected), Ok(rows));

    let with_label = |label: &[u8]| {
        let mut bytes = expected[..expected.len() - 7].to_vec();
        bytes.extend((label.len() as u32).to_le_bytes());
        bytes.extend(label);
        Rows::<RowsFixtureItem>::decode(&bytes)
    };
    assert!(with_label(b"sixteen bytes ok").is_ok());
    assert_eq!(
        with_label(b"seventeen bytes!!"),
        Err(FieldError::TextTooLong)
    );
    assert_eq!(with_label(&[0xff, 0xfe]), Err(FieldError::WrongType));

    let mut truncated = expected.clone();
    truncated.pop();
    assert!(Rows::<RowsFixtureItem>::decode(&truncated).is_err());

    // A length beyond any text bound is rejected before the bytes are read.
    let mut huge = expected[..expected.len() - 7].to_vec();
    huge.extend(u32::MAX.to_le_bytes());
    assert_eq!(
        Rows::<RowsFixtureItem>::decode(&huge),
        Err(FieldError::TextTooLong)
    );
}

#[test]
fn retained_bytes_count_text_length() {
    let mut rows = Rows::<RowsFixtureItem>::new();
    rows.push(RowsFixtureItem::default()).unwrap();
    let without = rows.retained_bytes();

    rows.get_mut(0).unwrap().label = Some("label".into());
    assert_eq!(rows.retained_bytes(), without + "label".len());
    assert_eq!(rows.get(0).unwrap().retained_bytes(), "label".len());
}

#[test]
fn contract_stream_carries_the_text_bound_after_the_hint() {
    #[derive(Default, SchemaRow)]
    struct Titled {
        #[schema(text = 300)]
        title: Option<Arc<str>>,
    }

    let mut stream = Vec::new();
    Rows::<Titled>::write_row_contract(0, &mut stream);
    let mut expected = Vec::new();
    expected.extend(0x1000_0000u32.to_le_bytes());
    expected.extend(1u16.to_le_bytes());
    expected.extend(5u32.to_le_bytes());
    expected.extend(b"title");
    expected.extend([DynamicPropertyKind::Text as u8, 1, 0]);
    expected.extend(300u32.to_le_bytes());
    assert_eq!(stream, expected);
}

#[test]
fn text_is_row_only() {
    let text = DynamicValue::Text("row".into());
    let mut properties = crate::components::dynamic_properties::DynamicProperties::default();
    assert_eq!(
        properties.set("title", text.clone()),
        Err(FieldError::WrongType)
    );
    assert!(properties.descriptors().is_empty());
    assert_eq!(
        DynamicPropertyKind::from_tag(DynamicPropertyKind::Text as u8),
        Err(FieldError::WrongType)
    );
    assert_eq!(
        DynamicValue::decode(&text.encode()),
        Err(FieldError::WrongType)
    );
}

fn tags(count: u32) -> Rows<RowsFixtureTag> {
    let mut rows = Rows::new();
    for value in 0..count {
        rows.push(RowsFixtureTag {
            value,
        })
        .unwrap();
    }

    rows
}

#[test]
fn batch_removal_returns_live_rows_in_slot_order_and_kills_their_slots() {
    let mut rows = tags(8);
    rows.remove(1);
    let capacity = rows.rows.capacity();

    // Unsorted and repeated slots, a dead slot, an unallocated slot and an
    // unaddressable slot: only live slots are removed.
    let removed = rows.remove_slots(&[6, 2, 6, 1, 9, u32::MAX, 4]);
    assert_eq!(
        removed.iter().map(|row| row.value).collect::<Vec<_>>(),
        [2, 4, 6]
    );
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter().map(|(slot, _)| slot).collect::<Vec<_>>(),
        [0, 3, 5, 7]
    );
    for dead in [1, 2, 4, 6] {
        assert_eq!(rows.slot_state(dead), RowSlotState::Dead);
        assert_eq!(
            rows.insert(
                dead,
                RowsFixtureTag {
                    value: 0
                }
            ),
            Err(FieldError::UnknownField)
        );
    }
    assert_eq!(rows.slot_state(9), RowSlotState::Unallocated);
    assert_eq!(rows.dead, [1, 2, 4, 6]);
    assert_eq!(rows.next_slot(), 8);

    // Row storage is kept for later growth; the dead record holds the new slots.
    assert_eq!(rows.rows.capacity(), capacity);
    assert_eq!(
        rows.retained_bytes(),
        capacity * std::mem::size_of::<(u32, RowsFixtureTag)>()
            + rows.dead.capacity() * std::mem::size_of::<u32>()
    );
}

#[test]
fn batch_removal_handles_empty_and_complete_requests() {
    let mut rows = tags(4);
    assert!(rows.remove_slots(&[]).is_empty());
    assert_eq!(rows.len(), 4);
    assert!(rows.remove_slots(&[7, 8]).is_empty());
    assert!(rows.dead.is_empty());

    let removed = rows.remove_slots(&[3, 2, 1, 0]);
    assert_eq!(
        removed.iter().map(|row| row.value).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert!(rows.is_empty());
    assert_eq!(rows.dead, [0, 1, 2, 3]);
    assert_eq!(
        rows.push(RowsFixtureTag {
            value: 4
        }),
        Ok(4)
    );
    assert!(rows.remove_slots(&[0, 1, 2, 3]).is_empty());
}

#[test]
fn batch_removal_scales_with_the_table_rather_than_per_slot() {
    const ROWS: u32 = 16_384;
    let half: Vec<u32> = (0..ROWS).step_by(2).collect();
    let table = || {
        let mut rows = Rows::<RowsFixtureItem>::new();
        for slot in 0..ROWS {
            rows.push(item(slot as f32)).unwrap();
        }

        rows
    };

    let mut single = table();
    let started = std::time::Instant::now();
    for slot in &half {
        single.remove(*slot);
    }
    let per_slot = started.elapsed();

    let mut batch = table();
    let started = std::time::Instant::now();
    let removed = batch.remove_slots(&half);
    let batched = started.elapsed();

    assert_eq!(removed.len(), half.len());
    assert_eq!(batch, single);
    assert_eq!(batch.dead, single.dead);
    // Per-slot removal shifts the table once per slot; one pass is far cheaper.
    assert!(
        batched * 20 < per_slot,
        "batch {batched:?} is not far below per-slot {per_slot:?}"
    );
}

mod world {
    use super::*;
    use crate::world::WorldLimits;
    use crate::{
        Batch, BatchOutcome, Command, EntityId, EntityMetadata, EntityRef, HostRuntime, WorldId,
    };

    fn run(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
        let mut world = host.world_mut(world).unwrap();
        world
            .enqueue(Batch {
                id: world.tick() + 1,
                operations,
            })
            .unwrap();
        world.step(0.0).unwrap().outcomes.remove(0)
    }

    fn set(entity: EntityId, offset: u32, value: crate::FieldValue) -> Command {
        Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::ROWS_FIXTURE,
            field: crate::FieldWrite {
                offset,
                value,
            },
        }
    }

    fn state(host: &mut HostRuntime, world: WorldId, entity: EntityId) -> RowsFixture {
        host.world_mut(world)
            .unwrap()
            .inspect(entity)
            .unwrap()
            .components
            .into_iter()
            .find_map(|value| match value {
                ComponentValue::RowsFixture(value) => Some(value),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn private_row_assignments_validate_only_the_final_table() {
        use crate::components::Scalar;
        use crate::systems::gui::motion::{GuiMotionPart, GuiThemeMotion};

        let mut host = HostRuntime::new();
        let world = host
            .create_world(
                WorldLimits::default(),
                &[
                    crate::systems::animation::AnimationSystem::ID,
                    crate::systems::constraints::ConstraintSystem::ID,
                    crate::systems::asset_dependencies::AssetDependencySystem::ID,
                    crate::systems::gui::GuiSystem::ID,
                ],
            )
            .unwrap();
        let table = |easing: u32| {
            let mut parts = Rows::new();
            parts
                .push(GuiMotionPart {
                    duration: Some(0.1),
                    easing: Some(easing),
                    ..Default::default()
                })
                .unwrap();
            parts
        };
        let invalid = table(7);
        let valid = table(2);
        let field = |parts: &Rows<GuiMotionPart>| crate::FieldWrite {
            offset: offset_of!(GuiThemeMotion, parts) as u32,
            value: crate::FieldValue::Rows(parts.encode()),
        };
        let created = run(
            &mut host,
            world,
            vec![Command::Create {
                alias: 0,
                metadata: Default::default(),
                adopt: false,
            }],
        );
        let entity = created.result.unwrap()[0].1;
        let insert = |fields| Command::InsertComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_THEME_MOTION,
            fields,
            adopt: false,
        };
        let accepted = run(
            &mut host,
            world,
            vec![insert(vec![field(&invalid), field(&valid)])],
        );
        assert!(accepted.result.is_ok(), "{accepted:?}");
        let expected = ComponentValue::GuiThemeMotion(GuiThemeMotion {
            parts: valid.clone(),
        });
        assert!(
            host.world_mut(world)
                .unwrap()
                .inspect(entity)
                .unwrap()
                .components
                .contains(&expected)
        );

        let rejected = run(
            &mut host,
            world,
            vec![
                Command::insert_value(
                    EntityRef::Handle(entity),
                    ComponentValue::Scalar(Scalar {
                        value: 7.0,
                    }),
                ),
                insert(vec![field(&valid), field(&invalid)]),
                Command::insert_value(
                    EntityRef::Handle(entity),
                    ComponentValue::Scalar(Scalar {
                        value: 99.0,
                    }),
                ),
            ],
        );
        assert_eq!(rejected.result.unwrap_err().operation, Some(1));
        let snapshot = host.world_mut(world).unwrap().inspect(entity).unwrap();
        assert!(snapshot.components.contains(&expected));
        assert!(
            snapshot
                .components
                .contains(&ComponentValue::Scalar(Scalar {
                    value: 7.0
                }))
        );
        let write = |parts: &Rows<GuiMotionPart>| Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_THEME_MOTION,
            field: field(parts),
        };
        let rejected = run(&mut host, world, vec![write(&invalid), write(&valid)]);
        assert_eq!(rejected.result.unwrap_err().operation, Some(0));
        assert!(
            host.world_mut(world)
                .unwrap()
                .inspect(entity)
                .unwrap()
                .components
                .contains(&expected)
        );
        assert!(run(&mut host, world, vec![write(&valid)]).result.is_ok());
    }

    #[test]
    fn malformed_row_assets_reject_without_poisoning_demand_or_losing_prior_writes() {
        let mut host = HostRuntime::new();
        let world = host
            .create_world(
                WorldLimits::default(),
                &[
                    crate::systems::animation::AnimationSystem::ID,
                    crate::systems::asset_dependencies::AssetDependencySystem::ID,
                ],
            )
            .unwrap();
        let mut rows = Rows::new();
        rows.insert(5, item(1.0)).unwrap();
        let created = run(
            &mut host,
            world,
            vec![
                Command::Create {
                    alias: 0,
                    metadata: EntityMetadata::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(0),
                    ComponentValue::RowsFixture(RowsFixture {
                        items: rows,
                        ..RowsFixture::default()
                    }),
                ),
            ],
        );
        let entity = created.result.unwrap()[0].1;
        let rejected = run(
            &mut host,
            world,
            vec![
                set(
                    entity,
                    item_offset(5, WEIGHT),
                    crate::FieldValue::Dynamic(DynamicValue::F32(2.0)),
                ),
                set(
                    entity,
                    item_offset(5, TEXTURE),
                    crate::FieldValue::Dynamic(DynamicValue::Asset(AssetSource {
                        kind: crate::TEXTURE_TYPE,
                        uri: "asset://ordinary-motion-A".into(),
                        variant: 0,
                    })),
                ),
                set(
                    entity,
                    item_offset(5, WEIGHT),
                    crate::FieldValue::Dynamic(DynamicValue::F32(3.0)),
                ),
            ],
        );
        assert!(rejected.result.is_err(), "{rejected:?}");
        assert_eq!(rejected.result.as_ref().unwrap_err().operation, Some(1));
        let stored = state(&mut host, world, entity);
        assert_eq!(stored.items.get(5).unwrap().weight, 2.0);
        assert_eq!(stored.items.get(5).unwrap().texture, None);
        assert!(
            host.world_mut(world)
                .unwrap()
                .resource_snapshots()
                .is_empty()
        );

        let corrected = run(
            &mut host,
            world,
            vec![set(
                entity,
                item_offset(5, TEXTURE),
                crate::FieldValue::Dynamic(DynamicValue::Asset(AssetSource {
                    kind: crate::TEXTURE_TYPE,
                    uri: "asset://2/42".into(),
                    variant: 0,
                })),
            )],
        );
        assert!(corrected.result.is_ok(), "{corrected:?}");
        assert_eq!(host.world_mut(world).unwrap().resource_snapshots().len(), 1);

        let mut invalid = state(&mut host, world, entity);
        invalid
            .items
            .get_mut(5)
            .unwrap()
            .texture
            .as_mut()
            .unwrap()
            .uri = "asset://malformed".into();
        for command in [
            set(
                entity,
                offset_of!(RowsFixture, items) as u32,
                crate::FieldValue::Rows(invalid.items.encode()),
            ),
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::RowsFixture(invalid),
            ),
        ] {
            let rejected = run(&mut host, world, vec![command]);
            assert!(rejected.result.is_err(), "{rejected:?}");
            assert_eq!(
                &*state(&mut host, world, entity)
                    .items
                    .get(5)
                    .unwrap()
                    .texture
                    .as_ref()
                    .unwrap()
                    .uri,
                "asset://2/42"
            );
            assert_eq!(host.world_mut(world).unwrap().resource_snapshots().len(), 1);
        }
    }

    #[test]
    fn world_writes_address_live_row_properties_by_offset() {
        let mut host = HostRuntime::new();
        let world = host.create_world(WorldLimits::default(), &[]).unwrap();
        let mut table = Rows::<RowsFixtureItem>::new();
        table.push(item(1.0)).unwrap();
        table.push(item(2.0)).unwrap();
        table.remove(0);

        let outcome = run(
            &mut host,
            world,
            vec![
                Command::Create {
                    alias: 1,
                    metadata: EntityMetadata {
                        symbolic_id: Some("rows".into()),
                        classes: vec![],
                    },
                    adopt: false,
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::ROWS_FIXTURE,
                    fields: vec![crate::FieldWrite {
                        offset: offset_of!(RowsFixture, items) as u32,
                        value: crate::FieldValue::Rows(table.encode()),
                    }],
                    adopt: false,
                },
            ],
        );
        let entity = outcome.result.unwrap()[0].1;

        let outcome = run(
            &mut host,
            world,
            vec![
                set(
                    entity,
                    item_offset(1, WEIGHT),
                    crate::FieldValue::Dynamic(DynamicValue::F32(6.0)),
                ),
                set(entity, item_offset(1, OFFSET), crate::FieldValue::Unset),
            ],
        );
        assert!(outcome.result.is_ok());
        let stored = state(&mut host, world, entity);
        assert_eq!(stored.items.get(1).unwrap().weight, 6.0);
        assert_eq!(stored.items.get(1).unwrap().offset, None);

        for rejected in [
            set(
                entity,
                item_offset(0, WEIGHT),
                crate::FieldValue::Dynamic(DynamicValue::F32(1.0)),
            ),
            set(entity, item_offset(1, WEIGHT), crate::FieldValue::Unset),
            set(
                entity,
                item_offset(1, COUNT),
                crate::FieldValue::Dynamic(DynamicValue::F32(1.0)),
            ),
        ] {
            assert!(run(&mut host, world, vec![rejected]).result.is_err());
        }

        // A symbolic reference addresses the same row property by offset.
        let outcome = run(
            &mut host,
            world,
            vec![Command::SetField {
                entity: EntityRef::Symbol("rows".into()),
                component: ComponentValue::ROWS_FIXTURE,
                field: crate::FieldWrite {
                    offset: item_offset(1, ROTATION),
                    value: crate::FieldValue::Dynamic(DynamicValue::Vec4([0.0, 1.0, 0.0, 0.0])),
                },
            }],
        );
        assert!(outcome.result.is_ok(), "{:?}", outcome.result);
        let stored = state(&mut host, world, entity);
        assert_eq!(
            stored.items.get(1).unwrap().rotation,
            Some([0.0, 1.0, 0.0, 0.0])
        );
        assert_eq!(stored.items.get(1).unwrap().weight, 6.0);
    }

    #[test]
    fn text_rows_are_written_bounded_and_never_animated() {
        use crate::ErrorReason;
        use crate::systems::animation::{
            AnimationControllerDescription, AnimationDriverDescription, AnimationProperty,
            AnimationTrackTarget,
        };

        let mut host = HostRuntime::new();
        let world = host
            .create_world(
                WorldLimits::default(),
                &[
                    crate::systems::animation::AnimationSystem::ID,
                    crate::systems::asset_dependencies::AssetDependencySystem::ID,
                ],
            )
            .unwrap();
        let mut table = Rows::<RowsFixtureItem>::new();
        table.push(item(1.0)).unwrap();
        let outcome = run(
            &mut host,
            world,
            vec![
                Command::Create {
                    alias: 1,
                    metadata: EntityMetadata {
                        symbolic_id: Some("text".into()),
                        classes: vec![],
                    },
                    adopt: false,
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::ROWS_FIXTURE,
                    fields: vec![crate::FieldWrite {
                        offset: offset_of!(RowsFixture, items) as u32,
                        value: crate::FieldValue::Rows(table.encode()),
                    }],
                    adopt: false,
                },
            ],
        );
        let entity = outcome.result.unwrap()[0].1;
        let label = item_offset(0, LABEL);
        let text = |value: &str| crate::FieldValue::String(value.into());

        let outcome = run(&mut host, world, vec![set(entity, label, text("base"))]);
        assert!(outcome.result.is_ok(), "{:?}", outcome.result);
        assert_eq!(
            state(&mut host, world, entity).items.get(0).unwrap().label,
            Some("base".into())
        );

        // Over-long text is an invalid value; text in a dynamic payload or at a
        // numeric property is an invalid field.
        for (write, reason) in [
            (text(&"x".repeat(17)), ErrorReason::InvalidValue),
            (
                crate::FieldValue::Dynamic(DynamicValue::Text("dynamic".into())),
                ErrorReason::InvalidField,
            ),
        ] {
            let outcome = run(&mut host, world, vec![set(entity, label, write)]);
            assert_eq!(outcome.result.unwrap_err().reason, reason);
        }
        let outcome = run(
            &mut host,
            world,
            vec![set(entity, item_offset(0, WEIGHT), text("1"))],
        );
        assert_eq!(
            outcome.result.unwrap_err().reason,
            ErrorReason::InvalidField
        );

        // A rejected write leaves the stored text unchanged.
        assert_eq!(
            state(&mut host, world, entity).items.get(0).unwrap().label,
            Some("base".into())
        );

        // Animation binds numeric row properties but rejects the text property.
        let description = |offset| AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: "asset://10/1".into(),
                variant: 0,
                track: 0,
                target: entity,
                property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: ComponentValue::ROWS_FIXTURE,
                    offsets: vec![offset],
                }),
                entity_bindings: Vec::new(),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            ..Default::default()
        };
        let mut world = host.world_mut(world).unwrap();
        assert!(
            world
                .create_animation_controller(description(item_offset(0, WEIGHT)))
                .is_ok()
        );
        assert_eq!(
            world.create_animation_controller(description(label)).err(),
            Some(ErrorReason::InvalidField)
        );
    }
}
