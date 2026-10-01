use super::*;
use ipp_core::systems::gui::local::{GuiButton, GuiEntityTarget};
use ipp_core::{Batch, Command, ComponentValue, EntityRef};
use ipp_protocol::references::WorldReference;

fn fixture() -> (Host<TestHostServices>, [u64; 3], GuiEntityTarget) {
    let mut host = Host::new().unwrap();
    for connection in 1..=3 {
        open(&mut host, connection);
    }
    let HostResponseBody::Attached {
        session: issuer,
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
        panic!("issuer")
    };
    let mut sessions = [issuer, 0, 0];
    for connection in 2..=3 {
        let HostResponseBody::Attached {
            session,
            ..
        } = control(&mut host, connection, HostRequestBody::OpenWorld(reference))
        else {
            panic!("observer")
        };
        sessions[connection as usize - 1] = session;
    }
    let operations = vec![
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
    let world = reference.resolve(host.runtime()).unwrap();
    host.runtime
        .world_mut(world.id())
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
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
    let observed = gui_target(&mut host, world.id(), entity, ComponentValue::GUI_BUTTON);

    for connection in 1..=3 {
        drop(drain(&mut host, connection));
    }
    (host, sessions, observed)
}

fn send(
    host: &mut Host<TestHostServices>,
    connection: u64,
    session: u64,
    request: u64,
    tag: u8,
    payload: &[u8],
) {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(tag);
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    host.receive_connection(connection, &bytes).unwrap();
}

fn subscribe(
    host: &mut Host<TestHostServices>,
    connection: u64,
    session: u64,
    world: WorldReference,
) {
    let mut payload = vec![0];
    payload.extend(world.id.to_le_bytes());
    payload.extend(world.incarnation.to_le_bytes());
    payload.push(0);
    send(host, connection, session, 1, 34, &payload);
}

fn press(
    host: &mut Host<TestHostServices>,
    session: u64,
    observed: &GuiEntityTarget,
    request: u64,
) {
    // A one-command batch page, identified by its request.
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(1);
    let mut page = gui_action_payload(observed, 0);
    page[..4].copy_from_slice(&(request as u32).to_le_bytes());
    bytes.extend(page);
    host.receive_connection(1, &bytes).unwrap();
}

/// Successful batch outcomes among the issuer's replies.
fn applied(replies: &[ReliableResponse]) -> usize {
    replies
        .iter()
        .filter(|reply| {
            // Batch identity and tick precede the outcome tag.
            !reply.starts_with(host::HOST_RESPONSE_MAGIC) && reply[24] == 1 && reply[41] == 0
        })
        .count()
}

fn drain(host: &mut Host<TestHostServices>, connection: u64) -> Vec<ReliableResponse> {
    std::iter::from_fn(|| host.take_connection_response(connection)).collect()
}

fn observations(replies: &[ReliableResponse]) -> Vec<&[u8]> {
    replies
        .iter()
        .filter(|reply| !reply.starts_with(host::HOST_RESPONSE_MAGIC) && reply[24] == 36)
        .map(|reply| &reply[..])
        .collect()
}

#[test]
fn slow_foreign_observer_fails_only_its_connection_and_handoff_retains_shared_credit() {
    let (mut host, sessions, observed) = fixture();
    for connection in 2..=3 {
        subscribe(
            &mut host,
            connection,
            sessions[connection as usize - 1],
            observed.world.into(),
        );
    }
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    for connection in 1..=3 {
        drop(drain(&mut host, connection));
    }
    let account = host.connections.states[&2].reply_budget.0.clone();
    let mut retained = Vec::new();
    let mut failed = false;
    let mut applied_actions = 0;
    // Every record pays at least its bookkeeping, so the ordinary share must run out.
    let groups = crate::reliable_output::ORDINARY_OUTPUT_BYTES
        / (16 * crate::reliable_output::RESPONSE_METADATA_BYTES)
        + 1;
    for group in 0..groups as u64 {
        for index in 0..16 {
            press(&mut host, sessions[0], &observed, 10 + group * 16 + index);
        }
        let failures = host.tick_worlds(0.0).unwrap();
        assert!(
            failures.iter().all(|(connection, _)| *connection == 2),
            "{failures:?}"
        );
        let issuer = drain(&mut host, 1);
        assert_eq!(applied(&issuer), 16);
        applied_actions += 16;
        let healthy = drain(&mut host, 3);
        let effects = observations(&healthy);
        assert_eq!(effects.len(), 16);
        assert!(
            effects
                .iter()
                .all(|effect| effect[45] == 1 && effect[16..24] == [0; 8])
        );
        let mut before = account.usage();
        let delivered = drain(&mut host, 2);
        for response in &delivered {
            if response.reservation.borrow().is_progress() {
                before.entries += 1;
                before.bytes += response.len() + crate::reliable_output::RESPONSE_METADATA_BYTES;
            }
        }
        retained.extend(delivered);
        assert_eq!(
            before,
            account.usage(),
            "Moving outbox bytes cannot release credit"
        );
        assert!(account.usage().bytes <= crate::reliable_output::ORDINARY_OUTPUT_BYTES);
        if !failures.is_empty() {
            failed = true;
            break;
        }
    }
    assert!(failed && applied_actions >= 64);
    assert!(!host.connection_accepts_input(2));
    host.close_connection(2);
    assert!(
        account.usage().entries > 0,
        "Unflushed bytes survived closure but lost credit"
    );
    drop(retained);
    assert_eq!(account.usage().entries, 0);
    press(&mut host, sessions[0], &observed, 200);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert_eq!(observations(&drain(&mut host, 3)).len(), 1);
    assert_eq!(applied(&drain(&mut host, 1)), 1);
}

#[test]
fn sibling_sessions_observation_payloads_share_the_physical_connection_account() {
    let (mut host, sessions, observed) = fixture();
    let HostResponseBody::Attached {
        session: sibling,
        ..
    } = control(
        &mut host,
        2,
        HostRequestBody::OpenWorld(observed.world.into()),
    )
    else {
        panic!("sibling")
    };
    for session in [sessions[1], sibling] {
        subscribe(&mut host, 2, session, observed.world.into());
    }
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    for connection in 1..=3 {
        drop(drain(&mut host, connection));
    }
    let account = host.connections.states[&2].reply_budget.0.clone();
    let before = account.usage();
    press(&mut host, sessions[0], &observed, 3);
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    let replies = drain(&mut host, 2);
    let effects = observations(&replies);
    assert_eq!(effects.len(), 2);
    assert_ne!(&effects[0][..8], &effects[1][..8]);
    assert_eq!(&effects[0][46..], &effects[1][46..]);
    assert_eq!(account.usage().entries, replies.len());
    assert!(
        account.usage().bytes
            > before.bytes + replies.iter().map(|reply| reply.len()).sum::<usize>()
    );
    drop(replies);
    assert_eq!(account.usage(), before);
}
