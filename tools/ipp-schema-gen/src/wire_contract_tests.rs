use super::{is_tag_space_name, read_wire_contract, tag_space_name};
use crate::binary_reader::Reader;

#[test]
fn animation_transition_tag_spaces_are_known_by_id_and_name() {
    assert_eq!(tag_space_name(30).unwrap(), "animation-transition-easing");
    assert_eq!(
        tag_space_name(31).unwrap(),
        "animation-transition-start-time"
    );
    assert!(is_tag_space_name("animation-transition-easing"));
    assert!(is_tag_space_name("animation-transition-start-time"));
    assert!(tag_space_name(7).is_err());
    assert_eq!(tag_space_name(32).unwrap(), "output-kind");
    assert_eq!(tag_space_name(33).unwrap(), "view-target");
    assert!(is_tag_space_name("view-target"));
    assert_eq!(tag_space_name(36).unwrap(), "presentation-request");
    assert_eq!(tag_space_name(37).unwrap(), "presentation-response");
    assert_eq!(tag_space_name(38).unwrap(), "presentation-error");
    for (tag, name) in [
        (39, "lifecycle-watch-change"),
        (40, "lifecycle-watch-target"),
        (41, "lifecycle-watch-record"),
        (42, "lifecycle-membership-result"),
        (43, "lifecycle-target-lifetime"),
        (44, "lifecycle-membership-rejection"),
        (45, "lifecycle-watch-kinds"),
    ] {
        assert_eq!(tag_space_name(tag).unwrap(), name);
        assert!(is_tag_space_name(name));
    }

    assert_eq!(tag_space_name(53).unwrap(), "gui-action");
    assert_eq!(tag_space_name(54).unwrap(), "output-target");
    assert!(is_tag_space_name("output-target"));
    for (id, name) in [
        (55, "dataset-request"),
        (56, "dataset-response"),
        (57, "dataset-delta"),
        (58, "dataset-kind"),
        (59, "dataset-value"),
    ] {
        assert_eq!(tag_space_name(id).unwrap(), name);
        assert!(is_tag_space_name(name));
    }
    assert_eq!(tag_space_name(60).unwrap(), "profile-request");
    assert_eq!(tag_space_name(61).unwrap(), "profile-status");
    assert_eq!(tag_space_name(62).unwrap(), "profile-gpu-sampling");
    assert!(is_tag_space_name("profile-gpu-sampling"));
    for (id, name) in [
        (63, "asset-export-request"),
        (64, "asset-export-response"),
        (65, "asset-representation"),
        (66, "asset-export-format"),
    ] {
        assert_eq!(tag_space_name(id).unwrap(), name);
        assert!(is_tag_space_name(name));
    }
    assert!(tag_space_name(67).is_err());
    assert!(!is_tag_space_name("animation-transition-unknown"));
    assert!(!is_tag_space_name("lifecycle-watch-unknown"));
}

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
