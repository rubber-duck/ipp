//! Real TCP coverage for the adapter's scheduling and resumable fragment decoding.

use super::*;
use std::net::Shutdown;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread::JoinHandle;
use tungstenite::protocol::Role;

pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn frame_deadline_subtracts_work_and_wakeup_jitter_without_accumulating_drift() {
    let origin = Instant::now();
    let mut deadline = origin + FRAME_INTERVAL;
    for frame in 1..=600 {
        let jitter = Duration::from_millis(if frame % 2 == 0 {
            3
        } else {
            1
        });
        let work = Duration::from_millis(8);
        let finished = deadline + jitter + work;
        deadline = advance_frame_deadline(deadline, finished);

        assert_eq!(deadline, origin + FRAME_INTERVAL * (frame + 1));
        assert_eq!(
            deadline.duration_since(finished),
            FRAME_INTERVAL - jitter - work
        );
    }
}

#[test]
fn frame_deadline_discards_overrun_backlog_without_adding_sleep() {
    let deadline = Instant::now();
    let finished = deadline + FRAME_INTERVAL * 3 + Duration::from_millis(5);
    let next = advance_frame_deadline(deadline, finished);

    assert_eq!(next, finished);
    // Once work fits the budget again, the next frame waits for its deadline.
    assert_eq!(
        advance_frame_deadline(next, finished + Duration::from_millis(8)),
        finished + FRAME_INTERVAL,
    );
}

#[test]
fn slow_frames_run_at_the_available_rate_without_extra_sleep() {
    let mut deadline = Instant::now();
    for _ in 0..60 {
        let finished = deadline + Duration::from_millis(20);
        deadline = advance_frame_deadline(deadline, finished);

        assert_eq!(deadline, finished);
    }
}

pub(super) fn socket_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    for stream in [&client, &server] {
        stream.set_read_timeout(Some(TEST_TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TEST_TIMEOUT)).unwrap();
        stream.set_nodelay(true).unwrap();
    }
    (client, server)
}

fn masked_fragment(opcode: u8, final_fragment: bool, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() < 126);
    let mask = [0x17, 0x29, 0x53, 0x71];
    let mut bytes = vec![
        opcode
            | if final_fragment {
                0x80
            } else {
                0
            },
        0x80 | payload.len() as u8,
    ];
    bytes.extend_from_slice(&mask);
    bytes.extend(
        payload
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ mask[i % 4]),
    );
    bytes
}

#[test]
fn read_budget_yields_inside_fragmented_message_and_resumes_exact_payload() {
    let (mut client, server) = socket_pair();
    let prefix = b"prefix retained across yield";
    let suffix = b"suffix after empty fragments";
    let mut wire = masked_fragment(2, false, prefix);
    // Empty continuations consume CPU/wire bytes without growing message payload.
    for _ in 0..READ_BUDGET_BYTES / 2 {
        wire.extend_from_slice(&masked_fragment(0, false, &[]));
    }
    wire.extend_from_slice(&masked_fragment(0, true, suffix));
    std::thread::scope(|scope| {
        let producer = scope.spawn(move || client.write_all(&wire));
        let mut socket = tungstenite::WebSocket::from_raw_socket(
            BudgetedStream::new(server),
            Role::Server,
            Some(WebSocketConfig::default().read_buffer_size(16 * 1024)),
        );

        // Blocking TCP here makes each WouldBlock deterministic evidence of the
        // adapter budget rather than a momentarily empty kernel receive buffer.
        let mut yields = 0;
        let message = loop {
            socket.get_mut().begin_iteration();
            match socket.read() {
                Err(Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert_eq!(socket.get_ref().remaining, 0);
                    yields += 1;
                    assert!(yields < 8, "fragment decoding must eventually complete");
                }
                Ok(message) => break message,
                Err(error) => panic!("fragment decode failed: {error}"),
            }
        };
        producer.join().unwrap().unwrap();
        assert!(yields >= 3, "one read must not drain every continuation");
        assert_eq!(
            message,
            Message::Binary([prefix.as_slice(), suffix.as_slice()].concat().into())
        );
    });
}

struct RunningConnection {
    session: u64,
    client: tungstenite::WebSocket<TcpStream>,
    server: Option<JoinHandle<Result<(), String>>>,
}

impl RunningConnection {
    fn start() -> Self {
        let (client, server) = socket_pair();
        let host = std::thread::spawn(move || {
            let (events, incoming) = mpsc::sync_channel(MAX_CONNECTIONS * 64);
            let transport = std::thread::spawn(move || connection(server, 7, &events));
            let mut host = NativeConnectionHost::new()?;
            let mut last = Instant::now();
            while !transport.is_finished() {
                for event in incoming.try_iter().take(MAX_CONNECTIONS * 64) {
                    host.receive(event);
                }
                let now = Instant::now();
                if now.duration_since(last) >= FRAME_INTERVAL {
                    host.tick(now.duration_since(last).as_secs_f64())?;
                    last = now;
                }
                host.flush();
                std::thread::sleep(IO_POLL_INTERVAL);
            }
            transport.join().unwrap()
        });
        let (client, _) = match tungstenite::client("ws://127.0.0.1/", client) {
            Ok(upgraded) => upgraded,
            Err(error) => {
                let _ = host.join();
                panic!("WebSocket upgrade failed: {error}");
            }
        };
        let mut running = Self {
            session: 0,
            client,
            server: Some(host),
        };
        running
            .client
            .send(Message::Binary(ipp_protocol::bootstrap().to_vec().into()))
            .unwrap();
        let reply = running.response();
        assert_eq!(&reply[..16], &ipp_protocol::bootstrap());
        assert_eq!(&reply[16..], &7u64.to_le_bytes());
        let create = ipp_protocol::host::encode_host_request(&ipp_protocol::host::HostRequest {
            connection: 7,
            request_id: 1,
            body: ipp_protocol::host::HostRequestBody::CreateWorld {
                options: Default::default(),
                temporary: true,
            },
        })
        .unwrap();
        running.client.send(Message::Binary(create.into())).unwrap();
        let response = ipp_protocol::host::decode_host_response(&running.response(), 7).unwrap();
        let ipp_protocol::host::HostResponseBody::Attached {
            session,
            ..
        } = response.body
        else {
            panic!("World not attached")
        };
        running.session = session;
        running
    }

    fn response(&mut self) -> Vec<u8> {
        match self.client.read().unwrap() {
            Message::Binary(bytes) => bytes.to_vec(),
            other => panic!("expected IPP response, received {other:?}"),
        }
    }

    fn finish(mut self) {
        self.client.close(None).unwrap();
        self.server.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for RunningConnection {
    fn drop(&mut self) {
        let _ = self.client.get_mut().shutdown(Shutdown::Both);
        if let Some(host) = self.server.take() {
            let _ = host.join();
        }
    }
}

struct FragmentFlood {
    stop: Arc<AtomicBool>,
    writer: Option<JoinHandle<io::Result<()>>>,
}

impl FragmentFlood {
    fn start(mut stream: TcpStream, request: Vec<u8>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let (started, ready) = mpsc::sync_channel(1);
        let writer = std::thread::spawn(move || {
            stream.write_all(&masked_fragment(2, false, &request[..8]))?;
            let chunk = masked_fragment(0, false, &[]).repeat(READ_BUDGET_BYTES / 6 + 1);
            stream.write_all(&chunk)?;
            let _ = started.send(());
            while !stopped.load(Ordering::Acquire) {
                stream.write_all(&chunk)?;
            }
            stream.write_all(&masked_fragment(0, true, &request[8..]))
        });
        let flood = Self {
            stop,
            writer: Some(writer),
        };
        ready.recv_timeout(TEST_TIMEOUT).unwrap();
        flood
    }

    fn finish(mut self) {
        self.stop.store(true, Ordering::Release);
        self.writer.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for FragmentFlood {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

pub(super) fn inspect(session: u64, request_id: u64) -> Vec<u8> {
    let mut bytes = session.to_le_bytes().to_vec();
    bytes.extend_from_slice(&request_id.to_le_bytes());
    bytes.push(3);
    bytes.push(0);
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&256u16.to_le_bytes());
    bytes
}

fn tick(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[16..24].try_into().unwrap())
}

#[test]
fn host_frames_progress_during_fragment_flood_then_inspection_completes() {
    let mut host = RunningConnection::start();
    let baseline = host.response();
    assert_eq!(baseline[24], 4);
    let mut last_tick = tick(&baseline);
    let mut last_time = f64::from_le_bytes(baseline[25..33].try_into().unwrap());
    let flood = FragmentFlood::start(
        host.client.get_ref().try_clone().unwrap(),
        inspect(host.session, 11),
    );

    // The incomplete IPP inspection cannot commit yet, but host frames must keep
    // arriving while the writer continuously supplies nonfinal fragments.
    for _ in 0..3 {
        let frame = host.response();
        assert_eq!(frame.len(), 33);
        assert_eq!(&frame[..8], &host.session.to_le_bytes());
        assert_eq!(&frame[8..16], &0u64.to_le_bytes());
        assert_eq!(frame[24], 4);
        let time = f64::from_le_bytes(frame[25..33].try_into().unwrap());
        assert!(tick(&frame) > last_tick);
        assert!(time > last_time);
        last_tick = tick(&frame);
        last_time = time;
    }
    flood.finish();

    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        assert!(
            Instant::now() < deadline,
            "fragmented inspection never completed"
        );
        let reply = host.response();
        if reply[24] == 4 {
            continue;
        }
        assert_eq!(reply[24], 3);
        assert_eq!(&reply[8..16], &11u64.to_le_bytes());
        assert!(tick(&reply) > last_tick);
        break;
    }
    host.finish();
}
