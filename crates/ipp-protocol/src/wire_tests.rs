use super::*;
use ipp_core::components::schema::ContractHash;
use std::collections::BTreeSet;

#[test]
fn lifecycle_diagnostic_tags_and_layouts_are_declared_in_every_build() {
    // Instrumentation never changes the contract, and every build answers.
    for name in [
        "REQUEST_LIFECYCLE_DIAGNOSTICS",
        "RESPONSE_LIFECYCLE_DIAGNOSTICS",
    ] {
        assert!(TAGS.iter().any(|tag| tag.name == name));
    }
    for name in [
        "request-lifecycle-diagnostics",
        "response-lifecycle-diagnostics",
    ] {
        assert!(LAYOUTS.iter().any(|layout| layout.name == name));
    }
}

#[test]
fn manifest_names_tags_and_layout_references_are_valid() {
    let layout_names = LAYOUTS
        .iter()
        .map(|layout| layout.name)
        .collect::<BTreeSet<_>>();
    let mut names = BTreeSet::new();
    let mut values = BTreeSet::new();

    assert_eq!(layout_names.len(), LAYOUTS.len());
    let tag_spaces = [
        "dataset-request",
        "dataset-response",
        "dataset-value",
        "dataset-kind",
        "dataset-delta",
        "value",
        "request",
        "command",
        "reference",
        "response",
        "outcome",
        "batch-error-scope",
        "runtime-failure-scope",
        "inspection-collection",
        "host-request",
        "gui-physical-request",
        "gui-physical-event",
        "gui-physical-response",
        "gui-physical-button",
        "gui-physical-key",
        "gui-native-edit",
        "gui-physical-disposition",
        "gui-action",
        "host-response",
        "world-selector",
        "output-kind",
        "output-target",
        "animation-target",
        "playback-control",
        "playback-state",
        "playback-event-kind",
        "option",
        "resource-status",
        "snapshot-value",
        "snapshot-reference",
        "view-target",
        "operation-effect",
        "attachment-receipt-state",
        "presentation-request",
        "presentation-response",
        "presentation-error",
        "geometry-pick-outcome",
        "lifecycle-observation",
        "lifecycle-watch-change",
        "lifecycle-watch-target",
        "lifecycle-watch-record",
        "lifecycle-membership-result",
        "lifecycle-target-lifetime",
        "lifecycle-membership-rejection",
        "lifecycle-watch-kinds",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    for layout in LAYOUTS {
        let mut fields = BTreeSet::new();
        for field in layout.fields {
            assert!(fields.insert(field.name));
            match field.encoding {
                FieldEncoding::Masked => assert!(
                    field.limit.is_power_of_two()
                        && (field.target == "bool" || layout_names.contains(field.target))
                ),
                FieldEncoding::Named => assert!(layout_names.contains(field.target)),
                FieldEncoding::List | FieldEncoding::U8CountedList => assert!(
                    layout_names.contains(field.target)
                        || tag_spaces.contains(field.target)
                        || ["u8", "u16", "u32", "u64", "utf8-65536", "empty"]
                            .contains(&field.target)
                ),
                FieldEncoding::Option => {
                    assert!(
                        ["bool", "u16", "u32", "u64", "utf8-65536"].contains(&field.target)
                            || layout_names.contains(field.target)
                    );
                }
                FieldEncoding::Variant | FieldEncoding::Union => {
                    assert!(tag_spaces.contains(field.target));
                }
                _ => assert!(field.target.is_empty()),
            }
        }
    }
    for tag in TAGS {
        assert!(names.insert(tag.name));
        assert!(values.insert((tag.space as u8, tag.value)));
        if tag.layout != "empty" && tag.layout != "present" {
            assert!(
                layout_names.contains(tag.layout),
                "missing layout for {}",
                tag.name
            );
            let layout = LAYOUTS
                .iter()
                .find(|layout| layout.name == tag.layout)
                .unwrap();
            assert!(layout.fields.iter().any(|field| {
                field.encoding == FieldEncoding::Variant
                    && field.target == tag_space_name(tag.space)
            }));
        }
    }
}

fn tag_space_name(space: TagSpace) -> &'static str {
    match space {
        TagSpace::Value => "value",
        TagSpace::Request => "request",
        TagSpace::Command => "command",
        TagSpace::Reference => "reference",
        TagSpace::Response => "response",
        TagSpace::Outcome => "outcome",
        TagSpace::BatchErrorScope => "batch-error-scope",
        TagSpace::RuntimeFailureScope => "runtime-failure-scope",
        TagSpace::InspectionCollection => "inspection-collection",
        TagSpace::HostRequest => "host-request",
        TagSpace::HostResponse => "host-response",
        TagSpace::WorldSelector => "world-selector",
        TagSpace::OutputKind => "output-kind",
        TagSpace::AnimationTarget => "animation-target",
        TagSpace::PlaybackControl => "playback-control",
        TagSpace::PlaybackState => "playback-state",
        TagSpace::PlaybackEvent => "playback-event-kind",
        TagSpace::AnimationTransitionEasing => "animation-transition-easing",
        TagSpace::AnimationTransitionStartTime => "animation-transition-start-time",

        TagSpace::Option => "option",
        TagSpace::AssetResourceStatus => "resource-status",
        TagSpace::SnapshotValue => "snapshot-value",
        TagSpace::SnapshotReference => "snapshot-reference",
        TagSpace::ViewTarget => "view-target",
        TagSpace::OperationEffect => "operation-effect",
        TagSpace::AttachmentReceiptState => "attachment-receipt-state",
        TagSpace::PresentationRequest => "presentation-request",
        TagSpace::PresentationResponse => "presentation-response",
        TagSpace::PresentationError => "presentation-error",
        TagSpace::GeometryPickOutcome => "geometry-pick-outcome",
        TagSpace::LifecycleObservation => "lifecycle-observation",
        TagSpace::LifecycleWatchChange => "lifecycle-watch-change",
        TagSpace::LifecycleWatchTarget => "lifecycle-watch-target",
        TagSpace::LifecycleWatchRecord => "lifecycle-watch-record",
        TagSpace::LifecycleMembershipResult => "lifecycle-membership-result",
        TagSpace::LifecycleTargetLifetime => "lifecycle-target-lifetime",
        TagSpace::LifecycleMembershipRejection => "lifecycle-membership-rejection",
        TagSpace::LifecycleWatchKinds => "lifecycle-watch-kinds",
        TagSpace::GuiPhysicalRequest => "gui-physical-request",
        TagSpace::GuiPhysicalEvent => "gui-physical-event",
        TagSpace::GuiPhysicalResponse => "gui-physical-response",
        TagSpace::GuiPhysicalButton => "gui-physical-button",
        TagSpace::GuiPhysicalKey => "gui-physical-key",
        TagSpace::GuiNativeEdit => "gui-native-edit",
        TagSpace::GuiPhysicalDisposition => "gui-physical-disposition",
        TagSpace::GuiAction => "gui-action",
        TagSpace::OutputTarget => "output-target",
        TagSpace::DatasetRequest => "dataset-request",
        TagSpace::DatasetResponse => "dataset-response",
        TagSpace::DatasetDelta => "dataset-delta",
        TagSpace::DatasetKind => "dataset-kind",
        TagSpace::DatasetValue => "dataset-value",
    }
}

#[test]
fn value_tags_are_exact_core_field_kinds() {
    assert_eq!(
        VALUE_BOOL,
        ipp_core::components::schema::FieldKind::Bool as u8
    );
    assert_eq!(SNAPSHOT_VALUE_BOOL, VALUE_BOOL);
    assert_eq!(
        VALUE_F32,
        ipp_core::components::schema::FieldKind::F32 as u8
    );
    assert_eq!(
        VALUE_ENTITY,
        ipp_core::components::schema::FieldKind::Entity as u8
    );
    assert_eq!(
        VALUE_U32,
        ipp_core::components::schema::FieldKind::U32 as u8
    );
    assert_eq!(
        VALUE_U64,
        ipp_core::components::schema::FieldKind::U64 as u8
    );
    assert_eq!(
        VALUE_STRING,
        ipp_core::components::schema::FieldKind::String as u8
    );
    assert_eq!(
        VALUE_BYTES,
        ipp_core::components::schema::FieldKind::Bytes as u8
    );
    assert_eq!(SNAPSHOT_VALUE_F32, VALUE_F32);
    assert_eq!(SNAPSHOT_VALUE_ENTITY, VALUE_ENTITY);
    assert_eq!(SNAPSHOT_VALUE_U32, VALUE_U32);
    assert_eq!(SNAPSHOT_VALUE_U64, VALUE_U64);
    assert_eq!(SNAPSHOT_VALUE_STRING, VALUE_STRING);
    assert_eq!(SNAPSHOT_VALUE_BYTES, VALUE_BYTES);
    assert_eq!(SNAPSHOT_REF_HANDLE, REF_HANDLE);
}

#[test]
fn asset_formats_have_unique_nonzero_names_and_type_ids() {
    let mut names = BTreeSet::new();
    let mut type_ids = BTreeSet::new();
    for format in ASSET_FORMATS {
        assert!(!format.name.is_empty());
        assert!(!format.format.is_empty());
        assert_ne!(format.type_id, 0);
        assert!(names.insert(format.name));
        assert!(type_ids.insert(format.type_id));
    }
}

#[test]
fn message_budget_bounds_share_one_declaration() {
    let limit = |layout: &str, field: &str| {
        LAYOUTS
            .iter()
            .find(|candidate| candidate.name == layout)
            .and_then(|layout| layout.fields.iter().find(|f| f.name == field))
            .map(|field| field.limit as usize)
    };

    // Generated codecs read these exported declarations; encoders use the constant.
    assert_eq!(
        limit("snapshot-value-bytes", "value"),
        Some(crate::MAX_MESSAGE_BYTES)
    );
    let convention = CONVENTIONS
        .iter()
        .find(|(name, _)| *name == "max-message-bytes")
        .map(|(_, value)| value.parse::<usize>().unwrap());
    assert_eq!(convention, Some(crate::MAX_MESSAGE_BYTES));
    let page_bytes = CONVENTIONS
        .iter()
        .find(|(name, _)| *name == "command-page-bytes")
        .map(|(_, value)| value.parse::<usize>().unwrap());
    assert_eq!(page_bytes, Some(crate::COMMAND_PAGE_BYTES));
    let outcome_aliases = CONVENTIONS
        .iter()
        .find(|(name, _)| *name == "batch-outcome-aliases")
        .map(|(_, value)| value.parse::<usize>().unwrap());
    assert_eq!(outcome_aliases, Some(crate::BATCH_OUTCOME_ALIASES));
    assert_eq!(
        limit("outcome-success", "aliases"),
        Some(crate::BATCH_OUTCOME_ALIASES)
    );
    assert_eq!(
        limit("request-submit-batch", "operations"),
        Some(crate::COMMAND_PAGE_COMMANDS)
    );

    // No bounded byte or text field may exceed the complete message budget.
    for layout in LAYOUTS {
        for field in layout.fields {
            if matches!(field.encoding, FieldEncoding::Bytes | FieldEncoding::Utf8) {
                assert!(
                    field.limit as usize <= crate::MAX_MESSAGE_BYTES,
                    "{}.{} exceeds the message budget",
                    layout.name,
                    field.name
                );
            }
        }
    }
}

#[test]
fn nested_field_order_and_encoding_change_contract_hash() {
    const FIRST: WireField = field!("first", U32);
    const SECOND: WireField = field!("second", Bytes, 32);
    let original = WireLayout {
        name: "fixture",
        fields: &[FIRST, SECOND],
    };
    let reordered = WireLayout {
        fields: &[SECOND, FIRST],
        ..original
    };
    let changed_encoding = WireLayout {
        fields: &[FIRST, field!("second", Utf8, 32)],
        ..original
    };

    let hash = |layout: &WireLayout| {
        let mut hash = ContractHash::default();
        write_layout(&mut hash, layout);
        hash.0
    };
    assert_ne!(hash(&original), hash(&reordered));
    assert_ne!(hash(&original), hash(&changed_encoding));

    let mut bytes = Vec::new();
    write_layout(&mut bytes, &original);
    assert!(!bytes.is_empty());
}

#[test]
fn physical_input_subcodec_has_canonical_nested_layouts() {
    for name in [
        "gui-physical-open",
        "gui-physical-event",
        "gui-physical-text",
        "gui-native-state",
        "gui-native-compose",
        "gui-physical-pointer-down",
    ] {
        assert!(
            LAYOUTS.iter().any(|layout| layout.name == name),
            "missing {name}"
        );
    }
}

#[test]
fn physical_key_and_native_field_changes_announce_a_new_hash() {
    let mut contract = Vec::new();
    crate::write_contract(&mut contract);
    let key = b"GUI_PHYSICAL_KEY_LEFT";
    let offset = contract
        .windows(key.len())
        .position(|value| value == key)
        .unwrap()
        + key.len()
        + 2;
    let mut changed_key = contract.clone();
    changed_key[offset] ^= 64;
    let layout = LAYOUTS
        .iter()
        .find(|layout| layout.name == "gui-native-state")
        .unwrap();
    let mut original_layout = Vec::new();
    write_layout(&mut original_layout, layout);
    let offset = contract
        .windows(original_layout.len())
        .position(|value| value == original_layout)
        .unwrap();
    let field_name = b"selection_start";
    let field_offset = original_layout
        .windows(field_name.len())
        .position(|value| value == field_name)
        .unwrap()
        + field_name.len();
    assert_eq!(original_layout[field_offset], FieldEncoding::U32 as u8);
    let mut changed_layout = original_layout.clone();
    changed_layout[field_offset] = FieldEncoding::U64 as u8;
    let mut changed_native = contract.clone();
    changed_native[offset..offset + original_layout.len()].copy_from_slice(&changed_layout);
    for changed in [changed_key, changed_native] {
        let mut hash = ContractHash::default();
        hash.write(&changed);
        assert_ne!(hash.0, crate::schema_hash());
    }
}

#[test]
fn cached_schema_hash_matches_the_hashed_contract() {
    let mut hash = ContractHash::default();
    crate::write_contract(&mut hash);

    assert_eq!(crate::schema_hash(), hash.0);
    assert_eq!(crate::schema_hash(), hash.0, "repeated reads use the cache");
}

#[test]
fn expression_asset_metadata_names_the_reviewed_shared_core_codec() {
    let format = ASSET_FORMATS
        .iter()
        .find(|format| format.name == "ASSET_EXPRESSION")
        .unwrap();
    assert_eq!(
        format.type_id,
        ipp_core::services::asset_management::expression::EXPRESSION_TYPE.0
    );
    assert_eq!(ipp_core::expressions::EXPRESSION_FORMAT_MAGIC, *b"IPPE");
    assert_eq!(ipp_core::expressions::EXPRESSION_FORMAT_VERSION, 1);
    assert!(format.format.starts_with("IPPE;version-u32=1;"));
}
