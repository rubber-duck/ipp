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
            "weight", "count", "offset", "rotation", "enabled", "texture", "delta", "size", "mark"
        ]
    );
    assert_eq!(layout.property_count(), 9);
    assert_eq!(layout.mask_bytes(), 2);
    assert_eq!(
        layout.properties[3],
        RowProperty {
            name: "rotation",
            kind: DynamicPropertyKind::Vec4,
            optional: true,
            hint: RowPropertyHint::Rotation,
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
        0x1000_0000 + Rows::<RowsFixtureItem>::MAX_SLOTS * 9
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
    restored.visit_row_assets(&mut |asset| sources.push(asset.uri.clone()));
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

mod world {
    use super::*;
    use crate::world::WorldLimits;
    use crate::{
        Batch, BatchOutcome, Command, ComponentOverlayMode, EntityId, EntityMetadata,
        EntityOverlayMode, EntityRef, HostRuntime, StateOverlayRef, WorldId,
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

    fn state(
        host: &mut HostRuntime,
        world: WorldId,
        entity: EntityId,
    ) -> (RowsFixture, RowsFixture) {
        let snapshot = host.world_mut(world).unwrap().inspect(entity).unwrap();
        let find = |values: Vec<ComponentValue>| {
            values
                .into_iter()
                .find_map(|value| match value {
                    ComponentValue::RowsFixture(value) => Some(value),
                    _ => None,
                })
                .unwrap()
        };
        (find(snapshot.base), find(snapshot.effective))
    }

    #[test]
    fn world_writes_and_overlays_address_live_row_properties_by_offset() {
        let mut host = HostRuntime::new();
        let world = host.create_world(WorldLimits::default()).unwrap();
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
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::ROWS_FIXTURE,
                    fields: vec![crate::FieldWrite {
                        offset: offset_of!(RowsFixture, items) as u32,
                        value: crate::FieldValue::Rows(table.encode()),
                    }],
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
        let (base, _) = state(&mut host, world, entity);
        assert_eq!(base.items.get(1).unwrap().weight, 6.0);
        assert_eq!(base.items.get(1).unwrap().offset, None);

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

        let outcome = run(
            &mut host,
            world,
            vec![
                Command::CreateStateOverlayOwner {
                    alias: 1,
                },
                Command::AttachEntityOverlayBinding {
                    owner: StateOverlayRef::Alias(1),
                    alias: 2,
                    symbolic_id: "rows".into(),
                    mode: EntityOverlayMode::Bound,
                },
                Command::AttachComponentStateOverlay {
                    owner: StateOverlayRef::Alias(1),
                    binding: StateOverlayRef::Alias(2),
                    alias: 3,
                    component: ComponentValue::ROWS_FIXTURE,
                    mode: ComponentOverlayMode::Bound,
                    fields: vec![crate::FieldWrite {
                        offset: item_offset(1, ROTATION),
                        value: crate::FieldValue::Dynamic(DynamicValue::Vec4([0.0, 1.0, 0.0, 0.0])),
                    }],
                },
            ],
        );
        assert!(outcome.result.is_ok(), "{:?}", outcome.result);
        let (base, effective) = state(&mut host, world, entity);
        assert_eq!(base.items.get(1).unwrap().rotation, None);
        assert_eq!(
            effective.items.get(1).unwrap().rotation,
            Some([0.0, 1.0, 0.0, 0.0])
        );
        assert_eq!(effective.items.get(1).unwrap().weight, 6.0);
    }
}
