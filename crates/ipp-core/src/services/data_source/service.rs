use super::*;

/// Host-owned literal prefix routing and private asynchronous input delivery.
#[derive(Default)]
pub struct DataSourceManagementService {
    sources: BTreeMap<String, DataSourceRegistration>,
    next_registration: u64,
    streams: reader::DataStreamProvider,
    pending_input: BTreeMap<u64, (Vec<u8>, usize)>,
}

type DataReaderCell = RefCell<Option<Box<dyn DataReader>>>;
type RegisteredDataReader = Rc<DataReaderCell>;
type DataWriterCell = RefCell<Option<Box<dyn DataWriter>>>;
type RegisteredDataWriter = Rc<DataWriterCell>;

struct DataSourceRegistration {
    id: DataSourceRegistrationId,
    source: Box<dyn DataSource>,
    readers: Vec<Weak<DataReaderCell>>,
    writers: Vec<Weak<DataWriterCell>>,
}

struct SourceDataReader(RegisteredDataReader);

impl DataReader for SourceDataReader {
    fn request_id(&self) -> Option<u64> {
        self.0.borrow().as_ref()?.request_id()
    }

    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<Result<usize, String>> {
        self.0.borrow_mut().as_mut().map_or_else(
            || Poll::Ready(Err("Data source registration was removed".into())),
            |reader| reader.poll_read(cx, output),
        )
    }
}

struct SourceDataWriter(RegisteredDataWriter);

impl DataWriter for SourceDataWriter {
    fn poll_write(&mut self, cx: &mut Context<'_>, input: &[u8]) -> Poll<Result<usize, String>> {
        self.0.borrow_mut().as_mut().map_or_else(
            || Poll::Ready(Err("Data source registration was removed".into())),
            |writer| writer.poll_write(cx, input),
        )
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        self.0.borrow_mut().as_mut().map_or_else(
            || Poll::Ready(Err("Data source registration was removed".into())),
            |writer| writer.poll_flush(cx),
        )
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        self.0.borrow_mut().as_mut().map_or_else(
            || Poll::Ready(Err("Data source registration was removed".into())),
            |writer| writer.poll_finish(cx),
        )
    }

    fn abort(&mut self) {
        if let Some(mut writer) = self.0.borrow_mut().take() {
            writer.abort();
        }
    }
}

impl Drop for SourceDataWriter {
    fn drop(&mut self) {
        self.abort();
    }
}

impl DataSourceManagementService {
    /// Construct an empty routing table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a disjoint literal prefix. No URI parsing or rewriting occurs.
    pub fn register(
        &mut self,
        prefix: &str,
        source: impl DataSource + 'static,
    ) -> Result<(), String> {
        if self
            .sources
            .keys()
            .any(|other| other.starts_with(prefix) || prefix.starts_with(other))
        {
            return Err("Data source prefixes overlap".into());
        }
        let next = self
            .next_registration
            .checked_add(1)
            .ok_or("Data source registration identity exhausted")?;
        self.sources.insert(
            prefix.to_owned(),
            DataSourceRegistration {
                id: DataSourceRegistrationId(next),
                source: Box::new(source),
                readers: Vec::new(),
                writers: Vec::new(),
            },
        );
        self.next_registration = next;
        Ok(())
    }

    /// Resolve the exact source incarnation before retaining immutable recovery state.
    pub fn registration_id(&self, identifier: &str) -> Option<DataSourceRegistrationId> {
        self.sources
            .iter()
            .find(|(prefix, _)| identifier.starts_with(prefix.as_str()))
            .map(|(_, source)| source.id)
    }

    /// Register a source whose I/O is supplied asynchronously by the Host.
    pub fn register_stream(&mut self, prefix: &str) -> Result<(), String> {
        self.register(prefix, self.streams.clone())
    }

    /// Remove a registration and synchronously cancel its still-live I/O.
    pub fn unregister(&mut self, prefix: &str) -> bool {
        let Some(source) = self.sources.remove(prefix) else {
            return false;
        };
        for reader in source.readers {
            if let Some(reader) = reader.upgrade() {
                reader.borrow_mut().take();
            }
        }
        for writer in source.writers {
            if let Some(writer) = writer.upgrade()
                && let Some(mut writer) = writer.borrow_mut().take()
            {
                writer.abort();
            }
        }
        self.pending_input.retain(|id, _| {
            self.streams
                .writer(*id)
                .is_some_and(|writer| writer.is_open())
        });
        true
    }

    fn source_mut(&mut self, identifier: &str) -> Result<&mut DataSourceRegistration, String> {
        for (prefix, source) in &mut self.sources {
            if identifier.starts_with(prefix) {
                return Ok(source);
            }
        }
        Err(format!("Data source is unavailable for {identifier}"))
    }

    /// Enumerate through the selected source, forwarding the complete identifier.
    pub fn list(&mut self, identifier: &str) -> Result<Vec<String>, String> {
        self.source_mut(identifier)?.source.list(identifier)
    }

    /// Open input without any asset object or type registration.
    pub fn open_read(
        &mut self,
        identifier: &str,
        options: DataReadOptions,
    ) -> Result<Box<dyn DataReader>, String> {
        let source = self.source_mut(identifier)?;
        source.readers.retain(|reader| reader.strong_count() != 0);
        let reader = Rc::new(RefCell::new(Some(
            source.source.open_read(identifier, options)?,
        )));
        source.readers.push(Rc::downgrade(&reader));
        Ok(Box::new(SourceDataReader(reader)))
    }

    /// Query destination capability without opening output.
    pub fn can_write(&self, identifier: &str) -> bool {
        self.sources
            .iter()
            .find(|(prefix, _)| identifier.starts_with(prefix.as_str()))
            .is_some_and(|(_, source)| source.source.can_write(identifier))
    }

    /// Open output only when the selected source grants write access.
    pub fn open_write(
        &mut self,
        identifier: &str,
        max_bytes: usize,
    ) -> Result<Box<dyn DataWriter>, String> {
        let source = self.source_mut(identifier)?;
        if !source.source.can_write(identifier) {
            return Err("Data source is read-only".into());
        }
        source.writers.retain(|writer| writer.strong_count() != 0);
        let writer = Rc::new(RefCell::new(Some(
            source.source.open_write(identifier, max_bytes)?,
        )));
        source.writers.push(Rc::downgrade(&writer));
        Ok(Box::new(SourceDataWriter(writer)))
    }

    /// Drain newly opened asynchronous reads in opening order.
    pub fn take_requests(&self) -> Vec<DataReadRequest> {
        self.take_selected_requests(|_| true)
    }

    pub(crate) fn take_selected_requests(
        &self,
        selected: impl FnMut(u64) -> bool,
    ) -> Vec<DataReadRequest> {
        self.streams
            .take_requests(selected)
            .into_iter()
            .map(|request| DataReadRequest {
                id: request.id,
                identifier: request.source,
                max_bytes: request.max_bytes,
                recovery: request.recovery,
            })
            .collect()
    }

    /// Drain work whose originating readers have been dropped.
    pub fn take_cancellations(&self) -> Vec<u64> {
        self.streams.take_closed()
    }

    /// Feed one bounded chunk. False requests retry after consumer progress.
    pub fn input_chunk(&self, id: u64, bytes: &[u8]) -> Result<bool, String> {
        self.streams
            .writer(id)
            .map_or(Ok(true), |writer| writer.push(bytes))
    }

    /// Finish the exact reader; stale completion cannot reach a later operation.
    pub fn input_end(&self, id: u64, result: Result<(), String>) {
        if let Some(writer) = self.streams.writer(id) {
            writer.finish(result);
        }
    }

    /// Retained asynchronous input staging.
    pub fn input_bytes(&self) -> usize {
        self.streams.buffered_bytes()
    }

    /// Accept a complete owned Host result for its originating reader.
    pub fn complete_read(
        &mut self,
        id: u64,
        result: Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        let Some(writer) = self
            .streams
            .writer(id)
            .filter(reader::DataStreamWriter::is_open)
        else {
            return Ok(());
        };
        if self.pending_input.contains_key(&id) {
            return Ok(());
        }
        match result {
            Err(error) => {
                if error.len() > 2048 {
                    return Err("Data error byte budget exhausted".into());
                }
                writer.finish(Err(error));
            }
            Ok(bytes) => {
                if writer.max_bytes().is_some_and(|limit| bytes.len() > limit) {
                    return Err("Data input byte budget exhausted".into());
                }
                self.pending_input.insert(id, (bytes, 0));
            }
        }
        Ok(())
    }

    /// Progress retained input at the Host service boundary.
    pub fn progress(&mut self) {
        self.pending_input.retain(|id, (bytes, offset)| {
            let Some(writer) = self
                .streams
                .writer(*id)
                .filter(reader::DataStreamWriter::is_open)
            else {
                return false;
            };
            let end = bytes.len().min(*offset + STREAM_CAPACITY);
            match writer.push(&bytes[*offset..end]) {
                Ok(true) => *offset = end,
                Ok(false) => return true,
                Err(error) => {
                    writer.finish(Err(error));
                    return false;
                }
            }
            if *offset == bytes.len() {
                writer.finish(Ok(()));
                false
            } else {
                true
            }
        });
    }
}

impl Drop for DataSourceManagementService {
    fn drop(&mut self) {
        while let Some(prefix) = self.sources.keys().next().cloned() {
            self.unregister(&prefix);
        }
    }
}
