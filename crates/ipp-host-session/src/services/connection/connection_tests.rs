use super::*;

struct TestHostServices;

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
    host.receive_connection(id, &ipp_protocol::bootstrap())
        .unwrap();
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

#[test]
fn source_delivery_fences_sessions_and_discards_partial_input_on_error_or_disconnect() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: Default::default(),
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
    } = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: Default::default(),
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
        bytes.push(23); // BeginBatch
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
    assert!(host.connections.states[&1].outbox.is_empty());

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
fn complete_source_delivery_progresses_while_a_world_batch_is_held() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: Default::default(),
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
    host.receive_connection(1, &world_frame(1, 23, &[]))
        .unwrap();
    host.tick(0.0).unwrap();
    let response = host.take_connection_response(1).unwrap();
    let batch = u64::from_le_bytes(response[25..33].try_into().unwrap());
    let mut empty_page = batch.to_le_bytes().to_vec();
    empty_page.extend(0u32.to_le_bytes());
    host.receive_connection(1, &world_frame(2, 24, &empty_page))
        .unwrap();
    host.tick(0.0).unwrap();
    while host.take_connection_response(1).is_some() {}
    let held_tick = host.session_mut(session).unwrap().world().tick();

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
    assert_eq!(host.session_mut(session).unwrap().world().tick(), held_tick);
    let resources = host
        .session_mut(session)
        .unwrap()
        .world()
        .resource_snapshots();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].source, name);
    assert_eq!(resources[0].status, ipp_core::AssetResourceStatus::Loaded);

    host.receive_connection(1, &world_frame(6, 25, &batch.to_le_bytes()))
        .unwrap();
    host.tick(0.0).unwrap();
    assert_eq!(
        host.session_mut(session).unwrap().world().tick(),
        held_tick + 1
    );
}

#[test]
fn named_source_delivery_preserves_literal_names_and_rejects_foreign_or_duplicate_ownership() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: Default::default(),
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
    assert_eq!(resources[0].source, name);
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
                uri: name.clone(),
                variant: 0,
            }]
            .active
    );
    assert_eq!(send(&mut host, 0, &begin), 1);
}

#[test]
fn queued_save_cannot_strand_the_terminator_of_its_own_command_batch() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        ..
    } = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: Default::default(),
            temporary: true,
        },
    )
    else {
        panic!("World not attached")
    };
    while host.take_connection_response(1).is_some() {}

    let envelope = |request: u64, tag: u8, suffix: &[u8]| {
        let mut bytes = session.to_le_bytes().to_vec();
        bytes.extend_from_slice(&request.to_le_bytes());
        bytes.push(tag);
        bytes.extend_from_slice(suffix);
        bytes
    };
    host.receive_connection(1, &envelope(11, 23, &[])).unwrap();
    host.tick(0.0).unwrap();
    let reply = host.take_connection_response(1).unwrap();
    assert_eq!(reply[24], 23);
    let batch_id = u64::from_le_bytes(reply[25..33].try_into().unwrap());
    while host.take_connection_response(1).is_some() {}
    let mut buffer = batch_id.to_le_bytes().to_vec();
    buffer.extend_from_slice(&0u32.to_le_bytes());
    host.receive_connection(1, &envelope(12, 24, &buffer))
        .unwrap();
    host.tick(0.0).unwrap();
    while host.take_connection_response(1).is_some() {}
    let tick = host.session_mut(session).unwrap().world().tick();

    host.receive_connection(
        1,
        &host::encode_host_request(&HostRequest {
            connection: 1,
            request_id: 13,
            body: HostRequestBody::SaveWorld,
        })
        .unwrap(),
    )
    .unwrap();
    host.receive_connection(1, &envelope(14, 25, &batch_id.to_le_bytes()))
        .unwrap();
    host.tick(0.0).unwrap();
    assert_eq!(host.session_mut(session).unwrap().world().tick(), tick + 1);
    assert_eq!(host.take_connection_response(1).unwrap()[24], 25);
    while host.take_connection_response(1).is_some() {}

    host.tick(0.0).unwrap();
    let reply = host.take_connection_response(1).unwrap();
    let saved = host::decode_host_response(&reply, 1).unwrap();
    assert_eq!(saved.request_id, 13);
    assert!(!matches!(saved.body, HostResponseBody::Error(_)));
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
    } = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: ipp_core::WorldCreateOptions {
                symbolic_id: "shared".into(),
                ..Default::default()
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
    } = control(
        &mut host,
        2,
        HostRequestBody::AttachWorld(ipp_core::WorldSelector::Id(world.id)),
    )
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
        } = control(
            &mut host,
            1,
            HostRequestBody::CreateWorld {
                options: Default::default(),
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
                options: Default::default(),
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
fn expensive_persistence_is_fair_and_later_connection_requests_remain_ordered() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    for id in 1..=3 {
        open(&mut host, id);
        control(
            &mut host,
            id,
            HostRequestBody::CreateWorld {
                options: Default::default(),
                temporary: false,
            },
        );
        while host.take_connection_response(id).is_some() {}
    }
    for id in 1..=3 {
        for (request_id, body) in [
            (20, HostRequestBody::SaveWorld),
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
        let requests: Vec<_> = host.connections.states[&id]
            .outbox
            .iter()
            .map(|bytes| host::decode_host_response(bytes, id).unwrap().request_id)
            .collect();
        assert_eq!(requests, vec![20, 21]);
        serviced.insert(id);
    }
    assert_eq!(serviced.len(), 3);
    assert!(
        host.connections.persistence.reserved < 64 << 10,
        "idle saves retain only actual output capacity"
    );
}
