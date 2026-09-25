//! Enter submission of focused single-line text inputs.

use super::test_support::*;
use super::*;
use crate::{GuiNodeData, GuiNodeId, WorldUpdateReport};

fn focused_text(text: &str) -> Fixture {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeData::TextInput {
            text: text.into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    let handle = GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2));
    route(
        &mut fixture,
        vec![GuiInputCommand::Focus {
            handle,
        }],
    );
    fixture
}

fn route(fixture: &mut Fixture, inputs: Vec<GuiInputCommand>) -> WorldUpdateReport {
    let mut context = world(fixture);
    for input in inputs {
        context.enqueue_gui_input_command(SESSION, input).unwrap();
    }
    context.step(0.0).unwrap()
}

fn enter() -> GuiInputCommand {
    GuiInputCommand::Key {
        key: GuiKey::Enter,
        pressed: true,
    }
}

/// Submissions a report published, with their revision, text and path.
fn submissions(report: &WorldUpdateReport) -> Vec<(u32, String, Vec<GuiNodeId>)> {
    report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::Submitted {
                revision,
                text,
                path,
                ..
            } => Some((*revision, text.clone(), path.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn enter_submits_the_committed_text_once_at_the_next_boundary() {
    let mut fixture = focused_text("ae");
    let report = route(&mut fixture, vec![enter()]);
    assert!(submissions(&report).is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());

    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        submissions(&report),
        vec![(1, "ae".to_owned(), vec![GuiNodeId(1), GuiNodeId(2)])]
    );
    let effect = report
        .gui_input_effects
        .iter()
        .find(|effect| matches!(effect.kind, GuiInputEffectKind::Submitted { .. }))
        .unwrap();
    assert_eq!(effect.session, SESSION);
    assert_eq!(effect.effect_tick, effect.source_tick + 1);
    assert!(report.gui_input_conflicts.is_empty());

    // Submission commits no value: the text and revision hold.
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 1)
    );
    assert!(submissions(&world(&mut fixture).step(0.0).unwrap()).is_empty());
}

#[test]
fn enter_after_same_tick_typing_submits_the_typed_revision() {
    let mut fixture = focused_text("a");
    route(
        &mut fixture,
        vec![
            GuiInputCommand::Text {
                text: "b".into(),
                fence: None,
            },
            enter(),
        ],
    );
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        submissions(&report),
        vec![(2, "ab".to_owned(), vec![GuiNodeId(1), GuiNodeId(2)])]
    );
}

#[test]
fn enter_during_composition_belongs_to_the_ime() {
    let mut fixture = focused_text("a");
    let panel = fixture.panel;
    let report = route(
        &mut fixture,
        vec![
            GuiInputCommand::UpdateComposition {
                text: "V".into(),
                caret_start: 1,
                caret_end: 1,
                fence: None,
            },
            enter(),
        ],
    );
    assert!(report.gui_unhandled_inputs.is_empty());
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(submissions(&report).is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    assert!(
        world(&mut fixture)
            .gui_text_composition(panel, GuiNodeId(2))
            .is_some()
    );

    // Once the IME commits, Enter submits the composed text.
    route(
        &mut fixture,
        vec![
            GuiInputCommand::CommitComposition {
                fence: None,
            },
            enter(),
        ],
    );
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        submissions(&report),
        vec![(2, "aV".to_owned(), vec![GuiNodeId(1), GuiNodeId(2)])]
    );
}
