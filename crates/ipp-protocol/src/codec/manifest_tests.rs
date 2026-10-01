use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[path = "view_manifest_tests.rs"]
mod view_tests;

#[path = "attachment_manifest_tests.rs"]
mod attachment_tests;

#[path = "lifecycle_watch_manifest_tests.rs"]
mod lifecycle_watch_tests;

#[cfg(feature = "diagnostics")]
#[path = "lifecycle_diagnostics_tests.rs"]
mod lifecycle_diagnostics_tests;

fn test_link() -> ipp_core::EntityLink {
    ipp_core::EntityLink {
        parent: None,
        order: ipp_core::EntityOrder::from_value(1).unwrap(),
    }
}

fn navigation_binding_fixture() -> ManifestValue {
    manifest_layout(
        "root-binding",
        [
            (
                "output",
                manifest_layout(
                    "output-reference",
                    [
                        (
                            "world",
                            manifest_layout(
                                "world-reference",
                                [
                                    ("id", ManifestValue::U64(11)),
                                    ("incarnation", ManifestValue::U64(12)),
                                ],
                            ),
                        ),
                        (
                            "target",
                            manifest_layout(
                                "output-target-camera",
                                [
                                    ("tag", ManifestValue::Tag("OUTPUT_TARGET_CAMERA")),
                                    ("entity", ManifestValue::U64(13)),
                                    ("incarnation", ManifestValue::U64(14)),
                                ],
                            ),
                        ),
                    ],
                ),
            ),
            ("width", ManifestValue::U32(640)),
            ("height", ManifestValue::U32(480)),
            ("device_pixel_ratio", ManifestValue::F64(1.25)),
            (
                "generation",
                manifest_layout(
                    "presentation-identity",
                    [
                        ("host", ManifestValue::U64(15)),
                        ("serial", ManifestValue::U64(16)),
                    ],
                ),
            ),
        ],
    )
}

fn navigation_request_fixtures(covered: &mut BTreeSet<&'static str>) {
    use ipp_core::systems::camera::CameraViewMotion;
    let binding = crate::presentation::RootBinding {
        output: crate::references::OutputReference {
            world: crate::references::WorldReference {
                id: 11,
                incarnation: 12,
            },
            target: crate::references::OutputTarget::Camera {
                entity: 13,
                incarnation: 14,
            },
        },
        viewport: ipp_core::WorldViewport {
            width: 640,
            height: 480,
            device_pixel_ratio: 1.25,
        },
        generation: crate::presentation::PresentationIdentity {
            host: 15,
            serial: 16,
        },
    };
    for historical in [false, true] {
        let publication = historical.then_some(crate::presentation::PresentationIdentity {
            host: 15,
            serial: 17,
        });
        let source = || {
            if historical {
                ManifestValue::Some(Box::new(manifest_layout(
                    "publication-reference",
                    [
                        ("host", ManifestValue::U64(15)),
                        ("revision", ManifestValue::U64(17)),
                    ],
                )))
            } else {
                ManifestValue::None
            }
        };
        for (kind, first, second, motion) in [
            (
                0,
                0.25,
                -0.5,
                CameraViewMotion::Rotate {
                    yaw: 0.25,
                    pitch: -0.5,
                },
            ),
            (
                1,
                -0.25,
                0.5,
                CameraViewMotion::Pan {
                    x: -0.25,
                    y: 0.5,
                },
            ),
            (
                2,
                0.75,
                0.0,
                CameraViewMotion::Zoom {
                    amount: 0.75,
                },
            ),
        ] {
            let fixture = ManifestFixture::new(
                "request-camera-navigate",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(37)),
                    ("tag", ManifestValue::Tag("REQUEST_CAMERA_NAVIGATE")),
                    ("binding", navigation_binding_fixture()),
                    ("publication", source()),
                    ("kind", ManifestValue::U32(kind)),
                    ("first", ManifestValue::F32(first)),
                    ("second", ManifestValue::F32(second)),
                ],
            );
            let bytes = encode_manifest_fixture(&fixture, covered);
            let decoded = decode_request(&bytes, 7).unwrap();
            assert_eq!((decoded.session, decoded.request_id), (7, 37));
            assert_eq!(
                decoded.body,
                RequestBody::CameraNavigate(crate::views::CameraNavigateRequest {
                    binding,
                    publication,
                    motion
                })
            );
            let mut malformed = bytes.clone();
            let kind_offset = malformed.len() - 12;
            malformed[kind_offset..kind_offset + 4].copy_from_slice(&3_u32.to_le_bytes());
            assert!(matches!(
                decode_request(&malformed, 7),
                Err(ProtocolError::Malformed("camera motion"))
            ));
            assert!(decode_request(&bytes[..bytes.len() - 1], 7).is_err());
        }
        let fixture = ManifestFixture::new(
            "request-geometry-pick",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(38)),
                ("tag", ManifestValue::Tag("REQUEST_GEOMETRY_PICK")),
                (
                    "view",
                    manifest_layout(
                        "view-bound",
                        [
                            ("tag", ManifestValue::Tag("VIEW_BOUND")),
                            ("binding", navigation_binding_fixture()),
                            ("publication", source()),
                        ],
                    ),
                ),
                ("x", ManifestValue::F32(0.25)),
                ("y", ManifestValue::F32(0.75)),
                ("include_view_plane", ManifestValue::Bool(true)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&fixture, covered), 7)
                .unwrap()
                .body,
            RequestBody::GeometryPickQuery(crate::views::GeometryPickQuery {
                view: crate::views::ViewQueryTarget::BoundView {
                    binding,
                    publication
                },
                x: 0.25,
                y: 0.75,
                include_view_plane: true,
            })
        );
    }
}

#[derive(Debug)]
enum ManifestValue {
    Bool(bool),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    Layout(Box<ManifestFixture>),
    List(Vec<ManifestValue>),
    None,
    Some(Box<ManifestValue>),
    Tag(&'static str),
}

#[derive(Debug)]
struct ManifestFixture {
    layout: &'static str,
    values: BTreeMap<&'static str, ManifestValue>,
}

impl ManifestFixture {
    fn new(
        layout: &'static str,
        values: impl IntoIterator<Item = (&'static str, ManifestValue)>,
    ) -> Self {
        Self {
            layout,
            values: values.into_iter().collect(),
        }
    }
}

fn empty_representation() -> ManifestValue {
    manifest_layout(
        "asset-representation",
        [
            ("decoded", ManifestValue::Bool(false)),
            ("graphics_ready", ManifestValue::None),
            ("source_bytes", ManifestValue::U64(0)),
            ("resident_bytes", ManifestValue::U64(0)),
            ("graphics_bytes", ManifestValue::None),
        ],
    )
}

fn manifest_layout(
    name: &'static str,
    values: impl IntoIterator<Item = (&'static str, ManifestValue)>,
) -> ManifestValue {
    ManifestValue::Layout(Box::new(ManifestFixture::new(name, values)))
}

fn manifest_tag_space(space: TagSpace) -> &'static str {
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
    }
}

fn encode_manifest_fixture(
    fixture: &ManifestFixture,
    covered: &mut BTreeSet<&'static str>,
) -> Vec<u8> {
    let layout = LAYOUTS
        .iter()
        .find(|layout| layout.name == fixture.layout)
        .unwrap_or_else(|| panic!("unknown manifest layout {}", fixture.layout));
    assert_eq!(
        fixture.values.len(),
        layout.fields.len(),
        "fixture field count for {}",
        fixture.layout
    );

    let mut bytes = Vec::new();
    for field in layout.fields {
        let value = fixture
            .values
            .get(field.name)
            .unwrap_or_else(|| panic!("missing {}.{}", fixture.layout, field.name));
        if field.encoding == FieldEncoding::Masked {
            let Some(ManifestValue::U16(mask)) = fixture.values.get("mask") else {
                panic!("missing presence mask")
            };
            assert_eq!(
                *mask & field.limit as u16 != 0,
                matches!(value, ManifestValue::Some(_))
            );
        }
        encode_manifest_value(
            &mut bytes,
            value,
            field.encoding,
            field.limit,
            field.target,
            covered,
        );
    }
    bytes
}

fn encode_manifest_value(
    bytes: &mut Vec<u8>,
    value: &ManifestValue,
    encoding: FieldEncoding,
    limit: u32,
    target: &str,
    covered: &mut BTreeSet<&'static str>,
) {
    match (encoding, value) {
        (FieldEncoding::Bool, ManifestValue::Bool(value)) => bytes.push(u8::from(*value)),
        (FieldEncoding::Masked, ManifestValue::None) => {}
        (FieldEncoding::Masked, ManifestValue::Some(value)) => {
            encode_manifest_nested(bytes, value, target, covered)
        }
        (FieldEncoding::U16, ManifestValue::U16(value)) => {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        (FieldEncoding::U32, ManifestValue::U32(value)) => {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        (FieldEncoding::U64, ManifestValue::U64(value)) => {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        (FieldEncoding::FiniteF32, ManifestValue::F32(value)) => {
            assert!(value.is_finite());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        (FieldEncoding::NonnegativeFiniteF64, ManifestValue::F64(value)) => {
            assert!(value.is_finite() && *value >= 0.0);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        (FieldEncoding::Utf8, ManifestValue::String(value)) => {
            assert!(value.len() <= limit as usize);
            bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }
        (FieldEncoding::Bytes, ManifestValue::Bytes(value)) => {
            assert!(value.len() <= limit as usize);
            bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
            bytes.extend_from_slice(value);
        }
        (FieldEncoding::Named, ManifestValue::Layout(nested)) => {
            assert_eq!(nested.layout, target);
            bytes.extend_from_slice(&encode_manifest_fixture(nested, covered));
        }
        (FieldEncoding::List | FieldEncoding::U8CountedList, ManifestValue::List(values)) => {
            assert!(values.len() <= limit as usize);
            if encoding == FieldEncoding::U8CountedList {
                bytes.push(u8::try_from(values.len()).unwrap());
            } else {
                bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
            }
            for value in values {
                encode_manifest_nested(bytes, value, target, covered);
            }
        }
        (FieldEncoding::Option, ManifestValue::None) => {
            bytes.push(manifest_tag("OPTION_NONE", "option", covered));
        }
        (FieldEncoding::Option, ManifestValue::Some(value)) => {
            bytes.push(manifest_tag("OPTION_SOME", "option", covered));
            encode_manifest_nested(bytes, value, target, covered);
        }
        (FieldEncoding::Variant, ManifestValue::Tag(name)) => {
            bytes.push(manifest_tag(name, target, covered));
        }
        (FieldEncoding::Union, ManifestValue::Layout(nested)) => {
            let valid = TAGS
                .iter()
                .any(|tag| tag.layout == nested.layout && manifest_tag_space(tag.space) == target);
            assert!(valid, "{} is not a member of {target}", nested.layout);
            bytes.extend_from_slice(&encode_manifest_fixture(nested, covered));
        }
        _ => panic!("manifest fixture type mismatch for {encoding:?} targeting {target}"),
    }
}

fn encode_manifest_nested(
    bytes: &mut Vec<u8>,
    value: &ManifestValue,
    target: &str,
    covered: &mut BTreeSet<&'static str>,
) {
    match (target, value) {
        ("bool", ManifestValue::Bool(value)) => bytes.push(u8::from(*value)),
        ("u16", ManifestValue::U16(value)) => bytes.extend_from_slice(&value.to_le_bytes()),
        ("u32", ManifestValue::U32(value)) => bytes.extend_from_slice(&value.to_le_bytes()),
        ("u64", ManifestValue::U64(value)) => bytes.extend_from_slice(&value.to_le_bytes()),
        ("utf8-65536", ManifestValue::String(value)) => {
            assert!(value.len() <= 65_536);
            bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }
        (_, ManifestValue::Layout(nested)) => {
            let valid = nested.layout == target
                || TAGS.iter().any(|tag| {
                    tag.layout == nested.layout && manifest_tag_space(tag.space) == target
                });
            assert!(valid, "{} is not a member of {target}", nested.layout);
            bytes.extend_from_slice(&encode_manifest_fixture(nested, covered));
        }
        _ => panic!("invalid nested manifest fixture for {target}"),
    }
}

fn manifest_tag(
    name: &'static str,
    expected_space: &str,
    covered: &mut BTreeSet<&'static str>,
) -> u8 {
    let tag = TAGS
        .iter()
        .find(|tag| tag.name == name)
        .unwrap_or_else(|| panic!("unknown manifest tag {name}"));
    assert_eq!(manifest_tag_space(tag.space), expected_space);
    covered.insert(tag.name);
    tag.value
}

fn manifest_reference_alias(alias: u32) -> ManifestValue {
    manifest_layout(
        "reference-alias",
        [
            ("tag", ManifestValue::Tag("REF_ALIAS")),
            ("alias", ManifestValue::U32(alias)),
        ],
    )
}

fn manifest_reference_symbol(symbol: &str) -> ManifestValue {
    manifest_layout(
        "reference-symbol",
        [
            ("tag", ManifestValue::Tag("REF_SYMBOL")),
            ("symbol", ManifestValue::String(symbol.into())),
        ],
    )
}

fn manifest_reference_handle(handle: u64) -> ManifestValue {
    manifest_layout(
        "reference-handle",
        [
            ("tag", ManifestValue::Tag("REF_HANDLE")),
            ("handle", ManifestValue::U64(handle)),
        ],
    )
}

fn manifest_placement(
    parent: Option<ManifestValue>,
    before: Option<ManifestValue>,
) -> ManifestValue {
    let wrap = |reference: Option<ManifestValue>| {
        reference.map_or(ManifestValue::None, |reference| {
            ManifestValue::Some(Box::new(manifest_layout(
                "reference-value",
                [("value", reference)],
            )))
        })
    };
    manifest_layout(
        "entity-placement",
        [("parent", wrap(parent)), ("before", wrap(before))],
    )
}

fn manifest_metadata(symbolic_id: Option<&str>, classes: &[&str]) -> ManifestValue {
    manifest_layout(
        "metadata",
        [
            (
                "symbolic_id",
                symbolic_id.map_or(ManifestValue::None, |value| {
                    ManifestValue::Some(Box::new(ManifestValue::String(value.into())))
                }),
            ),
            (
                "classes",
                ManifestValue::List(
                    classes
                        .iter()
                        .map(|value| ManifestValue::String((*value).into()))
                        .collect(),
                ),
            ),
        ],
    )
}

fn manifest_field(offset: u32, value: ManifestValue) -> ManifestValue {
    manifest_layout(
        "field",
        [("offset", ManifestValue::U32(offset)), ("value", value)],
    )
}

fn manifest_snapshot_field(offset: u32, value: ManifestValue) -> ManifestValue {
    manifest_layout(
        "snapshot-field",
        [("offset", ManifestValue::U32(offset)), ("value", value)],
    )
}

fn manifest_typed_value(
    layout: &'static str,
    tag: &'static str,
    value: ManifestValue,
) -> ManifestValue {
    manifest_layout(layout, [("tag", ManifestValue::Tag(tag)), ("value", value)])
}

fn manifest_command(
    layout: &'static str,
    tag: &'static str,
    values: impl IntoIterator<Item = (&'static str, ManifestValue)>,
) -> ManifestValue {
    manifest_layout(
        layout,
        std::iter::once(("tag", ManifestValue::Tag(tag))).chain(values),
    )
}

#[test]
fn rust_request_decoder_conforms_to_every_enabled_manifest_branch() {
    let handle_bits = 0x0000_0001_0000_0029;
    let handle = EntityId::from_bits(handle_bits);
    let metadata = EntityMetadata {
        symbolic_id: Some("oracle".into()),
        classes: vec!["first".into(), "second".into()],
    };
    let writes = vec![
        FieldWrite {
            offset: 101,
            value: FieldValue::World(None),
        },
        FieldWrite {
            offset: 102,
            value: FieldValue::Output(None),
        },
        FieldWrite {
            offset: 88,
            value: FieldValue::Dynamic(ipp_core::DynamicValue::I32(-3)),
        },
        FieldWrite {
            offset: 77,
            value: FieldValue::Bool(true),
        },
        FieldWrite {
            offset: 11,
            value: FieldValue::F32(1.25),
        },
        FieldWrite {
            offset: 22,
            value: FieldValue::Entity(EntityRef::Handle(handle)),
        },
        FieldWrite {
            offset: 33,
            value: FieldValue::U32(0x1234_5678),
        },
        FieldWrite {
            offset: 44,
            value: FieldValue::U64(0x1020_3040_5060_7080),
        },
        FieldWrite {
            offset: 55,
            value: FieldValue::String("oracle-value".into()),
        },
        FieldWrite {
            offset: 66,
            value: FieldValue::Bytes(vec![9, 7, 5]),
        },
        FieldWrite {
            offset: 99,
            value: FieldValue::Rows(vec![0, 0, 0, 0, 0, 0, 0, 0]),
        },
        FieldWrite {
            offset: 0x1000_0002,
            value: FieldValue::Unset,
        },
    ];
    let encoded_writes = vec![
        manifest_field(
            101,
            manifest_typed_value("value-world", "VALUE_WORLD", ManifestValue::None),
        ),
        manifest_field(
            102,
            manifest_typed_value("value-output", "VALUE_OUTPUT", ManifestValue::None),
        ),
        manifest_field(
            88,
            manifest_typed_value(
                "value-dynamic",
                "VALUE_DYNAMIC",
                ManifestValue::Bytes(ipp_core::DynamicValue::I32(-3).encode()),
            ),
        ),
        manifest_field(
            77,
            manifest_typed_value("value-bool", "VALUE_BOOL", ManifestValue::Bool(true)),
        ),
        manifest_field(
            11,
            manifest_typed_value("value-f32", "VALUE_F32", ManifestValue::F32(1.25)),
        ),
        manifest_field(
            22,
            manifest_typed_value(
                "value-entity",
                "VALUE_ENTITY",
                manifest_reference_handle(handle_bits),
            ),
        ),
        manifest_field(
            33,
            manifest_typed_value("value-u32", "VALUE_U32", ManifestValue::U32(0x1234_5678)),
        ),
        manifest_field(
            44,
            manifest_typed_value(
                "value-u64",
                "VALUE_U64",
                ManifestValue::U64(0x1020_3040_5060_7080),
            ),
        ),
        manifest_field(
            55,
            manifest_typed_value(
                "value-string",
                "VALUE_STRING",
                ManifestValue::String("oracle-value".into()),
            ),
        ),
        manifest_field(
            66,
            manifest_typed_value(
                "value-bytes",
                "VALUE_BYTES",
                ManifestValue::Bytes(vec![9, 7, 5]),
            ),
        ),
        manifest_field(
            99,
            manifest_typed_value(
                "value-rows",
                "VALUE_ROWS",
                ManifestValue::Bytes(vec![0, 0, 0, 0, 0, 0, 0, 0]),
            ),
        ),
        manifest_field(
            0x1000_0002,
            manifest_layout("value-unset", [("tag", ManifestValue::Tag("VALUE_UNSET"))]),
        ),
    ];

    #[allow(unused_mut)]
    let mut encoded_operations = vec![
        manifest_command(
            "command-create",
            "COMMAND_CREATE",
            [
                ("alias", ManifestValue::U32(1)),
                (
                    "metadata",
                    manifest_metadata(Some("oracle"), &["first", "second"]),
                ),
                ("adopt", ManifestValue::Bool(false)),
            ],
        ),
        manifest_command(
            "command-delete",
            "COMMAND_DELETE",
            [("entity", manifest_reference_handle(handle_bits))],
        ),
        manifest_command(
            "command-metadata",
            "COMMAND_METADATA",
            [
                ("entity", manifest_reference_alias(31)),
                ("metadata", manifest_metadata(None, &[])),
            ],
        ),
        manifest_command(
            "command-insert",
            "COMMAND_INSERT",
            [
                ("entity", manifest_reference_alias(31)),
                ("component", ManifestValue::U16(321)),
                ("fields", ManifestValue::List(encoded_writes)),
                ("adopt", ManifestValue::Bool(false)),
            ],
        ),
        manifest_command(
            "command-set",
            "COMMAND_SET",
            [
                ("entity", manifest_reference_handle(handle_bits)),
                ("component", ManifestValue::U16(322)),
                (
                    "field",
                    manifest_field(
                        77,
                        manifest_typed_value(
                            "value-u32",
                            "VALUE_U32",
                            ManifestValue::U32(0xa1b2_c3d4),
                        ),
                    ),
                ),
            ],
        ),
        manifest_command(
            "command-remove",
            "COMMAND_REMOVE",
            [
                ("entity", manifest_reference_alias(31)),
                ("component", ManifestValue::U16(323)),
            ],
        ),
        manifest_command(
            "command-place-entity",
            "COMMAND_PLACE_ENTITY",
            [
                ("entity", manifest_reference_alias(31)),
                (
                    "placement",
                    manifest_placement(
                        Some(manifest_reference_handle(handle_bits)),
                        Some(manifest_reference_symbol("sibling")),
                    ),
                ),
            ],
        ),
        manifest_command(
            "command-delete-subtree",
            "COMMAND_DELETE_SUBTREE",
            [("root", manifest_reference_alias(31))],
        ),
    ];
    #[allow(unused_mut)]
    let mut operations = vec![
        Command::Create {
            alias: 1,
            metadata: metadata.clone(),
            adopt: false,
        },
        Command::Delete {
            entity: EntityRef::Handle(handle),
        },
        Command::SetMetadata {
            entity: EntityRef::Alias(31),
            metadata: EntityMetadata::default(),
        },
        Command::InsertComponent {
            entity: EntityRef::Alias(31),
            component: 321,
            fields: writes,
            adopt: false,
        },
        Command::SetField {
            entity: EntityRef::Handle(handle),
            component: 322,
            field: FieldWrite {
                offset: 77,
                value: FieldValue::U32(0xa1b2_c3d4),
            },
        },
        Command::RemoveComponent {
            entity: EntityRef::Alias(31),
            component: 323,
        },
        Command::PlaceEntity {
            entity: EntityRef::Alias(31),
            placement: ipp_core::EntityPlacementRef {
                parent: Some(EntityRef::Handle(handle)),
                before: Some(EntityRef::Symbol("sibling".into())),
            },
        },
        Command::DeleteSubtree {
            root: EntityRef::Alias(31),
        },
    ];

    // Adoption, symbolic references and compare-and-set.
    encoded_operations.extend([
        manifest_command(
            "command-create",
            "COMMAND_CREATE",
            [
                ("alias", ManifestValue::U32(2)),
                ("metadata", manifest_metadata(Some("adopted"), &[])),
                ("adopt", ManifestValue::Bool(true)),
            ],
        ),
        manifest_command(
            "command-insert",
            "COMMAND_INSERT",
            [
                ("entity", manifest_reference_symbol("adopted")),
                ("component", ManifestValue::U16(324)),
                ("fields", ManifestValue::List(Vec::new())),
                ("adopt", ManifestValue::Bool(true)),
            ],
        ),
        manifest_command(
            "command-set-field-if",
            "COMMAND_SET_FIELD_IF",
            [
                ("entity", manifest_reference_symbol("adopted")),
                ("component", ManifestValue::U16(325)),
                (
                    "field",
                    manifest_field(
                        12,
                        manifest_typed_value(
                            "value-string",
                            "VALUE_STRING",
                            ManifestValue::String("next".into()),
                        ),
                    ),
                ),
                (
                    "expected",
                    manifest_typed_value(
                        "value-string",
                        "VALUE_STRING",
                        ManifestValue::String("current".into()),
                    ),
                ),
            ],
        ),
        manifest_command(
            "command-delete",
            "COMMAND_DELETE",
            [("entity", manifest_reference_symbol("retired"))],
        ),
    ]);
    operations.extend([
        Command::Create {
            alias: 2,
            metadata: EntityMetadata {
                symbolic_id: Some("adopted".into()),
                classes: Vec::new(),
            },
            adopt: true,
        },
        Command::InsertComponent {
            entity: EntityRef::Symbol("adopted".into()),
            component: 324,
            fields: Vec::new(),
            adopt: true,
        },
        Command::set_field_if(
            EntityRef::Symbol("adopted".into()),
            325,
            FieldWrite {
                offset: 12,
                value: FieldValue::String("next".into()),
            },
            FieldValue::String("current".into()),
        ),
        Command::Delete {
            entity: EntityRef::Symbol("retired".into()),
        },
    ]);

    encoded_operations.push(manifest_command(
        "command-set-dynamic-property",
        "COMMAND_SET_DYNAMIC_PROPERTY",
        [
            ("entity", manifest_reference_handle(handle_bits)),
            ("component", ManifestValue::U16(20)),
            ("name", ManifestValue::String("tint".into())),
            (
                "value",
                ManifestValue::Bytes(ipp_core::DynamicValue::Vec3([0.25; 3]).encode()),
            ),
        ],
    ));
    operations.push(Command::SetDynamicProperty {
        entity: EntityRef::Handle(EntityId::from_bits(handle_bits)),
        component: 20,
        name: "tint".into(),
        value: ipp_core::DynamicValue::Vec3([0.25; 3]),
    });
    let asset = ipp_core::DynamicValue::Asset(ipp_core::services::asset_management::AssetSource {
        kind: ipp_core::MESH_TYPE,
        uri: "file:///model.mesh".into(),
        variant: 7,
    });
    encoded_operations.push(manifest_command(
        "command-set-dynamic-property",
        "COMMAND_SET_DYNAMIC_PROPERTY",
        [
            ("entity", manifest_reference_handle(handle_bits)),
            ("component", ManifestValue::U16(20)),
            ("name", ManifestValue::String("geometry".into())),
            ("value", ManifestValue::Bytes(asset.encode())),
        ],
    ));
    operations.push(Command::SetDynamicProperty {
        entity: EntityRef::Handle(EntityId::from_bits(handle_bits)),
        component: 20,
        name: "geometry".into(),
        value: asset,
    });
    encoded_operations.push(manifest_command(
        "command-remove-dynamic-property",
        "COMMAND_REMOVE_DYNAMIC_PROPERTY",
        [
            ("entity", manifest_reference_handle(handle_bits)),
            ("component", ManifestValue::U16(20)),
            ("name", ManifestValue::String("old".into())),
        ],
    ));
    operations.push(Command::RemoveDynamicProperty {
        entity: EntityRef::Handle(EntityId::from_bits(handle_bits)),
        component: 20,
        name: "old".into(),
    });
    #[cfg(feature = "gui")]
    {
        use ipp_core::systems::gui::local::GuiLocalAction;

        let vector = |x: f32, y: f32| {
            manifest_layout(
                "gui-input-vector",
                [("x", ManifestValue::F32(x)), ("y", ManifestValue::F32(y))],
            )
        };
        for (layout, tag, values, action) in [
            (
                "gui-action-press",
                "GUI_ACTION_PRESS",
                vec![],
                GuiLocalAction::Press,
            ),
            (
                "gui-action-toggle",
                "GUI_ACTION_TOGGLE",
                vec![],
                GuiLocalAction::Toggle,
            ),
            (
                "gui-action-set-scalar",
                "GUI_ACTION_SET_SCALAR",
                vec![("value", ManifestValue::F32(0.5))],
                GuiLocalAction::SetScalar(0.5),
            ),
            (
                "gui-action-set-text",
                "GUI_ACTION_SET_TEXT",
                vec![("text", ManifestValue::String("typed".into()))],
                GuiLocalAction::SetText("typed".into()),
            ),
            (
                "gui-action-focus",
                "GUI_ACTION_FOCUS",
                vec![],
                GuiLocalAction::Focus,
            ),
            (
                "gui-action-blur",
                "GUI_ACTION_BLUR",
                vec![],
                GuiLocalAction::Blur,
            ),
            (
                "gui-action-submit",
                "GUI_ACTION_SUBMIT",
                vec![],
                GuiLocalAction::Submit,
            ),
            (
                "gui-action-scroll-to",
                "GUI_ACTION_SCROLL_TO",
                vec![("offset", vector(12.0, 34.0))],
                GuiLocalAction::ScrollTo([12.0, 34.0]),
            ),
            (
                "gui-action-scroll-by",
                "GUI_ACTION_SCROLL_BY",
                vec![("delta", vector(-12.0, 34.0))],
                GuiLocalAction::ScrollBy([-12.0, 34.0]),
            ),
            (
                "gui-action-scroll-to-index",
                "GUI_ACTION_SCROLL_TO_INDEX",
                vec![
                    ("index", ManifestValue::U32(40)),
                    ("offset", ManifestValue::F32(2.0)),
                ],
                GuiLocalAction::ScrollToIndex {
                    index: 40,
                    offset: 2.0,
                },
            ),
        ] {
            encoded_operations.push(manifest_command(
                "command-gui-action",
                "COMMAND_GUI_ACTION",
                [
                    ("entity", manifest_reference_handle(handle_bits)),
                    ("component", ManifestValue::U16(38)),
                    ("incarnation", ManifestValue::U64(9)),
                    (
                        "action",
                        manifest_layout(
                            layout,
                            std::iter::once(("tag", ManifestValue::Tag(tag))).chain(values),
                        ),
                    ),
                ],
            ));
            operations.push(Command::GuiAction {
                target: ipp_core::GuiActionTarget {
                    entity: EntityRef::Handle(EntityId::from_bits(handle_bits)),
                    component: 38,
                    incarnation: 9,
                },
                action,
            });
        }
    }
    let fixture = ManifestFixture::new(
        "request-submit-batch",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(17)),
            ("tag", ManifestValue::Tag("REQUEST_SUBMIT_BATCH")),
            ("batch_id", ManifestValue::U32(27)),
            ("last", ManifestValue::Bool(true)),
            ("operations", ManifestValue::List(encoded_operations)),
        ],
    );
    let mut covered = BTreeSet::new();
    let decoded = decode_request(&encode_manifest_fixture(&fixture, &mut covered), 7).unwrap();
    assert_eq!(
        decoded,
        Request {
            session: 7,
            request_id: 17,
            body: RequestBody::SubmitBatch(crate::BatchPage {
                batch_id: 27,
                last: true,
                operations
            }),
        }
    );

    let page = ManifestFixture::new(
        "request-submit-batch",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(0)),
            ("tag", ManifestValue::Tag("REQUEST_SUBMIT_BATCH")),
            ("batch_id", ManifestValue::U32(u32::MAX)),
            ("last", ManifestValue::Bool(false)),
            ("operations", ManifestValue::List(vec![])),
        ],
    );
    assert_eq!(
        decode_request(&encode_manifest_fixture(&page, &mut covered), 7)
            .unwrap()
            .body,
        RequestBody::SubmitBatch(crate::BatchPage {
            batch_id: u32::MAX,
            last: false,
            operations: vec![],
        })
    );

    for (collection, tag) in [
        (0, "INSPECT_SUMMARY"),
        (1, "INSPECT_ENTITIES"),
        (2, "INSPECT_RESOURCES"),
        (3, "INSPECT_CONTROLLERS"),
        (4, "INSPECT_RENDER_DIAGNOSTICS"),
        (5, "INSPECT_ENTITY_TREE"),
        #[cfg(feature = "gui")]
        (6, "INSPECT_GUI_FOCUS"),
        #[cfg(feature = "gui")]
        (7, "INSPECT_GUI_POINTERS"),
        #[cfg(feature = "surfaces")]
        (8, "INSPECT_CANVAS"),
    ] {
        let inspect = ManifestFixture::new(
            "request-inspect",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(18)),
                ("tag", ManifestValue::Tag("REQUEST_INSPECT")),
                ("collection", ManifestValue::Tag(tag)),
                ("after", ManifestValue::U64(0)),
                ("target", ManifestValue::U64(0)),
                ("limit", ManifestValue::U16(256)),
                ("max_depth", ManifestValue::U16(0)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&inspect, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::Inspect(crate::InspectionQuery {
                collection,
                ..Default::default()
            })
        );
    }

    let tree = ManifestFixture::new(
        "request-inspect",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(19)),
            ("tag", ManifestValue::Tag("REQUEST_INSPECT")),
            ("collection", ManifestValue::Tag("INSPECT_ENTITY_TREE")),
            ("after", ManifestValue::U64(42)),
            ("target", ManifestValue::U64(41)),
            ("limit", ManifestValue::U16(1)),
            ("max_depth", ManifestValue::U16(2)),
        ],
    );
    assert_eq!(
        decode_request(&encode_manifest_fixture(&tree, &mut covered), 7)
            .unwrap()
            .body,
        RequestBody::Inspect(crate::InspectionQuery {
            collection: 5,
            after: 42,
            target: 41,
            limit: 1,
            max_depth: 2,
        })
    );

    {
        use ipp_core::systems::animation::*;
        for (layout, tag, command) in [
            (
                "request-controller-create",
                "REQUEST_CONTROLLER_CREATE",
                AnimationControllerCommand::Create(AnimationControllerDescription::default()),
            ),
            (
                "request-controller-update",
                "REQUEST_CONTROLLER_UPDATE",
                AnimationControllerCommand::Update {
                    id: AnimationControllerId::from_bits(42),
                    description: AnimationControllerDescription::default(),
                },
            ),
            (
                "request-controller-delete",
                "REQUEST_CONTROLLER_DELETE",
                AnimationControllerCommand::Delete {
                    id: AnimationControllerId::from_bits(42),
                },
            ),
            (
                "request-controller-control",
                "REQUEST_CONTROLLER_CONTROL",
                AnimationControllerCommand::Control {
                    id: AnimationControllerId::from_bits(42),
                    control: AnimationPlaybackControl::Seek(1.25),
                },
            ),
            (
                "request-controller-transition",
                "REQUEST_CONTROLLER_TRANSITION",
                AnimationControllerCommand::Transition {
                    id: AnimationControllerId::from_bits(42),
                    transition: AnimationControllerTransition {
                        description: AnimationControllerDescription::default(),
                        duration: 0.5,
                        easing: AnimationTransitionEasing::Smoothstep,
                        start_time: AnimationTransitionStartTime::Seek(1.25),
                    },
                },
            ),
        ] {
            let mut values = vec![
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(19)),
                ("tag", ManifestValue::Tag(tag)),
            ];
            if !matches!(command, AnimationControllerCommand::Create(_)) {
                values.push(("id", ManifestValue::U64(42)));
            }
            match &command {
                AnimationControllerCommand::Create(_)
                | AnimationControllerCommand::Update {
                    ..
                } => values.push((
                    "description",
                    manifest_layout(
                        "controller-description",
                        [
                            ("speed", ManifestValue::F32(1.0)),
                            ("looping", ManifestValue::Bool(false)),
                            ("drivers", ManifestValue::List(Vec::new())),
                        ],
                    ),
                )),
                AnimationControllerCommand::Control {
                    ..
                } => {
                    values.push(("control", ManifestValue::U32(3)));
                    values.push(("time", ManifestValue::F64(1.25)));
                    values.push(("speed", ManifestValue::F32(0.0)));
                }
                AnimationControllerCommand::Transition {
                    transition,
                    ..
                } => {
                    values.push((
                        "description",
                        manifest_layout(
                            "controller-description",
                            [
                                ("speed", ManifestValue::F32(1.0)),
                                ("looping", ManifestValue::Bool(false)),
                                ("drivers", ManifestValue::List(Vec::new())),
                            ],
                        ),
                    ));
                    values.push(("duration", ManifestValue::F64(transition.duration)));
                    values.push(("easing", ManifestValue::U32(1)));
                    values.push(("start_time", ManifestValue::U32(3)));
                    values.push(("seek_time", ManifestValue::F64(1.25)));
                }
                _ => {}
            }
            assert_eq!(
                decode_request(
                    &encode_manifest_fixture(&ManifestFixture::new(layout, values), &mut covered),
                    7
                )
                .unwrap()
                .body,
                RequestBody::AnimationController(command)
            );
        }
    }
    for action in 0..=5 {
        let time = if action == 3 {
            1.25
        } else {
            0.0
        };
        let speed = if action == 5 {
            -1.5
        } else {
            0.0
        };
        let fixture = ManifestFixture::new(
            "request-playback",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tag", ManifestValue::Tag("REQUEST_PLAYBACK")),
                ("controller", ManifestValue::U64(42)),
                ("control", ManifestValue::U32(action)),
                ("time", ManifestValue::F64(time)),
                ("speed", ManifestValue::F32(speed)),
            ],
        );
        let expected = match action {
            0 => ipp_core::systems::animation::AnimationPlaybackControl::Play,
            1 => ipp_core::systems::animation::AnimationPlaybackControl::Pause,
            2 => ipp_core::systems::animation::AnimationPlaybackControl::Stop,
            3 => ipp_core::systems::animation::AnimationPlaybackControl::Seek(time),
            4 => ipp_core::systems::animation::AnimationPlaybackControl::Restart,
            _ => ipp_core::systems::animation::AnimationPlaybackControl::PlayAtSpeed(speed),
        };
        assert_eq!(
            decode_request(&encode_manifest_fixture(&fixture, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::AnimationPlaybackCommand {
                controller: ipp_core::systems::animation::AnimationControllerId::from_bits(42),
                control: expected
            }
        );
    }
    view_tests::requests(&mut covered);
    navigation_request_fixtures(&mut covered);

    for patch in render_state_patches() {
        let request = ManifestFixture::new(
            "request-render-state-update",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tag", ManifestValue::Tag("REQUEST_RENDER_STATE_UPDATE")),
                ("changes", manifest_render_patch(&patch)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::RenderStateUpdateCommand(patch)
        );
    }

    #[cfg(feature = "surfaces")]
    for (mask, extent, units_per_metre) in [
        (1, Some([640.0, 360.0]), None),
        (2, None, Some(400.0)),
        (3, Some([320.0, 180.0]), Some(200.0)),
    ] {
        let update = [
            ("mask", ManifestValue::U16(mask)),
            (
                "extent",
                extent.map_or(ManifestValue::None, |[width, height]| {
                    ManifestValue::Some(Box::new(manifest_layout(
                        "canvas-extent",
                        [
                            ("width", ManifestValue::F32(width)),
                            ("height", ManifestValue::F32(height)),
                        ],
                    )))
                }),
            ),
            (
                "density",
                units_per_metre.map_or(ManifestValue::None, |density| {
                    ManifestValue::Some(Box::new(manifest_layout(
                        "canvas-density",
                        [("units_per_metre", ManifestValue::F32(density))],
                    )))
                }),
            ),
        ];
        let request = ManifestFixture::new(
            "request-canvas-state-update",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tag", ManifestValue::Tag("REQUEST_CANVAS_STATE_UPDATE")),
                ("update", manifest_layout("canvas-state-update", update)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::CanvasStateUpdateCommand(ipp_core::CanvasStateUpdate {
                extent,
                units_per_metre,
            })
        );
    }

    lifecycle_request_fixtures(&mut covered);
    lifecycle_watch_tests::requests(&mut covered);
    attachment_tests::requests(&mut covered);

    #[cfg(feature = "gui")]
    {
        let world = crate::references::WorldReference {
            id: 1,
            incarnation: 2,
        };
        let mut subscribe = vec![0];
        subscribe.extend(1u64.to_le_bytes());
        subscribe.extend(2u64.to_le_bytes());
        subscribe.push(2);
        let mut unsubscribe = vec![1];
        unsubscribe.extend(1u64.to_le_bytes());
        unsubscribe.extend(2u64.to_le_bytes());
        unsubscribe.extend(3u64.to_le_bytes());
        unsubscribe.extend(4u64.to_le_bytes());
        for (control, expected) in [
            (
                subscribe,
                crate::gui::GuiObservationRequest::Subscribe {
                    world,
                    classes: ipp_core::systems::gui::observations::GuiObservationClasses::All,
                },
            ),
            (
                unsubscribe,
                crate::gui::GuiObservationRequest::Unsubscribe {
                    world,
                    subscription:
                        ipp_core::systems::gui::observations::GuiObservationSubscriptionId {
                            output: 3,
                            generation: 4,
                        },
                },
            ),
        ] {
            let fixture = ManifestFixture::new(
                "request-gui-observation",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(1)),
                    ("tag", ManifestValue::Tag("REQUEST_GUI_OBSERVATION")),
                    ("control", ManifestValue::Bytes(control)),
                ],
            );
            assert_eq!(
                decode_request(&encode_manifest_fixture(&fixture, &mut covered), 7)
                    .unwrap()
                    .body,
                RequestBody::GuiObservation(expected)
            );
        }
    }

    #[cfg(feature = "diagnostics")]
    lifecycle_diagnostics_tests::requests(&mut covered);

    let expected = TAGS
        .iter()
        .filter(|tag| {
            matches!(
                tag.space,
                TagSpace::Value
                    | TagSpace::Request
                    | TagSpace::Command
                    | TagSpace::Reference
                    | TagSpace::Option
                    | TagSpace::ViewTarget
                    | TagSpace::OutputTarget
                    | TagSpace::InspectionCollection
                    | TagSpace::LifecycleWatchChange
                    | TagSpace::LifecycleWatchTarget
                    | TagSpace::LifecycleWatchKinds
                    | TagSpace::GuiAction
            )
        })
        .map(|tag| tag.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(covered, expected, "request manifest tag coverage drifted");
}

fn assert_manifest_response(
    response: Response,
    fixture: ManifestFixture,
    covered: &mut BTreeSet<&'static str>,
) {
    assert_eq!(
        encode_response(&response).unwrap(),
        encode_manifest_fixture(&fixture, covered),
        "Rust response codec diverged from {}",
        fixture.layout
    );
}

#[test]
fn rust_response_encoder_conforms_to_every_enabled_manifest_branch() {
    let mut covered = BTreeSet::new();
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 0,
            tick: 1,
            body: ResponseBody::BatchAborted {
                batch_id: 27,
                message: "deadline".into(),
            },
        },
        ManifestFixture::new(
            "response-batch-aborted",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tick", ManifestValue::U64(1)),
                ("tag", ManifestValue::Tag("RESPONSE_BATCH_ABORTED")),
                ("batch_id", ManifestValue::U64(27)),
                ("message", ManifestValue::String("deadline".into())),
            ],
        ),
        &mut covered,
    );

    let entity = EntityId::from_bits(0x0000_0001_0000_0029);

    let resolved_values = [
        (
            88,
            ResolvedValue::Dynamic(ipp_core::DynamicValue::Mat2([1.0, 0.0, 0.0, 1.0])),
            manifest_typed_value(
                "snapshot-value-dynamic",
                "SNAPSHOT_VALUE_DYNAMIC",
                ManifestValue::Bytes(ipp_core::DynamicValue::Mat2([1.0, 0.0, 0.0, 1.0]).encode()),
            ),
        ),
        (
            77,
            ResolvedValue::Bool(false),
            manifest_typed_value(
                "snapshot-value-bool",
                "SNAPSHOT_VALUE_BOOL",
                ManifestValue::Bool(false),
            ),
        ),
        (
            11,
            ResolvedValue::F32(1.25),
            manifest_typed_value(
                "snapshot-value-f32",
                "SNAPSHOT_VALUE_F32",
                ManifestValue::F32(1.25),
            ),
        ),
        (
            22,
            ResolvedValue::Entity(entity),
            manifest_layout(
                "snapshot-value-entity",
                [
                    ("tag", ManifestValue::Tag("SNAPSHOT_VALUE_ENTITY")),
                    ("reference_tag", ManifestValue::Tag("SNAPSHOT_REF_HANDLE")),
                    ("value", ManifestValue::U64(entity.to_bits())),
                ],
            ),
        ),
        (
            33,
            ResolvedValue::U32(0x1234_5678),
            manifest_typed_value(
                "snapshot-value-u32",
                "SNAPSHOT_VALUE_U32",
                ManifestValue::U32(0x1234_5678),
            ),
        ),
        (
            44,
            ResolvedValue::U64(0x1020_3040_5060_7080),
            manifest_typed_value(
                "snapshot-value-u64",
                "SNAPSHOT_VALUE_U64",
                ManifestValue::U64(0x1020_3040_5060_7080),
            ),
        ),
        (
            55,
            ResolvedValue::String("snapshot-value".into()),
            manifest_typed_value(
                "snapshot-value-string",
                "SNAPSHOT_VALUE_STRING",
                ManifestValue::String("snapshot-value".into()),
            ),
        ),
        (
            66,
            ResolvedValue::Bytes(vec![9, 7, 5]),
            manifest_typed_value(
                "snapshot-value-bytes",
                "SNAPSHOT_VALUE_BYTES",
                ManifestValue::Bytes(vec![9, 7, 5]),
            ),
        ),
        (
            99,
            ResolvedValue::Rows(vec![1, 0, 0, 0, 0, 0, 0, 0]),
            manifest_typed_value(
                "snapshot-value-rows",
                "SNAPSHOT_VALUE_ROWS",
                ManifestValue::Bytes(vec![1, 0, 0, 0, 0, 0, 0, 0]),
            ),
        ),
        (
            0x1000_0002,
            ResolvedValue::Unset,
            manifest_layout(
                "snapshot-value-unset",
                [("tag", ManifestValue::Tag("SNAPSHOT_VALUE_UNSET"))],
            ),
        ),
    ];
    for (offset, value, expected) in resolved_values.into_iter().chain([
        (
            101,
            ResolvedValue::World(None),
            manifest_typed_value(
                "snapshot-value-world",
                "SNAPSHOT_VALUE_WORLD",
                ManifestValue::None,
            ),
        ),
        (
            102,
            ResolvedValue::Output(None),
            manifest_typed_value(
                "snapshot-value-output",
                "SNAPSHOT_VALUE_OUTPUT",
                ManifestValue::None,
            ),
        ),
    ]) {
        let mut writer = Writer::new(Vec::new());
        writer.resolved_field(offset, value).unwrap();
        let ManifestValue::Layout(fixture) = manifest_snapshot_field(offset, expected) else {
            unreachable!()
        };
        assert_eq!(
            writer.0,
            encode_manifest_fixture(&fixture, &mut covered),
            "resolved snapshot field {offset}"
        );
    }

    #[allow(unused_mut)]
    let mut success_values = vec![
        ("batch_id", ManifestValue::U64(27)),
        ("tick", ManifestValue::U64(37)),
        ("tag", ManifestValue::Tag("OUTCOME_SUCCESS")),
        (
            "aliases",
            ManifestValue::List(vec![manifest_layout(
                "alias-handle",
                [
                    ("alias", ManifestValue::U32(1)),
                    ("handle", ManifestValue::U64(entity.to_bits())),
                ],
            )]),
        ),
    ];
    success_values.push((
        "symbols",
        ManifestValue::List(vec![
            manifest_layout(
                "symbol-handle",
                [
                    ("symbol", ManifestValue::String("oracle".into())),
                    ("handle", ManifestValue::U64(entity.to_bits())),
                ],
            ),
            manifest_layout(
                "symbol-handle",
                [
                    ("symbol", ManifestValue::String("peer".into())),
                    ("handle", ManifestValue::U64(17)),
                ],
            ),
        ]),
    ));
    let success = manifest_layout("outcome-success", success_values);
    let success_fixture = ManifestFixture::new(
        "response-batch",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(17)),
            ("tick", ManifestValue::U64(37)),
            ("tag", ManifestValue::Tag("RESPONSE_BATCH")),
            ("outcome", success),
            ("effects", ManifestValue::List(vec![])),
        ],
    );
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 17,
            tick: 37,
            body: ResponseBody::Batch(crate::attachment_receipts::ReceiptBatchOutcome {
                outcome: BatchOutcome {
                    batch_id: 27,
                    tick: 37,
                    result: Ok(vec![(1, entity)]),
                    effects: vec![],
                    symbols: vec![
                        ("oracle".into(), entity),
                        ("peer".into(), EntityId::from_bits(17)),
                    ],
                },
                effects: vec![],
            }),
        },
        success_fixture,
        &mut covered,
    );

    for (scope, tag) in [
        (crate::RuntimeFailureScope::Draw, "FAILURE_DRAW"),
        (crate::RuntimeFailureScope::Resource, "FAILURE_RESOURCE"),
        (crate::RuntimeFailureScope::Context, "FAILURE_CONTEXT"),
        (crate::RuntimeFailureScope::World, "FAILURE_WORLD"),
    ] {
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 0,
                tick: 38,
                body: ResponseBody::RuntimeFailure {
                    scope,
                    faulted: false,
                    message: "recoverable".into(),
                },
            },
            ManifestFixture::new(
                "response-runtime-failure",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(0)),
                    ("tick", ManifestValue::U64(38)),
                    ("tag", ManifestValue::Tag("RESPONSE_RUNTIME_FAILURE")),
                    ("scope", ManifestValue::Tag(tag)),
                    ("faulted", ManifestValue::Bool(false)),
                    ("message", ManifestValue::String("recoverable".into())),
                ],
            ),
            &mut covered,
        );
    }

    let failure = manifest_layout(
        "outcome-failure",
        [
            ("batch_id", ManifestValue::U64(28)),
            ("tick", ManifestValue::U64(38)),
            ("tag", ManifestValue::Tag("OUTCOME_FAILURE")),
            ("scope", ManifestValue::Tag("BATCH_ERROR_OPERATION")),
            (
                "operation",
                ManifestValue::Some(Box::new(ManifestValue::U32(0x1234_5678))),
            ),
            ("reason", ManifestValue::String("InvalidField".into())),
            (
                "aliases",
                ManifestValue::List(vec![manifest_layout(
                    "alias-handle",
                    [
                        ("alias", ManifestValue::U32(9)),
                        ("handle", ManifestValue::U64(17)),
                    ],
                )]),
            ),
            (
                "symbols",
                ManifestValue::List(vec![manifest_layout(
                    "symbol-handle",
                    [
                        ("symbol", ManifestValue::String("failed".into())),
                        ("handle", ManifestValue::U64(18)),
                    ],
                )]),
            ),
        ],
    );
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 18,
            tick: 38,
            body: ResponseBody::Batch(crate::attachment_receipts::ReceiptBatchOutcome {
                outcome: BatchOutcome {
                    batch_id: 28,
                    tick: 38,
                    result: Err(ipp_core::BatchError {
                        scope: ipp_core::BatchErrorScope::Operation,
                        operation: Some(0x1234_5678),
                        reason: ipp_core::ErrorReason::InvalidField,
                        aliases: vec![(9, EntityId::from_bits(17))],
                    }),
                    symbols: vec![("failed".into(), EntityId::from_bits(18))],
                    effects: Vec::new(),
                },
                effects: vec![],
            }),
        },
        ManifestFixture::new(
            "response-batch",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(18)),
                ("tick", ManifestValue::U64(38)),
                ("tag", ManifestValue::Tag("RESPONSE_BATCH")),
                ("outcome", failure),
                ("effects", ManifestValue::List(vec![])),
            ],
        ),
        &mut covered,
    );

    assert_manifest_response(
        Response {
            session: 7,
            request_id: 0,
            tick: 39,
            body: ResponseBody::Frame {
                time: 1.75,
            },
        },
        ManifestFixture::new(
            "response-frame",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tick", ManifestValue::U64(39)),
                ("tag", ManifestValue::Tag("RESPONSE_FRAME")),
                ("time", ManifestValue::F64(1.75)),
            ],
        ),
        &mut covered,
    );

    let scalar_offset = std::mem::offset_of!(ipp_core::components::Scalar, value) as u32;
    let scalar = ipp_core::ComponentValue::Scalar(ipp_core::components::Scalar {
        value: 2.5,
    });
    let component = manifest_layout(
        "component",
        [
            ("type_id", ManifestValue::U16(1)),
            (
                "fields",
                ManifestValue::List(vec![manifest_snapshot_field(
                    scalar_offset,
                    manifest_typed_value(
                        "snapshot-value-f32",
                        "SNAPSHOT_VALUE_F32",
                        ManifestValue::F32(2.5),
                    ),
                )]),
            ),
        ],
    );
    let inspected_entity = manifest_layout(
        "entity",
        [
            ("id", ManifestValue::U64(entity.to_bits())),
            ("metadata", manifest_metadata(None, &["inspected"])),
            ("parent", ManifestValue::U64(0)),
            ("order_low", ManifestValue::U64(1)),
            ("order_high", ManifestValue::U64(0)),
            ("components", ManifestValue::List(vec![component])),
        ],
    );
    {
        use ipp_core::systems::animation::*;
        let controller = AnimationControllerState {
            id: AnimationControllerId::from_bits(42),
            state: AnimationPlaybackStatus::Paused,
            time: 1.25,
        };
        let event = AnimationPlaybackEvent {
            controller,
            kind: AnimationPlaybackEventKind::Paused,
            reason: None,
        };
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 0,
                tick: 40,
                body: ResponseBody::PlaybackEvents(vec![event]),
            },
            ManifestFixture::new(
                "response-playback",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(0)),
                    ("tick", ManifestValue::U64(40)),
                    ("tag", ManifestValue::Tag("RESPONSE_PLAYBACK")),
                    (
                        "events",
                        ManifestValue::List(vec![manifest_layout(
                            "playback-event",
                            [
                                (
                                    "controller",
                                    manifest_layout(
                                        "controller-state",
                                        [
                                            ("id", ManifestValue::U64(42)),
                                            ("state", ManifestValue::U32(2)),
                                            ("time", ManifestValue::F64(1.25)),
                                        ],
                                    ),
                                ),
                                ("kind", ManifestValue::U32(1)),
                                ("reason", ManifestValue::String(String::new())),
                            ],
                        )]),
                    ),
                ],
            ),
            &mut covered,
        );
    }
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 21,
            tick: 40,
            body: ResponseBody::AnimationController(Some(
                ipp_core::systems::animation::AnimationControllerId::from_bits(42),
            )),
        },
        ManifestFixture::new(
            "response-controller",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(21)),
                ("tick", ManifestValue::U64(40)),
                ("tag", ManifestValue::Tag("RESPONSE_CONTROLLER")),
                ("id", ManifestValue::U64(42)),
            ],
        ),
        &mut covered,
    );
    let mut inspect_values = vec![
        ("session", ManifestValue::U64(7)),
        ("request_id", ManifestValue::U64(20)),
        ("tick", ManifestValue::U64(40)),
        ("tag", ManifestValue::Tag("RESPONSE_INSPECT")),
        ("time", ManifestValue::F64(2.25)),
        ("next", ManifestValue::U64(0)),
        ("entities", ManifestValue::List(vec![inspected_entity])),
        ("resources", ManifestValue::List(Vec::new())),
        ("render_diagnostics", ManifestValue::List(Vec::new())),
    ];
    let inspected_controller = ipp_core::systems::animation::AnimationControllerSnapshot {
        id: ipp_core::systems::animation::AnimationControllerId::from_bits(42),
        description: ipp_core::systems::animation::AnimationControllerDescription {
            speed: -1.0,
            ..Default::default()
        },
        state: ipp_core::systems::animation::AnimationPlaybackStatus::Paused,
        time: 1.25,
        transition: Some(
            ipp_core::systems::animation::AnimationControllerTransitionState {
                duration: 0.5,
                elapsed: 0.25,
                easing: ipp_core::systems::animation::AnimationTransitionEasing::Smoothstep,
                pending: false,
            },
        ),
    };
    inspect_values.push((
        "controllers",
        ManifestValue::List(vec![manifest_layout(
            "animation-controller",
            [
                (
                    "state",
                    manifest_layout(
                        "controller-state",
                        [
                            ("id", ManifestValue::U64(42)),
                            ("state", ManifestValue::U32(2)),
                            ("time", ManifestValue::F64(1.25)),
                        ],
                    ),
                ),
                (
                    "description",
                    manifest_layout(
                        "controller-description",
                        [
                            ("speed", ManifestValue::F32(-1.0)),
                            ("looping", ManifestValue::Bool(false)),
                            ("drivers", ManifestValue::List(Vec::new())),
                        ],
                    ),
                ),
                (
                    "transition",
                    ManifestValue::Some(Box::new(manifest_layout(
                        "controller-transition-state",
                        [
                            ("duration", ManifestValue::F64(0.5)),
                            ("elapsed", ManifestValue::F64(0.25)),
                            ("easing", ManifestValue::U32(1)),
                            ("pending", ManifestValue::Bool(false)),
                        ],
                    ))),
                ),
            ],
        )]),
    ));
    // In GUI builds the GUI System query collections follow the controllers.
    #[cfg(feature = "gui")]
    let (gui_focus, gui_pointers) = {
        use ipp_core::systems::gui::local::{
            GuiEntityTarget, GuiFocusRecord, GuiInteractionFlags, GuiPointerRecord,
        };
        let mut host = ipp_core::HostRuntime::new();
        let world = host.create_world(Default::default(), &[]).unwrap();
        let world = host.world_ref(world).unwrap();
        let target = GuiEntityTarget {
            world,
            entity,
            component: ComponentValue::GUI_BUTTON,
            incarnation: 9,
        };
        let manifest_target = || {
            manifest_layout(
                "gui-target",
                [
                    (
                        "world",
                        manifest_layout(
                            "world-reference",
                            [
                                ("id", ManifestValue::U64(world.id().0)),
                                ("incarnation", ManifestValue::U64(world.incarnation())),
                            ],
                        ),
                    ),
                    ("entity", ManifestValue::U64(entity.to_bits())),
                    ("component", ManifestValue::U16(ComponentValue::GUI_BUTTON)),
                    ("incarnation", ManifestValue::U64(9)),
                ],
            )
        };
        inspect_values.push((
            "gui_focus",
            ManifestValue::List(vec![manifest_layout(
                "gui-focus",
                [
                    ("target", manifest_target()),
                    ("visible", ManifestValue::Bool(true)),
                ],
            )]),
        ));
        inspect_values.push((
            "gui_pointers",
            ManifestValue::List(
                [(3, true, false, true), (4, false, true, false)]
                    .into_iter()
                    .map(|(pointer, hovered, pressed, captured)| {
                        manifest_layout(
                            "gui-pointer",
                            [
                                ("target", manifest_target()),
                                ("pointer", ManifestValue::U64(pointer)),
                                ("hovered", ManifestValue::Bool(hovered)),
                                ("pressed", ManifestValue::Bool(pressed)),
                                ("captured", ManifestValue::Bool(captured)),
                            ],
                        )
                    })
                    .collect(),
            ),
        ));
        (
            vec![GuiFocusRecord {
                target,
                visible: true,
            }],
            [(3, true, false, true), (4, false, true, false)]
                .into_iter()
                .map(|(pointer, hovered, pressed, captured)| GuiPointerRecord {
                    target,
                    pointer,
                    state: GuiInteractionFlags {
                        hovered,
                        pressed,
                        captured,
                    },
                })
                .collect::<Vec<_>>(),
        )
    };
    // In Surface builds the Canvas System query record follows.
    #[cfg(feature = "surfaces")]
    let canvas = {
        let extent = |width, height| {
            manifest_layout(
                "canvas-extent",
                [
                    ("width", ManifestValue::F32(width)),
                    ("height", ManifestValue::F32(height)),
                ],
            )
        };
        inspect_values.push((
            "canvas",
            ManifestValue::Some(Box::new(manifest_layout(
                "canvas-state-record",
                [
                    (
                        "state",
                        manifest_layout(
                            "canvas-state",
                            [
                                ("extent", extent(640.0, 360.0)),
                                (
                                    "density",
                                    manifest_layout(
                                        "canvas-density",
                                        [("units_per_metre", ManifestValue::F32(400.0))],
                                    ),
                                ),
                            ],
                        ),
                    ),
                    (
                        "evaluated",
                        ManifestValue::Some(Box::new(manifest_layout(
                            "canvas-evaluated-extent",
                            [
                                ("extent", extent(320.0, 180.0)),
                                ("tick", ManifestValue::U64(39)),
                            ],
                        ))),
                    ),
                ],
            ))),
        ));
        Some(ipp_core::CanvasStateRecord {
            state: ipp_core::CanvasState {
                extent: [640.0, 360.0],
                units_per_metre: 400.0,
            },
            evaluated: Some(ipp_core::CanvasEvaluatedExtent {
                extent: [320.0, 180.0],
                tick: 39,
            }),
        })
    };
    inspect_values.shrink_to_fit();

    assert_manifest_response(
        Response {
            session: 7,
            request_id: 20,
            tick: 40,
            body: ResponseBody::Inspect {
                next: 0,
                controllers: vec![inspected_controller],
                time: 2.25,
                entities: vec![ipp_core::EntitySnapshot {
                    id: entity,
                    metadata: EntityMetadata {
                        symbolic_id: None,
                        classes: vec!["inspected".into()],
                    },
                    link: test_link(),
                    components: vec![scalar],
                }],
                resources: Vec::new(),
                render_diagnostics: Vec::new(),
                #[cfg(feature = "gui")]
                gui_focus,
                #[cfg(feature = "gui")]
                gui_pointers,
                #[cfg(feature = "surfaces")]
                canvas,
            },
        },
        ManifestFixture::new("response-inspect", inspect_values),
        &mut covered,
    );

    assert_manifest_response(
        Response {
            session: 7,
            request_id: 22,
            tick: 40,
            body: ResponseBody::EntityTree {
                next: 41,
                time: 2.25,
                nodes: vec![crate::EntityTreeNode {
                    id: ipp_core::EntityId::from_bits(41),
                    parent: Some(ipp_core::EntityId::from_bits(40)),
                    order: (1u128 << 80) | 17,
                    depth: 1,
                }],
            },
        },
        ManifestFixture::new(
            "response-entity-tree",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(22)),
                ("tick", ManifestValue::U64(40)),
                ("tag", ManifestValue::Tag("RESPONSE_ENTITY_TREE")),
                ("time", ManifestValue::F64(2.25)),
                ("next", ManifestValue::U64(41)),
                (
                    "nodes",
                    ManifestValue::List(vec![manifest_layout(
                        "entity-tree-node",
                        [
                            ("id", ManifestValue::U64(41)),
                            ("parent", ManifestValue::U64(40)),
                            ("order_low", ManifestValue::U64(17)),
                            ("order_high", ManifestValue::U64(1 << 16)),
                            ("depth", ManifestValue::U16(1)),
                        ],
                    )]),
                ),
            ],
        ),
        &mut covered,
    );

    // A dynamic component carries its own named-property descriptor table.
    let mut material = ipp_core::components::CustomMaterial::default();
    material
        .properties
        .set("tint", ipp_core::DynamicValue::F32(0.5))
        .unwrap();
    let material_fixture = |component: &ipp_core::components::CustomMaterial| {
        let fields = ComponentValue::CustomMaterial(component.clone())
            .fields()
            .into_iter()
            .map(|(offset, value)| {
                let value = match value {
                    ResolvedValue::Bytes(bytes) => manifest_typed_value(
                        "snapshot-value-bytes",
                        "SNAPSHOT_VALUE_BYTES",
                        ManifestValue::Bytes(bytes),
                    ),
                    ResolvedValue::Dynamic(value) => manifest_typed_value(
                        "snapshot-value-dynamic",
                        "SNAPSHOT_VALUE_DYNAMIC",
                        ManifestValue::Bytes(value.encode()),
                    ),
                    ResolvedValue::F32(value) => manifest_typed_value(
                        "snapshot-value-f32",
                        "SNAPSHOT_VALUE_F32",
                        ManifestValue::F32(value),
                    ),
                    ResolvedValue::U32(value) => manifest_typed_value(
                        "snapshot-value-u32",
                        "SNAPSHOT_VALUE_U32",
                        ManifestValue::U32(value),
                    ),
                    ResolvedValue::String(value) => manifest_typed_value(
                        "snapshot-value-string",
                        "SNAPSHOT_VALUE_STRING",
                        ManifestValue::String(value.to_string()),
                    ),
                    ResolvedValue::Bool(value) => manifest_typed_value(
                        "snapshot-value-bool",
                        "SNAPSHOT_VALUE_BOOL",
                        ManifestValue::Bool(value),
                    ),
                    other => panic!("unexpected CustomMaterial snapshot value {other:?}"),
                };
                manifest_snapshot_field(offset, value)
            })
            .collect();
        manifest_layout(
            "component",
            [
                (
                    "type_id",
                    ManifestValue::U16(ComponentValue::CustomMaterial(component.clone()).type_id()),
                ),
                ("fields", ManifestValue::List(fields)),
            ],
        )
    };
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 22,
            tick: 40,
            body: ResponseBody::Inspect {
                next: 0,
                controllers: Vec::new(),
                time: 2.25,
                entities: vec![ipp_core::EntitySnapshot {
                    id: entity,
                    metadata: EntityMetadata::default(),
                    link: test_link(),
                    components: vec![ComponentValue::CustomMaterial(material.clone())],
                }],
                resources: Vec::new(),
                render_diagnostics: Vec::new(),
                #[cfg(feature = "gui")]
                gui_focus: Vec::new(),
                #[cfg(feature = "gui")]
                gui_pointers: Vec::new(),
                #[cfg(feature = "surfaces")]
                canvas: None,
            },
        },
        ManifestFixture::new(
            "response-inspect",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(22)),
                ("tick", ManifestValue::U64(40)),
                ("tag", ManifestValue::Tag("RESPONSE_INSPECT")),
                ("time", ManifestValue::F64(2.25)),
                ("next", ManifestValue::U64(0)),
                (
                    "entities",
                    ManifestValue::List(vec![manifest_layout(
                        "entity",
                        [
                            ("id", ManifestValue::U64(entity.to_bits())),
                            ("metadata", manifest_metadata(None, &[])),
                            ("parent", ManifestValue::U64(0)),
                            ("order_low", ManifestValue::U64(1)),
                            ("order_high", ManifestValue::U64(0)),
                            (
                                "components",
                                ManifestValue::List(vec![material_fixture(&material)]),
                            ),
                        ],
                    )]),
                ),
                ("resources", ManifestValue::List(Vec::new())),
                ("render_diagnostics", ManifestValue::List(Vec::new())),
                ("controllers", ManifestValue::List(Vec::new())),
                #[cfg(feature = "gui")]
                ("gui_focus", ManifestValue::List(Vec::new())),
                #[cfg(feature = "gui")]
                ("gui_pointers", ManifestValue::List(Vec::new())),
                #[cfg(feature = "surfaces")]
                ("canvas", ManifestValue::None),
            ],
        ),
        &mut covered,
    );

    {
        use ipp_core::{AssetResourceSnapshot, AssetResourceStatus};

        let statuses = vec![
            AssetResourceStatus::Unloaded,
            AssetResourceStatus::Start,
            AssetResourceStatus::Progress {
                completed: 13,
                total: Some(23),
            },
            AssetResourceStatus::Loaded,
            AssetResourceStatus::Failed("network".into()),
        ];
        let encoded_statuses = vec![
            manifest_layout(
                "resource-status-unloaded",
                [("tag", ManifestValue::Tag("RESOURCE_UNLOADED"))],
            ),
            manifest_layout(
                "resource-status-start",
                [("tag", ManifestValue::Tag("RESOURCE_START"))],
            ),
            manifest_layout(
                "resource-status-progress",
                [
                    ("tag", ManifestValue::Tag("RESOURCE_PROGRESS")),
                    ("completed", ManifestValue::U64(13)),
                    (
                        "total",
                        ManifestValue::Some(Box::new(ManifestValue::U64(23))),
                    ),
                ],
            ),
            manifest_layout(
                "resource-status-loaded",
                [("tag", ManifestValue::Tag("RESOURCE_LOADED"))],
            ),
            manifest_layout(
                "resource-status-failed",
                [
                    ("tag", ManifestValue::Tag("RESOURCE_FAILED")),
                    ("error", ManifestValue::String("network".into())),
                ],
            ),
        ];
        let resources = statuses
            .into_iter()
            .enumerate()
            .map(|(index, status)| AssetResourceSnapshot {
                representation: Default::default(),
                id: 61 + index as u64,
                kind: ipp_core::services::asset_management::AssetTypeId(321),
                source: format!("https://example.test/{}", 61 + index).into(),
                variant: 71 + index as u32,
                status,
            })
            .collect::<Vec<_>>();
        let encoded_resources = encoded_statuses
            .into_iter()
            .enumerate()
            .map(|(index, status)| {
                manifest_layout(
                    "resource",
                    [
                        ("id", ManifestValue::U64(61 + index as u64)),
                        ("kind", ManifestValue::U16(321)),
                        (
                            "source",
                            ManifestValue::String(format!("https://example.test/{}", 61 + index)),
                        ),
                        ("variant", ManifestValue::U32(71 + index as u32)),
                        ("status", status),
                        ("representation", empty_representation()),
                    ],
                )
            })
            .collect::<Vec<_>>();
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 0,
                tick: 43,
                body: ResponseBody::Resources {
                    resources,
                },
            },
            ManifestFixture::new(
                "response-resources",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(0)),
                    ("tick", ManifestValue::U64(43)),
                    ("tag", ManifestValue::Tag("RESPONSE_RESOURCES")),
                    ("resources", ManifestValue::List(encoded_resources)),
                ],
            ),
            &mut covered,
        );
    }

    assert_manifest_response(
        Response {
            session: 7,
            request_id: 24,
            tick: 44,
            body: ResponseBody::Error {
                code: 0x1234,
                message: "unavailable".into(),
            },
        },
        ManifestFixture::new(
            "response-error",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(24)),
                ("tick", ManifestValue::U64(44)),
                ("tag", ManifestValue::Tag("RESPONSE_ERROR")),
                ("code", ManifestValue::U16(0x1234)),
                ("message", ManifestValue::String("unavailable".into())),
            ],
        ),
        &mut covered,
    );

    view_tests::responses(&mut covered);
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 37,
            tick: 19,
            body: ResponseBody::CameraNavigated,
        },
        ManifestFixture::new(
            "response-camera-navigated",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(37)),
                ("tick", ManifestValue::U64(19)),
                ("tag", ManifestValue::Tag("RESPONSE_CAMERA_NAVIGATED")),
            ],
        ),
        &mut covered,
    );

    for changes in render_state_patches()
        .into_iter()
        .filter(|patch| *patch != ipp_core::RenderStatePatch::default())
    {
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 0,
                tick: 47,
                body: ResponseBody::RenderStateUpdatedEvent(ipp_core::RenderStateChange {
                    tick: 47,
                    changes,
                }),
            },
            ManifestFixture::new(
                "response-render-state-updated",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(0)),
                    ("tick", ManifestValue::U64(47)),
                    ("tag", ManifestValue::Tag("RESPONSE_RENDER_STATE_UPDATED")),
                    ("changes", manifest_render_patch(&changes)),
                ],
            ),
            &mut covered,
        );
    }

    lifecycle_response_fixtures(&mut covered);
    lifecycle_watch_tests::responses(&mut covered);
    #[cfg(feature = "diagnostics")]
    lifecycle_diagnostics_tests::responses(&mut covered);
    attachment_tests::responses(&mut covered);

    #[cfg(feature = "gui")]
    {
        gui_observation_response_fixtures(&mut covered);
    }

    for tag in TAGS.iter().filter(|tag| {
        matches!(
            tag.space,
            TagSpace::Response
                | TagSpace::Outcome
                | TagSpace::AssetResourceStatus
                | TagSpace::SnapshotValue
                | TagSpace::SnapshotReference
                | TagSpace::GeometryPickOutcome
                | TagSpace::LifecycleObservation
                | TagSpace::OperationEffect
                | TagSpace::AttachmentReceiptState
        )
    }) {
        assert!(
            covered.contains(tag.name),
            "missing response fixture for {}",
            tag.name
        );
    }
}

fn render_state_patches() -> Vec<ipp_core::RenderStatePatch> {
    [None, Some(false), Some(true)]
        .into_iter()
        .flat_map(|show_all_debug_geometries| {
            [None, Some([0.25, 0.5, 1.0])]
                .into_iter()
                .flat_map(move |debug_geometry_color| {
                    [None, Some([2.0, 0.1, 0.0])]
                        .into_iter()
                        .map(move |ambient_light| ipp_core::RenderStatePatch {
                            show_all_debug_geometries,
                            debug_geometry_color,
                            ambient_light,
                        })
                })
        })
        .collect()
}

fn manifest_render_patch(patch: &ipp_core::RenderStatePatch) -> ManifestValue {
    manifest_layout(
        "render-state-patch",
        [
            (
                "mask",
                ManifestValue::U16(
                    u16::from(patch.show_all_debug_geometries.is_some())
                        | (u16::from(patch.debug_geometry_color.is_some()) << 1)
                        | (u16::from(patch.ambient_light.is_some()) << 2),
                ),
            ),
            (
                "showAllDebugGeometries",
                patch
                    .show_all_debug_geometries
                    .map_or(ManifestValue::None, |v| {
                        ManifestValue::Some(Box::new(ManifestValue::Bool(v)))
                    }),
            ),
            (
                "debugGeometryColor",
                patch
                    .debug_geometry_color
                    .map_or(ManifestValue::None, |[r, g, b]| {
                        ManifestValue::Some(Box::new(manifest_layout(
                            "linear-rgb",
                            [
                                ("r", ManifestValue::F32(r)),
                                ("g", ManifestValue::F32(g)),
                                ("b", ManifestValue::F32(b)),
                            ],
                        )))
                    }),
            ),
            (
                "ambientLight",
                patch
                    .ambient_light
                    .map_or(ManifestValue::None, |[r, g, b]| {
                        ManifestValue::Some(Box::new(manifest_layout(
                            "linear-rgb",
                            [
                                ("r", ManifestValue::F32(r)),
                                ("g", ManifestValue::F32(g)),
                                ("b", ManifestValue::F32(b)),
                            ],
                        )))
                    }),
            ),
        ],
    )
}

fn manifest_plane(plane: ipp_core::WorldPlane) -> ManifestValue {
    manifest_layout(
        "pick-view-plane",
        [
            ("point_x", ManifestValue::F32(plane.point[0])),
            ("point_y", ManifestValue::F32(plane.point[1])),
            ("point_z", ManifestValue::F32(plane.point[2])),
            ("normal_x", ManifestValue::F32(plane.normal[0])),
            ("normal_y", ManifestValue::F32(plane.normal[1])),
            ("normal_z", ManifestValue::F32(plane.normal[2])),
        ],
    )
}

fn lifecycle_request_fixtures(covered: &mut BTreeSet<&'static str>) {
    use ipp_core::systems::lifecycle_publisher::{LifecycleFilter, LifecyclePublisherCommand};
    #[allow(unused_mut)]
    let mut values = vec![
        ("session", ManifestValue::U64(7)),
        ("request_id", ManifestValue::U64(1)),
        ("tag", ManifestValue::Tag("REQUEST_LIFECYCLE_SUBSCRIBE")),
        ("subscription", ManifestValue::U64(2)),
        ("domains", ManifestValue::U16(3)),
        ("entity", ManifestValue::U64(42)),
        ("component", ManifestValue::U16(1)),
    ];
    values.push(("asset", ManifestValue::U64(0)));
    let bytes = encode_manifest_fixture(
        &ManifestFixture::new("request-lifecycle-subscribe", values),
        covered,
    );
    assert_eq!(
        decode_request(&bytes, 7).unwrap().body,
        RequestBody::LifecycleSubscription(LifecyclePublisherCommand::Subscribe {
            subscription: 2,
            filter: LifecycleFilter {
                entities: true,
                components: true,
                entity: Some(EntityId::from_bits(42)),
                component: Some(1),
                assets: false,
                asset: None,
            },
        })
    );
    let bytes = encode_manifest_fixture(
        &ManifestFixture::new(
            "request-lifecycle-unsubscribe",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(2)),
                ("tag", ManifestValue::Tag("REQUEST_LIFECYCLE_UNSUBSCRIBE")),
                ("subscription", ManifestValue::U64(2)),
            ],
        ),
        covered,
    );
    assert_eq!(
        decode_request(&bytes, 7).unwrap().body,
        RequestBody::LifecycleSubscription(LifecyclePublisherCommand::Unsubscribe {
            subscription: 2
        })
    );
}

fn lifecycle_response_fixtures(covered: &mut BTreeSet<&'static str>) {
    use ipp_core::systems::lifecycle_publisher::*;
    let envelope = |tag| {
        vec![
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(0)),
            ("tick", ManifestValue::U64(3)),
            ("tag", ManifestValue::Tag(tag)),
        ]
    };
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 1,
            tick: 3,
            body: ResponseBody::LifecycleSubscription,
        },
        ManifestFixture::new(
            "response-lifecycle-subscription",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tick", ManifestValue::U64(3)),
                ("tag", ManifestValue::Tag("RESPONSE_LIFECYCLE_SUBSCRIPTION")),
            ],
        ),
        covered,
    );
    let entity = EntityId::from_bits(42);
    let mut observations = Vec::new();
    let mut encoded = Vec::new();
    for (kind, tag) in [
        (EntityLifecycleKind::Created, "LIFECYCLE_ENTITY_CREATED"),
        (
            EntityLifecycleKind::MetadataChanged,
            "LIFECYCLE_ENTITY_METADATA_CHANGED",
        ),
        (EntityLifecycleKind::Deleted, "LIFECYCLE_ENTITY_DELETED"),
    ] {
        observations.push(LifecycleObservation::Entity {
            entity,
            kind,
        });
        encoded.push(manifest_layout(
            "lifecycle-entity",
            [
                ("tag", ManifestValue::Tag(tag)),
                ("entity", ManifestValue::U64(42)),
            ],
        ));
    }
    for (kind, tag, previous_incarnation, incarnation) in [
        (
            ComponentLifecycleKind::Inserted,
            "LIFECYCLE_COMPONENT_INSERTED",
            None,
            Some(1),
        ),
        (
            ComponentLifecycleKind::Updated,
            "LIFECYCLE_COMPONENT_UPDATED",
            Some(1),
            Some(1),
        ),
        (
            ComponentLifecycleKind::Replaced,
            "LIFECYCLE_COMPONENT_REPLACED",
            Some(1),
            Some(2),
        ),
        (
            ComponentLifecycleKind::Removed,
            "LIFECYCLE_COMPONENT_REMOVED",
            Some(2),
            None,
        ),
    ] {
        observations.push(LifecycleObservation::Component {
            entity,
            component: 1,
            kind,
            previous_incarnation,
            incarnation,
        });
        encoded.push(manifest_layout(
            "lifecycle-component",
            [
                ("tag", ManifestValue::Tag(tag)),
                ("entity", ManifestValue::U64(42)),
                ("component", ManifestValue::U16(1)),
                (
                    "previous_incarnation",
                    ManifestValue::U64(previous_incarnation.unwrap_or(0)),
                ),
                ("incarnation", ManifestValue::U64(incarnation.unwrap_or(0))),
            ],
        ));
    }
    for (kind, tag) in [
        (
            ipp_core::services::asset_management::AssetLifecycleKind::GraphicsInvalidated,
            "LIFECYCLE_ASSET_GRAPHICS_INVALIDATED",
        ),
        (
            ipp_core::services::asset_management::AssetLifecycleKind::StatusChanged,
            "LIFECYCLE_ASSET_STATUS_CHANGED",
        ),
        (
            ipp_core::services::asset_management::AssetLifecycleKind::Removed,
            "LIFECYCLE_ASSET_REMOVED",
        ),
    ] {
        observations.push(LifecycleObservation::Asset {
            kind,
            resource: ipp_core::AssetResourceSnapshot {
                representation: Default::default(),
                id: 9,
                kind: ipp_core::services::asset_management::AssetTypeId(1),
                source: "memory://asset".into(),
                variant: 0,
                status: ipp_core::AssetResourceStatus::Unloaded,
            },
        });
        encoded.push(manifest_layout(
            "lifecycle-asset",
            [
                ("tag", ManifestValue::Tag(tag)),
                (
                    "resource",
                    manifest_layout(
                        "resource",
                        [
                            ("representation", empty_representation()),
                            ("id", ManifestValue::U64(9)),
                            ("kind", ManifestValue::U16(1)),
                            ("source", ManifestValue::String("memory://asset".into())),
                            ("variant", ManifestValue::U32(0)),
                            (
                                "status",
                                manifest_layout(
                                    "resource-status-unloaded",
                                    [("tag", ManifestValue::Tag("RESOURCE_UNLOADED"))],
                                ),
                            ),
                        ],
                    ),
                ),
            ],
        ));
    }
    let events = observations
        .into_iter()
        .enumerate()
        .map(|(index, observation)| LifecyclePublication {
            subscription: 2,
            sequence: index as u64 + 1,
            tick: 2,
            observation,
        })
        .collect();
    let events_encoded = encoded
        .into_iter()
        .enumerate()
        .map(|(index, observation)| {
            manifest_layout(
                "lifecycle-publication",
                [
                    ("subscription", ManifestValue::U64(2)),
                    ("sequence", ManifestValue::U64(index as u64 + 1)),
                    ("tick", ManifestValue::U64(2)),
                    ("observation", observation),
                ],
            )
        })
        .collect();
    let mut fields = envelope("RESPONSE_LIFECYCLE_EVENTS");
    fields.push(("events", ManifestValue::List(events_encoded)));
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 0,
            tick: 3,
            body: ResponseBody::LifecycleEvents(LifecyclePublisherOutput(events)),
        },
        ManifestFixture::new("response-lifecycle-events", fields),
        covered,
    );
}

/// Registration results and applied effects of the GUI observation stream.
#[cfg(feature = "gui")]
fn gui_observation_response_fixtures(covered: &mut BTreeSet<&'static str>) {
    use ipp_core::systems::gui::local::{
        GuiEntityTarget, GuiLocalEffect, GuiLocalEffectKind, GuiLocalEffectSource,
    };
    use ipp_core::systems::gui::observations::{
        GuiEffectId, GuiObservationControlResult, GuiObservationRecord,
        GuiObservationSubscriptionId,
    };

    let mut host = ipp_core::HostRuntime::new();
    let world = host.create_world(Default::default(), &[]).unwrap();
    let world = host.world_ref(world).unwrap();
    let world_bytes = || {
        let mut bytes = world.id().0.to_le_bytes().to_vec();
        bytes.extend(world.incarnation().to_le_bytes());
        bytes
    };
    let subscription = GuiObservationSubscriptionId {
        output: 5,
        generation: 6,
    };
    let mut control = 5u64.to_le_bytes().to_vec();
    control.extend(6u64.to_le_bytes());
    control.push(0);
    control.extend(world_bytes());
    control.push(0);
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 3,
            tick: 0,
            body: ResponseBody::GuiObservation(GuiObservationRecord::Control {
                world,
                subscription,
                request: 3,
                result: GuiObservationControlResult::Subscribed,
            }),
        },
        ManifestFixture::new(
            "response-gui-observation",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(3)),
                ("tick", ManifestValue::U64(0)),
                ("tag", ManifestValue::Tag("RESPONSE_GUI_OBSERVATION")),
                ("record", ManifestValue::Bytes(control)),
            ],
        ),
        covered,
    );

    let entity = EntityId::from_bits(0x0000_0001_0000_0029);
    let effect = GuiLocalEffect {
        id: Some(GuiEffectId {
            world,
            ordinal: 8,
        }),
        target: GuiEntityTarget {
            world,
            entity,
            component: ComponentValue::GUI_TEXT_INPUT,
            incarnation: 9,
        },
        source: GuiLocalEffectSource::Semantic,
        tick: 10,
        ancestry: vec![EntityId::from_bits(11), entity].into(),
        kind: GuiLocalEffectKind::Submitted("sent".into()),
    };
    let mut record = 5u64.to_le_bytes().to_vec();
    record.extend(6u64.to_le_bytes());
    record.push(1);
    record.push(1);
    record.extend(world_bytes());
    record.extend(8u64.to_le_bytes());
    record.extend(world_bytes());
    record.extend(entity.to_bits().to_le_bytes());
    record.extend(ComponentValue::GUI_TEXT_INPUT.to_le_bytes());
    record.extend(9u64.to_le_bytes());
    record.push(0);
    record.extend(10u64.to_le_bytes());
    record.extend(2u32.to_le_bytes());
    record.extend(11u64.to_le_bytes());
    record.extend(entity.to_bits().to_le_bytes());
    record.push(4);
    record.extend(4u32.to_le_bytes());
    record.extend(b"sent");
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 0,
            tick: 0,
            body: ResponseBody::GuiObservation(GuiObservationRecord::Effect {
                subscription,
                effect: std::sync::Arc::new(effect),
            }),
        },
        ManifestFixture::new(
            "response-gui-observation",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tick", ManifestValue::U64(0)),
                ("tag", ManifestValue::Tag("RESPONSE_GUI_OBSERVATION")),
                ("record", ManifestValue::Bytes(record)),
            ],
        ),
        covered,
    );
}
