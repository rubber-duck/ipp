//! Connection-owned producer fences and finite dataset transfers.

use super::*;
use ipp_core::services::data::DataProducerHandle;
use ipp_core::services::io::IoUploadAssembly;
use ipp_protocol::dataset::{self, DatasetOperation, DatasetPage, DatasetResponse};
use std::time::Duration;

const INACTIVITY: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct DatasetConnection {
    last_request: u64,
    producers: BTreeMap<u64, DataProducerHandle>,
    transfers: BTreeMap<u64, DatasetTransfer>,
    declared_bytes: usize,
}

struct DatasetTransfer {
    producer: DataProducerHandle,
    upload: IoUploadAssembly,
    length: usize,
    progress: Duration,
    outcome: SharedReplyReservation,
}

impl DatasetConnection {
    pub(super) fn has_transfers(&self) -> bool {
        !self.transfers.is_empty()
    }

    pub(super) fn disconnect(self, runtime: &mut ipp_core::HostRuntime) {
        for producer in self.producers.values() {
            let _ = runtime.data_sources_mut().detach_producer(*producer);
        }
        // Dropping transfers cancels assembly and its reserved final output.
    }

    fn remove(&mut self, id: u64) -> Option<DatasetTransfer> {
        let transfer = self.transfers.remove(&id)?;
        self.declared_bytes -= transfer.length;
        Some(transfer)
    }
}

impl HostConnectionState {
    fn dataset_reply(
        &mut self,
        id: u64,
        body: DatasetResponse,
        reservation: SharedReplyReservation,
    ) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            let bytes = dataset::response(self.id, id, &body).map_err(|error| error.to_string())?;
            reservation
                .borrow_mut()
                .reserve_bytes(bytes.capacity())
                .map_err(|error| error.to_string())?;
            reservation.borrow_mut().encoded(bytes.capacity());
            self.outbox.push_back(ReliableResponse {
                bytes,
                reservation,
            });
            Ok(())
        })();
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }
}

impl<P: HostServices> Host<P> {
    pub(super) fn receive_dataset(&mut self, connection: u64, bytes: &[u8]) -> Result<(), String> {
        let request = dataset::decode(bytes, connection).map_err(|error| error.to_string())?;
        let mut state = self
            .connections
            .states
            .remove(&connection)
            .ok_or("Connection is closed")?;
        let result = (|| {
            if request.id <= state.datasets.last_request {
                return Err("dataset request identity reused or out of order".into());
            }
            state.datasets.last_request = request.id;
            // Credit/output is reserved before accepting any operation, including continuations.
            // Open transfers do not wait for World batches, queues or withheld final outcomes.
            let continuation = matches!(
                request.operation,
                DatasetOperation::Chunk { .. }
                    | DatasetOperation::Finish { .. }
                    | DatasetOperation::Cancel { .. }
            );
            if !continuation && state.admitted_requests(&self.sessions) >= crate::MAX_PENDING {
                return Err("connection congestion: dataset admission exhausted".into());
            }
            let reservation = state.reserve_reply(
                if matches!(
                    request.operation,
                    DatasetOperation::Read { .. }
                        | DatasetOperation::BindingView { .. }
                        | DatasetOperation::DriverStatus { .. }
                ) {
                    2 * dataset::PAGE_BYTES
                } else {
                    8192
                },
            )?;
            let body = self
                .apply_dataset(&mut state, request.id, request.operation)
                .unwrap_or_else(DatasetResponse::Error);
            state.dataset_reply(request.id, body, reservation)
        })();
        self.connections.states.insert(connection, state);
        result
    }

    fn apply_dataset(
        &mut self,
        state: &mut HostConnectionState,
        id: u64,
        operation: DatasetOperation<'_>,
    ) -> Result<DatasetResponse, String> {
        let now = self.connections.now;
        match operation {
            DatasetOperation::Create {
                name,
                kind,
                schema,
            } => {
                if state.datasets.producers.len() >= dataset::PRODUCERS {
                    return Err("dataset producer capacity exhausted".into());
                }
                let producer = self
                    .runtime
                    .data_sources_mut()
                    .create_source(name, kind, schema)
                    .map_err(|error| error.to_string())?;
                let token = producer.source().incarnation();
                state.datasets.producers.insert(token, producer);
                if let Ok(view) = self.runtime.data_sources().read_source(producer.source()) {
                    ipp_core::diagnostic!(
                        Info,
                        "[IPP {}] dataset.created connection={} source={} incarnation={}",
                        P::NAME,
                        state.id,
                        view.name(),
                        token
                    );
                }
                Ok(DatasetResponse::Created(token))
            }
            DatasetOperation::Begin {
                producer,
                length,
            } => {
                let producer = *state
                    .datasets
                    .producers
                    .get(&producer)
                    .ok_or("StaleProducer")?;
                let length =
                    usize::try_from(length).map_err(|_| "dataset update capacity exhausted")?;
                if length > dataset::UPDATE_BYTES
                    || state.datasets.transfers.len() >= dataset::TRANSFERS
                    || length > dataset::STAGING_BYTES.saturating_sub(state.datasets.declared_bytes)
                {
                    return Err(
                        "dataset transfer pressure: bounded staging capacity exhausted".into(),
                    );
                }
                let outcome = state.reserve_reply(8192)?;
                state.datasets.transfers.insert(
                    id,
                    DatasetTransfer {
                        producer,
                        length,
                        upload: IoUploadAssembly::new(length),
                        progress: now,
                        outcome,
                    },
                );
                state.datasets.declared_bytes += length;
                Ok(DatasetResponse::Credit)
            }
            DatasetOperation::Chunk {
                transfer,
                offset,
                bytes,
            } => {
                let input = state
                    .datasets
                    .transfers
                    .get_mut(&transfer)
                    .ok_or("Unknown dataset transfer")?;
                if let Err(error) = input.upload.push(offset, bytes) {
                    let input = state.datasets.remove(transfer).expect("known transfer");
                    state.dataset_reply(
                        transfer,
                        DatasetResponse::Refused(format!("dataset transport: {error:?}")),
                        input.outcome,
                    )?;
                    return Err(format!("dataset transport: {error:?}"));
                }
                input.progress = now;
                Ok(DatasetResponse::Credit)
            }
            DatasetOperation::Finish {
                transfer,
            } => {
                let input = state
                    .datasets
                    .remove(transfer)
                    .ok_or("Unknown dataset transfer")?;
                let body = match input.upload.finish() {
                    Err(error) => DatasetResponse::Refused(format!("dataset transport: {error:?}")),
                    Ok(bytes) => match dataset::decode_update(&bytes) {
                        Err(error) => {
                            DatasetResponse::Refused(format!("dataset transport: {error}"))
                        }
                        Ok(deltas) => {
                            drop(bytes);
                            // Exclusive Host mutation, shared with local producers. DataService
                            // owns per-delta validation, committed prefix, expiry and notification.
                            let result = self
                                .runtime
                                .data_sources_mut()
                                .apply_batch(input.producer, deltas);
                            let committed = result
                                .as_ref()
                                .copied()
                                .unwrap_or_else(|error| error.committed);
                            ipp_core::diagnostic!(
                                Debug,
                                "[IPP {}] dataset.updated connection={} incarnation={} committed_deltas={} assigned_rows={} failure={:?}",
                                P::NAME,
                                state.id,
                                input.producer.source().incarnation(),
                                committed.committed_deltas,
                                committed.assigned_rows,
                                result
                                    .as_ref()
                                    .err()
                                    .map(|error| (error.delta_index, error.reason))
                            );
                            DatasetResponse::Outcome(result)
                        }
                    },
                };
                state.dataset_reply(transfer, body, input.outcome)?;
                Ok(DatasetResponse::Credit)
            }
            DatasetOperation::Cancel {
                transfer,
            } => {
                if let Some(input) = state.datasets.remove(transfer) {
                    state.dataset_reply(
                        transfer,
                        DatasetResponse::Refused("dataset transfer cancelled".into()),
                        input.outcome,
                    )?;
                }
                Ok(DatasetResponse::Credit)
            }
            DatasetOperation::Release {
                producer,
            }
            | DatasetOperation::Destroy {
                producer,
            } => {
                let handle = *state
                    .datasets
                    .producers
                    .get(&producer)
                    .ok_or("StaleProducer")?;
                let result = if matches!(operation, DatasetOperation::Destroy { .. }) {
                    self.runtime.data_sources_mut().destroy_source(handle)
                } else {
                    self.runtime.data_sources_mut().detach_producer(handle)
                };
                result.map_err(|error| error.to_string())?;
                state.datasets.producers.remove(&producer);
                ipp_core::diagnostic!(
                    Info,
                    "[IPP {}] dataset.{} connection={} incarnation={}",
                    P::NAME,
                    if matches!(operation, DatasetOperation::Destroy { .. }) {
                        "destroyed"
                    } else {
                        "detached"
                    },
                    state.id,
                    producer
                );
                // Already admitted work remains fenced to the detached incarnation and reports
                // StaleProducer on finish. It never retargets another connection's replacement.
                Ok(DatasetResponse::Complete)
            }
            DatasetOperation::BindingView {
                session,
                entity,
                offset,
                limit,
            } => {
                if !state.sessions.contains(&session) {
                    return Err("World session belongs to another connection".into());
                }
                let world = self.session_mut(session).ok_or("World session ended")?;
                dataset::binding_observation(
                    world.world(),
                    ipp_core::EntityId::from_bits(entity),
                    offset,
                    limit,
                )
                .map(DatasetResponse::BindingView)
                .map_err(|error| error.to_string())
            }
            DatasetOperation::DriverStatus {
                session,
                entity,
            } => {
                if !state.sessions.contains(&session) {
                    return Err("World session belongs to another connection".into());
                }
                let world = self.session_mut(session).ok_or("World session ended")?;
                let status = world
                    .world()
                    .expression_driver_status(ipp_core::EntityId::from_bits(entity))
                    .ok_or("Missing expression driver")?;
                dataset::driver_observation(&status)
                    .map(DatasetResponse::DriverStatus)
                    .map_err(|error| error.to_string())
            }
            DatasetOperation::Read {
                name,
                incarnation,
                offset,
                limit,
            } => {
                let data = self.runtime.data_sources();
                let handle = data.resolve_source(&name).ok_or("MissingSource")?;
                if incarnation.is_some_and(|expected| expected != handle.incarnation()) {
                    return Err("StaleSource".into());
                }
                let view = data
                    .read_source(handle)
                    .map_err(|error| error.to_string())?;
                DatasetPage::observe(view, offset, limit)
                    .map(DatasetResponse::Page)
                    .map_err(|error| error.to_string())
            }
        }
    }

    pub(crate) fn expire_dataset_transfers(&mut self) {
        let now = self.connections.now;
        for state in self.connections.states.values_mut() {
            let expired: Vec<_> = state
                .datasets
                .transfers
                .iter_mut()
                .filter_map(|(&id, input)| {
                    if state.throttled {
                        input.progress = now;
                    }
                    (now.saturating_sub(input.progress) >= INACTIVITY).then_some(id)
                })
                .collect();
            for id in expired {
                let input = state.datasets.remove(id).expect("expired transfer");
                if let Err(error) = state.dataset_reply(
                    id,
                    DatasetResponse::Refused("dataset transfer timed out".into()),
                    input.outcome,
                ) {
                    state.failure = Some(error);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "datasets_tests.rs"]
mod tests;
