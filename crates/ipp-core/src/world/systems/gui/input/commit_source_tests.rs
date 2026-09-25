//! Committed control effects name what produced them.

use super::test_support::*;
use super::*;
use crate::{GuiCommand, GuiNodeId, WorldUpdateReport};

/// Committed control effects of one report: node, value, revision, session
/// and source.
fn commits(
    report: &WorldUpdateReport,
) -> Vec<(GuiNodeId, GuiControlValue, u32, u64, GuiCommitSource)> {
    report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                node,
                value,
                revision,
                source,
                ..
            } => Some((*node, value.clone(), *revision, effect.session, *source)),
            _ => None,
        })
        .collect()
}

fn handle(fixture: &mut Fixture, node: u32) -> GuiNodeHandle {
    let panel = fixture.panel;
    let root_incarnation = incarnation(fixture);
    GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(node))
}

#[test]
fn external_replacements_publish_the_committed_effect_marked_external() {
    const OTHER: u64 = 9;
    let mut fixture = setup();
    insert_nodes(&mut fixture, true, false);
    let checkbox = handle(&mut fixture, 2);
    let slider = handle(&mut fixture, 3);
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                OTHER,
                GuiCommand::SetControlValue {
                    handle: GuiNodeHandle {
                        session: OTHER,
                        ..checkbox
                    },
                    expected_revision: 1,
                    value: GuiControlValue::Bool(true),
                },
            )
            .unwrap();
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle: slider,
                    expected_revision: 1,
                    value: GuiControlValue::Scalar(0.75),
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(
        commits(&report),
        vec![
            (
                GuiNodeId(2),
                GuiControlValue::Bool(true),
                2,
                OTHER,
                GuiCommitSource::External,
            ),
            (
                GuiNodeId(3),
                GuiControlValue::Scalar(0.75),
                2,
                SESSION,
                GuiCommitSource::External,
            ),
        ]
    );
    let effect = &report.gui_input_effects[0];
    assert_eq!(effect.source_tick, report.tick);
    assert_eq!(effect.effect_tick, report.tick);
    let GuiInputEffectKind::ControlCommitted {
        path,
        ..
    } = &effect.kind
    else {
        unreachable!();
    };
    assert_eq!(path, &vec![GuiNodeId(1), GuiNodeId(2)]);

    // A stale replacement is refused and publishes nothing.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle: checkbox,
                    expected_revision: 1,
                    value: GuiControlValue::Bool(false),
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(commits(&report).is_empty());
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
}

#[test]
fn user_input_and_semantic_actions_carry_their_own_source() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let checkbox = handle(&mut fixture, 2);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: checkbox,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Enter,
                    pressed: true,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        commits(&report),
        vec![(
            GuiNodeId(2),
            GuiControlValue::Bool(true),
            2,
            SESSION,
            GuiCommitSource::User,
        )]
    );

    let target = GuiInputTarget {
        entity: checkbox.entity,
        node: checkbox.node_id,
        root_incarnation: checkbox.root_incarnation,
    };
    world(&mut fixture)
        .enqueue_gui_semantic_action_with_reply(
            SESSION,
            1,
            crate::GuiSemanticActionCommand {
                target,
                expected_revision: 2,
                action: crate::GuiSemanticAction::Toggle,
            },
        )
        .unwrap();
    world(&mut fixture).step(0.0).unwrap();
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        commits(&report),
        vec![(
            GuiNodeId(2),
            GuiControlValue::Bool(false),
            3,
            SESSION,
            GuiCommitSource::Semantic,
        )]
    );
}
