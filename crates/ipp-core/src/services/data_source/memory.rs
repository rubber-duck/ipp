//! Immutable input and atomic bounded memory publication.

use super::{DataReadOptions, DataReader, DataSource, DataWriter};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    task::{Context, Poll},
};

/// In-memory data source usable independently of assets.
#[derive(Clone, Default)]
pub struct MemoryDataSource {
    values: Rc<RefCell<BTreeMap<String, Rc<Vec<u8>>>>>,
    writable: bool,
}

impl MemoryDataSource {
    /// Construct an empty source with an explicit write capability.
    pub fn new(writable: bool) -> Self {
        Self {
            writable,
            ..Self::default()
        }
    }

    /// Register already owned immutable bytes once.
    pub fn insert(&self, identifier: String, bytes: Vec<u8>) -> Result<(), String> {
        let mut values = self.values.borrow_mut();
        if values.contains_key(&identifier) {
            return Err("Duplicate data identifier".into());
        }
        values.insert(identifier, Rc::new(bytes));
        Ok(())
    }

    /// Remove a name; already opened readers retain their immutable input.
    pub(crate) fn remove(&self, identifier: &str) {
        self.values.borrow_mut().remove(identifier);
    }
}

struct MemorySourceReader {
    bytes: Rc<Vec<u8>>,
    offset: usize,
}

impl DataReader for MemorySourceReader {
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

impl DataSource for MemoryDataSource {
    fn list(&mut self, identifier: &str) -> Result<Vec<String>, String> {
        Ok(self
            .values
            .borrow()
            .keys()
            .filter(|key| key.starts_with(identifier))
            .cloned()
            .collect())
    }

    fn open_read(
        &mut self,
        identifier: &str,
        options: DataReadOptions,
    ) -> Result<Box<dyn DataReader>, String> {
        if options.recovery && self.writable {
            return Err("Mutable memory data has no immutable recovery validator".into());
        }
        let bytes = self
            .values
            .borrow()
            .get(identifier)
            .cloned()
            .ok_or("Data is unavailable")?;
        if options.max_bytes.is_some_and(|limit| bytes.len() > limit) {
            return Err("Data input byte budget exhausted".into());
        }
        Ok(Box::new(MemorySourceReader {
            bytes,
            offset: 0,
        }))
    }

    fn can_write(&self, _identifier: &str) -> bool {
        self.writable
    }

    fn open_write(
        &mut self,
        identifier: &str,
        max_bytes: usize,
    ) -> Result<Box<dyn DataWriter>, String> {
        if !self.writable {
            return Err("Data source is read-only".into());
        }
        Ok(Box::new(MemorySourceWriter {
            source: self.clone(),
            identifier: identifier.to_owned(),
            bytes: Vec::new(),
            limit: max_bytes,
            closed: false,
        }))
    }
}

struct MemorySourceWriter {
    source: MemoryDataSource,
    identifier: String,
    bytes: Vec<u8>,
    limit: usize,
    closed: bool,
}

impl DataWriter for MemorySourceWriter {
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
        self.source.values.borrow_mut().insert(
            self.identifier.clone(),
            Rc::new(std::mem::take(&mut self.bytes)),
        );
        self.closed = true;
        Poll::Ready(Ok(()))
    }

    fn abort(&mut self) {
        self.bytes.clear();
        self.closed = true;
    }
}
