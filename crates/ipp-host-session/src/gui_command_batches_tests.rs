//! GUI buffers reuse the Host logical-command gate rather than introducing a
//! subsystem scheduler. These tests observe the real World between buffers.

use super::*;
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, GuiCommand,
    GuiContainerKind, GuiNodeContent, GuiNodeId, GuiNodeStyle, Surface,
};
use std::time::Duration;

struct Platform;

impl HostServices for Platform {
    const NAME: &'static str = "gui-command-batch-test";

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

fn begin(host: &mut Host<Platform>, request_id: u64) -> u64 {
    send(host, 1, request_id, RequestBody::BeginBatch);
    host.tick(0.01).unwrap();
    let replies = responses(host, 1);
    let reply = replies.iter().find(|reply| reply[24] == 23).unwrap();
    responses(host, 2);
    responses(host, 3);
    u64::from_le_bytes(reply[25..33].try_into().unwrap())
}

fn panel(host: &mut Host<Platform>) -> (EntityId, u64) {
    let mut session = host.session_mut(1).unwrap();
    let world = session.world_mut();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: EntityMetadata::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Surface(Surface::default()),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::GuiRoot(ipp_core::GuiRoot::default()),
                },
            ],
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    let incarnation = world
        .inspect_gui(entity, None, 1, 1)
        .unwrap()
        .root_incarnation;
    (entity, incarnation)
}

fn insert(entity: EntityId, incarnation: u64, id: u32, parent: Option<u32>) -> GuiCommand {
    GuiCommand::InsertNode {
        entity,
        root_incarnation: incarnation,
        id: GuiNodeId(id),
        parent: parent.map(GuiNodeId),
        index: 0,
        content: GuiNodeContent::Container(GuiContainerKind::Column),
        style: GuiNodeStyle::default(),
    }
}

fn gui_chunk(batch_id: u64, commands: Vec<GuiCommand>) -> RequestBody {
    RequestBody::GuiCommands {
        batch_id: Some(batch_id),
        commands,
    }
}

#[test]
fn gui_buffers_hold_one_world_while_a_peer_world_progresses() {
    let mut host = ready();
    let (entity, incarnation) = panel(&mut host);
    let batch_id = begin(&mut host, 10);
    let held_tick = host.session_mut(1).unwrap().world().tick();
    let peer_tick = host.session_mut(3).unwrap().world().tick();

    send(
        &mut host,
        1,
        11,
        gui_chunk(batch_id, vec![insert(entity, incarnation, 1, None)]),
    );
    send(
        &mut host,
        2,
        12,
        RequestBody::Batch(Batch {
            id: 12,
            operations: vec![Command::Create {
                alias: 90,
                metadata: EntityMetadata::default(),
            }],
        }),
    );
    host.tick(0.01).unwrap();
    assert_eq!(host.session_mut(1).unwrap().world().tick(), held_tick);
    assert!(host.session_mut(3).unwrap().world().tick() > peer_tick);
    assert!(responses(&mut host, 2).is_empty());

    send(
        &mut host,
        1,
        13,
        gui_chunk(batch_id, vec![insert(entity, incarnation, 2, Some(1))]),
    );
    host.tick(0.01).unwrap();
    assert_eq!(host.session_mut(1).unwrap().world().tick(), held_tick);
    assert!(responses(&mut host, 2).is_empty());
    assert_eq!(
        host.session_mut(1)
            .unwrap()
            .world()
            .gui_root(entity)
            .unwrap()
            .nodes()
            .len(),
        2
    );

    send(&mut host, 1, 14, RequestBody::EndBatch(batch_id));
    host.tick(0.01).unwrap();
    assert_eq!(host.session_mut(1).unwrap().world().tick(), held_tick + 1);
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 2);
    assert!(!responses(&mut host, 2).is_empty());
}

#[test]
fn failed_gui_buffer_reports_prefix_stops_suffix_and_allows_recovery() {
    let mut host = ready();
    let (entity, incarnation) = panel(&mut host);
    let batch_id = begin(&mut host, 20);
    let held_tick = host.session_mut(1).unwrap().world().tick();
    send(
        &mut host,
        1,
        21,
        gui_chunk(batch_id, vec![insert(entity, incarnation, 1, None)]),
    );
    host.tick(0.01).unwrap();
    responses(&mut host, 1);
    send(
        &mut host,
        1,
        22,
        gui_chunk(
            batch_id,
            vec![
                insert(entity, incarnation, 2, Some(1)),
                insert(entity, incarnation, 2, Some(1)),
                insert(entity, incarnation, 3, Some(1)),
            ],
        ),
    );
    host.tick(0.01).unwrap();
    let replies = responses(&mut host, 1);
    let reply = replies.iter().find(|reply| reply[24] == 28).unwrap();
    assert_eq!(reply[24], 28);
    assert_eq!(u32::from_le_bytes(reply[25..29].try_into().unwrap()), 1);
    assert_eq!(reply[29], 1);
    assert_eq!(host.session_mut(1).unwrap().world().tick(), held_tick + 1);
    let root = host
        .session_mut(1)
        .unwrap()
        .world()
        .gui_root(entity)
        .unwrap()
        .clone();
    assert!(root.nodes().node(GuiNodeId(2)).is_some());
    assert!(root.nodes().node(GuiNodeId(3)).is_none());

    send(
        &mut host,
        1,
        23,
        RequestBody::GuiCommands {
            batch_id: None,
            commands: vec![insert(entity, incarnation, 3, Some(1))],
        },
    );
    host.tick(0.01).unwrap();
    assert!(
        host.session_mut(1)
            .unwrap()
            .world()
            .gui_root(entity)
            .unwrap()
            .nodes()
            .node(GuiNodeId(3))
            .is_some()
    );
}

#[test]
fn gui_stream_deadline_and_disconnect_release_gate_for_cleanup() {
    let mut host = ready();
    let (entity, incarnation) = panel(&mut host);
    let batch_id = begin(&mut host, 30);
    let held_tick = host.session_mut(1).unwrap().world().tick();
    send(
        &mut host,
        1,
        31,
        gui_chunk(batch_id, vec![insert(entity, incarnation, 1, None)]),
    );
    host.tick(0.01).unwrap();
    responses(&mut host, 1);
    host.maintain_connections(Duration::from_secs(2));
    let aborted = responses(&mut host, 1);
    assert!(aborted.iter().any(|reply| reply[24] == 24));
    assert_eq!(host.session_mut(1).unwrap().world().tick(), held_tick);
    assert!(
        host.session_mut(1)
            .unwrap()
            .world()
            .gui_root(entity)
            .unwrap()
            .nodes()
            .node(GuiNodeId(1))
            .is_some()
    );

    host.detach_world_session(1).unwrap();
    host.tick(0.01).unwrap();
    assert!(
        host.session_mut(2)
            .unwrap()
            .world()
            .gui_root(entity)
            .unwrap()
            .nodes()
            .node(GuiNodeId(1))
            .is_some()
    );
    send(
        &mut host,
        2,
        32,
        RequestBody::Batch(Batch {
            id: 32,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_ROOT,
            }],
        }),
    );
    host.tick(0.01).unwrap();
    assert!(
        host.session_mut(2)
            .unwrap()
            .world()
            .gui_root(entity)
            .is_none()
    );
}
