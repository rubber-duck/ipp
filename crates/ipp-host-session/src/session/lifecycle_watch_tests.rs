use super::*;
use ipp_core::services::reliable_output::{OutputLimits, ReliableOutputAccount};

struct Platform;

impl HostServices for Platform {
    const NAME: &'static str = "lifecycle-watch-test";

    fn initialize(
        _: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

fn ready() -> Host<Platform> {
    let mut host = Host::new().unwrap();
    host.open_session(
        1,
        &[
            ipp_core::systems::lifecycle_publisher::LifecyclePublisherSystem::ID,
            ipp_core::systems::constraints::ConstraintSystem::ID,
        ],
    )
    .unwrap();
    host.session_mut(1)
        .unwrap()
        .receive(&ipp_protocol::contract::HELLO)
        .unwrap();
    host.session_mut(1).unwrap().take_response().unwrap();
    host
}

fn add(host: &mut Host<Platform>, request: u64, count: u64) {
    add_targets(host, request, &(1..=count).collect::<Vec<_>>());
}

fn add_targets(host: &mut Host<Platform>, request: u64, entities: &[u64]) {
    add_session_targets(host, 1, request, entities);
}

fn add_session_targets(host: &mut Host<Platform>, session: u64, request: u64, entities: &[u64]) {
    let world = host.session_mut(session).unwrap().world.world_ref();
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend_from_slice(&request.to_le_bytes());
    bytes.push(35);
    bytes.extend_from_slice(&world.id().0.to_le_bytes());
    bytes.extend_from_slice(&world.incarnation().to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&(entities.len() as u32).to_le_bytes());
    for entity in entities {
        bytes.push(0);
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.push(4);
    }
    host.session_mut(session).unwrap().receive(&bytes).unwrap();
}

#[test]
fn watch_without_the_publisher_is_rejected_with_its_reason_and_no_ack() {
    let mut host = Host::<Platform>::new().unwrap();
    host.open_session(1, &[ipp_core::systems::constraints::ConstraintSystem::ID])
        .unwrap();
    host.session_mut(1)
        .unwrap()
        .receive(&ipp_protocol::contract::HELLO)
        .unwrap();
    host.session_mut(1).unwrap().take_response().unwrap();

    add(&mut host, 1, 2);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());

    let response = host.session_mut(1).unwrap().take_response().unwrap();
    assert_eq!(response[8..16], 1u64.to_le_bytes());
    let reason = LIFECYCLE_WATCH_UNSUPPORTED.as_bytes();
    assert!(
        response
            .windows(reason.len())
            .any(|window| window == reason),
        "the rejection must name the missing System"
    );
    // Only the uncorrelated frame completion follows; no cancelled ACK for the request.
    while let Some(response) = host.session_mut(1).unwrap().take_response() {
        assert_eq!(response[8..16], 0u64.to_le_bytes());
    }

    let session = &host.sessions[&1];
    assert!(session.lifecycle_watch.is_none());
    assert!(session.reply_reservations.is_empty());
    assert!(session.request_origins.is_empty());
}

#[test]
fn watch_refused_by_a_full_world_queue_fails_the_connection() {
    let mut host = ready();
    {
        let mut session = host.session_mut(1).unwrap();
        while session
            .world
            .enqueue_system_command(ipp_core::systems::constraints::ConstraintSystem::ID, 1, ())
            .is_ok()
        {}
    }

    add(&mut host, 1, 1);
    host.session_mut(1).unwrap().prepare_request(0.0).unwrap();

    let failure = &host.sessions[&1].pending_errors[&0];
    assert_eq!(failure, "Lifecycle watch refused: Capacity");
}

#[test]
fn singleton_add_ack_bookkeeping_visits_only_requested_registrations() {
    let mut host = ready();
    for request in 1..=130 {
        add_targets(&mut host, request, &[request]);
        host.tick(0.0).unwrap();
        while host.session_mut(1).unwrap().take_response().is_some() {}
    }
    let watch = host.sessions[&1].lifecycle_watch.as_ref().unwrap();
    assert_eq!(watch.members.len(), 130);
    assert_eq!(watch.registration_visits, 130);
}

#[test]
fn cancelled_add_ack_bookkeeping_visits_only_its_pending_registrations() {
    let mut host = ready();
    add(&mut host, 1, 130);
    host.tick(0.0).unwrap();
    while host.session_mut(1).unwrap().take_response().is_some() {}
    host.sessions
        .get_mut(&1)
        .unwrap()
        .lifecycle_watch
        .as_mut()
        .unwrap()
        .registration_visits = 0;
    add_targets(&mut host, 2, &[999]);
    host.session_mut(1).unwrap().prepare_request(0.0).unwrap();
    host.session_mut(1).unwrap().world.release_system_session(1);
    assert!(host.drain_lifecycle_watches().is_empty());
    let watch = host.sessions[&1].lifecycle_watch.as_ref().unwrap();
    assert_eq!(watch.members.len(), 130);
    assert_eq!(watch.registration_visits, 1);
}

#[test]
fn partial_add_preparation_failure_releases_only_request_local_members_and_credit() {
    let mut host = ready();
    add(&mut host, 1, 130);
    host.tick(0.0).unwrap();
    while host.session_mut(1).unwrap().take_response().is_some() {}
    let watch = host
        .sessions
        .get_mut(&1)
        .unwrap()
        .lifecycle_watch
        .as_mut()
        .unwrap();
    watch.registration_visits = 0;
    let account = watch.output.account().clone();
    let before = account.usage();
    let member = watch
        .output
        .new_member(
            LifecycleWatchTarget::Entity(ipp_core::EntityId::from_bits(999)),
            LifecycleWatchKinds::ENTITY_DELETED,
        )
        .unwrap();
    let core_member_bytes = account.usage().bytes - before.bytes;
    drop(member);
    assert_eq!(account.usage(), before);
    add_targets(&mut host, 2, &[999, 1000]);
    let preparation_bytes = 2 * std::mem::size_of::<LifecycleWatchMember>() * 2;
    let pending_bytes = std::mem::size_of::<PendingAdd>()
        + reliable_output::RESPONSE_METADATA_BYTES
        + 2 * std::mem::size_of::<u64>();
    let first_member_bytes = std::mem::size_of::<Registration>()
        + reliable_output::RESPONSE_METADATA_BYTES
        + core_member_bytes;
    let held = account
        .reserve(OutputCharge {
            entries: 0,
            bytes: reliable_output::ORDINARY_OUTPUT_BYTES
                - account.usage().bytes
                - preparation_bytes
                - pending_bytes
                - first_member_bytes,
        })
        .unwrap();
    host.session_mut(1).unwrap().prepare_request(0.0).unwrap();
    let watch = host.sessions[&1].lifecycle_watch.as_ref().unwrap();
    assert_eq!(watch.members.len(), 130);
    assert!(watch.members.values().all(|entry| entry.member.is_active()));
    assert!(watch.pending_adds.is_empty());
    assert_eq!(watch.registration_visits, 1);
    drop(held);
    host.tick(0.0).unwrap();
    let reply = host.session_mut(1).unwrap().take_response().unwrap();
    assert_eq!(&reply[24..27], &[255, 1, 0]);
    drop(reply);
    while host.session_mut(1).unwrap().take_response().is_some() {}
    assert_eq!(account.status(), OutputStatus::Open);
    assert_eq!(account.usage(), before);
    add_targets(&mut host, 3, &[1001]);
    host.tick(0.0).unwrap();
    assert_eq!(
        host.sessions[&1]
            .lifecycle_watch
            .as_ref()
            .unwrap()
            .members
            .len(),
        131
    );
}

#[test]
fn default_output_budget_delivers_every_relevant_event_of_a_bulk_deletion() {
    let mut host = ready();
    host.session_mut(1)
        .unwrap()
        .world
        .enqueue(ipp_core::Batch {
            id: 900,
            operations: (0..129)
                .map(|alias| ipp_core::Command::Create {
                    alias,
                    metadata: Default::default(),
                    adopt: false,
                })
                .collect(),
        })
        .unwrap();
    host.tick(0.0).unwrap();
    let entities: Vec<_> = host
        .session_mut(1)
        .unwrap()
        .world
        .entities()
        .iter()
        .map(|entity| entity.id.to_bits())
        .collect();
    assert_eq!(entities.len(), 129);
    while host.session_mut(1).unwrap().take_response().is_some() {}
    add_targets(&mut host, 22, &entities);
    host.tick(0.0).unwrap();
    while host.session_mut(1).unwrap().take_response().is_some() {}
    let account = host.sessions[&1].reply_budget.0.clone();
    host.session_mut(1)
        .unwrap()
        .world
        .enqueue(ipp_core::Batch {
            id: 901,
            operations: entities
                .into_iter()
                .map(|entity| ipp_core::Command::Delete {
                    entity: ipp_core::EntityRef::Handle(ipp_core::EntityId::from_bits(entity)),
                })
                .collect(),
        })
        .unwrap();
    // More than 128 relevant deletions in one frame fit the connection's byte budget, so every
    // event is delivered and tracking continues.
    let failures = host.tick_worlds(0.0).unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(account.status(), OutputStatus::Open);
    assert!(host.session_mut(1).unwrap().world.entities().is_empty());
    let mut events = 0;
    while let Some(reply) = host.session_mut(1).unwrap().take_response() {
        if reply[24] == 37 {
            assert_eq!(reply[49], 1);
            events += 1;
        }
    }
    assert_eq!(events, 129);
    let watch = host.sessions[&1].lifecycle_watch.as_ref().unwrap();
    assert_eq!(watch.members.len(), 129);
    assert!(watch.members.values().all(|entry| entry.member.is_active()));
}

/// Open a second ready session, a separate connection, on its own World.
fn ready_peer(host: &mut Host<Platform>, session: u64) {
    host.open_session(
        session,
        &[
            ipp_core::systems::lifecycle_publisher::LifecyclePublisherSystem::ID,
            ipp_core::systems::constraints::ConstraintSystem::ID,
        ],
    )
    .unwrap();
    host.session_mut(session)
        .unwrap()
        .receive(&ipp_protocol::contract::HELLO)
        .unwrap();
    host.session_mut(session).unwrap().take_response().unwrap();
}

/// Create `count` entities in a session's World and return their identities in order.
fn create_entities(host: &mut Host<Platform>, session: u64, count: u32) -> Vec<u64> {
    host.session_mut(session)
        .unwrap()
        .world
        .enqueue(ipp_core::Batch {
            id: 900,
            operations: (0..count)
                .map(|alias| ipp_core::Command::Create {
                    alias,
                    metadata: Default::default(),
                    adopt: false,
                })
                .collect(),
        })
        .unwrap();
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    while host.session_mut(session).unwrap().take_response().is_some() {}
    host.session_mut(session)
        .unwrap()
        .world
        .entities()
        .iter()
        .map(|entity| entity.id.to_bits())
        .collect()
}

fn subscribe_entities(host: &mut Host<Platform>, session: u64, request: u64, subscription: u64) {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend_from_slice(&request.to_le_bytes());
    // Subscribe to entity observations of any entity.
    bytes.push(19);
    bytes.extend_from_slice(&subscription.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    host.session_mut(session).unwrap().receive(&bytes).unwrap();
}

fn delete_all(host: &mut Host<Platform>, session: u64, entities: &[u64]) {
    host.session_mut(session)
        .unwrap()
        .world
        .enqueue(ipp_core::Batch {
            id: 901,
            operations: delete(entities),
        })
        .unwrap();
}

#[test]
fn output_exhaustion_fails_only_its_connection_while_a_peer_receives_every_record() {
    const COUNT: u32 = 300;
    let mut host = ready();
    ready_peer(&mut host, 2);
    host.sessions.get_mut(&1).unwrap().reply_budget =
        reliable_output::SharedReplyBudget(ReliableOutputAccount::new(OutputLimits {
            bytes: 1024 * 1024,
            reply_reserve: 0,
        }));
    let exhausted = create_entities(&mut host, 1, COUNT);
    let healthy = create_entities(&mut host, 2, COUNT);

    add_session_targets(&mut host, 1, 10, &exhausted);
    add_session_targets(&mut host, 2, 20, &healthy);
    subscribe_entities(&mut host, 2, 21, 5);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    for session in [1, 2] {
        while host.session_mut(session).unwrap().take_response().is_some() {}
    }

    // Leave room for a few events on the first connection only.
    let account = host.sessions[&1].reply_budget.0.clone();
    let filler = account
        .reserve(OutputCharge {
            entries: 0,
            bytes: 1024 * 1024 - account.usage().bytes - 4096,
        })
        .unwrap();
    delete_all(&mut host, 1, &exhausted);
    delete_all(&mut host, 2, &healthy);
    let failures = host.tick_worlds(0.0).unwrap();

    assert_eq!(failures.len(), 1, "{failures:?}");
    let (connection, reason) = &failures[0];
    assert_eq!(*connection, 1);
    assert!(
        reason.starts_with("connection congestion: Lifecycle delivery unavailable: Capacity"),
        "{reason}"
    );
    assert_eq!(
        account.status(),
        OutputStatus::Failed(OutputFailure::Capacity)
    );

    let peer = host.sessions[&2].reply_budget.0.clone();
    assert_eq!(peer.status(), OutputStatus::Open);
    let mut watched = Vec::new();
    let mut published = Vec::new();
    let mut pages = 0;
    while let Some(reply) = host.session_mut(2).unwrap().take_response() {
        match reply[24] {
            37 => {
                assert_eq!(reply[49], 1);
                watched.push(u64::from_le_bytes(reply[58..66].try_into().unwrap()));
            }
            17 => {
                pages += 1;
                let count = u32::from_le_bytes(reply[25..29].try_into().unwrap()) as usize;
                assert!(count <= ipp_protocol::MAX_LIFECYCLE_PUBLICATIONS);
                for event in reply[29..].as_chunks::<33>().0.iter().take(count) {
                    // Entity deleted.
                    assert_eq!(event[24], 3);
                    published.push(u64::from_le_bytes(event[25..33].try_into().unwrap()));
                }
            }
            _ => {}
        }
    }
    assert_eq!(watched.len(), COUNT as usize);
    assert!(watched.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        pages,
        (COUNT as usize).div_ceil(ipp_protocol::MAX_LIFECYCLE_PUBLICATIONS)
    );
    assert_eq!(published, healthy);
    let watch = host.sessions[&2].lifecycle_watch.as_ref().unwrap();
    assert!(watch.members.values().all(|entry| entry.member.is_active()));
    drop(filler);
}

fn usage(host: &Host<Platform>, profile: &str, stage: &str) -> OutputCharge {
    let usage = host.sessions[&1].reply_budget.0.usage();
    println!(
        "lifecycle-account profile={profile} stage={stage} entries={} bytes={}",
        usage.entries, usage.bytes,
    );
    usage
}

fn native_commands(host: &mut Host<Platform>, operations: Vec<ipp_core::Command>) {
    host.session_mut(1)
        .unwrap()
        .world
        .enqueue(ipp_core::Batch {
            id: 950,
            operations,
        })
        .unwrap();
    let report = host.runtime.frame(0.0).unwrap();
    assert!(report.worlds.iter().all(|(_, result)| {
        result
            .as_ref()
            .is_ok_and(|report| report.outcomes.iter().all(|outcome| outcome.result.is_ok()))
    }));
}

fn delete(entities: &[u64]) -> Vec<ipp_core::Command> {
    entities
        .iter()
        .map(|entity| ipp_core::Command::Delete {
            entity: ipp_core::EntityRef::Handle(ipp_core::EntityId::from_bits(*entity)),
        })
        .collect()
}

fn observation_account_boundaries(profile: &str, limits: Option<OutputLimits>) {
    let mut host = ready();
    if let Some(limits) = limits {
        assert_eq!(
            host.sessions[&1].reply_budget.0.usage(),
            OutputCharge::default()
        );
        host.sessions.get_mut(&1).unwrap().reply_budget =
            reliable_output::SharedReplyBudget(ReliableOutputAccount::new(limits));
    }
    let ordinary_limit = limits.map_or(reliable_output::ORDINARY_OUTPUT_BYTES, |limits| {
        limits.bytes - limits.reply_reserve
    });
    let account = host.sessions[&1].reply_budget.0.clone();
    native_commands(
        &mut host,
        (0..260)
            .map(|alias| ipp_core::Command::Create {
                alias,
                metadata: Default::default(),
                adopt: false,
            })
            .collect(),
    );
    let entities: Vec<_> = host
        .session_mut(1)
        .unwrap()
        .world
        .entities()
        .iter()
        .map(|entity| entity.id.to_bits())
        .collect();
    assert_eq!(entities.len(), 260);
    assert_eq!(
        usage(&host, profile, "before-request"),
        OutputCharge::default()
    );

    add_targets(&mut host, 45, &entities[..130]);
    assert_eq!(
        usage(&host, profile, "request-reserved"),
        OutputCharge {
            entries: 1,
            bytes: 256 + reliable_output::RESPONSE_METADATA_BYTES,
        }
    );
    host.session_mut(1).unwrap().prepare_request(0.0).unwrap();
    let prepared = usage(&host, profile, "prepared-before-apply");
    assert_eq!(prepared.entries, 1);
    let output = host.sessions[&1]
        .lifecycle_watch
        .as_ref()
        .unwrap()
        .output
        .clone();
    let ack_charge = output.acknowledgement_charge(130, 0).unwrap();
    assert_eq!(ack_charge.entries, 1);
    assert!(prepared.bytes > ack_charge.bytes);
    println!(
        "lifecycle-account profile={profile} ack-core-peak-bytes={}",
        ack_charge.bytes
    );

    let report = host.runtime.frame(0.0).unwrap();
    assert!(
        report.worlds.iter().all(|(_, result)| result
            .as_ref()
            .unwrap()
            .system_command_outcomes
            .is_empty())
    );
    let applied = usage(&host, profile, "applied-ack-queued");
    assert_eq!(applied.entries, 1);
    assert!(applied.bytes < prepared.bytes);
    assert!(host.drain_lifecycle_watches().is_empty());
    let ack = host.sessions[&1].outbox.pop_front().unwrap();
    assert_eq!(ack[24], 37);
    assert_eq!(u64::from_le_bytes(ack[8..16].try_into().unwrap()), 45);
    assert!(host.sessions[&1].outbox.pop_front().is_none());
    let handed = usage(&host, profile, "encoded-ack-inflight");
    assert_eq!(handed.entries, 1);
    assert!(handed.bytes < applied.bytes);
    assert_eq!(ack.reservation.borrow().bytes, ack.bytes.capacity());
    let queue_node_bytes = output.acknowledgement_charge(0, 0).unwrap().bytes
        - wire::LIFECYCLE_ACK_BYTES
        - reliable_output::RESPONSE_METADATA_BYTES;
    let ack_retained_bytes = ack.bytes.capacity() + reliable_output::RESPONSE_METADATA_BYTES;
    let metadata_bytes = handed.bytes - ack_retained_bytes;
    let command_bytes = std::mem::size_of::<LifecycleMembershipCommand>()
        + 130 * std::mem::size_of::<LifecycleWatchMember>();
    let pending_add_bytes = std::mem::size_of::<PendingAdd>()
        + reliable_output::RESPONSE_METADATA_BYTES
        + 130 * std::mem::size_of::<u64>();
    assert_eq!(
        applied.bytes,
        metadata_bytes + ack_charge.bytes + pending_add_bytes
    );
    assert_eq!(prepared.bytes - applied.bytes, command_bytes);
    println!(
        "lifecycle-account profile={profile} pointer-bits={} metadata-zero-entries-bytes={metadata_bytes} queue-node-bytes={queue_node_bytes} ack-wire-bytes={} ack-one-entry-bytes={ack_retained_bytes} prepared-command-zero-entries-bytes={command_bytes}",
        usize::BITS,
        ack.bytes.capacity(),
    );
    assert!(output.pop_front().is_none());

    native_commands(&mut host, delete(&entities[130..]));
    assert_eq!(
        usage(&host, profile, "after-130-unrelated-deletions"),
        handed
    );
    assert!(output.pop_front().is_none());

    native_commands(&mut host, delete(&entities[..1]));
    let first_queued = usage(&host, profile, "first-event-queued");
    let event = output.pop_front().unwrap();
    assert!(matches!(
        event.record().body,
        LifecycleWatchRecordBody::Event { .. }
    ));
    assert_eq!(event.charge().entries, 1);
    let event_bytes = event.charge().bytes;
    assert_eq!(
        event_bytes,
        queue_node_bytes
            + wire::LIFECYCLE_WATCH_ENCODING.event_bytes
            + reliable_output::RESPONSE_METADATA_BYTES
    );
    assert_eq!(
        first_queued,
        OutputCharge {
            entries: handed.entries + 1,
            bytes: handed.bytes + event_bytes
        }
    );
    assert_eq!(usage(&host, profile, "first-event-inflight"), first_queued);
    assert!(output.pop_front().is_none());

    let mut held = Vec::new();
    if first_queued.bytes + 129 * event_bytes <= ordinary_limit {
        native_commands(&mut host, delete(&entities[1..130]));
        assert_eq!(account.status(), OutputStatus::Open);
        let all_queued = usage(&host, profile, "129-more-relevant-queued");
        assert_eq!(
            all_queued,
            OutputCharge {
                entries: first_queued.entries + 129,
                bytes: first_queued.bytes + 129 * event_bytes,
            }
        );
        while let Some(delivery) = output.pop_front() {
            assert!(matches!(
                delivery.record().body,
                LifecycleWatchRecordBody::Event { .. }
            ));
            assert_eq!(delivery.charge().bytes, event_bytes);
            held.push(delivery);
        }
        assert_eq!(held.len(), 129);
        assert_eq!(usage(&host, profile, "129-more-inflight"), all_queued);
        assert!(
            host.sessions[&1]
                .lifecycle_watch
                .as_ref()
                .unwrap()
                .members
                .values()
                .all(|entry| entry.member.is_active())
        );
    } else {
        native_commands(&mut host, delete(&entities[1..130]));
        assert_eq!(
            account.status(),
            OutputStatus::Failed(OutputFailure::Capacity)
        );
        let failed = usage(&host, profile, "physical-account-failed");
        assert!(failed.bytes <= ordinary_limit);
        assert!(failed.bytes + event_bytes > ordinary_limit);
        while let Some(delivery) = output.pop_front() {
            assert!(matches!(
                delivery.record().body,
                LifecycleWatchRecordBody::Event { .. }
            ));
            held.push(delivery);
        }
        assert_eq!(held.len(), failed.entries - first_queued.entries);
        assert_eq!(usage(&host, profile, "all-events-inflight"), failed);
    }

    let before_ack_completion = account.usage();
    let ack_bytes = ack.bytes.capacity() + reliable_output::RESPONSE_METADATA_BYTES;
    drop(ack);
    assert_eq!(
        usage(&host, profile, "ack-completed"),
        OutputCharge {
            entries: before_ack_completion.entries - 1,
            bytes: before_ack_completion.bytes - ack_bytes,
        }
    );
    let before_event_completion = account.usage();
    drop(event);
    assert_eq!(
        usage(&host, profile, "first-event-completed"),
        OutputCharge {
            entries: before_event_completion.entries - 1,
            bytes: before_event_completion.bytes - event_bytes,
        }
    );
    drop(held);
    host.detach_world_session(1);
    drop(output);
    assert_eq!(account.usage(), OutputCharge::default());
}

#[test]
fn production_default_accounts_ack_metadata_and_every_inflight_event() {
    observation_account_boundaries("production-default-8MiB", None);
}

#[test]
fn explicitly_tight_fixture_fails_its_byte_budget() {
    observation_account_boundaries(
        "fixture-tight-256KiB",
        Some(OutputLimits {
            bytes: 256 * 1024,
            reply_reserve: 0,
        }),
    );
}

#[test]
fn roomy_account_retains_every_relevant_event() {
    observation_account_boundaries(
        "core-fixture-roomy-16MiB",
        Some(OutputLimits {
            bytes: 16 * 1024 * 1024,
            reply_reserve: 0,
        }),
    );
}

#[test]
fn large_membership_page_has_one_owned_ack_and_retained_handoff_credit() {
    let mut host = ready();
    add(&mut host, 19, 2048);
    host.tick(0.0).unwrap();
    let session = host.sessions.get_mut(&1).unwrap();
    let watch = session.lifecycle_watch.as_ref().unwrap();
    assert_eq!(watch.members.len(), 2048);
    assert!(watch.members.values().all(|entry| entry.member.is_active()));
    let account = session.reply_budget.0.clone();
    let mut replies = Vec::new();
    while let Some(reply) = session.outbox.pop_front() {
        replies.push(reply);
    }
    let correlated: Vec<_> = replies
        .iter()
        .filter(|reply| u64::from_le_bytes(reply[8..16].try_into().unwrap()) == 19)
        .collect();
    assert_eq!(correlated.len(), 1);
    assert_eq!(correlated[0][24], 37);
    assert_eq!(&correlated[0][16..24], &0u64.to_le_bytes());
    let retained = account.usage();
    assert!(retained.bytes > correlated[0].len());
    drop(correlated);
    drop(replies);
    assert!(account.usage().bytes < retained.bytes);
    assert!(account.usage().bytes > 0);
    host.detach_world_session(1);
    assert_eq!(account.usage().bytes, 0);
}

#[test]
fn prepared_command_drop_has_only_cancelled_ack_and_no_generation_activation() {
    let mut host = ready();
    add(&mut host, 21, 65);
    host.session_mut(1).unwrap().prepare_request(0.0).unwrap();
    host.session_mut(1).unwrap().world.release_system_session(1);
    assert!(host.drain_lifecycle_watches().is_empty());
    let session = host.sessions.get_mut(&1).unwrap();
    assert!(session.lifecycle_watch.as_ref().unwrap().members.is_empty());
    let reply = session.outbox.pop_front().unwrap();
    assert_eq!(reply[24], 37);
    assert_eq!(u64::from_le_bytes(reply[8..16].try_into().unwrap()), 21);
    assert_eq!(&reply[49..], &[0, 0, 0, 2]);
    assert!(session.outbox.pop_front().is_none());
    assert!(session.replies.is_empty());
    assert!(session.request_origins.is_empty());
    assert!(session.reply_reservations.is_empty());
}

fn diagnostic_reply(
    host: &mut Host<Platform>,
    request: u64,
    world: ipp_protocol::references::WorldReference,
    output: u64,
) -> ReliableResponse {
    let mut bytes = 1u64.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(36);
    bytes.extend(world.id.to_le_bytes());
    bytes.extend(world.incarnation.to_le_bytes());
    bytes.extend(output.to_le_bytes());
    host.session_mut(1).unwrap().receive(&bytes).unwrap();
    host.tick(0.0).unwrap();
    let reply = host.session_mut(1).unwrap().take_response().unwrap();
    assert_eq!(
        u64::from_le_bytes(reply[8..16].try_into().unwrap()),
        request
    );
    while host.session_mut(1).unwrap().take_response().is_some() {}
    reply
}

#[test]
fn diagnostic_query_requires_live_exact_world_session_endpoint_and_holds_delivery_credit() {
    let mut host = ready();
    let world = host.session_mut(1).unwrap().world.world_ref().into();
    let absent = diagnostic_reply(&mut host, 20, world, 1);
    assert_eq!(&absent[24..27], &[255, 1, 0]);
    drop(absent);
    assert!(host.sessions[&1].lifecycle_watch.is_none());
    add(&mut host, 21, 1);
    host.tick(0.0).unwrap();
    while host.session_mut(1).unwrap().take_response().is_some() {}
    let output = host.sessions[&1]
        .lifecycle_watch
        .as_ref()
        .unwrap()
        .identity
        .unwrap();
    let foreign = diagnostic_reply(
        &mut host,
        22,
        ipp_protocol::references::WorldReference {
            incarnation: world.incarnation + 1,
            ..world
        },
        output,
    );
    assert_eq!(&foreign[24..27], &[255, 1, 0]);
    drop(foreign);
    let stale = diagnostic_reply(&mut host, 23, world, output + 1);
    assert_eq!(&stale[24..27], &[255, 1, 0]);
    drop(stale);
    let account = host.sessions[&1].reply_budget.0.clone();
    let before = account.usage();
    let sample = diagnostic_reply(&mut host, 24, world, output);
    assert_eq!(sample.len(), 83);
    assert_eq!(sample[24], 38);
    assert_eq!(&sample[16..24], &[0; 8]);
    assert_eq!(
        u64::from_le_bytes(sample[41..49].try_into().unwrap()),
        output
    );
    assert_eq!(account.usage().entries, before.entries + 1);
    assert_eq!(
        account.usage().bytes,
        before.bytes + sample.bytes.capacity() + reliable_output::RESPONSE_METADATA_BYTES
    );
    drop(sample);
    assert_eq!(account.usage(), before);
    host.sessions[&1]
        .lifecycle_watch
        .as_ref()
        .unwrap()
        .output
        .close();
    let closed = diagnostic_reply(&mut host, 25, world, output);
    assert_eq!(&closed[24..27], &[255, 1, 0]);
}

/// Lifecycle watch responses of session 1 whose record has `tag`.
fn watch_records(host: &mut Host<Platform>, tag: u8) -> Vec<Vec<u8>> {
    let mut records = Vec::new();
    while let Some(reply) = host.session_mut(1).unwrap().take_response() {
        if reply[24] == 37 && reply[49] == tag {
            records.push(reply.to_vec());
        }
    }
    records
}

/// The one Scalar value a value record reports, or `None` when it reports absence.
fn reported_scalar(record: &[u8]) -> Option<f32> {
    match record[66] {
        0 => None,
        _ => {
            assert_eq!(u32::from_le_bytes(record[67..71].try_into().unwrap()), 1);
            assert_eq!(
                record[75],
                ipp_core::components::schema::FieldKind::F32 as u8
            );
            Some(f32::from_le_bytes(record[76..80].try_into().unwrap()))
        }
    }
}

#[test]
fn value_members_report_current_then_changed_then_absent_values_once() {
    use ipp_core::{Command, ComponentValue, EntityRef, FieldValue, FieldWrite};

    let mut host = ready();
    let scalar = |value| {
        ComponentValue::Scalar(ipp_core::components::Scalar {
            value,
        })
    };
    native_commands(
        &mut host,
        vec![
            Command::Create {
                alias: 0,
                metadata: ipp_core::EntityMetadata {
                    symbolic_id: Some("valued".into()),
                    classes: Vec::new(),
                },
                adopt: false,
            },
            Command::insert_value(EntityRef::Alias(0), scalar(1.5)),
        ],
    );
    let entity = host
        .session_mut(1)
        .unwrap()
        .world
        .lookup_id("valued")
        .unwrap();
    let offset = std::mem::offset_of!(ipp_core::components::Scalar, value) as u32;
    let world = host.session_mut(1).unwrap().world.world_ref();
    let mut bytes = 1u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.push(35);
    bytes.extend_from_slice(&world.id().0.to_le_bytes());
    bytes.extend_from_slice(&world.incarnation().to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(2);
    bytes.extend_from_slice(&entity.to_bits().to_le_bytes());
    bytes.extend_from_slice(&ComponentValue::SCALAR.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.push(128);
    host.session_mut(1).unwrap().receive(&bytes).unwrap();
    host.tick(0.0).unwrap();

    // The sole ACK echoes the value target inside its charged encoding bound.
    let mut responses = Vec::new();
    while let Some(reply) = host.session_mut(1).unwrap().take_response() {
        if reply[24] == 37 {
            responses.push(reply.to_vec());
        }
    }
    let ack = responses
        .iter()
        .find(|reply| reply[49] == 0)
        .expect("value member acknowledgement");
    assert_eq!(u64::from_le_bytes(ack[8..16].try_into().unwrap()), 7);
    let target = ipp_core::systems::lifecycle_publisher::LifecycleWatchTarget::Value(
        entity,
        ComponentValue::SCALAR,
        vec![offset].into(),
    );
    let encoding = wire::LIFECYCLE_WATCH_ENCODING;
    assert!(
        ack.len() <= encoding.acknowledgement_bytes + encoding.baseline_capacity(&target).unwrap()
    );
    let output = host.sessions[&1]
        .lifecycle_watch
        .as_ref()
        .unwrap()
        .output
        .clone();
    assert_eq!(
        output.acknowledgement_charge(1, 1).unwrap().bytes
            - output.acknowledgement_charge(1, 0).unwrap().bytes,
        encoding.baseline_field_bytes
    );

    // The first Observe after the ACK reports the current value.
    let mut values: Vec<_> = responses
        .iter()
        .filter(|reply| reply[49] == 3)
        .map(|record| reported_scalar(record))
        .collect();
    if values.is_empty() {
        host.tick(0.0).unwrap();
        values = watch_records(&mut host, 3)
            .iter()
            .map(|record| reported_scalar(record))
            .collect();
    }
    assert_eq!(values, [Some(1.5)]);

    let apply = |host: &mut Host<Platform>, operations| {
        host.session_mut(1)
            .unwrap()
            .world
            .enqueue(ipp_core::Batch {
                id: 951,
                operations,
            })
            .unwrap();
        host.tick(0.0).unwrap();
        watch_records(host, 3)
    };
    let set = |value| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::SCALAR,
        field: FieldWrite {
            offset,
            value: FieldValue::F32(value),
        },
    };

    // One record per changed frame, none for an equal write or an idle frame.
    let changed = apply(&mut host, vec![set(2.0), set(2.5)]);
    assert_eq!(changed.len(), 1);
    assert_eq!(reported_scalar(&changed[0]), Some(2.5));
    let tick = u64::from_le_bytes(changed[0][58..66].try_into().unwrap());
    assert_eq!(
        tick,
        host.session_mut(1).unwrap().world.tick(),
        "records carry the evaluated tick"
    );
    assert!(apply(&mut host, vec![set(2.5)]).is_empty());
    assert!(apply(&mut host, Vec::new()).is_empty());

    // Absence is reported once.
    let removed = apply(
        &mut host,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::SCALAR,
        }],
    );
    assert_eq!(removed.len(), 1);
    assert_eq!(reported_scalar(&removed[0]), None);
    assert!(apply(&mut host, Vec::new()).is_empty());
}
