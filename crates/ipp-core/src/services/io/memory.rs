//! Immutable input and atomic bounded memory publication.

use super::{
    BufferIoReader, IoListFuture, IoListing, IoOpenReadFuture, IoOpenWriteFuture, IoReadOptions,
    IoReader, IoSource, IoWriteBackend, IoWriter, MemoryIoListing,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

/// In-memory data source usable independently of assets.
#[derive(Clone, Default)]
pub struct MemoryIoSource {
    values: Arc<Mutex<BTreeMap<String, Arc<Vec<u8>>>>>,
    writable: bool,
}

impl MemoryIoSource {
    /// Construct an empty source with an explicit write capability.
    pub fn new(writable: bool) -> Self {
        Self {
            writable,
            ..Self::default()
        }
    }

    /// Register already owned immutable bytes once.
    pub fn insert(&self, identifier: String, bytes: Vec<u8>) -> Result<(), String> {
        let mut values = self.values.lock().expect("memory IO source lock");
        if values.contains_key(&identifier) {
            return Err("Duplicate data identifier".into());
        }
        values.insert(identifier, Arc::new(bytes));
        Ok(())
    }

    /// Remove a name; already opened readers retain their immutable input.
    pub(crate) fn remove(&self, identifier: &str) {
        self.values
            .lock()
            .expect("memory IO source lock")
            .remove(identifier);
    }
}

impl IoSource for MemoryIoSource {
    fn list(&mut self, identifier: &str) -> IoListFuture {
        let entries: Vec<_> = self
            .values
            .lock()
            .expect("memory IO source lock")
            .keys()
            .filter(|key| key.starts_with(identifier))
            .cloned()
            .collect();
        Box::pin(std::future::ready(Ok(
            Box::new(MemoryIoListing::new(entries)) as Box<dyn IoListing>,
        )))
    }

    fn open_read(&mut self, identifier: &str, options: IoReadOptions) -> IoOpenReadFuture {
        let result = (|| {
            if options.recovery && self.writable {
                return Err("Mutable memory data has no immutable recovery validator".into());
            }
            let bytes = self
                .values
                .lock()
                .expect("memory IO source lock")
                .get(identifier)
                .cloned()
                .ok_or("Data is unavailable")?;
            if options.max_bytes.is_some_and(|limit| bytes.len() > limit) {
                return Err("Data input byte budget exhausted".into());
            }
            let length = bytes.len();
            Ok(Box::new(BufferIoReader::from_backing(bytes, 0..length)?) as Box<dyn IoReader>)
        })();
        Box::pin(std::future::ready(result))
    }

    fn can_write(&self, _identifier: &str) -> bool {
        self.writable
    }

    fn open_write(&mut self, identifier: &str, max_bytes: usize) -> IoOpenWriteFuture {
        if !self.writable {
            return Box::pin(std::future::ready(Err("Data source is read-only".into())));
        }
        Box::pin(std::future::ready(Ok(Box::new(MemorySourceWriter {
            source: self.clone(),
            identifier: identifier.to_owned(),
            bytes: Vec::new(),
            limit: max_bytes,
            closed: false,
        }) as Box<dyn IoWriter>)))
    }
}

struct MemorySourceWriter {
    source: MemoryIoSource,
    identifier: String,
    bytes: Vec<u8>,
    limit: usize,
    closed: bool,
}

impl IoWriteBackend for MemorySourceWriter {
    fn poll_write(&mut self, _cx: &mut Context<'_>, bytes: &[u8]) -> Poll<Result<usize, String>> {
        if self.closed || bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Poll::Ready(Err("Data output closed or byte budget exhausted".into()));
        }
        self.bytes.extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        Poll::Ready(if self.closed {
            Err("Data output is closed".into())
        } else {
            Ok(())
        })
    }

    fn poll_finish(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        if self.closed {
            return Poll::Ready(Err("Data output is closed".into()));
        }
        self.source
            .values
            .lock()
            .expect("memory IO source lock")
            .insert(
                self.identifier.clone(),
                Arc::new(std::mem::take(&mut self.bytes)),
            );
        self.closed = true;
        Poll::Ready(Ok(()))
    }

    fn abort(&mut self) {
        self.bytes.clear();
        self.closed = true;
    }
}
