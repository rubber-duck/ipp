use super::*;
use crate::services::reliable_output::OutputLimits;

fn fixture() -> (LifecycleWatchOutput, LifecycleWatchId, LifecycleObservation) {
    let mut host = crate::HostRuntime::new();
    let world = host
        .create_world(
            Default::default(),
            &[crate::systems::lifecycle_publisher::LifecyclePublisherSystem::ID],
        )
        .unwrap();
    let output = LifecycleWatchOutput::new(
        ReliableOutputAccount::new(OutputLimits {
            bytes: 16384,
            reply_reserve: 0,
        }),
        host.world_ref(world).unwrap(),
        7,
        LifecycleWatchEncoding {
            acknowledgement_bytes: 128,
            baseline_bytes: 48,
            event_bytes: 128,
            baseline_field_bytes: 4,
            value_bytes: 96,
            value_field_bytes: 40,
        },
    )
    .unwrap();
    let entity = EntityId::from_bits(9);
    let member = output
        .new_member(
            LifecycleWatchTarget::Entity(entity),
            LifecycleWatchKinds::ENTITY_DELETED,
        )
        .unwrap();
    (
        output,
        member.id(),
        LifecycleObservation::Entity {
            entity,
            kind: EntityLifecycleKind::Deleted,
        },
    )
}

#[test]
fn traffic_counts_only_retained_events_and_keeps_full_charge_after_drain_and_removal() {
    let (output, member, observation) = fixture();
    let ack = output
        .account()
        .reserve(output.acknowledgement_charge(0, 0).unwrap())
        .unwrap();
    output.0.retain(
        LifecycleWatchRecordBody::Acknowledgement {
            request: 1,
            action: LifecycleMembershipAction::Add,
            cut: None,
            result: LifecycleMembershipResult::Cancelled,
        },
        ack,
    );
    assert_eq!(output.traffic(), Some(LifecycleWatchTraffic::default()));
    drop(output.pop_front().unwrap());
    output.0.observe(member, 2, 3, &observation);
    let event = output.pop_front().unwrap();
    let charged = event.charge().bytes as u64;
    assert!(charged > output.0.encoding.event_bytes as u64);
    assert_eq!(output.traffic().unwrap().queued_bytes, charged);
    drop(event);
    output.0.observe(member, 3, 3, &observation);
    let retained = output.traffic().unwrap();
    assert_eq!(retained.queued_events, 2);
    assert_eq!(retained.queued_bytes, 2 * charged);
    output.0.remove(member);
    assert!(output.pop_front().is_none());
    assert_eq!(output.traffic(), Some(retained));
    output.0.retire();
    assert!(output.pop_front().is_none());
    assert_eq!(output.0.traffic.get(), retained);
    assert_eq!(output.traffic(), None);
}

#[test]
fn failed_capacity_and_closed_endpoint_retention_do_not_increment_traffic() {
    let (output, member, observation) = fixture();
    let held = output
        .account()
        .reserve(OutputCharge {
            entries: 0,
            bytes: 16384 - output.account().usage().bytes,
        })
        .unwrap();
    output.0.observe(member, 1, 1, &observation);
    assert_eq!(
        output.account().status(),
        OutputStatus::Failed(OutputFailure::Capacity)
    );
    assert!(output.pop_front().is_none());
    assert_eq!(output.0.traffic.get(), LifecycleWatchTraffic::default());
    assert_eq!(output.traffic(), None);
    drop(held);

    let (output, member, observation) = fixture();
    output.close();
    output.0.observe(member, 1, 1, &observation);
    assert!(output.pop_front().is_none());
    assert_eq!(output.0.traffic.get(), LifecycleWatchTraffic::default());
    assert_eq!(output.traffic(), None);
}
