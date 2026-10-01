use super::*;

fn test_link() -> ipp_core::EntityLink {
    ipp_core::EntityLink {
        parent: None,
        order: ipp_core::EntityOrder::from_value(1).unwrap(),
    }
}

fn request(tag: u8) -> Vec<u8> {
    let mut bytes = 7u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&9u64.to_le_bytes());
    bytes.push(tag);
    if tag == REQUEST_INSPECT {
        bytes.extend_from_slice(&[0; 17]);
        bytes.extend_from_slice(&256u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
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
fn row_property_writes_use_raw_offsets_while_named_properties_stay_rejected() {
    let row = 0x1000_0000u32 + 3 * 9 + 2;
    let mut bytes = row.to_le_bytes().to_vec();
    bytes.push(VALUE_UNSET);
    bytes.extend_from_slice(&row.to_le_bytes());
    bytes.push(VALUE_DYNAMIC);
    let value = ipp_core::DynamicValue::Vec3([1.0, 2.0, 3.0]).encode();
    bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&value);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.push(VALUE_ROWS);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);

    let mut reader = Reader {
        bytes: &bytes,
        at: 0,
    };
    assert_eq!(
        reader.field().unwrap(),
        FieldWrite {
            offset: row,
            value: FieldValue::Unset,
        }
    );
    assert_eq!(
        reader.field().unwrap(),
        FieldWrite {
            offset: row,
            value: FieldValue::Dynamic(ipp_core::DynamicValue::Vec3([1.0, 2.0, 3.0])),
        }
    );
    assert_eq!(
        reader.field().unwrap(),
        FieldWrite {
            offset: 8,
            value: FieldValue::Rows(vec![0; 8]),
        }
    );
    assert_eq!(reader.at, bytes.len());

    let mut dynamic = 0x8000_0001u32.to_le_bytes().to_vec();
    dynamic.push(VALUE_UNSET);
    assert!(
        Reader {
            bytes: &dynamic,
            at: 0,
        }
        .field()
        .is_err()
    );
}

#[test]
fn inspected_rows_tables_encode_once_with_their_kind_tag() {
    let table = vec![1, 0, 0, 0, 0, 0, 0, 0];
    let mut writer = Writer::new(Vec::new());
    writer
        .resolved_field(24, ResolvedValue::Rows(table.clone()))
        .unwrap();
    let mut expected = 24u32.to_le_bytes().to_vec();
    expected.push(SNAPSHOT_VALUE_ROWS);
    expected.extend_from_slice(&(table.len() as u32).to_le_bytes());
    expected.extend_from_slice(&table);
    assert_eq!(writer.0, expected);

    let mut writer = Writer::new(Vec::new());
    writer
        .resolved_field(0x1000_0000, ResolvedValue::Unset)
        .unwrap();
    let mut expected = 0x1000_0000u32.to_le_bytes().to_vec();
    expected.push(SNAPSHOT_VALUE_UNSET);
    assert_eq!(writer.0, expected);
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
        FieldValue::String("".into())
    );
}

#[test]
fn resource_events_are_ordered_bounded_and_unsolicited() {
    use ipp_core::{AssetResourceSnapshot, AssetResourceStatus};
    let resource = AssetResourceSnapshot {
        representation: Default::default(),
        id: 1,
        kind: ipp_core::services::asset_management::AssetTypeId(1),
        source: "x".repeat(4097).into(),
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
            source: "x".repeat(65537).into(),
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
            #[cfg(feature = "gui")]
            gui_focus: Vec::new(),
            #[cfg(feature = "gui")]
            gui_pointers: Vec::new(),
            #[cfg(feature = "surfaces")]
            canvas: None,
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
    // GUI builds append the empty focus and pointer collections.
    #[cfg(feature = "gui")]
    for _ in 0..2 {
        assert_eq!(reader.u32().unwrap(), 0);
    }
    // Surface builds append the absent Canvas System record.
    #[cfg(feature = "surfaces")]
    assert_eq!(reader.u8().unwrap(), 0);
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
            #[cfg(feature = "gui")]
            gui_focus: Vec::new(),
            #[cfg(feature = "gui")]
            gui_pointers: Vec::new(),
            #[cfg(feature = "surfaces")]
            canvas: None,
            next: 0,
            controllers: Vec::new(),
            time: 0.5,
            entities: vec![ipp_core::EntitySnapshot {
                id: EntityId::from_bits(42),
                metadata: EntityMetadata::default(),
                link: test_link(),
                components: vec![ComponentValue::CustomMaterial(material)],
            }],
            resources: Vec::new(),
            render_diagnostics: Vec::new(),
        },
    })
    .unwrap();

    // Response envelope, inspection header, entity identity/metadata, link and
    // the component count precede this component's u16 type and u32 count.
    assert_eq!(
        u32::from_le_bytes(bytes[88..92].try_into().unwrap()) as usize,
        field_count
    );
    assert!(bytes.len() < MAX_MESSAGE_BYTES);
}

#[test]
fn large_inspected_byte_fields_still_obey_the_message_budget() {
    let mut writer = Writer::new(Vec::new());
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
fn geometry_results_enforce_request_and_tick_correlation() {
    let mut response = Response {
        session: 7,
        request_id: 9,
        tick: 11,
        body: ResponseBody::GeometryPickResultEvent(crate::views::ViewQueryOutcome {
            request_id: 9,
            tick: 11,
            result: Err(ipp_core::ErrorReason::InvalidEntity),
        }),
    };
    assert!(encode_response(&response).is_ok());
    response.request_id = 0;
    assert!(encode_response(&response).is_err());
    response.request_id = 10;
    assert_eq!(
        encode_response(&response),
        Err(ProtocolError::Malformed("geometry outcome correlation"))
    );
    response.request_id = 9;
    response.tick = 12;
    assert!(encode_response(&response).is_err());
}

fn root_view_bytes() -> Vec<u8> {
    let mut writer = Writer::new(Vec::new());
    writer.u8(VIEW_ROOT).unwrap();
    writer
        .output_reference(crate::references::OutputReference {
            world: crate::references::WorldReference {
                id: 1,
                incarnation: 2,
            },
            target: crate::references::OutputTarget::Camera {
                entity: 3,
                incarnation: 4,
            },
        })
        .unwrap();
    writer.u32(640).unwrap();
    writer.u32(480).unwrap();
    writer.f64(1.0).unwrap();
    writer.0
}

#[test]
fn obsolete_implicit_camera_controls_are_not_protocol_operations() {
    for tag in [8, 11] {
        let mut bytes = request(tag);
        bytes[8..16].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            decode_request(&bytes, 7),
            Err(ProtocolError::Unsupported(tag))
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
    bytes.extend(root_view_bytes());
    bytes.extend_from_slice(&0.5f32.to_le_bytes());
    bytes.extend_from_slice(&0.5f32.to_le_bytes());
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
fn tree_query_bounds_and_cursor_form_are_enforced() {
    let mut bytes = request(REQUEST_INSPECT);
    bytes[17] = INSPECT_ENTITY_TREE;
    bytes[18..26].copy_from_slice(&42u64.to_le_bytes());
    bytes[26..34].copy_from_slice(&41u64.to_le_bytes());
    bytes[36..38].copy_from_slice(&64u16.to_le_bytes());
    assert!(matches!(
        decode_request(&bytes, 7).unwrap().body,
        RequestBody::Inspect(_)
    ));
    bytes[36..38].copy_from_slice(&65u16.to_le_bytes());
    assert!(decode_request(&bytes, 7).is_err());
    bytes[36..38].copy_from_slice(&0u16.to_le_bytes());
    bytes[17] = INSPECT_ENTITIES;
    assert!(decode_request(&bytes, 7).is_err());
    bytes[17] = INSPECT_ENTITY_TREE;
    for end in 0..bytes.len() {
        assert!(decode_request(&bytes[..end], 7).is_err());
    }
}

#[test]
fn bound_counts_before_allocating() {
    let mut b = request(REQUEST_SUBMIT_BATCH);
    b.extend_from_slice(&1u32.to_le_bytes());
    b.push(1);
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
            #[cfg(feature = "gui")]
            gui_focus: Vec::new(),
            #[cfg(feature = "gui")]
            gui_pointers: Vec::new(),
            #[cfg(feature = "surfaces")]
            canvas: None,
            next: 0,
            controllers: Vec::new(),
            time: 0.0,
            entities: vec![],
            resources: vec![],
            render_diagnostics: vec![],
        },
        ResponseBody::Batch(crate::attachment_receipts::ReceiptBatchOutcome {
            outcome: BatchOutcome {
                batch_id: 1,
                tick: 11,
                result: Ok(vec![]),
                symbols: vec![],
                effects: vec![],
            },
            effects: vec![],
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
                    #[cfg(feature = "gui")]
                    gui_focus: Vec::new(),
                    #[cfg(feature = "gui")]
                    gui_pointers: Vec::new(),
                    #[cfg(feature = "surfaces")]
                    canvas: None,
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
    let mut b = request(REQUEST_SUBMIT_BATCH);
    b.extend_from_slice(&1u32.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&1u32.to_le_bytes());
    b.push(COMMAND_CREATE);
    b.extend_from_slice(&4u32.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&3u32.to_le_bytes());
    b.extend_from_slice(b"abc");
    b.extend_from_slice(&0u32.to_le_bytes());
    b.push(0);
    let decoded = decode_request(&b, 7).unwrap();
    b.fill(0);
    let RequestBody::SubmitBatch(batch) = decoded.body else {
        panic!()
    };
    assert_eq!(
        batch.operations,
        vec![Command::Create {
            alias: 4,
            metadata: EntityMetadata {
                symbolic_id: Some("abc".into()),
                classes: vec![]
            },
            adopt: false,
        }]
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
        let mut bytes = request(REQUEST_SUBMIT_BATCH);
        bytes.extend(1u32.to_le_bytes());
        bytes.push(1);
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
        let RequestBody::SubmitBatch(decoded) = decoded.body else {
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
        decode_request_with_buffer(
            &batch(crate::COMMAND_PAGE_COMMANDS as u32 + 1),
            7,
            &mut buffer
        ),
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
    let RequestBody::SubmitBatch(decoded) = decode_request_with_buffer(&batch(1), 7, &mut buffer)
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

#[test]
fn batch_pages_carry_client_identity_and_an_exact_completion_flag() {
    let page = |last: u8| {
        let mut bytes = request(REQUEST_SUBMIT_BATCH);
        if last == 0 {
            // Only the final page is correlated.
            bytes[8..16].fill(0);
        }
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.push(last);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    };
    for (flag, last) in [(0, false), (1, true)] {
        let mut miscorrelated = page(flag);
        miscorrelated[8] ^= 9;
        assert_eq!(
            decode_request(&miscorrelated, 7),
            Err(ProtocolError::Malformed("reserved request identity"))
        );
        assert_eq!(
            decode_request(&page(flag), 7).unwrap().body,
            RequestBody::SubmitBatch(crate::BatchPage {
                batch_id: u32::MAX,
                last,
                operations: vec![],
            })
        );
    }
    assert_eq!(
        decode_request(&page(2), 7),
        Err(ProtocolError::Malformed("batch page completion flag"))
    );

    let mut oversized = page(1);
    oversized.resize(crate::COMMAND_PAGE_BYTES + 1, 0);
    assert_eq!(
        decode_request(&oversized, 7),
        Err(ProtocolError::Limit("command page bytes"))
    );
}

#[test]
fn malformed_page_content_belongs_to_its_batch_and_a_malformed_header_to_the_connection() {
    let page = |last: u8, commands: &[&[u8]]| {
        let mut bytes = request(REQUEST_SUBMIT_BATCH);
        if last == 0 {
            bytes[8..16].fill(0);
        }
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.push(last);
        bytes.extend_from_slice(&(commands.len() as u32).to_le_bytes());
        for command in commands {
            bytes.extend_from_slice(command);
        }
        bytes
    };
    let rejected = |last: bool, error| {
        Err(RequestDecodeError::BatchPage(RejectedBatchPage {
            session: 7,
            request_id: if last {
                9
            } else {
                0
            },
            batch_id: 5,
            last,
            error,
        }))
    };
    let delete: &[u8] = &[COMMAND_DELETE, REF_ALIAS, 1, 0, 0, 0];
    let unknown: &[u8] = &[250];

    // Any session is accepted without Host state; the Host fences it afterwards.
    let decoded = decode_world_request(&page(0, &[delete]), None, &mut Vec::new()).unwrap();
    assert_eq!(decoded.session, 7);
    for (flag, last) in [(0, false), (1, true)] {
        assert_eq!(
            decode_world_request(&page(flag, &[delete, unknown]), None, &mut Vec::new()),
            rejected(last, ProtocolError::Unsupported(250))
        );
        let mut trailing = page(flag, &[delete]);
        trailing.push(0);
        assert_eq!(
            decode_world_request(&trailing, None, &mut Vec::new()),
            rejected(last, ProtocolError::Malformed("trailing bytes"))
        );
        let mut oversized = page(flag, &[]);
        oversized.resize(crate::COMMAND_PAGE_BYTES + 1, 0);
        assert_eq!(
            decode_world_request(&oversized, None, &mut Vec::new()),
            rejected(last, ProtocolError::Limit("command page bytes"))
        );
    }

    // A page that cannot name its batch fails its connection.
    let mut miscorrelated = page(0, &[delete]);
    miscorrelated[8] = 1;
    assert_eq!(
        decode_world_request(&miscorrelated, None, &mut Vec::new()),
        Err(RequestDecodeError::Request(ProtocolError::Malformed(
            "reserved request identity"
        )))
    );
    assert_eq!(
        decode_world_request(&page(0, &[delete]), Some(8), &mut Vec::new()),
        Err(RequestDecodeError::Request(ProtocolError::SessionMismatch))
    );
}
