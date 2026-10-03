use super::*;
use crate::codec::Reader;
use ipp_core::{DynamicPropertyKind, DynamicValue, services::data::DataRowId};

#[test]
fn wire_budget_pages_actual_text_and_invalid_results_without_consuming_dirty() {
    let rows: Vec<_> = (1..=PAGE_ROWS as u64).map(DataRowId).collect();
    let values =
        vec![ExpressionResult::Valid(DynamicValue::Text("α".repeat(1000).into())); rows.len()];
    let invalid = vec![
        ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
            slot: 3
        });
        rows.len()
    ];
    let ready = DataBindingAvailability::Ready;
    let view = DataBindingView {
        source: None,
        binding_incarnation: 9,
        evaluated_tick: Some(7),
        availability: &ready,
        dirty: true,
        total_rows: rows.len(),
        offset: 0,
        next_offset: None,
        row_ids: &rows,
        columns: vec![
            DataBindingColumnView {
                name: "text",
                kind: DynamicPropertyKind::Text,
                values: &values,
            },
            DataBindingColumnView {
                name: "missing",
                kind: DynamicPropertyKind::F32,
                values: &invalid,
            },
        ],
    };
    let before = view.to_owned();
    let bytes = encode_view(&view).unwrap();
    assert!(bytes.len() <= PAGE_BYTES - 25);
    let mut reader = Reader {
        bytes: &bytes,
        at: 0,
    };
    assert_eq!(reader.u8().unwrap(), 0);
    assert_eq!(reader.u64().unwrap(), 0);
    assert_eq!(reader.u64().unwrap(), 9);
    assert_eq!(reader.u8().unwrap(), 1);
    assert_eq!(reader.u64().unwrap(), 7);
    for text in ["Ready", "", "", ""] {
        assert_eq!(reader.string().unwrap(), text);
    }
    assert_eq!(reader.u8().unwrap(), 1);
    assert_eq!(reader.u64().unwrap(), PAGE_ROWS as u64);
    assert_eq!(reader.u64().unwrap(), 0);
    assert_eq!(reader.u8().unwrap(), 1);
    let next = reader.u64().unwrap();
    assert!(next > 0 && next < PAGE_ROWS as u64);
    assert_eq!(reader.u32().unwrap(), 2);
    for (name, kind) in [
        ("text", DynamicPropertyKind::Text),
        ("missing", DynamicPropertyKind::F32),
    ] {
        assert_eq!(reader.string().unwrap(), name);
        assert_eq!(reader.u8().unwrap(), kind as u8);
    }
    assert_eq!(reader.u32().unwrap(), next as u32);
    for id in 1..=next {
        assert_eq!(reader.u64().unwrap(), id);
        assert_eq!(reader.u8().unwrap(), 1);
        assert_eq!(reader.u32().unwrap(), 1);
        assert_eq!(reader.u8().unwrap(), DynamicPropertyKind::Text as u8);
        assert_eq!(reader.string().unwrap(), "α".repeat(1000));
        assert_eq!(reader.u8().unwrap(), 0);
        assert_eq!(reader.string().unwrap(), "MissingInput");
        assert_eq!(reader.u8().unwrap(), 1);
        assert_eq!(reader.u64().unwrap(), 3);
    }
    assert_eq!(reader.at, bytes.len());
    assert_eq!(view.to_owned(), before);
    assert_eq!(encode_view(&view).unwrap(), bytes);
}

#[test]
fn oversized_single_row_refuses_instead_of_truncating_or_stalling() {
    let values = [ExpressionResult::Valid(DynamicValue::Text(
        "x".repeat(PAGE_BYTES - 64).into(),
    ))];
    let view = DataBindingView {
        source: None,
        binding_incarnation: 1,
        evaluated_tick: None,
        availability: &DataBindingAvailability::Ready,
        dirty: true,
        total_rows: 1,
        offset: 0,
        next_offset: None,
        row_ids: &[DataRowId(1)],
        columns: vec![DataBindingColumnView {
            name: "text",
            kind: DynamicPropertyKind::Text,
            values: &values,
        }],
    };
    assert!(matches!(
        encode_view(&view),
        Err(ProtocolError::Limit("binding page row"))
    ));
    assert!(view.dirty);
}
