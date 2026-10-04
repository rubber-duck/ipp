//! Single-buffer streams with lookahead-driven capacity and explicit admission.

use super::{
    IoCancellation, IoError, IoReadBackend, IoReadOptions, IoReadWindow, IoWindowBackend,
    STREAM_CAPACITY,
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

struct IoStreamState {
    buffer: Option<Vec<u8>>,
    offset: usize,
    minimum: usize,
    received: usize,
    limit: Option<usize>,
    end: Option<Result<(), IoError>>,
    reader_waker: Option<Waker>,
    storage_waker: Option<Waker>,
    input_waker: Option<Waker>,
    capacity: usize,
    alive: bool,
}

/// Input capability for one exact stream. A reservation owns the only fill buffer.
#[derive(Clone)]
pub struct IoStreamInput {
    shared: Weak<Mutex<IoStreamState>>,
    cancellation: IoCancellation,
    finished: Arc<AtomicBool>,
}

/// Independent stream cursor lending reusable input storage.
pub struct StreamIoReader {
    shared: Arc<Mutex<IoStreamState>>,
    cancellation: IoCancellation,
    buffer: Option<Vec<u8>>,
    offset: usize,
    final_window: bool,
    finished: Arc<AtomicBool>,
    request: Option<u64>,
}

impl StreamIoReader {
    /// Create a reader and its sole acquisition input capability.
    pub fn new(options: IoReadOptions) -> (Self, IoStreamInput) {
        let shared = Arc::new(Mutex::new(IoStreamState {
            buffer: Some(Vec::new()),
            offset: 0,
            minimum: 1,
            received: 0,
            limit: options.max_bytes,
            end: None,
            reader_waker: None,
            storage_waker: None,
            input_waker: None,
            capacity: 0,
            alive: true,
        }));
        let cancellation = IoCancellation::default();
        let finished = Arc::new(AtomicBool::new(false));
        let input = IoStreamInput {
            shared: Arc::downgrade(&shared),
            cancellation: cancellation.clone(),
            finished: finished.clone(),
        };
        (
            Self {
                shared,
                cancellation,
                buffer: None,
                offset: 0,
                final_window: false,
                finished,
                request: None,
            },
            input,
        )
    }

    pub(crate) fn set_request(&mut self, id: u64) {
        self.request = Some(id);
    }
}

impl IoReadBackend for StreamIoReader {
    fn retained_storage(&self) -> Option<super::IoReaderStorage> {
        Some(super::IoReaderStorage {
            identity: super::IoStorageId(Arc::as_ptr(&self.shared) as usize),
            bytes: self.shared.lock().expect("IO stream lock").capacity,
            mapped_bytes: 0,
        })
    }

    fn register_storage_waker(&mut self, waker: &Waker) {
        let previous = self
            .shared
            .lock()
            .expect("IO stream lock")
            .storage_waker
            .replace(waker.clone());
        drop(previous);
    }

    fn request_id(&self) -> Option<u64> {
        self.request
    }

    fn cancellation(&self) -> Option<IoCancellation> {
        Some(self.cancellation.clone())
    }

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
        minimum: NonZeroUsize,
    ) -> Poll<Result<(), IoError>> {
        self.cancellation.register(cx.waker());
        if self.cancellation.is_cancelled() {
            return Poll::Ready(Err("IO stream was cancelled".into()));
        }
        let mut state = self.shared.lock().expect("IO stream lock");
        if let Some(Err(error)) = &state.end {
            return Poll::Ready(Err(error.clone()));
        }
        state.minimum = minimum.get();
        state.reader_waker = Some(cx.waker().clone());
        let Some(mut buffer) = state.buffer.take() else {
            return Poll::Pending;
        };
        let available = buffer.len() - state.offset;
        if available >= minimum.get() || state.end.is_some() {
            self.offset = state.offset;
            self.final_window = state.end.is_some();
            self.buffer = Some(buffer);
            return Poll::Ready(Ok(()));
        }
        let target = minimum.get().saturating_add(STREAM_CAPACITY);
        let mut storage_wake = None;
        if buffer.capacity() < target {
            if state.offset != 0 {
                buffer.copy_within(state.offset.., 0);
                buffer.truncate(available);
                state.offset = 0;
            }
            if let Err(error) = buffer.try_reserve_exact(target.saturating_sub(buffer.len())) {
                state.buffer = Some(buffer);
                return Poll::Ready(Err(error.to_string()));
            }
            state.capacity = buffer.capacity();
            storage_wake = state.storage_waker.clone();
        }
        state.buffer = Some(buffer);
        let wake = state.input_waker.take();
        drop(state);
        if let Some(waker) = storage_wake {
            waker.wake();
        }
        if let Some(waker) = wake {
            waker.wake();
        }
        Poll::Pending
    }

    fn window(&mut self) -> IoReadWindow<'_> {
        IoReadWindow::new(self)
    }
}

impl IoWindowBackend for StreamIoReader {
    fn bytes(&self) -> &[u8] {
        &self.buffer.as_ref().expect("lent stream buffer")[self.offset..]
    }

    fn is_final(&self) -> bool {
        self.final_window
    }

    fn consume(&mut self, count: usize) -> Result<(), IoError> {
        self.offset = self.offset.checked_add(count).ok_or("IO cursor overflow")?;
        Ok(())
    }

    fn release(&mut self) {
        let Some(buffer) = self.buffer.take() else {
            return;
        };
        let mut state = self.shared.lock().expect("IO stream lock");
        state.offset = self.offset;
        if self.final_window && self.offset == buffer.len() {
            self.finished.store(true, Ordering::Release);
        }
        state.buffer = Some(buffer);
        let wake = state.input_waker.take();
        drop(state);
        if let Some(waker) = wake {
            waker.wake();
        }
    }
}

impl Drop for StreamIoReader {
    fn drop(&mut self) {
        self.release();
        let mut state = self.shared.lock().expect("IO stream lock");
        state.alive = false;
        let wake = state.input_waker.take();
        let observer = state.storage_waker.take();
        drop(state);
        drop(observer);
        self.cancellation.cancel();
        if let Some(waker) = wake {
            waker.wake();
        }
    }
}

impl IoStreamInput {
    /// Whether this exact originating reader still permits acquisition.
    pub fn is_open(&self) -> bool {
        !self.cancellation.is_cancelled()
            && self
                .shared
                .upgrade()
                .is_some_and(|shared| shared.lock().expect("IO stream lock").alive)
    }

    /// Fence used by native acquisition tasks while awaiting external operations.
    pub fn cancellation(&self) -> IoCancellation {
        self.cancellation.clone()
    }

    /// Retained allocation including buffers currently lent or being filled.
    pub fn buffered_bytes(&self) -> usize {
        self.shared
            .upgrade()
            .map_or(0, |shared| shared.lock().expect("IO stream lock").capacity)
    }

    pub(crate) fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Reserve eventual lent storage before copying or acquiring bytes.
    /// None is backpressure, including a currently active immutable window.
    pub fn reserve(&self, length: usize) -> Result<Option<IoInputReservation>, IoError> {
        self.reserve_inner(length, true)
    }

    fn reserve_inner(
        &self,
        length: usize,
        known_length: bool,
    ) -> Result<Option<IoInputReservation>, IoError> {
        if length == 0 || length > STREAM_CAPACITY {
            return Err("Invalid IO transport chunk length".into());
        }
        let Some(shared) = self.shared.upgrade() else {
            return Ok(None);
        };
        if self.cancellation.is_cancelled() {
            return Ok(None);
        }
        let mut state = shared.lock().expect("IO stream lock");
        if !state.alive {
            return Ok(None);
        }
        if state.end.is_some() {
            return Err("IO stream input already ended".into());
        }
        if known_length
            && state
                .limit
                .is_some_and(|limit| length > limit.saturating_sub(state.received))
        {
            return Err("IO input byte budget exhausted".into());
        }
        let Some(mut buffer) = state.buffer.take() else {
            return Ok(None);
        };
        let available = buffer.len() - state.offset;
        let target = state.minimum.saturating_add(STREAM_CAPACITY);
        if length > target.saturating_sub(available) {
            state.buffer = Some(buffer);
            return Ok(None);
        }
        if state.offset != 0
            && buffer
                .len()
                .checked_add(length)
                .is_none_or(|end| end > buffer.capacity())
        {
            buffer.copy_within(state.offset.., 0);
            buffer.truncate(available);
            state.offset = 0;
        }
        if let Err(error) = buffer.try_reserve_exact(length) {
            state.buffer = Some(buffer);
            return Err(error.to_string());
        }
        let start = buffer.len();
        let Some(end) = start.checked_add(length) else {
            state.buffer = Some(buffer);
            return Err("IO input length overflow".into());
        };
        buffer.resize(end, 0);
        let changed = state.capacity != buffer.capacity();
        state.capacity = buffer.capacity();
        let wake = changed.then(|| state.reader_waker.take()).flatten();
        let storage_wake = changed.then(|| state.storage_waker.clone()).flatten();
        drop(state);
        if let Some(waker) = storage_wake {
            waker.wake();
        }
        if let Some(waker) = wake {
            waker.wake();
        }
        Ok(Some(IoInputReservation {
            shared,
            cancellation: self.cancellation.clone(),
            buffer: Some(buffer),
            start,
            length,
        }))
    }

    /// Register backpressure wakeups and attempt admission atomically.
    pub fn poll_reserve(
        &self,
        cx: &mut Context<'_>,
        length: usize,
    ) -> Poll<Result<IoInputReservation, IoError>> {
        self.cancellation.register(cx.waker());
        let Some(shared) = self.shared.upgrade() else {
            return Poll::Ready(Err("IO reader was dropped".into()));
        };
        shared.lock().expect("IO stream lock").input_waker = Some(cx.waker().clone());
        match self.reserve(length) {
            Ok(Some(reservation)) => Poll::Ready(Ok(reservation)),
            Ok(None) if !self.is_open() => Poll::Ready(Err("IO reader was cancelled".into())),
            Ok(None) => Poll::Pending,
            Err(error) => Poll::Ready(Err(error)),
        }
    }

    /// Await available fill capacity for a source whose next read length is unknown.
    /// One extra byte may be acquired to detect a configured input bound; commit
    /// validates the actual initialized prefix before any of it becomes readable.
    pub fn poll_fill(
        &self,
        cx: &mut Context<'_>,
        maximum: usize,
    ) -> Poll<Result<IoInputReservation, IoError>> {
        self.cancellation.register(cx.waker());
        let Some(shared) = self.shared.upgrade() else {
            return Poll::Ready(Err("IO reader was dropped".into()));
        };
        let mut state = shared.lock().expect("IO stream lock");
        state.input_waker = Some(cx.waker().clone());
        if !state.alive || self.cancellation.is_cancelled() {
            return Poll::Ready(Err("IO reader was cancelled".into()));
        }
        let room = state.buffer.as_ref().map_or(0, |buffer| {
            state
                .minimum
                .saturating_add(STREAM_CAPACITY)
                .saturating_sub(buffer.len() - state.offset)
        });
        let length =
            maximum
                .min(STREAM_CAPACITY)
                .min(room)
                .min(state.limit.map_or(usize::MAX, |limit| {
                    limit.saturating_sub(state.received).saturating_add(1)
                }));
        drop(state);
        if length == 0 {
            return Poll::Pending;
        }
        match self.reserve_inner(length, false) {
            Ok(Some(reservation)) => Poll::Ready(Ok(reservation)),
            Ok(None) => Poll::Pending,
            Err(error) => Poll::Ready(Err(error)),
        }
    }

    /// Copy one admitted chunk; false requires retry without any input copy.
    pub fn push(&self, bytes: &[u8]) -> Result<bool, IoError> {
        if !self.is_open() {
            return Ok(true);
        }
        if bytes.is_empty() {
            return Ok(true);
        }
        let Some(mut reservation) = self.reserve(bytes.len())? else {
            return Ok(false);
        };
        reservation.bytes_mut().copy_from_slice(bytes);
        reservation.commit(bytes.len())?;
        Ok(true)
    }

    pub(crate) fn adopt_complete(&self, bytes: Vec<u8>) -> Result<(), IoError> {
        let Some(shared) = self.shared.upgrade() else {
            return Ok(());
        };
        let mut state = shared.lock().expect("IO stream lock");
        if !state.alive || self.cancellation.is_cancelled() {
            return Ok(());
        }
        if state.limit.is_some_and(|limit| bytes.len() > limit) {
            return Err("IO input byte budget exhausted".into());
        }
        if state.received != 0 || state.buffer.is_none() {
            return Err("IO source already has active input".into());
        }
        if state.end.is_some() {
            return Ok(());
        }
        state.received = bytes.len();
        let storage_wake = (state.capacity != bytes.capacity())
            .then(|| state.storage_waker.clone())
            .flatten();
        state.capacity = bytes.capacity();
        state.offset = 0;
        state.buffer = Some(bytes);
        state.end = Some(Ok(()));
        let wake = state.reader_waker.take();
        drop(state);
        if let Some(waker) = storage_wake {
            waker.wake();
        }
        if let Some(waker) = wake {
            waker.wake();
        }
        Ok(())
    }

    /// Mark successful EOF or failure on this exact acquisition.
    pub fn finish(&self, result: Result<(), IoError>) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let mut state = shared.lock().expect("IO stream lock");
        if state.end.is_none() {
            state.end = Some(result.map_err(super::bounded_error));
        }
        let wake = state.reader_waker.take();
        drop(state);
        if let Some(waker) = wake {
            waker.wake();
        }
    }
}

/// Exclusive fill lease over reusable storage, never a second staging queue.
/// Dropping an uncommitted lease restores capacity without publishing its bytes.
pub struct IoInputReservation {
    shared: Arc<Mutex<IoStreamState>>,
    cancellation: IoCancellation,
    buffer: Option<Vec<u8>>,
    start: usize,
    length: usize,
}

impl IoInputReservation {
    /// Exact admitted length, independent of the current memory address.
    pub fn len(&self) -> usize {
        self.length
    }

    /// Reservations always admit a nonempty transport span.
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Exact admitted destination; platform operations retain this lease until done.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.buffer.as_mut().expect("active IO reservation")
            [self.start..self.start + self.length]
    }

    /// Pointer for synchronous platform copying. Reacquire JS memory views after reserve.
    /// No Rust reference may remain active while JavaScript writes to this range.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.bytes_mut().as_mut_ptr()
    }

    /// Publish an initialized accepted prefix; replacement/cancellation fences it.
    pub fn commit(mut self, count: usize) -> Result<(), IoError> {
        if count > self.length {
            return Err("IO reservation commit exceeds admitted length".into());
        }
        let mut state = self.shared.lock().expect("IO stream lock");
        let received = state
            .received
            .checked_add(count)
            .ok_or("IO received length overflow")?;
        if state.limit.is_some_and(|limit| received > limit) {
            drop(state);
            return Err("IO input byte budget exhausted".into());
        }
        let mut buffer = self.buffer.take().expect("active IO reservation");
        buffer.truncate(
            self.start
                + if state.alive && !self.cancellation.is_cancelled() {
                    count
                } else {
                    0
                },
        );
        if state.alive && !self.cancellation.is_cancelled() {
            state.received = received;
        }
        state.buffer = Some(buffer);
        let wake = state.reader_waker.take();
        drop(state);
        if let Some(waker) = wake {
            waker.wake();
        }
        Ok(())
    }
}

impl Drop for IoInputReservation {
    fn drop(&mut self) {
        let Some(mut buffer) = self.buffer.take() else {
            return;
        };
        buffer.truncate(self.start);
        let mut state = self.shared.lock().expect("IO stream lock");
        state.buffer = Some(buffer);
        let reader = state.reader_waker.take();
        let input = state.input_waker.take();
        drop(state);
        if let Some(waker) = reader {
            waker.wake();
        }
        if let Some(waker) = input {
            waker.wake();
        }
    }
}
