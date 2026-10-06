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
