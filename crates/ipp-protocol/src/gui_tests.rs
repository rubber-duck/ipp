use super::*;
use crate::{RequestBody, decode_request};

fn envelope(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::from(7u64.to_le_bytes());
    bytes.extend(9u64.to_le_bytes());
    bytes.push(tag);
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    bytes
}

/// One final batch page carrying a single `GuiAction` command.
fn action(operation: u8, value: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::from(7u64.to_le_bytes());
    bytes.extend(9u64.to_le_bytes());
    bytes.push(crate::wire::REQUEST_SUBMIT_BATCH);
    bytes.extend(1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend(1u32.to_le_bytes());
    bytes.push(crate::wire::COMMAND_GUI_ACTION);
    bytes.push(crate::wire::REF_HANDLE);
    bytes.extend(3u64.to_le_bytes());
    bytes.extend(51u16.to_le_bytes());
    bytes.extend(4u64.to_le_bytes());
    bytes.push(operation);
    bytes.extend(value);
    bytes
}

#[test]
fn gui_action_command_syntax_is_complete_before_admission() {
    for (tag, payload, expected) in [
        (0, vec![], GuiLocalAction::Press),
        (1, vec![], GuiLocalAction::Toggle),
        (
            2,
            0.5f32.to_le_bytes().to_vec(),
            GuiLocalAction::SetScalar(0.5),
        ),
        (3, vec![0, 0, 0, 0], GuiLocalAction::SetText("".into())),
        (4, vec![], GuiLocalAction::Focus),
        (6, vec![], GuiLocalAction::Blur),
        (7, vec![], GuiLocalAction::Submit),
        (
            8,
            [12.0f32.to_le_bytes(), 34.0f32.to_le_bytes()].concat(),
            GuiLocalAction::ScrollTo([12.0, 34.0]),
        ),
        (
            9,
            [(-12.0f32).to_le_bytes(), 34.0f32.to_le_bytes()].concat(),
            GuiLocalAction::ScrollBy([-12.0, 34.0]),
        ),
        (
            10,
            [40u32.to_le_bytes(), 2.0f32.to_le_bytes()].concat(),
            GuiLocalAction::ScrollToIndex {
                index: 40,
                offset: 2.0,
            },
        ),
    ] {
        let bytes = action(tag, &payload);
        let request = decode_request(&bytes, 7).unwrap();
        let RequestBody::SubmitBatch(page) = request.body else {
            panic!("batch page")
        };
        assert_eq!(
            page.operations,
            [ipp_core::Command::GuiAction {
                target: ipp_core::GuiActionTarget {
                    entity: ipp_core::EntityRef::Handle(EntityId::from_bits(3)),
                    component: 51,
                    incarnation: 4,
                },
                action: expected,
            }]
        );
        for end in 0..bytes.len() {
            assert!(decode_request(&bytes[..end], 7).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_request(&trailing, 7).is_err());
        assert!(decode_request(&bytes, 8).is_err());
    }
    // Action 5 was the retired value replacement.
    for (tag, value) in [
        (8, vec![]),
        (5, vec![]),
        (5, vec![1, 1]),
        (11, vec![]),
        (2, f32::NAN.to_le_bytes().to_vec()),
        (3, vec![1, 0, 0, 0, 255]),
    ] {
        assert!(decode_request(&action(tag, &value), 7).is_err());
    }
}

#[test]
fn retired_gui_action_request_is_rejected() {
    let bytes = envelope(32, &[0; 35]);
    assert!(decode_request(&bytes, 7).is_err());
}

#[test]
fn retired_snapshot_request_is_rejected() {
    let bytes = envelope(31, &[1, 0, 0, 1, 0, 64, 0]);
    assert!(decode_request(&bytes, 7).is_err());
}

/// One observed effect record, as a subscribed client receives it.
fn observed(effect: GuiLocalEffect) -> Vec<u8> {
    let mut writer = Writer::new(Vec::new());
    writer
        .gui_observation(&GuiObservationRecord::Effect {
            subscription: GuiObservationSubscriptionId {
                output: 1,
                generation: 1,
            },
            effect: effect.into(),
        })
        .unwrap();
    writer.0
}

fn effect_target() -> GuiEntityTarget {
    let mut host = ipp_core::HostRuntime::new();
    let id = host.create_world(Default::default(), &[]).unwrap();
    GuiEntityTarget {
        world: host.world_ref(id).unwrap(),
        entity: EntityId::from_bits(3),
        component: 51,
        incarnation: 4,
    }
}

fn effect(target: GuiEntityTarget, kind: GuiLocalEffectKind) -> GuiLocalEffect {
    GuiLocalEffect {
        id: Some(ipp_core::systems::gui::observations::GuiEffectId {
            world: target.world,
            ordinal: 1,
        }),
        target,
        source: GuiLocalEffectSource::Semantic,
        tick: 8,
        ancestry: vec![EntityId::from_bits(3)].into(),
        kind,
    }
}

#[test]
fn focus_effect_encodes_result_and_change_without_a_value_revision() {
    let target = effect_target();
    for focused in [false, true] {
        let bytes = observed(effect(
            target,
            GuiLocalEffectKind::FocusChanged {
                focused,
                changed: true,
            },
        ));
        assert_eq!(&bytes[bytes.len() - 3..], &[1, u8::from(focused), 1]);
    }
}

#[test]
fn press_and_submission_effects_encode_their_payloads() {
    let target = effect_target();
    let pressed = observed(effect(target, GuiLocalEffectKind::Pressed));
    assert_eq!(pressed.last(), Some(&0));
    let submitted = observed(effect(target, GuiLocalEffectKind::Submitted("sent".into())));
    assert_eq!(submitted.len(), pressed.len() + 4 + 4);
    let kind = pressed.len() - 1;
    assert_eq!(submitted[kind], 4);
    assert_eq!(&submitted[kind + 1..kind + 5], &4u32.to_le_bytes());
    assert_eq!(&submitted[kind + 5..], b"sent");
}

#[test]
fn observation_registration_syntax_is_complete_before_world_admission() {
    let mut payloads = Vec::new();
    for class in 0..=2 {
        let mut payload = vec![0];
        payload.extend(1u64.to_le_bytes());
        payload.extend(2u64.to_le_bytes());
        payload.push(class);
        payloads.push(payload);
    }
    let mut unsubscribe = vec![1];
    for identity in [1u64, 2, 3, 4] {
        unsubscribe.extend(identity.to_le_bytes());
    }
    payloads.push(unsubscribe);

    for mut payload in payloads {
        let bytes = envelope(crate::wire::REQUEST_GUI_OBSERVATION, &payload);
        assert!(matches!(
            decode_request(&bytes, 7).unwrap().body,
            RequestBody::GuiObservation(_)
        ));
        for end in 0..bytes.len() {
            assert!(decode_request(&bytes[..end], 7).is_err());
        }
        payload.push(0);
        assert!(
            decode_request(&envelope(crate::wire::REQUEST_GUI_OBSERVATION, &payload), 7).is_err()
        );
    }
}

#[test]
fn observation_peak_encoding_bound_includes_framing_but_does_not_invent_a_tick() {
    use crate::{Response, ResponseBody, encode_response, encoded_response_size};
    use ipp_core::systems::gui::observations::GuiEffectId;
    use std::sync::Arc;

    let mut host = ipp_core::HostRuntime::new();
    let world = host.create_world(Default::default(), &[]).unwrap();
    let world = host.world_ref(world).unwrap();
    let subscription = GuiObservationSubscriptionId {
        output: 1,
        generation: 2,
    };
    for result in [
        GuiObservationControlResult::Subscribed,
        GuiObservationControlResult::Unsubscribed,
        GuiObservationControlResult::Cancelled,
        GuiObservationControlResult::Rejected(GuiObservationRejection::StaleWorld),
        GuiObservationControlResult::Rejected(GuiObservationRejection::StaleSubscription),
        GuiObservationControlResult::Rejected(GuiObservationRejection::AlreadySubscribed),
    ] {
        let response = Response {
            session: 7,
            request_id: 9,
            tick: 0,
            body: ResponseBody::GuiObservation(GuiObservationRecord::Control {
                world,
                subscription,
                request: 9,
                result,
            }),
        };
        let bytes = encode_response(&response).unwrap();
        assert_eq!(encoded_response_size(&response).unwrap(), bytes.len());
        assert!(bytes.capacity() <= GUI_OBSERVATION_ENCODING.control_bytes);
    }

    for text_bytes in [0, 65536] {
        let effect = GuiLocalEffect {
            id: Some(GuiEffectId {
                world,
                ordinal: 19,
            }),
            target: GuiEntityTarget {
                world,
                entity: EntityId::from_bits(3),
                component: 51,
                incarnation: 4,
            },
            source: GuiLocalEffectSource::Semantic,
            tick: 83,
            ancestry: vec![EntityId::from_bits(3); 1024].into(),
            kind: GuiLocalEffectKind::Submitted("x".repeat(text_bytes).into()),
        };
        let bound = GUI_OBSERVATION_ENCODING.effect_bytes
            + effect.ancestry.len() * GUI_OBSERVATION_ENCODING.ancestry_entry_bytes
            + text_bytes * GUI_OBSERVATION_ENCODING.text_byte_bytes;
        let mut response = Response {
            session: 7,
            request_id: 0,
            tick: 0,
            body: ResponseBody::GuiObservation(GuiObservationRecord::Effect {
                subscription,
                effect: Arc::new(effect),
            }),
        };
        let bytes = encode_response(&response).unwrap();
        assert_eq!(encoded_response_size(&response).unwrap(), bytes.len());
        assert!(bytes.capacity() <= bound);
        assert_eq!(&bytes[16..24], &0u64.to_le_bytes());
        response.tick = 83;
        assert!(encode_response(&response).is_err());
        response.tick = 0;
        response.request_id = 9;
        assert!(encode_response(&response).is_err());
    }
}

#[test]
fn routed_observation_fits_the_reserved_wire_bound() {
    use ipp_core::systems::gui::observations::GuiEffectId;
    use std::sync::Arc;
    let mut host = ipp_core::HostRuntime::new();
    let world = host.create_world(Default::default(), &[]).unwrap();
    let world = host.world_ref(world).unwrap();
    host.frame(0.0).unwrap();
    let publication = host.latest_publication(world.id()).unwrap();
    let effect = GuiLocalEffect {
        id: Some(GuiEffectId {
            world,
            ordinal: 1,
        }),
        target: GuiEntityTarget {
            world,
            entity: EntityId::from_bits(3),
            component: 47,
            incarnation: 4,
        },
        source: GuiLocalEffectSource::Routed {
            publication,
        },
        tick: 8,
        ancestry: vec![EntityId::from_bits(3)].into(),
        kind: GuiLocalEffectKind::FocusChanged {
            focused: true,
            changed: true,
        },
    };
    let response = crate::Response {
        session: 7,
        request_id: 0,
        tick: 0,
        body: crate::ResponseBody::GuiObservation(GuiObservationRecord::Effect {
            subscription: GuiObservationSubscriptionId {
                output: 1,
                generation: 2,
            },
            effect: Arc::new(effect),
        }),
    };
    let bytes = crate::encode_response(&response).unwrap();
    // Envelope and framing 29, subscription and record kind 17, identity 25,
    // target 34, routed source 17, tick 8, one ancestor 12 and the focus payload 3.
    assert_eq!(bytes.len(), 145);
    assert_eq!(
        crate::encoded_response_size(&response).unwrap(),
        bytes.len()
    );
    // The largest fixed payload replaces the two focus flags within the same bound.
    assert!(
        bytes.len() - 2 + GUI_EFFECT_PAYLOAD_BYTES
            <= GUI_OBSERVATION_ENCODING.effect_bytes
                + GUI_OBSERVATION_ENCODING.ancestry_entry_bytes
    );
}
