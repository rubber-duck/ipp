//! HOST-level proof for semantic snapshots and actions: the snapshot
//! serves a bounded tree with observed focus through the wire codec, and
//! actions dispatch through the validated control policy with stale,
//! unknown and unsupported refusals preserved as host errors.

use super::*;
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, GuiCommand,
    GuiContainerKind, GuiControlValue, GuiInputCommand, GuiNodeContent, GuiNodeHandle, GuiNodeId,
    GuiNodeStyle, GuiSemanticAction, GuiSemanticActionRequest, GuiSemanticSnapshotQuery, Surface,
};

struct Platform;

impl HostServices for Platform {
    const NAME: &'static str = "gui-semantics-test";

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

/// Collect every response tag waiting in the session outbox.
fn response_tags(host: &mut Host<Platform>) -> Vec<u8> {
    let mut tags = Vec::new();
    let mut session = host.session_mut(SESSION).unwrap();
    while let Some(bytes) = session.take_response() {
        assert!(bytes.len() > 24);
        tags.push(bytes[24]);
    }
    tags
}

/// Build one font-free panel holding a checkbox (node 2) and a button
/// (node 3, unmeasured without a font) through the world handle.
fn build_panel(host: &mut Host<Platform>) -> EntityId {
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
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(3),
            parent: Some(GuiNodeId(1)),
            index: 1,
            content: GuiNodeContent::Button {
                label: "a".into(),
            },
            style: GuiNodeStyle::default(),
        },
    ] {
        world.enqueue_gui_command(INPUT_SESSION, command).unwrap();
        world.step(0.0).unwrap();
    }
    panel
}

fn checkbox_value(host: &mut Host<Platform>, panel: EntityId) -> GuiControlValue {
    control_value(host, panel, GuiNodeId(2))
}

fn control_value(host: &mut Host<Platform>, panel: EntityId, node: GuiNodeId) -> GuiControlValue {
    host.session_mut(SESSION)
        .unwrap()
        .world()
        .inspect_gui(panel, Some(node), 1, 1)
        .unwrap()
        .nodes
        .first()
        .expect("target node")
        .control_value
        .clone()
}

fn root_incarnation(host: &mut Host<Platform>, panel: EntityId) -> u64 {
    host.session_mut(SESSION)
        .unwrap()
        .world()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation
}

#[test]
fn semantic_snapshot_serves_bounded_tree_with_focus() {
    let mut host = ready();
    let panel = build_panel(&mut host);
    // Observe focus through the world handle first.
    {
        let mut session = host.session_mut(SESSION).unwrap();
        let world = session.world_mut();
        let root_incarnation = world
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        world
            .enqueue_gui_input_command(
                INPUT_SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(
                        INPUT_SESSION,
                        panel,
                        root_incarnation,
                        GuiNodeId(2),
                        1,
                    ),
                },
            )
            .unwrap();
        world.step(0.0).unwrap();
    }
    // The host service builds the bounded tree with its roles, values
    // and observed focus.
    let tree = host
        .session_mut(SESSION)
        .unwrap()
        .gui_semantic_snapshot(&GuiSemanticSnapshotQuery {
            entity: panel,
            max_depth: 32,
            limit: 256,
        })
        .unwrap();
    assert_eq!(tree.entity, panel);
    assert_eq!(tree.nodes.len(), 3);
    let checkbox = tree.node(GuiNodeId(2)).unwrap();
    assert_eq!(checkbox.role, ipp_core::GuiSemanticRole::Checkbox);
    assert_eq!(checkbox.value, GuiControlValue::Bool(false));
    assert_eq!(checkbox.revision, 1);
    assert!(checkbox.available);
    assert_eq!(
        checkbox.actions,
        vec![
            ipp_core::GuiSemanticActionKind::Toggle,
            ipp_core::GuiSemanticActionKind::Focus
        ]
    );
    let button = tree.node(GuiNodeId(3)).unwrap();
    assert_eq!(button.role, ipp_core::GuiSemanticRole::Button);
    assert!(button.actions.is_empty());
    assert_eq!(
        tree.focused,
        Some(ipp_core::GuiSemanticFocus {
            id: GuiNodeId(2),
            lifetime: 1,
        })
    );
    // The same snapshot travels the wire codec as a correlated reply.
    send(
        &mut host,
        10,
        RequestBody::GuiSemanticSnapshot(GuiSemanticSnapshotQuery {
            entity: panel,
            max_depth: 32,
            limit: 256,
        }),
    );
    host.session_mut(SESSION).unwrap().tick(0.0).unwrap();
    let tags = response_tags(&mut host);
    // The tick also publishes its frame-completion marker alongside the
    // correlated reply; only the snapshot tag is asserted here.
    assert!(tags.contains(&33), "snapshot reply missing: {tags:?}");
}

#[test]
fn semantic_toggle_dispatches_through_control_policy() {
    let mut host = ready();
    let panel = build_panel(&mut host);
    let root_incarnation = root_incarnation(&mut host, panel);
    let action = GuiSemanticActionRequest {
        entity: panel,
        root_incarnation,
        node: GuiNodeId(2),
        lifetime: 1,
        expected_revision: 1,
        action: GuiSemanticAction::Toggle,
    };
    send(
        &mut host,
        10,
        RequestBody::GuiSemanticAction(Box::new(action)),
    );
    // The toggle commits through the routed input policy: the admission
    // reply (30) plus committed-effect observations (31) publish.
    let mut saw_command = false;
    let mut saw_observations = false;
    for _ in 0..4 {
        host.session_mut(SESSION).unwrap().tick(0.0).unwrap();
        for tag in response_tags(&mut host) {
            saw_command |= tag == 30;
            saw_observations |= tag == 31;
        }
        if checkbox_value(&mut host, panel) == GuiControlValue::Bool(true) {
            break;
        }
    }
    assert!(saw_command, "missing GuiInput admission reply");
    assert!(saw_observations, "missing committed observations");
    assert_eq!(
        checkbox_value(&mut host, panel),
        GuiControlValue::Bool(true)
    );
}

#[test]
fn semantic_action_is_one_capacity_slot_with_no_orphan_focus_or_fault() {
    let mut host = ready();
    host.test_limits(ipp_core::WorldLimits {
        max_queued_batches: 1,
        ..Default::default()
    });
    let panel = build_panel(&mut host);
    let root_incarnation = root_incarnation(&mut host, panel);
    let action = GuiSemanticActionRequest {
        entity: panel,
        root_incarnation,
        node: GuiNodeId(2),
        lifetime: 1,
        expected_revision: 1,
        action: GuiSemanticAction::Toggle,
    };
    send(
        &mut host,
        20,
        RequestBody::GuiSemanticAction(Box::new(action.clone())),
    );
    send(
        &mut host,
        21,
        RequestBody::GuiSemanticAction(Box::new(action)),
    );

    host.session_mut(SESSION).unwrap().tick(0.0).unwrap();
    let first = response_tags(&mut host);
    assert_eq!(
        first.iter().filter(|&&tag| tag == 30).count(),
        1,
        "{first:?}"
    );
    assert_eq!(
        first.iter().filter(|&&tag| tag == 255).count(),
        1,
        "{first:?}"
    );
    assert!(!first.contains(&22), "runtime publication fault: {first:?}");
    assert!(
        host.session_mut(SESSION)
            .unwrap()
            .gui_semantic_snapshot(&GuiSemanticSnapshotQuery {
                entity: panel,
                max_depth: 32,
                limit: 256,
            })
            .unwrap()
            .focused
            .is_none()
    );

    host.session_mut(SESSION).unwrap().tick(0.0).unwrap();
    let second = response_tags(&mut host);
    assert!(
        second.contains(&31),
        "missing committed observation: {second:?}"
    );
    assert!(
        !second.contains(&22),
        "runtime publication fault: {second:?}"
    );
    assert_eq!(
        checkbox_value(&mut host, panel),
        GuiControlValue::Bool(true)
    );
    assert!(
        host.session_mut(SESSION)
            .unwrap()
            .gui_semantic_snapshot(&GuiSemanticSnapshotQuery {
                entity: panel,
                max_depth: 32,
                limit: 256,
            })
            .unwrap()
            .focused
            .is_none()
    );
}

#[test]
fn semantic_action_targets_node_beyond_public_snapshot_page() {
    let mut host = ready();
    let panel = build_panel(&mut host);
    let root_incarnation = root_incarnation(&mut host, panel);
    let target = GuiNodeId(258);
    {
        let mut session = host.session_mut(SESSION).unwrap();
        let world = session.world_mut();
        world
            .enqueue_gui_command(
                INPUT_SESSION,
                GuiCommand::UpdateNode {
                    handle: GuiNodeHandle::new(
                        INPUT_SESSION,
                        panel,
                        root_incarnation,
                        GuiNodeId(1),
                        1,
                    ),
                    patch: ipp_core::GuiNodePatch {
                        content: Some(GuiNodeContent::Container(GuiContainerKind::Stack)),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        world.step(0.0).unwrap();

        let mut queued = 0;
        for id in 4..=target.0 {
            world
                .enqueue_gui_command(
                    INPUT_SESSION,
                    GuiCommand::InsertNode {
                        entity: panel,
                        root_incarnation,
                        id: GuiNodeId(id),
                        parent: Some(GuiNodeId(1)),
                        index: id - 2,
                        content: GuiNodeContent::Checkbox {
                            checked: false,
                        },
                        style: GuiNodeStyle::default(),
                    },
                )
                .unwrap();
            queued += 1;
            if queued == 60 {
                world.step(0.0).unwrap();
                queued = 0;
            }
        }
        if queued != 0 {
            world.step(0.0).unwrap();
        }
    }

    let bounded = host
        .session_mut(SESSION)
        .unwrap()
        .gui_semantic_snapshot(&GuiSemanticSnapshotQuery {
            entity: panel,
            max_depth: 32,
            limit: 256,
        })
        .unwrap();
    assert_eq!(bounded.nodes.len(), 256);
    assert!(bounded.node(target).is_none());

    send(
        &mut host,
        30,
        RequestBody::GuiSemanticAction(Box::new(GuiSemanticActionRequest {
            entity: panel,
            root_incarnation,
            node: target,
            lifetime: 1,
            expected_revision: 1,
            action: GuiSemanticAction::Toggle,
        })),
    );
    let mut tags = Vec::new();
    for _ in 0..4 {
        host.session_mut(SESSION).unwrap().tick(0.0).unwrap();
        tags.extend(response_tags(&mut host));
        if control_value(&mut host, panel, target) == GuiControlValue::Bool(true) {
            break;
        }
    }
    assert!(tags.contains(&30), "missing admission reply: {tags:?}");
    assert!(
        tags.contains(&31),
        "missing committed observation: {tags:?}"
    );
    assert_eq!(
        control_value(&mut host, panel, target),
        GuiControlValue::Bool(true)
    );
}

#[test]
fn semantic_unavailable_press_rejects_without_focus_or_orphan_outcome() {
    let mut host = ready();
    let panel = build_panel(&mut host);
    let root_incarnation = root_incarnation(&mut host, panel);
    // The font-free button is unavailable, so its semantic action set is
    // empty and the correlated request is rejected before input ownership or
    // focus can change.
    send(
        &mut host,
        11,
        RequestBody::GuiSemanticAction(Box::new(GuiSemanticActionRequest {
            entity: panel,
            root_incarnation,
            node: GuiNodeId(3),
            lifetime: 1,
            expected_revision: 0,
            action: GuiSemanticAction::Press,
        })),
    );
    let mut tags = Vec::new();
    for _ in 0..2 {
        host.session_mut(SESSION).unwrap().tick(0.0).unwrap();
        tags.extend(response_tags(&mut host));
    }
    assert!(tags.contains(&255), "missing correlated refusal: {tags:?}");
    assert!(!tags.contains(&30), "unavailable press admitted: {tags:?}");
    assert!(!tags.contains(&31), "unavailable press published: {tags:?}");
    let focused = host
        .session_mut(SESSION)
        .unwrap()
        .gui_semantic_snapshot(&GuiSemanticSnapshotQuery {
            entity: panel,
            max_depth: 32,
            limit: 256,
        })
        .unwrap()
        .focused;
    assert_eq!(focused, None);
}

#[test]
fn semantic_refusals_preserve_unknown_stale_and_unsupported() {
    let mut host = ready();
    let panel = build_panel(&mut host);
    let root_incarnation = root_incarnation(&mut host, panel);
    let resolve = |host: &mut Host<Platform>, request: &GuiSemanticActionRequest| {
        let reply = host
            .session_mut(SESSION)
            .unwrap()
            .resolve_semantic_action(10, request);
        match reply {
            WorldSessionReply::Rejected(message) => message,
            _ => panic!("expected rejection"),
        }
    };
    let unknown = resolve(
        &mut host,
        &GuiSemanticActionRequest {
            entity: panel,
            root_incarnation,
            node: GuiNodeId(999),
            lifetime: 1,
            expected_revision: 1,
            action: GuiSemanticAction::Toggle,
        },
    );
    assert!(unknown.contains("unknown node"), "{unknown}");
    let root = resolve(
        &mut host,
        &GuiSemanticActionRequest {
            entity: panel,
            root_incarnation: root_incarnation + 1,
            node: GuiNodeId(2),
            lifetime: 1,
            expected_revision: 1,
            action: GuiSemanticAction::Toggle,
        },
    );
    assert!(root.contains("unknown root"), "{root}");
    let stale = resolve(
        &mut host,
        &GuiSemanticActionRequest {
            entity: panel,
            root_incarnation,
            node: GuiNodeId(2),
            lifetime: 1,
            expected_revision: 999,
            action: GuiSemanticAction::Toggle,
        },
    );
    assert!(stale.contains("stale revision"), "{stale}");
    let unsupported = resolve(
        &mut host,
        &GuiSemanticActionRequest {
            entity: panel,
            root_incarnation,
            node: GuiNodeId(2),
            lifetime: 1,
            expected_revision: 1,
            action: GuiSemanticAction::Press,
        },
    );
    assert!(unsupported.contains("unsupported"), "{unsupported}");
    let lifetime = resolve(
        &mut host,
        &GuiSemanticActionRequest {
            entity: panel,
            root_incarnation,
            node: GuiNodeId(2),
            lifetime: 999,
            expected_revision: 1,
            action: GuiSemanticAction::Toggle,
        },
    );
    assert!(lifetime.contains("unknown node"), "{lifetime}");
    // Refusals travel the wire as correlated host errors (tag 255).
    send(
        &mut host,
        12,
        RequestBody::GuiSemanticAction(Box::new(GuiSemanticActionRequest {
            entity: panel,
            root_incarnation,
            node: GuiNodeId(999),
            lifetime: 1,
            expected_revision: 1,
            action: GuiSemanticAction::Toggle,
        })),
    );
    host.session_mut(SESSION).unwrap().tick(0.0).unwrap();
    // Ticks also publish frame-completion markers; only the refusal tag is
    // asserted here.
    let refusal_tags = response_tags(&mut host);
    assert!(
        refusal_tags.contains(&255),
        "refusal reply missing: {refusal_tags:?}"
    );
}
