use super::*;
use crate::world::lifecycle_watch::{self as wire, LifecycleWatchChange, LifecycleWatchRequest};
use ipp_core::systems::lifecycle_publisher::*;

fn world(id: u64, incarnation: u64) -> ManifestValue {
    manifest_layout(
        "world-reference",
        [
            ("id", ManifestValue::U64(id)),
            ("incarnation", ManifestValue::U64(incarnation)),
        ],
    )
}

pub(super) fn requests(covered: &mut BTreeSet<&'static str>) {
    for (bits, kind) in [
        (1, "LIFECYCLE_WATCH_ENTITY_CREATED"),
        (2, "LIFECYCLE_WATCH_ENTITY_METADATA_CHANGED"),
        (4, "LIFECYCLE_WATCH_ENTITY_DELETED"),
        (8, "LIFECYCLE_WATCH_COMPONENT_INSERTED"),
        (16, "LIFECYCLE_WATCH_COMPONENT_UPDATED"),
        (32, "LIFECYCLE_WATCH_COMPONENT_REPLACED"),
        (64, "LIFECYCLE_WATCH_COMPONENT_REMOVED"),
    ] {
        let (target, layout, tag) = if bits < 8 {
            (
                LifecycleWatchTarget::Entity(EntityId::from_bits(99)),
                "lifecycle-watch-entity",
                "LIFECYCLE_WATCH_ENTITY",
            )
        } else {
            (
                LifecycleWatchTarget::Component(EntityId::from_bits(99), 1),
                "lifecycle-watch-component",
                "LIFECYCLE_WATCH_COMPONENT",
            )
        };
        let mut fields = vec![
            ("tag", ManifestValue::Tag(tag)),
            ("entity", ManifestValue::U64(99)),
        ];
        if matches!(target, LifecycleWatchTarget::Component(..)) {
            fields.push(("component", ManifestValue::U16(1)));
        }
        let fixture = ManifestFixture::new(
            "request-lifecycle-watch",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(2)),
                ("tag", ManifestValue::Tag("REQUEST_LIFECYCLE_WATCH")),
                ("world", world(3, 5)),
                (
                    "change",
                    manifest_layout(
                        "lifecycle-watch-add",
                        [
                            ("tag", ManifestValue::Tag("LIFECYCLE_WATCH_ADD")),
                            (
                                "members",
                                ManifestValue::List(vec![manifest_layout(
                                    "lifecycle-watch-selection",
                                    [
                                        ("target", manifest_layout(layout, fields)),
                                        ("kinds", ManifestValue::Tag(kind)),
                                    ],
                                )]),
                            ),
                        ],
                    ),
                ),
            ],
        );
        let mut bytes = encode_manifest_fixture(&fixture, covered);
        let decoded = decode_request(&bytes, 7).unwrap();
        assert_eq!(
            decoded.body,
            RequestBody::LifecycleWatch(LifecycleWatchRequest {
                world: crate::references::WorldReference {
                    id: 3,
                    incarnation: 5
                },
                change: LifecycleWatchChange::Add(vec![(
                    target,
                    LifecycleWatchKinds::from_bits(bits).unwrap()
                )]),
            })
        );
        for length in 0..bytes.len() {
            assert!(decode_request(&bytes[..length], 7).is_err());
        }
        bytes.push(0);
        assert!(decode_request(&bytes, 7).is_err());
    }
    value_request(covered);
    let fixture = ManifestFixture::new(
        "request-lifecycle-watch",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(3)),
            ("tag", ManifestValue::Tag("REQUEST_LIFECYCLE_WATCH")),
            ("world", world(3, 5)),
            (
                "change",
                manifest_layout(
                    "lifecycle-watch-remove",
                    [
                        ("tag", ManifestValue::Tag("LIFECYCLE_WATCH_REMOVE")),
                        ("output", ManifestValue::U64(9)),
                        (
                            "generations",
                            ManifestValue::List(vec![ManifestValue::U64(2), ManifestValue::U64(6)]),
                        ),
                    ],
                ),
            ),
        ],
    );
    let mut bytes = encode_manifest_fixture(&fixture, covered);
    assert_eq!(
        decode_request(&bytes, 7).unwrap().body,
        RequestBody::LifecycleWatch(LifecycleWatchRequest {
            world: crate::references::WorldReference {
                id: 3,
                incarnation: 5
            },
            change: LifecycleWatchChange::Remove {
                output: 9,
                generations: vec![2, 6]
            },
        })
    );
    let end = bytes.len();
    bytes[end - 8..].copy_from_slice(&2u64.to_le_bytes());
    assert!(decode_request(&bytes, 7).is_err());
}

/// A value target names ascending schema field offsets and selects only value changes.
fn value_request(covered: &mut BTreeSet<&'static str>) {
    let add = |fields: &[u32], kinds: &'static str| {
        ManifestFixture::new(
            "request-lifecycle-watch",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(4)),
                ("tag", ManifestValue::Tag("REQUEST_LIFECYCLE_WATCH")),
                ("world", world(3, 5)),
                (
                    "change",
                    manifest_layout(
                        "lifecycle-watch-add",
                        [
                            ("tag", ManifestValue::Tag("LIFECYCLE_WATCH_ADD")),
                            (
                                "members",
                                ManifestValue::List(vec![manifest_layout(
                                    "lifecycle-watch-selection",
                                    [
                                        (
                                            "target",
                                            manifest_layout(
                                                "lifecycle-watch-value",
                                                [
                                                    (
                                                        "tag",
                                                        ManifestValue::Tag("LIFECYCLE_WATCH_VALUE"),
                                                    ),
                                                    ("entity", ManifestValue::U64(99)),
                                                    ("component", ManifestValue::U16(1)),
                                                    (
                                                        "fields",
                                                        ManifestValue::List(
                                                            fields
                                                                .iter()
                                                                .map(|offset| {
                                                                    ManifestValue::U32(*offset)
                                                                })
                                                                .collect(),
                                                        ),
                                                    ),
                                                ],
                                            ),
                                        ),
                                        ("kinds", ManifestValue::Tag(kinds)),
                                    ],
                                )]),
                            ),
                        ],
                    ),
                ),
            ],
        )
    };
    let bytes = encode_manifest_fixture(&add(&[0, 12], "LIFECYCLE_WATCH_VALUE_CHANGED"), covered);
    assert_eq!(
        decode_request(&bytes, 7).unwrap().body,
        RequestBody::LifecycleWatch(LifecycleWatchRequest {
            world: crate::references::WorldReference {
                id: 3,
                incarnation: 5
            },
            change: LifecycleWatchChange::Add(vec![(
                LifecycleWatchTarget::Value(EntityId::from_bits(99), 1, vec![0, 12].into()),
                LifecycleWatchKinds::VALUE_CHANGED,
            )]),
        })
    );
    for length in 0..bytes.len() {
        assert!(decode_request(&bytes[..length], 7).is_err());
    }
    // Other kinds, empty, repeated and descending offsets are malformed.
    let mut rejected = BTreeSet::new();
    for (fields, kinds) in [
        (&[0, 12][..], "LIFECYCLE_WATCH_COMPONENT_UPDATED"),
        (&[], "LIFECYCLE_WATCH_VALUE_CHANGED"),
        (&[12, 12], "LIFECYCLE_WATCH_VALUE_CHANGED"),
        (&[12, 0], "LIFECYCLE_WATCH_VALUE_CHANGED"),
    ] {
        let bytes = encode_manifest_fixture(&add(fields, kinds), &mut rejected);
        assert!(decode_request(&bytes, 7).is_err(), "{fields:?} {kinds}");
    }
}

fn record(body: LifecycleWatchRecordBody) -> LifecycleWatchRecord {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(Default::default(), &[]).unwrap();
    LifecycleWatchRecord {
        world: host.world_ref(id).unwrap(),
        session: 7,
        body,
    }
}

fn assert_record(
    record: LifecycleWatchRecord,
    expected: ManifestValue,
    request: u64,
    covered: &mut BTreeSet<&'static str>,
) {
    let fixture = ManifestFixture::new(
        "response-lifecycle-watch",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(request)),
            ("tick", ManifestValue::U64(0)),
            ("tag", ManifestValue::Tag("RESPONSE_LIFECYCLE_WATCH")),
            (
                "world",
                world(record.world.id().0, record.world.incarnation()),
            ),
            ("output", ManifestValue::U64(9)),
            ("record", expected),
        ],
    );
    let expected = encode_manifest_fixture(&fixture, covered);
    let size = wire::encoded_size(9, &record).unwrap();
    assert_eq!(size, expected.len());
    let mut bytes = Vec::with_capacity(size);
    wire::encode_into(9, &record, &mut bytes).unwrap();
    assert_eq!(bytes, expected);
    assert!(wire::encode_into(9, &record, &mut Vec::new()).is_err());
}

pub(super) fn responses(covered: &mut BTreeSet<&'static str>) {
    value_responses(covered);
    assert_record(
        record(LifecycleWatchRecordBody::Acknowledgement {
            request: 2,
            action: LifecycleMembershipAction::Add,
            cut: None,
            result: LifecycleMembershipResult::Cancelled,
        }),
        manifest_layout(
            "lifecycle-watch-ack",
            [
                ("tag", ManifestValue::Tag("LIFECYCLE_WATCH_ACK")),
                ("action", ManifestValue::Tag("LIFECYCLE_WATCH_ADD")),
                ("cut", ManifestValue::None),
                (
                    "result",
                    manifest_layout(
                        "lifecycle-watch-cancelled",
                        [("tag", ManifestValue::Tag("LIFECYCLE_MEMBERSHIP_CANCELLED"))],
                    ),
                ),
            ],
        ),
        2,
        covered,
    );
    assert_record(
        record(LifecycleWatchRecordBody::Event {
            member: LifecycleWatchId {
                output: 9,
                generation: 4,
            },
            sequence: 6,
            tick: 8,
            observation: LifecycleObservation::Component {
                entity: EntityId::from_bits(99),
                component: 1,
                kind: ComponentLifecycleKind::Replaced,
                previous_incarnation: Some(11),
                incarnation: Some(12),
            },
        }),
        manifest_layout(
            "lifecycle-watch-event",
            [
                ("tag", ManifestValue::Tag("LIFECYCLE_WATCH_EVENT")),
                ("generation", ManifestValue::U64(4)),
                ("sequence", ManifestValue::U64(6)),
                ("tick", ManifestValue::U64(8)),
                (
                    "observation",
                    manifest_layout(
                        "lifecycle-component",
                        [
                            ("tag", ManifestValue::Tag("LIFECYCLE_COMPONENT_REPLACED")),
                            ("entity", ManifestValue::U64(99)),
                            ("component", ManifestValue::U16(1)),
                            ("previous_incarnation", ManifestValue::U64(11)),
                            ("incarnation", ManifestValue::U64(12)),
                        ],
                    ),
                ),
            ],
        ),
        0,
        covered,
    );
}

/// A value baseline echoes its whole target; value records carry snapshot fields or absence.
fn value_responses(covered: &mut BTreeSet<&'static str>) {
    use ipp_core::components::schema::FieldValue;

    let target = LifecycleWatchTarget::Value(EntityId::from_bits(99), 1, vec![0, 12].into());
    let applied = record(LifecycleWatchRecordBody::Acknowledgement {
        request: 4,
        action: LifecycleMembershipAction::Add,
        cut: Some((2, 3)),
        result: LifecycleMembershipResult::Applied(vec![LifecycleMembershipBaseline {
            member: LifecycleWatchId {
                output: 9,
                generation: 5,
            },
            target: target.clone(),
            lifetime: LifecycleTargetLifetime::Component {
                entity_live: true,
                incarnation: Some(11),
            },
        }]),
    });
    let ack_bytes = wire::encoded_size(9, &applied).unwrap();
    assert_record(
        applied,
        manifest_layout(
            "lifecycle-watch-ack",
            [
                ("tag", ManifestValue::Tag("LIFECYCLE_WATCH_ACK")),
                ("action", ManifestValue::Tag("LIFECYCLE_WATCH_ADD")),
                (
                    "cut",
                    ManifestValue::Some(Box::new(manifest_layout(
                        "lifecycle-watch-cut",
                        [
                            ("sequence", ManifestValue::U64(2)),
                            ("tick", ManifestValue::U64(3)),
                        ],
                    ))),
                ),
                (
                    "result",
                    manifest_layout(
                        "lifecycle-watch-applied",
                        [
                            ("tag", ManifestValue::Tag("LIFECYCLE_MEMBERSHIP_APPLIED")),
                            (
                                "baselines",
                                ManifestValue::List(vec![manifest_layout(
                                    "lifecycle-watch-baseline",
                                    [
                                        ("generation", ManifestValue::U64(5)),
                                        (
                                            "target",
                                            manifest_layout(
                                                "lifecycle-watch-value",
                                                [
                                                    (
                                                        "tag",
                                                        ManifestValue::Tag("LIFECYCLE_WATCH_VALUE"),
                                                    ),
                                                    ("entity", ManifestValue::U64(99)),
                                                    ("component", ManifestValue::U16(1)),
                                                    (
                                                        "fields",
                                                        ManifestValue::List(vec![
                                                            ManifestValue::U32(0),
                                                            ManifestValue::U32(12),
                                                        ]),
                                                    ),
                                                ],
                                            ),
                                        ),
                                        (
                                            "lifetime",
                                            manifest_layout(
                                                "lifecycle-lifetime-component",
                                                [
                                                    (
                                                        "tag",
                                                        ManifestValue::Tag(
                                                            "LIFECYCLE_LIFETIME_COMPONENT",
                                                        ),
                                                    ),
                                                    ("entity_live", ManifestValue::Bool(true)),
                                                    ("incarnation", ManifestValue::U64(11)),
                                                ],
                                            ),
                                        ),
                                    ],
                                )]),
                            ),
                        ],
                    ),
                ),
            ],
        ),
        4,
        covered,
    );
    // The encoding bound Core charges covers the ACK with its echoed value target.
    let encoding = wire::LIFECYCLE_WATCH_ENCODING;
    assert!(
        ack_bytes <= encoding.acknowledgement_bytes + encoding.baseline_capacity(&target).unwrap()
    );
    assert_eq!(
        encoding.baseline_capacity(&target),
        Some(encoding.baseline_bytes + 2 * encoding.baseline_field_bytes)
    );

    let values = vec![
        (0, FieldValue::F32(1.5)),
        (12, FieldValue::String("label".into())),
    ];
    let value_record = record(LifecycleWatchRecordBody::Value {
        member: LifecycleWatchId {
            output: 9,
            generation: 5,
        },
        tick: 6,
        values: Some(values.clone()),
    });
    let value_bytes = wire::encoded_size(9, &value_record).unwrap();
    assert_record(
        value_record,
        manifest_layout(
            "lifecycle-watch-value-record",
            [
                ("tag", ManifestValue::Tag("LIFECYCLE_WATCH_VALUE_RECORD")),
                ("generation", ManifestValue::U64(5)),
                ("tick", ManifestValue::U64(6)),
                (
                    "values",
                    ManifestValue::Some(Box::new(manifest_layout(
                        "lifecycle-watch-values",
                        [(
                            "fields",
                            ManifestValue::List(vec![
                                manifest_snapshot_field(
                                    0,
                                    manifest_typed_value(
                                        "snapshot-value-f32",
                                        "SNAPSHOT_VALUE_F32",
                                        ManifestValue::F32(1.5),
                                    ),
                                ),
                                manifest_snapshot_field(
                                    12,
                                    manifest_typed_value(
                                        "snapshot-value-string",
                                        "SNAPSHOT_VALUE_STRING",
                                        ManifestValue::String("label".into()),
                                    ),
                                ),
                            ]),
                        )],
                    ))),
                ),
            ],
        ),
        0,
        covered,
    );
    assert!(value_bytes <= encoding.value_capacity(Some(&values)).unwrap());
    assert_record(
        record(LifecycleWatchRecordBody::Value {
            member: LifecycleWatchId {
                output: 9,
                generation: 5,
            },
            tick: 7,
            values: None,
        }),
        manifest_layout(
            "lifecycle-watch-value-record",
            [
                ("tag", ManifestValue::Tag("LIFECYCLE_WATCH_VALUE_RECORD")),
                ("generation", ManifestValue::U64(5)),
                ("tick", ManifestValue::U64(7)),
                ("values", ManifestValue::None),
            ],
        ),
        0,
        covered,
    );
}
