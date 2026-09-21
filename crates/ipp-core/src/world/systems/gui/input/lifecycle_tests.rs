//! Behavioural tests for GUI input lifecycle and commit fencing.

use super::super::super::test_support::font_source;
use super::test_support::*;
use super::*;
use crate::{
    Batch, Command, ComponentOverlayMode, ComponentValue, DynamicValue, EntityMetadata,
    EntityOverlayMode, EntityRef, StateOverlayRef,
};
use crate::{
    GuiCommand, GuiContainerKind, GuiEvaluatedContent, GuiNodeContent, GuiNodeId, GuiNodePatch,
    GuiNodeStyle,
};

#[test]
fn node_removal_without_new_input_invalidates_all_retained_cursors() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let panel = fixture.panel;
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let root_incarnation = incarnation(&mut fixture);

    // Establish input-owned scroll first, then retain a press/focus/hover on
    // the same fully fenced target without releasing the pointer.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Scroll {
                    panel: Some(panel),
                    position: at,
                    delta: [1.0, 2.0],
                    blockers: Vec::new(),
                    panel_distance: None,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    {
        let context = world(&mut fixture);
        assert!(context.gui_input_focus().is_some());
        assert!(context.gui_input_hover(1).is_some());
        assert!(context.gui_input_pressed(1).is_some());
        assert_eq!(context.gui_input_scroll(panel, GuiNodeId(2)), [1.0, 2.0]);
    }

    let report = {
        let mut context = world(&mut fixture);
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
    assert!(world(&mut fixture).gui_input_focus().is_none());
    assert!(world(&mut fixture).gui_input_hover(1).is_none());
    assert!(world(&mut fixture).gui_input_pressed(1).is_none());
    assert_eq!(
        world(&mut fixture).gui_input_scroll(panel, GuiNodeId(2)),
        [0.0, 0.0]
    );
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::FocusChanged {
            focus: None
        }
    )));
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::HoverChanged {
            pointer: 1,
            target: None,
            ..
        }
    )));
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::ScrollChanged {
            node: GuiNodeId(2),
            offset: [0.0, 0.0],
            ..
        }
    )));
    assert!(report.gui_input_cancellations.iter().any(|cancellation| {
        cancellation.reason == GuiInputCancelReason::TargetRemoved
            && cancellation
                .target
                .is_some_and(|target| target.node == GuiNodeId(2))
    }));
}

#[test]
fn disabling_focused_text_without_new_input_clears_caret_and_composition() {
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
                    start: 0,
                    end: 1,
                },
            )
            .unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::UpdateComposition {
                    text: "a".into(),
                    caret_start: 0,
                    caret_end: 1,
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
    assert!(
        world(&mut fixture)
            .gui_text_composition(panel, GuiNodeId(2))
            .is_some()
    );

    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
                    patch: GuiNodePatch {
                        enabled: Some(false),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(world(&mut fixture).gui_input_focus().is_none());
    assert!(
        world(&mut fixture)
            .gui_text_selection(panel, GuiNodeId(2))
            .is_none()
    );
    assert!(
        world(&mut fixture)
            .gui_text_composition(panel, GuiNodeId(2))
            .is_none()
    );
    assert!(matches!(
        report.gui_text_focus_updates.as_slice(),
        [GuiTextFocusUpdate::Cleared {
            session: SESSION,
            ..
        }]
    ));
}

#[test]
fn ready_text_becoming_unavailable_without_new_input_clears_focus() {
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
    assert!(world(&mut fixture).gui_input_focus().is_some());

    let font = fixture
        .host
        .asset_resources()
        .find(&font_source())
        .expect("registered font");
    fixture.host.asset_resources_mut().unload(font);

    // Borrowing the World flushes the Host's release barrier first. Layout
    // then publishes the unavailable node before input's idle reconciliation.
    let report = world(&mut fixture).step(0.0).unwrap();
    let view = world(&mut fixture).gui_layout_view(panel).unwrap();
    assert!(
        view.nodes
            .iter()
            .any(|node| node.node == GuiNodeId(2) && !node.available)
    );
    assert!(world(&mut fixture).gui_input_focus().is_none());
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::FocusChanged {
            focus: None
        }
    )));
    assert!(matches!(
        report.gui_text_focus_updates.as_slice(),
        [GuiTextFocusUpdate::Cleared {
            session: SESSION,
            ..
        }]
    ));
}

#[test]
fn root_replacement_without_new_input_does_not_transfer_focus() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let panel = fixture.panel;
    let old_incarnation = incarnation(&mut fixture);
    {
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle: GuiNodeHandle::new(SESSION, panel, old_incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    world(&mut fixture).step(0.0).unwrap();
    assert!(world(&mut fixture).gui_input_focus().is_some());

    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::RemoveComponent {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::GUI_ROOT,
                }],
            })
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(world(&mut fixture).gui_input_focus().is_none());
    assert!(report.gui_input_effects.iter().any(|effect| matches!(
        effect.kind,
        GuiInputEffectKind::FocusChanged {
            focus: None
        }
    )));
    {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::InsertComponentValue {
                    entity: EntityRef::Handle(panel),
                    value: ComponentValue::GuiRoot(GuiRoot::default()),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    assert_ne!(incarnation(&mut fixture), old_incarnation);
    assert!(world(&mut fixture).gui_input_focus().is_none());
}

#[test]
fn authored_commit_before_routing_chains_prediction() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let root_incarnation = incarnation(&mut fixture);
    // Routing runs after the current evaluation, so the tap predicts on
    // the authored commit instead of racing it: no conflict, one chained
    // toggle on top.
    {
        let panel = fixture.panel;
        let mut context = world(&mut fixture);
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[0].clone())
            .unwrap();
        // Taps commit on release: complete the tap in the same drain.
        context
            .enqueue_gui_input_command(SESSION, down_up(1, at)[1].clone())
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
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_input_cancellations.is_empty());
    let toggles: Vec<(GuiControlValue, u32)> = report
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
    assert_eq!(toggles, vec![(GuiControlValue::Bool(false), 3)]);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)),
        (GuiControlValue::Bool(false), 3)
    );
}

#[test]
fn control_commit_gate_fences_sessions_and_revisions() {
    let content = GuiNodeContent::Checkbox {
        checked: false,
    };
    let mut root = GuiRoot::default();
    root.nodes_mut()
        .insert_node(GuiNodeId(1), None, 0, content.clone())
        .unwrap();
    root.controls_mut().insert_initial(GuiNodeId(1), &content);
    let handle =
        |session| GuiNodeHandle::new(session, EntityId::from_bits(0x42), 3, GuiNodeId(1), 1);
    // A foreign session is refused without touching the value.
    assert!(
        commit_control_value(
            &mut root,
            3,
            9,
            &handle(SESSION),
            1,
            &GuiControlValue::Bool(true)
        )
        .is_err()
    );
    // The owning session commits revision 1 to 2.
    commit_control_value(
        &mut root,
        3,
        SESSION,
        &handle(SESSION),
        1,
        &GuiControlValue::Bool(true),
    )
    .unwrap();
    assert_eq!(
        root.control_state(GuiNodeId(1)).map(|state| state.revision),
        Some(2)
    );
    // A stale revision is refused: the admission gate holds.
    assert!(
        commit_control_value(
            &mut root,
            3,
            SESSION,
            &handle(SESSION),
            1,
            &GuiControlValue::Bool(false)
        )
        .is_err()
    );
}

/// Insert a column panel holding one painted checkbox with producer opacity.
fn insert_opacity_checkbox(fixture: &mut Fixture) {
    let root_incarnation = incarnation(fixture);
    let panel = fixture.panel;
    let commands = vec![
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
            style: GuiNodeStyle {
                width: Some(4.0),
                height: Some(3.0),
                background_color: Some([0.0, 0.0, 1.0, 1.0]),
                ..Default::default()
            },
        },
    ];
    let mut context = world(fixture);
    for command in commands {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

/// Effective inspected style opacity of one node.
fn inspected_opacity(fixture: &mut Fixture, node: GuiNodeId) -> f32 {
    let panel = fixture.panel;
    world(fixture)
        .inspect_gui(panel, Some(node), 1, 4)
        .unwrap()
        .nodes
        .iter()
        .find(|inspected| inspected.id == node)
        .map(|inspected| inspected.style.opacity)
        .unwrap()
}

/// Retained evaluated opacity of one node.
fn evaluated_opacity(fixture: &mut Fixture, node: GuiNodeId) -> f32 {
    let panel = fixture.panel;
    world(fixture)
        .gui_layout_view(panel)
        .unwrap()
        .nodes
        .iter()
        .find(|evaluated| evaluated.node == node)
        .map(|evaluated| evaluated.opacity)
        .unwrap()
}

/// Retained paint evidence: the background-box primitive opacity of one node.
fn committed_primitive_opacity(fixture: &mut Fixture, node: GuiNodeId) -> f32 {
    let panel = fixture.panel;
    world(fixture)
        .gui_layout_view(panel)
        .unwrap()
        .surface_primitives()
        .iter()
        .filter_map(|primitive| match primitive {
            crate::systems::surface::SurfaceRenderPrimitive::Box {
                style,
                ..
            } if matches!(
                style.identity,
                crate::systems::surface::SurfacePrimitiveIdentity::Gui(id)
                    if id.node == node
                        && id.lifetime == 1
                        && id.part == crate::systems::surface::GuiPrimitivePart::Background
            ) =>
            {
                Some(style.opacity)
            }
            _ => None,
        })
        .next()
        .unwrap()
}

#[test]
fn committed_toggle_retains_bound_style_overlay_and_restyles_on_release() {
    let mut fixture = setup();
    insert_opacity_checkbox(&mut fixture);
    // Bind the panel entity symbolically so a Bound overlay can observe it.
    {
        let panel = fixture.panel;
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::SetMetadata {
                    entity: EntityRef::Handle(panel),
                    metadata: EntityMetadata {
                        symbolic_id: Some("overlay-panel".into()),
                        classes: vec![],
                    },
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    // A live Bound style override: effective 0.25 over producer 1.0.
    let owner = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![
                    Command::CreateStateOverlayOwner {
                        alias: 10,
                    },
                    Command::AttachEntityOverlayBinding {
                        owner: StateOverlayRef::Alias(10),
                        alias: 11,
                        symbolic_id: "overlay-panel".into(),
                        mode: EntityOverlayMode::Bound,
                    },
                    Command::AttachComponentStateOverlay {
                        owner: StateOverlayRef::Alias(10),
                        binding: StateOverlayRef::Alias(11),
                        alias: 12,
                        component: ComponentValue::GUI_ROOT,
                        mode: ComponentOverlayMode::Bound,
                        fields: vec![],
                    },
                    Command::UpdateDynamicComponentStateOverlay {
                        owner: StateOverlayRef::Alias(10),
                        overlay: StateOverlayRef::Alias(12),
                        properties: vec![("node_2_opacity".to_owned(), DynamicValue::F32(0.25))],
                        clear: vec![],
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.outcomes[0].result.is_ok(), "{report:?}");
        report.outcomes[0]
            .state_overlays
            .iter()
            .find(|alias| alias.alias == 10)
            .expect("overlay owner handle")
            .id
    };
    assert_eq!(inspected_opacity(&mut fixture, GuiNodeId(2)), 0.25);
    let before = committed_bool(&mut fixture, GuiNodeId(2));
    assert_eq!(before.0, GuiControlValue::Bool(false));
    // A full tap must preserve the overlay regardless of whether activation
    // commits on press or release.
    let at = node_centre(&mut fixture, GuiNodeId(2));
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    }
    let mut committed = false;
    for _ in 0..4 {
        let report = world(&mut fixture).step(0.0).unwrap();
        if report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
        {
            committed = true;
            break;
        }
    }
    assert!(committed, "checkbox toggle never committed");
    // Committed frame: the value toggled while every overlay-styled output
    // still observes the live Bound override, not the producer value.
    let after = committed_bool(&mut fixture, GuiNodeId(2));
    assert_eq!(after.0, GuiControlValue::Bool(true));
    assert!(after.1 > before.1, "toggle must advance the revision");
    assert_eq!(inspected_opacity(&mut fixture, GuiNodeId(2)), 0.25);
    assert_eq!(evaluated_opacity(&mut fixture, GuiNodeId(2)), 0.25);
    assert_eq!(
        committed_primitive_opacity(&mut fixture, GuiNodeId(2)),
        0.25
    );
    match evaluated_content(&mut fixture, GuiNodeId(2)) {
        GuiEvaluatedContent::Checkbox {
            checked,
            ..
        } => assert!(checked),
        other => panic!("expected checkbox, got {other:?}"),
    }
    // Releasing the overlay restyles to the intact underlying producer value.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::ReleaseStateOverlayOwner {
                    owner: StateOverlayRef::Handle(owner),
                }],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    }
    assert_eq!(inspected_opacity(&mut fixture, GuiNodeId(2)), 1.0);
    assert_eq!(evaluated_opacity(&mut fixture, GuiNodeId(2)), 1.0);
    assert_eq!(
        committed_bool(&mut fixture, GuiNodeId(2)).0,
        GuiControlValue::Bool(true)
    );
}

#[test]
fn committed_toggle_pins_runtime_ancestor_path_and_drains_once() {
    let mut fixture = setup();
    insert_nodes(&mut fixture, false, false);
    let panel = fixture.panel;
    let root_incarnation = incarnation(&mut fixture);
    let at = node_centre(&mut fixture, GuiNodeId(2));
    let tick = world(&mut fixture).tick();
    {
        let mut context = world(&mut fixture);
        for command in down_up(1, at) {
            context.enqueue_gui_input_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    }
    let report = world(&mut fixture).step(0.0).unwrap();
    assert_eq!(report.tick, tick + 2);
    let commits = report
        .gui_input_effects
        .iter()
        .filter_map(|effect| match &effect.kind {
            GuiInputEffectKind::ControlCommitted {
                entity,
                root_incarnation: effect_root,
                node,
                lifetime,
                value,
                revision,
                path,
            } => Some((
                *entity,
                *effect_root,
                *node,
                *lifetime,
                path.clone(),
                value.clone(),
                *revision,
                effect.session,
                effect.source_tick,
                effect.effect_tick,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    // Exactly one commit carrying session, ticks and the pinned root-first
    // ancestor path the reconciler dispatches along.
    assert_eq!(
        commits,
        vec![(
            panel,
            root_incarnation,
            GuiNodeId(2),
            1,
            vec![GuiNodeId(1), GuiNodeId(2)],
            GuiControlValue::Bool(true),
            2,
            SESSION,
            tick + 1,
            tick + 2,
        )]
    );
    assert!(report.gui_input_cancellations.is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
    // Drained observations never repeat: the next frame is quiet.
    let report = world(&mut fixture).step(0.0).unwrap();
    assert!(report.gui_input_effects.is_empty());
    assert!(report.gui_input_cancellations.is_empty());
    assert!(report.gui_input_conflicts.is_empty());
    assert!(report.gui_unhandled_inputs.is_empty());
}
