use super::read_wire_contract;
use crate::binary_reader::Reader;

fn text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend((value.len() as u32).to_le_bytes());
    bytes.extend(value.as_bytes());
}

fn fixture(target: &str, space: u8, duplicate: bool) -> Vec<u8> {
    let mut bytes = vec![];
    for value in [3u16, 0, 0, 1] {
        bytes.extend(value.to_le_bytes());
    }
    text(&mut bytes, "asset-control");
    bytes.extend(1u16.to_le_bytes());
    text(&mut bytes, "tag");
    bytes.push(12); // Variant; a scalar discriminant in its exact declared domain.
    bytes.extend(0u32.to_le_bytes());
    text(&mut bytes, target);
    bytes.extend(
        (if duplicate {
            2u16
        } else {
            1u16
        })
        .to_le_bytes(),
    );
    for _ in 0..if duplicate {
        2
    } else {
        1
    } {
        text(&mut bytes, "ASSET_FIXTURE_TAG");
        bytes.extend([space, 1]);
        text(&mut bytes, "asset-control");
    }
    bytes.extend([0; 4]); // Asset format catalog and semantic rejection names.
    bytes
}

#[test]
fn exported_asset_domains_validate_matching_records_and_truncation() {
    for (space, target) in [
        (63, "asset-export-request"),
        (64, "asset-export-response"),
        (65, "asset-representation"),
        (66, "asset-export-format"),
    ] {
        let bytes = fixture(target, space, false);
        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        let contract = read_wire_contract(&mut reader, &[]).unwrap();
        assert_eq!(contract.tags[0].space, space);
        assert!(reader.is_complete());
        for end in 0..bytes.len() {
            assert!(
                read_wire_contract(
                    &mut Reader {
                        bytes: &bytes[..end],
                        at: 0
                    },
                    &[]
                )
                .is_err()
            );
        }
        for invalid in [
            fixture(target, 67, false),
            fixture("asset-export-unknown", space, false),
            fixture(target, space, true),
            fixture(
                if space == 63 {
                    "asset-export-response"
                } else {
                    "asset-export-request"
                },
                space,
                false,
            ),
        ] {
            assert!(
                read_wire_contract(
                    &mut Reader {
                        bytes: &invalid,
                        at: 0
                    },
                    &[]
                )
                .is_err()
            );
        }
    }
}
