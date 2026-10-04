//! Concrete lifecycle owner; payload implementations belong to their consumers.

use super::{
    Asset, AssetKey, AssetLoadProgress, AssetLoadScheduler, AssetLoadStatus, AssetLoadTask,
    AssetLoader, AssetSource, AssetStats, bounded_error,
};
use crate::services::io::{
    IoCancellation, IoReadBackend, IoReadOptions, IoReadWindow, IoReader, IoService,
};
use std::{
    cell::RefCell,
    future::poll_fn,
    num::NonZeroUsize,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};

pub(super) trait ErasedAssetLoader {
    fn take_failed_data(&mut self) -> Option<Box<dyn Asset>>;

    fn start_load(&mut self, reader: Box<dyn IoReader>) -> Result<(), String>;

    fn poll_load(&mut self, cx: &mut Context<'_>) -> Poll<Result<Box<dyn Asset>, String>>;
}

pub(super) struct TypedAssetLoader<L>(pub L);

impl<L: AssetLoader> ErasedAssetLoader for TypedAssetLoader<L> {
    fn take_failed_data(&mut self) -> Option<Box<dyn Asset>> {
        self.0
            .take_failed_data()
            .map(super::export::shared_cpu_data)
    }

    fn start_load(&mut self, reader: Box<dyn IoReader>) -> Result<(), String> {
        self.0.start_load(reader)
    }

    fn poll_load(&mut self, cx: &mut Context<'_>) -> Poll<Result<Box<dyn Asset>, String>> {
        self.0
            .poll_load(cx)
            .map(|result| result.map(super::export::shared_cpu_data))
    }
}

pub(super) type AssetLoaderConstructor = Rc<dyn Fn() -> Box<dyn ErasedAssetLoader>>;

/// Authoritative identity, readiness, cancellation and immutable recovery owner.
pub struct AssetProvider {
    key: AssetKey,
    source: AssetSource,
    make_loader: AssetLoaderConstructor,
    status: AssetLoadStatus,
    task: Option<Box<dyn AssetLoadTask>>,
    completion: Option<Rc<RefCell<Option<AssetLoadCompletion>>>>,
    consumed: Arc<AtomicU64>,
    request: Arc<AtomicU64>,
    data: Option<Box<dyn Asset>>,
    working_availability: super::export::AssetWorkingAvailability,
    source_bytes: u64,
    recovery: bool,
    source_registration: Option<crate::services::io::IoSourceRegistrationId>,
    pub(super) owned_input: Option<String>,
}

impl AssetProvider {
    pub(super) fn new(
        key: AssetKey,
        source: AssetSource,
        make_loader: AssetLoaderConstructor,
    ) -> Self {
        Self {
            key,
            source,
            make_loader,
            status: AssetLoadStatus::Unloaded,
            task: None,
            completion: None,
            consumed: Arc::new(AtomicU64::new(0)),
            request: Arc::new(AtomicU64::new(0)),
            data: None,
            working_availability: Default::default(),
            source_bytes: 0,
            recovery: false,
            source_registration: None,
            owned_input: None,
        }
    }

    /// Slot identity unchanged by unload and recovery.
    pub fn key(&self) -> AssetKey {
        self.key
    }

    /// Immutable typed source identity, independent of the slot.
    pub fn source(&self) -> &AssetSource {
        &self.source
    }

    /// Authoritative resource availability.
    pub fn status(&self) -> &AssetLoadStatus {
        &self.status
    }

    /// Reopening must reproduce previously accepted content.
    pub fn requires_recovery(&self) -> bool {
        self.recovery
    }

    /// Borrow loaded data only while the provider remains exclusively protected.
    pub fn data(&self) -> Option<&dyn Asset> {
        self.data.as_deref()
    }

    /// Snapshot the exact immutable CPU data without retaining original input.
    pub fn cpu_export_snapshot(&self) -> Option<super::export::AssetCpuSnapshot> {
        Some(super::export::AssetCpuSnapshot {
            data: self.data()?.cpu_snapshot()?,
            available: self.working_availability.cpu.clone(),
        })
    }

    /// Capture the working GPU generation independently of CPU availability.
    pub fn gpu_export_availability(&self) -> crate::services::io::IoCancellation {
        self.working_availability.gpu.clone()
    }

    /// Host-private I/O routing identity for this immutable typed source.
    pub fn read_identifier(&self) -> &str {
        self.owned_input.as_deref().unwrap_or(&self.source.uri)
    }

    /// Current input and retained allocation accounting.
    pub fn representation(&self) -> super::AssetRepresentationStatus {
        super::AssetRepresentationStatus {
            decoded: self.decoded_available(),
            graphics_ready: self.graphics_ready(),
            source_bytes: self.source_bytes,
            resident_bytes: self.stats().resident_bytes as u64,
            graphics_bytes: self.graphics_bytes().map(|bytes| bytes as u64),
        }
    }

    /// Current measured allocation and source sizes.
    pub fn stats(&self) -> AssetStats {
        AssetStats {
            source_bytes: self.source_bytes,
            resident_bytes: self.data.as_ref().map_or(0, |data| data.resident_bytes()),
        }
    }

    pub(super) fn request_id(&self) -> Option<u64> {
        let request = self.request.load(Ordering::Relaxed);
        (request != 0).then_some(request)
    }

    pub(super) fn adopt_owned_input(&mut self, identifier: String) {
        self.owned_input = Some(identifier);
        self.recovery = false;
        self.source_registration = None;
    }

    fn report(&mut self, status: AssetLoadStatus, events: &mut Vec<AssetLoadProgress>) {
        let status = match status {
            AssetLoadStatus::Failed(error) => AssetLoadStatus::Failed(bounded_error(error)),
            status => status,
        };
        self.status = status.clone();
        events.push(AssetLoadProgress {
            representation: self.representation(),
            key: self.key,
            source: self.source.clone(),
            status,
        });
    }

    pub(super) fn progress_load(
        &mut self,
        sources: &mut IoService,
        scheduler: Option<&dyn AssetLoadScheduler>,
        events: &mut Vec<AssetLoadProgress>,
    ) {
        if matches!(
            self.status,
            AssetLoadStatus::Loaded | AssetLoadStatus::Failed(_)
        ) {
            return;
        }
        if self.task.is_none() {
            let Some(scheduler) = scheduler else {
                return;
            };
            self.report(AssetLoadStatus::Start, events);
            let identifier = self.owned_input.as_deref().unwrap_or(&self.source.uri);
            let registration = sources.registration_id(identifier);
            if self.recovery && registration != self.source_registration {
                self.report(
                    AssetLoadStatus::Failed(
                        "Immutable recovery source registration changed".into(),
                    ),
                    events,
                );
                return;
            }
            // Routing captures the registration now, before the owned task awaits.
            let open = sources.open_read(
                identifier,
                IoReadOptions {
                    max_bytes: None,
                    recovery: self.recovery,
                },
            );
            self.source_registration = registration;
            self.consumed = Arc::new(AtomicU64::new(0));
            self.request = Arc::new(AtomicU64::new(0));
            let consumed = self.consumed.clone();
            let request = self.request.clone();
            let completion = Rc::new(RefCell::new(None));
            let completed = completion.clone();
            let mut loader = (self.make_loader)();
            self.task = Some(scheduler.spawn(Box::pin(async move {
                let result = async {
                    let reader = open.await?;
                    request.store(reader.request_id().unwrap_or(0), Ordering::Relaxed);
                    let reader = Box::new(CountingIoReader {
                        reader,
                        bytes: consumed,
                    }) as Box<dyn IoReader>;
                    loader.start_load(reader)?;
                    poll_fn(|cx| loader.poll_load(cx)).await
                }
                .await;
                let failed_data = if result.is_err() {
                    loader.take_failed_data()
                } else {
                    None
                };
                request.store(0, Ordering::Relaxed);
                *completed.borrow_mut() = Some(AssetLoadCompletion {
                    result,
                    failed_data,
                });
            })));
            self.completion = Some(completion);
        }
        let bytes = self.consumed.load(Ordering::Relaxed);
        if bytes != self.source_bytes {
            self.source_bytes = bytes;
            self.report(
                AssetLoadStatus::Progress {
                    completed: bytes,
                    total: None,
                },
                events,
            );
        }
        let completed = self
            .completion
            .as_ref()
            .and_then(|completion| completion.borrow_mut().take());
        let Some(completed) = completed else {
            return;
        };
        self.task = None;
        self.completion = None;
        if completed.result.is_err() {
            if self.data.is_none() {
                self.working_availability.invalidate_all();
                self.data = completed.failed_data;
            }
            self.recovery |= self.data.is_some();
        }
        match completed.result {
            Ok(data) => {
                // Reopening the same immutable source to recover graphics does not
                // revoke an independent retained CPU consumer.
                if self
                    .data
                    .as_ref()
                    .is_some_and(|data| data.cpu_snapshot().is_some())
                {
                    self.working_availability.invalidate_gpu();
                } else {
                    self.working_availability.invalidate_all();
                }
                self.data = Some(data);
                self.recovery = true;
                self.report(AssetLoadStatus::Loaded, events);
            }
            Err(error) => self.report(AssetLoadStatus::Failed(error), events),
        }
    }

    /// CPU metadata/decoded values remain available while graphics are recovering.
    pub fn decoded_available(&self) -> bool {
        self.data.is_some()
    }

    /// Availability of the optional graphics representation.
    pub fn graphics_ready(&self) -> Option<bool> {
        self.data.as_ref().and_then(|data| data.graphics_ready())
    }

    /// Known graphics allocation estimate, excluding opaque driver storage.
    pub fn graphics_bytes(&self) -> Option<usize> {
        self.data.as_ref().and_then(|data| data.graphics_bytes())
    }

    pub(super) fn invalidate_graphics(&mut self, events: &mut Vec<AssetLoadProgress>) {
        self.working_availability.invalidate_gpu();
        self.task = None;
        self.completion = None;
        self.request.store(0, Ordering::Relaxed);
        if let Some(data) = self.data.as_mut() {
            data.invalidate_graphics();
        }
        if self.status != AssetLoadStatus::Unloaded {
            self.report(AssetLoadStatus::Unloaded, events);
        }
    }

    pub(super) fn unload(&mut self, events: &mut Vec<AssetLoadProgress>) {
        self.working_availability.invalidate_all();
        self.task = None;
        self.completion = None;
        self.request.store(0, Ordering::Relaxed);
        self.data = None;
        self.source_bytes = 0;
        if self.status != AssetLoadStatus::Unloaded {
            self.report(AssetLoadStatus::Unloaded, events);
        }
    }
}

impl Drop for AssetProvider {
    fn drop(&mut self) {
        self.working_availability.cpu.cancel();
        self.working_availability.gpu.cancel();
    }
}

struct AssetLoadCompletion {
    result: Result<Box<dyn Asset>, String>,
    failed_data: Option<Box<dyn Asset>>,
}

struct CountingIoReader {
    reader: Box<dyn IoReader>,
    bytes: Arc<AtomicU64>,
}

impl IoReadBackend for CountingIoReader {
    fn register_storage_waker(&mut self, waker: &std::task::Waker) {
        self.reader.register_storage_waker(waker);
    }

    fn retained_storage(&self) -> Option<crate::services::io::IoReaderStorage> {
        self.reader.retained_storage()
    }

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
        minimum: NonZeroUsize,
    ) -> Poll<Result<(), String>> {
        self.reader.poll_ready(cx, minimum)
    }

    fn window(&mut self) -> IoReadWindow<'_> {
        self.reader.window().count_into(&self.bytes)
    }

    fn request_id(&self) -> Option<u64> {
        self.reader.request_id()
    }

    fn cancellation(&self) -> Option<IoCancellation> {
        self.reader.cancellation()
    }
}

#[cfg(test)]
#[path = "resource_export_tests.rs"]
mod export_tests;
