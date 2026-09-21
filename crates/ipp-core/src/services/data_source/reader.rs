//! Reader/provider contracts and a bounded host-fed asynchronous stream.

use super::{DataReadOptions, DataSource, STREAM_CAPACITY};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    rc::{Rc, Weak},
    sync::Arc,
    task::{Context, Poll, Waker},
};

/// Asynchronous owned input, independent of an executor or graphics backend.
pub trait DataReader {
    /// Host request correlation for asynchronous input, when present.
    fn request_id(&self) -> Option<u64> {
        None
    }

    /// Fill a caller-owned buffer. Zero means EOF, Pending means await more input.
    fn poll_read(&mut self, cx: &mut Context<'_>, output: &mut [u8])
    -> Poll<Result<usize, String>>;
}

/// Reader over exclusively owned or shared immutable bytes.
pub struct MemoryDataReader {
    bytes: Arc<Vec<u8>>,
    offset: usize,
}

impl MemoryDataReader {
    /// Retain immutable input without copying it for each reader.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: Arc::new(bytes.into()),
            offset: 0,
        }
    }
}

impl DataReader for MemoryDataReader {
    fn poll_read(
        &mut self,
        _cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<Result<usize, String>> {
        let n = output.len().min(self.bytes.len() - self.offset);
        output[..n].copy_from_slice(&self.bytes[self.offset..self.offset + n]);
        self.offset += n;
        Poll::Ready(Ok(n))
    }
}

/// Host work referencing a specific input stream, never the manager's lifecycle.
#[derive(Clone, Debug)]
pub(crate) struct DataStreamRequest {
    /// Private host stream correlation; a new reader receives a new value.
    pub id: u64,
    /// Named immutable input.
    pub source: String,
    /// Optional caller bound for this operation.
    pub max_bytes: Option<usize>,
    /// This reader must recover previously accepted content.
    pub recovery: bool,
}

struct DataStreamBuffer {
    bytes: VecDeque<u8>,
    received: usize,
    limit: Option<usize>,
    end: Option<Result<(), String>>,
    waker: Option<Waker>,
}

/// Writer tied directly to its originating reader. A dropped reader invalidates
/// this writer, so delayed network input cannot reach a subsequent load.
#[derive(Clone)]
pub(crate) struct DataStreamWriter {
    pipe: Weak<RefCell<DataStreamBuffer>>,
    finished: Rc<Cell<bool>>,
}

impl DataStreamWriter {
    /// Copy a bounded chunk. False means backpressure: keep the input and retry.
    /// Data for a cancelled reader is harmless and considered consumed.
    pub fn push(&self, bytes: &[u8]) -> Result<bool, String> {
        if bytes.len() > STREAM_CAPACITY {
            return Err("Asset chunk exceeds stream capacity".into());
        }
        let Some(pipe) = self.pipe.upgrade() else {
            return Ok(true);
        };
        let mut pipe = pipe.borrow_mut();
        if pipe.end.is_some() {
            return Err("Asset input already ended".into());
        }
        if pipe
            .limit
            .is_some_and(|limit| bytes.len() > limit.saturating_sub(pipe.received))
        {
            return Err("Data input byte budget exhausted".into());
        }
        if bytes.len() > STREAM_CAPACITY - pipe.bytes.len() {
            return Ok(false);
        }
        pipe.bytes
            .try_reserve(bytes.len())
            .map_err(|error| error.to_string())?;
        pipe.bytes.extend(bytes);
        pipe.received = pipe
            .received
            .checked_add(bytes.len())
            .ok_or("Data input length overflow")?;
        if let Some(waker) = pipe.waker.take() {
            waker.wake();
        }
        Ok(true)
    }

    /// Mark EOF or failure; buffered bytes are consumed before successful EOF.
    pub fn finish(&self, result: Result<(), String>) {
        if let Some(pipe) = self.pipe.upgrade() {
            let mut pipe = pipe.borrow_mut();
            if pipe.end.is_none() {
                pipe.end = Some(result.map_err(super::bounded_error));
            }
            if let Some(waker) = pipe.waker.take() {
                waker.wake();
            }
        }
    }

    /// Configured whole-input bound for this reader.
    pub fn max_bytes(&self) -> Option<usize> {
        self.pipe.upgrade().and_then(|pipe| pipe.borrow().limit)
    }

    /// Retained staging allocation, including space already consumed by the reader.
    pub fn buffered_bytes(&self) -> usize {
        self.pipe
            .upgrade()
            .map_or(0, |pipe| pipe.borrow().bytes.capacity())
    }

    /// Whether the originating resource still owns its reader.
    pub fn is_open(&self) -> bool {
        self.pipe.strong_count() != 0
    }
}

struct DataStreamReader {
    id: u64,
    pipe: Rc<RefCell<DataStreamBuffer>>,
    finished: Rc<Cell<bool>>,
}

impl Drop for DataStreamReader {
    fn drop(&mut self) {
        let pipe = self.pipe.borrow();
        self.finished
            .set(matches!(pipe.end, Some(Ok(()))) && pipe.bytes.is_empty());
    }
}

impl DataReader for DataStreamReader {
    fn request_id(&self) -> Option<u64> {
        Some(self.id)
    }

    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<Result<usize, String>> {
        let mut pipe = self.pipe.borrow_mut();
        if let Some(Err(error)) = &pipe.end {
            return Poll::Ready(Err(error.clone()));
        }
        let n = output.len().min(pipe.bytes.len());
        if n != 0 || output.is_empty() {
            for destination in &mut output[..n] {
                *destination = pipe.bytes.pop_front().expect("buffered byte");
            }
            return Poll::Ready(Ok(n));
        }
        if pipe.end.is_some() {
            return Poll::Ready(Ok(0));
        }
        pipe.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[derive(Default)]
struct DataStreamRegistry {
    next_id: u64,
    requests: VecDeque<DataStreamRequest>,
    writers: BTreeMap<u64, DataStreamWriter>,
}

/// Bounded provider bridge shared with the owning host. It only routes bytes;
/// source schemes and decoders remain separately registered.
#[derive(Clone, Default)]
pub(crate) struct DataStreamProvider {
    streams: Rc<RefCell<DataStreamRegistry>>,
}

impl DataStreamProvider {
    /// Take pending host I/O work in opening order.
    pub fn take_requests(&self, mut selected: impl FnMut(u64) -> bool) -> Vec<DataStreamRequest> {
        let mut streams = self.streams.borrow_mut();
        let pending: Vec<_> = streams.requests.drain(..).collect();
        let mut requests = Vec::new();
        for request in pending {
            if !streams
                .writers
                .get(&request.id)
                .is_some_and(DataStreamWriter::is_open)
            {
                continue;
            }
            if selected(request.id) {
                requests.push(request);
            } else {
                streams.requests.push_back(request);
            }
        }
        requests
    }

    /// Obtain the writer for this exact reader, including when the host awaits I/O.
    pub fn writer(&self, id: u64) -> Option<DataStreamWriter> {
        self.streams.borrow().writers.get(&id).cloned()
    }

    /// Drain stream identities whose readers were consumed or cancelled.
    pub fn take_closed(&self) -> Vec<u64> {
        let mut streams = self.streams.borrow_mut();
        let closed: Vec<_> = streams
            .writers
            .iter()
            .filter(|(_, writer)| !writer.is_open())
            .map(|(&id, _)| id)
            .collect();
        let cancelled = closed
            .iter()
            .filter(|id| !streams.writers[id].finished.get())
            .copied()
            .collect();
        for id in &closed {
            streams.writers.remove(id);
        }
        streams
            .requests
            .retain(|request| !closed.contains(&request.id));
        cancelled
    }

    /// Aggregate retained input across concurrent streams.
    pub fn buffered_bytes(&self) -> usize {
        self.streams
            .borrow()
            .writers
            .values()
            .map(DataStreamWriter::buffered_bytes)
            .sum()
    }
}

impl DataSource for DataStreamProvider {
    fn list(&mut self, _identifier: &str) -> Result<Vec<String>, String> {
        Err("Source listing is unavailable".into())
    }

    fn open_read(
        &mut self,
        source: &str,
        options: DataReadOptions,
    ) -> Result<Box<dyn DataReader>, String> {
        let DataReadOptions {
            max_bytes,
            recovery,
        } = options;
        let mut streams = self.streams.borrow_mut();
        streams.writers.retain(|_, writer| !writer.finished.get());
        let id = streams
            .next_id
            .checked_add(1)
            .ok_or("Asset stream identity exhausted")?;
        streams.next_id = id;
        let pipe = Rc::new(RefCell::new(DataStreamBuffer {
            bytes: VecDeque::new(),
            received: 0,
            limit: max_bytes,
            end: None,
            waker: None,
        }));
        let finished = Rc::new(Cell::new(false));
        streams.writers.insert(
            id,
            DataStreamWriter {
                pipe: Rc::downgrade(&pipe),
                finished: finished.clone(),
            },
        );
        streams.requests.push_back(DataStreamRequest {
            id,
            source: source.to_owned(),
            max_bytes,
            recovery,
        });
        Ok(Box::new(DataStreamReader {
            id,
            pipe,
            finished,
        }))
    }
}
