//! Behavioural tests for ordered GUI input routing.

use super::test_support::*;
use super::*;
use crate::CameraMotion;
use crate::components::{Camera, PickingGeometry, Transform};
use crate::systems::geometry::{GeometryDefinition, GeometryShape};
use crate::systems::surface::Surface;
use crate::{
    Batch, Command, ComponentValue, EntityMetadata, EntityRef, HostRuntime, WorldId, WorldLimits,
};
use crate::{
    GuiCommand, GuiContainerKind, GuiNodeContent, GuiNodeId, GuiNodePatch, GuiNodeStyle,
    GuiSemanticAction,
};

#[test]
fn down_up_one_tick_commits_toggle_with_source_and_effect_ticks() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let tick = world(&mut fixture).tick();
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        // Routing only queues envelopes; nothing applies yet. Hover-cursor
        // reports are routing-time observations (source_tick == effect_tick),
        // not queued envelope outcomes, so they alone may appear here: the
        // down sets hover and the up re-sets it after releasing capture.
        assert!(!report.gui_input_effects.is_empty());
        assert!(
            report
                .gui_input_effects
                .iter()
                .all(|effect| matches!(effect.kind, GuiInputEffectKind::HoverChanged { .. }))
        );
        assert!(context.gui_input_has_deferred());
    }
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(false)
    );
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(report.tick, tick + 2);
    let toggle = report
        .gui_input_effects
        .iter()
        .find_map(|effect| match &effect.kind {
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
        .unwrap();
    assert_eq!(toggle.0, GuiControlValue::Bool(true));
    assert_eq!(toggle.1, 2);
    assert_eq!((toggle.2, toggle.3), (tick + 1, tick + 2));
    assert!(report.gui_input_cancellations.is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    // The press-time focus commit queued during routing applies here and
    // reports exactly once; hover already reported in the routing step.
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::FocusChanged {
            focus: Some(_)
        }
    )));
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(true)
    );
}

#[test]
fn multiple_toggles_same_tick_chain_predicted_revisions() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    {
        let mut context = world(&mut fixture);
        // Two full presses of one pointer in one tick; a second pointer
        // would arbitrate instead of toggling (see below).
        for _ in [1, 2] {
            for command in down_up(1, at) {
                context.enqueue_gui_input_command(SESSION, command).unwrap();
            }
        }
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    let mut toggles: Vec<(GuiControlValue, u32)> = report
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
    toggles.sort_by_key(|(_, revision)| *revision);
    assert_eq!(
        toggles,
        vec![
            (GuiControlValue::Bool(true), 2),
            (GuiControlValue::Bool(false), 3),
        ]
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(false)
    );
}

#[test]
fn focus_then_key_space_chains_same_tick() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        // Same-tick key observes the focus cursor set by the press.
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Space,
                    pressed: true,
                },
            )
            .unwrap();
        // Taps commit on release: the press alone queues no value.
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[1].clone())
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    let mut toggles: Vec<(GuiControlValue, u32)> = report
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
    toggles.sort_by_key(|(_, revision)| *revision);
    // Space toggles on, the release completes the tap back off.
    assert_eq!(
        toggles,
        vec![
            (GuiControlValue::Bool(true), 2),
            (GuiControlValue::Bool(false), 3),
        ]
    );
}

#[test]
fn removal_before_routing_reports_unhandled() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let root_incarnation = incarnation(&mut fixture);
    // Routing runs after the current evaluation, so a same-drain removal is
    // already gone when the press routes: no envelope, no cancellation.
    let report = {
        let panel = fixture.panel;
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::RemoveNode {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.gui_unhandled_inputs.len(), 1);
    assert_eq!(
        report.gui_unhandled_inputs[0].reason,
        GuiUnhandledReason::NotFocusable
    );
    assert!(!world(&mut fixture).gui_input_has_deferred());
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(
        report
            .gui_input_effects
            .iter()
            .all(|effect| !matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    assert!(report.gui_input_cancellations.is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
}

#[test]
fn disable_before_routing_reports_stale_target() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let root_incarnation = incarnation(&mut fixture);
    // Routing runs after the current evaluation, so a same-drain hide is
    // already ineligible when the press routes: the retained hit test skips
    // the hidden node, the press lands on the plain container, never queued.
    let report = {
        let panel = fixture.panel;
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                    patch: GuiNodePatch {
                        opacity: Some(0.0),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.gui_unhandled_inputs.len(), 1);
    assert_eq!(
        report.gui_unhandled_inputs[0].reason,
        GuiUnhandledReason::NotFocusable
    );
    assert!(!world(&mut fixture).gui_input_has_deferred());
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(report.gui_input_cancellations.len(), 0);
    assert!(
        !report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
}

#[test]
fn second_pointer_on_pressed_control_arbitrates() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context
            .enqueue_gui_input_command(SESSION, down_up(2, at)[0].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_input_conflicts.len(), 1);
        assert_eq!(
            report.gui_input_conflicts[0].reason,
            GuiInputConflictReason::TouchArbitration {
                owner_pointer: 1
            }
        );
        // The losing pointer captures nothing.
        assert!(context.gui_input_focus().is_some());
    }
}

#[test]
fn miss_reports_unhandled_once_with_no_effect() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, [50.0, 50.0]) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        // The down misses; the up has no capture.
        assert_eq!(report.gui_unhandled_inputs.len(), 2);
        assert!(
            report
                .gui_unhandled_inputs
                .iter()
                .any(|unhandled| unhandled.reason == GuiUnhandledReason::NoPanelHit)
        );
        assert!(
            report
                .gui_unhandled_inputs
                .iter()
                .any(|unhandled| unhandled.reason == GuiUnhandledReason::NoCapture)
        );
        assert!(report.gui_input_effects.is_empty());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(report.gui_input_effects.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
}

#[test]
fn correlated_identical_misses_retain_session_and_request_identities() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let command = down_up(1, [50.0, 50.0])[0].clone();
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command_with_reply(SESSION, 41, command.clone())
            .unwrap();
        context
            .enqueue_gui_input_command_with_reply(SESSION, 42, command)
            .unwrap();
        context
            .enqueue_gui_input_command_with_reply(
                SESSION + 1,
                41,
                down_up(1, [50.0, 50.0])[0].clone(),
            )
            .unwrap();
        context.step(0.0).unwrap()
    };

    assert_eq!(report.gui_unhandled_inputs.len(), 3);
    assert_eq!(
        report
            .gui_unhandled_inputs
            .iter()
            .map(|input| { (input.session, input.source_request_id, input.reason.clone(),) })
            .collect::<Vec<_>>(),
        vec![
            (SESSION, 41, GuiUnhandledReason::NoPanelHit),
            (SESSION, 42, GuiUnhandledReason::NoPanelHit),
            (SESSION + 1, 41, GuiUnhandledReason::NotOwner),
        ]
    );
}

#[test]
fn explicit_blocker_wins_distance_compare() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let blocker = EntityId::from_bits(0x7777);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerDown {
                    pointer: 1,
                    panel: None,
                    position: at,
                    button: GuiPointerButton::Primary,
                    blockers: vec![super::super::super::GuiBlockerHit {
                        distance: 1.0,
                        entity: blocker,
                    }],
                    panel_distance: Some(5.0),
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        assert_eq!(
            report.gui_unhandled_inputs[0].reason,
            GuiUnhandledReason::Blocked {
                entity: blocker
            }
        );
    }
    // Without a panel distance the overlay counts as nearest and keeps it.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(3, at)[0].clone())
            .unwrap();
        context.step(0.0).unwrap();
        assert!(context.gui_input_has_deferred());
    }
}

#[test]
fn session_replacement_cancels_in_flight_envelopes() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context.step(0.0).unwrap();
        assert!(context.gui_input_has_deferred());
        context.release_system_session(SESSION);
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(
        report
            .gui_input_cancellations
            .iter()
            .any(|cancellation| cancellation.reason == GuiInputCancelReason::SessionReplaced)
    );
    assert!(
        !report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(false)
    );
}

#[test]
fn key_without_focus_is_unhandled() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let mut context = world(&mut fixture);
    context
        .enqueue_gui_input_command(
            SESSION,
            GuiInputCommand::Key {
                key: GuiKey::Enter,
                pressed: true,
            },
        )
        .unwrap();
    let report = context.step(0.0).unwrap();
    assert_eq!(report.gui_unhandled_inputs.len(), 1);
    assert_eq!(
        report.gui_unhandled_inputs[0].reason,
        GuiUnhandledReason::NoFocus
    );
}

#[test]
fn tab_moves_focus_across_controls() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, true);
    let first = node_centre(&mut fixture, GuiNodeId(2));
    let second_id = GuiNodeId(3);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, first)[0].clone())
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Tab,
                    pressed: true,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    let focus = world(&mut fixture).gui_input_focus().unwrap();
    assert_eq!(focus.target.node, second_id);
    assert_eq!(
        report
            .gui_input_effects
            .iter()
            .filter(|effect| matches!(effect.kind, GuiInputEffectKind::FocusChanged { .. }))
            .count(),
        2
    );
}

#[test]
fn slider_drag_sets_quantized_values_in_order() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, true, false);
    let panel = fixture.panel;
    let view = world(&mut fixture).gui_layout_view(panel).unwrap();
    let rect = view
        .nodes
        .iter()
        .find(|evaluated| evaluated.node == GuiNodeId(3))
        .map(|evaluated| evaluated.rect)
        .unwrap();
    let rail = crate::systems::gui::slider_rail(rect).unwrap();
    let at = |fraction: f32| {
        let thumb = rail.thumb_rect(fraction).unwrap();
        [thumb[0] + thumb[2] * 0.5, thumb[1] + thumb[3] * 0.5]
    };
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerDown {
                    pointer: 1,
                    panel: None,
                    position: at(0.0),
                    button: GuiPointerButton::Primary,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 1,
                    panel: None,
                    position: at(0.75),
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    let values: Vec<f32> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                value: GuiControlValue::Scalar(value),
                ..
            } => Some(*value),
            _ => None,
        })
        .collect();
    assert_eq!(values, vec![0.0, 0.75]);
}

#[test]
fn slider_off_step_down_up_at_painted_centre_preserves_value() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, true, false);
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(3), 1),
                    patch: GuiNodePatch {
                        content: Some(GuiNodeContent::Slider {
                            value: 5.0,
                            min: 0.0,
                            max: 10.0,
                            step: 2.0,
                        }),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(3), 1),
                    expected_revision: 1,
                    value: GuiControlValue::Scalar(5.0),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }

    let rect = node_rect(&mut fixture, panel, GuiNodeId(3));
    let thumb = crate::systems::gui::slider_rail(rect)
        .unwrap()
        .thumb_rect(0.5)
        .unwrap();
    let at = [thumb[0] + thumb[2] * 0.5, thumb[1] + thumb[3] * 0.5];
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    let values: Vec<_> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                value,
                ..
            } => Some(value.clone()),
            _ => None,
        })
        .collect();
    assert!(!values.is_empty());
    assert!(
        values
            .iter()
            .all(|value| *value == GuiControlValue::Scalar(5.0))
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(3)).0,
        GuiControlValue::Scalar(5.0),
    );

    let moved_thumb = crate::systems::gui::slider_rail(rect)
        .unwrap()
        .thumb_rect(0.8)
        .unwrap();
    let moved = [
        moved_thumb[0] + moved_thumb[2] * 0.5,
        moved_thumb[1] + moved_thumb[3] * 0.5,
    ];
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(2, at)[0].clone())
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 2,
                    panel: None,
                    position: moved,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(SESSION, down_up(2, moved)[1].clone())
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(
        report.gui_input_effects.iter().any(|effect| matches!(
            effect.kind,
            GuiInputEffectKind::ControlCommitted {
                value: GuiControlValue::Scalar(8.0),
                ..
            }
        )),
        "effects: {:?}",
        report.gui_input_effects,
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(3)).0,
        GuiControlValue::Scalar(8.0),
    );
}

#[test]
fn slider_value_mapping_clamps_and_quantizes() {
    let content = GuiNodeContent::Slider {
        value: 0.0,
        min: 0.0,
        max: 10.0,
        step: 2.0,
    };
    let rect = [0.0, 0.0, 4.0, 1.0];
    assert_eq!(
        slider_value_at(&content, rect, 1.0, &GuiControlValue::Scalar(0.0)),
        Some(GuiControlValue::Scalar(2.0))
    );
    assert_eq!(
        slider_value_at(&content, rect, -5.0, &GuiControlValue::Scalar(0.0)),
        Some(GuiControlValue::Scalar(0.0))
    );
    assert_eq!(
        slider_value_at(&content, rect, 50.0, &GuiControlValue::Scalar(0.0)),
        Some(GuiControlValue::Scalar(10.0))
    );
    assert_eq!(
        slider_value_at(
            &content,
            [0.0, 0.0, 0.0, 1.0],
            0.0,
            &GuiControlValue::Scalar(0.0),
        ),
        None
    );
    assert_eq!(
        slider_value_at(
            &content,
            [0.0, 0.0, -1.0, 1.0],
            0.0,
            &GuiControlValue::Scalar(0.0),
        ),
        None
    );
    assert_eq!(
        slider_value_at(
            &content,
            [0.0, f32::NAN, 1.0, 1.0],
            0.0,
            &GuiControlValue::Scalar(0.0),
        ),
        None
    );
    let base = GuiControlValue::Scalar(4.0);
    assert_eq!(
        slider_nudge(&content, &base, 1.0),
        Some(GuiControlValue::Scalar(6.0))
    );
    assert_eq!(
        slider_nudge(&content, &base, -10.0),
        Some(GuiControlValue::Scalar(0.0))
    );
}

#[test]
fn slider_painted_centres_round_trip_for_narrow_and_off_step_values() {
    let content = GuiNodeContent::Slider {
        value: 1.0,
        min: -3.0,
        max: 8.0,
        step: 2.5,
    };
    for rect in [[2.0, 4.0, 0.5, 1.0], [2.0, 4.0, 1.0, 1.0]] {
        let rail = crate::systems::gui::slider_rail(rect).unwrap();
        for value in [-3.0, 1.0, 8.0] {
            let fraction = (value + 3.0) / 11.0;
            let thumb = rail.thumb_rect(fraction).unwrap();
            let center = thumb[0] + thumb[2] * 0.5;
            assert_eq!(
                slider_value_at(&content, rect, center, &GuiControlValue::Scalar(value)),
                Some(GuiControlValue::Scalar(value)),
            );
        }
        assert_eq!(
            slider_value_at(
                &content,
                rect,
                f32::NEG_INFINITY,
                &GuiControlValue::Scalar(1.0),
            ),
            None,
        );
        assert_eq!(
            slider_value_at(
                &content,
                rect,
                rect[0] - 10.0,
                &GuiControlValue::Scalar(1.0),
            ),
            Some(GuiControlValue::Scalar(-3.0)),
        );
        assert_eq!(
            slider_value_at(
                &content,
                rect,
                rect[0] + rect[2] + 10.0,
                &GuiControlValue::Scalar(1.0),
            ),
            Some(GuiControlValue::Scalar(8.0)),
        );
    }
}

#[test]
fn button_press_commits_button_pressed_with_source_and_effect_ticks() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::Button {
            label: "AVa".into(),
        },
    );
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    let tick = world(&mut fixture).tick();
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        // Routing only queues envelopes; hover-cursor reports alone appear
        // here, exactly like the font-free checkbox press.
        assert!(!report.gui_input_effects.is_empty());
        assert!(
            report
                .gui_input_effects
                .iter()
                .all(|effect| matches!(effect.kind, GuiInputEffectKind::HoverChanged { .. }))
        );
        assert!(context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(report.tick, tick + 2);
    let presses: Vec<(Vec<GuiNodeId>, u64, u64)> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ButtonPressed {
                entity,
                root_incarnation: effect_root,
                node,
                lifetime,
                path,
            } => {
                assert_eq!((*entity, *node, *lifetime), (panel, GuiNodeId(2), 1));
                assert_eq!(*effect_root, root_incarnation);
                Some((path.clone(), effect.source_tick, effect.effect_tick))
            }
            _ => None,
        })
        .collect();
    // Exactly one press: the runtime ancestor path pins dispatch even if a
    // later edit moves the tree.
    assert_eq!(
        presses,
        vec![(vec![GuiNodeId(1), GuiNodeId(2)], tick + 1, tick + 2)]
    );
    // The press-time focus commit reports alongside the press; a button
    // queues no value envelope.
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::FocusChanged {
            focus: Some(_)
        }
    )));
    assert!(
        !report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    assert!(report.gui_input_cancellations.is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
}

#[test]
fn programmatic_focus_then_blur_reports_ordered_focus_changes() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    let target = GuiInputTarget {
        entity: panel,
        node: GuiNodeId(2),
        lifetime: 1,
        root_incarnation,
    };
    let tick = world(&mut fixture).tick();
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
            .enqueue_gui_input_command(SESSION, GuiInputCommand::Blur)
            .unwrap();
        // Blur clears the routing cursor synchronously; both commits still
        // apply at the next boundary.
        assert_eq!(context.gui_input_focus(), None);
        let report = context.step(0.0).unwrap();
        assert_eq!(report.tick, tick + 1);
        // Programmatic focus queues envelopes only: no cursor observations.
        assert!(report.gui_input_effects.is_empty());
        assert!(context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(report.tick, tick + 2);
    let changes: Vec<(Option<GuiInputFocus>, u64, u64)> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::FocusChanged {
                focus,
            } => Some((*focus, effect.source_tick, effect.effect_tick)),
            _ => None,
        })
        .collect();
    // Plain focus-commit, one envelope per command in order: this confirms
    // the focus path needs no silent variant.
    assert_eq!(
        changes,
        vec![
            (
                Some(GuiInputFocus {
                    target,
                    session: SESSION,
                }),
                tick + 1,
                tick + 2,
            ),
            (None, tick + 1, tick + 2),
        ]
    );
    assert!(report.gui_input_cancellations.is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
    assert_eq!(world(&mut fixture).gui_input_focus(), None);
}

#[test]
fn idle_step_reports_no_gui_work() {
    // Quiescent policy: with no input queued, frames run no input work and
    // report no GUI observations.
    let mut fixture = setup();
    insert_nodes(&mut fixture, true, true);
    for _ in 0..2 {
        let report = world(&mut fixture).step(0.0).unwrap();
        assert!(report.gui_input_effects.is_empty());
        assert!(report.gui_input_cancellations.is_empty());
        assert!(report.gui_input_conflicts.is_empty());
        assert!(report.gui_unhandled_inputs.is_empty());
        assert!(!world(&mut fixture).gui_input_has_deferred());
    }
}

#[test]
fn hidden_control_pointer_down_is_unhandled() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                    patch: GuiNodePatch {
                        opacity: Some(0.0),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    // Routing-time refusal: hit testing skips the invisible node, so the
    // press reaches no control target (only its visible container parent)
    // and stages no capture, focus or envelope. The unhandled observation
    // reports in the routing step itself, like other routing-time records.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        assert!(matches!(
            report.gui_unhandled_inputs[0].reason,
            GuiUnhandledReason::NotFocusable
        ));
        assert!(report.gui_input_effects.is_empty());
        assert!(report.gui_input_cancellations.is_empty());
        assert!(report.gui_input_conflicts.is_empty());
        assert!(!context.gui_input_has_deferred());
    }
    assert_eq!(world(&mut fixture).gui_input_focus(), None);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 1)
    );
}

#[test]
fn dragged_press_off_target_cancels_losing_checkbox() {
    // Touch arbitration: a press dragged off its control loses to the drag
    // gesture, cancelling the press-time toggle without an action.
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let away = [at[0] + 20.0, at[1] + 20.0];
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 1,
                    panel: None,
                    position: away,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerUp {
                    pointer: 1,
                    panel: None,
                    position: away,
                    button: GuiPointerButton::Primary,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        // The losing press cancels at routing time, so its cancellation
        // reports in this step; the surviving focus envelope still defers.
        let report = context.step(0.0).unwrap();
        assert!(report.gui_input_cancellations.iter().any(|cancellation| {
            cancellation.reason == GuiInputCancelReason::GestureCancelled
                && cancellation
                    .target
                    .is_some_and(|target| target.node == GuiNodeId(2))
        }));
        assert!(
            !report
                .gui_input_effects
                .iter()
                .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
        );
        assert!(context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    // The press-time focus commit is not click-type, so it still applies at
    // the mutation boundary while no value ever commits.
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::FocusChanged {
            focus: Some(_)
        }
    )));
    assert!(
        !report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 1)
    );
}

#[test]
fn focus_then_enter_activates_button_through_router() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::Button {
            label: "a".into(),
        },
    );
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    // Ordinary session-fenced focus followed by Enter in the same drain.
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
                    key: GuiKey::Enter,
                    pressed: true,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(
        report.gui_input_effects.iter().any(|effect| matches!(
            &effect.kind,
            GuiInputEffectKind::ButtonPressed {
                entity,
                node,
                lifetime,
                ..
            } if *entity == panel && *node == GuiNodeId(2) && *lifetime == 1
        )),
        "expected ButtonPressed, got {report:?}"
    );
    assert!(report.gui_input_conflicts.is_empty());
    assert_eq!(
        world(&mut fixture)
            .gui_input_focus()
            .map(|focus| focus.target.node),
        Some(GuiNodeId(2))
    );
}

#[test]
fn semantic_action_press_resolves_and_activates_button() {
    let mut fixture = setup();
    register_font(&mut fixture);
    insert_font_control(
        &mut fixture,
        GuiNodeContent::Button {
            label: "a".into(),
        },
    );
    let panel = fixture.panel;
    // Resolve through the production semantic path, then dispatch the one
    // atomic input-system operation.
    let command = {
        let context = world(&mut fixture);
        let tree = context.gui_semantic_snapshot(panel, 32, 256).unwrap();
        let revision = tree.node(GuiNodeId(2)).unwrap().revision;
        crate::systems::gui::semantics::action_command(
            &tree,
            GuiNodeId(2),
            revision,
            GuiSemanticAction::Press,
        )
        .unwrap()
    };
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_semantic_action_with_reply(SESSION, 77, command)
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(
        report.gui_input_effects.iter().any(|effect| matches!(
            &effect.kind,
            GuiInputEffectKind::ButtonPressed {
                entity,
                node,
                lifetime,
                ..
            } if *entity == panel && *node == GuiNodeId(2) && *lifetime == 1
        )),
        "expected ButtonPressed, got {report:?}"
    );
    assert!(report.gui_input_conflicts.is_empty());
    assert!(world(&mut fixture).gui_input_focus().is_none());
}

#[test]
fn semantic_action_rejects_owner_conflict_without_side_effect() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
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
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    let command = {
        let context = world(&mut fixture);
        let tree = context.gui_semantic_snapshot(panel, 32, 256).unwrap();
        crate::action_command(&tree, GuiNodeId(2), 1, GuiSemanticAction::Toggle).unwrap()
    };
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_semantic_action_with_reply(OTHER, 88, command)
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert_eq!(report.system_command_outcomes.len(), 1);
    assert_eq!(
        report.system_command_outcomes[0].result,
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 1)
    );
    assert_eq!(
        world(&mut fixture).gui_input_focus().unwrap().session,
        SESSION
    );
    assert!(!world(&mut fixture).gui_input_has_deferred());
}

#[test]
fn semantic_toggle_revalidates_revision_at_application() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    let command = {
        let context = world(&mut fixture);
        let tree = context.gui_semantic_snapshot(panel, 32, 256).unwrap();
        crate::action_command(&tree, GuiNodeId(2), 1, GuiSemanticAction::Toggle).unwrap()
    };
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_semantic_action_with_reply(SESSION, 89, command)
            .unwrap();
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                    expected_revision: 1,
                    value: GuiControlValue::Bool(true),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
    assert!(report.gui_input_conflicts.iter().any(|conflict| {
        conflict.reason
            == GuiInputConflictReason::RevisionMismatch {
                expected: 1,
                found: 2,
            }
    }));
    assert!(world(&mut fixture).gui_input_focus().is_none());
}

#[test]
fn hover_and_pressed_getters_mirror_routing_cursors() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let panel = fixture.panel;
    // No cursor before any input.
    assert_eq!(world(&mut fixture).gui_input_hover(1), None);
    assert_eq!(world(&mut fixture).gui_input_pressed(1), None);
    // A press captures its target and hovers it.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context.step(0.0).unwrap();
        let hover = context.gui_input_hover(1).unwrap();
        let pressed = context.gui_input_pressed(1).unwrap();
        assert_eq!((hover.entity, hover.node), (panel, GuiNodeId(2)));
        assert_eq!((pressed.entity, pressed.node), (panel, GuiNodeId(2)));
        assert_eq!(hover, pressed);
        // An untouched pointer still observes no cursor.
        assert_eq!(context.gui_input_hover(2), None);
        assert_eq!(context.gui_input_pressed(2), None);
    }
    // Releasing over the same control clears the press but keeps the hover.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[1].clone())
            .unwrap();
        context.step(0.0).unwrap();
        assert_eq!(context.gui_input_pressed(1), None);
        let hover = context.gui_input_hover(1).unwrap();
        assert_eq!((hover.entity, hover.node), (panel, GuiNodeId(2)));
    }
}

#[test]
fn hover_getter_tracks_moves_and_misses_without_mutation() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let panel = fixture.panel;
    // Hover follows an uncaptured move onto the control.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 4,
                    panel: None,
                    position: at,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
        let hover = context.gui_input_hover(4).unwrap();
        assert_eq!((hover.entity, hover.node), (panel, GuiNodeId(2)));
        assert_eq!(context.gui_input_pressed(4), None);
        // Getters are read-only: a second observation agrees and defers nothing new.
        assert_eq!(context.gui_input_hover(4), Some(hover));
    }
    // Moving far off-panel clears the hover without capturing.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 4,
                    panel: None,
                    position: [50.0, 50.0],
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
        assert_eq!(context.gui_input_hover(4), None);
        assert_eq!(context.gui_input_pressed(4), None);
    }
}

/// Second session observing and authoring beside the input owner.
const OTHER: u64 = 8;

#[test]
fn moved_checkbox_routes_against_current_evaluation() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, true);
    // Swap the two checkboxes, then press where node 3 used to be. Routing
    // after evaluation hits node 2 there; stale routing would hit node 3.
    let at = node_centre(&mut fixture, GuiNodeId(3));
    let root_incarnation = incarnation(&mut fixture);
    let tick = world(&mut fixture).tick();
    {
        let panel = fixture.panel;
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::MoveNode {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(3), 1),
                    parent: Some(GuiNodeId(1)),
                    index: 0,
                },
            )
            .unwrap();
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        assert_eq!(report.tick, tick + 1);
        assert!(report.gui_unhandled_inputs.is_empty());
        assert!(context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(report.tick, tick + 2);
    let toggles: Vec<(GuiNodeId, GuiControlValue, u32, u64, u64)> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                node,
                value,
                revision,
                ..
            } => Some((
                *node,
                value.clone(),
                *revision,
                effect.source_tick,
                effect.effect_tick,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        toggles,
        vec![(
            GuiNodeId(2),
            GuiControlValue::Bool(true),
            2,
            tick + 1,
            tick + 2
        )]
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(true)
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(3)).0,
        GuiControlValue::Bool(false)
    );
}

#[test]
fn resized_checkbox_routes_against_current_evaluation() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let panel = fixture.panel;
    let before = node_rect(&mut fixture, panel, GuiNodeId(2));
    // Widen the checkbox, then press just past its old right edge. Routing
    // after evaluation hits the grown node; stale routing misses it.
    let at = [before[0] + before[2] + 1.0, before[1] + before[3] / 2.0];
    let root_incarnation = incarnation(&mut fixture);
    let tick = world(&mut fixture).tick();
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                    patch: GuiNodePatch {
                        width: Some(Some(before[2] + 4.0)),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        assert_eq!(report.tick, tick + 1);
        assert!(report.gui_unhandled_inputs.is_empty());
        assert!(context.gui_input_has_deferred());
    }
    let after = node_rect(&mut fixture, panel, GuiNodeId(2));
    assert!(after[2] > before[2], "resize must grow the node");
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
    assert_eq!(
        commits,
        vec![(GuiControlValue::Bool(true), 2, tick + 1, tick + 2)]
    );
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
}

#[test]
fn newly_populated_panel_routes_same_evaluation() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    // Both panels share logical coordinates, so the press point below hits
    // the first panel's checkbox today and the new panel's once populated.
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let fresh = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
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
                        value: ComponentValue::GuiRoot(GuiRoot::default()),
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };
    let fresh_incarnation = world(&mut fixture)
        .inspect_gui(fresh, None, 1, 1)
        .unwrap()
        .root_incarnation;
    let tick = world(&mut fixture).tick();
    // Populate the new panel and press in one step: routing after evaluation
    // sees the new nodes and the topmost (newest) panel wins the overlap.
    {
        let mut context = world(&mut fixture);
        for command in [
            GuiCommand::InsertNode {
                entity: fresh,
                root_incarnation: fresh_incarnation,
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
                entity: fresh,
                root_incarnation: fresh_incarnation,
                id: GuiNodeId(2),
                parent: Some(GuiNodeId(1)),
                index: 0,
                content: GuiNodeContent::Checkbox {
                    checked: false,
                },
                style: GuiNodeStyle::default(),
            },
        ] {
            context.enqueue_gui_command(SESSION, command).unwrap();
        }
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        assert_eq!(report.tick, tick + 1);
        assert!(report.gui_unhandled_inputs.is_empty());
        assert!(context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(report.tick, tick + 2);
    let commits: Vec<(EntityId, GuiControlValue, u32, u64, u64)> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                entity,
                root_incarnation: _,
                value,
                revision,
                ..
            } => Some((
                *entity,
                value.clone(),
                *revision,
                effect.source_tick,
                effect.effect_tick,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        commits,
        vec![(fresh, GuiControlValue::Bool(true), 2, tick + 1, tick + 2)]
    );
    assert_eq!(
        committed_bool_on(&mut fixture, fresh, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
    // The first panel's overlapping checkbox never toggled.
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(false)
    );
}

#[test]
fn second_session_cannot_use_focused_control() {
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
    let root_incarnation = incarnation(&mut fixture);
    // The owner presses, then types; the observer's text, key and press all
    // arrive in the same step and must not touch the owner's control.
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
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
                OTHER,
                GuiInputCommand::Text {
                    text: "X".into(),
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                OTHER,
                GuiInputCommand::Key {
                    key: GuiKey::Space,
                    pressed: true,
                },
            )
            .unwrap();
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(OTHER, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        // The observer's text, key and press are refused; its release finds
        // no capture and reports the ordinary miss.
        let refused: Vec<(u64, GuiUnhandledReason)> = report
            .gui_unhandled_inputs
            .iter()
            .map(|unhandled| (unhandled.session, unhandled.reason.clone()))
            .collect();
        assert_eq!(
            refused,
            vec![
                (OTHER, GuiUnhandledReason::NotOwner),
                (OTHER, GuiUnhandledReason::NotOwner),
                (OTHER, GuiUnhandledReason::NotOwner),
                (OTHER, GuiUnhandledReason::NoCapture),
            ]
        );
        assert!(context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 2)
    );
    assert!(report.gui_input_conflicts.is_empty());
    // The observer still authors and inspects normally, without acquiring
    // the context: its next text is refused as well.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                OTHER,
                GuiCommand::UpdateNode {
                    handle: GuiNodeHandle::new(OTHER, panel, root_incarnation, GuiNodeId(2), 1),
                    patch: GuiNodePatch {
                        opacity: Some(0.5),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                OTHER,
                GuiInputCommand::Text {
                    text: "Y".into(),
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        assert_eq!(
            report.gui_unhandled_inputs[0].reason,
            GuiUnhandledReason::NotOwner
        );
    }
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Text("ae".into()), 2)
    );
    let inspected = world(&mut fixture)
        .inspect_gui(panel, Some(GuiNodeId(2)), 1, 4)
        .unwrap();
    assert_eq!(inspected.nodes[0].style.opacity, 0.5);
}

#[test]
fn identical_pointer_ids_do_not_cross_sessions() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    // The owner holds pointer 7 across steps; the observer's down, up and
    // cancel with the same ID must leave the capture untouched.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(7, at)[0].clone())
            .unwrap();
        context.step(0.0).unwrap();
        assert!(context.gui_input_has_deferred());
    }
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(OTHER, down_up(7, at)[0].clone())
            .unwrap();
        context
            .enqueue_gui_input_command(OTHER, down_up(7, at)[1].clone())
            .unwrap();
        context
            .enqueue_gui_input_command(
                OTHER,
                GuiInputCommand::PointerCancel {
                    pointer: 7,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(SESSION, down_up(7, at)[1].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        let refused: Vec<u64> = report
            .gui_unhandled_inputs
            .iter()
            .filter(|unhandled| unhandled.reason == GuiUnhandledReason::NotOwner)
            .map(|unhandled| unhandled.session)
            .collect();
        assert_eq!(refused, vec![OTHER, OTHER, OTHER]);
        assert!(
            report
                .gui_input_cancellations
                .iter()
                .all(|cancellation| cancellation.reason != GuiInputCancelReason::GestureCancelled)
        );
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    // The owner's press-time toggle applied exactly once; its focus cursor
    // survived the foreign pointer traffic.
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
    assert!(report.gui_input_cancellations.is_empty());
    let focus = world(&mut fixture).gui_input_focus().unwrap();
    assert_eq!(focus.session, SESSION);
}

#[test]
fn focus_replaces_context_and_cancels_in_flight() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    // The owner's press routes first in the step, then a validated
    // programmatic focus from another session explicitly replaces the
    // owner: the owner's routed toggle (value plus press-time focus) cancels
    // as replaced before the next boundary applies it.
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context
            .enqueue_gui_input_command(
                OTHER,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(OTHER, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        let replaced: Vec<u64> = report
            .gui_input_cancellations
            .iter()
            .filter(|cancellation| cancellation.reason == GuiInputCancelReason::SessionReplaced)
            .map(|cancellation| cancellation.session)
            .collect();
        assert_eq!(replaced, vec![SESSION, SESSION]);
        assert!(context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(false)
    );
    let changes: Vec<u64> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::FocusChanged {
                focus: Some(focus),
            } => Some(focus.session),
            _ => None,
        })
        .collect();
    assert_eq!(changes, vec![OTHER]);
    // Delayed input from the previous owner fences on the new epoch, while
    // the new owner interacts normally.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Space,
                    pressed: true,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                OTHER,
                GuiInputCommand::Key {
                    key: GuiKey::Space,
                    pressed: true,
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        assert_eq!(
            report.gui_unhandled_inputs[0].reason,
            GuiUnhandledReason::NotOwner
        );
        assert_eq!(report.gui_unhandled_inputs[0].session, SESSION);
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
    assert!(report.gui_input_conflicts.is_empty());
}

#[test]
fn disconnect_frees_context_for_next_session() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
        assert!(context.gui_input_has_deferred());
        context.release_system_session(SESSION);
    }
    // The disconnect drains the owner's routed toggle as replaced; the next
    // session acquires the freed context with an ordinary focus.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                OTHER,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(OTHER, panel, root_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(
            report
                .gui_input_cancellations
                .iter()
                .any(|cancellation| cancellation.session == SESSION
                    && cancellation.reason == GuiInputCancelReason::SessionReplaced)
        );
        assert!(context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(false)
    );
    let changes: Vec<u64> = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::FocusChanged {
                focus: Some(focus),
            } => Some(focus.session),
            _ => None,
        })
        .collect();
    assert_eq!(changes, vec![OTHER]);
}

#[test]
fn routed_envelope_wins_same_drain_revision_race_once() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let panel = fixture.panel;
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let root_incarnation = incarnation(&mut fixture);
    let tick = world(&mut fixture).tick();
    // Route the tap first: its release-time envelope expects revision 1 and
    // defers. Taps commit on release, so the press alone queues no value.
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
        assert!(context.gui_input_has_deferred());
    }
    // A stale authored write in the same drain cannot twin the routed commit.
    // Subsystem ingress admits before shared service progression, so the
    // routed envelope applies first at the boundary: exactly one commit, no
    // conflict record, and the stale write fails at the mutation boundary.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::SetControlValue {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                    expected_revision: 1,
                    value: GuiControlValue::Bool(true),
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
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
        assert_eq!(
            commits,
            vec![(GuiControlValue::Bool(true), 2, tick + 1, tick + 2)]
        );
        assert!(report.gui_input_conflicts.is_empty());
        assert!(report.gui_unhandled_inputs.is_empty());
        // The stale authored write failed silently at the mutation boundary
        // (fire-and-forget command): no twin commit, revision stays 2.
    }
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
    // Drained effects never repeat.
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_input_effects.is_empty());
}

#[test]
fn unhandled_press_preserves_full_input_for_scene_fallback() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let blocker = EntityId::from_bits(0x7777);
    // A blocked press reaches no GUI target: the complete input stays
    // observable for scene controls, exactly once, with no effect beside it.
    let sent = GuiInputCommand::PointerDown {
        pointer: 1,
        panel: None,
        position: at,
        button: GuiPointerButton::Primary,
        blockers: vec![super::super::super::GuiBlockerHit {
            distance: 0.5,
            entity: blocker,
        }],
        panel_distance: Some(5.0),
    };
    let tick = world(&mut fixture).tick();
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, sent.clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.tick, tick + 1);
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        let unhandled = &report.gui_unhandled_inputs[0];
        assert_eq!(unhandled.session, SESSION);
        assert_eq!(unhandled.source_request_id, 0);
        assert_eq!(unhandled.tick, tick + 1);
        assert_eq!(unhandled.input, sent);
        assert_eq!(
            unhandled.reason,
            GuiUnhandledReason::Blocked {
                entity: blocker
            }
        );
        assert!(report.gui_input_effects.is_empty());
        assert!(report.gui_input_cancellations.is_empty());
        assert!(report.gui_input_conflicts.is_empty());
        assert!(!context.gui_input_has_deferred());
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(report.gui_input_effects.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
}

/// Two overlapping in-world panels under one camera: the nearer panel
/// carries the smaller entity identity, so nearest-wins and
/// greatest-entity-wins disagree. Panels are 4x3 Surfaces with one
/// full-panel root checkbox each; no fonts are needed.
struct ProjectedFixture {
    host: HostRuntime,
    world: WorldId,
    near: EntityId,
    far: EntityId,
}

fn spawn_entity(
    host: &mut HostRuntime,
    world: WorldId,
    alias: u64,
    operations: Vec<Command>,
) -> EntityId {
    let mut full = vec![Command::Create {
        alias: alias as u32,
        metadata: EntityMetadata::default(),
    }];
    full.extend(operations);
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue(Batch {
            id: alias,
            operations: full,
        })
        .unwrap();
    let report = context.step(0.0).unwrap();
    report.outcomes[0].result.as_ref().unwrap()[0].1
}

fn panel_surface() -> Surface {
    let mut surface = Surface::default();
    surface.width = 4.0;
    surface.height = 3.0;
    surface
}

fn panel_transform(x: f32, y: f32, z: f32) -> Transform {
    Transform {
        x,
        y,
        z,
        ..Default::default()
    }
}

fn insert_full_checkbox(host: &mut HostRuntime, world: WorldId, panel: EntityId) {
    let incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_command(
            SESSION,
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation: incarnation,
                id: GuiNodeId(1),
                parent: None,
                index: 0,
                content: GuiNodeContent::Checkbox {
                    checked: false,
                },
                style: GuiNodeStyle {
                    width: Some(4.0),
                    height: Some(3.0),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
}

fn replace_full_checkbox_with_slider(host: &mut HostRuntime, world: WorldId, panel: EntityId) {
    let incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_command(
            SESSION,
            GuiCommand::UpdateNode {
                handle: GuiNodeHandle::new(SESSION, panel, incarnation, GuiNodeId(1), 1),
                patch: GuiNodePatch {
                    content: Some(GuiNodeContent::Slider {
                        value: 0.5,
                        min: 0.0,
                        max: 1.0,
                        step: 0.05,
                    }),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
}

fn setup_projected_with(camera: Camera, near_transform: Transform) -> ProjectedFixture {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let camera = spawn_entity(
        &mut host,
        world,
        1,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Transform(panel_transform(0.0, 0.0, 10.0)),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Camera(camera),
            },
        ],
    );
    let near = spawn_entity(
        &mut host,
        world,
        2,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Alias(2),
                value: ComponentValue::Transform(near_transform),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(2),
                value: ComponentValue::Surface(panel_surface()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(2),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            },
        ],
    );
    let far = spawn_entity(
        &mut host,
        world,
        3,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Alias(3),
                value: ComponentValue::Transform(panel_transform(0.0, 0.0, 0.0)),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(3),
                value: ComponentValue::Surface(panel_surface()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(3),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            },
        ],
    );
    insert_full_checkbox(&mut host, world, near);
    insert_full_checkbox(&mut host, world, far);
    {
        let mut context = host.world_mut(world).unwrap();
        context.enqueue_camera_activate(camera).unwrap();
        context.set_render_viewport(Some((800, 800)));
        context.step(0.0).unwrap();
        assert_eq!(context.active_camera(), Some(camera));
    }
    ProjectedFixture {
        host,
        world,
        near,
        far,
    }
}

fn setup_projected() -> ProjectedFixture {
    setup_projected_with(Camera::default(), panel_transform(0.0, 0.0, 5.0))
}

/// Committed checkbox value and revision on one projected panel.
fn committed_panel(
    host: &mut HostRuntime,
    world: WorldId,
    panel: EntityId,
) -> (GuiControlValue, u32) {
    let inspected = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, Some(GuiNodeId(1)), 1, 4)
        .unwrap();
    let node = inspected
        .nodes
        .iter()
        .find(|inspected| inspected.id == GuiNodeId(1))
        .unwrap();
    (node.control_value.clone(), node.control_revision)
}

/// First reported hover point for one pointer, if routing hovered a target.
fn hovered_point(report: &crate::WorldUpdateReport, pointer: u32) -> Option<[f32; 2]> {
    report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::HoverChanged {
                pointer: observed,
                target: Some(_),
                position,
            } if *observed == pointer => Some(*position),
            _ => None,
        })
        .next()
}

fn assert_point(actual: [f32; 2], expected: [f32; 2]) {
    for (actual, expected) in actual.iter().zip(expected.iter()) {
        assert!(
            (actual - expected).abs() < 1e-3,
            "point {actual:?} != expected {expected:?}"
        );
    }
}

fn committed_entities(report: &crate::WorldUpdateReport) -> Vec<(EntityId, GuiControlValue)> {
    report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                entity,
                value,
                ..
            } => Some((*entity, value.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn perspective_pointer_selects_nearest_not_greatest_entity() {
    let mut fixture = setup_projected();
    assert!(fixture.near < fixture.far);
    let tick = fixture.host.world_mut(fixture.world).unwrap().tick();
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        for command in down_up(1, [0.5, 0.5]) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        // Viewport centre maps through the z=5 Surface to logical (2, 1.5).
        assert_point(hovered_point(&report, 1).unwrap(), [2.0, 1.5]);
        assert!(report.gui_unhandled_inputs.is_empty());
        assert!(context.gui_input_has_deferred());
    }
    let report = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert_eq!(report.tick, tick + 2);
    assert_eq!(
        committed_entities(&report),
        vec![(fixture.near, GuiControlValue::Bool(true))]
    );
    let commit = report
        .gui_input_effects
        .iter()
        .find(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
        .unwrap();
    assert_eq!(
        (commit.source_tick, commit.effect_tick),
        (tick + 1, tick + 2)
    );
    assert_eq!(
        committed_panel(&mut fixture.host, fixture.world, fixture.near),
        (GuiControlValue::Bool(true), 2)
    );
    assert_eq!(
        committed_panel(&mut fixture.host, fixture.world, fixture.far).0,
        GuiControlValue::Bool(false)
    );
}

#[test]
fn perspective_off_center_pixel_maps_through_surface() {
    let mut fixture = setup_projected();
    // World (1, 0.5, 5) from a z=10 camera, fov 45 degrees, aspect 1:
    // extent tan(22.5deg) = 0.41421356, k = 1/5 = 0.2, x = 0.2/0.41421356,
    // y = 0.1/0.41421356, normalized ((x+1)/2, (1-y)/2).
    let at = [0.74142136, 0.37928932];
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        // Content (3, 1) on the 4x3 Surface maps to logical (3, 1).
        assert_point(hovered_point(&report, 1).unwrap(), [3.0, 1.0]);
        assert!(report.gui_unhandled_inputs.is_empty());
    }
    let report = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert_eq!(
        committed_entities(&report),
        vec![(fixture.near, GuiControlValue::Bool(true))]
    );
}

#[test]
fn projected_slider_capture_clamps_beyond_tilted_surface() {
    let angle = 20.0_f32.to_radians();
    let mut fixture = setup_projected_with(
        Camera::default(),
        Transform {
            x: 0.0,
            y: 0.0,
            z: 5.0,
            qy: (angle * 0.5).sin(),
            qw: (angle * 0.5).cos(),
            ..Default::default()
        },
    );
    replace_full_checkbox_with_slider(&mut fixture.host, fixture.world, fixture.near);

    let centre = [0.5, 0.5];
    // This viewport point meets the owned panel's plane beyond its +X edge.
    // Captured motion must retain that panel-space coordinate so the slider
    // clamps to max instead of reinterpreting viewport x=1.2 as logical x.
    let beyond_right = [1.2, 0.5];
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerDown {
                    pointer: 11,
                    panel: None,
                    position: centre,
                    button: GuiPointerButton::Primary,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 11,
                    panel: None,
                    position: beyond_right,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
        assert!(context.gui_input_pressed(11).is_some());
    }
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerUp {
                    pointer: 11,
                    panel: None,
                    position: beyond_right,
                    button: GuiPointerButton::Primary,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
        assert_eq!(context.gui_input_pressed(11), None);
    }
    assert_eq!(
        committed_panel(&mut fixture.host, fixture.world, fixture.near).0,
        GuiControlValue::Scalar(1.0)
    );

    // The opposite off-panel direction still clamps to min, and its off-panel
    // release unwinds capture without replacing the final drag value.
    let beyond_left = [-0.2, 0.5];
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerDown {
                    pointer: 11,
                    panel: None,
                    position: centre,
                    button: GuiPointerButton::Primary,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 11,
                    panel: None,
                    position: beyond_left,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerUp {
                    pointer: 11,
                    panel: None,
                    position: beyond_left,
                    button: GuiPointerButton::Primary,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
        assert_eq!(context.gui_input_pressed(11), None);
    }
    fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert_eq!(
        committed_panel(&mut fixture.host, fixture.world, fixture.near).0,
        GuiControlValue::Scalar(0.0)
    );

    // Cancellation stays independent of projection and removes the capture
    // plus its provisional press-time value.
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerDown {
                    pointer: 11,
                    panel: None,
                    position: centre,
                    button: GuiPointerButton::Primary,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerCancel {
                    pointer: 11,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
        assert_eq!(context.gui_input_pressed(11), None);
    }
    fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert_eq!(
        committed_panel(&mut fixture.host, fixture.world, fixture.near).0,
        GuiControlValue::Scalar(0.0)
    );
}

#[test]
fn orthographic_pointer_selects_nearest_panel() {
    let mut fixture = setup_projected_with(
        Camera {
            projection: 1,
            ortho_height: 4.0,
            ..Default::default()
        },
        panel_transform(0.0, 0.0, 5.0),
    );
    // Ortho half-extent 2 over aspect 1: world (1, 0.5) is ((1+2)/4, (1-0.25)/2).
    let at = [0.75, 0.375];
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        assert_point(hovered_point(&report, 1).unwrap(), [3.0, 1.0]);
        assert!(report.gui_unhandled_inputs.is_empty());
    }
    let report = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert_eq!(
        committed_entities(&report),
        vec![(fixture.near, GuiControlValue::Bool(true))]
    );
    assert_eq!(
        committed_panel(&mut fixture.host, fixture.world, fixture.far).0,
        GuiControlValue::Bool(false)
    );
}

#[test]
fn back_facing_panel_never_wins() {
    // The nearer panel turns its +Z face away (half-turn about Y): the ray
    // meets its back first and must fall through to the farther panel.
    let mut fixture = setup_projected_with(
        Camera::default(),
        Transform {
            x: 0.0,
            y: 0.0,
            z: 5.0,
            qx: 0.0,
            qy: 1.0,
            qz: 0.0,
            qw: 0.0,
            ..Default::default()
        },
    );
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        for command in down_up(1, [0.5, 0.5]) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        let report = context.step(0.0).unwrap();
        assert_point(hovered_point(&report, 1).unwrap(), [2.0, 1.5]);
        assert!(report.gui_unhandled_inputs.is_empty());
    }
    let report = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert_eq!(
        committed_entities(&report),
        vec![(fixture.far, GuiControlValue::Bool(true))]
    );
    assert_eq!(
        committed_panel(&mut fixture.host, fixture.world, fixture.near).0,
        GuiControlValue::Bool(false)
    );
}

#[test]
fn marked_blocker_resolves_from_scene_geometry() {
    let mut fixture = setup_projected();
    let blocker = spawn_entity(
        &mut fixture.host,
        fixture.world,
        4,
        vec![
            Command::InsertComponentValue {
                entity: EntityRef::Alias(4),
                value: ComponentValue::Transform(panel_transform(0.0, 0.0, 7.5)),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(4),
                value: ComponentValue::PickingGeometry(PickingGeometry {
                    geometry: GeometryDefinition::from(GeometryShape::Box {
                        min: [-1.0; 3],
                        max: [1.0; 3],
                    })
                    .encode()
                    .unwrap(),
                    ..Default::default()
                }),
            },
        ],
    );
    // A stale caller distance (100.0, far behind both panels) must not
    // decide: the evaluated box meets the ray at t=1.5, ahead of the near
    // panel at t=5, so the press blocks.
    let blocked = GuiInputCommand::PointerDown {
        pointer: 1,
        panel: None,
        position: [0.5, 0.5],
        button: GuiPointerButton::Primary,
        blockers: vec![super::super::super::GuiBlockerHit {
            distance: 100.0,
            entity: blocker,
        }],
        panel_distance: None,
    };
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        context.enqueue_gui_input_command(SESSION, blocked).unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        assert_eq!(
            report.gui_unhandled_inputs[0].reason,
            GuiUnhandledReason::Blocked {
                entity: blocker
            }
        );
        assert!(!context.gui_input_has_deferred());
    }
    let report = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert!(committed_entities(&report).is_empty());
    // Unmarked geometry never blocks: the same press without the mark
    // reaches the near panel.
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        for command in down_up(2, [0.5, 0.5]) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    }
    let report = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert_eq!(
        committed_entities(&report),
        vec![(fixture.near, GuiControlValue::Bool(true))]
    );
}

#[test]
fn viewport_density_and_resize_keep_viewport_mapping() {
    let mut fixture = setup_projected();
    // Same aspect at doubled density: identical normalized points route
    // identically (DPI cancels in the adapter normalization).
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        context.set_render_viewport(Some((1600, 1600)));
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 9,
                    panel: None,
                    position: [0.74142136, 0.37928932],
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_point(hovered_point(&report, 9).unwrap(), [3.0, 1.0]);
    }
    // Resized aspect remaps through the current viewport: aspect 2 halves
    // the horizontal extent, so world x=1 sits at (0.2/0.82842712+1)/2.
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        context.set_render_viewport(Some((800, 400)));
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerMove {
                    pointer: 10,
                    panel: None,
                    position: [0.6207107, 0.3792893],
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_point(hovered_point(&report, 10).unwrap(), [3.0, 1.0]);
    }
}

#[test]
fn camera_only_move_retargets_without_reflow() {
    let mut fixture = setup_projected();
    let before = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .gui_layout_view(fixture.near)
        .unwrap();
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        // Orbiting preserves the focus pivot (default focus distance 6
        // puts it at z=4 with the camera at z=10), so the swung center ray
        // meets z=5 at x=tan(yaw): yaw 1.2 clears both 4-wide panels.
        context
            .enqueue_camera_navigate(CameraMotion::Rotate {
                yaw: 1.2,
                pitch: 0.0,
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    // Camera motion never reflows layout or remeasures text.
    let after = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .gui_layout_view(fixture.near)
        .unwrap();
    assert_eq!(before.layout_revision, after.layout_revision);
    assert_eq!(before.reflow_count, after.reflow_count);
    assert_eq!(before.remeasure_count, after.remeasure_count);
    // The same viewport point now misses both panels: the swung ray leaves
    // the 4x3 extents, proving routing used the current camera.
    {
        let mut context = fixture.host.world_mut(fixture.world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, down_up(1, [0.5, 0.5])[0].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        assert_eq!(
            report.gui_unhandled_inputs[0].reason,
            GuiUnhandledReason::NoPanelHit
        );
        assert!(!context.gui_input_has_deferred());
    }
    let report = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .step(0.0)
        .unwrap();
    assert!(committed_entities(&report).is_empty());
    assert_eq!(
        committed_panel(&mut fixture.host, fixture.world, fixture.near).0,
        GuiControlValue::Bool(false)
    );
}

#[test]
fn checkbox_tap_commits_on_release_only() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    // Pressing stages a provisional press only: holds across ticks change
    // nothing, and the press cursor is observable meanwhile.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context.step(0.0).unwrap();
        assert!(context.gui_input_pressed(1).is_some());
    }
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 1)
    );
    world(&mut fixture).step(0.0).unwrap();
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 1)
    );
    // Releasing outside the control completes nothing.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerUp {
                    pointer: 1,
                    panel: None,
                    position: [5.0, 5.0],
                    button: GuiPointerButton::Primary,
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
        assert_eq!(context.gui_input_pressed(1), None);
    }
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 1)
    );
    // Cancelling discards the pending activation instead of retracting an
    // effect: nothing was ever published.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(2, at)[0].clone())
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerCancel {
                    pointer: 2,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(SESSION, down_up(2, at)[1].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(
            report
                .gui_unhandled_inputs
                .iter()
                .any(|unhandled| unhandled.reason == GuiUnhandledReason::NoCapture)
        );
    }
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 1)
    );
    world(&mut fixture).step(0.0).unwrap();
    // An in-bounds tap commits exactly once.
    {
        let mut context = world(&mut fixture);
        for command in down_up(3, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    }
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 1)
    );
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
    assert_eq!(commits, vec![(GuiControlValue::Bool(true), 2)]);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
}

#[test]
fn cancel_across_steps_discards_provisional_press() {
    // R08 cross-step case: down, cancel and up arrive on separate ticks
    // (one Host round-trip each). The provisional press must never commit:
    // the value and revision hold, no commit effect publishes.
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    // Commit true first so the test mirrors the scenario: a later stray
    // toggle would flip it back to false.
    {
        let mut context = world(&mut fixture);
        for command in down_up(9, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context.step(0.0).unwrap();
        assert!(context.gui_input_pressed(1).is_some());
    }
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerCancel {
                    pointer: 1,
                },
            )
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_input_conflicts.len(), 0);
    }
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[1].clone())
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(
            report
                .gui_unhandled_inputs
                .iter()
                .any(|unhandled| unhandled.reason == GuiUnhandledReason::NoCapture)
        );
    }
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(true), 2)
    );
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(report.gui_input_effects.is_empty());
}
