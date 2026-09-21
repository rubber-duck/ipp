//! Bounded native WebSocket ingress. Each connection has a single world owner.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tungstenite::protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode};
use tungstenite::{Error, Message};

use crate::NativeHost;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};

/// Maximum simultaneous independent sessions served by this small native host.
pub const MAX_CONNECTIONS: usize = 8;

/// Transport allocation limit, also enforced on complete application messages.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

// A message can contain arbitrarily many empty continuation frames. Limit actual
// socket reads, not only completed messages, so decoding must yield to the host.
const READ_BUDGET_BYTES: usize = 64 * 1024;

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
        ingress: Receiver<Vec<u8>>,
        queued: Arc<AtomicUsize>,
        throttled: Arc<AtomicBool>,
    },
    Closed {
        id: u64,
    },
}

struct HostConnectionOutput {
    sender: SyncSender<Vec<u8>>,
    failures: SyncSender<String>,
    pending: Option<Vec<u8>>,
    ingress: Receiver<Vec<u8>>,
    queued: Arc<AtomicUsize>,
    throttled: Arc<AtomicBool>,
}

// Shared production coordination used by the listener and transport test driver.
struct NativeConnectionHost {
    host: NativeHost,
    started: Instant,
    outputs: BTreeMap<u64, HostConnectionOutput>,
}

impl NativeConnectionHost {
    fn new() -> Result<Self, String> {
        Ok(Self {
            host: NativeHost::new()?,
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
            } => match self.host.open_connection(id) {
                Ok(_) => {
                    self.outputs.insert(
                        id,
                        HostConnectionOutput {
                            sender: replies,
                            failures,
                            pending: None,
                            ingress,
                            queued,
                            throttled,
                        },
                    );
                }
                Err(error) => {
                    let _ = failures.try_send(error);
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
            let mut failure = None;
            for _ in 0..64 {
                let admit = self.host.connection_accepts_input(id);
                let output = self.outputs.get(&id).expect("live connection");
                output.throttled.store(!admit, Ordering::Release);
                if !admit {
                    break;
                }
                let bytes = match output.ingress.try_recv() {
                    Ok(bytes) => bytes,
                    Err(_) => break,
                };
                output.queued.fetch_sub(1, Ordering::AcqRel);
                if let Err(error) = self.host.receive_connection(id, &bytes) {
                    failure = Some(error);
                    break;
                }
            }
            if let Some(error) = failure {
                if let Some(output) = self.outputs.remove(&id) {
                    let _ = output.failures.try_send(error);
                }
                self.host.close_connection(id);
            }
        }
        for (id, error) in self.host.tick_worlds(dt)? {
            if let Some(output) = self.outputs.remove(&id) {
                let _ = output.failures.try_send(error);
            }
            self.host.close_connection(id);
        }
        Ok(())
    }

    fn flush(&mut self) {
        let mut closed = Vec::new();
        for (&id, output) in &mut self.outputs {
            for _ in 0..64 {
                let Some(bytes) = output
                    .pending
                    .take()
                    .or_else(|| self.host.take_connection_response(id))
                else {
                    break;
                };
                match output.sender.try_send(bytes) {
                    Ok(()) => {}
                    Err(TrySendError::Full(bytes)) => {
                        output.pending = Some(bytes);
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
            self.outputs.remove(&id);
            self.host.close_connection(id);
        }
    }
}

/// Serve world-scoped connections with one owner for all worlds and services.
/// Socket threads perform transport I/O only; the Host owns simulation clocks.
pub fn serve(
    listener: TcpListener,
    file_access: Option<(String, crate::services::data_source::FileSystemDataSource)>,
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
    let (events, incoming) = mpsc::sync_channel(MAX_CONNECTIONS * 64);
    let mut host = NativeConnectionHost::new().map_err(io::Error::other)?;
    if let Some((prefix, source)) = file_access {
        host.host
            .runtime_mut()
            .data_sources_mut()
            .register(&prefix, source)
            .map_err(io::Error::other)?;
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

        for _ in 0..MAX_CONNECTIONS * 64 {
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

    let (replies, responses): (_, Receiver<Vec<u8>>) = mpsc::sync_channel(64);
    let (failures, failure) = mpsc::sync_channel(1);
    let (input, ingress) = mpsc::sync_channel(64);
    let queued = Arc::new(AtomicUsize::new(0));
    let throttled = Arc::new(AtomicBool::new(false));
    let mut local_throttled = false;
    events
        .send(HostConnectionEvent::Open {
            id: session_id,
            replies,
            failures,
            ingress,
            queued: queued.clone(),
            throttled: throttled.clone(),
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
        // Bound ingress work so a continuously readable peer cannot starve frames.
        for _ in 0..64 {
            let pending = queued.load(Ordering::Acquire);
            if local_throttled {
                local_throttled = pending >= 32;
            } else {
                local_throttled = pending >= 48;
            }
            if local_throttled || throttled.load(Ordering::Acquire) || blocked_since.is_some() {
                break;
            }
            match socket.read() {
                Ok(Message::Binary(bytes)) => {
                    queued.fetch_add(1, Ordering::AcqRel);
                    if input.try_send(bytes.to_vec()).is_err() {
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
            blocked_since = None;
            loop {
                let reply = match responses.try_recv() {
                    Ok(reply) => reply,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Err("Host closed the session".into()),
                };
                ready = true;
                match socket.send(Message::Binary(reply.into())) {
                    Ok(()) => {}
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
