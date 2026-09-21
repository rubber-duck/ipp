use super::*;

fn subscribe(
    system: &mut LifecyclePublisherSystem,
    session: u64,
    subscription: u64,
    filter: LifecycleFilter,
) {
    system
        .apply(
            session,
            &LifecyclePublisherCommand::Subscribe {
                subscription,
                filter,
            },
        )
        .unwrap();
    system.drain_events(session);
}

fn entity(id: u64) -> LifecycleObservation {
    LifecycleObservation::Entity {
        entity: EntityId::from_bits(id),
        kind: EntityLifecycleKind::Created,
    }
}

fn output(system: &mut LifecyclePublisherSystem, session: u64) -> Vec<LifecyclePublisherOutput> {
    system
        .drain_events(session)
        .into_iter()
        .map(|output| *output.downcast::<LifecyclePublisherOutput>().unwrap())
        .collect()
}

#[test]
fn exact_filters_preserve_sequence_and_session_isolation() {
    let mut system = LifecyclePublisherSystem::default();
    subscribe(
        &mut system,
        1,
        10,
        LifecycleFilter {
            entity: Some(EntityId::from_bits(7)),
            ..Default::default()
        },
    );
    subscribe(&mut system, 2, 10, LifecycleFilter::default());
    system.observe(1, &entity(8));
    system.observe(2, &entity(7));
    let one = output(&mut system, 1);
    let LifecyclePublisherOutput::Events(events) = &one[0] else {
        panic!()
    };
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sequence, 2);
    assert_eq!(events[0].tick, 2);
    let two = output(&mut system, 2);
    let LifecyclePublisherOutput::Events(events) = &two[0] else {
        panic!()
    };
    assert_eq!(events.len(), 2);
    assert!(output(&mut system, 3).is_empty());
}

#[test]
fn unsubscribe_and_release_discard_pending_events_and_never_retarget() {
    let mut system = LifecyclePublisherSystem::default();
    subscribe(&mut system, 1, 10, LifecycleFilter::default());
    subscribe(&mut system, 2, 10, LifecycleFilter::default());
    system.observe(1, &entity(7));
    system
        .apply(
            1,
            &LifecyclePublisherCommand::Unsubscribe {
                subscription: 10,
            },
        )
        .unwrap();
    assert!(output(&mut system, 1).is_empty());
    system.release_session(2);
    system.observe(2, &entity(8));
    assert!(output(&mut system, 1).is_empty());
    assert!(output(&mut system, 2).is_empty());
    assert!(system.sessions.is_empty());
}

#[test]
fn overflow_is_explicit_terminal_bounded_and_recoverable() {
    let mut system = LifecyclePublisherSystem::default();
    subscribe(&mut system, 1, 10, LifecycleFilter::default());
    for id in 1..=10_000 {
        system.observe(1, &entity(id));
    }
    assert!(system.sessions[&1].observations.is_empty());
    assert!(system.sessions[&1].subscriptions.is_empty());
    assert_eq!(
        output(&mut system, 1),
        vec![LifecyclePublisherOutput::Overflow {
            dropped: 129
        }]
    );
    subscribe(&mut system, 1, 11, LifecycleFilter::default());
    system.observe(2, &entity(7));
    assert!(matches!(
        output(&mut system, 1).as_slice(),
        [LifecyclePublisherOutput::Events(_)]
    ));
}

#[test]
fn component_incarnations_and_asset_residency_are_owned_observations() {
    let mut system = LifecyclePublisherSystem::default();
    subscribe(
        &mut system,
        1,
        10,
        LifecycleFilter {
            entities: false,
            component: Some(2),
            ..Default::default()
        },
    );
    let observation = LifecycleObservation::Component {
        entity: EntityId::from_bits(7),
        component: 2,
        kind: ComponentLifecycleKind::Replaced,
        previous_incarnation: Some(3),
        incarnation: Some(4),
    };
    system.observe(1, &observation);
    let values = output(&mut system, 1);
    let LifecyclePublisherOutput::Events(events) = &values[0] else {
        panic!()
    };
    assert_eq!(events[0].observation, observation);

    {
        let mut resource = crate::AssetResourceSnapshot {
            representation: Default::default(),
            id: 5,
            kind: crate::services::asset_management::AssetTypeId(1),
            source: "memory://resource".into(),
            variant: 0,
            status: crate::AssetResourceStatus::Loaded,
        };
        system.observe(
            2,
            &LifecycleObservation::Asset {
                resource: resource.clone(),
                kind: crate::services::asset_management::AssetLifecycleKind::StatusChanged,
            },
        );
        resource.status = crate::AssetResourceStatus::Unloaded;
        system.observe(
            3,
            &LifecycleObservation::Asset {
                resource,
                kind: crate::services::asset_management::AssetLifecycleKind::StatusChanged,
            },
        );
        let values = output(&mut system, 1);
        let LifecyclePublisherOutput::Events(events) = &values[0] else {
            panic!()
        };
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0].observation, LifecycleObservation::Asset { resource, .. } if resource.id == 5 && resource.status == crate::AssetResourceStatus::Loaded)
        );
        assert!(
            matches!(&events[1].observation, LifecycleObservation::Asset { resource, .. } if resource.id == 5 && resource.status == crate::AssetResourceStatus::Unloaded)
        );
    }
}
