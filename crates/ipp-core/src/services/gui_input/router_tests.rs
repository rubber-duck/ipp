use super::*;
use crate::services::gui_input::router::*;

struct DeliveryFactory(Rc<RefCell<Delivery>>);

fn nested_scroll_scene(capacity: f32) -> Scene {
    use crate::components::{GuiVirtualItem, GuiVirtualList};
    let mut scene = scene();
    let scopes = [
        (scene.root, scene.root_entity, 300.0),
        (scene.child, scene.child_entity, 100.0 + capacity),
    ];
    for (output, entity, extent) in scopes {
        scene
            .host
            .canvas_output(output.world(), [100.0, 100.0], 1.0);
        let mut commands = vec![
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 100.0,
                    height: 100.0,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiVirtualList(GuiVirtualList {
                    item_count: if output == scene.root {
                        3
                    } else {
                        1
                    },
                    item_extent: if output == scene.root {
                        100.0
                    } else {
                        extent
                    },
                    axis: 1,
                    overscan: 0,
                    ..Default::default()
                }),
            ),
        ];
        if output == scene.root {
            commands.push(Command::insert_value(
                EntityRef::Handle(scene.anchor),
                ComponentValue::GuiVirtualItem(GuiVirtualItem {
                    index: 0,
                }),
            ));
        } else {
            commands.push(Command::Delete {
                entity: EntityRef::Handle(scene.target.entity),
            });
        }
        apply(&mut scene.host, output.world(), commands);
    }
    apply(
        &mut scene.host,
        scene.root.world(),
        vec![Command::insert_value(
            EntityRef::Handle(scene.anchor),
            ComponentValue::WorldAttachment(WorldAttachment::surface(scene.child)),
        )],
    );
    scene
        .host
        .set_root_output(
            scene.root,
            WorldViewport {
                width: 128,
                height: 128,
                device_pixel_ratio: 1.0,
            },
        )
        .unwrap();
    scene.host.frame(0.0).unwrap();
    scene
}

fn scroll_offset(host: &mut HostRuntime, output: OutputRef, entity: EntityId) -> f32 {
    let snapshot = crate::systems::gui::test_support::read_control(
        &host.world_mut(output.world().id()).unwrap(),
        entity,
    )
    .unwrap();
    let crate::systems::gui::test_support::GuiTestValue::Scroll(offset) = snapshot.value else {
        panic!("expected scrolling control");
    };
    offset[1]
}

fn overlapping_scroll_samples(drag: bool) {
    let mut scene = nested_scroll_scene(5.0);
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: scene.root,
        expected_viewport: WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        },
    };
    let first_view = scene.host.resolve_view(query).unwrap();
    if drag {
        router
            .route(
                &mut scene.host,
                &mut context,
                first_view,
                GuiPhysicalInput::PointerDown {
                    pointer: 1,
                    point: [0.5, 0.5],
                    button: GuiPhysicalButton::Primary,
                },
                &mut DeliveryFactory(scene.ledger.clone()),
            )
            .unwrap();
    }
    let first = Rc::new(RefCell::new(Delivery::default()));
    let second = Rc::new(RefCell::new(Delivery::default()));
    let sample = |point| {
        if drag {
            GuiPhysicalInput::PointerMove {
                pointer: 1,
                point: [0.5, point],
            }
        } else {
            GuiPhysicalInput::Wheel {
                point: [0.5, 0.5],
                delta: [0.0, 20.0],
                shift: false,
            }
        }
    };
    router
        .route(
            &mut scene.host,
            &mut context,
            first_view,
            sample(0.34375),
            &mut DeliveryFactory(first.clone()),
        )
        .unwrap();
    let first_remainder = router.scroll_remainder(&context).unwrap();
    scene.host.frame(0.0).unwrap();
    assert_eq!(
        scroll_offset(&mut scene.host, scene.child, scene.child_entity),
        5.0
    );
    assert_eq!(
        scroll_offset(&mut scene.host, scene.root, scene.root_entity),
        0.0
    );
    assert_eq!(first.borrow().reserved, 1, "outer A must still be pending");
    let second_view = scene.host.resolve_view(query).unwrap();
    assert_ne!(second_view.publication, first_view.publication);
    router
        .route(
            &mut scene.host,
            &mut context,
            second_view,
            sample(0.1875),
            &mut DeliveryFactory(second.clone()),
        )
        .unwrap();
    let second_remainder = router.scroll_remainder(&context).unwrap();
    scene.host.frame(0.0).unwrap();
    assert_eq!(
        scroll_offset(&mut scene.host, scene.root, scene.root_entity),
        15.0
    );
    assert_eq!(first.borrow().reserved, 0);
    assert_eq!(second.borrow().reserved, 1);
    assert_eq!(first_remainder.remaining().unwrap(), [0.0, 0.0]);
    scene.host.frame(0.0).unwrap();
    assert_eq!(
        scroll_offset(&mut scene.host, scene.child, scene.child_entity),
        5.0
    );
    assert_eq!(
        scroll_offset(&mut scene.host, scene.root, scene.root_entity),
        35.0
    );
    assert_eq!(second_remainder.remaining().unwrap(), [0.0, 0.0]);
    for (ledger, source, inner_moved) in [
        (&first, first_view.publication, true),
        (&second, second_view.publication, false),
    ] {
        assert_eq!(ledger.borrow().reserved, 0);
        let effects: Vec<_> = terminals(ledger)
            .into_iter()
            .filter_map(|terminal| match terminal {
                GuiDeliveryTerminal::Written {
                    target,
                    source: written,
                    changed,
                } => {
                    assert_eq!(
                        written,
                        GuiLocalEffectSource::Routed {
                            publication: source
                        }
                    );
                    Some((target.world, changed))
                }
                GuiDeliveryTerminal::Applied(effect) => {
                    assert_eq!(
                        effect.source,
                        GuiLocalEffectSource::Routed {
                            publication: source
                        }
                    );
                    None
                }
                terminal => panic!("unexpected terminal {terminal:?}"),
            })
            .collect();
        // Each sample reaches the inner view, then the outer one with the
        // remainder; the first fills the inner capacity, so the second moves
        // only the outer view. The offsets are in the fields.
        assert_eq!(
            effects,
            [
                (scene.child.world(), inner_moved),
                (scene.root.world(), true)
            ]
        );
    }
    scene.host.frame(0.0).unwrap();
    assert_eq!(
        scroll_offset(&mut scene.host, scene.child, scene.child_entity),
        5.0
    );
    assert_eq!(
        scroll_offset(&mut scene.host, scene.root, scene.root_entity),
        35.0
    );
    router.release(&mut scene.host, context);
    scene.host.frame(0.0).unwrap();
    assert_eq!(scene.ledger.borrow().reserved, 0);
    assert!(scene.host.publication(first_view.publication).is_none());
    assert!(scene.host.publication(second_view.publication).is_none());
}

#[test]
fn overlapping_cross_world_wheels_keep_the_pending_outer_predecessor() {
    overlapping_scroll_samples(false);
}

#[test]
fn continuous_cross_world_drag_keeps_the_pending_outer_predecessor() {
    overlapping_scroll_samples(true);
}

#[test]
fn cross_world_scroll_parks_outer_fifo_and_retains_original_publication() {
    for capacity in [100.0, 5.0] {
        for drag in [false, true] {
            let mut scene = nested_scroll_scene(capacity);
            let mut router = GuiInputRouter::default();
            let (mut context, _) = router
                .bind(&scene.host, scene.root.world(), 900, Vec::new())
                .unwrap();
            let view = scene
                .host
                .resolve_view(crate::ViewQueryTarget::RootView {
                    output: scene.root,
                    expected_viewport: WorldViewport {
                        width: 128,
                        height: 128,
                        device_pixel_ratio: 1.0,
                    },
                })
                .unwrap();
            let mut delivery = DeliveryFactory(scene.ledger.clone());
            if drag {
                router
                    .route(
                        &mut scene.host,
                        &mut context,
                        view,
                        GuiPhysicalInput::PointerDown {
                            pointer: 1,
                            point: [0.5, 0.5],
                            button: GuiPhysicalButton::Primary,
                        },
                        &mut delivery,
                    )
                    .unwrap();
            }
            router
                .route(
                    &mut scene.host,
                    &mut context,
                    view,
                    if drag {
                        GuiPhysicalInput::PointerMove {
                            pointer: 1,
                            point: [0.5, 0.34375],
                        }
                    } else {
                        GuiPhysicalInput::Wheel {
                            point: [0.5, 0.5],
                            delta: [0.0, 20.0],
                            shift: false,
                        }
                    },
                    &mut delivery,
                )
                .unwrap();
            let remainder = router.scroll_remainder(&context).unwrap();
            scene
                .host
                .world_mut(scene.root.world().id())
                .unwrap()
                .enqueue(Batch {
                    id: 77,
                    operations: vec![Command::Create {
                        alias: 77,
                        metadata: Default::default(),
                        adopt: false,
                    }],
                })
                .unwrap();
            let first = scene.host.frame(0.0).unwrap();
            assert_eq!(
                scroll_offset(&mut scene.host, scene.child, scene.child_entity),
                capacity.min(20.0),
                "inner must consume first, drag={drag}"
            );
            assert_eq!(
                scroll_offset(&mut scene.host, scene.root, scene.root_entity),
                0.0
            );
            assert_eq!(
                scene.ledger.borrow().reserved,
                1,
                "outer input must remain parked"
            );
            assert!(
                scene.host.publication(view.publication).is_some(),
                "admitted source must survive normal turnover"
            );
            assert!(
                first.worlds[&scene.root.world().id()]
                    .as_ref()
                    .unwrap()
                    .outcomes
                    .is_empty(),
                "later parent batch overtook parked input"
            );
            let second = scene.host.frame(0.0).unwrap();
            assert_eq!(
                scroll_offset(&mut scene.host, scene.root, scene.root_entity),
                20.0 - capacity.min(20.0)
            );
            assert_eq!(remainder.remaining().unwrap(), [0.0, 0.0]);
            let outcomes = &second.worlds[&scene.root.world().id()]
                .as_ref()
                .unwrap()
                .outcomes;
            assert_eq!(outcomes.len(), 1);
            assert_eq!(outcomes[0].batch_id, 77);
            assert!(outcomes[0].result.is_ok());
            assert_eq!(scene.ledger.borrow().reserved, 0);
            router.release(&mut scene.host, context);
        }
    }
}

#[test]
fn parked_scroll_drains_and_terminal_failures_release_all_tickets() {
    for failure in [false, true] {
        let mut scene = nested_scroll_scene(5.0);
        let mut router = GuiInputRouter::default();
        let (mut context, _) = router
            .bind(&scene.host, scene.root.world(), 900, Vec::new())
            .unwrap();
        let view = scene
            .host
            .resolve_view(crate::ViewQueryTarget::RootView {
                output: scene.root,
                expected_viewport: WorldViewport {
                    width: 128,
                    height: 128,
                    device_pixel_ratio: 1.0,
                },
            })
            .unwrap();
        router
            .route(
                &mut scene.host,
                &mut context,
                view,
                GuiPhysicalInput::Wheel {
                    point: [0.5, 0.5],
                    delta: [0.0, 20.0],
                    shift: false,
                },
                &mut DeliveryFactory(scene.ledger.clone()),
            )
            .unwrap();
        scene.ledger.borrow_mut().fail_effect = failure;
        scene.host.frame(0.0).unwrap();
        scene.host.frame(0.0).unwrap();
        assert_eq!(scene.ledger.borrow().reserved, 0);
        assert_eq!(
            scroll_offset(&mut scene.host, scene.root, scene.root_entity),
            if failure {
                0.0
            } else {
                15.0
            }
        );
        assert!(scene.host.publication(view.publication).is_none());
        router.release(&mut scene.host, context);
    }
    for disconnect in [false, true] {
        let mut scene = nested_scroll_scene(5.0);
        let mut router = GuiInputRouter::default();
        let (mut context, _) = router
            .bind(&scene.host, scene.root.world(), 900, Vec::new())
            .unwrap();
        let viewport = WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        };
        let view = scene
            .host
            .resolve_view(crate::ViewQueryTarget::RootView {
                output: scene.root,
                expected_viewport: viewport,
            })
            .unwrap();
        router
            .route(
                &mut scene.host,
                &mut context,
                view,
                GuiPhysicalInput::Wheel {
                    point: [0.5, 0.5],
                    delta: [0.0, 20.0],
                    shift: false,
                },
                &mut DeliveryFactory(scene.ledger.clone()),
            )
            .unwrap();
        scene.host.frame(0.0).unwrap();
        if disconnect {
            router.release(&mut scene.host, context);
        } else {
            scene.host.set_root_output(scene.root, viewport).unwrap();
            scene.host.frame(0.0).unwrap();
            router.release(&mut scene.host, context);
        }
        scene.host.frame(0.0).unwrap();
        assert_eq!(scene.ledger.borrow().reserved, 0);
        assert_eq!(
            scroll_offset(&mut scene.host, scene.root, scene.root_entity),
            0.0
        );
        assert!(scene.host.publication(view.publication).is_none());
    }
}

#[test]
fn native_edits_preserve_graphemes_provisional_text_and_exact_replacement_fences() {
    use crate::systems::gui::local::{GuiTextComposition, GuiTextEdit};
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas(&mut host, world);
    let entity = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiTextInput(crate::components::GuiTextInput {
                text: "a\u{301}b".into(),
                ..Default::default()
            }),
            ComponentValue::GuiLayout(GuiLayout {
                width: 100.0,
                height: 30.0,
                ..Default::default()
            }),
        ],
        Some(root_entity),
    );
    let viewport = WorldViewport {
        width: 128,
        height: 128,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(root, viewport).unwrap();
    host.frame(0.0).unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: root,
        expected_viewport: viewport,
    };
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router.bind(&host, world, 900, Vec::new()).unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = DeliveryFactory(ledger.clone());
    let view = host.resolve_view(query).unwrap();
    router
        .route(
            &mut host,
            &mut context,
            view,
            GuiPhysicalInput::Key {
                key: GuiPhysicalKey::Tab,
                shift: false,
            },
            &mut delivery,
        )
        .unwrap();
    host.frame(0.0).unwrap();
    let mut state = router.with_native_text(&mut host, &context, |state| state.unwrap().clone());
    let original = state.fence;
    for edit in [
        GuiTextEdit::Backspace,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "é".into(),
            selection: [2, 2],
        }),
        GuiTextEdit::CancelComposition,
        GuiTextEdit::Backspace,
        GuiTextEdit::Insert("z".into()),
    ] {
        let before = state.text.clone();
        let composition_only = matches!(
            edit,
            GuiTextEdit::Compose(_) | GuiTextEdit::CancelComposition
        );
        let view = host.resolve_view(query).unwrap();
        router
            .route(
                &mut host,
                &mut context,
                view,
                GuiPhysicalInput::Text {
                    fence: state.fence,
                    edit,
                },
                &mut delivery,
            )
            .unwrap();
        host.frame(0.0).unwrap();
        state = router.with_native_text(&mut host, &context, |state| state.unwrap().clone());
        if composition_only {
            assert!(Arc::ptr_eq(&state.text, &before));
        }
    }
    assert_eq!(&*state.text, "z");
    let view = host.resolve_view(query).unwrap();
    router
        .route(
            &mut host,
            &mut context,
            view,
            GuiPhysicalInput::Text {
                fence: original,
                edit: GuiTextEdit::Insert("stale".into()),
            },
            &mut delivery,
        )
        .unwrap();
    host.frame(0.0).unwrap();
    assert_eq!(
        router.with_native_text(&mut host, &context, |state| state.unwrap().clone()),
        state
    );
    ledger.borrow_mut().fail_effect = true;
    let view = host.resolve_view(query).unwrap();
    router
        .route(
            &mut host,
            &mut context,
            view,
            GuiPhysicalInput::Text {
                fence: state.fence,
                edit: GuiTextEdit::Insert("capacity".into()),
            },
            &mut delivery,
        )
        .unwrap();
    host.frame(0.0).unwrap();
    assert_eq!(
        router.with_native_text(&mut host, &context, |state| state.unwrap().clone()),
        state
    );
    assert_eq!(
        crate::systems::gui::test_support::read_control(
            &host.world_mut(world.id()).unwrap(),
            entity
        )
        .unwrap()
        .value,
        crate::systems::gui::test_support::GuiTestValue::Text(state.text.clone())
    );
    assert!(terminals(&ledger).iter().any(|terminal| matches!(
        terminal,
        GuiDeliveryTerminal::Rejected(GuiInputError::Delivery(GuiDeliveryError::Capacity))
    )));
    ledger.borrow_mut().fail_effect = false;

    // A client write of equal text is still a new value: the native record
    // refreshes onto it with a new generation.
    host.world_mut(world.id())
        .unwrap()
        .enqueue(crate::Batch {
            id: 500,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_TEXT_INPUT,
                field: crate::FieldWrite {
                    offset: std::mem::offset_of!(crate::components::GuiTextInput, text) as u32,
                    value: crate::FieldValue::String(String::from("z").into()),
                },
            }],
        })
        .unwrap();
    host.frame(0.0).unwrap();
    let replaced = router.with_native_text(&mut host, &context, |state| state.unwrap().clone());
    assert_eq!(replaced.fence.target, state.fence.target);
    assert_eq!(replaced.text, state.text);
    assert!(!Arc::ptr_eq(&replaced.text, &state.text));
    assert!(replaced.fence.generation > state.fence.generation);
    let view = host.resolve_view(query).unwrap();
    router
        .route(
            &mut host,
            &mut context,
            view,
            GuiPhysicalInput::Key {
                key: GuiPhysicalKey::Escape,
                shift: false,
            },
            &mut delivery,
        )
        .unwrap();
    host.frame(0.0).unwrap();
    assert!(router.with_native_text(&mut host, &context, |state| state.is_none()));
    router.release(&mut host, context);
}

#[test]
fn rejected_stale_native_edit_keeps_focus_for_the_next_tab() {
    use crate::systems::gui::local::GuiTextEdit;
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas(&mut host, world);
    let text = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiTextInput(crate::components::GuiTextInput {
                text: "ab".into(),
                ..Default::default()
            }),
            ComponentValue::GuiLayout(GuiLayout {
                width: 100.0,
                height: 30.0,
                ..Default::default()
            }),
        ],
        Some(root_entity),
    );
    let next = button(&mut host, world, Some(root_entity));
    let viewport = WorldViewport {
        width: 128,
        height: 128,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(root, viewport).unwrap();
    host.frame(0.0).unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: root,
        expected_viewport: viewport,
    };
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router.bind(&host, world, 900, Vec::new()).unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = DeliveryFactory(ledger.clone());
    let mut route = |host: &mut HostRuntime, context: &mut GuiRoutingContext, input| {
        let view = host.resolve_view(query).unwrap();
        router
            .route(host, context, view, input, &mut delivery)
            .unwrap();
        host.frame(0.0).unwrap();
    };
    let tab = || GuiPhysicalInput::Key {
        key: GuiPhysicalKey::Tab,
        shift: false,
    };
    route(&mut host, &mut context, tab());
    let original = GuiInputRouter::default()
        .with_native_text(&mut host, &context, |state| state.unwrap().fence);
    route(
        &mut host,
        &mut context,
        GuiPhysicalInput::Text {
            fence: original,
            edit: GuiTextEdit::Insert("x".into()),
        },
    );
    // A second edit stamped with the consumed fence is stale and rejected.
    route(
        &mut host,
        &mut context,
        GuiPhysicalInput::Text {
            fence: original,
            edit: GuiTextEdit::Insert("y".into()),
        },
    );
    assert!(matches!(
        terminals(&ledger).last(),
        Some(GuiDeliveryTerminal::Rejected(_))
    ));
    let focused = |host: &mut HostRuntime, entity| {
        crate::systems::gui::test_support::read_control(
            &host.world_mut(world.id()).unwrap(),
            entity,
        )
        .unwrap()
        .focused
    };
    assert!(focused(&mut host, text));

    // The rejection neither drops the text focus nor fails the next Tab,
    // which moves on to the following control.
    let before = terminals(&ledger).len();
    route(&mut host, &mut context, tab());
    assert!(
        terminals(&ledger)[before..]
            .iter()
            .all(|terminal| matches!(terminal, GuiDeliveryTerminal::Applied(_))),
        "{:?}",
        &terminals(&ledger)[before..]
    );
    assert!(!focused(&mut host, text));
    assert!(focused(&mut host, next));
    router.release(&mut host, context);
}

#[test]
fn native_edit_for_an_earlier_focus_is_rejected_without_moving_focus() {
    use crate::systems::gui::local::GuiTextEdit;
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas(&mut host, world);
    let text = |host: &mut HostRuntime| {
        create(
            host,
            world,
            vec![
                ComponentValue::GuiTextInput(Default::default()),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 100.0,
                    height: 30.0,
                    ..Default::default()
                }),
            ],
            Some(root_entity),
        )
    };
    let first = text(&mut host);
    let second = text(&mut host);
    let viewport = WorldViewport {
        width: 128,
        height: 128,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(root, viewport).unwrap();
    host.frame(0.0).unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: root,
        expected_viewport: viewport,
    };
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router.bind(&host, world, 900, Vec::new()).unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = DeliveryFactory(ledger.clone());
    let tab = GuiPhysicalInput::Key {
        key: GuiPhysicalKey::Tab,
        shift: false,
    };
    let view = host.resolve_view(query).unwrap();
    router
        .route(&mut host, &mut context, view, tab.clone(), &mut delivery)
        .unwrap();
    host.frame(0.0).unwrap();
    let old = router.with_native_text(&mut host, &context, |state| state.unwrap().fence);
    assert_eq!(old.target.entity, first);
    let view = host.resolve_view(query).unwrap();
    router
        .route(&mut host, &mut context, view, tab, &mut delivery)
        .unwrap();
    host.frame(0.0).unwrap();

    // A delayed edit stamped for the first input arrives after focus moved on.
    let view = host.resolve_view(query).unwrap();
    assert_eq!(
        router.route(
            &mut host,
            &mut context,
            view,
            GuiPhysicalInput::Text {
                fence: old,
                edit: GuiTextEdit::Insert("late".into()),
            },
            &mut delivery,
        ),
        Err(GuiInputError::Unavailable)
    );
    host.frame(0.0).unwrap();
    let native = router.with_native_text(&mut host, &context, |state| state.cloned());
    assert_eq!(native.map(|state| state.fence.target.entity), Some(second));
    for (entity, focused) in [(first, false), (second, true)] {
        let snapshot = crate::systems::gui::test_support::read_control(
            &host.world_mut(world.id()).unwrap(),
            entity,
        )
        .unwrap();
        assert_eq!(snapshot.focused, focused);
        assert_eq!(
            snapshot.value,
            crate::systems::gui::test_support::GuiTestValue::Text("".into())
        );
    }
    router.release(&mut host, context);
}

#[test]
fn leaving_and_reentering_a_pressed_button_cannot_revive_a_cancelled_tap() {
    let mut scene = scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: scene.root,
        expected_viewport: WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        },
    };
    let mut delivery = DeliveryFactory(scene.ledger.clone());
    for input in [
        GuiPhysicalInput::PointerDown {
            button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
            pointer: 1,
            point: [0.05, 0.05],
        },
        GuiPhysicalInput::PointerMove {
            pointer: 1,
            point: [1.5, 1.5],
        },
        GuiPhysicalInput::PointerMove {
            pointer: 1,
            point: [0.05, 0.05],
        },
        GuiPhysicalInput::PointerUp {
            button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
            pointer: 1,
            point: [0.05, 0.05],
        },
    ] {
        let view = scene.host.resolve_view(query).unwrap();
        router
            .route(&mut scene.host, &mut context, view, input, &mut delivery)
            .unwrap();
        scene.host.frame(0.0).unwrap();
    }
    assert!(!terminals(&scene.ledger).iter().any(|terminal| matches!(
        terminal,
        GuiDeliveryTerminal::Applied(GuiLocalEffect {
            kind: GuiLocalEffectKind::Pressed,
            ..
        })
    )));
    router.release(&mut scene.host, context);
}

struct RefuseDelivery;

impl GuiRoutingDelivery for RefuseDelivery {
    fn command(&mut self) -> Result<Box<dyn GuiDeliveryPermit>, GuiInputError> {
        Err(GuiInputError::Capacity)
    }
}

impl GuiRoutingDelivery for DeliveryFactory {
    fn command(&mut self) -> Result<Box<dyn GuiDeliveryPermit>, GuiInputError> {
        Ok(Permit::boxed(&self.0))
    }
}

#[test]
fn failed_pointer_release_revokes_the_active_lease_without_another_frame() {
    let mut scene = scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: scene.root,
        expected_viewport: WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        },
    };
    let view = scene.host.resolve_view(query).unwrap();
    router
        .route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 3,
                point: [0.05, 0.05],
            },
            &mut DeliveryFactory(scene.ledger.clone()),
        )
        .unwrap();
    scene.host.frame(0.0).unwrap();
    let view = scene.host.resolve_view(query).unwrap();
    assert_eq!(
        router.route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerUp {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 3,
                point: [0.05, 0.05]
            },
            &mut RefuseDelivery
        ),
        Err(GuiInputError::Capacity)
    );
    let cancelled = router.synchronize(&mut scene.host, &mut context, Some(view));
    assert_eq!(cancelled.pointers(), &[3]);
    let snapshot = crate::systems::gui::test_support::read_control(
        &scene.host.world_mut(scene.child.world().id()).unwrap(),
        scene.target.entity,
    )
    .unwrap();
    assert!(!snapshot.interaction.pressed && !snapshot.interaction.captured);
    assert_eq!(
        scene.host.resolve_view(query).unwrap().publication,
        view.publication
    );
    router.release(&mut scene.host, context);
}

#[test]
fn pointer_focus_omits_ring_and_keyboard_focus_reveals_it_without_value_change() {
    use crate::systems::canvas::{CanvasPaintEntry, CanvasPart, CanvasPublication};
    let (mut host, _) = host();
    let world = world(&mut host);
    let entity = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiLayout(crate::components::GuiLayout {
                width: 100.0,
                height: 50.0,
                ..Default::default()
            }),
            ComponentValue::GuiButton(Default::default()),
        ],
        None,
    );
    let output = host.canvas_output(world, [100.0, 50.0], 1.0);
    let viewport = WorldViewport {
        width: 100,
        height: 50,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(output, viewport).unwrap();
    host.frame(0.0).unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output,
        expected_viewport: viewport,
    };
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router.bind(&host, world, 900, Vec::new()).unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = DeliveryFactory(ledger.clone());
    let initial = crate::systems::gui::test_support::read_control(
        &host.world_mut(world.id()).unwrap(),
        entity,
    )
    .unwrap()
    .value;
    for (input, expected) in [
        (
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.5, 0.5],
            },
            false,
        ),
        (
            GuiPhysicalInput::Key {
                key: GuiPhysicalKey::Tab,
                shift: false,
            },
            true,
        ),
        (
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.5, 0.5],
            },
            false,
        ),
    ] {
        let view = host.resolve_view(query).unwrap();
        router
            .route(&mut host, &mut context, view, input, &mut delivery)
            .unwrap();
        host.frame(0.0).unwrap();
        let view = host.resolve_view(query).unwrap();
        let canvas = host
            .output(view.publication, output)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap();
        let painted = canvas.entries.iter().any(|entry| matches!(entry.as_ref(), CanvasPaintEntry::Primitive { primitive, .. } if primitive.style().identity.part == CanvasPart::FocusRing));
        assert_eq!(painted, expected);
        let snapshot = crate::systems::gui::test_support::read_control(
            &host.world_mut(world.id()).unwrap(),
            entity,
        )
        .unwrap();
        assert!(snapshot.focused);
        assert_eq!(snapshot.value, initial);
    }
    assert!(
        terminals(&ledger)
            .iter()
            .all(|terminal| matches!(terminal, GuiDeliveryTerminal::Applied(_)))
    );
    router.release(&mut host, context);
}

#[test]
fn unavailable_view_cancels_idle_capture_once_without_advancing_worlds() {
    let mut scene = scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: scene.root,
        expected_viewport: WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        },
    };
    let view = scene.host.resolve_view(query).unwrap();
    router
        .route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 3,
                point: [0.05, 0.05],
            },
            &mut DeliveryFactory(scene.ledger.clone()),
        )
        .unwrap();
    scene.host.frame(0.0).unwrap();
    let published = scene.host.resolve_view(query).unwrap().publication;
    let cancelled = router.synchronize(&mut scene.host, &mut context, None);
    assert_eq!(cancelled.pointers(), &[3]);
    assert!(
        router
            .synchronize(&mut scene.host, &mut context, None)
            .is_empty()
    );
    assert_eq!(
        scene.host.resolve_view(query).unwrap().publication,
        published
    );
    let snapshot = crate::systems::gui::test_support::read_control(
        &scene.host.world_mut(scene.child.world().id()).unwrap(),
        scene.target.entity,
    )
    .unwrap();
    assert!(!snapshot.interaction.pressed && !snapshot.interaction.captured);
    router.release(&mut scene.host, context);
}

#[test]
fn releasing_context_discards_its_queued_commands_without_another_frame() {
    let mut scene = scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let view = scene
        .host
        .resolve_view(crate::ViewQueryTarget::RootView {
            output: scene.root,
            expected_viewport: WorldViewport {
                width: 128,
                height: 128,
                device_pixel_ratio: 1.0,
            },
        })
        .unwrap();
    router
        .route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.05, 0.05],
            },
            &mut DeliveryFactory(scene.ledger.clone()),
        )
        .unwrap();
    assert_eq!(scene.ledger.borrow().reserved, 4);
    assert!(terminals(&scene.ledger).is_empty());
    router.release(&mut scene.host, context);
    assert_eq!(scene.ledger.borrow().reserved, 0);
    let results = terminals(&scene.ledger);
    assert_eq!(results.len(), 4);
    assert!(
        results
            .iter()
            .all(|result| matches!(result, GuiDeliveryTerminal::Cancelled))
    );
    scene.host.frame(0.0).unwrap();
    assert_eq!(terminals(&scene.ledger).len(), 4);
    assert!(
        !crate::systems::gui::test_support::read_control(
            &scene.host.world_mut(scene.child.world().id()).unwrap(),
            scene.target.entity
        )
        .unwrap()
        .focused
    );
}

#[test]
fn releasing_pointer_outside_view_cancels_without_activation() {
    let mut scene = scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let view = scene
        .host
        .resolve_view(crate::ViewQueryTarget::RootView {
            output: scene.root,
            expected_viewport: WorldViewport {
                width: 128,
                height: 128,
                device_pixel_ratio: 1.0,
            },
        })
        .unwrap();
    let mut delivery = DeliveryFactory(scene.ledger.clone());
    router
        .route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.05, 0.05],
            },
            &mut delivery,
        )
        .unwrap();
    router
        .route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerUp {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [1.1, 0.05],
            },
            &mut delivery,
        )
        .unwrap();
    scene.host.frame(0.0).unwrap();
    let results = terminals(&scene.ledger);
    assert_eq!(results.len(), 6);
    assert!(results.iter().all(|result| matches!(result, GuiDeliveryTerminal::Applied(effect) if !matches!(effect.kind, GuiLocalEffectKind::Pressed))));
    router.release(&mut scene.host, context);
}

#[test]
fn ordinary_pointer_routes_to_child_and_commits_only_at_world_boundary() {
    let mut scene = scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let view = scene
        .host
        .resolve_view(crate::ViewQueryTarget::RootView {
            output: scene.root,
            expected_viewport: WorldViewport {
                width: 128,
                height: 128,
                device_pixel_ratio: 1.0,
            },
        })
        .unwrap();
    let mut delivery = DeliveryFactory(scene.ledger.clone());
    let point = [0.05, 0.05];
    let outcome = router
        .route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point,
            },
            &mut delivery,
        )
        .unwrap();
    assert_eq!(
        outcome,
        GuiRoutingDisposition::Routed {
            target: scene.target
        }
    );
    router
        .route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerUp {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point,
            },
            &mut delivery,
        )
        .unwrap();
    assert!(terminals(&scene.ledger).is_empty());
    scene.host.frame(0.0).unwrap();
    let results = terminals(&scene.ledger);
    assert_eq!(results.len(), 7);
    assert_eq!(
        results
            .iter()
            .filter(|terminal| matches!(
                terminal,
                GuiDeliveryTerminal::Applied(GuiLocalEffect {
                    kind: GuiLocalEffectKind::Pressed,
                    ..
                })
            ))
            .count(),
        1
    );
    assert!(
        results
            .iter()
            .all(|terminal| matches!(terminal, GuiDeliveryTerminal::Applied(_)))
    );
    let snapshot = crate::systems::gui::test_support::read_control(
        &scene.host.world_mut(scene.child.world().id()).unwrap(),
        scene.target.entity,
    )
    .unwrap();
    assert!(
        !snapshot.interaction.pressed
            && !snapshot.interaction.captured
            && !snapshot.interaction.hovered
    );
    router.release(&mut scene.host, context);
}

#[test]
fn equal_root_rebind_fences_physical_context_without_changing_authoring() {
    let mut scene = scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let viewport = WorldViewport {
        width: 128,
        height: 128,
        device_pixel_ratio: 1.0,
    };
    let view = scene
        .host
        .resolve_view(crate::ViewQueryTarget::RootView {
            output: scene.root,
            expected_viewport: viewport,
        })
        .unwrap();
    scene.host.set_root_output(scene.root, viewport).unwrap();
    assert_eq!(
        router.route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.05, 0.05]
            },
            &mut DeliveryFactory(scene.ledger.clone())
        ),
        Err(GuiInputError::StaleContext)
    );
    assert!(terminals(&scene.ledger).is_empty());
    assert!(
        crate::systems::gui::test_support::read_control(
            &scene.host.world_mut(scene.child.world().id()).unwrap(),
            scene.target.entity
        )
        .is_some()
    );
    router.release(&mut scene.host, context);
}

#[test]
fn routed_activation_survives_completed_publication_refresh_without_retargeting() {
    let mut scene = scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: scene.root,
        expected_viewport: WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        },
    };
    let first = scene.host.resolve_view(query).unwrap();
    router
        .route(
            &mut scene.host,
            &mut context,
            first,
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 3,
                point: [0.05, 0.05],
            },
            &mut DeliveryFactory(scene.ledger.clone()),
        )
        .unwrap();
    scene.host.frame(0.0).unwrap();
    let second = scene.host.resolve_view(query).unwrap();
    router
        .route(
            &mut scene.host,
            &mut context,
            second,
            GuiPhysicalInput::PointerUp {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 3,
                point: [0.05, 0.05],
            },
            &mut DeliveryFactory(scene.ledger.clone()),
        )
        .unwrap();
    scene.host.frame(0.0).unwrap();
    let results = terminals(&scene.ledger);
    assert_eq!(results.len(), 7);
    assert!(
        results
            .iter()
            .all(|terminal| matches!(terminal, GuiDeliveryTerminal::Applied(_))),
        "{results:?}"
    );
    assert!(matches!(
        results[5],
        GuiDeliveryTerminal::Applied(GuiLocalEffect {
            kind: GuiLocalEffectKind::Pressed,
            ..
        })
    ));
    router.release(&mut scene.host, context);
}

#[test]
fn wheel_burst_consumes_current_offsets_at_each_ordered_mutation() {
    use crate::components::{GuiLayout, GuiVirtualList};
    use crate::systems::gui::test_support::GuiTestValue;
    let (mut host, _) = host();
    let world = world(&mut host);
    let entity = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                width: 100.0,
                height: 50.0,
                ..Default::default()
            }),
            ComponentValue::GuiVirtualList(GuiVirtualList {
                item_count: 10,
                item_extent: 10.0,
                axis: 1,
                overscan: 0,
                ..Default::default()
            }),
        ],
        None,
    );
    let output = host.canvas_output(world, [100.0, 50.0], 1.0);
    let viewport = WorldViewport {
        width: 100,
        height: 50,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(output, viewport).unwrap();
    host.frame(0.0).unwrap();
    let view = host
        .resolve_view(crate::ViewQueryTarget::RootView {
            output,
            expected_viewport: viewport,
        })
        .unwrap();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router.bind(&host, world, 900, Vec::new()).unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut receipts = Vec::new();
    for _ in 0..3 {
        router
            .route(
                &mut host,
                &mut context,
                view,
                GuiPhysicalInput::Wheel {
                    point: [0.5, 0.5],
                    delta: [0.0, 20.0],
                    shift: false,
                },
                &mut DeliveryFactory(ledger.clone()),
            )
            .unwrap();
        receipts.push(router.scroll_remainder(&context).unwrap());
    }
    host.frame(0.0).unwrap();
    assert_eq!(
        receipts
            .iter()
            .map(|receipt| receipt.remaining().unwrap())
            .collect::<Vec<_>>(),
        [[0.0, 0.0], [0.0, 0.0], [0.0, 10.0]]
    );
    let snapshot = crate::systems::gui::test_support::read_control(
        &host.world_mut(world.id()).unwrap(),
        entity,
    )
    .unwrap();
    assert!(matches!(snapshot.value, GuiTestValue::Scroll(offset) if offset == [0.0, 50.0]));
    router.release(&mut host, context);
}

#[test]
fn focused_pending_and_applied_receipts_continue_across_publications() {
    let mut scene = nested_scroll_scene(5.0);
    let entity = create(
        &mut scene.host,
        scene.root.world(),
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                width: 20.0,
                height: 20.0,
                margin_top: 40.0,
                ..Default::default()
            }),
            ComponentValue::GuiSlider(crate::components::GuiSlider {
                min: 0.0,
                max: 10.0,
                step: 1.0,
                value: 0.0,
                ..Default::default()
            }),
        ],
        Some(scene.anchor),
    );
    scene.host.frame(0.0).unwrap();
    let query = crate::ViewQueryTarget::RootView {
        output: scene.root,
        expected_viewport: WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        },
    };
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.root.world(), 900, Vec::new())
        .unwrap();
    let view = scene.host.resolve_view(query).unwrap();
    router
        .route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::Wheel {
                point: [0.5, 0.5],
                delta: [0.0, 20.0],
                shift: false,
            },
            &mut DeliveryFactory(scene.ledger.clone()),
        )
        .unwrap();
    let focus = Rc::new(RefCell::new(Delivery::default()));
    assert!(matches!(
        router.route(
            &mut scene.host,
            &mut context,
            view,
            GuiPhysicalInput::Key { key: GuiPhysicalKey::Tab, shift: false },
            &mut DeliveryFactory(focus.clone()),
        ).unwrap(),
        GuiRoutingDisposition::Routed { target } if target.entity == entity
    ));
    scene.host.frame(0.0).unwrap();
    assert_eq!(focus.borrow().reserved, 1);
    assert!(terminals(&focus).is_empty());
    let mut previous_source = view.publication;
    for value in [1.0, 2.0] {
        let view = scene.host.resolve_view(query).unwrap();
        assert_ne!(view.publication, previous_source);
        let ledger = Rc::new(RefCell::new(Delivery::default()));
        router
            .route(
                &mut scene.host,
                &mut context,
                view,
                GuiPhysicalInput::Key {
                    key: GuiPhysicalKey::Right,
                    shift: false,
                },
                &mut DeliveryFactory(ledger.clone()),
            )
            .unwrap();
        scene.host.frame(0.0).unwrap();
        assert_eq!(
            crate::systems::gui::test_support::read_control(
                &scene.host.world_mut(scene.root.world().id()).unwrap(),
                entity
            )
            .unwrap()
            .value,
            crate::systems::gui::test_support::GuiTestValue::Scalar(value)
        );
        assert_eq!(terminals(&ledger).len(), 2);
        for terminal in terminals(&ledger) {
            let source = match terminal {
                GuiDeliveryTerminal::Applied(effect) => effect.source,
                GuiDeliveryTerminal::Written {
                    source,
                    ..
                } => source,
                terminal => panic!("unexpected terminal {terminal:?}"),
            };
            assert_eq!(
                source,
                GuiLocalEffectSource::Routed {
                    publication: view.publication
                }
            );
        }
        assert_eq!(ledger.borrow().reserved, 0);
        previous_source = view.publication;
    }
    assert_eq!(focus.borrow().reserved, 0);
    assert!(
        matches!(terminals(&focus).as_slice(), [GuiDeliveryTerminal::Applied(effect)]
        if effect.source == GuiLocalEffectSource::Routed { publication: view.publication })
    );
    router.release(&mut scene.host, context);
    assert_eq!(scene.ledger.borrow().reserved, 0);
}

#[test]
fn slider_key_burst_uses_each_applied_predecessor_without_predicted_values() {
    use crate::components::{GuiLayout, GuiSlider};
    let (mut host, _) = host();
    let world = world(&mut host);
    let entity = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                width: 100.0,
                height: 50.0,
                ..Default::default()
            }),
            ComponentValue::GuiSlider(GuiSlider {
                min: 0.0,
                max: 10.0,
                step: 1.0,
                value: 0.0,
                ..Default::default()
            }),
        ],
        None,
    );
    let output = host.canvas_output(world, [100.0, 50.0], 1.0);
    let viewport = WorldViewport {
        width: 100,
        height: 50,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(output, viewport).unwrap();
    host.frame(0.0).unwrap();
    let view = host
        .resolve_view(crate::ViewQueryTarget::RootView {
            output,
            expected_viewport: viewport,
        })
        .unwrap();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router.bind(&host, world, 900, Vec::new()).unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = DeliveryFactory(ledger.clone());
    for key in [
        GuiPhysicalKey::Tab,
        GuiPhysicalKey::Right,
        GuiPhysicalKey::Right,
        GuiPhysicalKey::Right,
    ] {
        router
            .route(
                &mut host,
                &mut context,
                view,
                GuiPhysicalInput::Key {
                    key,
                    shift: false,
                },
                &mut delivery,
            )
            .unwrap();
    }
    host.frame(0.0).unwrap();
    let snapshot = crate::systems::gui::test_support::read_control(
        &host.world_mut(world.id()).unwrap(),
        entity,
    )
    .unwrap();
    assert_eq!(
        snapshot.value,
        crate::systems::gui::test_support::GuiTestValue::Scalar(3.0)
    );
    assert!(terminals(&ledger).iter().all(|terminal| matches!(
        terminal,
        GuiDeliveryTerminal::Applied(_) | GuiDeliveryTerminal::Written { .. }
    )));
    router.release(&mut host, context);
}

#[test]
fn ordinary_scroll_thumb_uses_published_geometry_and_preserves_grab_offset() {
    use crate::components::{GuiLayout, GuiVirtualList};
    let (mut host, _) = host();
    let world = world(&mut host);
    let entity = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                width: 100.0,
                height: 50.0,
                ..Default::default()
            }),
            ComponentValue::GuiVirtualList(GuiVirtualList {
                item_count: 10,
                item_extent: 10.0,
                axis: 1,
                overscan: 0,
                bar_thickness: 2.5,
                bar_inset: 0.0,
                bar_end_inset: 0.0,
                ..Default::default()
            }),
        ],
        None,
    );
    let output = host.canvas_output(world, [100.0, 50.0], 1.0);
    let viewport = WorldViewport {
        width: 100,
        height: 50,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(output, viewport).unwrap();
    host.frame(0.0).unwrap();
    let view = host
        .resolve_view(crate::ViewQueryTarget::RootView {
            output,
            expected_viewport: viewport,
        })
        .unwrap();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router.bind(&host, world, 900, Vec::new()).unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = DeliveryFactory(ledger.clone());
    router
        .route(
            &mut host,
            &mut context,
            view,
            GuiPhysicalInput::PointerDown {
                button: crate::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.98, 0.1],
            },
            &mut delivery,
        )
        .unwrap();
    router
        .route(
            &mut host,
            &mut context,
            view,
            GuiPhysicalInput::PointerMove {
                pointer: 1,
                point: [0.98, 0.4],
            },
            &mut delivery,
        )
        .unwrap();
    host.frame(0.0).unwrap();
    let snapshot = crate::systems::gui::test_support::read_control(
        &host.world_mut(world.id()).unwrap(),
        entity,
    )
    .unwrap();
    let crate::systems::gui::test_support::GuiTestValue::Scroll(offset) = snapshot.value else {
        panic!("scroll value");
    };
    // The flush 2.5-unit bar leaves 47.5 units between its pointed ends; the
    // thumb shows half of them, so 15 units of drag move it 15 of its 23.75
    // units of travel over the capacity of 50.
    assert!(
        (offset[1] - 15.0 / 23.75 * 50.0).abs() < 0.001,
        "{offset:?}"
    );
    assert!(terminals(&ledger).iter().all(|terminal| matches!(
        terminal,
        GuiDeliveryTerminal::Applied(_) | GuiDeliveryTerminal::Written { .. }
    )));
    router.release(&mut host, context);
}

#[test]
fn tab_enters_published_tree_order_then_wraps_inside_exact_focus_scope() {
    use crate::components::{GuiBehavior, GuiButton, GuiLayout};
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas(&mut host, world);
    let scope = create(
        &mut host,
        world,
        vec![ComponentValue::GuiBehavior(GuiBehavior {
            focus_scope: true,
            ..Default::default()
        })],
        Some(root_entity),
    );
    let mut targets = Vec::new();
    for parent in [scope, scope, root_entity] {
        targets.push(create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 20.0,
                    height: 20.0,
                    ..Default::default()
                }),
            ],
            Some(parent),
        ));
    }
    let viewport = WorldViewport {
        width: 128,
        height: 128,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(root, viewport).unwrap();
    host.frame(0.0).unwrap();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router.bind(&host, world, 900, Vec::new()).unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    for expected in [targets[0], targets[1], targets[0]] {
        let view = host
            .resolve_view(crate::ViewQueryTarget::RootView {
                output: root,
                expected_viewport: viewport,
            })
            .unwrap();
        assert!(
            matches!(router.route(&mut host, &mut context, view, GuiPhysicalInput::Key { key: GuiPhysicalKey::Tab, shift: false }, &mut DeliveryFactory(ledger.clone())).unwrap(), GuiRoutingDisposition::Routed { target } if target.entity == expected)
        );
        host.frame(0.0).unwrap();
    }
    assert!(
        terminals(&ledger)
            .iter()
            .all(|terminal| matches!(terminal, GuiDeliveryTerminal::Applied(_)))
    );
    router.release(&mut host, context);
}
