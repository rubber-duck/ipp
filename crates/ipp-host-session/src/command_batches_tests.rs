use super::*;
use ipp_core::{Batch, Command, EntityMetadata, EntityRef};
use std::time::Duration;

struct Platform;

impl HostServices for Platform {
    const NAME: &'static str = "command-batch-test";

    fn initialize(_: &mut ipp_core::HostRuntime) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

fn ready() -> Host<Platform> {
    let mut host = Host::new().unwrap();
    for id in [1, 3] {
        host.open_session(id).unwrap();
        let mut session = host.session_mut(id).unwrap();
        session.receive(&ipp_protocol::bootstrap()).unwrap();
        session.take_response().unwrap();
    }
    host.sessions.insert(
        2,
        WorldSession::new(2, host.session_world(1).unwrap(), true, false),
    );
    host
}

fn send(host: &mut Host<Platform>, session: u64, request_id: u64, body: RequestBody) {
    host.session_mut(session)
        .unwrap()
        .receive_decoded(Request {
            session,
            request_id,
            body,
        })
        .unwrap();
}

fn responses(host: &mut Host<Platform>, session: u64) -> Vec<Vec<u8>> {
    let mut context = host.session_mut(session).unwrap();
    std::iter::from_fn(|| context.take_response()).collect()
}

fn begin(host: &mut Host<Platform>) -> u64 {
    send(host, 1, 1, RequestBody::BeginBatch);
    host.tick(0.01).unwrap();
    let replies = responses(host, 1);
    let reply = replies.iter().find(|reply| reply[24] == 23).unwrap();
    responses(host, 2);
    responses(host, 3);
    u64::from_le_bytes(reply[25..33].try_into().unwrap())
}

fn create(alias: u32) -> Command {
    Command::Create {
        alias,
        metadata: EntityMetadata::default(),
    }
}

fn create_overlay_owner(host: &mut Host<Platform>, session: u64) -> u64 {
    send(
        host,
        session,
        20,
        RequestBody::Batch(Batch {
            id: 20,
            operations: vec![Command::CreateStateOverlayOwner {
                alias: 0,
            }],
        }),
    );
    host.tick(0.01).unwrap();
    responses(host, session);
    *host.sessions[&session].owners.iter().next().unwrap()
}

#[test]
fn short_and_empty_buffers_hold_evaluation_until_explicit_terminator() {
    let mut host = ready();
    let id = begin(&mut host);
    let tick = host.session_mut(1).unwrap().world().tick();
    send(
        &mut host,
        1,
        2,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![create(17)],
        }),
    );
    send(
        &mut host,
        2,
        9,
        RequestBody::Batch(Batch {
            id: 9,
            operations: vec![create(90)],
        }),
    );
    host.tick(0.01).unwrap();
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 1);
    assert_eq!(host.session_mut(1).unwrap().world().tick(), tick);
    assert!(host.session_mut(3).unwrap().world().tick() > tick);
    assert_eq!(
        responses(&mut host, 1)
            .iter()
            .map(|reply| reply[24])
            .collect::<Vec<_>>(),
        [1]
    );
    assert!(responses(&mut host, 2).is_empty());

    send(
        &mut host,
        1,
        3,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![],
        }),
    );
    host.tick(0.01).unwrap();
    assert_eq!(host.session_mut(1).unwrap().world().tick(), tick);
    responses(&mut host, 1);

    // Aliases survive across buffers; unrelated same-session commands cannot
    // prevent a later continuation/terminator from reaching the open batch.
    send(
        &mut host,
        1,
        4,
        RequestBody::Batch(Batch {
            id: 4,
            operations: vec![create(91)],
        }),
    );
    send(
        &mut host,
        1,
        5,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![Command::SetMetadata {
                entity: EntityRef::Alias(17),
                metadata: EntityMetadata {
                    symbolic_id: Some("streamed".into()),
                    ..Default::default()
                },
            }],
        }),
    );
    host.tick(0.01).unwrap();
    assert_eq!(host.session_mut(1).unwrap().world().tick(), tick);
    responses(&mut host, 1);
    send(&mut host, 1, 6, RequestBody::EndBatch(id));
    host.tick(0.01).unwrap();
    assert_eq!(host.session_mut(1).unwrap().world().tick(), tick + 1);
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 3);
    let replies = responses(&mut host, 1);
    assert!(replies.iter().any(|reply| reply[24] == 25));
    assert!(replies.iter().any(|reply| reply[24] == 4));

    send(
        &mut host,
        1,
        7,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![create(99)],
        }),
    );
    host.tick(0.01).unwrap();
    assert_eq!(responses(&mut host, 1)[0][24], 255);
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 3);
}

#[test]
fn deadline_is_absolute_and_expires_without_simulation_progress() {
    let mut host = ready();
    let id = begin(&mut host);
    host.maintain_connections(Duration::from_secs(10));
    // Allocation alone does not start the timer or gate evaluation.
    host.tick(0.01).unwrap();
    let tick = host.session_mut(1).unwrap().world().tick();
    responses(&mut host, 1);
    send(
        &mut host,
        1,
        2,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![create(1)],
        }),
    );
    host.tick(0.01).unwrap();
    responses(&mut host, 1);
    host.maintain_connections(Duration::from_millis(11_999));
    send(
        &mut host,
        1,
        3,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![create(2)],
        }),
    );
    host.tick(0.01).unwrap();
    responses(&mut host, 1);
    host.maintain_connections(Duration::from_secs(12));
    assert_eq!(responses(&mut host, 1)[0][24], 24);
    assert_eq!(host.session_mut(1).unwrap().world().tick(), tick);
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 2);
    send(&mut host, 1, 4, RequestBody::EndBatch(id));
    host.tick(0.01).unwrap();
    assert_eq!(responses(&mut host, 1)[0][24], 255);
    assert_eq!(host.session_mut(1).unwrap().world().tick(), tick + 1);
    let next = begin(&mut host);
    assert_ne!(next, id);
}

#[test]
fn foreign_batch_ids_reject_and_owner_disconnect_releases_the_gate() {
    let mut host = ready();
    let id = begin(&mut host);
    send(
        &mut host,
        3,
        2,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![create(1)],
        }),
    );
    host.tick(0.01).unwrap();
    assert_eq!(responses(&mut host, 3)[0][24], 255);
    assert!(host.session_mut(3).unwrap().world().entities().is_empty());
    send(
        &mut host,
        1,
        2,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![create(1)],
        }),
    );
    host.tick(0.01).unwrap();
    let tick = host.session_mut(2).unwrap().world().tick();
    host.detach_world_session(1).unwrap();
    host.tick(0.01).unwrap();
    assert_eq!(host.session_mut(2).unwrap().world().tick(), tick + 1);
    assert_eq!(host.session_mut(2).unwrap().world().entities().len(), 1);
}

#[test]
fn peer_disconnect_defers_overlay_cleanup_until_the_stream_terminates() {
    let mut host = ready();
    let owner = create_overlay_owner(&mut host, 2);
    let id = begin(&mut host);
    send(
        &mut host,
        1,
        2,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![create(1)],
        }),
    );
    host.tick(0.01).unwrap();
    responses(&mut host, 1);

    host.detach_world_session(2).unwrap();
    assert!(
        host.session_mut(1)
            .unwrap()
            .world()
            .state_overlay_owner_is_live(owner)
    );

    send(&mut host, 1, 3, RequestBody::EndBatch(id));
    host.tick(0.01).unwrap();
    assert!(
        !host
            .session_mut(1)
            .unwrap()
            .world()
            .state_overlay_owner_is_live(owner)
    );
}

#[test]
fn owner_disconnect_flushes_deferred_peer_overlay_cleanup() {
    let mut host = ready();
    let world = host.session_world(1).unwrap();
    let owner = create_overlay_owner(&mut host, 2);
    let id = begin(&mut host);
    send(
        &mut host,
        1,
        2,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![create(1)],
        }),
    );
    host.tick(0.01).unwrap();
    responses(&mut host, 1);

    host.detach_world_session(2).unwrap();
    host.detach_world_session(1).unwrap();
    assert!(
        !host
            .runtime
            .world_mut(world)
            .unwrap()
            .state_overlay_owner_is_live(owner)
    );
}

#[test]
fn failed_page_retains_prior_effects_and_releases_the_world_gate() {
    let mut host = ready();
    let id = begin(&mut host);
    let tick = host.session_mut(1).unwrap().world().tick();
    send(
        &mut host,
        1,
        2,
        RequestBody::BatchChunk(Batch {
            id,
            operations: vec![
                create(1),
                Command::SetMetadata {
                    entity: EntityRef::Alias(99),
                    metadata: EntityMetadata::default(),
                },
            ],
        }),
    );

    host.tick(0.01).unwrap();
    assert_eq!(responses(&mut host, 1)[0][24], 1);
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 1);
    assert!(host.sessions[&1].command_batch.is_none());
    assert_eq!(host.session_mut(1).unwrap().world().tick(), tick + 1);
}
