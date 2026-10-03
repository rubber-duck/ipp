use super::*;
use ipp_core::components::{DynamicPropertyKind, DynamicValue};
use ipp_core::services::data::*;

#[test]
fn representation_decoding_preserves_later_domain_error_and_exact_integers() {
    let deltas = [
        DataDelta::Append {
            rows: vec![vec![
                DynamicValue::U32(u32::MAX),
                DynamicValue::I32(i32::MIN),
                DynamicValue::Text("raw α".into()),
                DynamicValue::Mat2([1., 2., 3., 4.]),
            ]],
        },
        DataDelta::Edit {
            row: DataRowId(1),
            values: vec![DynamicValue::F32(f32::NAN)],
        },
        DataDelta::Remove {
            row: DataRowId(1),
        },
    ];
    let bytes = encode_update(&deltas).unwrap();
    let decoded = decode_update(&bytes).unwrap();
    let mut service = DataService::new();
    let producer = service
        .create_source(
            "literal://Host/source".into(),
            DataSourceKind::Buffer,
            DataSchema {
                columns: vec![
                    DataColumn::new("unsigned", DynamicPropertyKind::U32),
                    DataColumn::new("signed", DynamicPropertyKind::I32),
                    DataColumn::text("label", 32),
                    DataColumn::new("matrix", DynamicPropertyKind::Mat2),
                ],
            },
        )
        .unwrap();
    let error = service.apply_batch(producer, decoded).unwrap_err();
    assert_eq!(error.delta_index, 1);
    assert_eq!(error.committed.committed_deltas, 1);
    assert_eq!(error.reason, DataError::InvalidRow);
    let view = service.read_source(producer.source()).unwrap();
    assert_eq!(
        view.rows().next().unwrap().values,
        match &deltas[0] {
            DataDelta::Append {
                rows,
            } => &rows[0],
            _ => unreachable!(),
        }
    );
}

#[test]
fn malformed_incomplete_and_excess_counts_never_decode_an_update() {
    let valid = encode_update(&[DataDelta::Append {
        rows: vec![vec![DynamicValue::F32(1.)]],
    }])
    .unwrap();
    for end in 0..valid.len() {
        assert!(decode_update(&valid[..end]).is_err());
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(decode_update(&trailing).is_err());
    assert!(decode_update(&u32::MAX.to_le_bytes()).is_err());
    assert!(decode_update(&vec![0; UPDATE_BYTES + 1]).is_err());
}

#[test]
fn pages_have_exact_types_stable_ids_and_explicit_record_bounds() {
    let mut service = DataService::new();
    let producer = service
        .create_source(
            "source".into(),
            DataSourceKind::Buffer,
            DataSchema {
                columns: vec![DataColumn::text("text", PAGE_BYTES * 2)],
            },
        )
        .unwrap();
    service
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: (0..10)
                    .map(|_| vec![DynamicValue::Text("a".repeat(12_000).into())])
                    .collect(),
            }],
        )
        .unwrap();
    let time = service.time();
    let first =
        DatasetPage::observe(service.read_source(producer.source()).unwrap(), 0, 128).unwrap();
    assert_eq!(
        first.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert_eq!(first.next_offset, Some(5));
    assert!(response(1, 2, &DatasetResponse::Page(first)).unwrap().len() <= PAGE_BYTES);
    let second =
        DatasetPage::observe(service.read_source(producer.source()).unwrap(), 5, 128).unwrap();
    assert_eq!(second.rows.len(), 5);
    assert_eq!(second.next_offset, None);
    assert_eq!(service.time(), time);
    service
        .apply_batch(
            producer,
            [DataDelta::Edit {
                row: DataRowId(1),
                values: vec![DynamicValue::Text("x".repeat(PAGE_BYTES).into())],
            }],
        )
        .unwrap();
    assert!(DatasetPage::observe(service.read_source(producer.source()).unwrap(), 0, 128).is_err());
}

#[test]
fn request_fences_and_chunk_bounds_are_independent_of_world_layout() {
    let bytes = encode_request(
        0x1234,
        7,
        &DatasetOperation::Chunk {
            transfer: 2,
            offset: 65_536,
            bytes: &[1, 2, 3],
        },
    )
    .unwrap();
    assert!(decode(&bytes, 0x1235).is_err());
    let decoded = decode(&bytes, 0x1234).unwrap();
    assert_eq!(decoded.id, 7);
    assert!(matches!(
        decoded.operation,
        DatasetOperation::Chunk {
            transfer: 2,
            offset: 65_536,
            bytes: [1, 2, 3]
        }
    ));
}
