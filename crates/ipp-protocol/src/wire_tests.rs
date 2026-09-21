use super::*;
use ipp_core::components::schema::ContractHash;
use std::collections::BTreeSet;

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
        "host-response",
        "world-selector",
        "animation-target",
        "playback-control",
        "playback-state",
        "playback-event-kind",
        "option",
        "entity-overlay-mode",
        "component-overlay-mode",
        "state-overlay-handle-kind",
        "state-overlay-lifecycle-reason",
        "resource-status",
        "snapshot-value",
        "snapshot-reference",
        "camera-motion",
        "geometry-pick-outcome",
        "lifecycle-observation",
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
                FieldEncoding::List => assert!(
                    layout_names.contains(field.target)
                        || tag_spaces.contains(field.target)
                        || ["u32", "utf8-65536", "empty"].contains(&field.target)
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
        TagSpace::AnimationTarget => "animation-target",
        TagSpace::PlaybackControl => "playback-control",
        TagSpace::PlaybackState => "playback-state",
        TagSpace::PlaybackEvent => "playback-event-kind",
        TagSpace::AnimationTransitionEasing => "animation-transition-easing",
        TagSpace::AnimationTransitionStartTime => "animation-transition-start-time",

        TagSpace::Option => "option",
        TagSpace::EntityOverlayMode => "entity-overlay-mode",
        TagSpace::ComponentOverlayMode => "component-overlay-mode",
        TagSpace::StateOverlayHandleKind => "state-overlay-handle-kind",
        TagSpace::StateOverlayLifecycleReason => "state-overlay-lifecycle-reason",
        TagSpace::AssetResourceStatus => "resource-status",
        TagSpace::SnapshotValue => "snapshot-value",
        TagSpace::SnapshotReference => "snapshot-reference",
        TagSpace::CameraMotion => "camera-motion",
        TagSpace::GeometryPickOutcome => "geometry-pick-outcome",
        TagSpace::LifecycleObservation => "lifecycle-observation",
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
fn nested_field_order_and_encoding_change_contract_hash() {
    const FIRST: WireField = field!("first", U32);
    const SECOND: WireField = field!("second", Bytes, 32);
    let original = WireLayout {
        name: "fixture",
        capability: Capability::Base,
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
