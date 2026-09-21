//! Behavioural tests for GUI text editing, caret and composition.

use super::test_support::*;
use super::*;
use crate::{GuiCommand, GuiEvaluatedContent, GuiNodeContent, GuiNodeId};

#[test]
fn text_append_then_backspace_commits_chained_revisions() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "a".into(),
            placeholder: "e".into(),
        },
    );
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let tick = world(&mut fixture).tick();
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        // Same-tick edits observe the press-time focus cursor.
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Text {
                    text: "e".into(),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Backspace,
                    pressed: true,
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(!report.gui_input_effects.is_empty());
        assert!(
            report
                .gui_input_effects
                .iter()
                .all(|effect| matches!(effect.kind, GuiInputEffectKind::HoverChanged { .. }))
        );
        assert!(context.gui_input_has_deferred());
    }
    // Nothing applies yet: the committed value still holds revision 1.
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("a".into()), 1)
    );
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(report.tick, tick + 2);
    let commits: Vec<(GuiControlValue, u32, u64, u64)> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                value,
                revision,
                ..
            } => Some((
                value.clone(),
                *revision,
                effect.source_tick,
                effect.effect_tick,
            )),
            _ => None,
        })
        .collect();
    // Append chains on revision 1, backspace chains on the prediction.
    assert_eq!(
        commits,
        vec![
            (GuiControlValue::Text("ae".into()), 2, tick + 1, tick + 2),
            (GuiControlValue::Text("a".into()), 3, tick + 1, tick + 2),
        ]
    );
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::FocusChanged {
            focus: Some(_)
        }
    )));
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("a".into()), 3)
    );
    assert!(report.gui_input_cancellations.is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
}

#[test]
fn typed_text_reflows_effective_measurement() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "a".into(),
            placeholder: "e".into(),
        },
    );
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let panel = fixture.panel;
    let before = world(&mut fixture).gui_layout_view(panel).unwrap();
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        // Same-tick typing observes the press-time focus cursor.
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Text {
                    text: "e".into(),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    // Initial revision 1 plus the routed append.
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 2)
    );
    let after = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert!(after.layout_revision > before.layout_revision);
    match evaluated_content(&mut fixture, GuiNodeId(2)) {
        GuiEvaluatedContent::TextInput {
            text,
            revision,
            ..
        } => {
            assert_eq!(text, "ae");
            assert_eq!(revision, 2);
        }
        other => panic!("expected text input, got {other:?}"),
    }
    // "ae" spans 0.55 + 0.55 em at the 0.1 m/em fixture font size.
    let rect = world(&mut fixture)
        .gui_layout_view(panel)
        .unwrap()
        .nodes
        .iter()
        .find(|evaluated| evaluated.node == GuiNodeId(2))
        .map(|evaluated| evaluated.rect)
        .unwrap();
    assert!((rect[2] - 0.11).abs() < 1e-5);
    assert!(report.gui_input_cancellations.is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
}

fn focus_handle(fixture: &mut Fixture) -> GuiNodeHandle {
    let panel = fixture.panel;
    let root_incarnation = incarnation(fixture);
    GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1)
}

#[test]
fn text_backspace_deletes_grapheme_not_scalar() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "a\u{301}".into(),
            placeholder: "e".into(),
        },
    );
    let handle = focus_handle(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Backspace,
                    pressed: true,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    let commits: Vec<(GuiControlValue, u32)> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                value,
                revision,
                ..
            } => Some((value.clone(), *revision)),
            _ => None,
        })
        .collect();
    // One grapheme (two scalars) deletes together; a scalar pop would leave "a".
    assert_eq!(commits, vec![(GuiControlValue::Text("".into()), 2)]);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("".into()), 2)
    );
    assert!(report.gui_input_conflicts.is_empty());
}

#[test]
fn text_insert_at_caret_and_replace_selection() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    // Caret between "a" and "e", then insert "V".
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 1,
                    end: 1,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Text {
                    text: "V".into(),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("aVe".into()), 2)
    );
    assert!(
        report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    // Selection 0..1 ("a") replaced by "e".
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 0,
                    end: 1,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Text {
                    text: "e".into(),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("eVe".into()), 3)
    );
    assert!(report.gui_input_conflicts.is_empty());
    // Caret sits after the replacement.
    assert_eq!(
        world(&mut fixture).gui_text_selection(panel, GuiNodeId(2)),
        Some((1, 1, 3))
    );
}

#[test]
fn text_caret_moves_never_reflow_or_remeasure() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    // Focus first so selection commands have a target.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    let before = world(&mut fixture).gui_layout_view(panel).unwrap();
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 0,
                    end: 1,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Right,
                    pressed: true,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Left,
                    pressed: true,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Home,
                    pressed: true,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::End,
                    pressed: true,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::UpdateComposition {
                    text: "V".into(),
                    caret_start: 1,
                    caret_end: 1,
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        // Transient only: no commits, conflicts or unhandled inputs.
        assert!(
            !report
                .gui_input_effects
                .iter()
                .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
        );
        assert!(report.gui_input_conflicts.is_empty());
        assert!(report.gui_unhandled_inputs.is_empty());
    }
    let after = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert_eq!(before.layout_revision, after.layout_revision);
    assert_eq!(before.reflow_count, after.reflow_count);
    assert_eq!(before.remeasure_count, after.remeasure_count);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 1)
    );
    // End key wins; provisional never touched the commit.
    assert_eq!(
        world(&mut fixture).gui_text_selection(panel, GuiNodeId(2)),
        Some((2, 2, 1))
    );
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        Some(("V".into(), 1, 1, 1))
    );
    // Commit reflows exactly once.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, GuiInputCommand::CommitComposition)
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("aeV".into()), 2)
    );
    let committed = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert!(committed.layout_revision > after.layout_revision);
    assert!(committed.remeasure_count > after.remeasure_count);
    assert!(report.gui_input_conflicts.is_empty());
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        None
    );
}

#[test]
fn composition_tracks_authored_reset_before_routing() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    // Routing runs after the current evaluation, so the provisional starts
    // against the authored reset (revision 2), not the pre-drain text.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::UpdateComposition {
                    text: "V".into(),
                    caret_start: 1,
                    caret_end: 1,
                },
            )
            .unwrap();
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                    expected_revision: 1,
                    value: GuiControlValue::Text("a".into()),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("a".into()), 2)
    );
    // The provisional fenced to the reset commits on top of it.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, GuiInputCommand::CommitComposition)
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(report.gui_input_conflicts.is_empty());
    let commits: Vec<(GuiControlValue, u32)> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                value,
                revision,
                ..
            } => Some((value.clone(), *revision)),
            _ => None,
        })
        .collect();
    assert_eq!(commits, vec![(GuiControlValue::Text("aV".into()), 3)]);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("aV".into()), 3)
    );
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        None
    );
}

#[test]
fn text_caret_and_selection_geometry_use_retained_metrics() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 1,
                    end: 1,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    let view = world(&mut fixture).gui_layout_view(panel).unwrap();
    let evaluated = view
        .nodes
        .iter()
        .find(|evaluated| evaluated.node == GuiNodeId(2))
        .unwrap();
    let origin = evaluated.content_origin;
    // "a" advances 0.55 em at 0.1 m/em: caret 1 sits 0.055 logical right.
    let caret = world(&mut fixture)
        .gui_text_caret_rect(panel, GuiNodeId(2))
        .unwrap();
    assert!((caret[0] - (origin[0] + 0.055)).abs() < 1e-5);
    assert!((caret[1] - origin[1]).abs() < 1e-5);
    assert_eq!(caret[2], 0.0);
    assert!((caret[3] - 0.12).abs() < 1e-5);
    // Full selection spans "ae" (0.11 logical wide, 0.12 tall).
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 0,
                    end: 2,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let rects = world(&mut fixture).gui_text_selection_rects(panel, GuiNodeId(2));
    assert_eq!(rects.len(), 1);
    assert!((rects[0][0] - origin[0]).abs() < 1e-5);
    assert!((rects[0][1] - origin[1]).abs() < 1e-5);
    assert!((rects[0][2] - 0.11).abs() < 1e-5);
    assert!((rects[0][3] - 0.12).abs() < 1e-5);

    // A backward range has the same normalized highlight but keeps its caret
    // at the leading edge for candidate/caret geometry.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 2,
                    end: 0,
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(
        world(&mut fixture).gui_text_selection(panel, GuiNodeId(2)),
        Some((0, 2, 1))
    );
    let backward_caret = world(&mut fixture)
        .gui_text_caret_rect(panel, GuiNodeId(2))
        .unwrap();
    assert!((backward_caret[0] - origin[0]).abs() < 1e-5);
    assert!(matches!(
        report.gui_text_focus_updates.as_slice(),
        [GuiTextFocusUpdate::Focused(state)] if state.selection_start == 2 && state.selection_end == 0
    ));
}

#[test]
fn backward_multibyte_focus_observation_preserves_anchor_and_caret() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "a😀b".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 5,
                    end: 1,
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };

    assert!(matches!(
        report.gui_text_focus_updates.as_slice(),
        [GuiTextFocusUpdate::Focused(state)]
            if state.text == "a😀b"
                && state.selection_start == 5
                && state.selection_end == 1
    ));
}

#[test]
fn pointer_drag_selection_publishes_each_caret_change() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: String::new(),
        },
    );
    let panel = fixture.panel;
    let view = world(&mut fixture).gui_layout_view(panel).unwrap();
    let node = view
        .nodes
        .iter()
        .find(|node| node.node == GuiNodeId(2))
        .unwrap();
    let start = [node.content_origin[0], node.rect[1] + node.rect[3] / 2.0];
    let end = [
        node.content_origin[0] + 0.11,
        node.rect[1] + node.rect[3] / 2.0,
    ];
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(3, start)[0].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.gui_text_focus_updates.iter().any(|update| matches!(
            update,
            GuiTextFocusUpdate::Focused(state)
                if state.selection_start == 0 && state.selection_end == 0
        )));
    }
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 3,
                    panel: Some(panel),
                    position: end,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(report.gui_text_focus_updates.iter().any(|update| matches!(
        update,
        GuiTextFocusUpdate::Focused(state)
            if state.selection_start == 0 && state.selection_end == 2
    )));
}

/// Derived paint primitives for one panel through the system boundary.
fn caret_primitives(
    fixture: &mut Fixture,
    panel: EntityId,
) -> Vec<crate::systems::surface::SurfaceRenderPrimitive> {
    let world_id = fixture.world;
    let mut context = world(fixture);
    context
        .with_system(GuiInputSystem::ID, |input: &mut GuiInputSystem, access| {
            let binding = input.layout.expect("layout dependency");
            let layout: &GuiLayoutSystem = access.dependency(binding).expect("layout");
            let sim: &WorldSimulationState = &*access.world;
            let assets = &*access.asset_acquisition;
            input.text_caret_primitives(layout, sim, panel, world_id, assets)
        })
        .expect("input system")
}

#[test]
fn composition_paints_provisional_text_without_committing() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "a".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    // No provisional, no glyph paint.
    assert!(
        caret_primitives(&mut fixture, panel)
            .iter()
            .all(|primitive| !matches!(
                primitive,
                crate::systems::surface::SurfaceRenderPrimitive::Glyphs { .. }
            ))
    );
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::UpdateComposition {
                    text: "e".into(),
                    caret_start: 1,
                    caret_end: 1,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    // Transient only: provisional observable, committed untouched.
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        Some(("e".into(), 1, 1, 1))
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("a".into()), 1)
    );
    // The provisional paints one "e" glyph (fixture glyph 5) at the
    // committed caret, plus exactly one caret bar at its end.
    let primitives = caret_primitives(&mut fixture, panel);
    let glyphs: Vec<_> = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            crate::systems::surface::SurfaceRenderPrimitive::Glyphs {
                glyphs,
                ..
            } => Some(glyphs),
            _ => None,
        })
        .collect();
    assert_eq!(glyphs.len(), 1);
    assert_eq!(
        glyphs[0]
            .iter()
            .map(|glyph| glyph.glyph_id)
            .collect::<Vec<_>>(),
        vec![5]
    );
    let boxes = primitives
        .iter()
        .filter(|primitive| {
            matches!(
                primitive,
                crate::systems::surface::SurfaceRenderPrimitive::Box { .. }
            )
        })
        .count();
    assert_eq!(boxes, 1);
    // Cancelling drops the glyph paint with the provisional.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, GuiInputCommand::CancelComposition)
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        None
    );
    assert!(
        caret_primitives(&mut fixture, panel)
            .iter()
            .all(|primitive| !matches!(
                primitive,
                crate::systems::surface::SurfaceRenderPrimitive::Glyphs { .. }
            ))
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("a".into()), 1)
    );
}

#[test]
fn composition_paints_while_typed_prediction_pending() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "a".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    // A typed edit leaves a prediction behind until the next boundary; the
    // live provisional still paints at the committed caret meanwhile.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Text {
                    text: "b".into(),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::UpdateComposition {
                    text: "e".into(),
                    caret_start: 1,
                    caret_end: 1,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
        assert!(context.gui_input_has_deferred());
    }
    // Transient only: provisional fenced to the prediction, commit untouched.
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        Some(("e".into(), 1, 1, 2))
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("a".into()), 1)
    );
    // The provisional paints one "e" glyph (fixture glyph 5) at the committed
    // caret, plus exactly one caret bar at its end.
    let primitives = caret_primitives(&mut fixture, panel);
    let glyphs: Vec<_> = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            crate::systems::surface::SurfaceRenderPrimitive::Glyphs {
                glyphs,
                ..
            } => Some(glyphs),
            _ => None,
        })
        .collect();
    assert_eq!(glyphs.len(), 1);
    assert_eq!(
        glyphs[0]
            .iter()
            .map(|glyph| glyph.glyph_id)
            .collect::<Vec<_>>(),
        vec![5]
    );
    let boxes = primitives
        .iter()
        .filter(|primitive| {
            matches!(
                primitive,
                crate::systems::surface::SurfaceRenderPrimitive::Box { .. }
            )
        })
        .count();
    assert_eq!(boxes, 1);
    // The pending edit still applies cleanly on the next boundary.
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ab".into()), 2)
    );
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
}

#[test]
fn text_delete_forward_and_home_end_chain() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Home,
                    pressed: true,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Delete,
                    pressed: true,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("e".into()), 2)
    );
    assert!(
        report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    // End then Backspace clears the remainder.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::End,
                    pressed: true,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Backspace,
                    pressed: true,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("".into()), 3)
    );
}

#[test]
fn delayed_selection_after_external_reset_conflicts_without_rebase() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    let handle = GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1);
    // Focus and select [0, 1): the cursor pins revision 1.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 0,
                    end: 1,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        world(&mut fixture).gui_text_selection(panel, GuiNodeId(2)),
        Some((0, 1, 1))
    );
    // An equal-length external replace moves the revision; the delayed
    // range (still valid offsets) conflicts instead of rebasing.
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
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.gui_input_conflicts.len(), 1);
    assert_eq!(
        report.gui_input_conflicts[0].reason,
        GuiInputConflictReason::RevisionMismatch {
            expected: 1,
            found: 2,
        }
    );
    assert!(report.gui_unhandled_inputs.is_empty());
    // The stale cursor reports the end of the current text; the next
    // insert lands there, never inside the replaced range.
    assert_eq!(
        world(&mut fixture).gui_text_selection(panel, GuiNodeId(2)),
        Some((2, 2, 2))
    );
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Text {
                    text: "A".into(),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(report.gui_input_conflicts.is_empty());
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("VeA".into()), 3)
    );
}

#[test]
fn composition_commit_after_blur_writes_nothing() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let handle = focus_handle(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::UpdateComposition {
                    text: "V".into(),
                    caret_start: 1,
                    caret_end: 1,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(SESSION, GuiInputCommand::Blur)
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    // Blur dropped the provisional with the focus.
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        None
    );
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, GuiInputCommand::CommitComposition)
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.gui_unhandled_inputs.len(), 1);
    assert_eq!(
        report.gui_unhandled_inputs[0].reason,
        GuiUnhandledReason::NoFocus
    );
    assert!(report.gui_input_conflicts.is_empty());
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 1)
    );
}

#[test]
fn composition_commit_after_equal_length_reset_conflicts() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let handle = focus_handle(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::UpdateComposition {
                    text: "V".into(),
                    caret_start: 1,
                    caret_end: 1,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    // An equal-length external replace moves the revision under the
    // provisional; the commit conflicts instead of writing newer text.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle,
                    expected_revision: 1,
                    value: GuiControlValue::Text("aV".into()),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(SESSION, GuiInputCommand::CommitComposition)
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.gui_input_conflicts.len(), 1);
    assert_eq!(
        report.gui_input_conflicts[0].reason,
        GuiInputConflictReason::RevisionMismatch {
            expected: 1,
            found: 2,
        }
    );
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("aV".into()), 2)
    );
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        None
    );
}

#[test]
fn caret_and_selection_paint_observe_transient_without_committing() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::TextInput {
            text: "ae".into(),
            placeholder: "e".into(),
        },
    );
    let panel = fixture.panel;
    let handle = focus_handle(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    // Focused caret geometry is observable while nothing commits.
    let caret = world(&mut fixture)
        .gui_text_caret_rect(panel, GuiNodeId(2))
        .unwrap();
    assert!(caret[3] > 0.0);
    assert!(
        world(&mut fixture)
            .gui_text_selection_rects(panel, GuiNodeId(2))
            .is_empty()
    );
    // Selecting [0, 2) paints the full "ae" span (0.11 logical wide)
    // with the committed value and revision untouched.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::SetTextSelection {
                    start: 0,
                    end: 2,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let rects = world(&mut fixture).gui_text_selection_rects(panel, GuiNodeId(2));
    assert_eq!(rects.len(), 1);
    assert!((rects[0][2] - 0.11).abs() < 1e-5);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 1)
    );
    // A provisional is observable without entering the commit.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::UpdateComposition {
                    text: "V".into(),
                    caret_start: 1,
                    caret_end: 1,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    assert_eq!(
        world(&mut fixture).gui_text_composition(panel, GuiNodeId(2)),
        Some(("V".into(), 1, 1, 1))
    );
    assert!(
        world(&mut fixture)
            .gui_text_caret_rect(panel, GuiNodeId(2))
            .is_some()
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 1)
    );
}
