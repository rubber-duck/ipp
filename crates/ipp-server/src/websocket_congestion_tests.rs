//! Shared Host with real independent TCP transports; no simulated ingress channel.
use super::tests::{TEST_TIMEOUT, socket_pair};
use super::*;
use std::net::Shutdown;
use std::thread::JoinHandle;

struct Peers {
    sockets: Vec<tungstenite::WebSocket<TcpStream>>,
    stop: Arc<AtomicBool>,
    host: Option<JoinHandle<()>>,
}

impl Peers {
    fn start() -> Self {
        let pairs: Vec<_> = (0..3).map(|_| socket_pair()).collect();
        let clients: Vec<_> = pairs
            .iter()
            .map(|(client, _)| client.try_clone().unwrap())
            .collect();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let host = std::thread::spawn(move || {
            let (events, incoming) = mpsc::sync_channel(MAX_CONNECTIONS * 64);
            let transports: Vec<_> = pairs
                .into_iter()
                .enumerate()
                .map(|(index, (_, server))| {
                    let events = events.clone();
                    std::thread::spawn(move || {
                        let _ = connection(server, index as u64 + 1, &events);
                        let _ = events.send(HostConnectionEvent::Closed {
                            id: index as u64 + 1,
                        });
                    })
                })
                .collect();
            let mut host = NativeConnectionHost::new().unwrap();
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
        Self {
            sockets,
            stop,
            host: Some(host),
        }
    }

    fn initialize(&mut self, index: usize, world: Option<ipp_core::WorldId>) -> u64 {
        use ipp_protocol::host::*;
        let connection = index as u64 + 1;
        self.sockets[index]
            .send(Message::Binary(ipp_protocol::bootstrap().to_vec().into()))
            .unwrap();
        let _ = self.read(index);
        let body = world.map_or_else(
            || HostRequestBody::CreateWorld {
                options: Default::default(),
                temporary: false,
            },
            |id| HostRequestBody::AttachWorld(ipp_core::WorldSelector::Id(id)),
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
        let response = decode_host_response(&self.read(index), connection).unwrap();
        let HostResponseBody::Attached {
            session,
            ..
        } = response.body
        else {
            panic!("attachment")
        };
        session
    }

    fn read(&mut self, index: usize) -> Vec<u8> {
        match self.sockets[index].read().unwrap() {
            Message::Binary(bytes) => bytes.to_vec(),
            other => panic!("unexpected {other:?}"),
        }
    }
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
    let noisy = peers.initialize(0, None);
    let healthy = peers.initialize(1, Some(ipp_core::WorldId(1)));
    // A third peer resets after its HTTP upgrade, before IPP bootstrap. This
    // setup failure must remain local while the two attached clients progress.
    peers.sockets[2].get_mut().shutdown(Shutdown::Both).unwrap();
    for request in 1u64..=200 {
        let mut bytes = noisy.to_le_bytes().to_vec();
        bytes.extend_from_slice(&request.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&request.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&0u32.to_le_bytes());
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
