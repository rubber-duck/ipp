use super::*;
use crate::attachment_receipts::{
    AttachmentEffect, AttachmentEffectKind, AttachmentReceipt, BatchOperationEffect,
    ReceiptBatchOutcome,
};
use ipp_core::Batch;

pub(super) fn requests(covered: &mut BTreeSet<&'static str>) {
    for release in [false, true] {
        let bytes = encode_manifest_fixture(
            &ManifestFixture::new(
                "request-attachment-receipt",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(1)),
                    ("tag", ManifestValue::Tag("REQUEST_ATTACHMENT_RECEIPT")),
                    ("receipt", ManifestValue::U64(91)),
                    ("release", ManifestValue::Bool(release)),
                ],
            ),
            covered,
        );
        assert_eq!(
            decode_request(&bytes, 7).unwrap().body,
            RequestBody::AttachmentReceipt {
                receipt: 91,
                release
            }
        );
        for end in 0..bytes.len() {
            assert!(decode_request(&bytes[..end], 7).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_request(&trailing, 7).is_err());
    }
    let bytes = encode_manifest_fixture(
        &ManifestFixture::new(
            "request-submit-batch",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tag", ManifestValue::Tag("REQUEST_SUBMIT_BATCH")),
                ("batch_id", ManifestValue::U32(3)),
                ("last", ManifestValue::Bool(true)),
                (
                    "operations",
                    ManifestValue::List(vec![manifest_command(
                        "command-detach-attachment-receipt",
                        "COMMAND_DETACH_ATTACHMENT_RECEIPT",
                        [("receipt", ManifestValue::U64(91))],
                    )]),
                ),
            ],
        ),
        covered,
    );
    assert_eq!(
        decode_request(&bytes, 7).unwrap().body,
        RequestBody::SubmitBatch(crate::BatchPage {
            batch_id: 3,
            last: true,
            operations: vec![Command::DetachWorldAttachmentReceipt {
                receipt: 91
            }]
        })
    );
    for end in 0..bytes.len() {
        assert!(decode_request(&bytes[..end], 7).is_err());
    }
}

fn reference(world: ipp_core::WorldRef) -> ManifestValue {
    manifest_layout(
        "world-reference",
        [
            ("id", ManifestValue::U64(world.id().0)),
            ("incarnation", ManifestValue::U64(world.incarnation())),
        ],
    )
}

pub(super) fn responses(covered: &mut BTreeSet<&'static str>) {
    for (retired, state) in [
        (None, "RECEIPT_RELEASED"),
        (Some(false), "RECEIPT_PENDING"),
        (Some(true), "RECEIPT_RETIRED"),
    ] {
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 1,
                tick: 2,
                body: ResponseBody::AttachmentReceipt {
                    receipt: 91,
                    retired,
                },
            },
            ManifestFixture::new(
                "response-attachment-receipt",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(1)),
                    ("tick", ManifestValue::U64(2)),
                    ("tag", ManifestValue::Tag("RESPONSE_ATTACHMENT_RECEIPT")),
                    ("receipt", ManifestValue::U64(91)),
                    ("state", ManifestValue::Tag(state)),
                ],
            ),
            covered,
        );
    }
    let mut host = ipp_core::HostRuntime::new();
    let parent = host
        .create_world(
            Default::default(),
            &[ipp_core::systems::world_attachment::WorldAttachmentSystem::ID],
        )
        .unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let child = host.world_ref(child).unwrap();
    let parent = host.world_ref(parent).unwrap();
    let operations = vec![
        Command::Create {
            alias: 0,
            metadata: Default::default(),
            adopt: false,
        },
        Command::insert_value(
            ipp_core::EntityRef::Alias(0),
            ComponentValue::WorldAttachment(ipp_core::WorldAttachment {
                child: Some(child),
                ..Default::default()
            }),
        ),
    ];
    host.world_mut(parent.id())
        .unwrap()
        .enqueue(Batch {
            id: 3,
            operations,
        })
        .unwrap();

    let mut applied = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&parent.id())
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0);
    applied.result.as_ref().unwrap();

    let ipp_core::OperationEffect::WorldAttachment(ipp_core::WorldAttachmentEffect::Written(token)) =
        &applied.effects[0].effect
    else {
        panic!("written receipt");
    };
    let token = token.clone();
    let receipt = AttachmentReceipt {
        id: 91,
        parent: parent.into(),
        anchor: token.anchor().to_bits(),
        incarnation: token.incarnation(),
        revision: token.identity().1,
        child: Some(child.into()),
    };
    applied.result = Ok(vec![]);
    for (kind, tag, effect) in [
        (
            AttachmentEffectKind::Written,
            "ATTACHMENT_WRITTEN",
            ipp_core::WorldAttachmentEffect::Written(token.clone()),
        ),
        (
            AttachmentEffectKind::Detached,
            "ATTACHMENT_DETACHED",
            ipp_core::WorldAttachmentEffect::Detached(token.clone()),
        ),
        (
            AttachmentEffectKind::Superseded,
            "ATTACHMENT_SUPERSEDED",
            ipp_core::WorldAttachmentEffect::Superseded(token.clone()),
        ),
    ] {
        applied.effects = vec![ipp_core::AppliedOperationEffect {
            operation: 1,
            effect: ipp_core::OperationEffect::WorldAttachment(effect),
        }];
        let fixture = ManifestFixture::new(
            "response-batch",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(1)),
                ("tick", ManifestValue::U64(applied.tick)),
                ("tag", ManifestValue::Tag("RESPONSE_BATCH")),
                (
                    "outcome",
                    manifest_layout(
                        "outcome-success",
                        [
                            ("batch_id", ManifestValue::U64(3)),
                            ("tick", ManifestValue::U64(applied.tick)),
                            ("tag", ManifestValue::Tag("OUTCOME_SUCCESS")),
                            ("aliases", ManifestValue::List(vec![])),
                            ("symbols", ManifestValue::List(vec![])),
                        ],
                    ),
                ),
                (
                    "effects",
                    ManifestValue::List(vec![manifest_layout(
                        "applied-operation-effect",
                        [
                            ("operation", ManifestValue::U32(1)),
                            (
                                "effect",
                                manifest_layout(
                                    "attachment-effect",
                                    [
                                        ("tag", ManifestValue::Tag(tag)),
                                        ("receipt", ManifestValue::U64(91)),
                                        ("parent", reference(parent)),
                                        ("anchor", ManifestValue::U64(receipt.anchor)),
                                        ("incarnation", ManifestValue::U64(receipt.incarnation)),
                                        ("revision", ManifestValue::U64(receipt.revision)),
                                        ("child", ManifestValue::Some(Box::new(reference(child)))),
                                    ],
                                ),
                            ),
                        ],
                    )]),
                ),
            ],
        );
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 1,
                tick: applied.tick,
                body: ResponseBody::Batch(ReceiptBatchOutcome {
                    outcome: applied.clone(),
                    effects: vec![BatchOperationEffect::Attachment(AttachmentEffect {
                        operation: 1,
                        kind,
                        receipt: receipt.clone(),
                    })],
                }),
            },
            fixture,
            covered,
        );
    }

    // An adopting create that found its symbolic id reports adoption at its operation.
    let operations = vec![Command::Create {
        alias: 5,
        metadata: ipp_core::EntityMetadata {
            symbolic_id: Some("adopted".into()),
            classes: Vec::new(),
        },
        adopt: false,
    }];
    host.world_mut(parent.id())
        .unwrap()
        .enqueue(Batch {
            id: 4,
            operations,
        })
        .unwrap();
    host.frame(0.0).unwrap();
    let operations = vec![
        Command::Create {
            alias: 6,
            metadata: Default::default(),
            adopt: false,
        },
        Command::Create {
            alias: 7,
            metadata: ipp_core::EntityMetadata {
                symbolic_id: Some("adopted".into()),
                classes: Vec::new(),
            },
            adopt: true,
        },
    ];
    host.world_mut(parent.id())
        .unwrap()
        .enqueue(Batch {
            id: 5,
            operations,
        })
        .unwrap();
    let adopted = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&parent.id())
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0);
    assert_eq!(
        adopted.effects,
        [ipp_core::AppliedOperationEffect {
            operation: 1,
            effect: ipp_core::OperationEffect::Adopted,
        }]
    );
    let aliases = adopted.result.clone().unwrap();
    let alias = |alias: u32, handle: ipp_core::EntityId| {
        manifest_layout(
            "alias-handle",
            [
                ("alias", ManifestValue::U32(alias)),
                ("handle", ManifestValue::U64(handle.to_bits())),
            ],
        )
    };
    assert_manifest_response(
        Response {
            session: 7,
            request_id: 2,
            tick: adopted.tick,
            body: ResponseBody::Batch(ReceiptBatchOutcome {
                outcome: adopted.clone(),
                effects: vec![BatchOperationEffect::Adopted {
                    operation: 1,
                }],
            }),
        },
        ManifestFixture::new(
            "response-batch",
            [
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(2)),
                ("tick", ManifestValue::U64(adopted.tick)),
                ("tag", ManifestValue::Tag("RESPONSE_BATCH")),
                (
                    "outcome",
                    manifest_layout(
                        "outcome-success",
                        [
                            ("batch_id", ManifestValue::U64(5)),
                            ("tick", ManifestValue::U64(adopted.tick)),
                            ("tag", ManifestValue::Tag("OUTCOME_SUCCESS")),
                            (
                                "aliases",
                                ManifestValue::List(
                                    aliases.iter().map(|(a, h)| alias(*a, *h)).collect(),
                                ),
                            ),
                            ("symbols", ManifestValue::List(vec![])),
                        ],
                    ),
                ),
                (
                    "effects",
                    ManifestValue::List(vec![manifest_layout(
                        "applied-operation-effect",
                        [
                            ("operation", ManifestValue::U32(1)),
                            (
                                "effect",
                                manifest_layout(
                                    "operation-effect-adopted",
                                    [("tag", ManifestValue::Tag("OPERATION_ADOPTED"))],
                                ),
                            ),
                        ],
                    )]),
                ),
            ],
        ),
        covered,
    );
}
