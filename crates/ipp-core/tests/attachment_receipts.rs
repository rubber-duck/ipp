//! Real ordered attachment writes, conditional cleanup and exact retirement receipts.

mod support;

use support::selection::ATTACHMENTS;
use support::world_failures::select_with_failures;

use ipp_core::{
    AppliedOperationEffect, Batch, BatchOutcome, Command, ComponentValue, EntityId, EntityRef,
    ErrorReason, FieldValue, FieldWrite, HostRuntime, OperationEffect, WorldAttachment,
    WorldAttachmentEffect, WorldAttachmentRetirement, WorldAttachmentToken, WorldId,
};

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 41,
            operations,
        })
        .unwrap();
    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn token(outcome: &BatchOutcome, operation: usize) -> WorldAttachmentToken {
    let effect = outcome
        .effects
        .iter()
        .find(|effect| effect.operation == operation)
        .unwrap();
    let OperationEffect::WorldAttachment(WorldAttachmentEffect::Written(token)) = &effect.effect
    else {
        panic!("applied attachment write receipt");
    };
    token.clone()
}

fn attach(
    host: &mut HostRuntime,
    parent: WorldId,
    child: WorldId,
) -> (EntityId, WorldAttachmentToken) {
    let child = host.world_ref(child).unwrap();
    let outcome = apply(
        host,
        parent,
        vec![
            Command::Create {
                alias: 0,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(0),
                ComponentValue::WorldAttachment(WorldAttachment::spatial(child)),
            ),
        ],
    );
    (outcome.result.as_ref().unwrap()[0].1, token(&outcome, 1))
}

fn child_write(anchor: EntityId, child: ipp_core::WorldRef) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(anchor),
        component: ComponentValue::WORLD_ATTACHMENT,
        field: FieldWrite {
            offset: std::mem::offset_of!(WorldAttachment, child) as u32,
            value: FieldValue::World(Some(child)),
        },
    }
}

fn detach(expected: &WorldAttachmentToken) -> Command {
    Command::DetachWorldAttachmentIf {
        expected: expected.clone(),
    }
}

#[test]
fn applied_receipt_survives_failure_and_same_value_writes_supersede_without_aba() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let (anchor, first) = attach(&mut host, parent, child);
    let child_ref = host.world_ref(child).unwrap();
    let outcome = apply(
        &mut host,
        parent,
        vec![
            child_write(anchor, child_ref),
            Command::SetField {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::SCALAR,
                field: FieldWrite {
                    offset: 0,
                    value: FieldValue::F32(1.0),
                },
            },
        ],
    );
    assert_eq!(outcome.result.as_ref().unwrap_err().operation, Some(1));
    let second = token(&outcome, 0);
    assert_ne!(first, second);
    assert_eq!(first.incarnation(), second.incarnation());
    assert_eq!(
        host.attachment_retirement(&first),
        Ok(WorldAttachmentRetirement::Retired)
    );
    let superseded = apply(&mut host, parent, vec![detach(&first)]);
    assert!(superseded.result.is_ok());
    assert_eq!(
        superseded.effects,
        [AppliedOperationEffect {
            operation: 0,
            effect: OperationEffect::WorldAttachment(WorldAttachmentEffect::Superseded(
                first.clone()
            ))
        }]
    );
    assert_eq!(
        host.attachment_retirement(&second),
        Ok(WorldAttachmentRetirement::Pending)
    );

    let inserted = apply(
        &mut host,
        parent,
        vec![Command::InsertComponent {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::WORLD_ATTACHMENT,
            fields: vec![FieldWrite {
                offset: std::mem::offset_of!(WorldAttachment, child) as u32,
                value: FieldValue::World(Some(child_ref)),
            }],
            adopt: false,
        }],
    );
    let third = token(&inserted, 0);
    assert_ne!(third, second);
    assert_ne!(third.incarnation(), second.incarnation());
    assert_eq!(
        host.attachment_retirement(&second),
        Ok(WorldAttachmentRetirement::Retired)
    );
    assert!(
        matches!(&apply(&mut host, parent, vec![detach(&second)]).effects[0].effect, OperationEffect::WorldAttachment(WorldAttachmentEffect::Superseded(found)) if found == &second)
    );

    let rejected = apply(
        &mut host,
        parent,
        vec![Command::SetField {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::WORLD_ATTACHMENT,
            field: FieldWrite {
                offset: std::mem::offset_of!(WorldAttachment, mode) as u32,
                value: FieldValue::U32(99),
            },
        }],
    );
    assert_eq!(
        rejected.result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
    assert!(rejected.effects.is_empty());

    let removed = apply(&mut host, parent, vec![detach(&third)]);
    assert_eq!(
        removed.effects[0].effect,
        OperationEffect::WorldAttachment(WorldAttachmentEffect::Detached(third.clone()))
    );
    assert_eq!(
        host.attachment_retirement(&third),
        Ok(WorldAttachmentRetirement::Retired)
    );
    let reinserted = apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child_ref)),
        )],
    );
    let fourth = token(&reinserted, 0);
    assert_ne!(fourth.incarnation(), third.incarnation());
    assert!(
        matches!(&apply(&mut host, parent, vec![detach(&third)]).effects[0].effect, OperationEffect::WorldAttachment(WorldAttachmentEffect::Superseded(found)) if found == &third)
    );
    assert_eq!(
        host.attachment_retirement(&fourth),
        Ok(WorldAttachmentRetirement::Pending)
    );
}

#[test]
fn selected_attachment_system_owns_receipts_and_foreign_world_cleanup_is_rejected() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(
            Default::default(),
            &[ipp_core::systems::world_attachment::WorldAttachmentSystem::ID],
        )
        .unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let peer = host
        .create_world(
            Default::default(),
            &[ipp_core::systems::world_attachment::WorldAttachmentSystem::ID],
        )
        .unwrap();
    let (_, receipt) = attach(&mut host, parent, child);
    let unsupported = apply(&mut host, child, vec![detach(&receipt)]);
    assert_eq!(
        unsupported.result.unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
    assert!(unsupported.effects.is_empty());
    let foreign_world = apply(&mut host, peer, vec![detach(&receipt)]);
    assert_eq!(
        foreign_world.result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
    assert!(foreign_world.effects.is_empty());
    host.frame(0.0).unwrap();
    assert_eq!(
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments[0]
            .token,
        receipt
    );
    assert_eq!(
        apply(&mut host, parent, vec![detach(&receipt)]).effects[0].effect,
        OperationEffect::WorldAttachment(WorldAttachmentEffect::Detached(receipt.clone()))
    );
    host.frame(0.0).unwrap();
    assert_eq!(
        host.attachment_retirement(&receipt),
        Ok(WorldAttachmentRetirement::Retired)
    );
}

#[test]
fn replacement_preserves_old_fence_until_old_path_retires() {
    let (mut host, failure) = support::world_failures::host_with_world_failures();
    let parent = host
        .create_world(Default::default(), &select_with_failures(&[ATTACHMENTS]))
        .unwrap();
    let old_child = host.create_world(Default::default(), &[]).unwrap();
    let new_child = host.create_world(Default::default(), &[]).unwrap();
    let (anchor, original) = attach(&mut host, parent, old_child);
    host.frame(0.0).unwrap();
    let before = host.latest_publication(parent).unwrap();
    assert_eq!(
        host.publication(before).unwrap().attachments[0].token,
        original
    );
    let new_child_ref = host.world_ref(new_child).unwrap();

    // While the parent's publications fail, its last completed path still holds the old edge.
    failure.fail_publication(Some(parent));
    let outcome = apply(
        &mut host,
        parent,
        vec![child_write(anchor, new_child_ref), detach(&original)],
    );
    let replacement = token(&outcome, 0);
    assert_eq!(
        outcome.effects[1].effect,
        OperationEffect::WorldAttachment(WorldAttachmentEffect::Superseded(original.clone()))
    );
    assert_eq!(host.latest_publication(parent), Some(before));
    assert_eq!(
        host.attachment_retirement(&original),
        Ok(WorldAttachmentRetirement::Pending)
    );
    assert!(
        host.frame(0.0)
            .unwrap()
            .publication_errors
            .contains_key(&parent)
    );
    assert_eq!(host.latest_publication(parent), Some(before));
    assert_eq!(
        host.attachment_retirement(&original),
        Ok(WorldAttachmentRetirement::Pending)
    );
    failure.fail_publication(None);
    host.frame(0.0).unwrap();
    assert_eq!(
        host.attachment_retirement(&original),
        Ok(WorldAttachmentRetirement::Retired)
    );
    assert_eq!(
        host.attachment_retirement(&replacement),
        Ok(WorldAttachmentRetirement::Pending)
    );
    let current = &host
        .publication(host.latest_publication(parent).unwrap())
        .unwrap()
        .attachments[0];
    assert_eq!(current.child.id(), new_child);
    assert_eq!(current.token, replacement);

    let old_child_ref = host.world_ref(old_child).unwrap();
    let returned = apply(&mut host, parent, vec![child_write(anchor, old_child_ref)]);
    let returned = token(&returned, 0);
    assert_ne!(returned, original);
    assert_eq!(
        host.attachment_retirement(&original),
        Ok(WorldAttachmentRetirement::Retired)
    );
    assert!(
        matches!(&apply(&mut host, parent, vec![detach(&original)]).effects[0].effect, OperationEffect::WorldAttachment(WorldAttachmentEffect::Superseded(found)) if found == &original)
    );
    assert_eq!(
        host.attachment_retirement(&returned),
        Ok(WorldAttachmentRetirement::Pending)
    );
}

#[test]
fn ordinary_removal_generation_reuse_and_destruction_do_not_retarget_receipts() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let (anchor, original) = attach(&mut host, parent, child);
    host.frame(0.0).unwrap();
    let removed = apply(
        &mut host,
        parent,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(anchor),
            component: ComponentValue::WORLD_ATTACHMENT,
        }],
    );
    assert_eq!(
        removed.effects[0].effect,
        OperationEffect::WorldAttachment(WorldAttachmentEffect::Detached(original.clone()))
    );
    assert_eq!(
        host.attachment_retirement(&original),
        Ok(WorldAttachmentRetirement::Retired)
    );
    assert!(
        apply(
            &mut host,
            parent,
            vec![Command::Delete {
                entity: EntityRef::Handle(anchor)
            }]
        )
        .result
        .is_ok()
    );
    let (replacement, current) = attach(&mut host, parent, child);
    assert_ne!(replacement, anchor);
    assert_eq!(replacement.index(), anchor.index());
    assert!(
        matches!(&apply(&mut host, parent, vec![detach(&original)]).effects[0].effect, OperationEffect::WorldAttachment(WorldAttachmentEffect::Superseded(found)) if found == &original)
    );
    assert_eq!(
        host.attachment_retirement(&current),
        Ok(WorldAttachmentRetirement::Pending)
    );

    let mut foreign = crate::support::task_scheduler::host();
    let foreign_parent = foreign
        .create_world(Default::default(), ATTACHMENTS)
        .unwrap();
    assert_eq!(
        foreign.attachment_retirement(&original),
        Err(ErrorReason::InvalidValue)
    );
    let failed = apply(&mut foreign, foreign_parent, vec![detach(&current)]);
    assert_eq!(failed.result.unwrap_err().reason, ErrorReason::InvalidValue);
    assert!(failed.effects.is_empty());
    assert!(host.destroy_world(child));
    assert_eq!(
        host.attachment_retirement(&current),
        Ok(WorldAttachmentRetirement::Retired)
    );
    host.frame(0.0).unwrap();
    assert!(host.destroy_world(parent));
    assert_eq!(
        host.attachment_retirement(&current),
        Ok(WorldAttachmentRetirement::Retired)
    );
}
