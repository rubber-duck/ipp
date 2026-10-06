use super::*;
use crate::reliable_output::{ReplyReservation, SharedReplyBudget};
use ipp_core::services::reliable_output::{OutputClass, OutputLimits, ReliableOutputAccount};
use ipp_core::{
    Batch, Command, ComponentValue, EntityRef, ErrorReason, HostRuntime, OperationEffect,
    WorldAttachment, WorldAttachmentEffect,
};

const ATTACHMENTS: &[ipp_core::systems::SystemId] =
    &[ipp_core::systems::world_attachment::WorldAttachmentSystem::ID];

fn budget_with_payload_capacity(bytes: usize) -> SharedReplyBudget {
    SharedReplyBudget(ReliableOutputAccount::new(OutputLimits {
        bytes: bytes + crate::reliable_output::RESPONSE_METADATA_BYTES,
        reply_reserve: 0,
    }))
}

fn reservation(budget: SharedReplyBudget) -> Rc<RefCell<ReplyReservation>> {
    Rc::new(RefCell::new(
        ReplyReservation::new(budget, OutputClass::Reply, 0).unwrap(),
    ))
}

fn create(alias: u32) -> Command {
    Command::Create {
        alias,
        metadata: Default::default(),
        adopt: false,
    }
}

fn receipt_effect(
    effect: &BatchOperationEffect,
) -> &ipp_protocol::world::attachment_receipts::AttachmentEffect {
    let BatchOperationEffect::Attachment(effect) = effect else {
        panic!("attachment effect expected, found {effect:?}");
    };
    effect
}

fn attachment(alias: u32) -> Command {
    Command::insert_value(
        EntityRef::Alias(alias),
        ComponentValue::WorldAttachment(WorldAttachment::default()),
    )
}

fn apply(
    host: &mut HostRuntime,
    world: ipp_core::WorldId,
    operations: Vec<Command>,
    registry: SharedReceipts,
    budget: SharedReplyBudget,
) -> (ipp_core::BatchOutcome, Rc<RefCell<ReplyReservation>>) {
    let reservation = reservation(budget);
    let aliases = operations
        .iter()
        .filter(|command| defines_alias(command))
        .count();
    let adoptions = operations
        .iter()
        .filter(|command| may_adopt(command))
        .count();
    let (sink, reservation) = ReceiptSink::new(
        registry,
        reservation,
        aliases,
        adoptions,
        &Default::default(),
    )
    .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_with_effect_sink(
            Batch {
                id: 1,
                operations,
            },
            Box::new(sink),
        )
        .unwrap();

    let outcome = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0);
    (outcome, reservation)
}

#[test]
fn registry_and_reply_capacity_fail_at_the_operation_with_receipt_prefix_intact() {
    for registry_capacity in [false, true] {
        let mut host = HostRuntime::new();
        let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
        let registry: SharedReceipts = Default::default();
        let budget = if registry_capacity {
            SharedReplyBudget::default()
        } else {
            budget_with_payload_capacity(
                128 + 2 * (ALIAS_WIRE_BYTES + ALIAS_RETAINED_BYTES) + MAX_ATTACHMENT_EFFECT_BYTES,
            )
        };
        if registry_capacity {
            registry.borrow_mut().limit = 1;
        }
        let (outcome, reservation) = apply(
            &mut host,
            world,
            vec![create(0), attachment(0), create(1), attachment(1)],
            registry.clone(),
            budget.clone(),
        );
        let error = outcome.result.as_ref().unwrap_err();
        assert_eq!(error.operation, Some(3));
        assert_eq!(error.reason, ErrorReason::Capacity);
        assert_eq!(error.aliases.len(), 2);
        assert_eq!(outcome.effects.len(), 1);
        assert_eq!(registry.borrow().tokens.len(), 1);
        let wire = registry.borrow().outcome(outcome).unwrap();
        assert_eq!(receipt_effect(&wire.effects[0]).operation, 1);
        let bytes = ipp_protocol::world::encode_response(&ipp_protocol::world::Response {
            session: 1,
            request_id: 1,
            tick: wire.outcome.tick,
            body: ipp_protocol::world::ResponseBody::Batch(wire),
        })
        .unwrap();
        reservation.borrow_mut().encoded(bytes.capacity());
        assert_eq!(budget.0.usage().entries, 1);
        assert_eq!(
            budget.0.usage().bytes,
            bytes.capacity() + crate::reliable_output::RESPONSE_METADATA_BYTES
        );
        drop(reservation);
        assert_eq!(budget.0.usage().entries, 0);
        assert_eq!(budget.0.usage().bytes, 0);
    }
}

#[test]
fn alias_reply_capacity_rejects_before_the_first_mutation_with_terminal_space_reserved() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let registry: SharedReceipts = Default::default();
    let budget = budget_with_payload_capacity(128);
    let (outcome, reservation) = apply(
        &mut host,
        world,
        vec![create(0)],
        registry.clone(),
        budget.clone(),
    );
    let error = outcome.result.as_ref().unwrap_err();
    assert_eq!(error.operation, Some(0));
    assert_eq!(error.reason, ErrorReason::Capacity);
    assert!(error.aliases.is_empty());
    assert!(host.world_mut(world).unwrap().entities().is_empty());
    let bytes = ipp_protocol::world::encode_response(&ipp_protocol::world::Response {
        session: 1,
        request_id: 1,
        tick: outcome.tick,
        body: ipp_protocol::world::ResponseBody::Batch(registry.borrow().outcome(outcome).unwrap()),
    })
    .unwrap();
    reservation.borrow_mut().encoded(bytes.capacity());
    assert_eq!(budget.0.usage().entries, 1);
    assert_eq!(
        budget.0.usage().bytes,
        bytes.capacity() + crate::reliable_output::RESPONSE_METADATA_BYTES
    );
    drop(reservation);
    assert_eq!(budget.0.usage().entries, 0);
    assert_eq!(budget.0.usage().bytes, 0);
}

#[test]
fn released_and_foreign_receipts_fail_after_prefix_and_same_value_writes_get_new_ids() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let registry: SharedReceipts = Default::default();
    let budget: SharedReplyBudget = Default::default();
    let (outcome, _) = apply(
        &mut host,
        world,
        vec![create(0), attachment(0)],
        registry.clone(),
        budget.clone(),
    );
    let wire = registry.borrow().outcome(outcome).unwrap();
    let first = receipt_effect(&wire.effects[0]).receipt.clone();
    registry.borrow_mut().release(first.id).unwrap();
    for registry in [registry.clone(), SharedReceipts::default()] {
        let (outcome, _) = apply(
            &mut host,
            world,
            vec![
                create(1),
                Command::DetachWorldAttachmentReceipt {
                    receipt: first.id,
                },
            ],
            registry,
            budget.clone(),
        );
        let error = outcome.result.unwrap_err();
        assert_eq!(error.operation, Some(1));
        assert_eq!(error.aliases.len(), 1);
    }
    let command = Command::SetField {
        entity: EntityRef::Handle(ipp_core::EntityId::from_bits(first.anchor)),
        component: ComponentValue::WORLD_ATTACHMENT,
        field: ipp_core::FieldWrite {
            offset: std::mem::offset_of!(WorldAttachment, child) as u32,
            value: ipp_core::FieldValue::World(None),
        },
    };
    let (outcome, _) = apply(&mut host, world, vec![command], registry.clone(), budget);
    outcome.result.as_ref().unwrap();
    let wire = registry.borrow().outcome(outcome).unwrap();
    assert_ne!(receipt_effect(&wire.effects[0]).receipt.id, first.id);
    assert_ne!(
        receipt_effect(&wire.effects[0]).receipt.revision,
        first.revision
    );
}

#[test]
fn closed_session_sink_rejects_queued_zero_effect_operations() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let registry: SharedReceipts = Default::default();
    let budget: SharedReplyBudget = Default::default();
    let reply = reservation(budget.clone());
    let (sink, reservation) =
        ReceiptSink::new(registry.clone(), reply, 1, 0, &Default::default()).unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_with_effect_sink(
            Batch {
                id: 1,
                operations: vec![create(0)],
            },
            Box::new(sink),
        )
        .unwrap();
    registry.borrow_mut().close();
    let report = host.frame(0.0).unwrap();
    let outcome = &report.worlds[&world].as_ref().unwrap().outcomes[0];
    assert_eq!(outcome.result.as_ref().unwrap_err().operation, Some(0));
    assert!(host.world_mut(world).unwrap().entities().is_empty());
    drop(reservation);
    assert_eq!(budget.0.usage().entries, 0);
    assert_eq!(budget.0.usage().bytes, 0);
}

#[test]
fn repeated_existing_effects_deduplicate_registry_slots_and_settle_unused_capacity() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let registry: SharedReceipts = Default::default();
    let budget: SharedReplyBudget = Default::default();
    let (outcome, _) = apply(
        &mut host,
        world,
        vec![create(0), attachment(0)],
        registry.clone(),
        budget.clone(),
    );
    let OperationEffect::WorldAttachment(WorldAttachmentEffect::Written(token)) =
        &outcome.effects[0].effect
    else {
        panic!("receipt");
    };
    registry.borrow_mut().limit = 1;
    let reply = reservation(budget.clone());
    let (mut sink, reservation) =
        ReceiptSink::new(registry.clone(), reply, 1, 0, &Default::default()).unwrap();
    sink.reserve(&OperationEffectDemand {
        fresh_attachment_tokens: 0,
        existing_attachment_tokens: vec![token.clone(), token.clone()],
        max_records: 3,
        adopting: false,
    })
    .unwrap();
    sink.emit(OperationEffect::WorldAttachment(
        WorldAttachmentEffect::Superseded(token.clone()),
    ));
    sink.settle();
    assert_eq!(registry.borrow().tokens.len(), 1);
    assert_eq!(
        reservation.borrow().bytes,
        128 + ALIAS_WIRE_BYTES + MAX_ATTACHMENT_EFFECT_BYTES
    );
    drop(sink);
    drop(reservation);
    assert_eq!(budget.0.usage().entries, 0);
    assert_eq!(budget.0.usage().bytes, 0);
}

#[test]
fn adopting_operations_report_adoption_without_a_receipt() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let registry: SharedReceipts = Default::default();
    let budget: SharedReplyBudget = Default::default();
    let named = |alias, adopt| Command::Create {
        alias,
        metadata: ipp_core::EntityMetadata {
            symbolic_id: Some("adopted".into()),
            classes: Vec::new(),
        },
        adopt,
    };
    let (outcome, _) = apply(
        &mut host,
        world,
        vec![named(0, false)],
        registry.clone(),
        budget.clone(),
    );
    let created = outcome.result.as_ref().unwrap()[0].1;
    assert!(outcome.effects.is_empty());

    // The second batch adopts the live entity and a fresh one it creates.
    let (outcome, reservation) = apply(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: true,
            },
            named(2, true),
        ],
        registry.clone(),
        budget.clone(),
    );
    assert_eq!(
        outcome
            .result
            .as_ref()
            .unwrap()
            .iter()
            .find(|(alias, _)| *alias == 2),
        Some(&(2, created))
    );
    let wire = registry.borrow().outcome(outcome).unwrap();
    assert_eq!(
        wire.effects,
        [BatchOperationEffect::Adopted {
            operation: 1
        }]
    );
    assert!(registry.borrow().tokens.is_empty());
    let bytes = ipp_protocol::world::encode_response(&ipp_protocol::world::Response {
        session: 1,
        request_id: 1,
        tick: wire.outcome.tick,
        body: ipp_protocol::world::ResponseBody::Batch(wire),
    })
    .unwrap();
    reservation.borrow_mut().encoded(bytes.capacity());
    drop(reservation);
    assert_eq!(budget.0.usage().entries, 0);
    assert_eq!(budget.0.usage().bytes, 0);
}
