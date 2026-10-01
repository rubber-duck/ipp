use super::*;

fn structural_controller(id: AnimationControllerId, target: EntityId) -> AnimationController {
    AnimationController {
        snapshot: AnimationControllerSnapshot {
            id,
            description: AnimationControllerDescription {
                drivers: vec![AnimationDriverDescription {
                    source: "asset://10/1".into(),
                    variant: 0,
                    track: 0,
                    target,
                    property: AnimationTrackTarget::EntityLink,
                    entity_bindings: Vec::new(),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                }],
                ..Default::default()
            },
            state: AnimationPlaybackStatus::Stopped,
            time: 0.0,
            transition: None,
        },
        drivers: Vec::new(),
        structural_drivers: Vec::new(),
        driver_targets: BTreeMap::new(),
        incarnations: Vec::new(),
        sought: false,
        directional_start_pending: false,
        duration: 0.0,
        ready: false,
        numeric_targets: Vec::new(),
        discrete_drivers: Vec::new(),
        numeric_outputs: Vec::new(),
        failure: None,
        transition: None,
        contributions: Default::default(),
    }
}

#[test]
fn structural_reverse_index_ignores_unrelated_node_churn() {
    let mut state = AnimationSystemState::default();
    for index in 1..=512 {
        let id = AnimationControllerId::from_bits(index);
        let target = EntityId::from_bits(index + 1000);
        state
            .controllers
            .insert(id, structural_controller(id, target));
        state.index_controller(id);
    }
    assert_eq!(state.structural_target_controllers.len(), 512);

    let mut staged = crate::world::WorldMutationState::default();
    for index in 1..=512 {
        staged
            .operation_deleted
            .insert(EntityId::from_bits(index + 2000));
    }
    staged.operation_deleted.insert(EntityId::from_bits(1256));
    assert_eq!(
        state.affected_by(&staged),
        vec![AnimationControllerId::from_bits(256)]
    );

    state.unindex_controller(AnimationControllerId::from_bits(256));
    assert!(state.affected_by(&staged).is_empty());
    state.rebuild_target_index();
    assert_eq!(
        state.affected_by(&staged),
        vec![AnimationControllerId::from_bits(256)]
    );
}

#[test]
fn real_world_deletions_visit_only_indexed_structural_controllers() {
    use crate::{Batch, Command, EntityMetadata, EntityRef, HostRuntime, WorldLimits};

    let mut host = HostRuntime::new();
    let id = host
        .create_world(
            WorldLimits::default(),
            &[
                crate::systems::animation::AnimationSystem::ID,
                crate::systems::asset_dependencies::AssetDependencySystem::ID,
            ],
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: (0..320)
                .map(|alias| Command::Create {
                    alias,
                    metadata: EntityMetadata::default(),
                    adopt: false,
                })
                .collect(),
        })
        .unwrap();
    let entities: Vec<_> = world
        .step(0.0)
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
        .into_iter()
        .map(|(_, entity)| entity)
        .collect();
    for &target in &entities[..256] {
        let description = structural_controller(AnimationControllerId::from_bits(1), target)
            .snapshot
            .description;
        world.create_animation_controller(description).unwrap();
    }

    STRUCTURAL_CANDIDATE_VISITS.set(0);
    world
        .enqueue(Batch {
            id: 2,
            operations: entities[256..]
                .iter()
                .copied()
                .map(|entity| Command::Delete {
                    entity: EntityRef::Handle(entity),
                })
                .collect(),
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    let unrelated_visits = STRUCTURAL_CANDIDATE_VISITS.get();
    assert!(unrelated_visits <= 128, "{unrelated_visits} indexed visits");

    world
        .enqueue(Batch {
            id: 3,
            operations: vec![Command::Delete {
                entity: EntityRef::Handle(entities[128]),
            }],
        })
        .unwrap();
    assert!(world.step(0.0).unwrap().outcomes[0].result.is_ok());
    let targeted_visits = STRUCTURAL_CANDIDATE_VISITS.get() - unrelated_visits;
    assert!(targeted_visits <= 4, "{targeted_visits} indexed visits");
}
