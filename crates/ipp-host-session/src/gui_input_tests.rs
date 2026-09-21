//! HOST-level proof for shared GUI input admission: a logical batch
//! starting after routing waits for earlier input to drain instead of
//! rejecting it or overtaking it, while the world keeps evaluating.

use super::*;
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, GuiCommand,
    GuiContainerKind, GuiInputCommand, GuiNodeContent, GuiNodeHandle, GuiNodeId, GuiNodeStyle,
    GuiPointerButton,
};
use ipp_core::{GuiControlValue, Surface};

struct Platform;

impl HostServices for Platform {
    const NAME: &'static str = "gui-input-test";

    fn initialize(_: &mut ipp_core::HostRuntime) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

const SESSION: u64 = 1;
const INPUT_SESSION: u64 = 7;

fn ready() -> Host<Platform> {
    let mut host = Host::new().unwrap();
    host.open_session(SESSION).unwrap();
    {
        let mut session = host.session_mut(SESSION).unwrap();
        session.receive(&ipp_protocol::bootstrap()).unwrap();
        session.take_response().unwrap();
    }
    host
}

fn send(host: &mut Host<Platform>, request_id: u64, body: RequestBody) {
    host.session_mut(SESSION)
        .unwrap()
        .receive_decoded(Request {
            session: SESSION,
            request_id,
            body,
        })
        .unwrap();
}

fn drain_responses(host: &mut Host<Platform>) {
    let mut session = host.session_mut(SESSION).unwrap();
    while session.take_response().is_some() {}
}

/// Build one font-free panel with a single checkbox through the world
/// handle, returning the panel and the checkbox centre.
fn build_panel(host: &mut Host<Platform>) -> (EntityId, [f32; 2]) {
    let mut session = host.session_mut(SESSION).unwrap();
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
                    value: ComponentValue::Surface({
                        let mut surface = Surface::default();
                        surface.width = 10.0;
                        surface.height = 10.0;
                        surface
                    }),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::GuiRoot(ipp_core::GuiRoot::default()),
                },
            ],
        })
        .unwrap();
    let report = world.step(0.0).unwrap();
    let panel = report.outcomes[0].result.as_ref().unwrap()[0].1;
    let root_incarnation = world
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    for command in [
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(1),
            parent: None,
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::Column),
            style: GuiNodeStyle {
                width: Some(10.0),
                height: Some(10.0),
                ..Default::default()
            },
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(2),
            parent: Some(GuiNodeId(1)),
            index: 0,
            content: GuiNodeContent::Checkbox {
                checked: false,
            },
            style: GuiNodeStyle::default(),
        },
    ] {
        world.enqueue_gui_command(INPUT_SESSION, command).unwrap();
    }
    world.step(0.0).unwrap();
    let view = world.gui_layout_view(panel).unwrap();
    let rect = view
        .nodes
        .iter()
        .find(|node| node.node == GuiNodeId(2))
        .map(|node| node.rect)
        .unwrap();
    assert!(rect[2] > 0.0 && rect[3] > 0.0);
    (panel, [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0])
}

fn checkbox_value(host: &mut Host<Platform>, panel: EntityId) -> GuiControlValue {
    host.session_mut(SESSION)
        .unwrap()
        .world()
        .inspect_gui(panel, Some(GuiNodeId(2)), 1, 4)
        .unwrap()
        .nodes
        .iter()
        .find(|node| node.id == GuiNodeId(2))
        .unwrap()
        .control_value
        .clone()
}

fn pointer_down(position: [f32; 2]) -> GuiInputCommand {
    GuiInputCommand::PointerDown {
        pointer: 1,
        panel: None,
        position,
        button: GuiPointerButton::Primary,
        blockers: Vec::new(),
        panel_distance: None,
    }
}

fn click(position: [f32; 2]) -> [GuiInputCommand; 2] {
    [
        pointer_down(position),
        GuiInputCommand::PointerUp {
            pointer: 1,
            panel: None,
            position,
            button: GuiPointerButton::Primary,
            blockers: Vec::new(),
            panel_distance: None,
        },
    ]
}

#[test]
fn logical_batch_after_routing_waits_for_input_drain() {
    let mut host = ready();
    let (panel, at) = build_panel(&mut host);
    // Route one completed press gesture directly; it stays queued until
    // evaluation drains it.
    for command in click(at) {
        host.session_mut(SESSION)
            .unwrap()
            .world_mut()
            .enqueue_gui_input_command(INPUT_SESSION, command)
            .unwrap();
    }

    send(&mut host, 10, RequestBody::BeginBatch);
    host.tick(0.01).unwrap();
    drain_responses(&mut host);
    let stream = host.sessions[&SESSION].command_batch.as_ref().unwrap().id;
    let tick = host.session_mut(SESSION).unwrap().world().tick();

    // A chunk arriving with queued input waits instead of dying: no entity,
    // no rejection, and the stream stays allocated but unstarted.
    send(
        &mut host,
        11,
        RequestBody::BatchChunk(Batch {
            id: stream,
            operations: vec![Command::Create {
                alias: 40,
                metadata: EntityMetadata::default(),
            }],
        }),
    );
    host.tick(0.01).unwrap();
    drain_responses(&mut host);
    assert_eq!(host.session_mut(SESSION).unwrap().world().tick(), tick + 1);
    assert_eq!(
        host.session_mut(SESSION).unwrap().world().entities().len(),
        1
    );
    assert!(host.sessions[&SESSION].command_batch.is_some());
    assert!(
        host.sessions[&SESSION]
            .command_batch
            .as_ref()
            .unwrap()
            .started
            .is_none()
    );

    // The routed press applied at this boundary while the chunk waited on
    // the routed envelopes: input first, never overtaken. Asserting the
    // committed value here (rather than a tick later) pins that order.
    assert_eq!(
        checkbox_value(&mut host, panel),
        GuiControlValue::Bool(true)
    );

    // With the input drained the withheld chunk applies at the next stream
    // pass. Static paint must not enqueue pointless animation cleanup that
    // delays this otherwise unrelated batch.
    host.tick(0.01).unwrap();
    drain_responses(&mut host);
    assert_eq!(
        checkbox_value(&mut host, panel),
        GuiControlValue::Bool(true)
    );
    assert_eq!(
        host.session_mut(SESSION).unwrap().world().entities().len(),
        2
    );
    assert!(host.sessions[&SESSION].command_batch.is_some());
    assert!(
        host.sessions[&SESSION]
            .command_batch
            .as_ref()
            .unwrap()
            .started
            .is_some()
    );

    send(&mut host, 12, RequestBody::EndBatch(stream));
    host.tick(0.01).unwrap();
    drain_responses(&mut host);
    assert!(host.sessions[&SESSION].command_batch.is_none());
    assert_eq!(
        checkbox_value(&mut host, panel),
        GuiControlValue::Bool(true)
    );
    assert_eq!(
        host.session_mut(SESSION).unwrap().world().entities().len(),
        2
    );
}

#[test]
fn chunk_with_queued_input_is_transient_capacity() {
    let mut host = ready();
    let (_, at) = build_panel(&mut host);
    for command in click(at) {
        host.session_mut(SESSION)
            .unwrap()
            .world_mut()
            .enqueue_gui_input_command(INPUT_SESSION, command)
            .unwrap();
    }
    // Direct-core admission: transient Capacity, not a terminal rejection.
    let outcome = host
        .session_mut(SESSION)
        .unwrap()
        .world_mut()
        .apply_command_chunk(Batch {
            id: 50,
            operations: vec![],
        });
    assert!(matches!(outcome, Err(ipp_core::ErrorReason::Capacity)));
    // Draining the input (route, then apply) readmits chunks.
    host.session_mut(SESSION)
        .unwrap()
        .world_mut()
        .step(0.01)
        .unwrap();
    let outcome = host
        .session_mut(SESSION)
        .unwrap()
        .world_mut()
        .apply_command_chunk(Batch {
            id: 50,
            operations: vec![],
        });
    assert!(matches!(outcome, Err(ipp_core::ErrorReason::Capacity)));
    host.session_mut(SESSION)
        .unwrap()
        .world_mut()
        .step(0.01)
        .unwrap();
    host.session_mut(SESSION)
        .unwrap()
        .world_mut()
        .apply_command_chunk(Batch {
            id: 50,
            operations: vec![],
        })
        .unwrap();
}

/// Wire ingress for GUI input joins the reply-fenced enqueue path: both
/// commands in a completed press gesture route, apply and publish correlated
/// `response-gui-input` replies carrying the caller's request identities.
#[test]
fn wire_gui_input_admits_press_with_correlated_reply() {
    let mut host = ready();
    let (panel, at) = build_panel(&mut host);
    let [down, up] = click(at);
    send(&mut host, 10, RequestBody::GuiInput(Box::new(down)));
    send(&mut host, 11, RequestBody::GuiInput(Box::new(up)));
    host.tick(0.01).unwrap();
    for request_id in [10u64, 11] {
        let reply = host.session_mut(SESSION).unwrap().take_response().unwrap();
        // session-u64, request-u64, tick-u64, then the response tag.
        assert_eq!(reply[24], 30, "expected response-gui-input");
        assert_eq!(
            &reply[8..16],
            &request_id.to_le_bytes(),
            "reply must carry the caller request identity"
        );
        assert_eq!(
            u16::from_le_bytes(reply[33..35].try_into().unwrap()),
            0,
            "panel-owned input must report a handled disposition"
        );
    }
    drain_responses(&mut host);
    // Wire admission joins the system-command path, so the gesture routes one
    // tick later than directly-enqueued input: still false after the reply
    // tick, committed after the next boundary when the release applies.
    assert_eq!(
        checkbox_value(&mut host, panel),
        GuiControlValue::Bool(false)
    );
    host.tick(0.01).unwrap();
    drain_responses(&mut host);
    assert_eq!(
        checkbox_value(&mut host, panel),
        GuiControlValue::Bool(true)
    );
}

#[test]
fn wire_gui_input_reply_correlates_authoritative_no_panel_miss() {
    let mut host = ready();
    build_panel(&mut host);
    send(
        &mut host,
        20,
        RequestBody::GuiInput(Box::new(GuiInputCommand::PointerDown {
            pointer: 2,
            panel: None,
            position: [50.0, 50.0],
            button: GuiPointerButton::Primary,
            blockers: Vec::new(),
            panel_distance: None,
        })),
    );
    host.tick(0.01).unwrap();

    let mut saw_private_observation = false;
    let mut routing_reply = None;
    let mut session = host.session_mut(SESSION).unwrap();
    while let Some(reply) = session.take_response() {
        let request_id = u64::from_le_bytes(reply[8..16].try_into().unwrap());
        if request_id == 0 && reply[24] == 32 {
            saw_private_observation = true;
        }
        if request_id == 20 {
            routing_reply = Some(reply);
        }
    }
    let reply = routing_reply.expect("missing correlated GUI routing reply");
    assert_eq!(reply[24], 30, "expected response-gui-input");
    assert_eq!(
        u64::from_le_bytes(reply[25..33].try_into().unwrap()),
        u64::from_le_bytes(reply[16..24].try_into().unwrap()),
        "routing disposition carries its source frame"
    );
    assert_eq!(
        u16::from_le_bytes(reply[33..35].try_into().unwrap()),
        1,
        "outside press must report noPanelHit"
    );
    assert_eq!(reply[35], 0, "noPanelHit has no blocker entity");
    assert!(
        saw_private_observation,
        "correlated routing must retain supplier-private observations"
    );
}

/// A wire-submitted input that core refuses at the command boundary still
/// publishes a correlated error reply instead of stalling or overtaking.
#[test]
fn wire_gui_input_rejection_correlates_error_reply() {
    let mut host = ready();
    let (panel, _) = build_panel(&mut host);
    let incarnation = host
        .session_mut(SESSION)
        .unwrap()
        .world()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    send(
        &mut host,
        11,
        RequestBody::GuiInput(Box::new(GuiInputCommand::Focus {
            handle: GuiNodeHandle::new(INPUT_SESSION + 1, panel, incarnation, GuiNodeId(2), 1),
        })),
    );
    host.tick(0.01).unwrap();
    let reply = host.session_mut(SESSION).unwrap().take_response().unwrap();
    assert_eq!(reply[24], 255, "expected response-error");
    assert_eq!(
        &reply[8..16],
        &11u64.to_le_bytes(),
        "rejection must carry the caller request identity"
    );
    drain_responses(&mut host);
    assert_eq!(
        checkbox_value(&mut host, panel),
        GuiControlValue::Bool(false)
    );
}

#[test]
fn input_handles_fence_against_foreign_sessions() {
    let mut host = ready();
    let (panel, _) = build_panel(&mut host);
    let incarnation = host
        .session_mut(SESSION)
        .unwrap()
        .world()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    // A programmatic focus with a foreign session fence is refused at the
    // command boundary and correlated back to its request.
    host.session_mut(SESSION)
        .unwrap()
        .world_mut()
        .enqueue_gui_input_command_with_reply(
            INPUT_SESSION,
            99,
            GuiInputCommand::Focus {
                handle: GuiNodeHandle::new(INPUT_SESSION + 1, panel, incarnation, GuiNodeId(2), 1),
            },
        )
        .unwrap();
    let report = host
        .session_mut(SESSION)
        .unwrap()
        .world_mut()
        .step(0.01)
        .unwrap();
    assert_eq!(report.system_command_outcomes.len(), 1);
    assert_eq!(report.system_command_outcomes[0].request_id, 99);
    assert!(report.system_command_outcomes[0].result.is_err());
    assert!(report.gui_unhandled_inputs.is_empty());
}
