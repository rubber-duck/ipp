use super::*;

fn hello(boundary: &mut WasmHostBoundary, connection: u64) {
    assert!(boundary.connection_open(connection));
    let bytes = ipp_protocol::HELLO;
    assert!(!boundary.reserve(bytes.len()).is_null());
    boundary.input_mut().copy_from_slice(&bytes);
    assert!(boundary.receive(connection, bytes.len()));
}

fn available(boundary: &WasmHostBoundary, connection: u64) -> usize {
    let host = boundary.host.as_ref().unwrap();
    let mut lower = 0;
    let mut upper = 8 * MAX_MESSAGE_BYTES + 1;
    while upper - lower > 1 {
        let middle = lower + (upper - lower) / 2;
        if host
            .reserve_connection_output_bytes(connection, middle)
            .is_ok()
        {
            lower = middle;
        } else {
            upper = middle;
        }
    }
    lower
}

#[test]
fn copied_wasm_output_keeps_credit_through_mutations_until_exact_completion() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.open(1));
    hello(&mut boundary, 1);
    let before = available(&boundary, 1);
    assert_eq!(boundary.poll(1), 1);
    let delivery = boundary.output_delivery_id();
    let bytes = boundary.output().to_vec();
    let peak = available(&boundary, 1);
    assert!(before - peak >= bytes.len());
    assert!(boundary.output_copied(1, delivery));
    assert!(boundary.output_ptr().is_null());
    let retained = available(&boundary, 1);
    assert!(retained > peak);
    assert_eq!(boundary.poll(1), 0);
    assert!(boundary.tick(0.0));
    assert!(boundary.service_resources());
    assert_eq!(available(&boundary, 1), retained);
    assert_eq!(boundary.connection_pending(1), 1);
    drop(bytes);
    assert!(boundary.delivery_complete(1, delivery));
    assert!(available(&boundary, 1) >= retained + ipp_protocol::HELLO.len());
    assert_eq!(boundary.connection_pending(1), 0);
}

/// Deliver one Host control request on `connection` and return its decoded reply.
fn control(
    boundary: &mut WasmHostBoundary,
    connection: u64,
    request_id: u64,
    body: ipp_protocol::host::HostRequestBody,
) -> ipp_protocol::host::HostResponseBody {
    let bytes = ipp_protocol::host::encode_host_request(&ipp_protocol::host::HostRequest {
        connection,
        request_id,
        body,
    })
    .unwrap();
    assert!(!boundary.reserve(bytes.len()).is_null());
    boundary.input_mut().copy_from_slice(&bytes);
    assert!(boundary.receive(connection, bytes.len()));
    assert!(boundary.tick(0.0));
    loop {
        assert_eq!(boundary.poll(connection), 1);
        let delivery = boundary.output_delivery_id();
        let reply = boundary
            .output()
            .starts_with(ipp_protocol::host::HOST_RESPONSE_MAGIC)
            .then(|| ipp_protocol::host::decode_host_response(boundary.output(), connection))
            .transpose()
            .unwrap();
        assert!(boundary.output_copied(connection, delivery));
        assert!(boundary.delivery_complete(connection, delivery));
        if let Some(reply) = reply
            && reply.request_id == request_id
        {
            return reply.body;
        }
    }
}

/// Attach `connection` to a new temporary World so each Host frame queues progress for it.
fn attach(boundary: &mut WasmHostBoundary, connection: u64) {
    let ipp_protocol::host::HostResponseBody::Created {
        reference,
        ..
    } = control(
        boundary,
        connection,
        1,
        ipp_protocol::host::HostRequestBody::CreateWorld {
            options: ipp_protocol::host::WorldCreateOptions::new(Vec::new()),
            temporary: true,
        },
    )
    else {
        panic!("World not created")
    };
    assert!(matches!(
        control(
            boundary,
            connection,
            2,
            ipp_protocol::host::HostRequestBody::OpenWorld(reference),
        ),
        ipp_protocol::host::HostResponseBody::Attached { .. }
    ));
    while boundary.poll(connection) == 1 {
        let delivery = boundary.output_delivery_id();
        assert!(boundary.output_copied(connection, delivery));
        assert!(boundary.delivery_complete(connection, delivery));
    }
}

#[test]
fn reply_copies_use_the_reply_reserve_when_ordinary_output_is_full() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.open(1));
    hello(&mut boundary, 1);
    let held = boundary
        .host
        .as_ref()
        .unwrap()
        .reserve_connection_output_bytes(1, available(&boundary, 1))
        .unwrap();
    assert_eq!(available(&boundary, 1), 0);
    assert_eq!(boundary.poll(1), 1);
    let delivery = boundary.output_delivery_id();
    assert_eq!(
        boundary.output(),
        ipp_protocol::accept_hello(&ipp_protocol::HELLO, 1).unwrap()
    );
    assert!(boundary.output_copied(1, delivery));
    assert!(boundary.delivery_complete(1, delivery));
    assert!(!boundary.connection_failure(1));
    drop(held);
}

#[test]
fn copy_peak_capacity_failure_is_connection_local_and_exposes_no_client_bytes() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.open(1));
    hello(&mut boundary, 1);
    hello(&mut boundary, 2);
    let delivery = {
        assert_eq!(boundary.poll(1), 1);
        boundary.output_delivery_id()
    };
    assert!(boundary.output_copied(1, delivery));
    assert!(boundary.delivery_complete(1, delivery));
    attach(&mut boundary, 1);
    assert!(boundary.tick(0.0));

    // Room for the next progress record (its small frame plus 1 KiB bookkeeping) but not for
    // its transport copy, which charges the same again.
    let room = 1536;
    let held = boundary
        .host
        .as_ref()
        .unwrap()
        .reserve_connection_output_bytes(1, available(&boundary, 1) - room)
        .unwrap();
    assert_eq!(boundary.poll(1), -1);
    assert_eq!(boundary.output_delivery_id(), 0);
    assert_eq!(boundary.connection_pending(1), 0);
    assert!(
        std::str::from_utf8(boundary.output())
            .unwrap()
            .contains("copy capacity")
    );
    assert_eq!(boundary.poll(2), 1);
    let delivery = boundary.output_delivery_id();
    assert!(boundary.output_copied(2, delivery));
    assert!(boundary.delivery_complete(2, delivery));
    assert!(boundary.accepts_input(2));
    drop(held);
}

#[test]
fn duplicate_foreign_and_retired_acknowledgements_cannot_release_peer_credit() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.open(1));
    hello(&mut boundary, 1);
    hello(&mut boundary, 2);
    assert_eq!(boundary.poll(1), 1);
    let first = boundary.output_delivery_id();
    assert!(boundary.output_copied(1, first));
    assert_eq!(boundary.poll(2), 1);
    let second = boundary.output_delivery_id();
    assert!(second > first);
    assert!(boundary.output_copied(2, second));
    let retained = available(&boundary, 2);
    assert!(boundary.delivery_complete(1, first));
    assert!(!boundary.delivery_complete(1, first));
    assert_eq!(available(&boundary, 2), retained);
    assert_eq!(boundary.connection_pending(2), 1);
    assert!(boundary.connection_dispose(1));
    assert!(!boundary.connection_open(1));
    assert!(!boundary.delivery_complete(1, second));
    assert_eq!(available(&boundary, 2), retained);
    assert!(boundary.delivery_complete(2, second));
    assert!(boundary.accepts_input(2));
}

#[test]
fn closed_unacknowledged_connections_bound_reconnect_churn_until_disposal() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.open(1));
    for connection in 1..=MAX_CONNECTIONS as u64 {
        hello(&mut boundary, connection);
        assert_eq!(boundary.poll(connection), 1);
        let delivery = boundary.output_delivery_id();
        assert!(boundary.output_copied(connection, delivery));
        assert!(boundary.connection_close(connection));
        assert_eq!(boundary.connection_pending(connection), 1);
    }
    assert_eq!(boundary.connections.len(), MAX_CONNECTIONS);
    assert!(!boundary.connection_open(MAX_CONNECTIONS as u64 + 1));
    assert!(boundary.tick(0.0));
    assert_eq!(boundary.connections.len(), MAX_CONNECTIONS);
    assert!(boundary.connection_dispose(1));
    assert!(boundary.connection_open(MAX_CONNECTIONS as u64 + 1));
    assert_eq!(boundary.connections.len(), MAX_CONNECTIONS);
    assert!(!boundary.delivery_complete(1, 1));
    assert!(boundary.accepts_input(MAX_CONNECTIONS as u64 + 1));
}

#[test]
fn host_reopen_cannot_discard_live_or_closing_delivery_accounts() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.open(1));
    hello(&mut boundary, 1);
    assert_eq!(boundary.poll(1), 1);
    let delivery = boundary.output_delivery_id();
    assert!(boundary.output_copied(1, delivery));
    assert!(!boundary.open(2));
    assert_eq!(boundary.connection_pending(1), 1);
    assert!(boundary.connection_close(1));
    assert!(!boundary.open(2));
    assert_eq!(boundary.connection_pending(1), 1);
    assert!(boundary.connection_dispose(1));
    boundary.close();
    assert!(boundary.open(2));
    assert!(boundary.connection_open(2));
    assert!(!boundary.delivery_complete(1, delivery));
    assert!(boundary.accepts_input(2));
}
