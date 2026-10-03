use super::{decode_data_windows, encode_data_windows};
use crate::{DynamicValue, ErrorReason, services::data::*};

#[test]
fn portable_windows_round_trip_all_numeric_anchors_and_intersections() {
    let windows = vec![
        DataWindow::Count(0),
        DataWindow::Count(usize::MAX),
        DataWindow::Range {
            column: "time α".into(),
            width: 0.0,
            anchor: DataWindowAnchor::Latest,
        },
        DataWindow::Range {
            column: "time".into(),
            width: 0.5,
            anchor: DataWindowAnchor::HostTime {
                units_per_second: 1000.0,
            },
        },
        DataWindow::Range {
            column: "x".into(),
            width: 2.0,
            anchor: DataWindowAnchor::Supplied(DynamicValue::F32(1.0)),
        },
        DataWindow::Range {
            column: "y".into(),
            width: 2.0,
            anchor: DataWindowAnchor::Supplied(DynamicValue::I32(-4)),
        },
        DataWindow::Range {
            column: "z".into(),
            width: 2.0,
            anchor: DataWindowAnchor::Supplied(DynamicValue::U32(u32::MAX)),
        },
    ];
    let encoded = encode_data_windows(&windows).unwrap();
    assert_eq!(&encoded[..8], b"IPPW\x01\x00\x07\x00");
    assert_eq!(decode_data_windows(&encoded).unwrap(), windows);
    assert!(encode_data_windows(&[]).unwrap().is_empty());
    assert!(decode_data_windows(&[]).unwrap().is_empty());
}

#[test]
fn truncation_trailing_bytes_unknown_tags_and_target_count_overflow_are_rejected() {
    let encoded = encode_data_windows(&[DataWindow::Range {
        column: "raw".into(),
        width: 2.0,
        anchor: DataWindowAnchor::Supplied(DynamicValue::U32(7)),
    }])
    .unwrap();
    for end in 1..encoded.len() {
        assert_eq!(
            decode_data_windows(&encoded[..end]),
            Err(ErrorReason::InvalidValue)
        );
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert_eq!(
        decode_data_windows(&trailing),
        Err(ErrorReason::InvalidValue)
    );
    let mut unknown = encoded.clone();
    unknown[8] = 9;
    assert_eq!(
        decode_data_windows(&unknown),
        Err(ErrorReason::InvalidValue)
    );
    let mut version = encoded;
    version[4] = 2;
    assert_eq!(
        decode_data_windows(&version),
        Err(ErrorReason::InvalidValue)
    );
    let mut count = b"IPPW\x01\x00\x01\x00\x00".to_vec();
    count.extend_from_slice(&u64::MAX.to_le_bytes());
    if usize::BITS < 64 {
        assert_eq!(decode_data_windows(&count), Err(ErrorReason::InvalidValue));
    } else {
        assert_eq!(
            decode_data_windows(&count).unwrap(),
            [DataWindow::Count(usize::MAX)]
        );
    }
}

#[test]
fn invalid_constraints_and_bounded_codec_allocations_are_rejected() {
    for window in [
        DataWindow::Range {
            column: "".into(),
            width: 1.0,
            anchor: DataWindowAnchor::Latest,
        },
        DataWindow::Range {
            column: "x".into(),
            width: -1.0,
            anchor: DataWindowAnchor::Latest,
        },
        DataWindow::Range {
            column: "x".into(),
            width: f64::INFINITY,
            anchor: DataWindowAnchor::Latest,
        },
        DataWindow::Range {
            column: "x".into(),
            width: 1.0,
            anchor: DataWindowAnchor::HostTime {
                units_per_second: 0.0,
            },
        },
        DataWindow::Range {
            column: "x".into(),
            width: 1.0,
            anchor: DataWindowAnchor::Supplied(DynamicValue::Bool(true)),
        },
        DataWindow::Range {
            column: "x".repeat(4097),
            width: 1.0,
            anchor: DataWindowAnchor::Latest,
        },
    ] {
        assert_eq!(
            encode_data_windows(&[window]),
            Err(ErrorReason::InvalidValue)
        );
    }
    assert_eq!(
        encode_data_windows(&vec![DataWindow::Count(1); 257]),
        Err(ErrorReason::InvalidValue)
    );
    let big = vec![
        DataWindow::Range {
            column: "x".repeat(4096),
            width: 1.0,
            anchor: DataWindowAnchor::Latest
        };
        32
    ];
    assert_eq!(encode_data_windows(&big), Err(ErrorReason::InvalidValue));
    assert_eq!(
        decode_data_windows(&vec![0; 65537]),
        Err(ErrorReason::InvalidValue)
    );
}
