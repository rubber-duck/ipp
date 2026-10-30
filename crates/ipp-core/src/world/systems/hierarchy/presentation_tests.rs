use super::*;
use crate::{Batch, Command, ComponentValue, EntityPlacementRef, EntityRef, HostRuntime};

#[test]
fn phase_memo_handles_collisions_deep_branches_gate_edits_and_cycles() {
    let mut host = HostRuntime::default();
    let world = host
        .create_world(Default::default(), &[super::super::HierarchySystem::ID])
        .unwrap();
    let mut context = host.world_mut(world).unwrap();
    let count = MEMO_SLOTS * 2 + 1;
    let mut operations = Vec::new();
    for alias in 0..count as u32 {
        operations.push(Command::Create {
            alias,
            metadata: Default::default(),
            adopt: false,
        });
        if alias > 0 {
            operations.push(Command::PlaceEntity {
                entity: EntityRef::Alias(alias),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Alias(alias - 1)),
                    before: None,
                },
            });
        }
    }
    context
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let entities: Vec<_> = context
        .step(0.0)
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
        .into_iter()
        .map(|(_, id)| id)
        .collect();
    let middle = MEMO_SLOTS;
    for (batch, open) in [(2, false), (3, true)] {
        context
            .enqueue(Batch {
                id: batch,
                operations: vec![Command::insert_value(
                    EntityRef::Handle(entities[middle]),
                    ComponentValue::PresentationGate(super::super::PresentationGate {
                        open,
                    }),
                )],
            })
            .unwrap();
        context
            .step(0.0)
            .unwrap()
            .outcomes
            .remove(0)
            .result
            .unwrap();
        let mut memo = PresentationEligibility::default();
        for &reverse in &[false, true, false] {
            for index in 0..count {
                let index = if reverse {
                    count - index - 1
                } else {
                    index
                };
                let expected = open || index < middle;
                assert_eq!(memo.open(context.world, entities[index]), expected);
                assert_eq!(
                    PresentationEligibility::open_unmemoized(context.world, entities[index]),
                    expected
                );
            }
        }
    }
    context
        .enqueue(Batch {
            id: 4,
            operations: vec![Command::PlaceEntity {
                entity: EntityRef::Handle(entities[0]),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(entities[count - 1])),
                    before: None,
                },
            }],
        })
        .unwrap();
    assert_eq!(
        context
            .step(0.0)
            .unwrap()
            .outcomes
            .remove(0)
            .result
            .unwrap_err()
            .reason,
        crate::ErrorReason::UnsupportedDependency
    );
    // Rejected cycles preserve the prior open graph. Invalid ancestry, such as
    // unavailable spatial dependencies, must still close every descendant.
    context.world.state.links.invalid.insert(entities[0]);
    let mut memo = PresentationEligibility::default();
    for entity in entities {
        assert!(!memo.open(context.world, entity));
        assert!(!PresentationEligibility::open_unmemoized(
            context.world,
            entity
        ));
    }
}
