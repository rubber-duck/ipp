use super::*;
use ipp_core::components::{GuiButton, GuiLayout};
use ipp_core::services::gui_input::router::GuiPhysicalInput;
use ipp_core::{EntityPlacementRef, WorldRef};
use ipp_protocol::host::gui_input::{GuiPhysicalRequest, GuiPhysicalResponse};

fn ordinary() -> (crate::Host<Platform>, PresentationView, WorldRef) {
    let (mut host, _, _, _) = connected_surface();
    let world = host
        .runtime
        .create_world(
            Default::default(),
            &[
                ipp_core::systems::canvas::CanvasSystem::ID,
                ipp_core::systems::gui::GuiSystem::ID,
                ipp_core::systems::gui::GuiLayoutSystem::ID,
            ],
        )
        .unwrap();
    let reference = host.runtime.world_ref(world).unwrap();
    host.runtime
        .world_mut(world)
        .unwrap()
        .enqueue_canvas_state_update(ipp_core::CanvasStateUpdate {
            extent: Some([100.0, 100.0]),
            units_per_metre: Some(1.0),
        })
        .unwrap();
    let commands = vec![
        Command::Create {
            alias: 1,
            metadata: Default::default(),
            adopt: false,
        },
        Command::Create {
            alias: 2,
            metadata: Default::default(),
            adopt: false,
        },
        Command::PlaceEntity {
            entity: EntityRef::Alias(2),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Alias(1)),
                before: None,
            },
        },
        Command::insert_value(
            EntityRef::Alias(2),
            ComponentValue::GuiButton(GuiButton::default()),
        ),
        Command::insert_value(
            EntityRef::Alias(2),
            ComponentValue::GuiLayout(GuiLayout {
                width: 40.0,
                height: 40.0,
                ..Default::default()
            }),
        ),
    ];
    host.runtime
        .world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: commands,
        })
        .unwrap();

    host.runtime
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap();

    let output = ipp_core::OutputRef::canvas(reference);
    host.runtime
        .set_root_output(
            output,
            WorldViewport {
                width: 100,
                height: 100,
                device_pixel_ratio: 1.0,
            },
        )
        .unwrap();
    host.tick_worlds(0.0).unwrap();
    let binding = RootBinding::from(
        host.runtime
            .root_output_binding(reference)
            .unwrap()
            .unwrap(),
    );
    let view = select(
        &mut host.presentation,
        &mut host.runtime,
        &mut host.services,
        binding,
    );
    control_replies(&mut host);
    (host, view, reference)
}

fn physical(host: &mut crate::Host<Platform>, request: u64, input: GuiPhysicalRequest) {
    control(host, request, HostRequestBody::GuiInput(input));
}

fn acquire(host: &mut crate::Host<Platform>, view: PresentationView) -> u64 {
    physical(
        host,
        100,
        GuiPhysicalRequest::Open {
            view,
            blockers: Vec::new(),
        },
    );
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    control_replies(host)
        .into_iter()
        .find_map(|reply| match reply.body {
            HostResponseBody::GuiInput(GuiPhysicalResponse::Opened(context)) => Some(context),
            _ => None,
        })
        .unwrap()
}

#[test]
fn connection_routes_ordinary_canvas_pointer_and_settles_existing_outbox_once() {
    let (mut host, view, _) = ordinary();
    let context = acquire(&mut host, view);
    physical(
        &mut host,
        101,
        GuiPhysicalRequest::Event {
            context,
            input: GuiPhysicalInput::PointerDown {
                button: ipp_core::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.1, 0.1],
            },
        },
    );
    physical(
        &mut host,
        102,
        GuiPhysicalRequest::Event {
            context,
            input: GuiPhysicalInput::PointerUp {
                button: ipp_core::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.1, 0.1],
            },
        },
    );
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert!(
        control_replies(&mut host)
            .iter()
            .all(|reply| !matches!(reply.request_id, 101 | 102))
    );
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    let replies = control_replies(&mut host);
    let outcomes: Vec<_> = replies
        .into_iter()
        .filter_map(|reply| match reply.body {
            HostResponseBody::GuiInput(response) => Some((reply.request_id, response)),
            _ => None,
        })
        .collect();
    assert!(
        matches!(
            &outcomes[..],
            [
                (
                    101,
                    GuiPhysicalResponse::Routed {
                        applied: 4,
                        rejected: 0,
                        cancelled: 0,
                        error: None,
                        ..
                    }
                ),
                (
                    102,
                    GuiPhysicalResponse::Routed {
                        applied: 3,
                        rejected: 0,
                        cancelled: 0,
                        error: None,
                        ..
                    }
                )
            ]
        ),
        "{outcomes:?}"
    );
    host.tick_worlds(0.0).unwrap();
    assert!(control_replies(&mut host).is_empty());
}

#[test]
fn replacement_releases_queued_payloads_before_binding_the_new_context() {
    let (mut host, view, _) = ordinary();
    let context = acquire(&mut host, view);
    physical(
        &mut host,
        101,
        GuiPhysicalRequest::Event {
            context,
            input: GuiPhysicalInput::PointerDown {
                button: ipp_core::services::gui_input::router::GuiPhysicalButton::Primary,
                pointer: 1,
                point: [0.1, 0.1],
            },
        },
    );
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    physical(
        &mut host,
        102,
        GuiPhysicalRequest::Open {
            view,
            blockers: Vec::new(),
        },
    );
    assert!(host.process_host_requests().is_empty());
    let replies = control_replies(&mut host);
    assert!(replies.iter().any(|reply| matches!(
        reply.body,
        HostResponseBody::GuiInput(GuiPhysicalResponse::Routed {
            applied: 0,
            cancelled: 4,
            ..
        })
    )));
    assert!(replies.iter().any(|reply| matches!(reply.body, HostResponseBody::GuiInput(GuiPhysicalResponse::Revoked(old)) if old == context)));
    assert!(replies.iter().any(|reply| matches!(reply.body, HostResponseBody::GuiInput(GuiPhysicalResponse::Opened(new)) if new != context)));
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert!(control_replies(&mut host).is_empty());
}
