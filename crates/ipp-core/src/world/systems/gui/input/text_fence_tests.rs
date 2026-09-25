//! Focus- and revision-fenced text, selection and composition commands.

use super::test_support::*;
use super::*;
use crate::{GuiCommand, GuiNodeData, GuiNodeId, WorldUpdateReport};

/// Insert a second font-backed text input below the fixture's node 2.
fn insert_second_text(fixture: &mut Fixture) {
    let panel = fixture.panel;
    let root_incarnation = incarnation(fixture);
    let mut context = world(fixture);
    context
        .enqueue_gui_command(
            SESSION,
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation,
                id: GuiNodeId(3),
                parent: Some(GuiNodeId(1)),
                index: 1,
                data: GuiNodeData::TextInput {
                    text: "b".into(),
                    placeholder: "e".into(),
                },
                values: crate::GuiNodeDataRow::default(),
                style: font_style(),
            },
        )
        .unwrap();
    context.step(0.0).unwrap();
}

fn text_fixture() -> Fixture {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeData::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    fixture
}

fn handle(fixture: &mut Fixture, node: u32) -> GuiNodeHandle {
    let panel = fixture.panel;
    let root_incarnation = incarnation(fixture);
    GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(node))
}

/// Latest authoritative focused text state a report published.
fn published(report: &WorldUpdateReport) -> Option<GuiTextFocusState> {
    report
        .gui_text_focus_updates
        .iter()
        .rev()
        .find_map(|update| match update {
            GuiTextFocusUpdate::Focused(state) => Some(state.clone()),
            GuiTextFocusUpdate::Cleared {
                ..
            } => None,
        })
}

/// The fence a native buffer stamps after observing `state`.
fn fence_of(state: &GuiTextFocusState) -> GuiTextFence {
    GuiTextFence {
        context_generation: state.context_generation,
        focus_generation: state.focus_generation,
        target: state.target,
        revision: state.revision,
    }
}

/// Step one frame with the given inputs, returning its report.
fn route(fixture: &mut Fixture, session: u64, inputs: Vec<GuiInputCommand>) -> WorldUpdateReport {
    let mut context = world(fixture);
    for input in inputs {
        context.enqueue_gui_input_command(session, input).unwrap();
    }
    context.step(0.0).unwrap()
}

/// Focus one text input and return the fence its native buffer observes.
fn focus_fence(fixture: &mut Fixture, node: u32) -> GuiTextFence {
    let handle = handle(fixture, node);
    let report = route(
        fixture,
        SESSION,
        vec![GuiInputCommand::Focus {
            handle,
        }],
    );
    let state = published(&report).expect("focus publishes the text state");
    assert_eq!(state.target.node, GuiNodeId(node));
    fence_of(&state)
}

#[test]
fn delayed_paste_after_focus_move_conflicts_without_writing_either_input() {
    let mut fixture = text_fixture();
    insert_second_text(&mut fixture);
    let first = focus_fence(&mut fixture, 2);
    let second = focus_fence(&mut fixture, 3);
    assert_ne!(first.focus_generation, second.focus_generation);

    // The clipboard read started while node 2 was focused and resolves after
    // the move: its stamp names the old focus generation and target.
    let report = route(
        &mut fixture,
        SESSION,
        vec![GuiInputCommand::Text {
            text: "pasted".into(),
            fence: Some(first),
        }],
    );
    assert_eq!(report.gui_input_conflicts.len(), 1);
    assert_eq!(
        report.gui_input_conflicts[0].reason,
        GuiInputConflictReason::FocusMismatch
    );
    assert_eq!(report.gui_input_conflicts[0].target, Some(first.target));
    assert!(report.gui_unhandled_inputs.is_empty());
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 1)
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(3)),
        (GuiControlValue::Text("b".into()), 1)
    );

    // The same paste stamped against the current focus lands on node 3.
    route(
        &mut fixture,
        SESSION,
        vec![GuiInputCommand::Text {
            text: "!".into(),
            fence: Some(second),
        }],
    );
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(3)),
        (GuiControlValue::Text("b!".into()), 2)
    );
}

#[test]
fn fenced_selection_after_equal_length_replacement_conflicts_and_refreshes() {
    let mut fixture = text_fixture();
    let panel = fixture.panel;
    let handle = handle(&mut fixture, 2);
    let observed = focus_fence(&mut fixture, 2);
    assert_eq!(observed.revision, 1);

    // An equal-length external replacement lands before the delayed range:
    // its offsets stay in bounds but name the replaced text.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle,
                    expected_revision: 1,
                    value: GuiControlValue::Text("Ve".into()),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 0,
                    end: 1,
                    fence: Some(observed),
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.gui_input_conflicts.len(), 1);
    assert_eq!(
        report.gui_input_conflicts[0].reason,
        GuiInputConflictReason::FocusMismatch
    );
    assert_eq!(
        world(&mut fixture).gui_text_selection(panel, GuiNodeId(2)),
        Some((2, 2, 2))
    );

    // The replacement republished the focused text without further input:
    // same target, a moved focus generation and the new revision.
    let refreshed = published(&report).expect("replacement refreshes the text bridge");
    assert_eq!(refreshed.target, observed.target);
    assert_eq!(refreshed.text, "Ve");
    assert_eq!(refreshed.revision, 2);
    assert_ne!(refreshed.focus_generation, observed.focus_generation);

    // A range chosen against the refreshed text applies exactly.
    let current = fence_of(&refreshed);
    route(
        &mut fixture,
        SESSION,
        vec![GuiInputCommand::SetTextSelection {
            start: 0,
            end: 1,
            fence: Some(current),
        }],
    );
    assert_eq!(
        world(&mut fixture).gui_text_selection(panel, GuiNodeId(2)),
        Some((0, 1, 2))
    );
}

#[test]
fn stamped_insertions_chain_on_own_edits_but_never_on_newer_text() {
    let mut fixture = text_fixture();
    let observed = focus_fence(&mut fixture, 2);

    // Two keystrokes sent before the first one's observation arrives carry
    // the same stamp; the second chains on the first's prediction.
    let report = route(
        &mut fixture,
        SESSION,
        vec![
            GuiInputCommand::Text {
                text: "x".into(),
                fence: Some(observed),
            },
            GuiInputCommand::Text {
                text: "y".into(),
                fence: Some(observed),
            },
        ],
    );
    assert!(report.gui_input_conflicts.is_empty());
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("aexy".into()), 3)
    );

    // A stamp naming a revision the text never reached conflicts.
    let future = GuiTextFence {
        revision: 9,
        ..observed
    };
    let report = route(
        &mut fixture,
        SESSION,
        vec![GuiInputCommand::Text {
            text: "z".into(),
            fence: Some(future),
        }],
    );
    assert_eq!(
        report
            .gui_input_conflicts
            .iter()
            .map(|conflict| conflict.reason.clone())
            .collect::<Vec<_>>(),
        vec![GuiInputConflictReason::RevisionMismatch {
            expected: 9,
            found: 3,
        }]
    );

    // A selection names its offsets' revision exactly: the older stamp
    // conflicts even though the focus is unchanged.
    let report = route(
        &mut fixture,
        SESSION,
        vec![GuiInputCommand::SetTextSelection {
            start: 0,
            end: 1,
            fence: Some(observed),
        }],
    );
    assert_eq!(
        report
            .gui_input_conflicts
            .iter()
            .map(|conflict| conflict.reason.clone())
            .collect::<Vec<_>>(),
        vec![GuiInputConflictReason::RevisionMismatch {
            expected: 1,
            found: 3,
        }]
    );
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("aexy".into()), 3)
    );
}

#[test]
fn stamped_insertion_after_external_replacement_conflicts_without_rebase() {
    let mut fixture = text_fixture();
    let handle = handle(&mut fixture, 2);
    let observed = focus_fence(&mut fixture, 2);
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle,
                    expected_revision: 1,
                    value: GuiControlValue::Text("reset".into()),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Text {
                    text: "late".into(),
                    fence: Some(observed),
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.gui_input_conflicts.len(), 1);
    assert_eq!(
        report.gui_input_conflicts[0].reason,
        GuiInputConflictReason::FocusMismatch
    );
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("reset".into()), 2)
    );
}

#[test]
fn stamped_composition_commit_after_node_removal_conflicts() {
    let mut fixture = text_fixture();
    insert_second_text(&mut fixture);
    let panel = fixture.panel;
    let observed = focus_fence(&mut fixture, 3);
    route(
        &mut fixture,
        SESSION,
        vec![GuiInputCommand::UpdateComposition {
            text: "V".into(),
            caret_start: 1,
            caret_end: 1,
            fence: Some(observed),
        }],
    );
    assert!(
        world(&mut fixture)
            .gui_text_composition(panel, GuiNodeId(3))
            .is_some()
    );

    // The composed node is removed before the IME commit arrives.
    let removed = handle(&mut fixture, 3);
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::RemoveNode {
                    handle: removed,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::CommitComposition {
                    fence: Some(observed),
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.gui_input_conflicts.len(), 1);
    assert_eq!(
        report.gui_input_conflicts[0].reason,
        GuiInputConflictReason::FocusMismatch
    );
    assert!(report.gui_unhandled_inputs.is_empty());
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 1)
    );
}

#[test]
fn stamped_composition_from_a_replaced_session_conflicts() {
    const OTHER: u64 = 8;
    let mut fixture = text_fixture();
    let observed = focus_fence(&mut fixture, 2);
    route(
        &mut fixture,
        SESSION,
        vec![GuiInputCommand::UpdateComposition {
            text: "V".into(),
            caret_start: 1,
            caret_end: 1,
            fence: Some(observed),
        }],
    );

    // Another session takes the input context with a programmatic focus on
    // the same node; the first session's commit names the old context.
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    let other = GuiNodeHandle::new(OTHER, panel, root_incarnation, GuiNodeId(2));
    route(
        &mut fixture,
        OTHER,
        vec![GuiInputCommand::Focus {
            handle: other,
        }],
    );
    let report = route(
        &mut fixture,
        SESSION,
        vec![GuiInputCommand::CommitComposition {
            fence: Some(observed),
        }],
    );
    assert_eq!(report.gui_input_conflicts.len(), 1);
    assert_eq!(report.gui_input_conflicts[0].session, SESSION);
    assert_eq!(
        report.gui_input_conflicts[0].reason,
        GuiInputConflictReason::FocusMismatch
    );
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 1)
    );
}
