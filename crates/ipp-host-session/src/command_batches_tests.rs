//! Batch page assembly through the production connection ingress path.

use super::*;
use crate::HostConnectionMessage;
use crate::command_batches::{BATCH_DEADLINE, MAX_BUFFERED_BATCH_BYTES};
use ipp_protocol::host::{self, HostRequest, HostRequestBody, HostResponseBody};

struct Platform;

impl HostServices for Platform {
    const NAME: &'static str = "command-batch-test";

    fn initialize(
        _: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

const REQUEST_SUBMIT_BATCH: u8 = 1;

const RESPONSE_BATCH: u8 = 1;

const RESPONSE_BATCH_ABORTED: u8 = 24;

const RESPONSE_ERROR: u8 = 255;

const COMMAND_CREATE: u8 = 1;

const COMMAND_PLACE_ENTITY: u8 = 17;

const COMMAND_METADATA: u8 = 3;

const COMMAND_DELETE: u8 = 2;

const REF_ALIAS: u8 = 1;

const REF_SYMBOL: u8 = 3;

fn control(host: &mut Host<Platform>, connection: u64, body: HostRequestBody) -> HostResponseBody {
    while host.take_connection_response(connection).is_some() {}
    host.receive_connection(
        connection,
        &host::encode_host_request(&HostRequest {
            connection,
            request_id: 1,
            body,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(host.tick_worlds(0.0).unwrap().is_empty());
    host::decode_host_response(
        &host.take_connection_response(connection).unwrap(),
        connection,
    )
    .unwrap()
    .body
}

/// Open a connection with one session on a fresh World.
fn connect(host: &mut Host<Platform>, connection: u64) -> u64 {
    connect_with(host, connection, Vec::new())
}

/// Open a connection with one session on a fresh World of `systems`.
fn connect_with(host: &mut Host<Platform>, connection: u64, systems: Vec<String>) -> u64 {
    host.open_connection(connection).unwrap();
    host.receive_connection(connection, &ipp_protocol::HELLO)
        .unwrap();
    host.take_connection_response(connection).unwrap();
    let HostResponseBody::Created {
        reference,
        ..
    } = control(
        host,
        connection,
        HostRequestBody::CreateWorld {
            options: ipp_protocol::host::WorldCreateOptions::new(systems),
            temporary: true,
        },
    )
    else {
        panic!("World not created")
    };
    open_session(host, connection, reference)
}

fn open_session(
    host: &mut Host<Platform>,
    connection: u64,
    reference: ipp_protocol::references::WorldReference,
) -> u64 {
    let HostResponseBody::Attached {
        session,
        ..
    } = control(host, connection, HostRequestBody::OpenWorld(reference))
    else {
        panic!("World not attached")
    };
    drain(host, connection);
    session
}

fn drain(host: &mut Host<Platform>, connection: u64) -> Vec<Vec<u8>> {
    std::iter::from_fn(|| host.take_connection_response(connection))
        .map(|response| response.to_vec())
        .collect()
}

fn create(alias: u32) -> Vec<u8> {
    let mut bytes = vec![COMMAND_CREATE];
    bytes.extend(alias.to_le_bytes());
    bytes.push(0);
    bytes.extend(0u32.to_le_bytes());
    bytes.push(0);
    bytes
}

/// Place `child` below `parent`, both named by creation aliases.
fn place(child: u32, parent: u32) -> Vec<u8> {
    let mut bytes = vec![COMMAND_PLACE_ENTITY, REF_ALIAS];
    bytes.extend(child.to_le_bytes());
    bytes.extend([1, REF_ALIAS]);
    bytes.extend(parent.to_le_bytes());
    bytes.push(0);
    bytes
}

/// A metadata command of about `bytes` encoded bytes for alias 1.
fn bulky(bytes: usize) -> Vec<u8> {
    let half = bytes / 2;
    let mut command = vec![COMMAND_METADATA, REF_ALIAS];
    command.extend(1u32.to_le_bytes());
    command.push(1);
    command.extend((half as u32).to_le_bytes());
    command.extend(std::iter::repeat_n(b's', half));
    command.extend(1u32.to_le_bytes());
    command.extend((half as u32).to_le_bytes());
    command.extend(std::iter::repeat_n(b'c', half));
    command
}

fn page(session: u64, request: u64, batch: u32, last: bool, commands: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(REQUEST_SUBMIT_BATCH);
    bytes.extend(batch.to_le_bytes());
    bytes.push(u8::from(last));
    bytes.extend((commands.len() as u32).to_le_bytes());
    for command in commands {
        bytes.extend(command);
    }
    bytes
}

fn send(host: &mut Host<Platform>, connection: u64, bytes: &[u8]) {
    host.receive_connection(connection, bytes).unwrap();
}

/// Deliver a message decoded away from the Host, as a native socket thread does.
fn send_decoded(host: &mut Host<Platform>, connection: u64, bytes: &[u8]) {
    host.receive_connection_message(connection, HostConnectionMessage::decode(bytes.to_vec()))
        .unwrap();
}

/// Delete of alias 1: six encoded bytes, one full command slot once decoded.
fn delete() -> Vec<u8> {
    let mut bytes = vec![COMMAND_DELETE, REF_ALIAS];
    bytes.extend(1u32.to_le_bytes());
    bytes
}

/// A command tag no decoder implements.
fn unknown() -> Vec<u8> {
    vec![250]
}

/// Entity tree as (name, parent name, sibling order), independent of identities.
fn tree(host: &mut Host<Platform>, session: u64) -> Vec<(String, Option<String>, String)> {
    let entities = entities(host, session);
    let name = |id| {
        entities
            .iter()
            .find(|entity| entity.id == id)
            .and_then(|entity| entity.metadata.symbolic_id.clone())
            .unwrap_or_default()
    };
    let mut tree: Vec<_> = entities
        .iter()
        .map(|entity| {
            (
                entity.metadata.symbolic_id.clone().unwrap_or_default(),
                entity.link.parent.map(name),
                format!("{:?}", entity.link.order),
            )
        })
        .collect();
    tree.sort();
    tree
}

fn tick(host: &mut Host<Platform>) {
    assert_eq!(host.tick_worlds(0.0).unwrap(), Vec::new());
}

fn entities(host: &mut Host<Platform>, session: u64) -> Vec<ipp_core::EntitySnapshot> {
    host.session_mut(session).unwrap().world().entities()
}

/// Correlated replies with their request identity and tag, excluding frame progress.
fn replies(responses: &[Vec<u8>]) -> Vec<(u64, u8)> {
    responses
        .iter()
        .filter(|response| response[24] != 4)
        .map(|response| {
            (
                u64::from_le_bytes(response[8..16].try_into().unwrap()),
                response[24],
            )
        })
        .collect()
}

fn batch_reply(responses: &[Vec<u8>], request: u64) -> (u64, bool) {
    let response = responses
        .iter()
        .find(|response| {
            response[24] == RESPONSE_BATCH
                && u64::from_le_bytes(response[8..16].try_into().unwrap()) == request
        })
        .expect("batch outcome");
    (
        u64::from_le_bytes(response[25..33].try_into().unwrap()),
        response[41] == 0,
    )
}

fn error_message(responses: &[Vec<u8>], request: u64) -> String {
    let response = responses
        .iter()
        .find(|response| {
            response[24] == RESPONSE_ERROR
                && u64::from_le_bytes(response[8..16].try_into().unwrap()) == request
        })
        .expect("rejected batch");
    let length = u32::from_le_bytes(response[27..31].try_into().unwrap()) as usize;
    String::from_utf8(response[31..31 + length].to_vec()).unwrap()
}

fn reply_entries(host: &Host<Platform>, connection: u64) -> usize {
    host.connections.states[&connection].reply_entries()
}

#[test]
fn a_single_page_batch_is_one_correlated_request() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);

    send(&mut host, 1, &page(session, 5, 7, true, &[create(1)]));
    assert_eq!(reply_entries(&host, 1), 1);
    tick(&mut host);

    let responses = drain(&mut host, 1);
    assert_eq!(replies(&responses), [(5, RESPONSE_BATCH)]);
    assert_eq!(batch_reply(&responses, 5), (7, true));
    assert_eq!(entities(&mut host, session).len(), 1);
}

#[test]
fn pipelined_pages_apply_once_at_the_final_page_with_cross_page_aliases() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);

    send(&mut host, 1, &page(session, 0, 3, false, &[create(1)]));
    send(
        &mut host,
        1,
        &page(session, 0, 3, false, &[create(2), place(2, 1)]),
    );
    assert!(
        drain(&mut host, 1).is_empty(),
        "non-final pages produce no output"
    );
    assert_eq!(reply_entries(&host, 1), 1, "one reply for the whole batch");
    assert_eq!(host.connections.states[&1].batches.open_len(), 1);
    for _ in 0..3 {
        tick(&mut host);
    }
    assert!(
        entities(&mut host, session).is_empty(),
        "no page is observable before the final page"
    );
    assert!(replies(&drain(&mut host, 1)).is_empty());

    send(
        &mut host,
        1,
        &page(session, 9, 3, true, &[create(3), place(3, 1)]),
    );
    tick(&mut host);
    let responses = drain(&mut host, 1);
    assert_eq!(replies(&responses), [(9, RESPONSE_BATCH)]);
    assert_eq!(batch_reply(&responses, 9), (3, true));
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);
    assert_eq!(reply_entries(&host, 1), 0);

    // Aliases from pages one and two resolve in pages two and three.
    let world = host.session_mut(session).unwrap();
    let roots: Vec<_> = world.world().entity_children(None).collect();
    assert_eq!(roots.len(), 1);
    assert_eq!(world.world().entity_children(Some(roots[0])).count(), 2);
}

#[test]
fn interleaved_identities_on_one_connection_assemble_separately() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);

    send(&mut host, 1, &page(session, 0, 1, false, &[create(1)]));
    send(&mut host, 1, &page(session, 0, 2, false, &[create(1)]));
    send(&mut host, 1, &page(session, 11, 2, true, &[create(2)]));
    send(&mut host, 1, &page(session, 0, 1, false, &[create(2)]));
    send(&mut host, 1, &page(session, 12, 1, true, &[create(3)]));
    tick(&mut host);

    let responses = drain(&mut host, 1);
    assert_eq!(batch_reply(&responses, 11), (2, true));
    assert_eq!(batch_reply(&responses, 12), (1, true));
    assert_eq!(entities(&mut host, session).len(), 5);
}

#[test]
fn equal_identities_on_two_connections_do_not_interfere() {
    let mut host = Host::<Platform>::new().unwrap();
    let first = connect(&mut host, 1);
    let second = connect(&mut host, 2);

    send(&mut host, 1, &page(first, 0, 4, false, &[create(1)]));
    send(
        &mut host,
        2,
        &page(second, 0, 4, false, &[create(1), create(2)]),
    );
    send(&mut host, 1, &page(first, 21, 4, true, &[]));
    send(&mut host, 2, &page(second, 22, 4, true, &[create(3)]));
    tick(&mut host);

    assert_eq!(batch_reply(&drain(&mut host, 1), 21), (4, true));
    assert_eq!(batch_reply(&drain(&mut host, 2), 22), (4, true));
    assert_eq!(entities(&mut host, first).len(), 1);
    assert_eq!(entities(&mut host, second).len(), 3);
}

#[test]
fn identities_are_reusable_after_completion_across_the_counter_wrap() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);

    for (request, batch) in [(31, u32::MAX - 1), (32, u32::MAX), (33, 0), (34, u32::MAX)] {
        send(&mut host, 1, &page(session, 0, batch, false, &[create(1)]));
        send(
            &mut host,
            1,
            &page(session, request, batch, true, &[create(2)]),
        );
        tick(&mut host);
        assert_eq!(
            batch_reply(&drain(&mut host, 1), request),
            (batch.into(), true)
        );
    }
    assert_eq!(entities(&mut host, session).len(), 8);
    assert_eq!(host.connections.states[&1].batches.open_len(), 0);
}

#[test]
fn pages_for_another_session_fail_only_their_batch() {
    let mut host = Host::<Platform>::new().unwrap();
    let first = connect(&mut host, 1);
    let HostResponseBody::Created {
        reference,
        ..
    } = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: ipp_protocol::host::WorldCreateOptions::new(Vec::new()),
            temporary: true,
        },
    )
    else {
        panic!("World not created")
    };
    let second = open_session(&mut host, 1, reference);

    send(&mut host, 1, &page(first, 0, 5, false, &[create(1)]));
    send(&mut host, 1, &page(second, 0, 5, false, &[create(2)]));
    send(&mut host, 1, &page(second, 0, 6, false, &[create(1)]));
    send(&mut host, 1, &page(first, 41, 5, true, &[create(3)]));
    send(&mut host, 1, &page(second, 42, 6, true, &[]));
    tick(&mut host);

    let responses = drain(&mut host, 1);
    assert!(error_message(&responses, 41).contains("different World sessions"));
    assert_eq!(batch_reply(&responses, 42), (6, true));
    assert!(entities(&mut host, first).is_empty());
    assert_eq!(entities(&mut host, second).len(), 1);
}

#[test]
fn a_failure_on_the_first_pipelined_page_answers_once_and_applies_nothing() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    let peer = connect(&mut host, 2);
    let command = bulky(120_000);
    let bulky_page = page(session, 0, 1, false, std::slice::from_ref(&command));

    // Batch 1 fills the connection's buffered budget without completing.
    send(&mut host, 1, &bulky_page);
    let charge = host.connections.states[&1].batches.buffered_bytes();
    while host.connections.states[&1].batches.buffered_bytes() + charge <= MAX_BUFFERED_BATCH_BYTES
    {
        send(&mut host, 1, &bulky_page);
    }
    assert!(
        host.connections.states[&1].batches.buffered_bytes() + charge > MAX_BUFFERED_BATCH_BYTES
    );
    // Batch 2 fails on its first page; its later pages were already in flight.
    send(
        &mut host,
        1,
        &page(session, 0, 2, false, std::slice::from_ref(&command)),
    );
    send(&mut host, 1, &page(session, 0, 2, false, &[create(1)]));
    send(&mut host, 1, &page(session, 51, 2, true, &[create(2)]));
    // An equal identity on another connection has its own budget.
    send(&mut host, 2, &page(peer, 0, 2, false, &[create(1)]));
    send(
        &mut host,
        2,
        &page(peer, 52, 2, true, std::slice::from_ref(&command)),
    );
    tick(&mut host);

    let responses = drain(&mut host, 1);
    assert_eq!(replies(&responses), [(51, RESPONSE_ERROR)]);
    assert!(error_message(&responses, 51).contains("buffered batch budget"));
    assert!(entities(&mut host, session).is_empty());
    assert_eq!(host.connections.states[&1].batches.open_len(), 1);
    assert_eq!(batch_reply(&drain(&mut host, 2), 52), (2, true));

    // Alias 1 of the budgeted batch is created by its final page, after the
    // metadata commands that name it, so application stops at its first command.
    send(&mut host, 1, &page(session, 53, 1, true, &[create(1)]));
    tick(&mut host);
    assert_eq!(batch_reply(&drain(&mut host, 1), 53), (1, false));
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);
}

#[test]
fn an_open_batch_counts_once_against_the_pending_request_limit() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    send(&mut host, 1, &page(session, 0, 1, false, &[create(1)]));
    for request in 1..MAX_PENDING as u64 {
        send(
            &mut host,
            1,
            &page(session, 100 + request, 1000 + request as u32, true, &[]),
        );
    }
    assert_eq!(reply_entries(&host, 1), MAX_PENDING);

    send(&mut host, 1, &page(session, 0, 1, false, &[create(2)]));
    assert!(
        host.receive_connection(1, &page(session, 0, 2, false, &[create(1)]))
            .is_err(),
        "opening another batch is ordinary admission"
    );
}

#[test]
fn leaked_batches_refuse_only_their_own_connection_with_the_cause() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    let peer = connect(&mut host, 2);
    let step = BATCH_DEADLINE * 3 / 4;

    // A client that never sends final pages: every batch expires to a failed
    // marker that keeps its reply reserved until its final page or close.
    for batch in 0..MAX_PENDING as u32 {
        send(&mut host, 1, &page(session, 0, batch, false, &[create(1)]));
    }
    host.maintain_connections(step);
    host.maintain_connections(step * 3);
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);
    assert_eq!(reply_entries(&host, 1), MAX_PENDING);

    let refusal = host
        .receive_connection(
            1,
            &page(session, 0, MAX_PENDING as u32, false, &[create(1)]),
        )
        .unwrap_err();
    assert!(refusal.contains("congestion"), "{refusal}");
    assert!(
        refusal.contains(&format!("{MAX_PENDING} held by open or failed batches")),
        "{refusal}"
    );
    // Transports close a connection whose ingress failed.
    assert!(host.close_connection(1));

    send(&mut host, 2, &page(peer, 0, 1, false, &[create(1)]));
    send(&mut host, 2, &page(peer, 71, 1, true, &[create(2)]));
    tick(&mut host);
    assert_eq!(batch_reply(&drain(&mut host, 2), 71), (1, true));
    assert_eq!(entities(&mut host, peer).len(), 2);
}

#[test]
fn a_batch_without_its_final_page_fails_loudly_at_the_progress_deadline() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    let step = BATCH_DEADLINE * 3 / 4;

    send(&mut host, 1, &page(session, 0, 8, false, &[create(1)]));
    host.maintain_connections(step);
    send(&mut host, 1, &page(session, 0, 8, false, &[create(2)]));
    host.maintain_connections(step * 2);
    assert!(
        drain(&mut host, 1).is_empty(),
        "each accepted page restarts the deadline"
    );
    host.maintain_connections(step + BATCH_DEADLINE);

    let responses = drain(&mut host, 1);
    let aborted = responses
        .iter()
        .find(|response| response[24] == RESPONSE_BATCH_ABORTED)
        .expect("deadline notice");
    assert_eq!(u64::from_le_bytes(aborted[8..16].try_into().unwrap()), 0);
    assert_eq!(u64::from_le_bytes(aborted[25..33].try_into().unwrap()), 8);
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);

    // Pages already in flight never start a new, partial batch.
    send(&mut host, 1, &page(session, 0, 8, false, &[create(3)]));
    send(&mut host, 1, &page(session, 61, 8, true, &[create(4)]));
    tick(&mut host);
    let responses = drain(&mut host, 1);
    assert_eq!(replies(&responses), [(61, RESPONSE_ERROR)]);
    assert!(error_message(&responses, 61).contains("received no page"));
    assert!(entities(&mut host, session).is_empty());
    assert_eq!(reply_entries(&host, 1), 0);
}

#[test]
fn a_slow_batch_waiting_behind_earlier_branch_input_is_not_cut_off() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    let HostResponseBody::Created {
        reference,
        ..
    } = control(
        &mut host,
        1,
        HostRequestBody::CreateWorld {
            options: ipp_protocol::host::WorldCreateOptions::new(Vec::new()),
            temporary: true,
        },
    )
    else {
        panic!("World not created")
    };
    let branch = open_session(&mut host, 1, reference);
    let step = BATCH_DEADLINE * 3 / 4;

    send(&mut host, 1, &page(session, 0, 1, false, &[create(1)]));

    // Earlier input of another branch fills the shared connection's admission,
    // so the Host stops reading it and the batch's next page waits in transport.
    for request in 1..=(MAX_PENDING * 3 / 4) as u64 {
        send(
            &mut host,
            1,
            &page(branch, 100 + request, 100 + request as u32, true, &[]),
        );
    }
    assert!(!host.connection_accepts_input(1));
    for turn in 1..=4 {
        host.maintain_connections(step * turn);
    }
    assert!(
        drain(&mut host, 1).is_empty(),
        "throttled time does not consume the deadline"
    );

    // The branch input completes and the Host reads the connection again; the
    // batch keeps progressing more slowly than one deadline per page.
    tick(&mut host);
    assert_eq!(replies(&drain(&mut host, 1)).len(), MAX_PENDING * 3 / 4);
    assert!(host.connection_accepts_input(1));
    host.maintain_connections(step * 5);
    send(&mut host, 1, &page(session, 0, 1, false, &[create(2)]));
    host.maintain_connections(step * 6);
    send(&mut host, 1, &page(session, 9, 1, true, &[create(3)]));
    tick(&mut host);
    assert_eq!(batch_reply(&drain(&mut host, 1), 9), (1, true));
    assert_eq!(entities(&mut host, session).len(), 3);

    // Once read, a batch that stops sending pages still fails at the deadline.
    send(&mut host, 1, &page(session, 0, 2, false, &[create(1)]));
    host.maintain_connections(step * 6 + BATCH_DEADLINE);
    let responses = drain(&mut host, 1);
    assert!(
        responses
            .iter()
            .any(|response| response[24] == RESPONSE_BATCH_ABORTED)
    );
    assert_eq!(
        entities(&mut host, session).len(),
        3,
        "applied effects remain"
    );
}

#[test]
fn detaching_a_session_or_closing_its_connection_clears_open_batches() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    send(&mut host, 1, &page(session, 0, 1, false, &[create(1)]));
    assert_eq!(
        control(
            &mut host,
            1,
            HostRequestBody::DetachWorld {
                session,
            },
        ),
        HostResponseBody::Complete
    );
    assert_eq!(host.connections.states[&1].batches.open_len(), 0);
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);

    let session = connect(&mut host, 2);
    send(&mut host, 2, &page(session, 0, 1, false, &[create(1)]));
    assert!(host.close_connection(2));
    assert!(!host.connections.states.contains_key(&2));
}

#[test]
fn a_session_without_a_connection_accepts_only_complete_batches() {
    let mut host = Host::<Platform>::new().unwrap();
    host.open_session(1, &[]).unwrap();
    let mut session = host.session_mut(1).unwrap();
    session.receive(&ipp_protocol::HELLO).unwrap();
    session.take_response().unwrap();

    assert!(
        session
            .receive(&page(1, 0, 1, false, &[create(1)]))
            .is_err()
    );
    session.receive(&page(1, 3, 1, true, &[create(1)])).unwrap();
    drop(session);
    host.tick(0.0).unwrap();
    assert_eq!(host.session_mut(1).unwrap().world().entities().len(), 1);
}

/// Named entity `alias` with a symbolic id, so trees compare without identities.
fn named(alias: u32) -> Vec<u8> {
    let name = format!("entity-{alias}");
    let mut bytes = vec![COMMAND_CREATE];
    bytes.extend(alias.to_le_bytes());
    bytes.push(1);
    bytes.extend((name.len() as u32).to_le_bytes());
    bytes.extend(name.as_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.push(0);
    bytes
}

#[test]
fn pages_decoded_on_arrival_apply_exactly_like_one_page() {
    let mut host = Host::<Platform>::new().unwrap();
    let whole = connect(&mut host, 1);
    let paged = connect(&mut host, 2);
    let host_decoded = connect(&mut host, 3);
    let commands: Vec<_> = (1..=40)
        .flat_map(|alias| {
            let mut commands = vec![named(alias)];
            if alias > 1 {
                commands.push(place(alias, alias / 2));
            }
            commands
        })
        .collect();

    send(&mut host, 1, &page(whole, 1, 1, true, &commands));
    let pages: Vec<_> = commands.chunks(13).collect();
    for (index, commands) in pages.iter().enumerate() {
        let last = index + 1 == pages.len();
        let request = if last {
            2
        } else {
            0
        };
        send_decoded(&mut host, 2, &page(paged, request, 1, last, commands));
        send(
            &mut host,
            3,
            &page(host_decoded, request, 1, last, commands),
        );
        if !last {
            tick(&mut host);
            assert!(
                entities(&mut host, paged).is_empty()
                    && entities(&mut host, host_decoded).is_empty(),
                "a decoded page applied before its final page"
            );
        }
    }
    tick(&mut host);

    assert_eq!(batch_reply(&drain(&mut host, 1), 1), (1, true));
    assert_eq!(replies(&drain(&mut host, 2)), [(2, RESPONSE_BATCH)]);
    assert_eq!(replies(&drain(&mut host, 3)), [(2, RESPONSE_BATCH)]);
    let expected = tree(&mut host, whole);
    assert_eq!(expected.len(), 40);
    assert_eq!(tree(&mut host, paged), expected);
    assert_eq!(tree(&mut host, host_decoded), expected);
}

#[test]
fn a_malformed_non_final_page_fails_its_batch_once_on_the_final_page() {
    for decoded_by_transport in [false, true] {
        let mut host = Host::<Platform>::new().unwrap();
        let session = connect(&mut host, 1);
        let deliver = |host: &mut Host<Platform>, bytes: &[u8]| {
            if decoded_by_transport {
                send_decoded(host, 1, bytes);
            } else {
                send(host, 1, bytes);
            }
        };

        deliver(&mut host, &page(session, 0, 4, false, &[create(1)]));
        deliver(
            &mut host,
            &page(session, 0, 4, false, &[create(2), unknown()]),
        );
        deliver(&mut host, &page(session, 0, 4, false, &[create(3)]));
        tick(&mut host);
        assert!(
            replies(&drain(&mut host, 1)).is_empty(),
            "a page was answered"
        );
        assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);

        deliver(&mut host, &page(session, 81, 4, true, &[create(4)]));
        tick(&mut host);
        let responses = drain(&mut host, 1);
        assert_eq!(replies(&responses), [(81, RESPONSE_ERROR)]);
        assert_eq!(
            error_message(&responses, 81),
            "Batch 4 page 2 could not be decoded: Unsupported(250)"
        );
        assert!(entities(&mut host, session).is_empty());
        assert_eq!(reply_entries(&host, 1), 0);

        // The connection stays usable, and so does the identity.
        deliver(&mut host, &page(session, 82, 4, true, &[create(1)]));
        tick(&mut host);
        assert_eq!(batch_reply(&drain(&mut host, 1), 82), (4, true));
        assert_eq!(entities(&mut host, session).len(), 1);
    }
}

#[test]
fn a_malformed_single_page_batch_is_rejected_without_failing_its_connection() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    send_decoded(
        &mut host,
        1,
        &page(session, 91, 2, true, &[create(1), unknown()]),
    );
    tick(&mut host);
    let responses = drain(&mut host, 1);
    assert_eq!(
        error_message(&responses, 91),
        "Batch 2 page 1 could not be decoded: Unsupported(250)"
    );
    assert!(entities(&mut host, session).is_empty());
}

#[test]
fn buffered_pages_charge_their_decoded_commands_not_their_encoding() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    let slot = std::mem::size_of::<ipp_core::Command>();
    // Each kept page also holds its own vector header.
    let header = std::mem::size_of::<Vec<ipp_core::Command>>();

    // Payload-free commands: the charge is exactly their slots.
    let deletes = vec![delete(); 100];
    send_decoded(&mut host, 1, &page(session, 0, 1, false, &deletes));
    assert_eq!(
        host.connections.states[&1].batches.buffered_bytes(),
        100 * slot + header
    );

    // Owned payloads are charged at their capacity beyond the slots.
    send_decoded(&mut host, 1, &page(session, 0, 2, false, &[named(7)]));
    let name = "entity-7".len();
    assert_eq!(
        host.connections.states[&1].batches.buffered_bytes(),
        101 * slot + name + 2 * header
    );

    // Full pages of six-byte deletes encode to about 6 KiB but retain 1024
    // slots each, so the budget runs out long before its size in encoded pages.
    let page_commands = vec![delete(); ipp_protocol::COMMAND_PAGE_COMMANDS];
    let encoded = page(session, 0, 3, false, &page_commands).len();
    let mut sent = 0;
    while host.connections.states[&1].batches.buffered_bytes()
        + ipp_protocol::COMMAND_PAGE_COMMANDS * slot
        + header
        <= MAX_BUFFERED_BATCH_BYTES
    {
        send_decoded(&mut host, 1, &page(session, 0, 3, false, &page_commands));
        sent += 1;
        assert!(sent < 1000, "the budget never filled");
    }
    assert!(sent * encoded < MAX_BUFFERED_BATCH_BYTES / 8);
    send_decoded(&mut host, 1, &page(session, 0, 3, false, &page_commands));
    send_decoded(&mut host, 1, &page(session, 93, 3, true, &[]));
    tick(&mut host);
    let responses = drain(&mut host, 1);
    assert!(error_message(&responses, 93).contains("buffered batch budget"));
    assert!(host.connections.states[&1].batches.buffered_bytes() <= MAX_BUFFERED_BATCH_BYTES);

    // Completing the other batches releases every charge.
    send_decoded(&mut host, 1, &page(session, 94, 1, true, &[]));
    send_decoded(&mut host, 1, &page(session, 95, 2, true, &[]));
    tick(&mut host);
    drain(&mut host, 1);
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);
}

#[test]
fn a_whole_batch_reports_every_alias_up_to_what_one_outcome_carries() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    let page_commands = ipp_protocol::COMMAND_PAGE_COMMANDS as u32;
    let creates = |page: u32| -> Vec<Vec<u8>> {
        (0..page_commands)
            .map(|index| create(page * page_commands + index + 1))
            .collect()
    };

    // Five full pages of creations answer once with all their aliases, which
    // a per-page outcome never had to carry.
    for page_index in 0..5 {
        send_decoded(
            &mut host,
            1,
            &page(session, 0, 1, false, &creates(page_index)),
        );
    }
    send_decoded(&mut host, 1, &page(session, 101, 1, true, &[]));
    tick(&mut host);
    assert_eq!(batch_reply(&drain(&mut host, 1), 101), (1, true));
    assert_eq!(
        entities(&mut host, session).len(),
        5 * page_commands as usize
    );

    // A batch defining more aliases than one outcome reports fails as a whole.
    let pages = ipp_protocol::BATCH_OUTCOME_ALIASES as u32 / page_commands + 1;
    for page_index in 0..pages {
        send_decoded(
            &mut host,
            1,
            &page(session, 0, 2, false, &creates(page_index)),
        );
    }
    send_decoded(&mut host, 1, &page(session, 102, 2, true, &[]));
    tick(&mut host);
    let responses = drain(&mut host, 1);
    assert!(
        error_message(&responses, 102).contains("one batch outcome cannot report"),
        "{}",
        error_message(&responses, 102)
    );
    assert_eq!(
        entities(&mut host, session).len(),
        5 * page_commands as usize
    );
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);
}

#[test]
fn a_batch_whose_symbol_reports_one_outcome_cannot_carry_is_refused_before_it_applies() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    let limit = ipp_protocol::BATCH_OUTCOME_ALIASES;
    let named = |alias: u32, symbol: &str| {
        let mut bytes = vec![COMMAND_CREATE];
        bytes.extend(alias.to_le_bytes());
        bytes.push(1);
        bytes.extend((symbol.len() as u32).to_le_bytes());
        bytes.extend(symbol.as_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.push(0);
        bytes
    };
    let delete_symbol = |symbol: &str| {
        let mut bytes = vec![COMMAND_DELETE, REF_SYMBOL];
        bytes.extend((symbol.len() as u32).to_le_bytes());
        bytes.extend(symbol.as_bytes());
        bytes
    };
    let submit = |host: &mut Host<Platform>, request: u64, batch: u32, commands: &[Vec<u8>]| {
        let mut pages: Vec<&[Vec<u8>]> = Vec::new();
        let mut first = 0;
        let mut bytes = 0;
        for (index, command) in commands.iter().enumerate() {
            if index - first == ipp_protocol::COMMAND_PAGE_COMMANDS
                || bytes + command.len() > ipp_protocol::COMMAND_PAGE_BYTES / 2
            {
                pages.push(&commands[first..index]);
                first = index;
                bytes = 0;
            }
            bytes += command.len();
        }
        pages.push(&commands[first..]);
        for chunk in pages {
            send_decoded(host, 1, &page(session, 0, batch, false, chunk));
        }
        send_decoded(host, 1, &page(session, request, batch, true, &[]));
        tick(host);
        drain(host, 1)
    };
    let live = ["a", "b", "c", "d"];
    let creates: Vec<_> = (1..)
        .zip(live)
        .map(|(alias, symbol)| named(alias, symbol))
        .collect();
    assert_eq!(
        batch_reply(&submit(&mut host, 101, 1, &creates), 101),
        (1, true)
    );
    let deletes = |extra: Vec<String>| -> Vec<Vec<u8>> {
        live.iter()
            .map(|symbol| delete_symbol(symbol))
            .chain(extra.iter().map(|symbol| delete_symbol(symbol)))
            .collect()
    };

    // One distinct symbol more than an outcome reports refuses the whole batch
    // with the outcome-limit reason, before its leading deletions apply.
    let absent = |count: usize| (0..count).map(|index| format!("x{index}")).collect();
    let responses = submit(&mut host, 102, 2, &deletes(absent(limit + 1 - live.len())));
    assert!(
        error_message(&responses, 102).contains("one batch outcome cannot report"),
        "{}",
        error_message(&responses, 102)
    );
    assert_eq!(entities(&mut host, session).len(), live.len());

    // At the limit the batch is admitted and its reports fit one message: every
    // symbol resolves and the batch applies whole.
    let symbols: Vec<String> = absent(limit - live.len());
    let named_creates: Vec<_> = (1..)
        .zip(&symbols)
        .map(|(alias, symbol)| named(alias, symbol))
        .collect();
    assert_eq!(
        batch_reply(&submit(&mut host, 103, 3, &named_creates), 103),
        (3, true)
    );
    assert_eq!(entities(&mut host, session).len(), limit);
    let responses = submit(&mut host, 107, 7, &deletes(symbols));
    assert_eq!(batch_reply(&responses, 107), (7, true));
    assert!(
        responses
            .iter()
            .all(|response| response.len() < ipp_protocol::MAX_MESSAGE_BYTES)
    );
    assert!(entities(&mut host, session).is_empty());
    let creates: Vec<_> = (1..)
        .zip(live)
        .map(|(alias, symbol)| named(alias, symbol))
        .collect();
    assert_eq!(
        batch_reply(&submit(&mut host, 108, 8, &creates), 108),
        (8, true)
    );

    // A few live symbols whose text exceeds one message are refused the same
    // way instead of applying and then failing the connection at encoding.
    let long: Vec<String> = (0..20)
        .map(|index| format!("{index}{}", "y".repeat(60_000)))
        .collect();
    let creates: Vec<_> = (1..)
        .zip(&long)
        .map(|(alias, symbol)| named(alias, symbol))
        .collect();
    assert_eq!(
        batch_reply(&submit(&mut host, 104, 4, &creates), 104),
        (4, true)
    );
    let responses = submit(&mut host, 105, 5, &deletes(long.clone()));
    assert_eq!(batch_reply(&responses, 105), (5, false));
    assert_eq!(entities(&mut host, session).len(), live.len() + long.len());

    // The connection keeps serving: the short symbolic deletions alone apply.
    let responses = submit(&mut host, 106, 6, &deletes(Vec::new()));
    assert_eq!(batch_reply(&responses, 106), (6, true));
    assert_eq!(entities(&mut host, session).len(), long.len());
}

#[test]
fn the_largest_admissible_batch_is_answered_in_one_reply_and_one_alias_more_is_refused() {
    let mut host = Host::<Platform>::new().unwrap();
    let session = connect(&mut host, 1);
    let limit = ipp_protocol::BATCH_OUTCOME_ALIASES as u32;

    // As many commands as the maintained Blender stress import, with every
    // alias one outcome reports: creations first, then placements below alias 1.
    let commands = 40_064;
    let batch = |aliases: u32| -> Vec<Vec<u8>> {
        (1..=aliases)
            .map(create)
            .chain((2..).map(|child| place(child, 1)))
            .take(commands)
            .collect()
    };
    let submit = |host: &mut Host<Platform>, request: u64, batch: u32, commands: &[Vec<u8>]| {
        for chunk in commands.chunks(ipp_protocol::COMMAND_PAGE_COMMANDS) {
            send_decoded(host, 1, &page(session, 0, batch, false, chunk));
        }
        send_decoded(host, 1, &page(session, request, batch, true, &[]));
        tick(host);
        drain(host, 1)
    };

    // One alias more than an outcome reports is refused with the outcome-limit
    // reason before anything applies.
    let responses = submit(&mut host, 101, 1, &batch(limit + 1));
    assert!(
        error_message(&responses, 101).contains("one batch outcome cannot report"),
        "{}",
        error_message(&responses, 101)
    );
    assert!(entities(&mut host, session).is_empty());
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);

    // The same connection then applies the largest admissible batch, answered
    // in one reply of one encoded alias report per creation.
    let responses = submit(&mut host, 102, 2, &batch(limit));
    assert_eq!(batch_reply(&responses, 102), (2, true));
    assert_eq!(entities(&mut host, session).len(), limit as usize);
    let reply = responses
        .iter()
        .find(|response| u64::from_le_bytes(response[8..16].try_into().unwrap()) == 102)
        .unwrap();
    let alias_reports = limit as usize * crate::attachment_receipts::ALIAS_WIRE_BYTES;
    assert!(reply.len() > alias_reports && reply.len() <= alias_reports + 128);
    assert!(reply.len() < ipp_protocol::MAX_MESSAGE_BYTES);
}

#[test]
fn the_largest_admissible_adopting_reconnect_applies_whole_and_one_report_more_is_refused() {
    use crate::attachment_receipts::ALIAS_WIRE_BYTES;
    use ipp_core::ComponentValue;
    use ipp_protocol::attachment_receipts::ADOPTED_EFFECT_BYTES;

    const COMMAND_INSERT: u8 = 4;

    let mut host = Host::<Platform>::new().unwrap();
    let systems = [
        ipp_core::systems::constraints::ConstraintSystem::ID,
        ipp_core::systems::hierarchy::HierarchySystem::ID,
    ];
    let session = connect_with(&mut host, 1, systems.map(|id| id.0.into()).to_vec());

    // A React-style mount and reconnect: every declared entity is created with
    // its symbolic id and each of its two components inserted, all adopting;
    // `rewrites` further adopting inserts write alias 1's Scalar again, as a
    // recommit does.
    let adopting_create = |alias: u32| {
        let name = format!("entity-{alias}");
        let mut bytes = vec![COMMAND_CREATE];
        bytes.extend(alias.to_le_bytes());
        bytes.push(1);
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.push(1);
        bytes
    };
    let adopting_insert = |alias: u32, component: u16| {
        let mut bytes = vec![COMMAND_INSERT, REF_ALIAS];
        bytes.extend(alias.to_le_bytes());
        bytes.extend(component.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.push(1);
        bytes
    };
    let entities_limit = ipp_protocol::BATCH_OUTCOME_ALIASES as u32;
    let reconnect = |rewrites: u32| -> Vec<Vec<u8>> {
        (1..=entities_limit)
            .flat_map(|alias| {
                [
                    adopting_create(alias),
                    adopting_insert(alias, ComponentValue::TRANSFORM),
                    adopting_insert(alias, ComponentValue::SCALAR),
                ]
            })
            .chain((0..rewrites).map(|_| adopting_insert(1, ComponentValue::SCALAR)))
            .collect()
    };
    let submit = |host: &mut Host<Platform>, request: u64, batch: u32, commands: &[Vec<u8>]| {
        for chunk in commands.chunks(ipp_protocol::COMMAND_PAGE_COMMANDS) {
            send_decoded(host, 1, &page(session, 0, batch, false, chunk));
        }
        send_decoded(host, 1, &page(session, request, batch, true, &[]));
        tick(host);
        drain(host, 1)
    };
    let reply = |responses: &[Vec<u8>], request: u64| {
        responses
            .iter()
            .find(|response| u64::from_le_bytes(response[8..16].try_into().unwrap()) == request)
            .unwrap()
            .clone()
    };

    // Every alias and every possible adoption is admitted with the batch at its
    // encoded size: 12 bytes per alias and 5 per adopting operation, beside the
    // 256 bytes each batch reply reserves when its final page arrives. The
    // largest admissible reconnect fills the 1 MiB reply with rewrites after the
    // alias limit's entities.
    let per_entity = ALIAS_WIRE_BYTES + 3 * ADOPTED_EFFECT_BYTES;
    let entity_bytes = 256 + entities_limit as usize * per_entity;
    let rewrites = ((ipp_protocol::MAX_MESSAGE_BYTES - entity_bytes) / ADOPTED_EFFECT_BYTES) as u32;
    assert_eq!((entities_limit, rewrites), (32_768, 32_716));

    // The first mount creates and inserts, so it reports no adoption.
    let responses = submit(&mut host, 101, 1, &reconnect(0));
    assert_eq!(batch_reply(&responses, 101), (1, true));
    assert_eq!(entities(&mut host, session).len(), entities_limit as usize);

    // One possible adoption report more than the reply holds refuses the batch
    // at its first operation with the capacity reason, before anything applies:
    // its leading adopting creations would otherwise have bound their entities.
    let responses = submit(&mut host, 102, 2, &reconnect(rewrites + 1));
    assert_eq!(batch_reply(&responses, 102), (2, false));
    let refused = reply(&responses, 102);
    assert_eq!(u32::from_le_bytes(refused[44..48].try_into().unwrap()), 0);
    let length = u32::from_le_bytes(refused[48..52].try_into().unwrap()) as usize;
    assert_eq!(&refused[52..52 + length], b"Capacity");
    assert_eq!(&refused[52 + length..52 + length + 8], [0; 8]);
    assert_eq!(entities(&mut host, session).len(), entities_limit as usize);
    assert_eq!(host.connections.states[&1].batches.buffered_bytes(), 0);

    // The same connection then applies the largest admissible reconnect whole:
    // every one of its 131,020 operations adopts, answered in one reply of one
    // alias report per entity and one five-byte adoption report per operation.
    let responses = submit(&mut host, 103, 3, &reconnect(rewrites));
    assert_eq!(batch_reply(&responses, 103), (3, true));
    assert_eq!(entities(&mut host, session).len(), entities_limit as usize);
    let answered = reply(&responses, 103);
    let aliases = 46 + entities_limit as usize * ALIAS_WIRE_BYTES;
    let symbols = u32::from_le_bytes(answered[aliases..aliases + 4].try_into().unwrap());
    assert_eq!(symbols, 0);
    let effects = u32::from_le_bytes(answered[aliases + 4..aliases + 8].try_into().unwrap());
    assert_eq!(effects, 3 * entities_limit + rewrites);
    assert_eq!(
        answered.len(),
        aliases + 8 + effects as usize * ADOPTED_EFFECT_BYTES
    );
    assert!(answered.len() <= ipp_protocol::MAX_MESSAGE_BYTES);
}
