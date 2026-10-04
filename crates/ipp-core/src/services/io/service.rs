//! Literal source routing with source-incarnation cancellation fences.

use super::*;
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

/// Host-owned routing; asynchronous operations capture registrations before returning.
#[derive(Default)]
pub struct IoService {
    sources: BTreeMap<String, IoSourceRegistration>,
    next_registration: u64,
    streams: stream_source::IoStreamProvider,
}

struct IoRegistrationLifetime {
    revoked: IoCancellation,
    acquisitions: Mutex<Vec<IoCancellation>>,
}

impl IoRegistrationLifetime {
    fn attach(&self, cancellation: Option<IoCancellation>) {
        if let Some(cancellation) = cancellation {
            let mut acquisitions = self.acquisitions.lock().expect("IO registration lock");
            acquisitions.retain(|active| !active.is_cancelled());
            if self.revoked.is_cancelled() {
                cancellation.cancel();
            } else {
                acquisitions.push(cancellation);
            }
        }
    }

    fn revoke(&self) {
        self.revoked.cancel();
        let acquisitions =
            std::mem::take(&mut *self.acquisitions.lock().expect("IO registration lock"));
        for acquisition in acquisitions {
            acquisition.cancel();
        }
    }
}

struct IoSourceRegistration {
    id: IoSourceRegistrationId,
    source: Box<dyn IoSource>,
    lifetime: Arc<IoRegistrationLifetime>,
}

struct SourceIoReader {
    reader: Box<dyn IoReader>,
    lifetime: Arc<IoRegistrationLifetime>,
    revocation_waiter: super::IoCancellationWaiter,
}

impl IoReadBackend for SourceIoReader {
    fn register_storage_waker(&mut self, waker: &std::task::Waker) {
        self.reader.register_storage_waker(waker);
    }

    fn retained_storage(&self) -> Option<crate::services::io::IoReaderStorage> {
        self.reader.retained_storage()
    }

    fn request_id(&self) -> Option<u64> {
        self.reader.request_id()
    }

    fn cancellation(&self) -> Option<IoCancellation> {
        self.reader.cancellation()
    }

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
        minimum: NonZeroUsize,
    ) -> Poll<Result<(), IoError>> {
        self.revocation_waiter.register(cx.waker());
        if self.lifetime.revoked.is_cancelled() {
            return Poll::Ready(Err("Data source registration was removed".into()));
        }
        self.reader.poll_ready(cx, minimum)
    }

    fn window(&mut self) -> IoReadWindow<'_> {
        self.reader.window()
    }
}

struct SourceIoWriter {
    writer: Box<dyn IoWriter>,
    lifetime: Arc<IoRegistrationLifetime>,
    revocation_waiter: super::IoCancellationWaiter,
}

impl SourceIoWriter {
    fn live(&mut self, cx: &Context<'_>) -> Result<(), IoError> {
        self.revocation_waiter.register(cx.waker());
        if self.lifetime.revoked.is_cancelled() {
            self.writer.abort();
            Err("Data source registration was removed".into())
        } else {
            Ok(())
        }
    }
}

impl IoWriteBackend for SourceIoWriter {
    fn poll_write(&mut self, cx: &mut Context<'_>, input: &[u8]) -> Poll<Result<usize, IoError>> {
        if let Err(error) = self.live(cx) {
            return Poll::Ready(Err(error));
        }
        self.writer.poll_write(cx, input)
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        if let Err(error) = self.live(cx) {
            return Poll::Ready(Err(error));
        }
        self.writer.poll_flush(cx)
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        if let Err(error) = self.live(cx) {
            return Poll::Ready(Err(error));
        }
        self.writer.poll_finish(cx)
    }

    fn abort(&mut self) {
        self.writer.abort();
    }
}

impl Drop for SourceIoWriter {
    fn drop(&mut self) {
        self.writer.abort();
    }
}

struct SourceIoListing {
    listing: Box<dyn IoListing>,
    lifetime: Arc<IoRegistrationLifetime>,
    revocation_waiter: super::IoCancellationWaiter,
}

impl IoListingBackend for SourceIoListing {
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<String>, IoError>> {
        self.revocation_waiter.register(cx.waker());
        if self.lifetime.revoked.is_cancelled() {
            Poll::Ready(Err("Data source registration was removed".into()))
        } else {
            self.listing.poll_next(cx)
        }
    }
}

async fn await_registration<T>(
    mut operation: IoOperation<T>,
    revoked: IoCancellation,
) -> Result<T, IoError> {
    let waiter = revoked.waiter();
    std::future::poll_fn(|cx| {
        waiter.register(cx.waker());
        if revoked.is_cancelled() {
            Poll::Ready(Err("Data source registration was removed".into()))
        } else {
            operation.as_mut().poll(cx)
        }
    })
    .await
}

impl IoService {
    /// Construct an empty routing table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a disjoint literal prefix; generic routing never interprets identifiers.
    pub fn register(
        &mut self,
        prefix: &str,
        source: impl IoSource + 'static,
    ) -> Result<(), IoError> {
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
            IoSourceRegistration {
                id: IoSourceRegistrationId(next),
                source: Box::new(source),
                lifetime: Arc::new(IoRegistrationLifetime {
                    revoked: IoCancellation::default(),
                    acquisitions: Mutex::default(),
                }),
            },
        );
        self.next_registration = next;
        Ok(())
    }

    /// Resolve the current exact registration for immutable recovery validation.
    pub fn registration_id(&self, identifier: &str) -> Option<IoSourceRegistrationId> {
        self.sources
            .iter()
            .find(|(prefix, _)| identifier.starts_with(prefix.as_str()))
            .map(|(_, source)| source.id)
    }

    /// Capture availability for one exact routing registration. This fence is
    /// cancelled on removal; it never tracks a replacement registration.
    pub fn registration_cancellation(
        &self,
        identifier: &str,
        registration: IoSourceRegistrationId,
    ) -> Option<IoCancellation> {
        self.sources
            .iter()
            .find(|(prefix, source)| {
                identifier.starts_with(prefix.as_str()) && source.id == registration
            })
            .map(|(_, source)| source.lifetime.revoked.clone())
    }

    /// Register source acquisition supplied by a Host platform bridge.
    pub fn register_stream(&mut self, prefix: &str) -> Result<(), IoError> {
        self.register(prefix, self.streams.clone())
    }

    /// Revoke acquisition immediately while preserving backing borrowed by active windows.
    pub fn unregister(&mut self, prefix: &str) -> bool {
        let Some(source) = self.sources.remove(prefix) else {
            return false;
        };
        source.lifetime.revoke();
        true
    }

    fn source_mut(&mut self, identifier: &str) -> Result<&mut IoSourceRegistration, IoError> {
        self.sources
            .iter_mut()
            .find(|(prefix, _)| identifier.starts_with(prefix.as_str()))
            .map(|(_, source)| source)
            .ok_or_else(|| format!("Data source is unavailable for {identifier}"))
    }

    /// Capture registration synchronously, then await an incremental listing.
    pub fn list(&mut self, identifier: &str) -> IoListFuture {
        let source = match self.source_mut(identifier) {
            Ok(source) => source,
            Err(error) => return Box::pin(std::future::ready(Err(error))),
        };
        let operation = source.source.list(identifier);
        let lifetime = source.lifetime.clone();
        Box::pin(async move {
            let listing = await_registration(operation, lifetime.revoked.clone()).await?;
            if lifetime.revoked.is_cancelled() {
                return Err("Data source registration was removed".into());
            }
            Ok(Box::new(SourceIoListing {
                listing,
                revocation_waiter: lifetime.revoked.waiter(),
                lifetime,
            }) as Box<dyn IoListing>)
        })
    }

    /// Capture exact registration at this call, before any asynchronous suspension.
    pub fn open_read(&mut self, identifier: &str, options: IoReadOptions) -> IoOpenReadFuture {
        let source = match self.source_mut(identifier) {
            Ok(source) => source,
            Err(error) => return Box::pin(std::future::ready(Err(error))),
        };
        let operation = source.source.open_read(identifier, options);
        let lifetime = source.lifetime.clone();
        Box::pin(async move {
            let reader = await_registration(operation, lifetime.revoked.clone()).await?;
            if lifetime.revoked.is_cancelled() {
                return Err("Data source registration was removed".into());
            }
            lifetime.attach(reader.cancellation());
            Ok(Box::new(SourceIoReader {
                reader,
                revocation_waiter: lifetime.revoked.waiter(),
                lifetime,
            }) as Box<dyn IoReader>)
        })
    }

    /// Open only the granted exact source registration. Capture happens at this
    /// call; a replacement can neither inherit the grant nor retarget pending work.
    /// Provider-local names remain immutable by their publication contract.
    pub fn open_read_registered(
        &mut self,
        identifier: &str,
        registration: IoSourceRegistrationId,
        options: IoReadOptions,
        grant: IoCancellation,
    ) -> IoOpenReadFuture {
        if self.registration_id(identifier) != Some(registration) {
            return Box::pin(std::future::ready(Err(
                "Granted source registration changed".into(),
            )));
        }
        let mut operation = self.open_read(identifier, options);
        Box::pin(async move {
            let waiter = grant.waiter();
            let reader = std::future::poll_fn(|cx| {
                waiter.register(cx.waker());
                if grant.is_cancelled() {
                    Poll::Ready(Err("Source read grant was revoked".into()))
                } else {
                    operation.as_mut().poll(cx)
                }
            })
            .await?;
            if let Some(acquisition) = reader.cancellation() {
                grant.link(&acquisition);
            }
            Ok(Box::new(GrantedIoReader {
                reader,
                grant,
                waiter,
            }) as Box<dyn IoReader>)
        })
    }

    /// Query explicit destination capability without opening output.
    pub fn can_write(&self, identifier: &str) -> bool {
        self.sources
            .iter()
            .find(|(prefix, _)| identifier.starts_with(prefix.as_str()))
            .is_some_and(|(_, source)| source.source.can_write(identifier))
    }

    /// Capture destination capability and registration before asynchronous publication work.
    pub fn open_write(&mut self, identifier: &str, max_bytes: usize) -> IoOpenWriteFuture {
        let source = match self.source_mut(identifier) {
            Ok(source) => source,
            Err(error) => return Box::pin(std::future::ready(Err(error))),
        };
        if !source.source.can_write(identifier) {
            return Box::pin(std::future::ready(Err("Data source is read-only".into())));
        }
        let operation = source.source.open_write(identifier, max_bytes);
        let lifetime = source.lifetime.clone();
        Box::pin(async move {
            let mut writer = await_registration(operation, lifetime.revoked.clone()).await?;
            if lifetime.revoked.is_cancelled() {
                writer.abort();
                return Err("Data source registration was removed".into());
            }
            Ok(Box::new(SourceIoWriter {
                writer,
                revocation_waiter: lifetime.revoked.waiter(),
                lifetime,
            }) as Box<dyn IoWriter>)
        })
    }

    /// Drain platform acquisitions in opening order.
    pub fn take_requests(&self) -> Vec<IoReadRequest> {
        self.take_selected_requests(|_| true)
    }

    pub(crate) fn take_selected_requests(
        &self,
        selected: impl FnMut(u64) -> bool,
    ) -> Vec<IoReadRequest> {
        self.streams.take_requests(selected)
    }

    /// Drain cancelled acquisitions, preserving successful consumption as completion.
    pub fn take_cancellations(&self) -> Vec<u64> {
        self.streams.take_closed()
    }

    /// Reserve exact reader storage before crossing the platform memory boundary.
    pub fn reserve_input(
        &self,
        id: u64,
        length: usize,
    ) -> Result<Option<IoInputReservation>, IoError> {
        self.streams
            .input(id)
            .map_or(Ok(None), |input| input.reserve(length))
    }

    /// Wait for exact acquisition capacity, registering the producer wakeup.
    pub fn poll_reserve_input(
        &self,
        id: u64,
        length: usize,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<IoInputReservation>, IoError>> {
        let Some(input) = self.streams.input(id) else {
            return Poll::Ready(Ok(None));
        };
        input
            .poll_reserve(cx, length)
            .map(|result| result.map(Some))
    }

    /// Feed one admitted chunk; false requests retry with no input copy performed.
    pub fn input_chunk(&self, id: u64, bytes: &[u8]) -> Result<bool, IoError> {
        self.streams
            .input(id)
            .map_or(Ok(true), |input| input.push(bytes))
    }

    /// Finish this exact acquisition; stale identities never target new readers.
    pub fn input_end(&self, id: u64, result: Result<(), IoError>) {
        if let Some(input) = self.streams.input(id) {
            input.finish(result);
        }
    }

    /// Retained stream storage, including outstanding windows and fill leases.
    pub fn input_bytes(&self) -> usize {
        self.streams.buffered_bytes()
    }

    /// Adopt a complete owned Host result directly, without a second staging queue.
    pub fn complete_read(
        &mut self,
        id: u64,
        result: Result<Vec<u8>, IoError>,
    ) -> Result<(), IoError> {
        let Some(input) = self.streams.input(id) else {
            return Ok(());
        };
        match result {
            Ok(bytes) => input.adopt_complete(bytes),
            Err(error) => {
                input.finish(Err(error));
                Ok(())
            }
        }
    }
}

impl Drop for IoService {
    fn drop(&mut self) {
        while let Some(prefix) = self.sources.keys().next().cloned() {
            self.unregister(&prefix);
        }
    }
}

struct GrantedIoReader {
    reader: Box<dyn IoReader>,
    grant: IoCancellation,
    waiter: super::IoCancellationWaiter,
}

impl IoReadBackend for GrantedIoReader {
    fn register_storage_waker(&mut self, waker: &std::task::Waker) {
        self.reader.register_storage_waker(waker);
    }

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
        minimum: std::num::NonZeroUsize,
    ) -> Poll<Result<(), IoError>> {
        self.waiter.register(cx.waker());
        if self.grant.is_cancelled() {
            Poll::Ready(Err("Source read grant was revoked".into()))
        } else {
            self.reader.poll_ready(cx, minimum)
        }
    }

    fn window(&mut self) -> IoReadWindow<'_> {
        self.reader.window()
    }

    fn retained_storage(&self) -> Option<super::IoReaderStorage> {
        self.reader.retained_storage()
    }

    fn request_id(&self) -> Option<u64> {
        self.reader.request_id()
    }

    fn cancellation(&self) -> Option<IoCancellation> {
        self.reader.cancellation()
    }
}
