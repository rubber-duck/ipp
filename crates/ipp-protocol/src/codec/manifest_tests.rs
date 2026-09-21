use super::*;
use std::collections::{BTreeMap, BTreeSet};

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
        (FieldEncoding::List, ManifestValue::List(values)) => {
            assert!(values.len() <= limit as usize);
            bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
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

fn manifest_reference_handle(handle: u64) -> ManifestValue {
    manifest_layout(
        "reference-handle",
        [
            ("tag", ManifestValue::Tag("REF_HANDLE")),
            ("handle", ManifestValue::U64(handle)),
        ],
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
    ];
    let encoded_writes = vec![
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
    ];
    #[allow(unused_mut)]
    let mut operations = vec![
        Command::Create {
            alias: 1,
            metadata: metadata.clone(),
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
    ];

    {
        encoded_operations.extend([
            manifest_command(
                "command-create-state-overlay-owner",
                "COMMAND_CREATE_STATE_OVERLAY_OWNER",
                [("alias", ManifestValue::U32(2))],
            ),
            manifest_command(
                "command-release-state-overlay-owner",
                "COMMAND_RELEASE_STATE_OVERLAY_OWNER",
                [("owner", manifest_reference_handle(51))],
            ),
            manifest_command(
                "command-attach-entity-overlay-binding",
                "COMMAND_ATTACH_ENTITY_OVERLAY_BINDING",
                [
                    ("owner", manifest_reference_alias(32)),
                    ("alias", ManifestValue::U32(3)),
                    ("symbolic_id", ManifestValue::String("owned".into())),
                    ("mode", ManifestValue::Tag("ENTITY_OVERLAY_MODE_OWNED")),
                ],
            ),
            manifest_command(
                "command-attach-entity-overlay-binding",
                "COMMAND_ATTACH_ENTITY_OVERLAY_BINDING",
                [
                    ("owner", manifest_reference_handle(52)),
                    ("alias", ManifestValue::U32(4)),
                    ("symbolic_id", ManifestValue::String("bound".into())),
                    ("mode", ManifestValue::Tag("ENTITY_OVERLAY_MODE_BOUND")),
                ],
            ),
            manifest_command(
                "command-release-entity-overlay-binding",
                "COMMAND_RELEASE_ENTITY_OVERLAY_BINDING",
                [
                    ("owner", manifest_reference_alias(33)),
                    ("binding", manifest_reference_handle(53)),
                ],
            ),
        ]);
        for (mode_name, alias, mode) in [
            (
                "COMPONENT_OVERLAY_MODE_AUTO",
                10,
                ipp_core::ComponentOverlayMode::Auto,
            ),
            (
                "COMPONENT_OVERLAY_MODE_BOUND",
                11,
                ipp_core::ComponentOverlayMode::Bound,
            ),
            (
                "COMPONENT_OVERLAY_MODE_OWNED",
                12,
                ipp_core::ComponentOverlayMode::Owned,
            ),
        ] {
            encoded_operations.push(manifest_command(
                "command-attach-component-state-overlay",
                "COMMAND_ATTACH_COMPONENT_STATE_OVERLAY",
                [
                    ("owner", manifest_reference_alias(34)),
                    ("binding", manifest_reference_handle(54)),
                    ("alias", ManifestValue::U32(alias)),
                    ("component", ManifestValue::U16(330 + alias as u16)),
                    ("mode", ManifestValue::Tag(mode_name)),
                    ("fields", ManifestValue::List(Vec::new())),
                ],
            ));
            operations.push(Command::AttachComponentStateOverlay {
                owner: ipp_core::StateOverlayRef::Alias(34),
                binding: ipp_core::StateOverlayRef::Handle(54),
                alias,
                component: 330 + alias as u16,
                mode,
                fields: Vec::new(),
            });
        }
        encoded_operations.extend([
            manifest_command(
                "command-update-component-state-overlay",
                "COMMAND_UPDATE_COMPONENT_STATE_OVERLAY",
                [
                    ("owner", manifest_reference_handle(55)),
                    ("overlay", manifest_reference_alias(35)),
                    ("fields", ManifestValue::List(Vec::new())),
                    (
                        "clear",
                        ManifestValue::List(vec![ManifestValue::U32(71), ManifestValue::U32(72)]),
                    ),
                ],
            ),
            manifest_command(
                "command-release-component-state-overlay",
                "COMMAND_RELEASE_COMPONENT_STATE_OVERLAY",
                [
                    ("owner", manifest_reference_alias(36)),
                    ("overlay", manifest_reference_handle(56)),
                ],
            ),
        ]);
        operations.splice(
            6..6,
            [
                Command::CreateStateOverlayOwner {
                    alias: 2,
                },
                Command::ReleaseStateOverlayOwner {
                    owner: ipp_core::StateOverlayRef::Handle(51),
                },
                Command::AttachEntityOverlayBinding {
                    owner: ipp_core::StateOverlayRef::Alias(32),
                    alias: 3,
                    symbolic_id: "owned".into(),
                    mode: ipp_core::EntityOverlayMode::Owned,
                },
                Command::AttachEntityOverlayBinding {
                    owner: ipp_core::StateOverlayRef::Handle(52),
                    alias: 4,
                    symbolic_id: "bound".into(),
                    mode: ipp_core::EntityOverlayMode::Bound,
                },
                Command::ReleaseEntityOverlayBinding {
                    owner: ipp_core::StateOverlayRef::Alias(33),
                    binding: ipp_core::StateOverlayRef::Handle(53),
                },
            ],
        );
        operations.extend([
            Command::UpdateComponentStateOverlay {
                owner: ipp_core::StateOverlayRef::Handle(55),
                overlay: ipp_core::StateOverlayRef::Alias(35),
                fields: Vec::new(),
                clear: vec![71, 72],
            },
            Command::ReleaseComponentStateOverlay {
                owner: ipp_core::StateOverlayRef::Alias(36),
                overlay: ipp_core::StateOverlayRef::Handle(56),
            },
        ]);
    }

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
    {
        encoded_operations.push(manifest_command(
            "command-update-dynamic-component-state-overlay",
            "COMMAND_UPDATE_DYNAMIC_COMPONENT_STATE_OVERLAY",
            [
                ("owner", manifest_reference_handle(51)),
                ("overlay", manifest_reference_handle(55)),
                (
                    "properties",
                    ManifestValue::List(vec![manifest_layout(
                        "dynamic-property-write",
                        [
                            ("name", ManifestValue::String("amount".into())),
                            (
                                "value",
                                ManifestValue::Bytes(ipp_core::DynamicValue::F32(0.5).encode()),
                            ),
                        ],
                    )]),
                ),
                (
                    "clear",
                    ManifestValue::List(vec![manifest_layout(
                        "dynamic-property-name",
                        [("name", ManifestValue::String("old".into()))],
                    )]),
                ),
            ],
        ));
        operations.push(Command::UpdateDynamicComponentStateOverlay {
            owner: ipp_core::StateOverlayRef::Handle(51),
            overlay: ipp_core::StateOverlayRef::Handle(55),
            properties: vec![("amount".into(), ipp_core::DynamicValue::F32(0.5))],
            clear: vec!["old".into()],
        });
    }

    let fixture = ManifestFixture::new(
        "request-batch",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(17)),
            ("tag", ManifestValue::Tag("REQUEST_BATCH")),
            ("batch_id", ManifestValue::U64(27)),
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
            body: RequestBody::Batch(Batch {
                id: 27,
                operations
            }),
        }
    );

    for (layout, tag, body, extra) in [
        (
            "request-begin-batch",
            "REQUEST_BEGIN_BATCH",
            RequestBody::BeginBatch,
            vec![],
        ),
        (
            "request-end-batch",
            "REQUEST_END_BATCH",
            RequestBody::EndBatch(27),
            vec![("batch_id", ManifestValue::U64(27))],
        ),
        (
            "request-batch",
            "REQUEST_BATCH_CHUNK",
            RequestBody::BatchChunk(Batch {
                id: 27,
                operations: vec![],
            }),
            vec![
                ("batch_id", ManifestValue::U64(27)),
                ("operations", ManifestValue::List(vec![])),
            ],
        ),
    ] {
        let values = [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(17)),
            ("tag", ManifestValue::Tag(tag)),
        ]
        .into_iter()
        .chain(extra);
        let bytes = encode_manifest_fixture(&ManifestFixture::new(layout, values), &mut covered);
        assert_eq!(decode_request(&bytes, 7).unwrap().body, body);
    }

    for (collection, tag) in [
        (0, "INSPECT_SUMMARY"),
        (1, "INSPECT_ENTITIES"),
        (2, "INSPECT_RESOURCES"),
        (3, "INSPECT_CONTROLLERS"),
        (4, "INSPECT_RENDER_DIAGNOSTICS"),
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
    {
        let request = ManifestFixture::new(
            "request-camera-activate",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tag", ManifestValue::Tag("REQUEST_CAMERA_ACTIVATE")),
                ("entity", ManifestValue::U64(handle_bits)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::CameraActivateCommand {
                entity: handle
            },
        );
    }
    for (motion, name, fields) in [
        (
            ipp_core::CameraMotion::Rotate {
                yaw: 0.25,
                pitch: -0.5,
            },
            "camera-motion-rotate",
            vec![
                ("tag", ManifestValue::Tag("CAMERA_MOTION_ROTATE")),
                ("yaw", ManifestValue::F32(0.25)),
                ("pitch", ManifestValue::F32(-0.5)),
            ],
        ),
        (
            ipp_core::CameraMotion::Pan {
                x: 0.25,
                y: -0.5,
                width: 640,
                height: 480,
            },
            "camera-motion-pan",
            vec![
                ("tag", ManifestValue::Tag("CAMERA_MOTION_PAN")),
                ("x", ManifestValue::F32(0.25)),
                ("y", ManifestValue::F32(-0.5)),
                ("width", ManifestValue::U32(640)),
                ("height", ManifestValue::U32(480)),
            ],
        ),
        (
            ipp_core::CameraMotion::Zoom {
                amount: 0.25,
            },
            "camera-motion-zoom",
            vec![
                ("tag", ManifestValue::Tag("CAMERA_MOTION_ZOOM")),
                ("amount", ManifestValue::F32(0.25)),
            ],
        ),
    ] {
        let request = ManifestFixture::new(
            "request-camera-navigate",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tag", ManifestValue::Tag("REQUEST_CAMERA_NAVIGATE")),
                ("motion", manifest_layout(name, fields)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::CameraNavigateCommand(motion)
        );
    }
    {
        let request = ManifestFixture::new(
            "request-geometry-pick",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(21)),
                ("tag", ManifestValue::Tag("REQUEST_GEOMETRY_PICK")),
                ("x", ManifestValue::F32(0.25)),
                ("y", ManifestValue::F32(0.75)),
                ("width", ManifestValue::U32(640)),
                ("height", ManifestValue::U32(480)),
                ("include_view_plane", ManifestValue::Bool(false)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::GeometryPickQuery(ipp_core::GeometryPickQuery {
                x: 0.25,
                y: 0.75,
                width: 640,
                height: 480,
                include_view_plane: false,
            }),
        );
    }

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

    {
        let plane = ipp_core::WorldPlane {
            point: [1.0, 2.0, 3.0],
            normal: [0.0, 0.0, -1.0],
        };
        let request = ManifestFixture::new(
            "request-camera-project",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(22)),
                ("tag", ManifestValue::Tag("REQUEST_CAMERA_PROJECT")),
                ("x", ManifestValue::F32(1.25)),
                ("y", ManifestValue::F32(-0.5)),
                ("width", ManifestValue::U32(640)),
                ("height", ManifestValue::U32(480)),
                ("plane", manifest_plane(plane)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::CameraProjectQuery(ipp_core::CameraProjectQuery {
                x: 1.25,
                y: -0.5,
                width: 640,
                height: 480,
                plane
            })
        );
    }

    lifecycle_request_fixtures(&mut covered);

    #[cfg(feature = "surfaces")]
    {
        let mut edit = vec![1, 3];
        edit.extend(42u64.to_le_bytes());
        edit.extend(9u32.to_le_bytes());
        let request = ManifestFixture::new(
            "request-surface",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tag", ManifestValue::Tag("REQUEST_SURFACE")),
                ("edit", ManifestValue::Bytes(edit)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::SurfaceCommand(ipp_core::systems::surface::SurfaceCommand::Remove {
                entity: EntityId::from_bits(42),
                id: ipp_core::systems::surface::SurfaceItemId(9),
            }),
        );
    }

    #[cfg(feature = "gui")]
    {
        let mut edit = vec![2];
        edit.extend(1u32.to_le_bytes());
        edit.push(4);
        edit.extend(7u64.to_le_bytes());
        edit.extend(42u64.to_le_bytes());
        edit.extend(3u64.to_le_bytes());
        edit.extend(9u32.to_le_bytes());
        edit.extend(1u32.to_le_bytes());
        let request = ManifestFixture::new(
            "request-gui",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tag", ManifestValue::Tag("REQUEST_GUI")),
                (
                    "batch_id",
                    ManifestValue::Some(Box::new(ManifestValue::U64(99))),
                ),
                ("edits", ManifestValue::Bytes(edit)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::GuiCommands {
                batch_id: Some(99),
                commands: vec![ipp_core::systems::gui::GuiCommand::RemoveNode {
                    handle: ipp_core::systems::gui::GuiNodeHandle::new(
                        7,
                        EntityId::from_bits(42),
                        3,
                        ipp_core::systems::gui::GuiNodeId(9),
                        1,
                    ),
                }],
            },
        );

        let mut query = vec![1];
        query.extend(42u64.to_le_bytes());
        query.push(1);
        query.extend(9u32.to_le_bytes());
        query.extend(10u32.to_le_bytes());
        query.extend(50u32.to_le_bytes());
        let request = ManifestFixture::new(
            "request-gui-inspect",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tag", ManifestValue::Tag("REQUEST_GUI_INSPECT")),
                ("query", ManifestValue::Bytes(query)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::GuiInspect(ipp_core::systems::gui::GuiInspectQuery {
                entity: EntityId::from_bits(42),
                node_id: Some(ipp_core::systems::gui::GuiNodeId(9)),
                max_depth: 10,
                limit: 50,
            }),
        );

        let mut input = vec![1, 1];
        input.extend(5u32.to_le_bytes());
        input.push(1);
        input.extend(42u64.to_le_bytes());
        input.extend(1.5f32.to_le_bytes());
        input.extend(2.5f32.to_le_bytes());
        input.push(0);
        input.extend(1u32.to_le_bytes());
        input.extend(43u64.to_le_bytes());
        input.extend(0.5f32.to_le_bytes());
        input.push(0);
        let request = ManifestFixture::new(
            "request-gui-input",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tag", ManifestValue::Tag("REQUEST_GUI_INPUT")),
                ("input", ManifestValue::Bytes(input)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::GuiInput(Box::new(ipp_core::GuiInputCommand::PointerDown {
                pointer: 5,
                panel: Some(EntityId::from_bits(42)),
                position: [1.5, 2.5],
                button: ipp_core::GuiPointerButton::Primary,
                blockers: vec![ipp_core::GuiBlockerHit {
                    entity: EntityId::from_bits(43),
                    distance: 0.5,
                }],
                panel_distance: None,
            })),
        );

        let mut snapshot_query = vec![1];
        snapshot_query.extend(42u64.to_le_bytes());
        snapshot_query.extend(10u32.to_le_bytes());
        snapshot_query.extend(50u32.to_le_bytes());
        let request = ManifestFixture::new(
            "request-gui-semantic-snapshot",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tag", ManifestValue::Tag("REQUEST_GUI_SEMANTIC_SNAPSHOT")),
                ("query", ManifestValue::Bytes(snapshot_query)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::GuiSemanticSnapshot(ipp_core::GuiSemanticSnapshotQuery {
                entity: EntityId::from_bits(42),
                max_depth: 10,
                limit: 50,
            }),
        );

        let mut semantic_action = vec![2];
        semantic_action.extend(42u64.to_le_bytes());
        semantic_action.extend(3u64.to_le_bytes());
        semantic_action.extend(9u32.to_le_bytes());
        semantic_action.extend(1u32.to_le_bytes());
        semantic_action.extend(2u32.to_le_bytes());
        semantic_action.push(1);
        let request = ManifestFixture::new(
            "request-gui-semantic-action",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tag", ManifestValue::Tag("REQUEST_GUI_SEMANTIC_ACTION")),
                ("action", ManifestValue::Bytes(semantic_action)),
            ],
        );
        assert_eq!(
            decode_request(&encode_manifest_fixture(&request, &mut covered), 7)
                .unwrap()
                .body,
            RequestBody::GuiSemanticAction(Box::new(ipp_core::GuiSemanticActionRequest {
                entity: EntityId::from_bits(42),
                root_incarnation: 3,
                node: ipp_core::GuiNodeId(9),
                lifetime: 1,
                expected_revision: 2,
                action: ipp_core::GuiSemanticAction::Toggle,
            })),
        );
    }

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
                    | TagSpace::EntityOverlayMode
                    | TagSpace::ComponentOverlayMode
                    | TagSpace::CameraMotion
                    | TagSpace::InspectionCollection
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
    for (layout, tag, body, request_id, extra) in [
        (
            "response-batch-identity",
            "RESPONSE_BATCH_STARTED",
            ResponseBody::BatchStarted(27),
            17,
            vec![],
        ),
        (
            "response-batch-identity",
            "RESPONSE_BATCH_FINISHED",
            ResponseBody::BatchFinished(27),
            18,
            vec![],
        ),
        (
            "response-batch-aborted",
            "RESPONSE_BATCH_ABORTED",
            ResponseBody::BatchAborted {
                batch_id: 27,
                message: "deadline".into(),
            },
            0,
            vec![("message", ManifestValue::String("deadline".into()))],
        ),
    ] {
        let values = [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(request_id)),
            ("tick", ManifestValue::U64(1)),
            ("tag", ManifestValue::Tag(tag)),
            ("batch_id", ManifestValue::U64(27)),
        ]
        .into_iter()
        .chain(extra);
        assert_manifest_response(
            Response {
                session: 7,
                request_id,
                tick: 1,
                body,
            },
            ManifestFixture::new(layout, values),
            &mut covered,
        );
    }

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
    ];
    for (offset, value, expected) in resolved_values {
        let mut writer = Writer(Vec::new());
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
        "stateOverlays",
        ManifestValue::List(vec![
            manifest_layout(
                "state-overlay-alias",
                [
                    ("alias", ManifestValue::U32(2)),
                    ("id", ManifestValue::U64(51)),
                    ("kind", ManifestValue::Tag("STATE_OVERLAY_KIND_OWNER")),
                    ("entity", ManifestValue::None),
                ],
            ),
            manifest_layout(
                "state-overlay-alias",
                [
                    ("alias", ManifestValue::U32(3)),
                    ("id", ManifestValue::U64(52)),
                    (
                        "kind",
                        ManifestValue::Tag("STATE_OVERLAY_KIND_ENTITY_BINDING"),
                    ),
                    (
                        "entity",
                        ManifestValue::Some(Box::new(ManifestValue::U64(entity.to_bits()))),
                    ),
                ],
            ),
            manifest_layout(
                "state-overlay-alias",
                [
                    ("alias", ManifestValue::U32(4)),
                    ("id", ManifestValue::U64(53)),
                    ("kind", ManifestValue::Tag("STATE_OVERLAY_KIND_COMPONENT")),
                    (
                        "entity",
                        ManifestValue::Some(Box::new(ManifestValue::U64(entity.to_bits()))),
                    ),
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
        ],
    );
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 17,
            tick: 37,
            body: ResponseBody::Batch(BatchOutcome {
                batch_id: 27,
                tick: 37,
                result: Ok(vec![(1, entity)]),
                state_overlays: vec![
                    ipp_core::StateOverlayAlias {
                        alias: 2,
                        id: 51,
                        kind: ipp_core::StateOverlayHandleKind::Owner,
                        entity: None,
                    },
                    ipp_core::StateOverlayAlias {
                        alias: 3,
                        id: 52,
                        kind: ipp_core::StateOverlayHandleKind::EntityOverlayBinding,
                        entity: Some(entity),
                    },
                    ipp_core::StateOverlayAlias {
                        alias: 4,
                        id: 53,
                        kind: ipp_core::StateOverlayHandleKind::ComponentStateOverlay,
                        entity: Some(entity),
                    },
                ],
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
            ("stateOverlays", ManifestValue::List(Vec::new())),
        ],
    );
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 18,
            tick: 38,
            body: ResponseBody::Batch(BatchOutcome {
                batch_id: 28,
                tick: 38,
                result: Err(ipp_core::BatchError {
                    scope: ipp_core::BatchErrorScope::Operation,
                    operation: Some(0x1234_5678),
                    reason: ipp_core::ErrorReason::InvalidField,
                    aliases: vec![(9, EntityId::from_bits(17))],
                }),
                state_overlays: Vec::new(),
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
            ("base", ManifestValue::List(vec![component])),
            ("effective", ManifestValue::List(Vec::new())),
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
                    base: vec![scalar],
                    effective: Vec::new(),
                }],
                resources: Vec::new(),
                render_diagnostics: Vec::new(),
            },
        },
        ManifestFixture::new("response-inspect", inspect_values),
        &mut covered,
    );

    {
        let diagnostics = vec![
            ipp_core::StateOverlayLifecycleDiagnostic {
                owner: 71,
                state_overlay: 81,
                entity,
                component: None,
                reason: ipp_core::StateOverlayLifecycleReason::EntityDeleted,
            },
            ipp_core::StateOverlayLifecycleDiagnostic {
                owner: 72,
                state_overlay: 82,
                entity,
                component: Some(321),
                reason: ipp_core::StateOverlayLifecycleReason::ComponentReplaced,
            },
            ipp_core::StateOverlayLifecycleDiagnostic {
                owner: 73,
                state_overlay: 83,
                entity,
                component: Some(322),
                reason: ipp_core::StateOverlayLifecycleReason::ComponentRemoved,
            },
        ];
        let encoded = vec![
            manifest_layout(
                "state-overlay-lifecycle-diagnostic",
                [
                    ("owner", ManifestValue::U64(71)),
                    ("stateOverlay", ManifestValue::U64(81)),
                    ("entity", ManifestValue::U64(entity.to_bits())),
                    ("component", ManifestValue::None),
                    ("reason", ManifestValue::Tag("STATE_OVERLAY_ENTITY_DELETED")),
                ],
            ),
            manifest_layout(
                "state-overlay-lifecycle-diagnostic",
                [
                    ("owner", ManifestValue::U64(72)),
                    ("stateOverlay", ManifestValue::U64(82)),
                    ("entity", ManifestValue::U64(entity.to_bits())),
                    (
                        "component",
                        ManifestValue::Some(Box::new(ManifestValue::U16(321))),
                    ),
                    (
                        "reason",
                        ManifestValue::Tag("STATE_OVERLAY_COMPONENT_REPLACED"),
                    ),
                ],
            ),
            manifest_layout(
                "state-overlay-lifecycle-diagnostic",
                [
                    ("owner", ManifestValue::U64(73)),
                    ("stateOverlay", ManifestValue::U64(83)),
                    ("entity", ManifestValue::U64(entity.to_bits())),
                    (
                        "component",
                        ManifestValue::Some(Box::new(ManifestValue::U16(322))),
                    ),
                    (
                        "reason",
                        ManifestValue::Tag("STATE_OVERLAY_COMPONENT_REMOVED"),
                    ),
                ],
            ),
        ];
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 0,
                tick: 41,
                body: ResponseBody::Lifecycle {
                    diagnostics,
                },
            },
            ManifestFixture::new(
                "response-state-overlay-lifecycle",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(0)),
                    ("tick", ManifestValue::U64(41)),
                    (
                        "tag",
                        ManifestValue::Tag("RESPONSE_STATE_OVERLAY_LIFECYCLE"),
                    ),
                    ("diagnostics", ManifestValue::List(encoded)),
                ],
            ),
            &mut covered,
        );
    }

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
                source: format!("https://example.test/{}", 61 + index),
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

    assert_manifest_response(
        Response {
            session: 7,
            request_id: 0,
            tick: 45,
            body: ResponseBody::CameraStateChangedEvent(ipp_core::CameraStateChange {
                tick: 45,
                changes: ipp_core::CameraStatePatch {
                    active_camera: Some(entity),
                },
            }),
        },
        ManifestFixture::new(
            "response-camera-state-changed",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(0)),
                ("tick", ManifestValue::U64(45)),
                ("tag", ManifestValue::Tag("RESPONSE_CAMERA_STATE_CHANGED")),
                (
                    "changes",
                    manifest_layout(
                        "camera-state-patch",
                        [
                            ("mask", ManifestValue::U16(1)),
                            (
                                "activeCamera",
                                ManifestValue::Some(Box::new(manifest_layout(
                                    "camera-entity",
                                    [("id", ManifestValue::U64(entity.to_bits()))],
                                ))),
                            ),
                        ],
                    ),
                ),
            ],
        ),
        &mut covered,
    );
    for (camera, result) in [
        (Some(entity), Ok(None)),
        (
            Some(entity),
            Ok(Some(ipp_core::GeometryPickHit {
                entity,
                position: [1.0, 2.0, 3.0],
                distance: 4.0,
                part: 0,
                view_plane: None,
            })),
        ),
        (
            Some(entity),
            Ok(Some(ipp_core::GeometryPickHit {
                entity,
                position: [1.0, 2.0, 3.0],
                distance: 4.0,
                part: 8,
                view_plane: Some(ipp_core::WorldPlane {
                    point: [1.0, 2.0, 3.0],
                    normal: [0.0, 0.0, -1.0],
                }),
            })),
        ),
        (None, Err(ipp_core::ErrorReason::NoActiveCamera)),
    ] {
        let encoded = match &result {
            Ok(None) => manifest_layout(
                "pick-result-miss",
                vec![("tag", ManifestValue::Tag("PICK_OUTCOME_MISS"))],
            ),
            Ok(Some(hit)) => {
                let mut fields = vec![
                    ("tag", ManifestValue::Tag("PICK_OUTCOME_HIT")),
                    ("entity", ManifestValue::U64(entity.to_bits())),
                    ("position_x", ManifestValue::F32(1.0)),
                    ("position_y", ManifestValue::F32(2.0)),
                    ("position_z", ManifestValue::F32(3.0)),
                    ("distance", ManifestValue::F32(4.0)),
                ];
                fields.push(("part", ManifestValue::U32(hit.part)));
                fields.push((
                    "view_plane",
                    match hit.view_plane {
                        None => ManifestValue::None,
                        Some(plane) => ManifestValue::Some(Box::new(manifest_layout(
                            "pick-view-plane",
                            [
                                ("point_x", ManifestValue::F32(plane.point[0])),
                                ("point_y", ManifestValue::F32(plane.point[1])),
                                ("point_z", ManifestValue::F32(plane.point[2])),
                                ("normal_x", ManifestValue::F32(plane.normal[0])),
                                ("normal_y", ManifestValue::F32(plane.normal[1])),
                                ("normal_z", ManifestValue::F32(plane.normal[2])),
                            ],
                        ))),
                    },
                ));
                manifest_layout("pick-result-hit", fields)
            }
            Err(_) => manifest_layout(
                "pick-result-failure",
                vec![
                    ("tag", ManifestValue::Tag("PICK_OUTCOME_FAILURE")),
                    ("reason", ManifestValue::String("NoActiveCamera".into())),
                ],
            ),
        };
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 26,
                tick: 46,
                body: ResponseBody::GeometryPickResultEvent(ipp_core::GeometryPickOutcome {
                    request_id: 26,
                    tick: 46,
                    camera,
                    result,
                }),
            },
            ManifestFixture::new(
                "response-geometry-pick",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(26)),
                    ("tick", ManifestValue::U64(46)),
                    ("tag", ManifestValue::Tag("RESPONSE_GEOMETRY_PICK")),
                    (
                        "camera",
                        camera.map_or(ManifestValue::None, |camera| {
                            ManifestValue::Some(Box::new(ManifestValue::U64(camera.to_bits())))
                        }),
                    ),
                    ("result", encoded),
                ],
            ),
            &mut covered,
        );
    }

    for result in [
        Ok(Some([1.0, 2.0, 3.0])),
        Ok(None),
        Err(ipp_core::ErrorReason::InvalidViewport),
    ] {
        let position = match result {
            Ok(Some([x, y, z])) => ManifestValue::Some(Box::new(manifest_layout(
                "world-point",
                [
                    ("x", ManifestValue::F32(x)),
                    ("y", ManifestValue::F32(y)),
                    ("z", ManifestValue::F32(z)),
                ],
            ))),
            _ => ManifestValue::None,
        };
        let error = if result.is_err() {
            ManifestValue::Some(Box::new(ManifestValue::String("InvalidViewport".into())))
        } else {
            ManifestValue::None
        };
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 23,
                tick: 48,
                body: ResponseBody::CameraProjectResultEvent(ipp_core::CameraProjectOutcome {
                    request_id: 23,
                    tick: 48,
                    camera: Some(entity),
                    result,
                }),
            },
            ManifestFixture::new(
                "response-camera-project",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(23)),
                    ("tick", ManifestValue::U64(48)),
                    ("tag", ManifestValue::Tag("RESPONSE_CAMERA_PROJECT")),
                    (
                        "camera",
                        ManifestValue::Some(Box::new(ManifestValue::U64(entity.to_bits()))),
                    ),
                    ("ok", ManifestValue::Bool(result.is_ok())),
                    ("position", position),
                    ("error", error),
                ],
            ),
            &mut covered,
        );
    }

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

    #[cfg(feature = "surfaces")]
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 1,
            tick: 47,
            body: ResponseBody::SurfaceCommand,
        },
        ManifestFixture::new(
            "response-surface",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tick", ManifestValue::U64(47)),
                ("tag", ManifestValue::Tag("RESPONSE_SURFACE")),
            ],
        ),
        &mut covered,
    );

    #[cfg(feature = "gui")]
    {
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 1,
                tick: 47,
                body: ResponseBody::GuiCommands {
                    applied: 1,
                    error: None,
                },
            },
            ManifestFixture::new(
                "response-gui",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(1)),
                    ("tick", ManifestValue::U64(47)),
                    ("tag", ManifestValue::Tag("RESPONSE_GUI")),
                    ("applied", ManifestValue::U32(1)),
                    ("error", ManifestValue::None),
                ],
            ),
            &mut covered,
        );

        assert_manifest_response(
            Response {
                session: 7,
                request_id: 2,
                tick: 47,
                body: ResponseBody::GuiInput {
                    tick: 47,
                    unhandled: None,
                },
            },
            ManifestFixture::new(
                "response-gui-input",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(2)),
                    ("tick", ManifestValue::U64(47)),
                    ("tag", ManifestValue::Tag("RESPONSE_GUI_INPUT")),
                    (
                        "routing",
                        manifest_layout(
                            "gui-input-routing",
                            [
                                ("tick", ManifestValue::U64(47)),
                                ("reason", ManifestValue::U16(0)),
                                ("blocker", ManifestValue::None),
                            ],
                        ),
                    ),
                ],
            ),
            &mut covered,
        );

        let mut payload = vec![1u8];
        payload.extend(42u64.to_le_bytes());
        payload.extend(3u64.to_le_bytes());
        payload.extend(0u32.to_le_bytes());
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 1,
                tick: 47,
                body: ResponseBody::GuiInspect(ipp_core::systems::gui::GuiInspectResponse {
                    root_entity: EntityId::from_bits(42),
                    root_incarnation: 3,
                    nodes: Vec::new(),
                }),
            },
            ManifestFixture::new(
                "response-gui-inspect",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(1)),
                    ("tick", ManifestValue::U64(47)),
                    ("tag", ManifestValue::Tag("RESPONSE_GUI_INSPECT")),
                    ("payload", ManifestValue::Bytes(payload)),
                ],
            ),
            &mut covered,
        );

        // One committed button press: version, effect count, kind, head
        // (session, ticks, entity, node, lifetime, empty path), then empty
        // conflict and cancellation counts.
        let mut observations = vec![3u8];
        observations.extend(1u32.to_le_bytes());
        observations.push(0);
        observations.extend(7u64.to_le_bytes());
        observations.extend(11u64.to_le_bytes());
        observations.extend(12u64.to_le_bytes());
        observations.extend(EntityId::from_bits(100).to_bits().to_le_bytes());
        observations.extend(3u64.to_le_bytes());
        observations.extend(30u32.to_le_bytes());
        observations.extend(1u32.to_le_bytes());
        observations.extend(0u32.to_le_bytes());
        observations.extend(0u32.to_le_bytes());
        observations.extend(0u32.to_le_bytes());
        observations.extend(0u32.to_le_bytes());
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 0,
                tick: 47,
                body: ResponseBody::GuiObservations {
                    effects: vec![ipp_core::GuiInputEffect {
                        session: 7,
                        source_tick: 11,
                        effect_tick: 12,
                        kind: ipp_core::GuiInputEffectKind::ButtonPressed {
                            entity: EntityId::from_bits(100),
                            root_incarnation: 3,
                            node: ipp_core::GuiNodeId(30),
                            lifetime: 1,
                            path: Vec::new(),
                        },
                    }],
                    text_focus_updates: Vec::new(),
                    conflicts: Vec::new(),
                    cancellations: Vec::new(),
                },
            },
            ManifestFixture::new(
                "response-gui-observations",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(0)),
                    ("tick", ManifestValue::U64(47)),
                    ("tag", ManifestValue::Tag("RESPONSE_GUI_OBSERVATIONS")),
                    ("observations", ManifestValue::Bytes(observations)),
                ],
            ),
            &mut covered,
        );

        // One semantic snapshot: version, panel identity, two nodes,
        // then the observed focus. Names and values are whole bounded strings;
        // oversize values reject instead of publishing a misleading prefix.
        let mut snapshot = vec![1u8];
        snapshot.extend(42u64.to_le_bytes());
        snapshot.extend(3u64.to_le_bytes());
        snapshot.extend(12u64.to_le_bytes());
        snapshot.extend(2u32.to_le_bytes());
        snapshot.extend(1u32.to_le_bytes());
        snapshot.extend(0u32.to_le_bytes());
        snapshot.extend(1u32.to_le_bytes());
        snapshot.push(0);
        snapshot.push(0);
        snapshot.push(0);
        snapshot.extend(0u32.to_le_bytes());
        for bound in [0.0f32, 0.0, 10.0, 10.0] {
            snapshot.extend(bound.to_le_bytes());
        }
        snapshot.extend([1, 1, 1, 0]);
        snapshot.extend(2u32.to_le_bytes());
        snapshot.extend(1u32.to_le_bytes());
        snapshot.extend(1u32.to_le_bytes());
        snapshot.push(4);
        snapshot.push(1);
        snapshot.extend(2u32.to_le_bytes());
        snapshot.extend("Go".as_bytes());
        snapshot.push(0);
        snapshot.extend(0u32.to_le_bytes());
        for bound in [1.0f32, 1.0, 2.0, 1.0] {
            snapshot.extend(bound.to_le_bytes());
        }
        snapshot.extend([1, 1, 1]);
        snapshot.push(1);
        snapshot.push(0);
        snapshot.push(1);
        snapshot.extend(2u32.to_le_bytes());
        snapshot.extend(1u32.to_le_bytes());
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 1,
                tick: 47,
                body: ResponseBody::GuiSemanticSnapshot(ipp_core::GuiSemanticTree {
                    entity: EntityId::from_bits(42),
                    root_incarnation: 3,
                    evaluation_tick: 12,
                    nodes: vec![
                        ipp_core::GuiSemanticNode {
                            id: ipp_core::GuiNodeId(1),
                            lifetime: 1,
                            parent: None,
                            role: ipp_core::GuiSemanticRole::Container,
                            name: None,
                            value: ipp_core::GuiControlValue::None,
                            revision: 0,
                            bounds: [0.0, 0.0, 10.0, 10.0],
                            enabled: true,
                            visible: true,
                            available: true,
                            actions: Vec::new(),
                        },
                        ipp_core::GuiSemanticNode {
                            id: ipp_core::GuiNodeId(2),
                            lifetime: 1,
                            parent: Some(ipp_core::GuiNodeId(1)),
                            role: ipp_core::GuiSemanticRole::Button,
                            name: Some("Go".into()),
                            value: ipp_core::GuiControlValue::None,
                            revision: 0,
                            bounds: [1.0, 1.0, 2.0, 1.0],
                            enabled: true,
                            visible: true,
                            available: true,
                            actions: vec![ipp_core::GuiSemanticActionKind::Press],
                        },
                    ],
                    focused: Some(ipp_core::GuiSemanticFocus {
                        id: ipp_core::GuiNodeId(2),
                        lifetime: 1,
                    }),
                }),
            },
            ManifestFixture::new(
                "response-gui-semantic-snapshot",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(1)),
                    ("tick", ManifestValue::U64(47)),
                    ("tag", ManifestValue::Tag("RESPONSE_GUI_SEMANTIC_SNAPSHOT")),
                    ("snapshot", ManifestValue::Bytes(snapshot)),
                ],
            ),
            &mut covered,
        );

        // One supplier-private unhandled Blur: version, input count,
        // session, tick, versioned input tag, NoFocus reason.
        let mut unhandled = vec![1u8];
        unhandled.extend(1u32.to_le_bytes());
        unhandled.extend(7u64.to_le_bytes());
        unhandled.extend(11u64.to_le_bytes());
        unhandled.extend([1, 9]);
        unhandled.push(3);
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 0,
                tick: 47,
                body: ResponseBody::GuiUnhandledInputs {
                    inputs: vec![ipp_core::GuiUnhandledInput {
                        session: 7,
                        source_request_id: 0,
                        tick: 11,
                        input: ipp_core::GuiInputCommand::Blur,
                        reason: ipp_core::GuiUnhandledReason::NoFocus,
                    }],
                },
            },
            ManifestFixture::new(
                "response-gui-unhandled",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(0)),
                    ("tick", ManifestValue::U64(47)),
                    ("tag", ManifestValue::Tag("RESPONSE_GUI_UNHANDLED")),
                    ("unhandled", ManifestValue::Bytes(unhandled)),
                ],
            ),
            &mut covered,
        );
    }

    for tag in TAGS.iter().filter(|tag| {
        matches!(
            tag.space,
            TagSpace::Response
                | TagSpace::Outcome
                | TagSpace::StateOverlayHandleKind
                | TagSpace::StateOverlayLifecycleReason
                | TagSpace::AssetResourceStatus
                | TagSpace::SnapshotValue
                | TagSpace::SnapshotReference
                | TagSpace::GeometryPickOutcome
                | TagSpace::LifecycleObservation
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
    let mut fields = envelope("RESPONSE_LIFECYCLE_OVERFLOW");
    fields.push(("dropped", ManifestValue::U64(129)));
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 0,
            tick: 3,
            body: ResponseBody::LifecycleEvents(LifecyclePublisherOutput::Overflow {
                dropped: 129,
            }),
        },
        ManifestFixture::new("response-lifecycle-overflow", fields),
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
            body: ResponseBody::LifecycleEvents(LifecyclePublisherOutput::Events(events)),
        },
        ManifestFixture::new("response-lifecycle-events", fields),
        covered,
    );
}
