//! Effect capacity rejects before callbacks while preserving the ordered applied prefix.

mod support;

use support::selection::{ATTACHMENTS, CONSTRAINTS, select};

use ipp_core::{
    Batch, BatchOutcome, Command, ComponentValue, EntityPlacementRef, EntityRef, ErrorReason,
    HostRuntime, OperationEffect, OperationEffectDemand, OperationEffectSink, WorldAttachment,
    WorldAttachmentEffect, WorldAttachmentToken, WorldId,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Counts operation callbacks.
struct CallbackObserver(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl ipp_core::systems::SystemFactory for CallbackObserver {
    fn id(&self) -> ipp_core::systems::SystemId {
        ipp_core::systems::SystemId("test.effect-callback-observer")
    }

    fn create(
        &self,
        _: &mut ipp_core::systems::SystemInitContext<'_>,
    ) -> Result<Box<dyn ipp_core::systems::System>, ipp_core::systems::SystemInitError> {
        Ok(Box::new(Self(self.0.clone())))
    }
}

impl ipp_core::systems::System for CallbackObserver {
    fn before_operation(
        &mut self,
        _: &mut ipp_core::systems::SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn update(&mut self, _: &mut ipp_core::systems::SystemUpdateContext<'_, '_>) {}
}

#[derive(Default)]
struct Budget {
    slots: usize,
    records: usize,
    tokens: BTreeMap<(u64, u64), WorldAttachmentToken>,
    emitted: Vec<OperationEffect>,
    reserved: usize,
    settles: usize,
}

impl OperationEffectSink for Budget {
    fn reserve(&mut self, demand: &OperationEffectDemand) -> Result<(), ErrorReason> {
        assert_eq!(self.reserved, 0);
        let missing = demand
            .existing_attachment_tokens
            .iter()
            .filter(|token| !self.tokens.contains_key(&token.identity()))
            .count();
        if self.tokens.len() + missing + demand.fresh_attachment_tokens > self.slots
            || self.emitted.len() + demand.max_records > self.records
        {
            return Err(ErrorReason::Capacity);
        }
        self.reserved = demand.max_records;
        Ok(())
    }

    fn emit(&mut self, effect: OperationEffect) {
        self.reserved = self
            .reserved
            .checked_sub(1)
            .expect("unreserved committed effect");
        let OperationEffect::WorldAttachment(
            WorldAttachmentEffect::Written(token)
            | WorldAttachmentEffect::Detached(token)
            | WorldAttachmentEffect::Superseded(token),
        ) = &effect
        else {
            panic!("only attachment effects are admitted here: {effect:?}");
        };
        self.tokens.insert(token.identity(), token.clone());
        assert!(self.tokens.len() <= self.slots);
        self.emitted.push(effect);
        assert!(self.emitted.len() <= self.records);
    }

    fn settle(&mut self) {
        self.reserved = 0;
        self.settles += 1;
    }

    fn attachment_receipt(&self, receipt: u64) -> Result<WorldAttachmentToken, ErrorReason> {
        self.tokens
            .values()
            .find(|token| token.identity().1 == receipt)
            .cloned()
            .ok_or(ErrorReason::InvalidEntity)
    }
}

struct SharedBudget(Rc<RefCell<Budget>>);

impl OperationEffectSink for SharedBudget {
    fn reserve(&mut self, demand: &OperationEffectDemand) -> Result<(), ErrorReason> {
        self.0.borrow_mut().reserve(demand)
    }

    fn emit(&mut self, effect: OperationEffect) {
        self.0.borrow_mut().emit(effect);
    }

    fn settle(&mut self) {
        self.0.borrow_mut().settle();
    }

    fn attachment_receipt(&self, receipt: u64) -> Result<WorldAttachmentToken, ErrorReason> {
        self.0.borrow().attachment_receipt(receipt)
    }
}

fn create(alias: u32) -> Command {
    Command::Create {
        alias,
        metadata: Default::default(),
        adopt: false,
    }
}

fn attach(entity: EntityRef) -> Command {
    Command::insert_value(
        entity,
        ComponentValue::WorldAttachment(WorldAttachment::default()),
    )
}

fn apply(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
    budget: Option<&mut Budget>,
) -> BatchOutcome {
    let batch = Batch {
        id: 1,
        operations,
    };
    let mut context = host.world_mut(world).unwrap();
    let shared = match budget {
        Some(budget) => {
            let shared = Rc::new(RefCell::new(std::mem::take(budget)));
            context
                .enqueue_with_effect_sink(batch, Box::new(SharedBudget(shared.clone())))
                .unwrap();
            Some((budget, shared))
        }
        None => {
            context.enqueue(batch).unwrap();
            None
        }
    };
    drop(context);

    let outcome = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0);
    if let Some((budget, shared)) = shared {
        *budget = shared.take();
    }
    outcome
}

#[test]
fn capacity_failure_retains_aliases_and_receipts_before_the_failed_operation() {
    let callbacks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut factories = ipp_core::systems::compiled_system_factories();
    factories.push(std::sync::Arc::new(CallbackObserver(callbacks.clone())));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let world = host
        .create_world(
            Default::default(),
            &[
                ATTACHMENTS,
                &[ipp_core::systems::SystemId("test.effect-callback-observer")],
            ]
            .concat(),
        )
        .unwrap();
    let mut budget = Budget {
        slots: 1,
        records: 8,
        ..Default::default()
    };
    let outcome = apply(
        &mut host,
        world,
        vec![
            create(1),
            attach(EntityRef::Alias(1)),
            create(2),
            attach(EntityRef::Alias(2)),
        ],
        Some(&mut budget),
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(3));
    assert_eq!(error.reason, ErrorReason::Capacity);
    assert_eq!(error.aliases.len(), 2);
    assert_eq!(outcome.effects.len(), 1);
    assert_eq!(outcome.effects[0].operation, 1);
    assert_eq!(budget.emitted.len(), 1);
    assert_eq!(budget.settles, 4);
    assert_eq!(budget.reserved, 0);
    assert_eq!(callbacks.load(std::sync::atomic::Ordering::Relaxed), 3);
    assert_eq!(host.world_mut(world).unwrap().entities().len(), 2);
    assert!(
        host.world_mut(world)
            .unwrap()
            .inspect(error.aliases[1].1)
            .unwrap()
            .components
            .is_empty()
    );
}

#[test]
fn failed_operation_releases_unused_reservation_and_existing_tokens_are_deduplicated() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let mut budget = Budget {
        slots: 1,
        records: 3,
        ..Default::default()
    };
    let invalid = WorldAttachment {
        mode: 255,
        ..Default::default()
    };
    let failed = apply(
        &mut host,
        world,
        vec![
            create(1),
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::WorldAttachment(invalid),
            ),
        ],
        Some(&mut budget),
    );
    let error = failed.result.unwrap_err();
    assert_eq!(error.operation, Some(1));
    assert!(failed.effects.is_empty());
    assert_eq!(budget.reserved, 0);
    assert_eq!(budget.settles, 2);
    let entity = error.aliases[0].1;
    assert!(
        apply(
            &mut host,
            world,
            vec![attach(EntityRef::Handle(entity))],
            Some(&mut budget)
        )
        .result
        .is_ok()
    );
    let token = budget.tokens.values().next().unwrap().clone();
    let detach = || Command::DetachWorldAttachmentReceipt {
        receipt: token.identity().1,
    };
    let detached = apply(
        &mut host,
        world,
        vec![detach(), detach()],
        Some(&mut budget),
    );
    assert!(detached.result.is_ok());
    assert!(matches!(
        detached.effects[0].effect,
        OperationEffect::WorldAttachment(WorldAttachmentEffect::Detached(_))
    ));
    assert!(matches!(
        detached.effects[1].effect,
        OperationEffect::WorldAttachment(WorldAttachmentEffect::Superseded(_))
    ));
    assert_eq!(budget.tokens.len(), 1);
    assert_eq!(budget.emitted.len(), 3);
}

#[test]
fn subtree_reservation_accounts_every_token_and_fails_before_any_deletion() {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CONSTRAINTS]))
        .unwrap();
    let mut operations = vec![create(0), attach(EntityRef::Alias(0))];
    for alias in 1..4 {
        operations.extend([
            create(alias),
            attach(EntityRef::Alias(alias)),
            Command::PlaceEntity {
                entity: EntityRef::Alias(alias),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Alias(0)),
                    before: None,
                },
            },
        ]);
    }
    let created = apply(&mut host, world, operations, None).result.unwrap();
    let root = created[0].1;
    let declaration = apply(
        &mut host,
        world,
        vec![Command::InsertComponent {
            entity: EntityRef::Handle(root),
            component: ComponentValue::SCALAR,
            fields: Vec::new(),
            adopt: false,
        }],
        None,
    );
    assert!(declaration.result.is_ok());
    let mut budget = Budget {
        slots: 4,
        records: 3,
        ..Default::default()
    };
    let deletion = || Command::DeleteSubtree {
        root: EntityRef::Handle(root),
    };
    let failed = apply(&mut host, world, vec![deletion()], Some(&mut budget));
    assert_eq!(failed.result.unwrap_err().reason, ErrorReason::Capacity);
    assert!(failed.effects.is_empty());
    assert_eq!(host.world_mut(world).unwrap().entities().len(), 4);
    assert!(
        host.world_mut(world)
            .unwrap()
            .inspect(root)
            .unwrap()
            .components
            .iter()
            .any(|component| component.type_id() == ComponentValue::SCALAR),
        "the refused deletion has no effect"
    );
    budget.records = 4;
    let deleted = apply(&mut host, world, vec![deletion()], Some(&mut budget));
    assert!(deleted.result.is_ok());
    assert_eq!(deleted.effects.len(), 4);
    assert_eq!(budget.tokens.len(), 4);
    assert!(host.world_mut(world).unwrap().entities().is_empty());
}

#[test]
fn queued_receipt_release_is_revalidated_at_execution_and_preserves_prior_effects() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let budget = Rc::new(RefCell::new(Budget {
        slots: 8,
        records: 8,
        ..Default::default()
    }));
    apply(
        &mut host,
        world,
        vec![create(0), attach(EntityRef::Alias(0))],
        Some(&mut budget.borrow_mut()),
    );
    let receipt = budget.borrow().tokens.values().next().unwrap().identity().1;
    host.world_mut(world)
        .unwrap()
        .enqueue_with_effect_sink(
            Batch {
                id: 5,
                operations: vec![
                    create(1),
                    attach(EntityRef::Alias(1)),
                    Command::DetachWorldAttachmentReceipt {
                        receipt,
                    },
                ],
            },
            Box::new(SharedBudget(budget.clone())),
        )
        .unwrap();
    budget.borrow_mut().tokens.clear();
    let frame = host.frame(0.0).unwrap();
    let outcome = &frame.worlds[&world].as_ref().unwrap().outcomes[0];
    let error = outcome.result.as_ref().unwrap_err();
    assert_eq!(error.operation, Some(2));
    assert_eq!(error.reason, ErrorReason::InvalidEntity);
    assert_eq!(error.aliases.len(), 1);
    assert_eq!(outcome.effects.len(), 1);
    assert_eq!(outcome.effects[0].operation, 1);
    assert_eq!(budget.borrow().reserved, 0);
}

#[test]
fn foreign_receipt_and_unknown_receipt_fail_at_original_operation_index() {
    let mut host = HostRuntime::new();
    let source = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let target = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let mut budget = Budget {
        slots: 8,
        records: 8,
        ..Default::default()
    };
    apply(
        &mut host,
        source,
        vec![create(0), attach(EntityRef::Alias(0))],
        Some(&mut budget),
    );
    let receipt = budget.tokens.values().next().unwrap().identity().1;
    for receipt in [receipt, u64::MAX] {
        let outcome = apply(
            &mut host,
            target,
            vec![
                create(1),
                attach(EntityRef::Alias(1)),
                Command::DetachWorldAttachmentReceipt {
                    receipt,
                },
            ],
            Some(&mut budget),
        );
        let error = outcome.result.unwrap_err();
        assert_eq!(error.operation, Some(2));
        assert_eq!(error.aliases.len(), 1);
        assert_eq!(outcome.effects.len(), 1);
        assert_eq!(outcome.effects[0].operation, 1);
        assert_eq!(budget.reserved, 0);
    }
}

#[test]
fn ordinary_component_removal_and_entity_deletion_reserve_existing_receipts() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let created = apply(
        &mut host,
        world,
        vec![
            create(0),
            attach(EntityRef::Alias(0)),
            create(1),
            attach(EntityRef::Alias(1)),
        ],
        None,
    )
    .result
    .unwrap();
    let mut budget = Budget {
        slots: 1,
        records: 2,
        ..Default::default()
    };
    let outcome = apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(created[0].1),
                component: ComponentValue::WORLD_ATTACHMENT,
            },
            Command::Delete {
                entity: EntityRef::Handle(created[1].1),
            },
        ],
        Some(&mut budget),
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(1));
    assert_eq!(error.reason, ErrorReason::Capacity);
    assert_eq!(outcome.effects.len(), 1);
    assert_eq!(outcome.effects[0].operation, 0);
    assert!(
        host.world_mut(world)
            .unwrap()
            .inspect(created[1].1)
            .is_some()
    );
}
