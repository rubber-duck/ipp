//! Shared Host with real independent TCP transports; no simulated ingress channel.
use super::tests::{TEST_TIMEOUT, socket_pair};
use super::*;
use std::net::Shutdown;
use std::sync::Mutex;
use std::thread::JoinHandle;

/// The Host's ordinary output share: its byte budget minus the reserve kept for one
/// maximum-size reply and its copy. Transport allocations can only use this share.
const ORDINARY_OUTPUT_BYTES: usize = 8 * MAX_MESSAGE_BYTES - (2 * MAX_MESSAGE_BYTES + 2048);

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct OutputSample {
    connection: u64,
    completed: u64,
    inflight: usize,
    retained_bytes: usize,
    used_bytes: Option<usize>,
}

struct Peers {
    sockets: Vec<tungstenite::WebSocket<TcpStream>>,
    stop: Arc<AtomicBool>,
    host: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    samples: SyncSender<SyncSender<Vec<OutputSample>>>,
    #[cfg(target_os = "linux")]
    failures: Arc<Mutex<BTreeMap<u64, String>>>,
    #[cfg(target_os = "linux")]
    addresses: Vec<(std::net::SocketAddr, std::net::SocketAddr)>,
}

#[test]
fn physical_output_credit_survives_channel_handoff_until_socket_completion() {
    let mut host = NativeConnectionHost::<NativeHostServices>::new().unwrap();
    let (replies, receiver) = mpsc::sync_channel(1);
    let (failures, _failure_receiver) = mpsc::sync_channel(1);
    let (_sender, ingress) = mpsc::sync_channel(1);
    let completed = Arc::new(AtomicU64::new(0));
    host.receive(HostConnectionEvent::Open {
        id: 1,
        replies,
        failures,
        ingress,
        queued: Arc::new(AtomicUsize::new(0)),
        throttled: Arc::new(AtomicBool::new(false)),
        completed: completed.clone(),
        released: Arc::new(AtomicBool::new(true)),
    });
    host.host
        .receive_connection(1, &ipp_protocol::HELLO)
        .unwrap();
    host.flush();
    let bytes = receiver.try_recv().unwrap();
    assert_eq!(host.outputs[&1].inflight.len(), 1);
    let transport_charge = 2 * (MAX_MESSAGE_BYTES + 1024) + 16384;
    let reply_charge = bytes.capacity() + 1024;
    let remaining = host
        .host
        .reserve_connection_output_bytes(1, ORDINARY_OUTPUT_BYTES - transport_charge - reply_charge)
        .unwrap();
    assert!(host.host.reserve_connection_output_bytes(1, 1).is_err());
    host.flush();
    assert_eq!(host.outputs[&1].inflight.len(), 1);
    assert!(host.host.reserve_connection_output_bytes(1, 1).is_err());
    drop(bytes);
    completed.store(1, Ordering::Release);
    host.flush();
    assert!(host.outputs[&1].inflight.is_empty());
    let freed = host
        .host
        .reserve_connection_output_bytes(1, reply_charge)
        .unwrap();
    assert!(host.host.reserve_connection_output_bytes(1, 1).is_err());
    drop((freed, remaining));
}

impl Peers {
    fn start() -> Self {
        Self::start_with(3, |_| ()).0
    }

    fn start_with<Data: Send + 'static>(
        count: usize,
        fixture: impl FnOnce(&mut NativeConnectionHost<NativeHostServices>) -> Data + Send + 'static,
    ) -> (Self, Data) {
        let pairs: Vec<_> = (0..count).map(|_| socket_pair()).collect();
        #[cfg(target_os = "linux")]
        let addresses = pairs
            .iter()
            .map(|(_, server)| (server.local_addr().unwrap(), server.peer_addr().unwrap()))
            .collect();
        let clients: Vec<_> = pairs
            .iter()
            .map(|(client, _)| client.try_clone().unwrap())
            .collect();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let failures = Arc::new(Mutex::new(BTreeMap::new()));
        let recorded_failures = failures.clone();
        #[cfg(target_os = "linux")]
        let (samples, measurements) = mpsc::sync_channel::<SyncSender<Vec<OutputSample>>>(1);
        let (ready, initialized) = mpsc::sync_channel(1);
        let host = std::thread::spawn(move || {
            let (events, incoming) = mpsc::sync_channel(MAX_CONNECTIONS * 64);
            let transports: Vec<_> = pairs
                .into_iter()
                .enumerate()
                .map(|(index, (_, server))| {
                    let events = events.clone();
                    let failures = recorded_failures.clone();
                    std::thread::spawn(move || {
                        if let Err(reason) = connection(server, index as u64 + 1, &events) {
                            failures.lock().unwrap().insert(index as u64 + 1, reason);
                        }
                        let _ = events.send(HostConnectionEvent::Closed {
                            id: index as u64 + 1,
                        });
                    })
                })
                .collect();
            let mut host = NativeConnectionHost::<NativeHostServices>::new().unwrap();
            ready.send(fixture(&mut host)).unwrap();
            let mut last = Instant::now();
            while !stopping.load(Ordering::Acquire) {
                for event in incoming.try_iter().take(MAX_CONNECTIONS * 64) {
                    host.receive(event);
                }
                let now = Instant::now();
                if now.duration_since(last) >= FRAME_INTERVAL {
                    host.tick(now.duration_since(last).as_secs_f64()).unwrap();
                    last = now;
                }
                host.flush();
                #[cfg(target_os = "linux")]
                if let Ok(reply) = measurements.try_recv() {
                    let values = host
                        .outputs
                        .iter()
                        .map(|(&id, output)| OutputSample {
                            connection: id,
                            completed: output.acknowledged,
                            inflight: output.inflight.len(),
                            retained_bytes: output
                                .inflight
                                .iter()
                                .map(|lease| lease.capacity() + 1024)
                                .sum(),
                            used_bytes: remaining_output_bytes(&host.host, id)
                                .map(|remaining| ORDINARY_OUTPUT_BYTES - remaining),
                        })
                        .collect();
                    let _ = reply.send(values);
                }
                std::thread::sleep(IO_POLL_INTERVAL);
            }
            for transport in transports {
                transport.join().unwrap();
            }
        });
        let sockets = clients
            .into_iter()
            .map(|client| tungstenite::client("ws://127.0.0.1/", client).unwrap().0)
            .collect();
        let data = initialized.recv_timeout(TEST_TIMEOUT).unwrap();
        (
            Self {
                sockets,
                stop,
                host: Some(host),
                #[cfg(target_os = "linux")]
                samples,
                #[cfg(target_os = "linux")]
                failures,
                #[cfg(target_os = "linux")]
                addresses,
            },
            data,
        )
    }

    #[cfg(target_os = "linux")]
    fn sample(&self) -> Vec<OutputSample> {
        let (reply, values) = mpsc::sync_channel(1);
        self.samples.send(reply).unwrap();
        values.recv_timeout(TEST_TIMEOUT).unwrap()
    }

    fn initialize(
        &mut self,
        index: usize,
        world: Option<ipp_protocol::references::WorldReference>,
    ) -> (u64, ipp_protocol::references::WorldReference) {
        use ipp_protocol::host::*;
        let connection = index as u64 + 1;
        self.sockets[index]
            .send(Message::Binary(ipp_protocol::HELLO.to_vec().into()))
            .unwrap();
        let _ = self.read(index);
        let body = world.map_or_else(
            || HostRequestBody::CreateWorld {
                options: ipp_protocol::host::WorldCreateOptions::new(Vec::new()),
                temporary: false,
            },
            HostRequestBody::OpenWorld,
        );
        self.sockets[index]
            .send(Message::Binary(
                encode_host_request(&HostRequest {
                    connection,
                    request_id: 1,
                    body,
                })
                .unwrap()
                .into(),
            ))
            .unwrap();
        let mut response = decode_host_response(&self.read(index), connection).unwrap();
        if let HostResponseBody::Created {
            reference,
            ..
        } = response.body
        {
            self.sockets[index]
                .send(Message::Binary(
                    encode_host_request(&HostRequest {
                        connection,
                        request_id: 2,
                        body: HostRequestBody::OpenWorld(reference),
                    })
                    .unwrap()
                    .into(),
                ))
                .unwrap();
            response = decode_host_response(&self.read(index), connection).unwrap();
        }
        let HostResponseBody::Attached {
            session,
            reference,
            ..
        } = response.body
        else {
            panic!("attachment")
        };
        (session, reference)
    }

    fn read(&mut self, index: usize) -> Vec<u8> {
        match self.sockets[index].read().unwrap() {
            Message::Binary(bytes) => bytes.to_vec(),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[cfg(target_os = "linux")]
fn remaining_output_bytes(host: &Host<NativeHostServices>, connection: u64) -> Option<usize> {
    drop(host.reserve_connection_output_bytes(connection, 0).ok()?);
    let mut lower = 0;
    let mut upper = ORDINARY_OUTPUT_BYTES;
    while lower < upper {
        let candidate = lower + (upper - lower).div_ceil(2);
        match host.reserve_connection_output_bytes(connection, candidate) {
            Ok(lease) => {
                drop(lease);
                lower = candidate;
            }
            Err(_) => upper = candidate - 1,
        }
    }
    Some(lower)
}

impl Drop for Peers {
    fn drop(&mut self) {
        for socket in &mut self.sockets {
            let _ = socket.get_mut().shutdown(Shutdown::Both);
        }
        self.stop.store(true, Ordering::Release);
        if let Some(host) = self.host.take() {
            host.join().unwrap();
        }
    }
}

#[test]
fn noisy_sender_and_stalled_reader_preserve_shared_world_peer_and_ordered_outcomes() {
    let mut peers = Peers::start();
    let (noisy, world) = peers.initialize(0, None);
    let (healthy, _) = peers.initialize(1, Some(world));
    // A third peer resets after its HTTP upgrade, before the IPP hello. This
    // setup failure must remain local while the two attached clients progress.
    peers.sockets[2].get_mut().shutdown(Shutdown::Both).unwrap();
    for request in 1u64..=200 {
        // A single final page of one create command under batch identity `request`.
        let mut bytes = noisy.to_le_bytes().to_vec();
        bytes.extend_from_slice(&request.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&(request as u32).to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(0);
        peers.sockets[0]
            .send(Message::Binary(bytes.into()))
            .unwrap();
    }
    // Do not consume any noisy-peer response until another connection proves
    // progress on the same World. Its ingress spans several admission windows.
    let query = super::tests::inspect(healthy, 500);
    peers.sockets[1]
        .send(Message::Binary(query.into()))
        .unwrap();
    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        let bytes = peers.read(1);
        if u64::from_le_bytes(bytes[8..16].try_into().unwrap()) == 500 {
            assert_eq!(bytes[24], 3);
            break;
        }
        assert!(Instant::now() < deadline);
    }
    let mut outcomes = Vec::new();
    while outcomes.len() < 200 {
        let bytes = peers.read(0);
        if bytes[24] == 1 {
            outcomes.push(u64::from_le_bytes(bytes[8..16].try_into().unwrap()));
        }
        assert!(Instant::now() < deadline);
    }
    assert_eq!(
        outcomes,
        (1..=200).collect::<Vec<_>>(),
        "accepted outcomes must be ordered, unique and complete"
    );
}

#[cfg(target_os = "linux")]
fn socket_queue(addresses: (std::net::SocketAddr, std::net::SocketAddr)) -> Option<(usize, usize)> {
    let encode = |address: std::net::SocketAddr| {
        let std::net::SocketAddr::V4(address) = address else {
            panic!("IPv4 fixture")
        };
        format!(
            "{:08X}:{:04X}",
            u32::from_ne_bytes(address.ip().octets()),
            address.port()
        )
    };
    let local = encode(addresses.0);
    let peer = encode(addresses.1);
    let table = std::fs::read_to_string("/proc/net/tcp").unwrap();
    table.lines().skip(1).find_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields[1] != local || fields[2] != peer {
            return None;
        }
        let (transmit, receive) = fields[4].split_once(':').unwrap();
        Some((
            usize::from_str_radix(transmit, 16).unwrap(),
            usize::from_str_radix(receive, 16).unwrap(),
        ))
    })
}

#[cfg(target_os = "linux")]
fn gui_send(peers: &mut Peers, index: usize, session: u64, request: u64, tag: u8, payload: &[u8]) {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend(request.to_le_bytes());
    bytes.push(tag);
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    ipp_protocol::decode_request(&bytes, session).unwrap();
    peers.sockets[index]
        .send(Message::Binary(bytes.into()))
        .unwrap();
}

#[cfg(target_os = "linux")]
fn gui_reply(peers: &mut Peers, index: usize, request: u64, tag: u8) -> Vec<u8> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        let bytes = peers.read(index);
        if bytes[24] == tag && u64::from_le_bytes(bytes[8..16].try_into().unwrap()) == request {
            return bytes;
        }
        assert_eq!(
            bytes[24],
            4,
            "unexpected reply: {:?}",
            &bytes[..bytes.len().min(80)]
        );
        assert!(Instant::now() < deadline, "GUI reply did not arrive");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn gui_os_stalled_receiver_retains_socket_credit_while_issuer_and_observer_progress() {
    use ipp_core::{Batch, Command, ComponentValue, EntityRef};
    let text = "x".repeat(65_536);
    let seed: std::sync::Arc<str> = text.as_str().into();
    let (mut peers, target) = Peers::start_with(4, move |host| {
        let id = host
            .host
            .runtime_mut()
            .create_world(
                Default::default(),
                &[
                    ipp_core::systems::canvas::CanvasSystem::ID,
                    ipp_core::systems::gui::GuiSystem::ID,
                ],
            )
            .unwrap();
        let mut world = host.host.runtime_mut().world_mut(id).unwrap();
        world
            .enqueue(Batch {
                id: 1,
                operations: vec![
                    Command::Create {
                        alias: 1,
                        metadata: Default::default(),
                        adopt: false,
                    },
                    Command::insert_value(
                        EntityRef::Alias(1),
                        ComponentValue::GuiTextInput(ipp_core::systems::gui::local::GuiTextInput {
                            text: seed,
                            ..Default::default()
                        }),
                    ),
                ],
            })
            .unwrap();
        let entity = world.step(0.0).unwrap().outcomes[0]
            .result
            .as_ref()
            .unwrap()[0]
            .1;
        let incarnation = world
            .component_incarnation(entity, ComponentValue::GUI_TEXT_INPUT)
            .unwrap();
        (world.world_ref(), entity, incarnation)
    });
    let (world, entity, incarnation) = target;
    let reference = ipp_protocol::references::WorldReference::from(world);
    let (issuer, _) = peers.initialize(0, Some(reference));
    let (stalled, _) = peers.initialize(1, Some(reference));
    let (healthy, _) = peers.initialize(2, Some(reference));
    for (index, session) in [(1, stalled), (2, healthy)] {
        let mut payload = vec![0];
        payload.extend(reference.id.to_le_bytes());
        payload.extend(reference.incarnation.to_le_bytes());
        payload.push(0);
        gui_send(&mut peers, index, session, 10, 34, &payload);
        let ack = gui_reply(&mut peers, index, 10, 36);
        assert_eq!(ack[45], 0);
        assert_eq!(ack[62], 0);
    }

    let evidence_directory = std::path::Path::new("../../target/transport-proofs");
    std::fs::create_dir_all(evidence_directory).unwrap();
    let evidence_path = evidence_directory.join(format!(
        "native-gui-stall-{}.txt",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut evidence = std::fs::File::create(&evidence_path).unwrap();
    writeln!(
        evidence,
        "hello={:?} addresses={:?}",
        ipp_protocol::HELLO,
        peers.addresses
    )
    .unwrap();
    let mut ordinal = 0;
    let mut physical_stall = false;
    let mut stable_credit_samples = 0;
    let mut previous: Option<OutputSample> = None;
    let mut applied = 0u32;
    // Each submission echoes the whole text field in its applied effect.
    for submission in 0..192u64 {
        let request = 100 + submission;
        // One final batch page (request 1) with one GuiAction command (24)
        // submitting (7) the text input named by handle (reference 0).
        let mut bytes = issuer.to_le_bytes().to_vec();
        bytes.extend(request.to_le_bytes());
        bytes.push(1);
        bytes.extend((request as u32).to_le_bytes());
        bytes.push(1);
        bytes.extend(1u32.to_le_bytes());
        bytes.push(24);
        bytes.push(0);
        bytes.extend(entity.to_bits().to_le_bytes());
        bytes.extend(ComponentValue::GUI_TEXT_INPUT.to_le_bytes());
        bytes.extend(incarnation.to_le_bytes());
        bytes.push(7);
        ipp_protocol::decode_request(&bytes, issuer).unwrap();
        peers.sockets[0]
            .send(Message::Binary(bytes.into()))
            .unwrap();
        let outcome = gui_reply(&mut peers, 0, request, 1);
        let observed = gui_reply(&mut peers, 2, 0, 36);
        // Batch identity and tick precede the outcome tag.
        assert_eq!(outcome[41], 0, "issuer must remain applied");
        assert_eq!(observed[45], 1);
        let effect = &observed[46..];
        assert_eq!(effect[0], 1);
        let next = u64::from_le_bytes(effect[17..25].try_into().unwrap());
        assert!(next > ordinal);
        ordinal = next;
        let text_start = effect.len() - text.len();
        assert_eq!(&effect[text_start..], text.as_bytes());
        assert_eq!(effect[text_start - 5], 4, "submitted effect kind");
        assert_eq!(
            u32::from_le_bytes(effect[text_start - 4..text_start].try_into().unwrap()) as usize,
            text.len()
        );
        applied += 1;

        let samples = peers.sample();
        let queued = socket_queue(peers.addresses[1]);
        writeln!(
            evidence,
            "applied={applied} kernel={queued:?} output={samples:?}"
        )
        .unwrap();
        if let Some(current) = samples.into_iter().find(|sample| sample.connection == 2) {
            assert!(current.inflight <= 66, "bounded socket/channel handoff");
            if let Some(used) = current.used_bytes {
                assert!(used <= ORDINARY_OUTPUT_BYTES);
                assert!(used >= current.retained_bytes + 2 * (MAX_MESSAGE_BYTES + 1024) + 16384);
            }
            if queued.is_some_and(|(transmit, _)| transmit > 0)
                && current.retained_bytes > MAX_MESSAGE_BYTES
            {
                physical_stall = true;
                if let Some(before) = &previous
                    && before.completed == current.completed
                {
                    assert!(
                        current.retained_bytes >= before.retained_bytes,
                        "credit released without actual socket completion"
                    );
                    stable_credit_samples += 1;
                }
            }
            previous = Some(current);
        }
        if peers.failures.lock().unwrap().contains_key(&2) {
            break;
        }
    }
    assert!(
        physical_stall && stable_credit_samples >= 3,
        "no sustained OS write stall demonstrated; evidence {evidence_path:?}"
    );
    let failures = peers.failures.lock().unwrap().clone();
    writeln!(
        evidence,
        "failures={failures:?} stable_credit_samples={stable_credit_samples}"
    )
    .unwrap();
    assert_eq!(failures.keys().copied().collect::<Vec<_>>(), [2]);
    assert!(failures[&2].contains("congestion"));

    let (replacement, replacement_world) = peers.initialize(3, Some(reference));
    assert_eq!(replacement_world, reference);
    peers.sockets[3]
        .send(Message::Binary(
            super::tests::inspect(replacement, 900).into(),
        ))
        .unwrap();
    assert_eq!(gui_reply(&mut peers, 3, 900, 3)[24], 3);
    peers.sockets[0]
        .send(Message::Binary(super::tests::inspect(issuer, 901).into()))
        .unwrap();
    gui_reply(&mut peers, 0, 901, 3);
    peers.sockets[2]
        .send(Message::Binary(super::tests::inspect(healthy, 902).into()))
        .unwrap();
    gui_reply(&mut peers, 2, 902, 3);
    writeln!(evidence, "replacement_connection=4 healthy_issuer=1 healthy_observer=3 exact_effects={applied} world={reference:?}").unwrap();
}
