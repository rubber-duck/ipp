//! Deterministic multi-World pressure on one physical connection at default output limits.
//!
//! Sixteen GUI panel Worlds share one connection with lifecycle watches whose value members
//! observe layout-written scroll extents, while a frame delivery stays held at the native or
//! copied completion seam. This reproduces the uncorrelated burst that exhausted the former
//! fixed record reserve; it does not replay the hardware driver's scheduling or claim GPU
//! evidence.

use super::*;
use crate::reliable_output::{
    MAX_OUTPUT_BYTES, ORDINARY_OUTPUT_BYTES, PreparedOutputCopy, ResponseLease,
};
use ipp_core::services::reliable_output::{
    OutputCharge, OutputFailure, OutputStatus, ReliableOutputLease,
};
use ipp_core::systems::gui::layout::GuiLayout;
use ipp_core::systems::gui::local::{GuiScrollView, GuiVirtualItem, GuiVirtualList};
use ipp_core::{Batch, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, WorldRef};
use ipp_protocol::references::WorldReference;
use std::mem::offset_of;

const PANELS: usize = 16;

/// The former fixed record limit per connection (64 requests plus 21 events).
const FORMER_RECORD_LIMIT: usize = 85;

struct Panel {
    world: WorldRef,
    session: u64,
    peer: u64,
    list: EntityId,
    scroll: EntityId,
    body: EntityId,
}

/// Content extents after [`realize`]: ten thousand 0.17 items with two taller ones, and the
/// taller scroll body.
const REALIZED_LIST_CONTENT: f32 = 1700.08;
const REALIZED_SCROLL_CONTENT: f32 = 0.58;

enum Completion {
    Native(ResponseLease),
    Copy(PreparedOutputCopy),
}

fn drain(host: &mut Host<TestHostServices>, connection: u64) -> Vec<ReliableResponse> {
    std::iter::from_fn(|| host.take_connection_response(connection)).collect()
}

fn apply(
    host: &mut Host<TestHostServices>,
    world: WorldRef,
    operations: Vec<Command>,
) -> Vec<(u32, EntityId)> {
    host.runtime
        .world_mut(world.id())
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    host.runtime
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world.id())
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
}

fn request(session: u64, id: u64, tag: u8, payload: Vec<u8>, framed: bool) -> Vec<u8> {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend(id.to_le_bytes());
    bytes.push(tag);
    if framed {
        bytes.extend((payload.len() as u32).to_le_bytes());
    }
    bytes.extend(payload);
    bytes
}

/// Watch the list entity and component, plus value members for the vertical content extent
/// of the list and the scroll view.
fn watch(world: WorldRef, session: u64, panel: (EntityId, EntityId), id: u64) -> Vec<u8> {
    let (list, scroll) = panel;
    let mut payload = world.id().0.to_le_bytes().to_vec();
    payload.extend(world.incarnation().to_le_bytes());
    payload.push(0);
    payload.extend(4u32.to_le_bytes());
    payload.push(0);
    payload.extend(list.to_bits().to_le_bytes());
    payload.push(4);
    payload.push(1);
    payload.extend(list.to_bits().to_le_bytes());
    payload.extend(ComponentValue::GUI_VIRTUAL_LIST.to_le_bytes());
    payload.push(8 | 32 | 64);
    for (entity, component, offset) in [
        (
            list,
            ComponentValue::GUI_VIRTUAL_LIST,
            offset_of!(GuiVirtualList, content_y),
        ),
        (
            scroll,
            ComponentValue::GUI_SCROLL_VIEW,
            offset_of!(GuiScrollView, content_y),
        ),
    ] {
        payload.push(2);
        payload.extend(entity.to_bits().to_le_bytes());
        payload.extend(component.to_le_bytes());
        payload.extend(1u32.to_le_bytes());
        payload.extend((offset as u32).to_le_bytes());
        payload.push(128);
    }
    request(session, id, 35, payload, false)
}

/// Inspect the list entity through the ordinary entity collection.
fn inspect(session: u64, id: u64, entity: EntityId) -> Vec<u8> {
    let mut payload = vec![1];
    payload.extend(0u64.to_le_bytes());
    payload.extend(entity.to_bits().to_le_bytes());
    payload.extend(256u16.to_le_bytes());
    payload.extend(0u16.to_le_bytes());
    request(session, id, 3, payload, false)
}

/// The one vertical content extent a lifecycle value record reports, if `reply` is one.
fn value_record(reply: &[u8]) -> Option<f32> {
    // Envelope (25), World (16), output (8), record tag 3, generation, tick, presence, count.
    if reply.starts_with(host::HOST_RESPONSE_MAGIC) || reply[24] != 37 || reply[49] != 3 {
        return None;
    }
    assert_eq!(reply[66], 1, "present values");
    assert_eq!(u32::from_le_bytes(reply[67..71].try_into().unwrap()), 1);
    assert_eq!(
        reply[75],
        ipp_core::components::schema::FieldKind::F32 as u8
    );
    Some(f32::from_le_bytes(reply[76..80].try_into().unwrap()))
}

fn panel_operations() -> Vec<Command> {
    let mut operations = Vec::new();
    for (alias, height) in [(1, 1.6), (2, 0.28), (3, 0.28), (4, 0.5)] {
        operations.push(Command::Create {
            alias,
            metadata: Default::default(),
            adopt: false,
        });
        operations.push(Command::insert_value(
            EntityRef::Alias(alias),
            ComponentValue::GuiLayout(GuiLayout {
                kind: if alias == 1 {
                    2
                } else {
                    0
                },
                width: 2.04,
                height,
                ..Default::default()
            }),
        ));
        if alias != 1 {
            operations.push(Command::PlaceEntity {
                entity: EntityRef::Alias(alias),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Alias(if alias == 4 {
                        3
                    } else {
                        1
                    })),
                    before: None,
                },
            });
        }
    }
    operations.extend([
        Command::insert_value(
            EntityRef::Alias(2),
            ComponentValue::GuiVirtualList(GuiVirtualList {
                item_count: 10_000,
                item_extent: 0.17,
                overscan: 2,
                axis: 1,
                ..Default::default()
            }),
        ),
        Command::insert_value(
            EntityRef::Alias(3),
            ComponentValue::GuiScrollView(GuiScrollView::default()),
        ),
    ]);
    operations
}

/// Sixteen watched panel Worlds plus 18 idle Worlds on connection 1; connection 2 is an
/// independent peer watching the same panels.
fn fixture() -> (Host<TestHostServices>, Vec<Panel>) {
    let mut host = Host::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    let mut panels = Vec::new();
    for _ in 0..PANELS {
        let HostResponseBody::Attached {
            session,
            reference,
            ..
        } = create_and_open(
            &mut host,
            1,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions {
                    canvas: Some(ipp_core::CanvasState {
                        extent: [2.4, 1.6],
                        units_per_metre: 1.0,
                    }),
                    ..host::WorldCreateOptions::new(vec![
                        "ipp.lifecycle-publisher".into(),
                        "ipp.canvas".into(),
                        "ipp.gui".into(),
                        "ipp.gui-layout".into(),
                    ])
                },
                temporary: false,
            },
        )
        else {
            panic!("GUI World")
        };
        let world = reference.resolve(host.runtime()).unwrap();
        let entities = apply(&mut host, world, panel_operations());
        assert!(host.tick_worlds(0.0).unwrap().is_empty());
        drop(drain(&mut host, 1));
        let HostResponseBody::Attached {
            session: peer,
            ..
        } = control(&mut host, 2, HostRequestBody::OpenWorld(reference))
        else {
            panic!("peer")
        };
        let panel = Panel {
            world,
            session,
            peer,
            list: entities[1].1,
            scroll: entities[2].1,
            body: entities[3].1,
        };
        let members = (panel.list, panel.scroll);
        host.receive_connection(1, &watch(world, session, members, 3))
            .unwrap();
        host.receive_connection(2, &watch(world, peer, members, 3))
            .unwrap();
        assert!(host.tick_worlds(0.0).unwrap().is_empty());
        assert!(host.tick_worlds(0.0).unwrap().is_empty());
        // Each watch reports the current extents first.
        for connection in [1, 2] {
            let current: Vec<_> = drain(&mut host, connection)
                .iter()
                .filter_map(|reply| value_record(reply))
                .collect();
            assert_eq!(current.len(), 2, "connection {connection}");
            assert!(!current.contains(&REALIZED_SCROLL_CONTENT));
        }
        panels.push(panel);
    }
    for _ in 0..18 {
        create_and_open(
            &mut host,
            1,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions::new(vec![]),
                temporary: false,
            },
        );
    }
    drop(drain(&mut host, 1));
    drop(drain(&mut host, 2));
    assert_eq!(host.connections.states[&1].sessions.len(), 34);
    // Lifecycle watches pre-reserve no output; only delivered records are charged.
    assert_eq!(
        host.connections.states[&1].reply_budget.0.usage().entries,
        0
    );
    (host, panels)
}

/// Queue four virtual items and a taller body so both scroll views change extent at the next frame.
fn realize(host: &mut Host<TestHostServices>, panel: &Panel) {
    let mut operations = vec![Command::SetField {
        entity: EntityRef::Handle(panel.body),
        component: ComponentValue::GUI_LAYOUT,
        field: ipp_core::FieldWrite {
            offset: offset_of!(GuiLayout, height) as u32,
            value: ipp_core::FieldValue::F32(0.58),
        },
    }];
    for index in 0..4 {
        let entity = EntityRef::Alias(index + 1);
        operations.extend([
            Command::Create {
                alias: index + 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                entity.clone(),
                ComponentValue::GuiVirtualItem(GuiVirtualItem {
                    index,
                }),
            ),
            Command::insert_value(
                entity.clone(),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 2.04,
                    height: if index % 3 == 0 {
                        0.21
                    } else {
                        0.17
                    },
                    ..Default::default()
                }),
            ),
            Command::PlaceEntity {
                entity,
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(panel.list)),
                    before: None,
                },
            },
        ]);
    }
    host.runtime
        .world_mut(panel.world.id())
        .unwrap()
        .enqueue(Batch {
            id: 2,
            operations,
        })
        .unwrap();
}

/// Every panel's two realized extents arrive exactly once per watch on one connection.
fn assert_values(panels: &[Panel], responses: &[ReliableResponse], peer: bool, watches: usize) {
    let values: Vec<_> = responses
        .iter()
        .filter_map(|reply| {
            let value = value_record(reply)?;
            Some((u64::from_le_bytes(reply[..8].try_into().unwrap()), value))
        })
        .collect();
    assert_eq!(values.len(), 2 * watches * PANELS);
    for panel in panels {
        let session = if peer {
            panel.peer
        } else {
            panel.session
        };
        let mut actual: Vec<_> = values
            .iter()
            .filter(|(owner, _)| *owner == session)
            .map(|(_, value)| *value)
            .collect();
        actual.sort_by(f32::total_cmp);
        let mut expected = vec![REALIZED_SCROLL_CONTENT; watches];
        expected.extend(vec![REALIZED_LIST_CONTENT; watches]);
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
        }
    }
}

/// Hold connection 1's first frame at the transport completion seam, then mount every panel's
/// items and admit a root-binding query, an entity inspection and a lifecycle watch per panel
/// until ingress throttles.
fn held_burst(copy: bool) -> (Host<TestHostServices>, Vec<Panel>, Completion) {
    let (mut host, panels) = fixture();
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    let frame = host.take_connection_response(1).unwrap();
    assert_eq!(frame[24], 4);
    let completion = if copy {
        let mut pending = frame.prepare_copy().ok().unwrap();
        let transferred = pending.bytes().to_vec();
        pending.release_source();
        assert!(pending.bytes().is_empty());
        drop(transferred);
        Completion::Copy(pending)
    } else {
        let (bytes, pending) = frame.into_parts();
        drop(bytes);
        Completion::Native(pending)
    };
    assert_eq!(
        host.connections.states[&1].reply_budget.0.usage().entries,
        1
    );
    for panel in &panels {
        realize(&mut host, panel);
    }

    let mut admitted = 0;
    for (index, panel) in panels.iter().enumerate() {
        for bytes in [
            host::encode_host_request(&HostRequest {
                connection: 1,
                request_id: 100 + index as u64,
                body: HostRequestBody::GetRootOutputBinding(WorldReference::from(panel.world)),
            })
            .unwrap(),
            inspect(panel.session, 10, panel.list),
            watch(panel.world, panel.session, (panel.list, panel.scroll), 11),
        ] {
            if !host.connection_accepts_input(1) {
                break;
            }
            host.receive_connection(1, &bytes).unwrap();
            admitted += 1;
        }
    }
    // Every request is admitted; ingress throttles at three quarters of the request count.
    assert_eq!(admitted, 3 * PANELS);
    assert_eq!(admitted, crate::MAX_PENDING * 3 / 4);
    assert!(!host.connection_accepts_input(1));
    assert!(host.connection_accepts_input(2));
    (host, panels, completion)
}

fn release(completion: Completion) {
    match completion {
        Completion::Native(pending) => drop(pending),
        Completion::Copy(pending) => drop(pending),
    }
}

/// The peer connection remains usable and receives no further observations after cleanup.
fn assert_peer_healthy(host: &mut Host<TestHostServices>) {
    assert!(host.connection_accepts_input(2));
    let response = control(
        host,
        2,
        HostRequestBody::ListWorlds {
            after: 0,
        },
    );
    assert!(matches!(response, HostResponseBody::Worlds { .. }));
}

fn default_limit_burst_is_delivered(copy: bool) {
    let (mut host, panels, completion) = held_burst(copy);
    let account = host.connections.states[&1].reply_budget.0.clone();
    let failures = host.tick_worlds(0.0).unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(account.status(), OutputStatus::Open);

    // The burst retains more records than the former fixed limit, far inside the byte budget.
    let peak = account.usage();
    println!(
        "sixteen-panel burst copy={copy} peak entries={} bytes={} replies={:?}",
        peak.entries,
        peak.bytes,
        account.reply_usage()
    );
    assert!(peak.entries > FORMER_RECORD_LIMIT, "{peak:?}");
    assert!(peak.bytes < ORDINARY_OUTPUT_BYTES / 4, "{peak:?}");

    let delivered = drain(&mut host, 1);
    let host_replies = delivered
        .iter()
        .filter(|reply| reply.starts_with(host::HOST_RESPONSE_MAGIC))
        .count();
    assert_eq!(host_replies, PANELS);
    for panel in &panels {
        let session: Vec<_> = delivered
            .iter()
            .filter(|reply| !reply.starts_with(host::HOST_RESPONSE_MAGIC))
            .filter(|reply| u64::from_le_bytes(reply[..8].try_into().unwrap()) == panel.session)
            .collect();
        for request in [10u64, 11] {
            assert_eq!(
                session
                    .iter()
                    .filter(|reply| u64::from_le_bytes(reply[8..16].try_into().unwrap()) == request)
                    .count(),
                1,
                "panel session {} reply {request}",
                panel.session
            );
        }
    }
    // The first watch reports both changes; the second reports the same current extents.
    assert_values(&panels, &delivered, false, 2);
    let observed = drain(&mut host, 2);
    assert_values(&panels, &observed, true, 1);

    // Delivered records stay charged until physical completion, then return their credit.
    assert_eq!(account.usage(), peak);
    drop(delivered);
    assert!(account.usage().bytes < peak.bytes);
    host.close_connection(1);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert_eq!(account.usage().entries, 1);
    release(completion);
    assert_eq!(account.usage(), OutputCharge::default());
    assert_peer_healthy(&mut host);
    host.close_connection(2);
}

#[test]
fn native_held_completion_delivers_sixteen_panel_burst_at_default_limits() {
    default_limit_burst_is_delivered(false);
}

#[test]
fn copied_held_completion_delivers_sixteen_panel_burst_at_default_limits() {
    default_limit_burst_is_delivered(true);
}

/// Hold all but `headroom` bytes of connection 1's ordinary share.
fn exhaust(host: &Host<TestHostServices>, headroom: usize) -> ReliableOutputLease {
    let account = &host.connections.states[&1].reply_budget.0;
    account
        .reserve(OutputCharge {
            entries: 0,
            bytes: ORDINARY_OUTPUT_BYTES - account.usage().bytes - headroom,
        })
        .unwrap()
}

#[test]
fn genuine_byte_exhaustion_fails_only_its_connection_and_returns_all_credit() {
    let (mut host, panels, completion) = held_burst(false);
    let account = host.connections.states[&1].reply_budget.0.clone();
    let filler = exhaust(&host, 16 * 1024);
    let failures = host.tick_worlds(0.0).unwrap();
    assert!(!failures.is_empty());
    assert!(
        failures
            .iter()
            .all(|(connection, error)| *connection == 1 && error.contains("congestion")),
        "{failures:?}"
    );
    assert_eq!(
        account.status(),
        OutputStatus::Failed(OutputFailure::Capacity)
    );
    assert!(account.usage().bytes <= ORDINARY_OUTPUT_BYTES);

    // The peer still receives every value record of the same burst.
    let observed = drain(&mut host, 2);
    assert_values(&panels, &observed, true, 1);

    host.close_connection(1);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    drop(filler);
    release(completion);
    assert_eq!(account.usage(), OutputCharge::default());
    assert!(
        drain(&mut host, 2)
            .iter()
            .all(|reply| value_record(reply).is_none())
    );
    assert_peer_healthy(&mut host);
    host.close_connection(2);
}

#[test]
fn admitted_replies_remain_deliverable_when_uncorrelated_output_fills_its_share() {
    let (mut host, panels) = fixture();
    let account = host.connections.states[&1].reply_budget.0.clone();
    let filler = exhaust(&host, 0);

    // Uncorrelated output can no longer be retained ...
    let session = panels[0].session;
    assert!(
        host.session_mut(session)
            .unwrap()
            .queue_response(
                0,
                ipp_protocol::ResponseBody::RuntimeFailure {
                    scope: ipp_protocol::RuntimeFailureScope::Resource,
                    faulted: false,
                    message: "event".into(),
                },
            )
            .unwrap_err()
            .contains("congestion")
    );
    assert_eq!(account.status(), OutputStatus::Open);

    // ... yet an admitted control request and a World query are answered from the reply
    // reserve, including a transport copy of each reply.
    host.receive_connection(
        1,
        &host::encode_host_request(&HostRequest {
            connection: 1,
            request_id: 7,
            body: HostRequestBody::ListWorlds {
                after: 0,
            },
        })
        .unwrap(),
    )
    .unwrap();
    host.receive_connection(1, &inspect(session, 8, panels[0].list))
        .unwrap();
    let failures = host.tick_worlds(0.0).unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    let replies: Vec<_> = std::iter::from_fn(|| host.take_connection_response(1))
        .filter(|reply| {
            reply.starts_with(host::HOST_RESPONSE_MAGIC)
                || u64::from_le_bytes(reply[8..16].try_into().unwrap()) == 8
        })
        .map(|reply| reply.prepare_copy().ok().expect("reply copy credit"))
        .collect();
    assert_eq!(replies.len(), 2);
    let listed = host::decode_host_response(replies[0].bytes(), 1).unwrap();
    assert_eq!(listed.request_id, 7);
    assert!(matches!(listed.body, HostResponseBody::Worlds { .. }));
    assert!(account.usage().bytes > ORDINARY_OUTPUT_BYTES);
    assert!(account.usage().bytes <= MAX_OUTPUT_BYTES);
    assert_eq!(account.status(), OutputStatus::Open);

    drop(replies);
    drop(filler);
    host.close_connection(1);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert_eq!(account.usage(), OutputCharge::default());
    assert_peer_healthy(&mut host);
}
