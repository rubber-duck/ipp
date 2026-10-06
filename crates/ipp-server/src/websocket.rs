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
use async_channel::{Receiver, Sender, TryRecvError, TrySendError};
use ipp_host_session::services::task_scheduler::TaskHandle;
use ipp_host_session::{
    Host, HostConnectionMessage, HostServices, ReliableResponse, ResponseLease,
};
use std::collections::{BTreeMap, VecDeque};

/// Maximum simultaneous connections served by this small native host. Each connection may
/// open any number of World sessions.
///
/// Each connection owns an I/O task and its share of the Host's output budget; eight
/// covers a development client, tools and a few peers. A further connection is answered
/// with HTTP 503 and closed before the WebSocket handshake.
pub const MAX_CONNECTIONS: usize = 8;

/// Transport allocation limit: the protocol's complete application message budget.
pub const MAX_MESSAGE_BYTES: usize = ipp_protocol::MAX_MESSAGE_BYTES;

/// Socket bytes read per connection before yielding to peer I/O tasks.
///
/// A message can contain arbitrarily many empty continuation frames, so actual socket reads
/// are limited, not only completed messages, and decoding yields to peer tasks. 64 KiB is a
/// few TCP segments per pass; the next pass continues where this one stopped.
const READ_BUDGET_BYTES: usize = 64 * 1024;

/// Decoded messages an I/O task may hold for the Host thread, per connection.
///
/// The socket reads only while the Host admits the connection's input and this window has
/// room, so a throttled Host backs its sender up through TCP instead of failing it. This is
/// read-ahead, not admission: the Host still admits each message itself. Matching the Host's
/// request admission window ([`ipp_host_session::MAX_PENDING`]) lets one Host frame refill a
/// connection's whole window, and each frame's per-connection service allowance drains it.
const INGRESS_MESSAGES: usize = ipp_host_session::MAX_PENDING;

/// Replies and events handed to an I/O task and not yet written, per connection.
///
/// Their bytes stay charged to the Host's reliable-output account until the socket flushes
/// them, so this count bounds only the hand-off channel. Matching [`INGRESS_MESSAGES`] lets one
/// flush pass hand over the replies to a full window of requests.
const OUTPUT_MESSAGES: usize = INGRESS_MESSAGES;

/// Connection lifecycle events waiting for the Host thread: each connection task sends
/// at most one `Open` and one `Closed`, and tasks beyond this wait for the Host to drain.
const CONNECTION_EVENTS: usize = 2 * MAX_CONNECTIONS;

#[derive(Debug)]
struct BudgetedStream {
    stream: async_io::Async<TcpStream>,
    waiting_write: bool,
    remaining: usize,
    written: u64,
}

impl BudgetedStream {
    fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(None)?;
        Ok(Self {
            stream: async_io::Async::new(stream)?,
            waiting_write: false,
            // The HTTP upgrade has its own timeout and bounded async turns.
            remaining: READ_BUDGET_BYTES,
            written: 0,
        })
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
        self.waiting_write = false;
        let count = self.stream.get_ref().read(&mut buffer[..limit])?;
        self.remaining -= count;
        Ok(count)
    }
}

impl Write for BudgetedStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.waiting_write = true;
        let count = self.stream.get_ref().write(buffer)?;
        self.written = self.written.saturating_add(count as u64);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.get_ref().flush()
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
        replies: Sender<Vec<u8>>,
        notify: Sender<()>,
        failures: Sender<String>,
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
    sender: Sender<Vec<u8>>,
    notify: Sender<()>,
    failures: Sender<String>,
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
                notify,
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
                            notify,
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
                if output.throttled.swap(!admit, Ordering::AcqRel) != !admit {
                    let _ = output.notify.try_send(());
                }
                if !admit {
                    break;
                }
                let message = match output.ingress.try_recv() {
                    Ok(message) => message,
                    Err(_) => break,
                };
                output.queued.fetch_sub(1, Ordering::AcqRel);
                let _ = output.notify.try_send(());
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
                    let _ = output.notify.try_send(());
                }
                self.host.close_connection(id);
            }
        }
        for (id, error) in self.host.tick_worlds(dt)? {
            if let Some(output) = self.outputs.get_mut(&id) {
                output.failed = true;
                output.pending = None;
                report_connection_failure(&output.failures, &error);
                let _ = output.notify.try_send(());
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
                        let _ = output.notify.try_send(());
                    }
                    Err(TrySendError::Full(bytes)) => {
                        output.pending = Some(ReliableResponse::from_parts(bytes, lease));
                        break;
                    }
                    Err(TrySendError::Closed(_)) => {
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
        for output in self.outputs.values() {
            let _ = output.notify.try_send(());
        }
        self.host.shutdown_tasks();
        // Every cancelled transport drops its encoded output before Host leases.
        debug_assert!(
            self.outputs
                .values()
                .all(|output| output.released.load(Ordering::Acquire))
        );
    }
}

struct SocketOutputLifetime {
    released: Arc<AtomicBool>,
    events: Sender<HostConnectionEvent>,
    id: u64,
    host_thread: std::thread::Thread,
}

impl Drop for SocketOutputLifetime {
    fn drop(&mut self) {
        self.released.store(true, Ordering::Release);
        let _ = self.events.try_send(HostConnectionEvent::Closed {
            id: self.id,
        });
        self.host_thread.unpark();
    }
}

fn report_connection_failure(sender: &Sender<String>, error: &str) {
    let end = error.floor_char_boundary(2048);
    let _ = sender.try_send(error[..end].to_owned());
}

/// Host settings chosen at startup.
#[derive(Default)]
pub struct ServeOptions {
    /// Filesystem data source registered under its literal prefix.
    pub file_access: Option<(String, std::path::PathBuf)>,
    /// Explicit HTTP source namespaces; registration grants no export authority.
    pub http_prefixes: Vec<String>,
    /// Soft target in bytes for completed assets kept after their last consumer;
    /// `None` keeps the Host default and 0 evicts on release.
    pub asset_cache_bytes: Option<usize>,
}

/// Serve world-scoped connections with one owner for all worlds and services.
/// I/O tasks perform transport I/O and decode World requests as they arrive,
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
    let log_level = crate::diagnostics::level_from_env()?;
    if !listener.local_addr()?.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the PoC host requires a loopback listener",
        ));
    }
    let address = listener.local_addr()?;
    let listener = async_io::Async::new(listener)?;
    let active = Arc::new(AtomicUsize::new(0));
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let sessions = AtomicU64::new(u64::try_from(seed).map_err(io::Error::other)?);
    let (events, incoming) = async_channel::bounded(CONNECTION_EVENTS);
    let mut host = NativeConnectionHost::<P>::new().map_err(io::Error::other)?;
    if let Some((prefix, root)) = options.file_access {
        let source = crate::services::io::FileSystemIoSource::new(
            &prefix,
            root,
            false,
            host.host.task_schedulers().io(),
        )
        .map_err(io::Error::other)?;
        host.host
            .runtime_mut()
            .io_mut()
            .register(&prefix, source)
            .map_err(io::Error::other)?;
    }

    for prefix in options.http_prefixes {
        if !prefix.starts_with("http://") && !prefix.starts_with("https://") {
            return Err(io::Error::other(
                "HTTP source prefix requires http:// or https://",
            ));
        }
        let source = crate::services::io::HttpIoSource::new(host.host.task_schedulers().io())
            .map_err(io::Error::other)?;
        host.host
            .runtime_mut()
            .io_mut()
            .register(&prefix, source)
            .map_err(io::Error::other)?;
    }

    if let Some(bytes) = options.asset_cache_bytes {
        host.host
            .runtime_mut()
            .asset_resources_mut()
            .set_idle_resident_bytes_target(bytes);
    }
    crate::diagnostics::install(log_level, 0);
    let host_thread = std::thread::current();
    let wake_thread = host_thread.clone();
    host.host
        .set_task_wakeup(Arc::new(move || wake_thread.unpark()));
    let io = host.host.task_schedulers().io();
    let (accepted, accepted_connections) = async_channel::bounded(MAX_CONNECTIONS);
    let accept_thread = host_thread.clone();
    let accept_task: TaskHandle<io::Result<()>> = io.spawn(async move {
        loop {
            let stream = listener.accept().await?.0.into_inner()?;
            accepted
                .send(stream)
                .await
                .map_err(|_| io::Error::other("Host listener closed"))?;
            accept_thread.unpark();
        }
    });
    let mut connection_tasks: Vec<TaskHandle<()>> = Vec::new();
    ready(address)?;
    let mut last_frame = Instant::now();
    let mut next_frame = last_frame + FRAME_INTERVAL;
    loop {
        for _ in 0..MAX_CONNECTIONS {
            let mut stream = match accepted_connections.try_recv() {
                Ok(stream) => stream,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => return Err(io::Error::other("Host listener closed")),
            };
            if stream.set_nodelay(true).is_err() {
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
            let thread = host_thread.clone();
            connection_tasks.push(io.spawn(async move {
                let _guard = guard;
                if let Err(error) = connection(stream, session_id, &events, thread).await {
                    crate::diagnostics::install(log_level, session_id);
                    diagnostic!(
                        Error,
                        "[IPP server] session.failed session={} reason={}",
                        session_id,
                        error
                    );
                }
            }));
        }

        for _ in 0..CONNECTION_EVENTS {
            let event = match incoming.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => {
                    return Err(io::Error::other("Host ingress closed"));
                }
            };
            host.receive(event);
        }

        connection_tasks.retain(|task| !task.is_finished());
        if accept_task.is_finished() {
            return Err(io::Error::other("Host listener stopped"));
        }
        host.host.progress_resources().map_err(io::Error::other)?;
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
        std::thread::park_timeout(next_frame.saturating_duration_since(Instant::now()));
    }
}

async fn connection(
    stream: TcpStream,
    session_id: u64,
    events: &Sender<HostConnectionEvent>,
    host_thread: std::thread::Thread,
) -> Result<(), String> {
    let released = Arc::new(AtomicBool::new(false));
    let _output_lifetime = SocketOutputLifetime {
        released: released.clone(),
        events: events.clone(),
        id: session_id,
        host_thread: host_thread.clone(),
    };
    let config = WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_MESSAGE_BYTES + 1024)
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let handshake_deadline = Instant::now() + Duration::from_secs(10);
    let mut upgrade = tungstenite::accept_with_config(
        BudgetedStream::new(stream).map_err(|error| error.to_string())?,
        Some(config),
    );
    let mut socket = loop {
        match upgrade {
            Ok(socket) => break socket,
            Err(tungstenite::HandshakeError::Failure(error)) => return Err(error.to_string()),
            Err(tungstenite::HandshakeError::Interrupted(mut pending)) => {
                let stream = pending.get_ref().get_ref();
                if stream.remaining == 0 {
                    futures_lite::future::yield_now().await;
                } else {
                    wait_socket(stream, handshake_deadline).await?;
                }
                pending.get_mut().get_mut().begin_iteration();
                upgrade = pending.handshake();
            }
        }
    };

    let (replies, responses): (_, Receiver<Vec<u8>>) = async_channel::bounded(OUTPUT_MESSAGES);
    let (notify, notified) = async_channel::bounded(1);
    let (failures, failure) = async_channel::bounded(1);
    let (input, ingress) = async_channel::bounded(INGRESS_MESSAGES);
    let queued = Arc::new(AtomicUsize::new(0));
    let throttled = Arc::new(AtomicBool::new(false));
    let completed = Arc::new(AtomicU64::new(0));
    let mut submitted = 0u64;
    events
        .send(HostConnectionEvent::Open {
            id: session_id,
            replies,
            notify,
            failures,
            ingress,
            queued: queued.clone(),
            throttled: throttled.clone(),
            completed: completed.clone(),
            released,
        })
        .await
        .map_err(|_| "Host is closed")?;
    host_thread.unpark();
    let mut ready = false;
    let connected_at = Instant::now();
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
                    host_thread.unpark();
                    futures_lite::future::yield_now().await;
                }
                Ok(Message::Close(_)) => return finish_close(&mut socket).await,
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
        }

        let now = Instant::now();
        if !ready && now.duration_since(connected_at) >= Duration::from_secs(10) {
            return Err("IPP hello timed out".into());
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
            host_thread.unpark();
            blocked_since = None;
            for _ in 0..OUTPUT_MESSAGES {
                let reply = match responses.try_recv() {
                    Ok(reply) => reply,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Closed) => return Err("Host closed the session".into()),
                };
                ready = true;
                submitted = submitted
                    .checked_add(1)
                    .ok_or("socket delivery identity exhausted")?;
                match socket.send(Message::Binary(reply.into())) {
                    Ok(()) => {
                        completed.store(submitted, Ordering::Release);
                        host_thread.unpark();
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

        let readable = queued.load(Ordering::Acquire) < INGRESS_MESSAGES
            && !throttled.load(Ordering::Acquire)
            && blocked_since.is_none();
        if readable && socket.get_ref().remaining == 0 {
            futures_lite::future::yield_now().await;
            continue;
        }
        let timeout = if !ready {
            connected_at + Duration::from_secs(10)
        } else if let Some(since) = blocked_since {
            since + Duration::from_secs(30)
        } else {
            // No periodic connection polling: only readiness and Host notifications.
            now + Duration::from_secs(24 * 60 * 60)
        };
        futures_lite::future::or(
            async {
                if readable {
                    socket
                        .get_ref()
                        .stream
                        .readable()
                        .await
                        .map_err(|error| error.to_string())
                } else if blocked_since.is_some() {
                    socket
                        .get_ref()
                        .stream
                        .writable()
                        .await
                        .map_err(|error| error.to_string())
                } else {
                    std::future::pending().await
                }
            },
            futures_lite::future::or(
                async {
                    notified
                        .recv()
                        .await
                        .map(|_| ())
                        .map_err(|_| "Host closed".to_string())
                },
                async {
                    async_io::Timer::at(timeout).await;
                    Ok(())
                },
            ),
        )
        .await?;
    }
}

async fn wait_socket(stream: &BudgetedStream, deadline: Instant) -> Result<(), String> {
    futures_lite::future::or(
        async {
            if stream.waiting_write {
                stream.stream.writable().await
            } else {
                stream.stream.readable().await
            }
            .map_err(|error| error.to_string())
        },
        async {
            async_io::Timer::at(deadline).await;
            Err("WebSocket I/O timed out".into())
        },
    )
    .await
}

const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
#[cfg(test)]
const IO_POLL_INTERVAL: Duration = Duration::from_millis(2);

/// Keep the original 60 Hz cadence across wakeup jitter and frame work. On an
/// overrun, continue immediately without retaining a backlog of missed frames.
fn advance_frame_deadline(deadline: Instant, now: Instant) -> Instant {
    (deadline + FRAME_INTERVAL).max(now)
}

async fn finish_close(socket: &mut tungstenite::WebSocket<BudgetedStream>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match socket.flush() {
            Ok(()) | Err(Error::ConnectionClosed | Error::AlreadyClosed) => return Ok(()),
            Err(Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("WebSocket close timed out".into());
                }
                wait_socket(socket.get_ref(), deadline).await?;
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
