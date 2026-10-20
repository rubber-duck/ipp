use super::*;
use ipp_core::services::reliable_output::{OutputLimits, ReliableOutputAccount};
use ipp_core::systems::gui::local::{GuiEntityTarget, GuiNativeTextState, GuiTextFence};

#[test]
fn native_output_reserves_simultaneous_payload_and_framing_before_copy() {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            Default::default(),
            &[
                ipp_core::systems::canvas::CanvasSystem::ID,
                ipp_core::systems::gui::GuiSystem::ID,
            ],
        )
        .unwrap();
    let state = GuiNativeTextState {
        fence: GuiTextFence {
            target: GuiEntityTarget {
                world: host.world_ref(world).unwrap(),
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::GUI_TEXT_INPUT,
                incarnation: 1,
            },
            generation: 1,
        },
        text: "x".repeat(32_768).into(),
        selection: [32_768; 2],
        composition: None,
        masked: false,
        client_input: false,
    };
    let size = ipp_protocol::host::gui_input::native_state_size(&state).unwrap();
    for (available, accepted) in [(512 + size, false), (512 + 2 * size, true)] {
        let budget = SharedReplyBudget(ReliableOutputAccount::new(OutputLimits {
            bytes: available + crate::reliable_output::RESPONSE_METADATA_BYTES,
            reply_reserve: 0,
        }));
        let outbox = SessionOutbox::default();
        let reservation = Rc::new(RefCell::new(
            ReplyReservation::new(
                budget.clone(),
                ipp_core::services::reliable_output::OutputClass::Ordinary,
                512,
            )
            .unwrap(),
        ));
        let reply = Rc::new(RefCell::new(Reply {
            native: None,
            connection: 1,
            request: 1,
            slot: Some(outbox.reserve().unwrap()),
            reservation,
            building: false,
            pending: 1,
            disposition: 0,
            applied: 0,
            rejected: 0,
            cancelled: 0,
            error: None,
            scroll: None,
            hit: None,
        }));
        let mut permit = Box::new(Child {
            native: None,
            reply: reply.clone(),
            settled: false,
        });
        let before = budget.0.usage();
        if accepted {
            permit.prepare_native(&state).unwrap();
            assert_eq!(budget.0.usage().bytes - before.bytes, 2 * size);
            assert_eq!(permit.native.as_ref().unwrap().capacity(), size);
            permit.settle(GuiDeliveryTerminal::NativeApplied(state.clone()));
            let response = outbox.pop_front().unwrap();
            let decoded = ipp_protocol::host::decode_host_response(&response.bytes, 1).unwrap();
            assert!(matches!(
                decoded.body,
                HostResponseBody::GuiInput(GuiPhysicalResponse::Routed {
                    applied: 1,
                    native: Some(_),
                    ..
                })
            ));
            assert!(budget.0.usage().bytes >= response.bytes.capacity());
            drop(response);
        } else {
            assert_eq!(
                permit.prepare_native(&state),
                Err(GuiDeliveryError::Capacity)
            );
            assert!(permit.native.is_none());
            assert_eq!(budget.0.usage(), before);
            permit.settle(GuiDeliveryTerminal::Rejected(GuiInputError::Delivery(
                GuiDeliveryError::Capacity,
            )));
            let response = outbox.pop_front().unwrap();
            let decoded = ipp_protocol::host::decode_host_response(&response.bytes, 1).unwrap();
            assert!(matches!(
                decoded.body,
                HostResponseBody::GuiInput(GuiPhysicalResponse::Routed {
                    applied: 0,
                    rejected: 1,
                    ..
                })
            ));
            drop(response);
        }
        drop(reply);
        assert_eq!(budget.0.usage().bytes, 0);
        assert_eq!(outbox.len(), 0);
    }
}

fn wheel_hit_fixture() -> (GuiEntityTarget, ipp_core::WorldAttachmentToken) {
    let mut host = HostRuntime::new();
    let parent = host
        .create_world(
            Default::default(),
            &[ipp_core::systems::world_attachment::WorldAttachmentSystem::ID],
        )
        .unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let child = host.world_ref(child).unwrap();
    host.world_mut(parent)
        .unwrap()
        .enqueue(ipp_core::Batch {
            id: 1,
            operations: vec![
                ipp_core::Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                ipp_core::Command::insert_value(
                    ipp_core::EntityRef::Alias(1),
                    ipp_core::ComponentValue::WorldAttachment(ipp_core::WorldAttachment::spatial(
                        child,
                    )),
                ),
            ],
        })
        .unwrap();
    let mut frame = host.frame(0.0).unwrap();
    let outcome = frame
        .worlds
        .remove(&parent)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0);
    let token = outcome
        .effects
        .iter()
        .find_map(|effect| match &effect.effect {
            ipp_core::OperationEffect::WorldAttachment(
                ipp_core::WorldAttachmentEffect::Written(token),
            ) => Some(token.clone()),
            _ => None,
        })
        .unwrap();
    (
        GuiEntityTarget {
            world: child,
            entity: ipp_core::EntityId::from_bits(99),
            component: ipp_core::ComponentValue::GUI_BUTTON,
            incarnation: 7,
        },
        token,
    )
}

fn wheel_reply(available: usize) -> (Rc<RefCell<Reply>>, SessionOutbox, SharedReplyBudget) {
    let budget = SharedReplyBudget(ReliableOutputAccount::new(OutputLimits {
        bytes: available + crate::reliable_output::RESPONSE_METADATA_BYTES,
        reply_reserve: 0,
    }));
    let outbox = SessionOutbox::default();
    let reservation = Rc::new(RefCell::new(
        ReplyReservation::new(budget.clone(), OutputClass::Ordinary, 512).unwrap(),
    ));
    let reply = Rc::new(RefCell::new(Reply {
        native: None,
        connection: 1,
        request: 2,
        slot: Some(outbox.reserve().unwrap()),
        reservation,
        building: true,
        pending: 0,
        disposition: 3,
        applied: 0,
        rejected: 0,
        cancelled: 0,
        error: None,
        scroll: None,
        hit: None,
    }));
    (reply, outbox, budget)
}

#[test]
fn wheel_hit_reserves_path_and_encoded_reply_before_copying() {
    let (target, token) = wheel_hit_fixture();
    let depth = ipp_protocol::world::view_queries::MAX_CAMERA_PATH_DEPTH;
    let path = vec![token; depth];
    let retained = std::mem::size_of::<Reply>()
        + std::mem::size_of::<Pending>()
        + 8 * std::mem::size_of::<usize>()
        + depth
            * std::mem::size_of::<ipp_protocol::world::view_queries::CameraAttachmentReference>();
    let bytes = 512 + ipp_protocol::host::gui_input::input_hit_size(depth).unwrap();
    for (available, accepted) in [(retained + bytes - 1, false), (retained + bytes, true)] {
        let (reply, outbox, budget) = wheel_reply(available);
        let result = Delivery(reply.clone()).wheel_hit(GuiRoutingHit {
            target,
            path: &path,
        });
        assert_eq!(result.is_ok(), accepted);
        if accepted {
            assert_eq!(reply.borrow().hit.as_ref().unwrap().path.capacity(), depth);
        } else {
            assert_eq!(result, Err(GuiInputError::Capacity));
            assert!(reply.borrow().hit.is_none());
        }
        reply.borrow_mut().building = false;
        finish(&reply);
        let response = outbox.pop_front().unwrap();
        let decoded = ipp_protocol::host::decode_host_response(&response.bytes, 1).unwrap();
        let HostResponseBody::GuiInput(GuiPhysicalResponse::Routed {
            hit,
            ..
        }) = decoded.body
        else {
            panic!("routing terminal");
        };
        assert_eq!(hit.is_some(), accepted);
        if let Some(hit) = hit {
            assert_eq!(hit.target.world, target.world.into());
            assert_eq!(hit.target.entity, target.entity);
            assert_eq!(hit.target.incarnation, target.incarnation);
            assert_eq!(hit.path.len(), depth);
            assert_eq!(hit.path[0].revision, path[0].identity().1);
        }
        drop(response);
        drop(reply);
        assert_eq!(budget.0.usage().bytes, 0);
    }
    let (reply, _, budget) = wheel_reply(retained + bytes);
    let overlong = vec![path[0].clone(); depth + 1];
    let before = budget.0.usage();
    assert_eq!(
        Delivery(reply.clone()).wheel_hit(GuiRoutingHit {
            target,
            path: &overlong
        }),
        Err(GuiInputError::Capacity)
    );
    assert!(reply.borrow().hit.is_none());
    assert_eq!(budget.0.usage(), before);
}

#[test]
fn rejected_or_cancelled_wheel_children_never_publish_a_usable_hit() {
    let (target, token) = wheel_hit_fixture();
    for rejected in [false, true] {
        let (reply, outbox, budget) = wheel_reply(16_384);
        let mut delivery = Delivery(reply.clone());
        delivery
            .wheel_hit(GuiRoutingHit {
                target,
                path: std::slice::from_ref(&token),
            })
            .unwrap();
        let permit = delivery.command().unwrap();
        reply.borrow_mut().building = false;
        if rejected {
            permit.settle(GuiDeliveryTerminal::Rejected(GuiInputError::Unavailable));
        } else {
            drop(permit);
        }
        let response = outbox.pop_front().unwrap();
        let decoded = ipp_protocol::host::decode_host_response(&response.bytes, 1).unwrap();
        assert!(matches!(
            decoded.body,
            HostResponseBody::GuiInput(GuiPhysicalResponse::Routed {
                hit: None,
                ..
            })
        ));
        drop(response);
        drop(delivery);
        drop(reply);
        assert_eq!(budget.0.usage().bytes, 0);
    }
}

#[test]
fn wheel_hit_keeps_the_credit_for_already_admitted_children() {
    let (target, token) = wheel_hit_fixture();
    let (reply, outbox, budget) = wheel_reply(16_384);
    let mut delivery = Delivery(reply.clone());
    let first = delivery.command().unwrap();
    let second = delivery.command().unwrap();
    let before = budget.0.usage().bytes;
    delivery
        .wheel_hit(GuiRoutingHit {
            target,
            path: std::slice::from_ref(&token),
        })
        .unwrap();
    let extra = ipp_protocol::host::gui_input::input_hit_size(1).unwrap()
        + std::mem::size_of::<ipp_protocol::world::view_queries::CameraAttachmentReference>();
    assert_eq!(budget.0.usage().bytes, before + extra);
    reply.borrow_mut().building = false;
    drop(first);
    drop(second);
    let response = outbox.pop_front().unwrap();
    drop(response);
    drop(delivery);
    drop(reply);
    assert_eq!(budget.0.usage().bytes, 0);
}
