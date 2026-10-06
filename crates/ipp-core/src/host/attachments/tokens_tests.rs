use super::*;
use crate::{
    Batch, Command, ComponentValue, EntityRef, FieldValue, FieldWrite, OperationEffect,
    WorldAttachment,
};

fn write(
    host: &mut HostRuntime,
    world: crate::WorldId,
    operations: Vec<Command>,
) -> crate::BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
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
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    outcome
}

fn written(outcome: &crate::BatchOutcome) -> Vec<WorldAttachmentToken> {
    outcome
        .effects
        .iter()
        .map(|effect| {
            let OperationEffect::WorldAttachment(WorldAttachmentEffect::Written(token)) =
                &effect.effect
            else {
                panic!("write receipt");
            };
            token.clone()
        })
        .collect()
}

#[test]
fn edge_retains_only_current_and_published_receipts_not_write_history() {
    let mut host = HostRuntime::new();
    let parent = host
        .create_world(
            Default::default(),
            &[crate::systems::world_attachment::WorldAttachmentSystem::ID],
        )
        .unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let child = host.world_ref(child).unwrap();
    let outcome = write(
        &mut host,
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
    let anchor = outcome.result.as_ref().unwrap()[0].1;
    let original = written(&outcome).remove(0);
    drop(outcome);
    assert_eq!(
        host.attachment_retirement(&original),
        Ok(WorldAttachmentRetirement::Pending)
    );

    // One batch writes a long history between two publications of the edge.
    let outcome = write(
        &mut host,
        parent,
        (0..4096)
            .map(|_| Command::SetField {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::WORLD_ATTACHMENT,
                field: FieldWrite {
                    offset: std::mem::offset_of!(WorldAttachment, child) as u32,
                    value: FieldValue::World(Some(child)),
                },
            })
            .collect(),
    );
    let mut history = written(&outcome);
    drop(outcome);
    assert_eq!(history.len(), 4096);
    let current = history.pop().unwrap();
    assert_eq!(host.topology.tokens.len(), 1);
    assert_eq!(
        host.attachment_retirement(&current),
        Ok(WorldAttachmentRetirement::Pending)
    );
    let released: Vec<_> = std::iter::once(original)
        .chain(history)
        .map(|receipt| {
            assert_eq!(
                host.attachment_retirement(&receipt),
                Ok(WorldAttachmentRetirement::Retired)
            );
            assert_eq!(Arc::strong_count(&receipt.0), 1);
            Arc::downgrade(&receipt.0)
        })
        .collect();
    assert!(released.iter().all(|receipt| receipt.upgrade().is_none()));

    let removed = write(
        &mut host,
        parent,
        vec![Command::DetachWorldAttachmentIf {
            expected: current.clone(),
        }],
    );
    drop(removed);
    assert!(host.topology.tokens.is_empty());
    assert_eq!(
        host.attachment_retirement(&current),
        Ok(WorldAttachmentRetirement::Retired)
    );
    assert_eq!(Arc::strong_count(&current.0), 1);
    let current_weak = Arc::downgrade(&current.0);
    drop(current);
    assert!(current_weak.upgrade().is_none());
}

#[test]
fn detached_receipt_has_stable_terminal_state_and_no_host_ownership() {
    let receipt = {
        let mut host = HostRuntime::new();
        let parent = host
            .create_world(
                Default::default(),
                &[crate::systems::world_attachment::WorldAttachmentSystem::ID],
            )
            .unwrap();
        let child = host.create_world(Default::default(), &[]).unwrap();
        let child = host.world_ref(child).unwrap();
        let outcome = write(
            &mut host,
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
        written(&outcome).remove(0)
    };
    assert!(receipt.0.retired.load(AtomicOrdering::Acquire));
    assert_eq!(Arc::strong_count(&receipt.0), 1);
    let weak = Arc::downgrade(&receipt.0);
    drop(receipt);
    assert!(weak.upgrade().is_none());
}
