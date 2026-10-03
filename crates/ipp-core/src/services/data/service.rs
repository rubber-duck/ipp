use super::{
    DataAvailability, DataConsumerHandle, DataConsumerIdentity, DataConsumerNotification,
    DataConsumerRequest, DataConsumerState, DataError, DataProducerHandle, DataReadView,
    DataSchema, DataSourceHandle, DataSourceKind, DataWindow, DataWindowAnchor,
    consumer::DataConsumer, retention, source::DataSource,
};

/// Host policy; explicit windows bypass the default stream cap.
#[derive(Clone, Copy, Debug)]
pub struct DataServiceConfig {
    /// Newest contiguous sample suffix retained by consumers without explicit windows.
    pub default_stream_bytes: usize,
}

impl Default for DataServiceConfig {
    fn default() -> Self {
        Self {
            default_stream_bytes: 8 << 20,
        }
    }
}

/// Sole owner of source payload and mutation. It holds no component rows or asset references.
pub struct DataService {
    pub(super) identity: u64,
    pub(super) next_source: u64,
    pub(super) next_consumer: u64,
    pub(super) sources: Vec<DataSource>,
    pub(super) consumers: Vec<DataConsumer>,
    pub(super) config: DataServiceConfig,
    pub(super) time: f64,
    evaluating: bool,
    consumer_cleanup_deferred: bool,
    expiry_pending: bool,
}

impl Default for DataService {
    fn default() -> Self {
        Self::new()
    }
}

impl DataService {
    /// Construct an empty service with the default Host retention policy.
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let identity = NEXT
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |value| value.checked_add(1),
            )
            .expect("Data Service identity exhausted");
        Self {
            identity,
            next_source: 0,
            next_consumer: 0,
            sources: Vec::new(),
            consumers: Vec::new(),
            config: Default::default(),
            time: 0.0,
            evaluating: false,
            consumer_cleanup_deferred: false,
            expiry_pending: false,
        }
    }

    /// Observe the current Host retention policy.
    pub fn config(&self) -> DataServiceConfig {
        self.config
    }

    /// Change Host policy and expire unneeded samples immediately; history never returns.
    pub fn configure(&mut self, config: DataServiceConfig) {
        self.config = config;
        for consumer in &mut self.consumers {
            consumer.default_start = 0;
        }
        self.expire_all();
    }

    /// Observe Host elapsed seconds without advancing time or expiring rows.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Create or replace a detached source. A live producer must detach or destroy first.
    /// Schema changes always create a fresh incarnation, never mutate an old schema.
    pub fn create_source(
        &mut self,
        name: String,
        kind: DataSourceKind,
        schema: DataSchema,
    ) -> Result<DataProducerHandle, DataError> {
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(DataError::InvalidName);
        }
        schema.validate()?;
        let previous = self.sources.iter().position(|source| source.name == name);
        if previous.is_some_and(|index| self.sources[index].producer) {
            return Err(DataError::ProducerExists);
        }
        let incarnation = self.next_source.checked_add(1).ok_or(DataError::Capacity)?;
        let mut latest = Vec::new();
        latest
            .try_reserve_exact(schema.columns.len())
            .map_err(|_| DataError::Capacity)?;
        latest.resize(schema.columns.len(), None);
        if previous.is_none() {
            self.sources
                .try_reserve(1)
                .map_err(|_| DataError::Capacity)?;
        }
        let handle = DataSourceHandle {
            service: self.identity,
            incarnation,
        };
        let source = DataSource {
            handle,
            name,
            kind,
            schema,
            producer: true,
            next_row: 0,
            rows: Vec::new(),
            latest,
        };
        if let Some(index) = previous {
            self.sources[index] = source;
        } else {
            self.sources.push(source);
        }
        self.next_source = incarnation;
        self.resolve_consumers();
        self.expire_all();
        Ok(DataProducerHandle {
            source: handle,
        })
    }

    /// Leave the old incarnation readable while a consumer retains it.
    pub fn detach_producer(&mut self, producer: DataProducerHandle) -> Result<(), DataError> {
        let index = self.producer_index(producer)?;
        self.sources[index].producer = false;
        self.collect_unused();
        Ok(())
    }

    /// Explicit source destruction makes every name-bound consumer unavailable.
    pub fn destroy_source(&mut self, producer: DataProducerHandle) -> Result<(), DataError> {
        let index = self.producer_index(producer)?;
        self.sources.remove(index);
        self.resolve_consumers();
        Ok(())
    }

    /// Resolve the current incarnation by name without acquiring consumer demand.
    pub fn resolve_source(&self, name: &str) -> Option<DataSourceHandle> {
        self.sources
            .iter()
            .find(|source| source.name == name)
            .map(|source| source.handle)
    }

    /// Borrow the current retained union under an exact incarnation fence.
    pub fn read_source(&self, handle: DataSourceHandle) -> Result<DataReadView<'_>, DataError> {
        let index = self.source_index(handle).ok_or(DataError::StaleSource)?;
        Ok(DataReadView {
            source: &self.sources[index],
            consumer: None,
            time: self.time,
        })
    }

    /// Validate authored request syntax and Host-time conversion without acquiring demand.
    /// Missing sources and schema incompatibility remain observable availability states.
    pub fn validate_consumer_request(
        &self,
        request: &DataConsumerRequest,
    ) -> Result<(), DataError> {
        retention::validate_request(request)?;
        self.validate_time_windows(request, self.time)
    }

    /// Preflight a window edit against previous authored demand without changing notifications.
    /// Uses the same forward-anchor checks as `update_consumer`; retargeting starts fresh.
    pub fn validate_window_update(
        &self,
        previous: &DataConsumerRequest,
        next: &DataConsumerRequest,
    ) -> Result<(), DataError> {
        self.validate_consumer_request(next)?;
        let source = self.sources.iter().find(|source| source.name == next.name);
        retention::validate_forward(previous, next, source, self.time)
    }

    /// Registration can precede production; missing or incompatible sources stay observable.
    pub fn register_consumer(
        &mut self,
        identity: DataConsumerIdentity,
        request: DataConsumerRequest,
    ) -> Result<DataConsumerHandle, DataError> {
        self.validate_consumer_request(&request)?;
        if self
            .consumers
            .iter()
            .any(|consumer| consumer.identity == identity)
        {
            return Err(DataError::ConsumerExists);
        }
        let serial = self
            .next_consumer
            .checked_add(1)
            .ok_or(DataError::Capacity)?;
        self.consumers
            .try_reserve(1)
            .map_err(|_| DataError::Capacity)?;
        let handle = DataConsumerHandle {
            service: self.identity,
            serial,
        };
        self.consumers.push(DataConsumer {
            handle,
            identity,
            request,
            state: DataConsumerState {
                source: None,
                availability: DataAvailability::Unavailable(DataError::MissingSource),
            },
            default_start: 0,
            pending_changed: true,
            pending_availability: true,
        });
        self.next_consumer = serial;
        self.resolve_consumers();
        self.expire_all();
        Ok(handle)
    }

    /// Rebind or change windows; validation failure preserves the previous demand.
    pub fn update_consumer(
        &mut self,
        handle: DataConsumerHandle,
        request: DataConsumerRequest,
    ) -> Result<(), DataError> {
        let index = self.consumer_index(handle)?;
        self.validate_window_update(&self.consumers[index].request, &request)?;
        self.consumers[index].request = request;
        self.consumers[index].default_start = 0;
        self.consumers[index].pending_changed = true;
        self.resolve_consumers();
        self.expire_all();
        self.collect_unused();
        Ok(())
    }

    /// Compare committed binding demand without resetting an unchanged default window.
    pub(crate) fn consumer_request(
        &self,
        handle: DataConsumerHandle,
    ) -> Result<&DataConsumerRequest, DataError> {
        Ok(&self.consumers[self.consumer_index(handle)?].request)
    }

    /// Release one exact binding lifetime and expire newly unneeded streaming rows.
    pub fn release_consumer(&mut self, handle: DataConsumerHandle) -> Result<(), DataError> {
        let index = self.consumer_index(handle)?;
        self.consumers.remove(index);
        self.expire_all();
        self.collect_unused();
        Ok(())
    }

    /// World teardown releases only that exact World's demand, never its producer or siblings.
    pub fn release_world(&mut self, world: crate::WorldRef) {
        self.consumers
            .retain(|consumer| consumer.identity.world != world);
        self.expire_all();
        self.collect_unused();
    }

    /// Observe readiness without consuming notifications or changing storage.
    pub fn consumer_state(
        &self,
        handle: DataConsumerHandle,
    ) -> Result<DataConsumerState, DataError> {
        Ok(self.consumers[self.consumer_index(handle)?].state)
    }

    /// Consume coalesced flags once at a binding's World mutation boundary.
    pub fn take_notification(
        &mut self,
        handle: DataConsumerHandle,
    ) -> Result<Option<DataConsumerNotification>, DataError> {
        let index = self.consumer_index(handle)?;
        let consumer = &mut self.consumers[index];
        if !consumer.pending_changed && !consumer.pending_availability {
            return Ok(None);
        }
        Ok(Some(DataConsumerNotification {
            state: consumer.state,
            changed: std::mem::take(&mut consumer.pending_changed),
            availability_changed: std::mem::take(&mut consumer.pending_availability),
        }))
    }

    /// Borrow only this consumer's current selected rows; unavailable bindings report their reason.
    pub fn read_consumer(&self, handle: DataConsumerHandle) -> Result<DataReadView<'_>, DataError> {
        let consumer = &self.consumers[self.consumer_index(handle)?];
        if let DataAvailability::Unavailable(error) = consumer.state.availability {
            return Err(error);
        }
        let index = self
            .source_index(consumer.state.source.expect("ready source"))
            .ok_or(DataError::StaleSource)?;
        Ok(DataReadView {
            source: &self.sources[index],
            consumer: Some(consumer),
            time: self.time,
        })
    }

    pub(super) fn source_index(&self, handle: DataSourceHandle) -> Option<usize> {
        self.sources
            .iter()
            .position(|source| source.handle == handle)
    }

    pub(super) fn producer_index(&self, producer: DataProducerHandle) -> Result<usize, DataError> {
        self.source_index(producer.source)
            .filter(|index| self.sources[*index].producer)
            .ok_or(DataError::StaleProducer)
    }

    fn consumer_index(&self, handle: DataConsumerHandle) -> Result<usize, DataError> {
        self.consumers
            .iter()
            .position(|consumer| consumer.handle == handle)
            .ok_or(DataError::StaleConsumer)
    }

    fn resolve_consumers(&mut self) {
        for consumer in &mut self.consumers {
            let source = self
                .sources
                .iter()
                .find(|source| source.name == consumer.request.name);
            let state = match source {
                None => DataConsumerState {
                    source: None,
                    availability: DataAvailability::Unavailable(DataError::MissingSource),
                },
                Some(source) => DataConsumerState {
                    source: Some(source.handle),
                    availability: match retention::validate_source_windows(
                        &consumer.request,
                        source,
                    ) {
                        Ok(()) => DataAvailability::Ready,
                        Err(error) => DataAvailability::Unavailable(error),
                    },
                },
            };
            if state != consumer.state {
                if state.source != consumer.state.source {
                    consumer.default_start = 0;
                }
                consumer.state = state;
                consumer.pending_changed = true;
                consumer.pending_availability = true;
            }
        }
    }

    pub(super) fn notify_source(&mut self, handle: DataSourceHandle) {
        for consumer in &mut self.consumers {
            if consumer.state.source == Some(handle) {
                consumer.pending_changed = true;
            }
        }
    }

    pub(super) fn expire_all(&mut self) {
        if self.evaluating || self.consumer_cleanup_deferred {
            self.expiry_pending = true;
            return;
        }
        for index in 0..self.sources.len() {
            self.expire_source(index);
        }
    }

    pub(super) fn expire_source(&mut self, index: usize) {
        self.expire_source_at(index, None);
    }

    pub(super) fn expire_source_at(&mut self, index: usize, supplied_start: Option<u128>) {
        let source = &mut self.sources[index];
        if source.kind != DataSourceKind::Streaming {
            return;
        }
        let mut bytes = 0usize;
        let mut default_start = u128::from(source.next_row) + 1;
        for row in source.rows.iter().rev() {
            let Some(next) = bytes.checked_add(row.bytes) else {
                break;
            };
            if next > self.config.default_stream_bytes {
                break;
            }
            bytes = next;
            default_start = u128::from(row.id.0);
        }
        let default_start = supplied_start.unwrap_or(default_start);
        for consumer in &mut self.consumers {
            if consumer.state.source == Some(source.handle)
                && consumer.default_start < default_start
            {
                consumer.default_start = default_start;
                if consumer.request.windows.is_empty() {
                    consumer.pending_changed = true;
                }
            }
        }
        // All predicates inspect one immutable committed cut before compaction.
        let next_row = source.next_row;
        let schema = &source.schema;
        let latest = &source.latest;
        let handle = source.handle;
        let time = self.time;
        let consumers = &self.consumers;
        source.rows.retain(|row| {
            consumers.iter().any(|consumer| {
                consumer.state.source == Some(handle)
                    && consumer.state.availability == DataAvailability::Ready
                    && retention::contains_parts(next_row, schema, latest, consumer, row, time)
            })
        });
        // A fully expired stream releases reusable row capacity as well as payload.
        if source.rows.is_empty() {
            source.rows = Vec::new();
        } else if source.rows.capacity() / 4 > source.rows.len() {
            // Keep append reuse, but release a formerly large window's spare storage.
            source.rows.shrink_to(source.rows.len().saturating_mul(2));
        }
    }

    fn collect_unused(&mut self) {
        if self.evaluating || self.consumer_cleanup_deferred {
            self.expiry_pending = true;
            return;
        }
        self.sources.retain(|source| {
            source.producer
                || self
                    .consumers
                    .iter()
                    .any(|consumer| consumer.request.name == source.name)
        });
    }

    fn validate_time_windows(
        &self,
        request: &DataConsumerRequest,
        time: f64,
    ) -> Result<(), DataError> {
        if request.windows.iter().any(|window| matches!(window,
            DataWindow::Range { anchor: DataWindowAnchor::HostTime { units_per_second }, .. } if !(time * units_per_second).is_finite()
        )) {
            return Err(DataError::InvalidTime);
        }
        Ok(())
    }

    pub(super) fn begin_consumer_updates(&mut self) -> bool {
        std::mem::replace(&mut self.consumer_cleanup_deferred, true)
    }

    pub(super) fn end_consumer_updates(&mut self, previously_deferred: bool) {
        self.consumer_cleanup_deferred = previously_deferred;
        if !previously_deferred && !self.evaluating && std::mem::take(&mut self.expiry_pending) {
            self.expire_all();
            self.collect_unused();
        }
    }

    /// Freeze source storage while one World evaluates. Systems receive no producer access.
    pub(crate) fn begin_evaluation(&mut self) {
        assert!(!self.evaluating, "nested data evaluation");
        self.evaluating = true;
    }

    /// Deferred component removals may release demand during commit. Compact only after readers.
    /// Also used by WorldContext cleanup when a phase fails or unwinds.
    pub(crate) fn end_evaluation(&mut self) {
        self.evaluating = false;
        if std::mem::take(&mut self.expiry_pending) {
            self.expire_all();
            self.collect_unused();
        }
    }

    /// Host scheduling only. Reads and consumer APIs cannot advance the clock.
    pub(crate) fn advance_time(&mut self, delta: f64) -> Result<(), DataError> {
        let time = self.time + delta;
        if !delta.is_finite() || delta < 0.0 || !time.is_finite() {
            return Err(DataError::InvalidTime);
        }
        for consumer in &self.consumers {
            self.validate_time_windows(&consumer.request, time)?;
        }
        self.time = time;
        if delta > 0.0 {
            let uses_time = |consumer: &DataConsumer| {
                consumer.request.windows.iter().any(|window| {
                    matches!(
                        window,
                        DataWindow::Range {
                            anchor: DataWindowAnchor::HostTime { .. },
                            ..
                        }
                    )
                })
            };
            for consumer in &mut self.consumers {
                if uses_time(consumer) {
                    consumer.pending_changed = true;
                }
            }

            // Source writes, demand changes and policy changes already expire
            // their committed cut. Only Host-time ranges can move with the clock;
            // count, supplied and latest anchors need no repeated row scan.
            if self.evaluating || self.consumer_cleanup_deferred {
                self.expiry_pending = true;
            } else {
                for index in 0..self.sources.len() {
                    let handle = self.sources[index].handle;
                    if self.consumers.iter().any(|consumer| {
                        consumer.state.source == Some(handle)
                            && consumer.state.availability == DataAvailability::Ready
                            && uses_time(consumer)
                    }) {
                        self.expire_source(index);
                    }
                }
            }
        }
        Ok(())
    }
}
