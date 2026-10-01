//! Bounded native WebSocket ingress. Each connection multiplexes Host requests and World
//! sessions; one Host thread owns every World and service.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tungstenite::protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode};
use tungstenite::{Error, Message};

use crate::services::NativeHostServices;
use ipp_host_session::{
    Host, HostConnectionMessage, HostServices, ReliableResponse, ResponseLease,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};

/// Maximum simultaneous connections served by this small native host. Each connection may
/// open any number of World sessions.
///
/// Each connection owns a socket thread and its share of the Host's output budget; eight
/// covers a development client, tools and a few peers. A further connection is answered
/// with HTTP 503 and closed before the WebSocket handshake.
pub const MAX_CONNECTIONS: usize = 8;

/// Transport allocation limit: the protocol's complete application message budget.
pub const MAX_MESSAGE_BYTES: usize = ipp_protocol::MAX_MESSAGE_BYTES;

/// Socket bytes read per connection before yielding to the Host.
///
/// A message can contain arbitrarily many empty continuation frames, so actual socket reads
/// are limited, not only completed messages, and decoding yields to the Host. 64 KiB is a
/// few TCP segments per pass; the next pass continues where this one stopped.
const READ_BUDGET_BYTES: usize = 64 * 1024;

/// Decoded messages a socket thread may hold for the Host thread, per connection.
///
/// The socket reads only while the Host admits the connection's input and this window has
/// room, so a throttled Host backs its sender up through TCP instead of failing it. This is
/// read-ahead, not admission: the Host still admits each message itself. Matching the Host's
/// request admission window ([`ipp_host_session::MAX_PENDING`]) lets one Host frame refill a
/// connection's whole window, and each frame's per-connection service allowance drains it.
const INGRESS_MESSAGES: usize = ipp_host_session::MAX_PENDING;

/// Replies and events handed to a socket thread and not yet written, per connection.
///
/// Their bytes stay charged to the Host's reliable-output account until the socket flushes
/// them, so this count bounds only the hand-off channel. Matching [`INGRESS_MESSAGES`] lets one
/// flush pass hand over the replies to a full window of requests.
const OUTPUT_MESSAGES: usize = INGRESS_MESSAGES;

/// Connection lifecycle events waiting for the Host thread: each connection thread sends
/// exactly one `Open` and one `Closed`, and threads beyond this wait for the Host to drain.
const CONNECTION_EVENTS: usize = 2 * MAX_CONNECTIONS;

#[derive(Debug)]
struct BudgetedStream {
    stream: TcpStream,
    remaining: usize,
    written: u64,
}

impl BudgetedStream {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            // The blocking HTTP upgrade precedes the runtime scheduling loop.
            remaining: usize::MAX,
            written: 0,
        }
    }

    fn begin_iteration(&mut self) {
        self.remaining = READ_BUDGET_BYTES;
    }
}

impl Read for BudgetedStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            // Tungstenite retains partial headers, payloads and fragmented messages
            // across WouldBlock. Never report EOF or rebuild its decoder here.
            return Err(io::ErrorKind::WouldBlock.into());
        }

        let limit = buffer.len().min(self.remaining);
        let count = self.stream.read(&mut buffer[..limit])?;
        self.remaining -= count;
        Ok(count)
    }
}

impl Write for BudgetedStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.stream.write(buffer)?;
        self.written = self.written.saturating_add(count as u64);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

struct ConnectionGuard(Arc<AtomicUsize>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

enum HostConnectionEvent {
    Open {
        id: u64,
        replies: SyncSender<Vec<u8>>,
        failures: SyncSender<String>,
        ingress: Receiver<HostConnectionMessage>,
        queued: Arc<AtomicUsize>,
        throttled: Arc<AtomicBool>,
        completed: Arc<AtomicU64>,
        released: Arc<AtomicBool>,
    },
    Closed {
        id: u64,
    },
}

struct HostConnectionOutput {
    sender: SyncSender<Vec<u8>>,
    failures: SyncSender<String>,
    pending: Option<ReliableResponse>,
    inflight: VecDeque<ResponseLease>,
    acknowledged: u64,
    completed: Arc<AtomicU64>,
    released: Arc<AtomicBool>,
    failed: bool,
    _transport_memory: ipp_core::services::reliable_output::ReliableOutputLease,
    ingress: Receiver<HostConnectionMessage>,
    queued: Arc<AtomicUsize>,
    throttled: Arc<AtomicBool>,
}

// Shared production coordination used by the listener and transport test driver.
struct NativeConnectionHost<P: HostServices> {
    host: Host<P>,
    started: Instant,
    outputs: BTreeMap<u64, HostConnectionOutput>,
}

impl<P: HostServices> NativeConnectionHost<P> {
    fn new() -> Result<Self, String> {
        Ok(Self {
            host: Host::new()?,
            started: Instant::now(),
            outputs: BTreeMap::new(),
        })
    }

    fn receive(&mut self, event: HostConnectionEvent) {
        self.host.maintain_connections(self.started.elapsed());
        match event {
            HostConnectionEvent::Open {
                id,
                replies,
                failures,
                ingress,
                queued,
                throttled,
                completed,
                released,
            } => match self.host.open_connection(id) {
                Ok(_) => {
                    let memory = match self
                        .host
                        .reserve_connection_output_bytes(id, 2 * (MAX_MESSAGE_BYTES + 1024) + 16384)
                    {
                        Ok(memory) => memory,
                        Err(error) => {
                            report_connection_failure(&failures, &error);
                            self.host.close_connection(id);
                            return;
                        }
                    };
                    self.outputs.insert(
                        id,
                        HostConnectionOutput {
                            sender: replies,
                            failures,
                            pending: None,
                            inflight: VecDeque::new(),
                            acknowledged: 0,
                            completed,
                            released,
                            failed: false,
                            _transport_memory: memory,
                            ingress,
                            queued,
                            throttled,
                        },
                    );
                }
                Err(error) => {
                    report_connection_failure(&failures, &error);
                }
            },
            HostConnectionEvent::Closed {
                id,
            } => {
                self.outputs.remove(&id);
                self.host.close_connection(id);
            }
        }
    }

    fn tick(&mut self, dt: f64) -> Result<(), String> {
        self.host.maintain_connections(self.started.elapsed());
        // Each sender owns a bounded ingress queue. Visit every connection with
        // the same service allowance, independent of a noisy peer's readiness.
        let ids: Vec<_> = self.outputs.keys().copied().collect();
        for id in ids {
            if self.outputs[&id].failed {
                continue;
            }
            let mut failure = None;
            for _ in 0..INGRESS_MESSAGES {
                let admit = self.host.connection_accepts_input(id);
                let output = self.outputs.get(&id).expect("live connection");
                output.throttled.store(!admit, Ordering::Release);
                if !admit {
                    break;
                }
                let message = match output.ingress.try_recv() {
                    Ok(message) => message,
                    Err(_) => break,
                };
                output.queued.fetch_sub(1, Ordering::AcqRel);
                if let Err(error) = self.host.receive_connection_message(id, message) {
                    failure = Some(error);
                    break;
                }
            }
            if let Some(error) = failure {
                if let Some(output) = self.outputs.get_mut(&id) {
                    output.failed = true;
                    output.pending = None;
                    report_connection_failure(&output.failures, &error);
                }
                self.host.close_connection(id);
            }
        }
        for (id, error) in self.host.tick_worlds(dt)? {
            if let Some(output) = self.outputs.get_mut(&id) {
                output.failed = true;
                output.pending = None;
                report_connection_failure(&output.failures, &error);
            }
            self.host.close_connection(id);
        }
        Ok(())
    }

    fn flush(&mut self) {
        let mut closed = Vec::new();
        for (&id, output) in &mut self.outputs {
            let completed = output.completed.load(Ordering::Acquire);
            while output.acknowledged < completed {
                output
                    .inflight
                    .pop_front()
                    .expect("socket completion has retained lease");
                output.acknowledged += 1;
            }
            if output.failed {
                continue;
            }
            for _ in 0..OUTPUT_MESSAGES {
                let Some(bytes) = output
                    .pending
                    .take()
                    .or_else(|| self.host.take_connection_response(id))
                else {
                    break;
                };
                let (bytes, lease) = bytes.into_parts();
                match output.sender.try_send(bytes) {
                    Ok(()) => {
                        output.inflight.push_back(lease);
                    }
                    Err(TrySendError::Full(bytes)) => {
                        output.pending = Some(ReliableResponse::from_parts(bytes, lease));
                        break;
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        closed.push(id);
                        break;
                    }
                }
            }
        }
        for id in closed {
            if let Some(output) = self.outputs.get_mut(&id) {
                output.failed = true;
                output.pending = None;
            }
            self.host.close_connection(id);
        }
    }
}

impl<P: HostServices> Drop for NativeConnectionHost<P> {
    fn drop(&mut self) {
        for (&id, output) in &self.outputs {
            report_connection_failure(&output.failures, "Host closed");
            self.host.close_connection(id);
        }
        while self
            .outputs
            .values()
            .any(|output| !output.released.load(Ordering::Acquire))
        {
            std::thread::sleep(IO_POLL_INTERVAL);
        }
    }
}

struct SocketOutputLifetime(Arc<AtomicBool>);

impl Drop for SocketOutputLifetime {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn report_connection_failure(sender: &SyncSender<String>, error: &str) {
    let end = error.floor_char_boundary(2048);
    let _ = sender.try_send(error[..end].to_owned());
}

/// Host settings chosen at startup.
#[derive(Default)]
pub struct ServeOptions {
    /// Filesystem data source registered under its literal prefix.
    pub file_access: Option<(String, crate::services::data_source::FileSystemDataSource)>,
    /// Soft target in bytes for completed assets kept after their last consumer;
    /// `None` keeps the Host default and 0 evicts on release.
    pub asset_cache_bytes: Option<usize>,
}

/// Serve world-scoped connections with one owner for all worlds and services.
/// Socket threads perform transport I/O and decode World requests as they arrive,
/// in parallel with Host frames; the Host owns simulation clocks and all state.
pub fn serve(
    listener: TcpListener,
    options: ServeOptions,
    ready: impl FnOnce(std::net::SocketAddr) -> io::Result<()>,
) -> io::Result<()> {
    serve_with::<NativeHostServices>(listener, options, ready)
}

/// [`serve`] with platform services composed by an embedder, such as a test host
/// that presents through its own graphics context. `P` is initialized on the
/// calling thread, which then runs every Host frame and presentation.
pub fn serve_with<P: HostServices>(
    listener: TcpListener,
    options: ServeOptions,
    ready: impl FnOnce(std::net::SocketAddr) -> io::Result<()>,
) -> io::Result<()> {
    #[cfg(feature = "diagnostics")]
    let log_level = crate::diagnostics::level_from_env()?;
    if !listener.local_addr()?.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the PoC host requires a loopback listener",
        ));
    }
    listener.set_nonblocking(true)?;
    let active = Arc::new(AtomicUsize::new(0));
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let sessions = AtomicU64::new(u64::try_from(seed).map_err(io::Error::other)?);
    let (events, incoming) = mpsc::sync_channel(CONNECTION_EVENTS);
    let mut host = NativeConnectionHost::<P>::new().map_err(io::Error::other)?;
    if let Some((prefix, source)) = options.file_access {
        host.host
            .runtime_mut()
            .data_sources_mut()
            .register(&prefix, source)
            .map_err(io::Error::other)?;
    }

    if let Some(bytes) = options.asset_cache_bytes {
        host.host
            .runtime_mut()
            .asset_resources_mut()
            .set_idle_resident_bytes_target(bytes);
    }
    #[cfg(feature = "diagnostics")]
    crate::diagnostics::install(log_level, 0);
    ready(listener.local_addr()?)?;
    let mut last_frame = Instant::now();
    let mut next_frame = last_frame + FRAME_INTERVAL;
    loop {
        for _ in 0..MAX_CONNECTIONS {
            let (mut stream, _) = match listener.accept() {
                Ok(incoming) => incoming,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            };
            if stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
                .and_then(|()| stream.set_nodelay(true))
                .is_err()
            {
                continue;
            }
            if active
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < MAX_CONNECTIONS).then_some(count + 1)
                })
                .is_err()
            {
                // Rejection must not block the shared simulation owner.
                if stream.set_nonblocking(true).is_err() {
                    continue;
                }
                let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let guard = ConnectionGuard(Arc::clone(&active));
            let session_id = sessions
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
                .map_err(|_| io::Error::other("session identity space exhausted"))?;
            let events = events.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("ipp-connection-{session_id}"))
                .spawn(move || {
                    let _guard = guard;
                    #[cfg(feature = "diagnostics")]
                    crate::diagnostics::install(log_level, session_id);
                    if let Err(error) = connection(stream, session_id, &events) {
                        diagnostic!(
                            Error,
                            "[IPP server] session.failed session={} reason={}",
                            session_id,
                            error
                        );
                        #[cfg(not(feature = "diagnostics"))]
                        eprintln!("session {session_id}: {error}");
                    }
                    let _ = events.send(HostConnectionEvent::Closed {
                        id: session_id,
                    });
                });
            // A failed spawn drops the captured stream and connection guard.
            if let Err(error) = spawned {
                diagnostic!(Warn, "[IPP server] connection setup failed: {}", error);
                let _ = error;
            }
        }

        for _ in 0..CONNECTION_EVENTS {
            let event = match incoming.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(io::Error::other("Host ingress closed"));
                }
            };
            host.receive(event);
        }

        let now = Instant::now();
        let frame_due = now >= next_frame;
        if frame_due {
            let dt = now.duration_since(last_frame).as_secs_f64().min(0.25);
            last_frame = now;
            host.tick(dt).map_err(io::Error::other)?;
        }
        host.flush();
        if frame_due {
            next_frame = advance_frame_deadline(next_frame, Instant::now());
        }
        std::thread::sleep(
            next_frame
                .saturating_duration_since(Instant::now())
                .min(IO_POLL_INTERVAL),
        );
    }
}

fn connection(
    stream: TcpStream,
    session_id: u64,
    events: &SyncSender<HostConnectionEvent>,
) -> Result<(), String> {
    let released = Arc::new(AtomicBool::new(false));
    let _output_lifetime = SocketOutputLifetime(released.clone());
    let config = WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_MESSAGE_BYTES + 1024)
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let mut socket = tungstenite::accept_with_config(BudgetedStream::new(stream), Some(config))
        .map_err(|error| error.to_string())?;
    // Tungstenite retains partial frames and buffered writes across WouldBlock.
    // This thread owns only the socket; world updates run on the shared Host.
    socket
        .get_mut()
        .stream
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    socket
        .get_mut()
        .stream
        .set_read_timeout(None)
        .map_err(|error| error.to_string())?;
    socket
        .get_mut()
        .stream
        .set_write_timeout(None)
        .map_err(|error| error.to_string())?;

    let (replies, responses): (_, Receiver<Vec<u8>>) = mpsc::sync_channel(OUTPUT_MESSAGES);
    let (failures, failure) = mpsc::sync_channel(1);
    let (input, ingress) = mpsc::sync_channel(INGRESS_MESSAGES);
    let queued = Arc::new(AtomicUsize::new(0));
    let throttled = Arc::new(AtomicBool::new(false));
    let completed = Arc::new(AtomicU64::new(0));
    let mut submitted = 0u64;
    events
        .send(HostConnectionEvent::Open {
            id: session_id,
            replies,
            failures,
            ingress,
            queued: queued.clone(),
            throttled: throttled.clone(),
            completed: completed.clone(),
            released,
        })
        .map_err(|_| "Host is closed")?;
    let mut ready = false;
    let connected_at = Instant::now();
    let mut next_frame = connected_at + FRAME_INTERVAL;
    let mut blocked_since = None;

    loop {
        // Terminal failure has independent reserved delivery, even when reliable
        // data fills the output channel or the socket cannot flush it.
        if let Ok(error) = failure.try_recv() {
            let congestion = error.contains("congestion");
            let _ = socket.close(Some(CloseFrame {
                code: if congestion {
                    CloseCode::Again
                } else {
                    CloseCode::Protocol
                },
                reason: if congestion {
                    "connection congestion"
                } else {
                    "invalid IPP request"
                }
                .into(),
            }));
            return Err(error);
        }
        // Reset only when the host regains control, not between socket.read calls.
        // The decoder may also consume its bounded buffer from the previous turn.
        socket.get_mut().begin_iteration();
        // Bound ingress work so a continuously readable peer cannot starve frames. While the
        // Host throttles this connection or the read-ahead window is full, unread messages
        // wait in the socket: the sender sees TCP backpressure, never a failure.
        for _ in 0..INGRESS_MESSAGES {
            if queued.load(Ordering::Acquire) >= INGRESS_MESSAGES
                || throttled.load(Ordering::Acquire)
                || blocked_since.is_some()
            {
                break;
            }
            match socket.read() {
                Ok(Message::Binary(bytes)) => {
                    queued.fetch_add(1, Ordering::AcqRel);
                    // Decoding needs no Host state; the encoding is released here.
                    let message = HostConnectionMessage::decode(bytes.to_vec());
                    if input.try_send(message).is_err() {
                        queued.fetch_sub(1, Ordering::AcqRel);
                        return Err("connection ingress closed".into());
                    }
                }
                Ok(Message::Close(_)) => return finish_close(&mut socket),
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
                Ok(Message::Text(_)) => {
                    let _ = socket.close(Some(CloseFrame {
                        code: CloseCode::Unsupported,
                        reason: "binary IPP messages required".into(),
                    }));
                    return Err("text message rejected".into());
                }
                Ok(Message::Frame(_)) => return Err("unexpected raw frame".into()),
                Err(Error::ConnectionClosed | Error::AlreadyClosed) => return Ok(()),
                Err(Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.to_string()),
            }
            if Instant::now() >= next_frame {
                break;
            }
        }

        let now = Instant::now();
        if !ready && now.duration_since(connected_at) >= Duration::from_secs(10) {
            return Err("IPP bootstrap timed out".into());
        }
        if now >= next_frame {
            next_frame = now + FRAME_INTERVAL;
        }

        let before_write = socket.get_ref().written;
        let flushed = match socket.flush() {
            Ok(()) => true,
            Err(Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => false,
            Err(Error::ConnectionClosed | Error::AlreadyClosed) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        if flushed {
            completed.store(submitted, Ordering::Release);
            blocked_since = None;
            loop {
                let reply = match responses.try_recv() {
                    Ok(reply) => reply,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Err("Host closed the session".into()),
                };
                ready = true;
                submitted = submitted
                    .checked_add(1)
                    .ok_or("socket delivery identity exhausted")?;
                match socket.send(Message::Binary(reply.into())) {
                    Ok(()) => {
                        completed.store(submitted, Ordering::Release);
                    }
                    // This frame is already retained by Tungstenite: do not resend.
                    Err(Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                        blocked_since = Some(Instant::now());
                        break;
                    }
                    Err(Error::ConnectionClosed | Error::AlreadyClosed) => return Ok(()),
                    Err(error) => return Err(error.to_string()),
                }
            }
        } else {
            if socket.get_ref().written > before_write {
                blocked_since = Some(now);
            }
            let since = blocked_since.get_or_insert(now);
            if now.duration_since(*since) >= Duration::from_secs(30) {
                let _ = socket.close(Some(CloseFrame {
                    code: CloseCode::Again,
                    reason: "connection congestion".into(),
                }));
                return Err("connection congestion: no delivery progress for 30 seconds".into());
            }
        }

        // Small bounded polling is enough for this std-only host. A future I/O
        // reactor can replace it without changing session scheduling or clients.
        std::thread::sleep(
            next_frame
                .saturating_duration_since(Instant::now())
                .min(IO_POLL_INTERVAL),
        );
    }
}

const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
const IO_POLL_INTERVAL: Duration = Duration::from_millis(2);

/// Keep the original 60 Hz cadence across wakeup jitter and frame work. On an
/// overrun, continue immediately without retaining a backlog of missed frames.
fn advance_frame_deadline(deadline: Instant, now: Instant) -> Instant {
    (deadline + FRAME_INTERVAL).max(now)
}

fn finish_close(socket: &mut tungstenite::WebSocket<BudgetedStream>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match socket.flush() {
            Ok(()) | Err(Error::ConnectionClosed | Error::AlreadyClosed) => return Ok(()),
            Err(Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("WebSocket close timed out".into());
                }
                std::thread::sleep(IO_POLL_INTERVAL);
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "websocket_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "websocket_congestion_tests.rs"]
mod congestion_tests;
