use super::*;

#[path = "asset_sources_tests.rs"]
mod asset_sources;

#[path = "ended_session_tests.rs"]
mod ended_sessions;

#[path = "graph_transfer_tests.rs"]
mod graph_transfers;

#[path = "gui_admission_tests.rs"]
mod gui_admission;
#[path = "gui_observations_tests.rs"]
mod gui_observations;

#[path = "gui_output_pressure_tests.rs"]
mod gui_output_pressure;

struct TestHostServices;

/// Exact action target of one control component, as a client learns it.
fn gui_target(
    host: &mut Host<TestHostServices>,
    world: ipp_core::WorldId,
    entity: ipp_core::EntityId,
    component: u16,
) -> ipp_core::systems::gui::local::GuiEntityTarget {
    let world = host.runtime.world_mut(world).unwrap();
    ipp_core::systems::gui::local::GuiEntityTarget {
        world: world.world_ref(),
        entity,
        component,
        incarnation: world.component_incarnation(entity, component).unwrap(),
    }
}

/// Length-prefixed `request-gui-action` payload for a fixed-size action.
fn gui_action_payload(
    target: &ipp_core::systems::gui::local::GuiEntityTarget,
    operation: u8,
) -> Vec<u8> {
    // One final batch page: batch identity, last flag and one GuiAction command.
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.push(1);
    payload.extend(1u32.to_le_bytes());
    payload.push(24);
    payload.push(0);
    payload.extend(target.entity.to_bits().to_le_bytes());
    payload.extend(target.component.to_le_bytes());
    payload.extend(target.incarnation.to_le_bytes());
    payload.push(operation);
    payload
}

impl HostServices for TestHostServices {
    const NAME: &'static str = "connection-test";

    fn initialize(_host: &mut ipp_core::HostRuntime) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _host: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

fn open(host: &mut Host<TestHostServices>, id: u64) {
    host.open_connection(id).unwrap();
    host.receive_connection(id, &ipp_protocol::HELLO).unwrap();
    host.take_connection_response(id).unwrap();
}

fn control(host: &mut Host<TestHostServices>, id: u64, body: HostRequestBody) -> HostResponseBody {
    while host.take_connection_response(id).is_some() {}
    host.receive_connection(
        id,
        &host::encode_host_request(&HostRequest {
            connection: id,
            request_id: 1,
            body,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    host::decode_host_response(&host.take_connection_response(id).unwrap(), id)
        .unwrap()
        .body
}

fn create_and_open(
    host: &mut Host<TestHostServices>,
    id: u64,
    request: HostRequestBody,
) -> HostResponseBody {
    let response = control(host, id, request);
    let HostResponseBody::Created {
        reference,
        ..
    } = response
    else {
        panic!("World not created: {response:?}")
    };
    control(host, id, HostRequestBody::OpenWorld(reference))
}

#[test]
fn closing_peer_session_preserves_originating_save_and_world_lifetime() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session: first,
        reference,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(Vec::new()),
            temporary: false,
        },
    )
    else {
        panic!("first session")
    };
    let HostResponseBody::Attached {
        session: second,
        ..
    } = control(&mut host, 1, HostRequestBody::OpenWorld(reference))
    else {
        panic!("second session")
    };
    let HostResponseBody::Transfer {
        ..
    } = control(
        &mut host,
        1,
        HostRequestBody::SaveWorld {
            session: first,
        },
    )
    else {
        panic!("save")
    };
    assert!(matches!(
        control(
            &mut host,
            1,
            HostRequestBody::DetachWorld {
                session: second
            }
        ),
        HostResponseBody::Complete
    ));
    assert_eq!(
        host.connections.states[&1]
            .transfer
            .as_ref()
            .unwrap()
            .origin,
        Some(first)
    );
    assert!(host.sessions.contains_key(&first));
    assert!(matches!(
        control(
            &mut host,
            1,
            HostRequestBody::DetachWorld {
                session: first
            }
        ),
        HostResponseBody::Complete
    ));
    assert!(host.connections.states[&1].transfer.is_none());
    assert!(host.runtime().world_ref(WorldId(reference.id)).is_some());
    assert_eq!(host.connections.persistence.reserved, 0);
}

#[test]
fn contract_requests_follow_the_hello_and_are_charged_as_replies_until_delivered() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    host.open_connection(1).unwrap();
    assert!(
        host.receive_connection(1, &ipp_protocol::CONTRACT_REQUEST)
            .unwrap_err()
            .contains("hello magic")
    );
    host.close_connection(1);

    open(&mut host, 2);
    let idle = host.connections.states[&2].reply_budget.0.usage().bytes;
    let mut admitted = 0;
    let refusal = loop {
        match host.receive_connection(2, &ipp_protocol::CONTRACT_REQUEST) {
            Ok(()) => admitted += 1,
            Err(error) => break error,
        }
    };
    assert!(refusal.contains("congestion"), "{refusal}");
    assert!((1..=crate::MAX_PENDING).contains(&admitted));
    assert_eq!(host.connections.states[&2].reply_entries(), admitted);
    assert!(
        host.connections.states[&2].reply_budget.0.usage().bytes
            >= idle + admitted * ipp_protocol::export_contract().len()
    );

    // Each reply stays charged until the transport drops it after delivery.
    let replies: Vec<_> = std::iter::from_fn(|| host.take_connection_response(2)).collect();
    assert_eq!(replies.len(), admitted);
    assert!(
        replies
            .iter()
            .all(|reply| reply.bytes == ipp_protocol::contract_reply())
    );
    assert!(
        host.receive_connection(2, &ipp_protocol::CONTRACT_REQUEST)
            .is_err()
    );
    drop(replies);
    assert_eq!(
        host.connections.states[&2].reply_budget.0.usage().bytes,
        idle
    );
    host.receive_connection(2, &ipp_protocol::CONTRACT_REQUEST)
        .unwrap();
}

#[test]
fn multiplex_reliable_output_budget_is_per_connection_not_per_session() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    for connection in [1, 2] {
        open(&mut host, connection);
        create_and_open(
            &mut host,
            connection,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions::new(Vec::new()),
                temporary: false,
            },
        );
    }
    for _ in 0..3 {
        create_and_open(
            &mut host,
            1,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions::new(Vec::new()),
                temporary: false,
            },
        );
    }
    while host.take_connection_response(1).is_some() {}
    while host.take_connection_response(2).is_some() {}
    let mut congested = false;
    let message = "reliable event must not coalesce ".repeat(60);
    for _ in 0..=crate::reliable_output::MAX_OUTPUT_BYTES / message.len() {
        let id = *host.connections.states[&1].sessions.first().unwrap();
        if let Err(error) = host.session_mut(id).unwrap().queue_response(
            0,
            ipp_protocol::ResponseBody::RuntimeFailure {
                scope: ipp_protocol::RuntimeFailureScope::Resource,
                faulted: false,
                message: message.clone(),
            },
        ) {
            assert!(error.contains("congestion"), "{error}");
            congested = true;
            break;
        }
        let failures = host.tick_worlds(0.0).unwrap();
        while host.take_connection_response(2).is_some() {}
        let total = host.connections.states[&1].reply_budget.0.usage().bytes;
        assert!(
            total <= crate::reliable_output::ORDINARY_OUTPUT_BYTES,
            "uncorrelated output entered the reply reserve or exceeded the shared budget"
        );
        if !failures.is_empty() {
            assert!(
                failures
                    .iter()
                    .all(|(connection, error)| *connection == 1 && error.contains("congestion"))
            );
            congested = true;
            break;
        }
    }
    assert!(
        congested,
        "undrained multiplex connection never exhausted its bounded output"
    );
    host.close_connection(1);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert!(host.take_connection_response(2).is_some());
}

#[test]
fn multiplex_progress_remains_bounded_through_physical_completion() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    for _ in 0..33 {
        create_and_open(
            &mut host,
            1,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions::new(Vec::new()),
                temporary: false,
            },
        );
    }
    while host.take_connection_response(1).is_some() {}
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    let account = host.connections.states[&1].reply_budget.0.clone();
    let mut copies = Vec::new();
    while let Some(response) = host.take_connection_response(1) {
        assert_eq!(response[24], 4);
        let mut copy = response.prepare_copy().ok().unwrap();
        copy.release_source();
        copies.push(copy);
    }
    assert_eq!(copies.len(), 1);
    let charge = account.usage();
    for _ in 0..100 {
        assert!(host.tick_worlds(0.0).unwrap().is_empty());
        assert!(host.take_connection_response(1).is_none());
        assert_eq!(account.usage(), charge);
    }
    drop(copies);
    let mut delivered = 0;
    while let Some(response) = host.take_connection_response(1) {
        assert_eq!(response[24], 4);
        delivered += 1;
    }
    assert_eq!(delivered, 33);
    assert_eq!(account.usage(), Default::default());
}

#[test]
fn deferred_progress_is_fair_despite_sibling_replies() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    for _ in 0..3 {
        create_and_open(
            &mut host,
            1,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions::new(Vec::new()),
                temporary: false,
            },
        );
    }
    while host.take_connection_response(1).is_some() {}
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    let sessions: Vec<_> = host.connections.states[&1]
        .sessions
        .iter()
        .copied()
        .collect();

    let mut progress = host.take_connection_response(1).unwrap();
    let mut delivered = BTreeMap::new();
    for _ in 0..6 {
        assert_eq!(progress[24], 4);
        let session = u64::from_le_bytes(progress[..8].try_into().unwrap());
        let tick = u64::from_le_bytes(progress[16..24].try_into().unwrap());
        assert!(
            delivered
                .insert(session, tick)
                .is_none_or(|old| old <= tick)
        );
        assert!(host.tick_worlds(0.0).unwrap().is_empty());
        host.session_mut(sessions[0])
            .unwrap()
            .queue_response(
                0,
                ipp_protocol::ResponseBody::RuntimeFailure {
                    scope: ipp_protocol::RuntimeFailureScope::Resource,
                    faulted: false,
                    message: "sibling semantic traffic".into(),
                },
            )
            .unwrap();
        let reply = host.take_connection_response(1).unwrap();
        assert_eq!(&reply[..8], &sessions[0].to_le_bytes());
        assert_ne!(reply[24], 4);
        drop(reply);
        assert!(host.take_connection_response(1).is_none());
        drop(progress);
        progress = host.take_connection_response(1).unwrap();
    }
    assert_eq!(delivered.len(), sessions.len());
}

#[test]
fn selected_system_names_resolve_against_registered_factories_and_readiness_is_world_specific() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let unknown = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(vec!["ipp.not-registered".into()]),
            temporary: false,
        },
    );
    assert!(
        matches!(unknown, HostResponseBody::Error(message) if message.contains("Unknown registered system"))
    );
    assert_eq!(host.runtime().world_ids().len(), 0);

    // There is no default selection: a request without one is refused, and
    // the connection stays usable.
    let absent = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions {
                selected_systems: None,
                ..host::WorldCreateOptions::new(Vec::new())
            },
            temporary: false,
        },
    );
    assert_eq!(
        absent,
        HostResponseBody::Error(super::service::WORLD_SELECTION_REQUIRED.into())
    );
    assert_eq!(host.runtime().world_ids().len(), 0);

    let names = host
        .runtime()
        .system_ids()
        .map(|system| system.0.to_owned())
        .collect::<Vec<_>>();
    let HostResponseBody::Attached {
        world,
        manifest,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(names),
            temporary: false,
        },
    )
    else {
        panic!("selected World should attach")
    };
    assert_eq!(
        manifest,
        host::WorldManifest::from_core(host.runtime().world_manifest(world.id).unwrap())
    );
    assert!(manifest.operations.contains(&0));
}

#[test]
fn source_delivery_fences_sessions_and_discards_partial_input_on_error_or_disconnect() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(Vec::new()),
            temporary: false,
        },
    )
    else {
        panic!("attachment expected")
    };
    while host.take_connection_response(1).is_some() {}
    let frame = |session: u64, id: u64, tag: u8, payload: &[u8]| {
        let mut bytes = ipp_protocol::asset_source::REQUEST_MAGIC.to_vec();
        bytes.extend(session.to_le_bytes());
        bytes.extend(id.to_le_bytes());
        bytes.push(tag);
        bytes.extend(payload);
        bytes
    };
    let name = format!("client://{session}/fixture#immutable");
    let mut begin = 1u32.to_le_bytes().to_vec();
    begin.extend((name.len() as u32).to_le_bytes());
    begin.extend(name.as_bytes());
    begin.extend(0u32.to_le_bytes());
    begin.extend(4u64.to_le_bytes());
    let tick = host.session_mut(session).unwrap().world().tick();
    host.receive_connection(1, &frame(session, 1, 0, &begin))
        .unwrap();
    assert_eq!(host.take_connection_response(1).unwrap()[20], 0);
    assert_eq!(host.sessions[&session].source_transfers.len(), 1);
    assert!(host.sessions[&session].pending.is_empty());
    assert_eq!(host.session_mut(session).unwrap().world().tick(), tick);
    assert!(
        host.receive_connection(1, &frame(session + 1, 2, 2, &1u64.to_le_bytes()))
            .is_err()
    );
    let mut chunk = 1u64.to_le_bytes().to_vec();
    chunk.extend(1u64.to_le_bytes()); // Wrong offset discards staging.
    chunk.extend(2u32.to_le_bytes());
    chunk.extend([1, 2]);
    host.receive_connection(1, &frame(session, 2, 1, &chunk))
        .unwrap();
    assert_eq!(host.take_connection_response(1).unwrap()[20], 1);
    assert!(host.sessions[&session].source_transfers.is_empty());
    assert!(host.sessions[&session].client_sources.is_empty());
    host.receive_connection(1, &frame(session, 3, 0, &begin))
        .unwrap();
    host.take_connection_response(1).unwrap();
    host.receive_connection(1, &frame(session, 4, 2, &3u64.to_le_bytes()))
        .unwrap();
    assert_eq!(host.take_connection_response(1).unwrap()[20], 1);
    assert!(host.sessions[&session].source_transfers.is_empty());
    assert!(host.sessions[&session].client_sources.is_empty());
    host.receive_connection(1, &frame(session, 5, 0, &begin))
        .unwrap();
    assert_eq!(host.sessions[&session].source_transfers.len(), 1);
    assert!(host.close_connection(1));
    assert!(!host.sessions.contains_key(&session));
}

#[test]
fn source_replies_respect_existing_control_plane_reservations() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(Vec::new()),
            temporary: false,
        },
    )
    else {
        panic!("attachment expected")
    };
    while host.take_connection_response(1).is_some() {}

    for request in 1..=crate::MAX_PENDING as u64 {
        let mut bytes = session.to_le_bytes().to_vec();
        bytes.extend(request.to_le_bytes());
        bytes.push(1); // A complete, empty batch.
        bytes.extend((request as u32).to_le_bytes());
        bytes.push(1);
        bytes.extend(0u32.to_le_bytes());
        host.receive_connection(1, &bytes).unwrap();
    }

    let name = format!("client://{session}/fixture#congested");
    let mut source = ipp_protocol::asset_source::REQUEST_MAGIC.to_vec();
    source.extend(session.to_le_bytes());
    source.extend(999u64.to_le_bytes());
    source.push(0);
    source.extend(1u32.to_le_bytes());
    source.extend((name.len() as u32).to_le_bytes());
    source.extend(name.as_bytes());
    source.extend(0u32.to_le_bytes());
    source.extend(0u64.to_le_bytes());

    assert!(host.receive_connection(1, &source).is_err());
    assert!(host.sessions[&session].source_transfers.is_empty());
    assert_eq!(host.connections.states[&1].outbox.len(), 0);

    host.connections
        .states
        .get_mut(&1)
        .unwrap()
        .pending
        .pop_front();
    host.receive_connection(1, &source).unwrap();
    assert_eq!(host.sessions[&session].source_transfers.len(), 1);
    assert_eq!(host.take_connection_response(1).unwrap()[20], 0);
}

#[test]
fn complete_source_delivery_progresses_while_a_world_batch_is_open() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(Vec::new()),
            temporary: false,
        },
    )
    else {
        panic!("attachment expected")
    };
    while host.take_connection_response(1).is_some() {}

    let world_frame = |request: u64, tag: u8, suffix: &[u8]| {
        let mut bytes = session.to_le_bytes().to_vec();
        bytes.extend(request.to_le_bytes());
        bytes.push(tag);
        bytes.extend(suffix);
        bytes
    };
    let page = |last: bool| {
        let mut bytes = 5u32.to_le_bytes().to_vec();
        bytes.push(u8::from(last));
        bytes.extend(0u32.to_le_bytes());
        bytes
    };
    host.receive_connection(1, &world_frame(0, 1, &page(false)))
        .unwrap();
    host.tick(0.0).unwrap();
    while host.take_connection_response(1).is_some() {}
    let open_tick = host.session_mut(session).unwrap().world().tick();

    let name = format!("client://{session}/fixture#held-world");
    let mut payload = b"IPPT".to_vec();
    for value in [3u32, 1, 1] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend([32, 64, 96, 255]);
    let source_frame = |request: u64, tag: u8, suffix: &[u8]| {
        let mut bytes = ipp_protocol::asset_source::REQUEST_MAGIC.to_vec();
        bytes.extend(session.to_le_bytes());
        bytes.extend(request.to_le_bytes());
        bytes.push(tag);
        bytes.extend(suffix);
        bytes
    };
    let mut begin = 2u32.to_le_bytes().to_vec();
    begin.extend((name.len() as u32).to_le_bytes());
    begin.extend(name.as_bytes());
    begin.extend(0u32.to_le_bytes());
    begin.extend((payload.len() as u64).to_le_bytes());
    host.receive_connection(1, &source_frame(3, 0, &begin))
        .unwrap();
    host.take_connection_response(1).unwrap();
    let mut chunk = 3u64.to_le_bytes().to_vec();
    chunk.extend(0u64.to_le_bytes());
    chunk.extend((payload.len() as u32).to_le_bytes());
    chunk.extend(payload);
    host.receive_connection(1, &source_frame(4, 1, &chunk))
        .unwrap();
    host.take_connection_response(1).unwrap();
    host.receive_connection(1, &source_frame(5, 2, &3u64.to_le_bytes()))
        .unwrap();
    host.take_connection_response(1).unwrap();

    host.tick(0.0).unwrap();
    assert_eq!(
        host.session_mut(session).unwrap().world().tick(),
        open_tick + 1,
        "an open batch never holds its World"
    );
    let resources = host
        .session_mut(session)
        .unwrap()
        .world()
        .resource_snapshots();
    assert_eq!(resources.len(), 1);
    assert_eq!(&*resources[0].source, name);
    assert_eq!(resources[0].status, ipp_core::AssetResourceStatus::Loaded);

    host.receive_connection(1, &world_frame(6, 1, &page(true)))
        .unwrap();
    host.tick(0.0).unwrap();
    let replies: Vec<_> = std::iter::from_fn(|| host.take_connection_response(1))
        .filter(|reply| reply[24] == 1)
        .collect();
    assert_eq!(replies.len(), 1);
    assert_eq!(&replies[0][8..16], &6u64.to_le_bytes());
}

#[test]
fn named_source_delivery_preserves_literal_names_and_rejects_foreign_or_duplicate_ownership() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(Vec::new()),
            temporary: false,
        },
    )
    else {
        panic!("attachment expected")
    };
    while host.take_connection_response(1).is_some() {}
    let mut request = 0u64;
    let mut send = |host: &mut Host<TestHostServices>, tag: u8, suffix: &[u8]| {
        request += 1;
        let mut bytes = ipp_protocol::asset_source::REQUEST_MAGIC.to_vec();
        bytes.extend(session.to_le_bytes());
        bytes.extend(request.to_le_bytes());
        bytes.push(tag);
        bytes.extend(suffix);
        host.receive_connection(1, &bytes).unwrap();
        host.take_connection_response(1).unwrap()[20]
    };
    let descriptor = |name: &str| {
        let mut bytes = 2u32.to_le_bytes().to_vec();
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes
    };
    let name = format!("client://{session}/scene/glow#literal%23guid");
    let mut payload = b"IPPT".to_vec();
    for value in [3u32, 1, 1] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend([32, 64, 96, 255]);
    let mut begin = descriptor(&name);
    begin.extend((payload.len() as u64).to_le_bytes());
    assert_eq!(send(&mut host, 0, &begin), 0);
    let mut chunk = 1u64.to_le_bytes().to_vec();
    chunk.extend(0u64.to_le_bytes());
    chunk.extend((payload.len() as u32).to_le_bytes());
    chunk.extend(payload);
    assert_eq!(send(&mut host, 1, &chunk), 0);
    assert!(host.sessions[&session].client_sources.is_empty());
    assert_eq!(send(&mut host, 2, &1u64.to_le_bytes()), 0);
    host.tick(0.0).unwrap();
    let resources = host
        .session_mut(session)
        .unwrap()
        .world()
        .resource_snapshots();
    assert_eq!(resources.len(), 1);
    assert_eq!(&*resources[0].source, name);
    assert_eq!(resources[0].status, ipp_core::AssetResourceStatus::Loaded);
    while host.take_connection_response(1).is_some() {}
    assert_eq!(send(&mut host, 0, &begin), 1);
    let mut foreign = descriptor(&format!(
        "client://{}/scene/glow#literal%23guid",
        session + 1
    ));
    foreign.extend(0u64.to_le_bytes());
    assert_eq!(send(&mut host, 0, &foreign), 1);
    assert_eq!(send(&mut host, 4, &descriptor(&name)), 0);
    host.tick(0.0).unwrap();
    assert!(
        host.session_mut(session)
            .unwrap()
            .world()
            .resource_snapshots()
            .is_empty()
    );
    assert_eq!(host.sessions[&session].client_sources.len(), 1);
    assert!(
        !host.sessions[&session].client_sources
            [&ipp_core::services::asset_management::AssetSource {
                kind: ipp_core::services::asset_management::AssetTypeId(2),
                uri: name.as_str().into(),
                variant: 0,
            }]
            .active
    );
    assert_eq!(send(&mut host, 0, &begin), 1);
}

#[test]
fn a_save_between_pages_captures_none_of_the_open_batch() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(Vec::new()),
            temporary: true,
        },
    )
    else {
        panic!("World not attached")
    };
    while host.take_connection_response(1).is_some() {}

    let page = |request: u64, last: bool| {
        let mut bytes = session.to_le_bytes().to_vec();
        bytes.extend_from_slice(&request.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.push(u8::from(last));
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1); // Create a distinct alias without metadata.
        bytes.extend_from_slice(&(request as u32).to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(0);
        bytes
    };
    host.receive_connection(1, &page(0, false)).unwrap();
    host.receive_connection(
        1,
        &host::encode_host_request(&HostRequest {
            connection: 1,
            request_id: 13,
            body: HostRequestBody::SaveWorld {
                session,
            },
        })
        .unwrap(),
    )
    .unwrap();
    host.tick(0.0).unwrap();
    let saved = std::iter::from_fn(|| host.take_connection_response(1))
        .find(|reply| reply.starts_with(host::HOST_RESPONSE_MAGIC))
        .expect("save completes while the batch is open");
    let saved = host::decode_host_response(&saved, 1).unwrap();
    assert_eq!(saved.request_id, 13);
    assert!(!matches!(saved.body, HostResponseBody::Error(_)));
    assert!(
        host.session_mut(session)
            .unwrap()
            .world()
            .entities()
            .is_empty()
    );

    host.receive_connection(1, &page(14, true)).unwrap();
    host.tick(0.0).unwrap();
    assert_eq!(
        host.session_mut(session).unwrap().world().entities().len(),
        2,
        "the batch applies as one whole at its final page"
    );
}

#[test]
fn shared_world_updates_once_and_retained_worlds_keep_ticking_without_sessions() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    assert_eq!(host.runtime().world_ids().len(), 0);
    let HostResponseBody::Attached {
        world,
        session: first,
        reference,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: ipp_protocol::host::WorldCreateOptions {
                symbolic_id: "shared".into(),
                ..ipp_protocol::host::WorldCreateOptions::new(Vec::new())
            },
            temporary: false,
        },
    )
    else {
        panic!("World not attached")
    };
    let HostResponseBody::Attached {
        session: second,
        ..
    } = control(&mut host, 2, HostRequestBody::OpenWorld(reference))
    else {
        panic!("World not attached")
    };
    assert_ne!(first, second);
    let tick = host.runtime_mut().world_mut(world.id).unwrap().tick();
    host.tick_worlds(0.5).unwrap();
    let world = host.runtime_mut().world_mut(world.id).unwrap();
    assert_eq!(world.time(), 0.5);
    assert_eq!(world.tick(), tick + 1);
    let id = world.id();
    drop(world);
    host.close_connection(1);
    host.close_connection(2);
    host.tick_worlds(0.25).unwrap();
    assert_eq!(host.runtime_mut().world_mut(id).unwrap().time(), 0.75);
}

#[test]
fn sessions_are_not_reused_by_another_host_in_the_same_process() {
    let mut sessions = std::collections::BTreeSet::new();
    for _ in 0..3 {
        let mut host = Host::<TestHostServices>::new().unwrap();
        open(&mut host, 1);
        let HostResponseBody::Attached {
            session,
            ..
        } = create_and_open(
            &mut host,
            1,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions::new(Vec::new()),
                temporary: true,
            },
        )
        else {
            panic!("World not attached")
        };
        assert!(sessions.insert(session));
        host.close_connection(1);
        assert_eq!(host.runtime().world_ids().len(), 0);
    }
}

#[test]
fn idle_transfers_charge_only_buffers_and_expire_after_accepted_progress() {
    use std::time::Duration;
    let mut host = Host::<TestHostServices>::new().unwrap();
    for id in 1..=3 {
        open(&mut host, id);
    }
    let begin = |host: &mut Host<TestHostServices>, id| {
        let HostResponseBody::Transfer {
            job,
        } = control(
            host,
            id,
            HostRequestBody::BeginWorldLoad {
                bytes: 64 << 20,
            },
        )
        else {
            panic!("load did not begin");
        };
        job
    };
    let first = begin(&mut host, 1);
    let second = begin(&mut host, 2);
    assert_eq!(host.connections.persistence.reserved, 0);
    let write = |host: &mut Host<TestHostServices>, id, job, offset| {
        control(
            host,
            id,
            HostRequestBody::WriteWorldLoad {
                job,
                offset,
                bytes: vec![0; 64 << 10],
            },
        )
    };
    assert!(matches!(
        write(&mut host, 1, first, 0),
        HostResponseBody::Complete
    ));
    assert!(matches!(
        write(&mut host, 2, second, 0),
        HostResponseBody::Complete
    ));
    assert_eq!(host.connections.persistence.reserved, 128 << 10);
    host.maintain_connections(Duration::from_secs(29));
    assert!(matches!(
        write(&mut host, 2, second, 64 << 10),
        HostResponseBody::Complete
    ));
    host.maintain_connections(Duration::from_secs(30));
    assert!(host.connections.states[&1].transfer.is_none());
    assert!(host.connections.states[&2].transfer.is_some());
    assert_eq!(host.connections.persistence.reserved, 128 << 10);
    host.maintain_connections(Duration::from_secs(59));
    assert_eq!(host.connections.persistence.reserved, 0);
    assert!(matches!(
        control(
            &mut host,
            2,
            HostRequestBody::WriteWorldLoad {
                job: second,
                offset: 128 << 10,
                bytes: vec![0]
            }
        ),
        HostResponseBody::Error(_)
    ));
    let third = begin(&mut host, 3);
    assert!(matches!(
        write(&mut host, 3, third, 0),
        HostResponseBody::Complete
    ));
    assert!(matches!(
        write(&mut host, 3, third, 1),
        HostResponseBody::Error(_)
    ));
    assert_eq!(
        host.connections.persistence.reserved, 0,
        "rejected chunks release transfer storage"
    );
    let third = begin(&mut host, 3);
    write(&mut host, 3, third, 0);
    control(
        &mut host,
        3,
        HostRequestBody::CancelWorldTransfer {
            job: third,
        },
    );
    assert_eq!(host.connections.persistence.reserved, 0);
    let third = begin(&mut host, 3);
    write(&mut host, 3, third, 0);
    host.close_connection(3);
    assert_eq!(host.connections.persistence.reserved, 0);
}

#[test]
fn expensive_persistence_is_fair_while_independent_host_discovery_progresses() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    for id in 1..=3 {
        open(&mut host, id);
        create_and_open(
            &mut host,
            id,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions::new(Vec::new()),
                temporary: false,
            },
        );
        while host.take_connection_response(id).is_some() {}
    }
    for id in 1..=3 {
        for (request_id, body) in [
            (
                20,
                HostRequestBody::SaveWorld {
                    session: *host.connections.states[&id].sessions.first().unwrap(),
                },
            ),
            (
                21,
                HostRequestBody::ListWorlds {
                    after: 0,
                },
            ),
        ] {
            host.receive_connection(
                id,
                &host::encode_host_request(&HostRequest {
                    connection: id,
                    request_id,
                    body,
                })
                .unwrap(),
            )
            .unwrap();
        }
    }
    let mut serviced = std::collections::BTreeSet::new();
    for _ in 0..3 {
        assert!(host.tick_worlds(0.0).unwrap().is_empty());
        let newly: Vec<_> = host
            .connections
            .states
            .iter()
            .filter(|(id, state)| !serviced.contains(*id) && state.transfer.is_some())
            .map(|(&id, _)| id)
            .collect();
        assert_eq!(newly.len(), 1, "one expensive operation per Host frame");
        let id = newly[0];
        let mut requests = Vec::new();
        while let Some(bytes) = host.connections.states[&id].outbox.pop_front() {
            requests.push(host::decode_host_response(&bytes, id).unwrap().request_id);
        }
        assert_eq!(requests.len(), 2);
        assert!(requests.contains(&20));
        assert!(requests.contains(&21));
        serviced.insert(id);
    }
    assert_eq!(serviced.len(), 3);
    assert!(
        host.connections.persistence.reserved < 64 << 10,
        "idle saves retain only actual output capacity"
    );
}

#[test]
fn long_lived_metadata_does_not_throttle_but_undelivered_records_do() {
    use crate::reliable_output::ORDINARY_OUTPUT_BYTES;
    use ipp_core::services::reliable_output::OutputCharge;

    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let account = host.connections.states[&1].reply_budget.0.clone();

    // Registrations and transport buffers occupy most of the ordinary share indefinitely.
    let metadata = account
        .reserve(OutputCharge {
            entries: 0,
            bytes: ORDINARY_OUTPUT_BYTES - 1024 * 1024,
        })
        .unwrap();
    assert!(host.connection_accepts_input(1));

    // Undelivered records filling three quarters of the remaining room throttle ingress, and
    // input resumes once the backlog drains below half.
    let backlog = account
        .reserve(OutputCharge {
            entries: 1,
            bytes: 3 * 1024 * 1024 / 4,
        })
        .unwrap();
    assert!(!host.connection_accepts_input(1));
    drop(backlog);
    assert!(host.connection_accepts_input(1));
    drop(metadata);
    host.close_connection(1);
}
