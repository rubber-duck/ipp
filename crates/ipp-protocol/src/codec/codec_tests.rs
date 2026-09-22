use super::*;

fn request(tag: u8) -> Vec<u8> {
    let mut bytes = 7u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&9u64.to_le_bytes());
    bytes.push(tag);
    if tag == REQUEST_INSPECT {
        bytes.extend_from_slice(&[0; 17]);
        bytes.extend_from_slice(&256u16.to_le_bytes());
    }
    bytes
}

#[test]
fn playback_controls_reject_invalid_identity_time_and_tags_before_queueing() {
    for (control, time, speed) in [
        (0u32, 1.0f64, 0.0f32),
        (3, -1.0, 0.0),
        (3, f64::NAN, 0.0),
        (3, f64::INFINITY, 0.0),
        (6, 0.0, 0.0),
    ] {
        let mut bytes = request(REQUEST_PLAYBACK);
        bytes[8..16].fill(0);
        bytes.extend_from_slice(&41u64.to_le_bytes());
        bytes.extend_from_slice(&control.to_le_bytes());
        bytes.extend_from_slice(&time.to_le_bytes());
        bytes.extend_from_slice(&speed.to_le_bytes());
        assert!(decode_request(&bytes, 7).is_err());
    }
    let mut bytes = request(REQUEST_PLAYBACK);
    bytes.extend_from_slice(&41u64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0f64.to_le_bytes());
    bytes.extend_from_slice(&0f32.to_le_bytes());
    assert!(decode_request(&bytes, 7).is_err());
    bytes[8..16].fill(0);
    assert!(decode_request(&bytes, 7).is_ok());
    for end in 0..bytes.len() {
        assert!(decode_request(&bytes[..end], 7).is_err());
    }
    bytes.push(0);
    assert!(decode_request(&bytes, 7).is_err());

    let mut bytes = request(REQUEST_PLAYBACK);
    bytes[8..16].fill(0);
    bytes.extend_from_slice(&41u64.to_le_bytes());
    bytes.extend_from_slice(&(PLAYBACK_CONTROL_PLAY_AT_SPEED as u32).to_le_bytes());
    bytes.extend_from_slice(&0f64.to_le_bytes());
    bytes.extend_from_slice(&(-1.0f32).to_le_bytes());
    assert!(decode_request(&bytes, 7).is_ok());

    for speed in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut bytes = request(REQUEST_PLAYBACK);
        bytes[8..16].fill(0);
        bytes.extend_from_slice(&41u64.to_le_bytes());
        bytes.extend_from_slice(&(PLAYBACK_CONTROL_PLAY_AT_SPEED as u32).to_le_bytes());
        bytes.extend_from_slice(&0f64.to_le_bytes());
        bytes.extend_from_slice(&speed.to_le_bytes());
        assert!(decode_request(&bytes, 7).is_err());
    }
}

#[test]
fn integer_field_codecs_preserve_full_width() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.push(VALUE_U64);
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    bytes.extend_from_slice(&28u32.to_le_bytes());
    bytes.push(VALUE_U32);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    let mut reader = Reader {
        bytes: &bytes,
        at: 0,
    };
    assert_eq!(
        reader.field().unwrap(),
        FieldWrite {
            offset: 16,
            value: FieldValue::U64(u64::MAX)
        }
    );
    assert_eq!(
        reader.field().unwrap(),
        FieldWrite {
            offset: 28,
            value: FieldValue::U32(u32::MAX)
        }
    );
    assert_eq!(reader.at, bytes.len());
}

#[test]
fn owned_field_codecs_are_feature_independent() {
    let mut bytes = 12u32.to_le_bytes().to_vec();
    bytes.push(VALUE_STRING);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(b"name");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.push(VALUE_BYTES);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[7, 8, 9]);

    let mut reader = Reader {
        bytes: &bytes,
        at: 0,
    };
    assert_eq!(
        reader.field().unwrap(),
        FieldWrite {
            offset: 12,
            value: FieldValue::String("name".into()),
        }
    );
    assert_eq!(
        reader.field().unwrap(),
        FieldWrite {
            offset: 16,
            value: FieldValue::Bytes(vec![7, 8, 9]),
        }
    );
    assert_eq!(reader.at, bytes.len());
}

#[test]
fn string_fields_own_strict_bounded_utf8_and_encode_at_target_offsets() {
    let offset = std::mem::offset_of!(ipp_core::components::MeshInstance, source) as u32;
    let source = "https://example.test/é.ippm";
    let mut bytes = offset.to_le_bytes().to_vec();
    bytes.push(VALUE_STRING);
    bytes.extend_from_slice(&(source.len() as u32).to_le_bytes());
    bytes.extend_from_slice(source.as_bytes());
    let decoded = Reader {
        bytes: &bytes,
        at: 0,
    }
    .field()
    .unwrap();
    bytes[9..].fill(0);
    assert_eq!(
        decoded,
        FieldWrite {
            offset,
            value: FieldValue::String(source.into())
        }
    );

    for n in 0..bytes.len() {
        assert!(
            Reader {
                bytes: &bytes[..n],
                at: 0
            }
            .field()
            .is_err()
        );
    }
    bytes[9] = 0xff;
    assert_eq!(
        Reader {
            bytes: &bytes,
            at: 0
        }
        .field(),
        Err(ProtocolError::Malformed("utf8"))
    );
    bytes[5..9].copy_from_slice(&65537u32.to_le_bytes());
    assert_eq!(
        Reader {
            bytes: &bytes,
            at: 0
        }
        .field(),
        Err(ProtocolError::Limit("count"))
    );
    bytes.truncate(9);
    bytes[5..9].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        Reader {
            bytes: &bytes,
            at: 0
        }
        .field()
        .unwrap()
        .value,
        FieldValue::String(String::new())
    );
}

#[test]
fn resource_events_are_ordered_bounded_and_unsolicited() {
    use ipp_core::{AssetResourceSnapshot, AssetResourceStatus};
    let resource = AssetResourceSnapshot {
        representation: Default::default(),
        id: 1,
        kind: ipp_core::services::asset_management::AssetTypeId(1),
        source: "x".repeat(4097),
        variant: u32::MAX,
        status: AssetResourceStatus::Failed("e".repeat(2048)),
    };
    let encode = |resources, request_id| {
        encode_response(&Response {
            session: 7,
            request_id,
            tick: 9,
            body: ResponseBody::Resources {
                resources,
            },
        })
    };
    let resources: Vec<_> = (1..=16)
        .map(|id| AssetResourceSnapshot {
            id,
            ..resource.clone()
        })
        .collect();
    let bytes = encode(resources.clone(), 0).unwrap();
    assert_eq!(bytes[24], 9);
    assert_eq!(&bytes[25..29], &16u32.to_le_bytes());
    assert!(bytes.len() < MAX_MESSAGE_BYTES);
    assert!(encode(resources, 1).is_err());
    assert!(encode(vec![], 0).is_err());
    assert!(encode(vec![resource.clone(); 129], 0).is_err());
    assert!(encode(vec![resource.clone(); 2], 0).is_ok());
    for invalid in [
        AssetResourceSnapshot {
            source: "x".repeat(65537),
            ..resource.clone()
        },
        AssetResourceSnapshot {
            status: AssetResourceStatus::Failed("e".repeat(65537)),
            ..resource.clone()
        },
        AssetResourceSnapshot {
            kind: ipp_core::services::asset_management::AssetTypeId(0),
            ..resource.clone()
        },
    ] {
        assert!(encode(vec![invalid], 0).is_err());
    }
    assert!(
        encode(
            vec![AssetResourceSnapshot {
                status: AssetResourceStatus::Loaded,
                ..resource
            }],
            0
        )
        .is_ok()
    );
}

#[test]
fn inspection_encodes_current_resource_statuses_after_entities() {
    use ipp_core::{AssetResourceKind, AssetResourceSnapshot, AssetResourceStatus};

    let response = Response {
        session: 7,
        request_id: 1,
        tick: 9,
        body: ResponseBody::Inspect {
            next: 0,
            controllers: Vec::new(),
            time: 0.5,
            entities: vec![],
            render_diagnostics: vec![ipp_core::RenderDiagnostic {
                entity: EntityId::from_bits(0x100000001),
                reason: ipp_core::ErrorReason::InvalidAsset,
            }],
            resources: [
                AssetResourceStatus::Start,
                AssetResourceStatus::Loaded,
                AssetResourceStatus::Failed("fetch failed".into()),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, status)| AssetResourceSnapshot {
                representation: Default::default(),
                id: i as u64 + 1,
                kind: AssetResourceKind::Mesh,
                source: "https://example.test/é.ippm".into(),
                variant: u32::MAX,
                status,
            })
            .collect(),
        },
    };
    let bytes = encode_response(&response).unwrap();
    let mut reader = Reader {
        bytes: &bytes,
        at: 41,
    };
    assert_eq!(reader.u32().unwrap(), 0);
    assert_eq!(reader.u32().unwrap(), 3);
    for status in 0..3 {
        assert_eq!(reader.u64().unwrap(), status as u64 + 1);
        assert_eq!(reader.u16().unwrap(), 1);
        assert_eq!(reader.string().unwrap(), "https://example.test/é.ippm");
        assert_eq!(reader.u32().unwrap(), u32::MAX);
        assert_eq!(reader.u8().unwrap(), [1, 3, 4][status as usize]);
        if status == 2 {
            assert_eq!(reader.string().unwrap(), "fetch failed");
        }
        assert_eq!(reader.take(19).unwrap(), &[0; 19]);
    }
    assert_eq!(reader.u32().unwrap(), 1);
    assert_eq!(reader.u64().unwrap(), 0x100000001);
    assert_eq!(reader.string().unwrap(), "InvalidAsset");
    assert_eq!(reader.u32().unwrap(), 0);
    assert_eq!(reader.at, bytes.len());
}

#[test]
fn inspection_encodes_dynamic_components_beyond_static_field_limits() {
    let mut material = ipp_core::components::CustomMaterial::default();
    for index in 0..3000 {
        material
            .properties
            .set(
                &format!("node_{index}_background_corner_radius"),
                ipp_core::DynamicValue::F32(index as f32),
            )
            .unwrap();
    }
    let fields = ComponentValue::CustomMaterial(material.clone()).fields();
    let field_count = fields.len();
    assert!(field_count > 256);
    assert!(fields.iter().any(|(_, value)| {
        matches!(value, ResolvedValue::Bytes(bytes) if bytes.len() > 65_536)
    }));

    let bytes = encode_response(&Response {
        session: 7,
        request_id: 1,
        tick: 9,
        body: ResponseBody::Inspect {
            next: 0,
            controllers: Vec::new(),
            time: 0.5,
            entities: vec![ipp_core::EntitySnapshot {
                id: EntityId::from_bits(42),
                metadata: EntityMetadata::default(),
                base: vec![ComponentValue::CustomMaterial(material)],
                effective: Vec::new(),
            }],
            resources: Vec::new(),
            render_diagnostics: Vec::new(),
        },
    })
    .unwrap();

    // Response envelope, inspection header, entity identity/metadata and the
    // base-component count precede this component's u16 type and u32 count.
    assert_eq!(
        u32::from_le_bytes(bytes[64..68].try_into().unwrap()) as usize,
        field_count
    );
    assert!(bytes.len() < MAX_MESSAGE_BYTES);
}

#[test]
fn effective_inspection_refers_to_an_identical_base_descriptor_table() {
    let mut material = ipp_core::components::CustomMaterial::default();
    for index in 0..3000 {
        material
            .properties
            .set(
                &format!("node_{index}_background_corner_radius"),
                ipp_core::DynamicValue::F32(index as f32),
            )
            .unwrap();
    }
    let table = descriptor_table(&material.properties);
    assert!(table.len() > 65_536);

    // An animated effective value keeps the same descriptor identities.
    let mut animated = material.clone();
    animated
        .properties
        .set(
            "node_0_background_corner_radius",
            ipp_core::DynamicValue::F32(9.0),
        )
        .unwrap();
    let mut renamed = material.clone();
    renamed
        .properties
        .set("extra", ipp_core::DynamicValue::F32(1.0))
        .unwrap();
    let encode = |base: Vec<ComponentValue>, effective: Vec<ComponentValue>| {
        encode_response(&Response {
            session: 7,
            request_id: 1,
            tick: 9,
            body: ResponseBody::Inspect {
                next: 0,
                controllers: Vec::new(),
                time: 0.5,
                entities: vec![ipp_core::EntitySnapshot {
                    id: EntityId::from_bits(42),
                    metadata: EntityMetadata::default(),
                    base,
                    effective,
                }],
                resources: Vec::new(),
                render_diagnostics: Vec::new(),
            },
        })
        .unwrap()
    };
    let occurrences = |bytes: &[u8]| bytes.windows(table.len()).filter(|w| *w == table).count();

    let base_only = encode(
        vec![ComponentValue::CustomMaterial(material.clone())],
        vec![],
    );
    let shared = encode(
        vec![ComponentValue::CustomMaterial(material.clone())],
        vec![ComponentValue::CustomMaterial(animated)],
    );
    assert_eq!(occurrences(&shared), 1);
    // The effective copy adds its values and a one-byte reference, not a second table.
    assert!(shared.len() - base_only.len() < base_only.len() - table.len());
    let reference = [
        DYNAMIC_METADATA_OFFSET.to_le_bytes().as_slice(),
        &[SNAPSHOT_VALUE_BASE_DESCRIPTORS],
    ]
    .concat();
    assert!(shared.windows(reference.len()).any(|w| w == reference));

    // Different descriptors, or no base counterpart, still carry a complete table.
    let changed = encode(
        vec![ComponentValue::CustomMaterial(material.clone())],
        vec![ComponentValue::CustomMaterial(renamed.clone())],
    );
    assert_eq!(occurrences(&changed), 1);
    let renamed_table = descriptor_table(&renamed.properties);
    assert!(
        changed
            .windows(renamed_table.len())
            .any(|w| w == renamed_table)
    );
    let effective_only = encode(vec![], vec![ComponentValue::CustomMaterial(material)]);
    assert_eq!(occurrences(&effective_only), 1);
    assert!(
        !effective_only
            .windows(reference.len())
            .any(|w| w == reference)
    );
}

const DYNAMIC_METADATA_OFFSET: u32 = ipp_core::components::dynamic_properties::DYNAMIC_METADATA;

fn descriptor_table(
    properties: &ipp_core::components::dynamic_properties::DynamicProperties,
) -> Vec<u8> {
    match properties.field(DYNAMIC_METADATA_OFFSET) {
        Ok(ResolvedValue::Bytes(table)) => table,
        other => panic!("descriptor table unavailable: {other:?}"),
    }
}

#[test]
fn large_inspected_byte_fields_still_obey_the_message_budget() {
    let mut writer = Writer(Vec::new());
    assert_eq!(
        writer.resolved_field(0, ResolvedValue::Bytes(vec![0; MAX_MESSAGE_BYTES])),
        Err(ProtocolError::Limit("message")),
    );
}

#[test]
fn removed_builtin_requests_reject() {
    for tag in [6, 7] {
        assert_eq!(
            decode_request(&request(tag), 7),
            Err(ProtocolError::Unsupported(tag))
        );
    }
}

#[test]
fn omitted_asset_capabilities_reject_their_wire_tags() {
    for (tag, enabled) in [
        (5, true),
        (6, cfg!(feature = "builtin-assets")),
        (7, cfg!(feature = "builtin-assets")),
    ] {
        if !enabled {
            assert_eq!(
                decode_request(&request(tag), 7),
                Err(ProtocolError::Unsupported(tag))
            );
        }
    }
}

#[test]
fn camera_notifications_and_geometry_results_enforce_distinct_identities() {
    let mut response = Response {
        session: 7,
        request_id: 0,
        tick: 11,
        body: ResponseBody::CameraStateChangedEvent(ipp_core::CameraStateChange {
            tick: 11,
            changes: ipp_core::CameraStatePatch {
                active_camera: Some(EntityId::from_bits(9)),
            },
        }),
    };
    assert!(encode_response(&response).is_ok());
    response.request_id = 9;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("reserved response identity"))
    );
    response.request_id = 0;
    response.tick = 12;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("camera state change tick"))
    );
    response.tick = 11;
    response.body = ResponseBody::CameraStateChangedEvent(ipp_core::CameraStateChange {
        tick: 11,
        changes: ipp_core::CameraStatePatch::default(),
    });
    assert!(encode_response(&response).is_err());

    {
        response.body = ResponseBody::GeometryPickResultEvent(ipp_core::GeometryPickOutcome {
            request_id: 9,
            tick: 11,
            camera: None,
            result: Err(ipp_core::ErrorReason::NoActiveCamera),
        });
        assert!(encode_response(&response).is_err());
        response.request_id = 9;
        assert!(encode_response(&response).is_ok());
        response.request_id = 10;
        assert_eq!(
            encode_response(&response),
            Err(ProtocolError::Malformed("geometry outcome correlation"))
        );
        response.request_id = 9;
        response.tick = 12;
        assert_eq!(
            encode_response(&response),
            Err(ProtocolError::Malformed("geometry outcome correlation"))
        );
    }
}

#[test]
fn system_input_identities_and_malformed_navigation_reject_before_queueing() {
    for (tag, payload) in [
        (REQUEST_CAMERA_ACTIVATE, 9u64.to_le_bytes().to_vec()),
        (REQUEST_RENDER_STATE_UPDATE, 0u16.to_le_bytes().to_vec()),
        (
            REQUEST_CAMERA_NAVIGATE,
            [vec![CAMERA_MOTION_ZOOM], 0.5f32.to_le_bytes().to_vec()].concat(),
        ),
    ] {
        let mut bytes = request(tag);
        bytes.extend(payload);
        assert_eq!(
            decode_request(&bytes, 7),
            Err(ProtocolError::Malformed("reserved request identity"))
        );
        bytes[8..16].copy_from_slice(&0u64.to_le_bytes());
        assert!(decode_request(&bytes, 7).is_ok());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_request(&trailing, 7).is_err());
        bytes.pop();
        assert!(decode_request(&bytes, 7).is_err());
    }
    let mut bytes = request(REQUEST_CAMERA_NAVIGATE);
    bytes[8..16].copy_from_slice(&0u64.to_le_bytes());
    bytes.push(255);
    assert_eq!(
        decode_request(&bytes, 7),
        Err(ProtocolError::Malformed("camera motion tag"))
    );
    bytes[17] = CAMERA_MOTION_ZOOM;
    bytes.extend_from_slice(&f32::NAN.to_le_bytes());
    assert!(decode_request(&bytes, 7).is_err());

    {
        let mut bytes = request(REQUEST_GEOMETRY_PICK);
        bytes.extend_from_slice(&[0; 17]);
        bytes[8..16].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            decode_request(&bytes, 7),
            Err(ProtocolError::Malformed("reserved request identity"))
        );
    }
}

#[test]
fn unavailable_transient_events_reject_before_payload_decode() {
    for (tag, enabled) in [(8, true), (9, true), (10, true), (11, true)] {
        if !enabled {
            assert_eq!(
                decode_request(&request(tag), 7),
                Err(ProtocolError::Unsupported(tag))
            );
        }
    }
}

#[test]
fn geometry_pick_flags_require_a_canonical_boolean_and_exact_payload() {
    let mut bytes = request(REQUEST_GEOMETRY_PICK);
    bytes.extend_from_slice(&0.5f32.to_le_bytes());
    bytes.extend_from_slice(&0.5f32.to_le_bytes());
    bytes.extend_from_slice(&640u32.to_le_bytes());
    bytes.extend_from_slice(&480u32.to_le_bytes());
    assert!(decode_request(&bytes, 7).is_err());
    for flag in 0..=255 {
        let mut encoded = bytes.clone();
        encoded.push(flag);
        let result = decode_request(&encoded, 7);
        if flag <= 1 {
            let RequestBody::GeometryPickQuery(query) = result.unwrap().body else {
                panic!("expected query")
            };
            assert_eq!(query.include_view_plane, flag == 1);
            encoded.push(0);
            assert!(decode_request(&encoded, 7).is_err());
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn bootstrap_rejects_before_schema_decode() {
    let b = crate::bootstrap();
    assert_eq!(crate::accept_bootstrap(&b, 7).unwrap().len(), 24);
    let mut wrong = b;
    wrong[8] ^= 1;
    assert_eq!(
        crate::accept_bootstrap(&wrong, 7),
        Err(ProtocolError::SchemaMismatch)
    );
    assert!(crate::accept_bootstrap(&b[..15], 7).is_err());
}

#[test]
fn session_truncation_trailing_and_tags_reject() {
    let b = request(REQUEST_INSPECT);
    assert_eq!(
        decode_request(&b, 7).unwrap().body,
        RequestBody::Inspect(Default::default())
    );
    for n in 0..b.len() {
        assert!(decode_request(&b[..n], 7).is_err());
    }
    assert_eq!(decode_request(&b, 8), Err(ProtocolError::SessionMismatch));
    let mut trailing = b;
    trailing.push(0);
    assert!(decode_request(&trailing, 7).is_err());
    assert_eq!(
        decode_request(&request(99), 7),
        Err(ProtocolError::Unsupported(99))
    );
}

#[test]
fn bound_counts_before_allocating() {
    let mut b = request(REQUEST_BATCH);
    b.extend_from_slice(&1u64.to_le_bytes());
    b.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        decode_request(&b, 7),
        Err(ProtocolError::Limit(_))
    ));
    assert!(matches!(
        decode_request(&vec![0; MAX_MESSAGE_BYTES + 1], 7),
        Err(ProtocolError::Limit(_))
    ));
}

#[test]
fn reserved_request_identity_and_retired_step_reject() {
    let mut bytes = request(REQUEST_INSPECT);
    bytes[8..16].fill(0);
    assert_eq!(
        decode_request(&bytes, 7),
        Err(ProtocolError::Malformed("reserved request identity"))
    );

    let mut bytes = request(2);
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    assert_eq!(
        decode_request(&bytes, 7),
        Err(ProtocolError::Unsupported(2))
    );
}

#[test]
fn frame_encoding_and_response_identity_are_fenced() {
    let mut response = Response {
        session: 7,
        request_id: 0,
        tick: 11,
        body: ResponseBody::Frame {
            time: 0.25,
        },
    };
    let mut expected = 7u64.to_le_bytes().to_vec();
    expected.extend_from_slice(&0u64.to_le_bytes());
    expected.extend_from_slice(&11u64.to_le_bytes());
    expected.push(4);
    expected.extend_from_slice(&0.25f64.to_le_bytes());
    assert_eq!(encode_response(&response).unwrap(), expected);

    response.request_id = 1;
    assert!(encode_response(&response).is_err());
    response.request_id = 0;
    response.session = 0;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::SessionMismatch)
    );
    response.session = 7;

    for body in [
        ResponseBody::Error {
            code: 1,
            message: "rejected".into(),
        },
        ResponseBody::Inspect {
            next: 0,
            controllers: Vec::new(),
            time: 0.0,
            entities: vec![],
            resources: vec![],
            render_diagnostics: vec![],
        },
        ResponseBody::Batch(BatchOutcome {
            batch_id: 1,
            tick: 11,
            result: Ok(vec![]),
            state_overlays: vec![],
        }),
    ] {
        response.body = body;
        response.request_id = 0;
        assert!(encode_response(&response).is_err());
        response.request_id = 1;
        assert!(encode_response(&response).is_ok());
    }
}

#[test]
fn response_times_are_finite_and_nonnegative() {
    for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        for (request_id, body) in [
            (
                0,
                ResponseBody::Frame {
                    time,
                },
            ),
            (
                1,
                ResponseBody::Inspect {
                    next: 0,
                    controllers: Vec::new(),
                    time,
                    entities: vec![],
                    resources: vec![],
                    render_diagnostics: vec![],
                },
            ),
        ] {
            assert!(
                encode_response(&Response {
                    session: 7,
                    request_id,
                    tick: 1,
                    body,
                })
                .is_err()
            );
        }
    }
}

#[test]
fn decoded_metadata_owns_source() {
    let mut b = request(REQUEST_BATCH);
    b.extend_from_slice(&1u64.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    b.push(COMMAND_CREATE);
    b.extend_from_slice(&4u32.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&3u32.to_le_bytes());
    b.extend_from_slice(b"abc");
    b.extend_from_slice(&0u32.to_le_bytes());
    let decoded = decode_request(&b, 7).unwrap();
    b.fill(0);
    let RequestBody::Batch(batch) = decoded.body else {
        panic!()
    };
    assert_eq!(
        batch.operations,
        vec![Command::Create {
            alias: 4,
            metadata: EntityMetadata {
                symbolic_id: Some("abc".into()),
                classes: vec![]
            }
        }]
    );
}

#[test]
fn declarations_keep_owned_data_and_reject_truncation_and_invalid_modes() {
    let mut w = Writer(request(REQUEST_BATCH));
    w.u64(1).unwrap();
    w.u32(1).unwrap();
    w.u8(COMMAND_ATTACH_ENTITY_OVERLAY_BINDING).unwrap();
    w.u8(REF_ALIAS).unwrap();
    w.u32(3).unwrap();
    w.u32(4).unwrap();
    w.string("owned entity").unwrap();
    w.u8(0).unwrap();
    let decoded = decode_request(&w.0, 7).unwrap();
    for length in 0..w.0.len() {
        assert!(decode_request(&w.0[..length], 7).is_err());
    }
    *w.0.last_mut().unwrap() = 2;
    assert_eq!(
        decode_request(&w.0, 7),
        Err(ProtocolError::Malformed("entity mode"))
    );
    w.0.fill(0);
    let RequestBody::Batch(batch) = decoded.body else {
        panic!()
    };
    assert_eq!(
        batch.operations,
        vec![Command::AttachEntityOverlayBinding {
            owner: ipp_core::StateOverlayRef::Alias(3),
            alias: 4,
            symbolic_id: "owned entity".into(),
            mode: ipp_core::EntityOverlayMode::Owned,
        }]
    );
}

#[test]
fn lifecycle_uses_unsolicited_identity_and_scoped_resource_handles() {
    let mut response = Response {
        session: 7,
        request_id: 0,
        tick: 9,
        body: ResponseBody::Lifecycle {
            diagnostics: vec![ipp_core::StateOverlayLifecycleDiagnostic {
                owner: 11,
                state_overlay: 12,
                entity: ipp_core::EntityId::from_bits(13),
                component: Some(1),
                reason: ipp_core::StateOverlayLifecycleReason::ComponentReplaced,
            }],
        },
    };
    let bytes = encode_response(&response).unwrap();
    assert_eq!(bytes[24], RESPONSE_STATE_OVERLAY_LIFECYCLE);
    assert_eq!(&bytes[25..29], &1u32.to_le_bytes());
    assert_eq!(&bytes[29..37], &11u64.to_le_bytes());
    assert_eq!(&bytes[37..45], &12u64.to_le_bytes());
    response.request_id = 1;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("reserved response identity"))
    );
}
#[test]
fn boolean_wire_values_reject_noncanonical_encodings() {
    for byte in [0, 1, 2, 255] {
        let bytes = [0, 0, 0, 0, VALUE_BOOL, byte];
        let decoded = Reader {
            bytes: &bytes,
            at: 0,
        }
        .field();
        if byte <= 1 {
            assert_eq!(decoded.unwrap().value, FieldValue::Bool(byte == 1));
        } else {
            assert_eq!(decoded, Err(ProtocolError::Malformed("boolean encoding")));
        }
    }
}

#[test]
fn render_state_notifications_validate_identity_tick_and_committed_colors() {
    let mut response = Response {
        session: 7,
        request_id: 0,
        tick: 2,
        body: ResponseBody::RenderStateUpdatedEvent(ipp_core::RenderStateChange {
            tick: 2,
            changes: ipp_core::RenderStatePatch {
                show_all_debug_geometries: Some(true),
                ..Default::default()
            },
        }),
    };
    assert!(encode_response(&response).is_ok());
    response.request_id = 2;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("reserved response identity"))
    );
    response.request_id = 0;
    response.tick = 3;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("render state change tick"))
    );
    response.tick = 2;
    response.body = ResponseBody::RenderStateUpdatedEvent(ipp_core::RenderStateChange {
        tick: 2,
        changes: ipp_core::RenderStatePatch::default(),
    });
    assert!(encode_response(&response).is_err());
    for value in [f32::NAN, f32::INFINITY, -1.0, 2.0] {
        response.body = ResponseBody::RenderStateUpdatedEvent(ipp_core::RenderStateChange {
            tick: 2,
            changes: ipp_core::RenderStatePatch {
                show_all_debug_geometries: Some(true),
                debug_geometry_color: Some([value, 0.0, 0.0]),
                ambient_light: None,
            },
        });
        assert!(encode_response(&response).is_err());
    }
}

#[test]
fn direct_asset_failure_encoding_enforces_the_shared_utf8_byte_limit() {
    use ipp_core::{AssetResourceSnapshot, AssetResourceStatus};
    let limit = ipp_core::services::asset_management::MAX_ASSET_ERROR_BYTES;
    for message in ["é".repeat(limit / 2), format!("{}a", "é".repeat(limit / 2))] {
        let valid = message.len() <= limit;
        let response = Response {
            session: 7,
            request_id: 0,
            tick: 1,
            body: ResponseBody::Resources {
                resources: vec![AssetResourceSnapshot {
                    representation: Default::default(),
                    id: 1,
                    kind: ipp_core::MESH_TYPE,
                    source: "test://mesh".into(),
                    variant: 0,
                    status: AssetResourceStatus::Failed(message),
                }],
            },
        };
        assert_eq!(encode_response(&response).is_ok(), valid);
    }
}

#[test]
fn decoder_reuses_grown_command_capacity_and_preserves_failure_fences() {
    let batch = |count: u32| {
        let mut bytes = request(REQUEST_BATCH);
        bytes.extend(1u64.to_le_bytes());
        bytes.extend(count.to_le_bytes());
        for _ in 0..count {
            bytes.extend([5, 0]); // scalar SetField and concrete entity
            bytes.extend(1u64.to_le_bytes());
            bytes.extend(ipp_core::ComponentValue::SCALAR.to_le_bytes());
            bytes.extend(0u32.to_le_bytes());
            bytes.push(1);
            bytes.extend(3.0f32.to_le_bytes());
        }
        bytes
    };
    let mut buffer = Vec::with_capacity(256);
    let mut capacity = buffer.capacity();
    for count in [1, 16, 200, 256, 3, 0, 256] {
        let pointer = buffer.as_ptr();
        let decoded = decode_request_with_buffer(&batch(count), 7, &mut buffer).unwrap();
        let RequestBody::Batch(decoded) = decoded.body else {
            panic!("batch expected");
        };
        assert_eq!(decoded.operations.len(), count as usize);
        if count as usize <= capacity {
            assert_eq!(decoded.operations.as_ptr(), pointer);
        }
        capacity = decoded.operations.capacity();
        assert!((256..=256).contains(&capacity));
        buffer = decoded.operations;
        buffer.clear();
    }
    let pointer = buffer.as_ptr();
    assert!(matches!(
        decode_request_with_buffer(&batch(257), 7, &mut buffer),
        Err(ProtocolError::Limit(_))
    ));
    assert_eq!(buffer.as_ptr(), pointer);
    assert!(matches!(
        decode_request_with_buffer(&batch(1), 8, &mut buffer),
        Err(ProtocolError::SessionMismatch)
    ));
    assert_eq!(buffer.as_ptr(), pointer);
    let bytes = batch(2);
    assert!(decode_request_with_buffer(&bytes[..bytes.len() - 1], 7, &mut buffer).is_err());
    buffer.clear();
    let RequestBody::Batch(decoded) = decode_request_with_buffer(&batch(1), 7, &mut buffer)
        .unwrap()
        .body
    else {
        panic!("batch expected");
    };
    assert_eq!(decoded.operations.len(), 1);
    assert_eq!(decoded.operations.as_ptr(), pointer);
}

#[test]
fn response_encoding_reuses_growth_and_clears_failed_partial_messages() {
    let mut response = Response {
        session: 1,
        request_id: 0,
        tick: 2,
        body: ResponseBody::Frame {
            time: 0.25,
        },
    };
    let mut bytes = Vec::with_capacity(64);
    let address = bytes.as_ptr();
    for tick in 1..100 {
        response.tick = tick;
        encode_response_into(&response, &mut bytes).unwrap();
        assert_eq!(bytes, encode_response(&response).unwrap());
        assert_eq!(bytes.as_ptr(), address);
    }
    response.request_id = 3;
    response.body = ResponseBody::Error {
        code: 3,
        message: "x".repeat(8192),
    };
    encode_response_into(&response, &mut bytes).unwrap();
    let grown = (bytes.as_ptr(), bytes.capacity());
    response.body = ResponseBody::Frame {
        time: f64::NAN,
    };
    response.request_id = 0;
    assert!(encode_response_into(&response, &mut bytes).is_err());
    assert!(bytes.is_empty());
    assert_eq!((bytes.as_ptr(), bytes.capacity()), grown);
    response.body = ResponseBody::Frame {
        time: 1.0,
    };
    encode_response_into(&response, &mut bytes).unwrap();
    assert_eq!(bytes, encode_response(&response).unwrap());
    assert_eq!((bytes.as_ptr(), bytes.capacity()), grown);
}

#[cfg(feature = "gui")]
#[test]
fn gui_input_decodes_every_action_and_rejects_before_queueing() {
    use ipp_core::{GuiInputCommand, GuiKey, GuiPointerButton};
    let frame = |payload: &[u8], request_id: u64| {
        let mut bytes = 7u64.to_le_bytes().to_vec();
        bytes.extend_from_slice(&request_id.to_le_bytes());
        bytes.push(REQUEST_GUI_INPUT);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    };
    let text_payload = |action: u8, text: &str| {
        [
            vec![1, action],
            (text.len() as u32).to_le_bytes().to_vec(),
            text.as_bytes().to_vec(),
        ]
        .concat()
    };
    let down = [
        vec![1, 1],
        5u32.to_le_bytes().to_vec(),
        vec![1],
        42u64.to_le_bytes().to_vec(),
        1.5f32.to_le_bytes().to_vec(),
        2.5f32.to_le_bytes().to_vec(),
        vec![1],
        0u32.to_le_bytes().to_vec(),
        vec![0],
    ]
    .concat();
    assert_eq!(
        decode_request(&frame(&down, 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::PointerDown {
            pointer: 5,
            panel: Some(EntityId::from_bits(42)),
            position: [1.5, 2.5],
            button: GuiPointerButton::Secondary,
            blockers: Vec::new(),
            panel_distance: None,
        }))
    );
    let mut zero_panel = down.clone();
    zero_panel[7..15].fill(0);
    assert!(matches!(
        decode_request(&frame(&zero_panel, 3), 7),
        Err(ProtocolError::Malformed("GUI input panel entity"))
    ));
    let up = [
        vec![1, 2],
        5u32.to_le_bytes().to_vec(),
        vec![0],
        1.5f32.to_le_bytes().to_vec(),
        2.5f32.to_le_bytes().to_vec(),
        vec![2],
        1u32.to_le_bytes().to_vec(),
        43u64.to_le_bytes().to_vec(),
        0.5f32.to_le_bytes().to_vec(),
        vec![1],
        3.25f32.to_le_bytes().to_vec(),
    ]
    .concat();
    assert_eq!(
        decode_request(&frame(&up, 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::PointerUp {
            pointer: 5,
            panel: None,
            position: [1.5, 2.5],
            button: GuiPointerButton::Auxiliary,
            blockers: vec![ipp_core::GuiBlockerHit {
                entity: EntityId::from_bits(43),
                distance: 0.5,
            }],
            panel_distance: Some(3.25),
        }))
    );
    let mut zero_blocker = up.clone();
    zero_blocker[20..28].fill(0);
    assert!(matches!(
        decode_request(&frame(&zero_blocker, 3), 7),
        Err(ProtocolError::Malformed("GUI input blocker entity"))
    ));
    let moved = [
        vec![1, 3],
        9u32.to_le_bytes().to_vec(),
        vec![0],
        0.5f32.to_le_bytes().to_vec(),
        0.75f32.to_le_bytes().to_vec(),
        0u32.to_le_bytes().to_vec(),
        vec![0],
    ]
    .concat();
    assert_eq!(
        decode_request(&frame(&moved, 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::PointerMove {
            pointer: 9,
            panel: None,
            position: [0.5, 0.75],
            blockers: Vec::new(),
            panel_distance: None,
        }))
    );
    assert_eq!(
        decode_request(&frame(&[1, 4, 9, 0, 0, 0], 3), 7)
            .unwrap()
            .body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::PointerCancel {
            pointer: 9
        }))
    );
    let scroll = [
        vec![1, 5],
        vec![0],
        1.5f32.to_le_bytes().to_vec(),
        2.5f32.to_le_bytes().to_vec(),
        0.0f32.to_le_bytes().to_vec(),
        (-4.0f32).to_le_bytes().to_vec(),
        0u32.to_le_bytes().to_vec(),
        vec![0],
    ]
    .concat();
    assert_eq!(
        decode_request(&frame(&scroll, 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::Scroll {
            panel: None,
            position: [1.5, 2.5],
            delta: [0.0, -4.0],
            blockers: Vec::new(),
            panel_distance: None,
        }))
    );
    assert_eq!(
        decode_request(&frame(&[1, 6, 10, 0], 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::Key {
            key: GuiKey::Home,
            pressed: false,
        }))
    );
    assert_eq!(
        decode_request(&frame(&text_payload(7, "héllo"), 3), 7)
            .unwrap()
            .body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::Text {
            text: "héllo".into(),
        }))
    );
    let legal_text = "a".repeat(ipp_core::MAX_GUI_TEXT_BYTES);
    assert_eq!(
        decode_request(&frame(&text_payload(7, &legal_text), 3), 7)
            .unwrap()
            .body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::Text {
            text: legal_text,
        }))
    );
    assert!(
        decode_request(
            &frame(
                &text_payload(7, &"a".repeat(ipp_core::MAX_GUI_TEXT_BYTES + 1)),
                3,
            ),
            7,
        )
        .is_err()
    );
    let focus = [
        vec![1, 8],
        7u64.to_le_bytes().to_vec(),
        42u64.to_le_bytes().to_vec(),
        3u64.to_le_bytes().to_vec(),
        9u32.to_le_bytes().to_vec(),
        1u32.to_le_bytes().to_vec(),
    ]
    .concat();
    assert_eq!(
        decode_request(&frame(&focus, 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::Focus {
            handle: ipp_core::systems::gui::GuiNodeHandle::new(
                7,
                EntityId::from_bits(42),
                3,
                ipp_core::systems::gui::GuiNodeId(9),
                1,
            ),
        }))
    );
    assert_eq!(
        decode_request(&frame(&[1, 9], 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::Blur))
    );
    assert_eq!(
        decode_request(&frame(&[1, 10, 2, 0, 0, 0, 7, 0, 0, 0], 3), 7)
            .unwrap()
            .body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::SetTextSelection {
            start: 2,
            end: 7,
        }))
    );
    let composition = [
        vec![1, 11],
        ("世界".len() as u32).to_le_bytes().to_vec(),
        "世界".as_bytes().to_vec(),
        6u32.to_le_bytes().to_vec(),
        6u32.to_le_bytes().to_vec(),
    ]
    .concat();
    assert_eq!(
        decode_request(&frame(&composition, 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::UpdateComposition {
            text: "世界".into(),
            caret_start: 6,
            caret_end: 6,
        }))
    );
    assert_eq!(
        decode_request(&frame(&[1, 12], 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::CommitComposition))
    );
    assert_eq!(
        decode_request(&frame(&[1, 13], 3), 7).unwrap().body,
        RequestBody::GuiInput(Box::new(GuiInputCommand::CancelComposition))
    );

    // Correlated inputs require a nonzero request identity.
    assert_eq!(
        decode_request(&frame(&[1, 9], 0), 7),
        Err(ProtocolError::Malformed("reserved request identity"))
    );
    for payload in [
        vec![2, 9],
        vec![1, 14],
        vec![1, 1, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3],
        vec![1, 6, 12, 1],
        {
            let mut bad = vec![1, 3, 9, 0, 0, 0, 0, 0];
            bad.extend_from_slice(&f32::NAN.to_le_bytes());
            bad.extend_from_slice(&0.75f32.to_le_bytes());
            bad.extend_from_slice(&0u32.to_le_bytes());
            bad.push(0);
            bad
        },
        {
            let mut bad = vec![1, 5, 0];
            bad.extend_from_slice(&1.5f32.to_le_bytes());
            bad.extend_from_slice(&2.5f32.to_le_bytes());
            bad.extend_from_slice(&0.0f32.to_le_bytes());
            bad.extend_from_slice(&(-4.0f32).to_le_bytes());
            bad.extend_from_slice(&1025u32.to_le_bytes());
            bad.push(0);
            bad
        },
    ] {
        assert!(decode_request(&frame(&payload, 3), 7).is_err());
    }
    let mut trailing = frame(&[1, 9], 3);
    trailing.push(0);
    assert!(decode_request(&trailing, 7).is_err());
    let truncated = frame(&[1, 9], 3);
    for end in 0..truncated.len() {
        assert!(decode_request(&truncated[..end], 7).is_err());
    }
    let bytes = encode_response(&Response {
        session: 7,
        request_id: 3,
        tick: 11,
        body: ResponseBody::GuiInput {
            tick: 11,
            unhandled: None,
        },
    })
    .unwrap();
    assert_eq!(bytes[24], RESPONSE_GUI_INPUT);
    assert_eq!(bytes.len(), 36);
}

#[cfg(feature = "gui")]
#[test]
fn gui_input_responses_require_correlated_identity() {
    let mut response = Response {
        session: 7,
        request_id: 3,
        tick: 11,
        body: ResponseBody::GuiInput {
            tick: 11,
            unhandled: Some(ipp_core::GuiUnhandledReason::NoPanelHit),
        },
    };
    assert!(encode_response(&response).is_ok());
    response.request_id = 0;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("reserved response identity"))
    );
}

#[cfg(feature = "gui")]
#[test]
fn gui_inspection_envelope_fits_one_legal_maximum_text_value() {
    use ipp_core::{GuiControlValue, GuiInspectResponse, GuiInspectedNode, GuiNodeContent};

    let response = |text: String| Response {
        session: 7,
        request_id: 3,
        tick: 11,
        body: ResponseBody::GuiInspect(GuiInspectResponse {
            root_entity: EntityId::from_bits(42),
            root_incarnation: 3,
            nodes: vec![GuiInspectedNode {
                id: ipp_core::GuiNodeId(1),
                parent: None,
                children: Vec::new(),
                content: GuiNodeContent::Text(text),
                style: ipp_core::GuiNodeStyle::default(),
                control_value: GuiControlValue::None,
                control_revision: 0,
                lifetime: 1,
            }],
        }),
    };

    let encoded = encode_response(&response("a".repeat(ipp_core::MAX_GUI_TEXT_BYTES))).unwrap();
    assert!(encoded.len() > 65_536);
    assert!(encoded.len() < MAX_MESSAGE_BYTES);
    assert_eq!(
        encode_response(&response("a".repeat(ipp_core::MAX_GUI_TEXT_BYTES + 1))),
        Err(ProtocolError::Limit("GUI text"))
    );
}

#[cfg(feature = "gui")]
#[test]
fn gui_semantics_decode_bounded_queries_and_fenced_actions() {
    use ipp_core::{GuiSemanticAction, GuiSemanticActionRequest, GuiSemanticSnapshotQuery};
    let frame = |tag: u8, payload: &[u8], request_id: u64| {
        let mut bytes = 7u64.to_le_bytes().to_vec();
        bytes.extend_from_slice(&request_id.to_le_bytes());
        bytes.push(tag);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    };
    let query = [
        vec![1],
        42u64.to_le_bytes().to_vec(),
        10u32.to_le_bytes().to_vec(),
        50u32.to_le_bytes().to_vec(),
    ]
    .concat();
    assert_eq!(
        decode_request(&frame(REQUEST_GUI_SEMANTIC_SNAPSHOT, &query, 3), 7)
            .unwrap()
            .body,
        RequestBody::GuiSemanticSnapshot(GuiSemanticSnapshotQuery {
            entity: EntityId::from_bits(42),
            max_depth: 10,
            limit: 50,
        })
    );
    let action = |kind: u8, payload: &[u8]| {
        [
            vec![2],
            42u64.to_le_bytes().to_vec(),
            3u64.to_le_bytes().to_vec(),
            9u32.to_le_bytes().to_vec(),
            1u32.to_le_bytes().to_vec(),
            2u32.to_le_bytes().to_vec(),
            vec![kind],
            payload.to_vec(),
        ]
        .concat()
    };
    for (kind, payload, expected) in [
        (0, vec![], GuiSemanticAction::Press),
        (1, vec![], GuiSemanticAction::Toggle),
        (
            2,
            1.5f32.to_le_bytes().to_vec(),
            GuiSemanticAction::SetScalar(1.5),
        ),
        (
            3,
            [
                ("héllo".len() as u32).to_le_bytes().to_vec(),
                "héllo".as_bytes().to_vec(),
            ]
            .concat(),
            GuiSemanticAction::SetText("héllo".into()),
        ),
        (4, vec![], GuiSemanticAction::Focus),
    ] {
        assert_eq!(
            decode_request(
                &frame(REQUEST_GUI_SEMANTIC_ACTION, &action(kind, &payload), 3),
                7
            )
            .unwrap()
            .body,
            RequestBody::GuiSemanticAction(Box::new(GuiSemanticActionRequest {
                entity: EntityId::from_bits(42),
                root_incarnation: 3,
                node: ipp_core::GuiNodeId(9),
                lifetime: 1,
                expected_revision: 2,
                action: expected,
            }))
        );
    }
    let legal_text = "a".repeat(ipp_core::MAX_GUI_TEXT_BYTES);
    let legal_payload = [
        (legal_text.len() as u32).to_le_bytes().to_vec(),
        legal_text.as_bytes().to_vec(),
    ]
    .concat();
    assert_eq!(
        decode_request(
            &frame(REQUEST_GUI_SEMANTIC_ACTION, &action(3, &legal_payload), 3),
            7,
        )
        .unwrap()
        .body,
        RequestBody::GuiSemanticAction(Box::new(GuiSemanticActionRequest {
            entity: EntityId::from_bits(42),
            root_incarnation: 3,
            node: ipp_core::GuiNodeId(9),
            lifetime: 1,
            expected_revision: 2,
            action: GuiSemanticAction::SetText(legal_text),
        }))
    );
    let oversize = [
        ((ipp_core::MAX_GUI_TEXT_BYTES + 1) as u32)
            .to_le_bytes()
            .to_vec(),
        vec![b'a'; ipp_core::MAX_GUI_TEXT_BYTES + 1],
    ]
    .concat();
    assert!(
        decode_request(
            &frame(REQUEST_GUI_SEMANTIC_ACTION, &action(3, &oversize), 3),
            7,
        )
        .is_err()
    );
    // Correlated queries require a nonzero request identity.
    assert_eq!(
        decode_request(&frame(REQUEST_GUI_SEMANTIC_SNAPSHOT, &query, 0), 7),
        Err(ProtocolError::Malformed("reserved request identity"))
    );
    assert_eq!(
        decode_request(&frame(REQUEST_GUI_SEMANTIC_ACTION, &action(1, &[]), 0), 7),
        Err(ProtocolError::Malformed("reserved request identity"))
    );
    // Zero targets, unknown actions, bad versions and nonfinite scalars
    // reject before queueing.
    let mut bad_entity = vec![1];
    bad_entity.extend_from_slice(&0u64.to_le_bytes());
    bad_entity.extend_from_slice(&10u32.to_le_bytes());
    bad_entity.extend_from_slice(&50u32.to_le_bytes());
    let mut bad_node = vec![2];
    bad_node.extend_from_slice(&42u64.to_le_bytes());
    bad_node.extend_from_slice(&3u64.to_le_bytes());
    bad_node.extend_from_slice(&0u32.to_le_bytes());
    bad_node.extend_from_slice(&1u32.to_le_bytes());
    bad_node.extend_from_slice(&2u32.to_le_bytes());
    bad_node.push(1);
    let mut bad_scalar = vec![2];
    bad_scalar.extend_from_slice(&42u64.to_le_bytes());
    bad_scalar.extend_from_slice(&3u64.to_le_bytes());
    bad_scalar.extend_from_slice(&9u32.to_le_bytes());
    bad_scalar.extend_from_slice(&1u32.to_le_bytes());
    bad_scalar.extend_from_slice(&2u32.to_le_bytes());
    bad_scalar.push(2);
    bad_scalar.extend_from_slice(&f32::NAN.to_le_bytes());
    for (tag, payload) in [
        (
            REQUEST_GUI_SEMANTIC_SNAPSHOT,
            vec![2, 0, 0, 0, 0, 0, 0, 0, 0],
        ),
        (REQUEST_GUI_SEMANTIC_SNAPSHOT, bad_entity),
        (REQUEST_GUI_SEMANTIC_ACTION, bad_node),
        (REQUEST_GUI_SEMANTIC_ACTION, action(5, &[])),
        (REQUEST_GUI_SEMANTIC_ACTION, bad_scalar),
    ] {
        assert!(decode_request(&frame(tag, &payload, 3), 7).is_err());
    }
    let mut trailing = frame(REQUEST_GUI_SEMANTIC_SNAPSHOT, &query, 3);
    trailing.push(0);
    assert!(decode_request(&trailing, 7).is_err());
    let truncated = frame(REQUEST_GUI_SEMANTIC_ACTION, &action(1, &[]), 3);
    for end in 0..truncated.len() {
        assert!(decode_request(&truncated[..end], 7).is_err());
    }
}

#[cfg(feature = "gui")]
#[test]
fn gui_semantic_snapshots_preserve_legal_text_and_reject_oversize() {
    use ipp_core::{
        GuiControlValue, GuiSemanticFocus, GuiSemanticNode, GuiSemanticRole, GuiSemanticTree,
    };
    let tree = GuiSemanticTree {
        entity: EntityId::from_bits(42),
        root_incarnation: 3,
        evaluation_tick: 12,
        nodes: vec![GuiSemanticNode {
            id: ipp_core::GuiNodeId(5),
            lifetime: 1,
            parent: None,
            role: GuiSemanticRole::TextInput,
            name: Some("name".into()),
            value: GuiControlValue::Text("é".repeat(ipp_core::MAX_GUI_TEXT_BYTES / 2)),
            revision: 3,
            bounds: [1.0, 4.0, 4.0, 1.0],
            enabled: true,
            visible: true,
            available: true,
            actions: vec![
                ipp_core::GuiSemanticActionKind::SetText,
                ipp_core::GuiSemanticActionKind::Focus,
            ],
        }],
        focused: Some(GuiSemanticFocus {
            id: ipp_core::GuiNodeId(5),
            lifetime: 1,
        }),
    };
    let mut response = Response {
        session: 7,
        request_id: 3,
        tick: 12,
        body: ResponseBody::GuiSemanticSnapshot(tree),
    };
    let bytes = encode_response(&response).unwrap();
    assert_eq!(bytes[24], RESPONSE_GUI_SEMANTIC_SNAPSHOT);
    assert!(bytes.len() < MAX_MESSAGE_BYTES);
    let text_bytes = &bytes[85..85 + ipp_core::MAX_GUI_TEXT_BYTES];
    assert_eq!(
        text_bytes,
        "é".repeat(ipp_core::MAX_GUI_TEXT_BYTES / 2).as_bytes()
    );

    if let ResponseBody::GuiSemanticSnapshot(tree) = &mut response.body {
        tree.nodes[0].value = GuiControlValue::Text("a".repeat(ipp_core::MAX_GUI_TEXT_BYTES + 1));
    }
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Limit("count"))
    );
    if let ResponseBody::GuiSemanticSnapshot(tree) = &mut response.body {
        tree.nodes[0].value = GuiControlValue::Text("ok".into());
    }
    response.request_id = 0;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("reserved response identity"))
    );
}

#[cfg(feature = "gui")]
#[test]
fn gui_observations_encode_committed_state_unsolicited() {
    use ipp_core::{
        GuiControlValue, GuiInputCancelReason, GuiInputCancellation, GuiInputCommand,
        GuiInputConflict, GuiInputConflictReason, GuiInputEffect, GuiInputEffectKind,
        GuiInputTarget, GuiNodeId, GuiUnhandledInput, GuiUnhandledReason,
    };
    let button = GuiInputEffect {
        session: 7,
        source_tick: 11,
        effect_tick: 12,
        kind: GuiInputEffectKind::ButtonPressed {
            entity: EntityId::from_bits(100),
            root_incarnation: 3,
            node: GuiNodeId(20),
            lifetime: 1,
            path: vec![GuiNodeId(10), GuiNodeId(20)],
        },
    };
    let conflict = GuiInputConflict {
        session: 7,
        source_tick: 11,
        effect_tick: 12,
        target: Some(GuiInputTarget {
            entity: EntityId::from_bits(100),
            node: GuiNodeId(30),
            lifetime: 1,
            root_incarnation: 3,
        }),
        reason: GuiInputConflictReason::RevisionMismatch {
            expected: 1,
            found: 2,
        },
    };
    let cancel = GuiInputCancellation {
        session: 7,
        source_tick: 11,
        effect_tick: 12,
        target: None,
        reason: GuiInputCancelReason::GestureCancelled,
    };
    let mut response = Response {
        session: 7,
        request_id: 0,
        tick: 12,
        body: ResponseBody::GuiObservations {
            effects: vec![button],
            conflicts: vec![conflict],
            cancellations: vec![cancel],
            text_focus_updates: Vec::new(),
        },
    };
    let bytes = encode_response(&response).unwrap();
    assert_eq!(bytes.len(), 191);
    assert_eq!(bytes[24], RESPONSE_GUI_OBSERVATIONS);
    assert_eq!(&bytes[25..29], &162u32.to_le_bytes());
    assert_eq!(bytes[29], 3);
    assert_eq!(&bytes[30..34], &1u32.to_le_bytes());
    assert_eq!(bytes[34], 0);
    assert_eq!(&bytes[35..43], &7u64.to_le_bytes());
    assert_eq!(&bytes[75..79], &20u32.to_le_bytes());
    assert_eq!(&bytes[83..87], &2u32.to_le_bytes());
    assert_eq!(&bytes[87..91], &10u32.to_le_bytes());
    assert_eq!(&bytes[91..95], &20u32.to_le_bytes());
    assert_eq!(bytes[148], 0);
    assert_eq!(&bytes[149..153], &1u32.to_le_bytes());
    assert_eq!(&bytes[153..157], &2u32.to_le_bytes());
    assert_eq!(bytes[186], 3);
    response.request_id = 1;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("reserved response identity"))
    );
    response.request_id = 0;
    response.body = ResponseBody::GuiObservations {
        effects: Vec::new(),
        conflicts: Vec::new(),
        cancellations: Vec::new(),
        text_focus_updates: Vec::new(),
    };
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("empty gui observations"))
    );

    let control = GuiInputEffect {
        session: 7,
        source_tick: 11,
        effect_tick: 12,
        kind: GuiInputEffectKind::ControlCommitted {
            entity: EntityId::from_bits(100),
            root_incarnation: 3,
            node: GuiNodeId(30),
            lifetime: 1,
            value: GuiControlValue::Text("hello".into()),
            revision: 2,
            path: Vec::new(),
        },
    };
    let bytes = encode_response(&Response {
        session: 7,
        request_id: 0,
        tick: 12,
        body: ResponseBody::GuiObservations {
            effects: vec![control],
            conflicts: Vec::new(),
            cancellations: Vec::new(),
            text_focus_updates: Vec::new(),
        },
    })
    .unwrap();
    assert_eq!(bytes.len(), 113);
    assert_eq!(bytes[34], 1);
    assert_eq!(&bytes[87..91], &2u32.to_le_bytes());
    assert_eq!(bytes[91], 3);
    assert_eq!(&bytes[92..96], &5u32.to_le_bytes());
    assert_eq!(&bytes[96..101], b"hello");

    let unhandled = Response {
        session: 7,
        request_id: 0,
        tick: 11,
        body: ResponseBody::GuiUnhandledInputs {
            inputs: vec![GuiUnhandledInput {
                session: 7,
                source_request_id: 0,
                tick: 11,
                input: GuiInputCommand::Blur,
                reason: GuiUnhandledReason::NoFocus,
            }],
        },
    };
    let bytes = encode_response(&unhandled).unwrap();
    assert_eq!(bytes.len(), 53);
    assert_eq!(bytes[24], RESPONSE_GUI_UNHANDLED);
    assert_eq!(bytes[50], 1);
    assert_eq!(bytes[51], 9);
    assert_eq!(bytes[52], 3);
}

#[cfg(feature = "gui")]
#[test]
fn gui_observation_text_encodes_whole_or_rejects() {
    use ipp_core::{GuiControlValue, GuiInputEffect, GuiInputEffectKind, GuiNodeId};
    let control = |text: String| GuiInputEffect {
        session: 7,
        source_tick: 11,
        effect_tick: 12,
        kind: GuiInputEffectKind::ControlCommitted {
            entity: EntityId::from_bits(100),
            root_incarnation: 3,
            node: GuiNodeId(30),
            lifetime: 1,
            value: GuiControlValue::Text(text),
            revision: 2,
            path: Vec::new(),
        },
    };
    let bytes = encode_response(&Response {
        session: 7,
        request_id: 0,
        tick: 12,
        body: ResponseBody::GuiObservations {
            effects: vec![control("a".repeat(ipp_core::MAX_GUI_TEXT_BYTES))],
            conflicts: Vec::new(),
            cancellations: Vec::new(),
            text_focus_updates: Vec::new(),
        },
    })
    .unwrap();
    assert_eq!(
        &bytes[92..96],
        &(ipp_core::MAX_GUI_TEXT_BYTES as u32).to_le_bytes()
    );
    assert_eq!(
        &bytes[96..96 + ipp_core::MAX_GUI_TEXT_BYTES],
        "a".repeat(ipp_core::MAX_GUI_TEXT_BYTES).as_bytes()
    );
    assert_eq!(
        encode_response(&Response {
            session: 7,
            request_id: 0,
            tick: 12,
            body: ResponseBody::GuiObservations {
                effects: vec![control("a".repeat(ipp_core::MAX_GUI_TEXT_BYTES + 1))],
                conflicts: Vec::new(),
                cancellations: Vec::new(),
                text_focus_updates: Vec::new(),
            },
        }),
        Err(ProtocolError::Limit("count"))
    );
    assert_eq!(
        encode_response(&Response {
            session: 7,
            request_id: 0,
            tick: 12,
            body: ResponseBody::GuiObservations {
                effects: vec![control(format!("{}é", "a".repeat(65535)))],
                conflicts: Vec::new(),
                cancellations: Vec::new(),
                text_focus_updates: Vec::new(),
            },
        }),
        Err(ProtocolError::Limit("count"))
    );
}

#[cfg(feature = "gui")]
#[test]
fn gui_observation_bodies_chunk_broadcast_and_filter_unhandled() {
    use crate::gui_observation_bodies;
    use ipp_core::{
        GuiControlValue, GuiInputCancelReason, GuiInputCancellation, GuiInputCommand,
        GuiInputConflict, GuiInputConflictReason, GuiInputEffect, GuiInputEffectKind, GuiNodeId,
        GuiUnhandledInput, GuiUnhandledReason, WorldUpdateReport,
    };
    let effect = |node: u32| GuiInputEffect {
        session: 7,
        source_tick: 11,
        effect_tick: 12,
        kind: GuiInputEffectKind::ControlCommitted {
            entity: EntityId::from_bits(100),
            root_incarnation: 3,
            node: GuiNodeId(node),
            lifetime: 1,
            value: GuiControlValue::Bool(true),
            revision: 2,
            path: vec![GuiNodeId(10), GuiNodeId(node)],
        },
    };
    let report = WorldUpdateReport {
        gui_input_effects: vec![
            effect(20),
            effect(30),
            GuiInputEffect {
                session: 7,
                source_tick: 11,
                effect_tick: 12,
                kind: GuiInputEffectKind::FocusChanged {
                    focus: None,
                },
            },
        ],
        gui_input_conflicts: vec![GuiInputConflict {
            session: 7,
            source_tick: 11,
            effect_tick: 12,
            target: None,
            reason: GuiInputConflictReason::TouchArbitration {
                owner_pointer: 3,
            },
        }],
        gui_input_cancellations: vec![GuiInputCancellation {
            session: 7,
            source_tick: 11,
            effect_tick: 12,
            target: None,
            reason: GuiInputCancelReason::SessionReplaced,
        }],
        gui_unhandled_inputs: vec![
            GuiUnhandledInput {
                session: 7,
                source_request_id: 71,
                tick: 11,
                input: GuiInputCommand::Blur,
                reason: GuiUnhandledReason::NoFocus,
            },
            GuiUnhandledInput {
                session: 8,
                source_request_id: 72,
                tick: 11,
                input: GuiInputCommand::Blur,
                reason: GuiUnhandledReason::NotOwner,
            },
        ],
        ..Default::default()
    };
    let bodies = gui_observation_bodies(&report, 7);
    assert_eq!(bodies.len(), 4);
    let ResponseBody::GuiObservations {
        effects,
        conflicts,
        cancellations,
        ..
    } = &bodies[0]
    else {
        panic!("effects expected")
    };
    assert_eq!(effects.len(), 2);
    assert!(conflicts.is_empty() && cancellations.is_empty());
    let ResponseBody::GuiUnhandledInputs {
        inputs,
    } = bodies.last().unwrap()
    else {
        panic!("unhandled expected")
    };
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].session, 7);
    for body in &bodies {
        assert!(
            encode_response(&Response {
                session: 7,
                request_id: 0,
                tick: 12,
                body: body.clone(),
            })
            .is_ok()
        );
    }
    let foreign = gui_observation_bodies(&report, 8);
    let ResponseBody::GuiUnhandledInputs {
        inputs,
    } = foreign.last().unwrap()
    else {
        panic!("unhandled expected")
    };
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].session, 8);

    let report = WorldUpdateReport {
        gui_input_effects: (0u32..130).map(|node| effect(node + 1)).collect(),
        ..Default::default()
    };
    let bodies = gui_observation_bodies(&report, 7);
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        assert!(
            encode_response(&Response {
                session: 7,
                request_id: 0,
                tick: 12,
                body: body.clone(),
            })
            .is_ok()
        );
    }
    assert!(
        encode_response(&Response {
            session: 7,
            request_id: 0,
            tick: 12,
            body: ResponseBody::GuiObservations {
                effects: (0u32..129).map(|node| effect(node + 1)).collect(),
                conflicts: Vec::new(),
                cancellations: Vec::new(),
                text_focus_updates: Vec::new(),
            },
        })
        .is_err()
    );
}

#[cfg(feature = "gui")]
#[test]
fn gui_edit_and_patch_carry_enabled_lane() {
    use ipp_core::{EntityId, GuiCommand};
    let frame = |payload: &[u8]| {
        let mut bytes = 7u64.to_le_bytes().to_vec();
        bytes.extend_from_slice(&9u64.to_le_bytes());
        bytes.push(REQUEST_GUI);
        bytes.push(0);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    };
    let batch = |command: Vec<u8>| {
        let mut payload = vec![2];
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&command);
        payload
    };
    // Insert style: mask-carried lanes only, so every other lane is absent.
    let style = |mask: u16, enabled: &[u8]| {
        let mut payload = vec![1];
        payload.extend_from_slice(&EntityId::from_bits(42).to_bits().to_le_bytes());
        payload.extend_from_slice(&3u64.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&[1, 5]);
        payload.extend_from_slice(&mask.to_le_bytes());
        for _ in 0..4 {
            payload.extend_from_slice(&1.0f32.to_le_bytes());
        }
        payload.extend_from_slice(&1.0f32.to_le_bytes());
        payload.extend_from_slice(&0.1f32.to_le_bytes());
        payload.extend_from_slice(enabled);
        payload
    };
    match &decode_request(&frame(&batch(style(1 << 13, &[0]))), 7)
        .unwrap()
        .body
    {
        RequestBody::GuiCommands {
            batch_id: None,
            commands,
        } => match &commands[0] {
            GuiCommand::InsertNode {
                style,
                ..
            } => assert!(!style.enabled),
            other => panic!("expected insert, got {other:?}"),
        },
        other => panic!("expected gui command, got {other:?}"),
    }
    match &decode_request(&frame(&batch(style(0, &[]))), 7)
        .unwrap()
        .body
    {
        RequestBody::GuiCommands {
            batch_id: None,
            commands,
        } => match &commands[0] {
            GuiCommand::InsertNode {
                style,
                ..
            } => assert!(style.enabled),
            other => panic!("expected insert, got {other:?}"),
        },
        other => panic!("expected gui command, got {other:?}"),
    }

    // Update patch: every lane tag precedes the trailing enabled tag.
    let patch = |enabled: &[u8]| {
        let mut payload = vec![2];
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&EntityId::from_bits(42).to_bits().to_le_bytes());
        payload.extend_from_slice(&3u64.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.push(0);
        payload.push(1);
        payload.extend_from_slice(&[0; 16]);
        payload.extend_from_slice(enabled);
        payload
    };
    match &decode_request(&frame(&batch(patch(&[2, 0]))), 7)
        .unwrap()
        .body
    {
        RequestBody::GuiCommands {
            batch_id: None,
            commands,
        } => match &commands[0] {
            GuiCommand::UpdateNode {
                patch,
                ..
            } => assert_eq!(patch.enabled, Some(false)),
            other => panic!("expected update, got {other:?}"),
        },
        other => panic!("expected gui command, got {other:?}"),
    }
    match &decode_request(&frame(&batch(patch(&[0]))), 7).unwrap().body {
        RequestBody::GuiCommands {
            batch_id: None,
            commands,
        } => match &commands[0] {
            GuiCommand::UpdateNode {
                patch,
                ..
            } => assert_eq!(patch.enabled, None),
            other => panic!("expected update, got {other:?}"),
        },
        other => panic!("expected gui command, got {other:?}"),
    }
}
