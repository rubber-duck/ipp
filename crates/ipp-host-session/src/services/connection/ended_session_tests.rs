//! Late World messages of a session that has ended on its connection.
//!
//! A client keeps sending on a session until it reads the Host's detach reply or
//! destruction notice, so messages for that session can arrive after the Host
//! ended it. Each test orders those arrivals explicitly after the Host processed
//! the end, which a real transport only does when a Host frame falls between them.

use super::*;

/// An inspect request of `session`.
fn inspect(session: u64, request: u64) -> Vec<u8> {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(3);
    bytes.push(1);
    bytes.extend([0; 16]);
    bytes.extend(256u16.to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    bytes
}

/// One empty batch page of `session`.
fn page(session: u64, request: u64, batch: u32, last: bool) -> Vec<u8> {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(1);
    bytes.extend(batch.to_le_bytes());
    bytes.push(u8::from(last));
    bytes.extend(0u32.to_le_bytes());
    bytes
}

/// A client asset source frame of `session` that ends an absent transfer.
fn source(session: u64, request: u64) -> Vec<u8> {
    let mut bytes = ipp_protocol::asset_source::REQUEST_MAGIC.to_vec();
    bytes.extend(session.to_le_bytes());
    bytes.extend(request.to_le_bytes());
    bytes.push(2);
    bytes.extend(1u64.to_le_bytes());
    bytes
}

fn attached_world(
    host: &mut Host<TestHostServices>,
    id: u64,
) -> (u64, ipp_protocol::references::WorldReference) {
    let HostResponseBody::Attached {
        session,
        reference,
        ..
    } = create_and_open(
        host,
        id,
        HostRequestBody::CreateWorld {
            options: host::WorldCreateOptions::new(Vec::new()),
            temporary: false,
        },
    )
    else {
        panic!("attachment expected")
    };
    (session, reference)
}

/// Deliver every late message shape on its own receive path and require that the
/// connection neither fails nor answers them.
fn deliver_late_messages(host: &mut Host<TestHostServices>, session: u64) {
    while host.take_connection_response(1).is_some() {}
    host.receive_connection(1, &inspect(session, 7)).unwrap();
    host.receive_connection(1, &page(session, 8, 3, true))
        .unwrap();
    host.receive_connection_message(1, HostConnectionMessage::decode(page(session, 0, 4, false)))
        .unwrap();
    host.receive_connection_message(1, HostConnectionMessage::decode(page(session, 9, 4, true)))
        .unwrap();
    host.receive_connection(1, &source(session, 10)).unwrap();
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert!(host.connections.states[&1].failure.is_none());
    assert!(host.take_connection_response(1).is_none());
}

/// The connection still opens and authors Worlds after the late messages.
fn assert_usable(host: &mut Host<TestHostServices>) {
    let (session, _) = attached_world(host, 1);
    while host.take_connection_response(1).is_some() {}
    host.receive_connection(1, &inspect(session, 11)).unwrap();
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    assert!(
        std::iter::from_fn(|| host.take_connection_response(1)).any(|reply| {
            !reply.starts_with(host::HOST_RESPONSE_MAGIC) && reply[8..16] == 11u64.to_le_bytes()
        })
    );
}

#[test]
fn late_messages_of_a_destroyed_world_session_leave_the_connection_usable() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let (session, reference) = attached_world(&mut host, 1);
    assert!(matches!(
        control(&mut host, 1, HostRequestBody::DestroyWorld(reference)),
        HostResponseBody::Detached { session: detached, .. } if detached == session
    ));
    deliver_late_messages(&mut host, session);
    assert_usable(&mut host);
}

#[test]
fn late_messages_of_a_detached_session_leave_the_connection_usable() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    let (session, _) = attached_world(&mut host, 1);
    assert!(matches!(
        control(
            &mut host,
            1,
            HostRequestBody::DetachWorld {
                session
            }
        ),
        HostResponseBody::Complete
    ));
    deliver_late_messages(&mut host, session);
    assert_usable(&mut host);
}

#[test]
fn a_session_the_connection_never_held_still_fails_it() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    open(&mut host, 1);
    open(&mut host, 2);
    let (foreign, reference) = attached_world(&mut host, 2);
    assert!(matches!(
        control(&mut host, 2, HostRequestBody::DestroyWorld(reference)),
        HostResponseBody::Detached { .. }
    ));
    assert_eq!(
        host.receive_connection(1, &inspect(foreign, 7)),
        Err("SessionMismatch".into())
    );
    assert_eq!(
        host.receive_connection_message(
            1,
            HostConnectionMessage::decode(page(foreign, 8, 3, true))
        ),
        Err("SessionMismatch".into())
    );
    assert!(host.receive_connection(1, &source(foreign, 9)).is_err());
}
