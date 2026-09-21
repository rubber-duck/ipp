//! Concrete lifecycle owner; payload implementations belong to their consumers.

use super::{
    Asset, AssetKey, AssetLoadProgress, AssetLoadStatus, AssetLoader, AssetSource, AssetStats,
    bounded_error,
};
use crate::services::data_source::{DataReadOptions, DataReader, DataSourceManagementService};
use std::{
    rc::Rc,
    task::{Context, Poll},
};

pub(super) trait ErasedAssetLoader {
    fn take_failed_data(&mut self) -> Option<Box<dyn Asset>>;

    fn poll_load(
        &mut self,
        reader: &mut dyn DataReader,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Box<dyn Asset>, String>>;
}

pub(super) struct TypedAssetLoader<L>(pub L);

impl<L: AssetLoader> ErasedAssetLoader for TypedAssetLoader<L> {
    fn take_failed_data(&mut self) -> Option<Box<dyn Asset>> {
        self.0
            .take_failed_data()
            .map(|data| Box::new(data) as Box<dyn Asset>)
    }

    fn poll_load(
        &mut self,
        reader: &mut dyn DataReader,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Box<dyn Asset>, String>> {
        self.0
            .poll_load(reader, cx)
            .map(|result| result.map(|data| Box::new(data) as Box<dyn Asset>))
    }
}

pub(super) type AssetLoaderConstructor = Rc<dyn Fn() -> Box<dyn ErasedAssetLoader>>;

/// Authoritative identity, readiness, cancellation and immutable recovery owner.
pub struct AssetProvider {
    key: AssetKey,
    source: AssetSource,
    make_loader: AssetLoaderConstructor,
    status: AssetLoadStatus,
    reader: Option<CountingDataReader>,
    loader: Option<Box<dyn ErasedAssetLoader>>,
    data: Option<Box<dyn Asset>>,
    source_bytes: u64,
    recovery: bool,
    source_registration: Option<crate::services::data_source::DataSourceRegistrationId>,
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
            reader: None,
            loader: None,
            data: None,
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
        self.reader.as_ref()?.reader.request_id()
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

    pub(super) fn poll_load(
        &mut self,
        sources: &mut DataSourceManagementService,
        cx: &mut Context<'_>,
        events: &mut Vec<AssetLoadProgress>,
    ) {
        if matches!(
            self.status,
            AssetLoadStatus::Loaded | AssetLoadStatus::Failed(_)
        ) {
            return;
        }
        if self.loader.is_none() {
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
            match sources.open_read(
                identifier,
                DataReadOptions {
                    max_bytes: None,
                    recovery: self.recovery,
                },
            ) {
                Ok(reader) => {
                    self.source_registration = registration;
                    self.reader = Some(CountingDataReader {
                        reader,
                        bytes: 0,
                    });
                    self.loader = Some((self.make_loader)());
                }
                Err(error) => {
                    self.report(AssetLoadStatus::Failed(error), events);
                    return;
                }
            }
        }

        let reader = self.reader.as_mut().expect("active loader has reader");
        let result = self
            .loader
            .as_mut()
            .expect("active loader")
            .poll_load(reader, cx);
        let bytes = reader.bytes;
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
        if let Poll::Ready(result) = result {
            self.reader = None;
            if result.is_err() {
                let failed = self
                    .loader
                    .as_mut()
                    .and_then(|loader| loader.take_failed_data());
                if self.data.is_none() {
                    self.data = failed;
                }
                self.recovery |= self.data.is_some();
            }
            self.loader = None;
            match result {
                Ok(data) => {
                    self.data = Some(data);
                    self.recovery = true;
                    self.report(AssetLoadStatus::Loaded, events);
                }
                Err(error) => self.report(AssetLoadStatus::Failed(error), events),
            }
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
        self.reader = None;
        self.loader = None;
        if let Some(data) = self.data.as_mut() {
            data.invalidate_graphics();
        }
        if self.status != AssetLoadStatus::Unloaded {
            self.report(AssetLoadStatus::Unloaded, events);
        }
    }

    pub(super) fn unload(&mut self, events: &mut Vec<AssetLoadProgress>) {
        self.reader = None;
        self.loader = None;
        self.data = None;
        self.source_bytes = 0;
        if self.status != AssetLoadStatus::Unloaded {
            self.report(AssetLoadStatus::Unloaded, events);
        }
    }
}

struct CountingDataReader {
    reader: Box<dyn DataReader>,
    bytes: u64,
}

impl DataReader for CountingDataReader {
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<Result<usize, String>> {
        match self.reader.poll_read(cx, output) {
            Poll::Ready(Ok(n)) => {
                if n > output.len() {
                    return Poll::Ready(Err("Data reader returned an invalid byte count".into()));
                }
                self.bytes = self.bytes.saturating_add(n as u64);
                Poll::Ready(Ok(n))
            }
            result => result,
        }
    }
}
