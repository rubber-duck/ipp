use super::*;
use ipp_core::systems::gui::local::{GuiButton, GuiEntityTarget};
use ipp_core::{Batch, Command, ComponentValue, EntityRef};
use ipp_protocol::references::WorldReference;

fn fixture() -> (Host<TestHostServices>, u64, WorldReference, GuiEntityTarget) {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let HostResponseBody::Attached {
        session,
        reference,
        ..
    } = create_and_open(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(vec!["ipp.canvas".into(), "ipp.gui".into()]),
            temporary: false,
        },
    )
    else {
        panic!("GUI session")
    };
    let world = reference.resolve(host.runtime()).unwrap();
    let commands = vec![
        Command::Create {
            alias: 1,
            metadata: Default::default(),
            adopt: false,
        },
        Command::insert_value(
            EntityRef::Alias(1),
            ComponentValue::GuiButton(GuiButton::default()),
        ),
    ];
    host.runtime
        .world_mut(world.id())
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: commands,
        })
        .unwrap();

    let entity = host
        .runtime
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world.id())
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()[0]
        .1;

    host.tick_worlds(0.0).unwrap();
    let target = gui_target(&mut host, world.id(), entity, ComponentValue::GUI_BUTTON);
    drain(&mut host);
    (host, session, reference, target)
}

fn world_request(
    host: &mut Host<TestHostServices>,
    session: u64,
    request: u64,
    tag: u8,
    payload: &[u8],
) {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(tag);
    bytes.extend(payload);
    host.receive_connection(1, &bytes).unwrap();
}

fn drain(host: &mut Host<TestHostServices>) -> Vec<ReliableResponse> {
    std::iter::from_fn(|| host.take_connection_response(1)).collect()
}

/// Submit a one-command batch pressing the button.
fn action(host: &mut Host<TestHostServices>, session: u64, target: &GuiEntityTarget) {
    world_request(host, session, 41, 1, &gui_action_payload(target, 0));
}

#[test]
fn gui_does_not_pass_blocked_host_control() {
    let (mut host, session, reference, target) = fixture();
    host.session_mut(session)
        .unwrap()
        .receive_decoded(
            ipp_protocol::Request {
                session,
                request_id: 39,
                body: ipp_protocol::RequestBody::Inspect(Default::default()),
            },
            None,
        )
        .unwrap();
    host.receive_connection(
        1,
        &host::encode_host_request(&HostRequest {
            connection: 1,
            request_id: 40,
            body: HostRequestBody::RenameWorld {
                world: ipp_core::WorldSelector::Id(WorldId(reference.id)),
                symbolic_id: "ordered".into(),
            },
        })
        .unwrap(),
    )
    .unwrap();
    action(&mut host, session, &target);
    assert!(host.process_host_requests().is_empty());
    assert_eq!(host.connections.states[&1].pending.len(), 2);
    assert!(host.admit_session_requests(0.0).is_empty());
    host.tick_worlds(0.0).unwrap();
    host.tick_worlds(0.0).unwrap();
    let replies = drain(&mut host);

    // The batch outcome answers request 41 with success.
    let outcome = replies
        .iter()
        .find(|reply| {
            !reply.starts_with(host::HOST_RESPONSE_MAGIC)
                && reply[8..16] == 41u64.to_le_bytes()
                && reply[24] == 1
        })
        .unwrap();
    // Batch identity and tick precede the outcome tag.
    assert_eq!(&outcome[25..33], &1u64.to_le_bytes());
    assert_eq!(outcome[41], 0);
}
