//! Connection-owned bulk leases and Host retained-output pressure policy.
//!
//! The default 512 MiB severe threshold preserves the previous combined capture
//! and persistence envelope as a configurable Host policy. It is not a measure
//! of OS pressure. Platform hosts may additionally signal actual severe pressure.
//! Immutable backing is charged once across recipient leases; reliable delivery
//! storage remains in the existing physical-output accounts until destruction.

use crate::attachment_receipts::{ReplyReservation, SharedReplyBudget, SharedReplyReservation};
use crate::services::task_scheduler::{HostScheduler, TaskHandle};
use crate::{ReliableResponse, outbox::SessionOutbox};
use ipp_core::services::io::{IoCancellation, IoReader, IoReaderStorage, IoStorageId};
use ipp_core::services::reliable_output::{OutputCharge, OutputStatus, ReliableOutputLease};
use ipp_protocol::bulk_read::{self, BulkReadDescriptor, BulkReadReference, BulkReadResponse};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    num::NonZeroUsize,
    rc::{Rc, Weak},
    sync::Arc,
};

// Covers a retained task/future, bounded channel, state/maps, shared charge and
// allocator headers. Chunk record/copy overhead is charged independently by
// ReliableOutputAccount. Existing ordinary output credit bounds lease count via
// each reserved revocation notice; there is no independent reference-count cap.
const LEASE_BYTES: usize = 2048;

/// Configurable severe retained-output threshold, independent of asset cache budgets.
#[derive(Clone, Copy, Debug)]
pub struct BulkReadPolicy {
    /// Retained backing, delivery records and active output scratch together.
    pub severe_retained_bytes: NonZeroUsize,
}

/// Live output ownership and physical-delivery accounting at a Host boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkReadUsage {
    /// Unique output backing and owned read storage plus lease bookkeeping.
    pub backing_bytes: usize,
    /// Connection-owned unread leases.
    pub leases: usize,
    /// Existing physical-output accounts, including copies and record bookkeeping.
    pub delivery_bytes: usize,
}

impl Default for BulkReadPolicy {
    fn default() -> Self {
        Self {
            severe_retained_bytes: NonZeroUsize::new(512 << 20).unwrap(),
        }
    }
}

#[derive(Default)]
pub(crate) struct BulkReadService {
    pub(crate) policy: BulkReadPolicy,
    next: u64,
    retained: Rc<Cell<usize>>,
    backings: BTreeMap<IoStorageId, Weak<Backing>>,
    storage: Rc<RefCell<BTreeMap<IoStorageId, Weak<Charge>>>>,
    allocations: BTreeMap<u64, Weak<AllocationState>>,
    limit: Rc<Cell<usize>>,
    other_retained: Rc<Cell<usize>>,
}

struct AllocationState {
    charge: Rc<Charge>,
    cancellation: IoCancellation,
    limit: Rc<Cell<usize>>,
    other_retained: Rc<Cell<usize>>,
}

/// Host-local ownership of pending encoding/readback scratch and output storage.
/// Drop releases only accounting; the operation must own its actual allocations
/// and observe cancellation before publication. A published backing adopts this
/// charge, so the same output allocation is never charged twice.
pub struct BulkOutputAllocation(Rc<AllocationState>);

impl BulkOutputAllocation {
    /// Charge actual owned capacity, including bounded private staging/scratch.
    pub fn resize(&self, bytes: usize) -> Result<(), String> {
        if self.0.cancellation.is_cancelled() {
            return Err("Bulk output allocation revoked".into());
        }
        // Charge observed capacity even on refusal: it remains owned until its
        // operation destroys it. Pressure never pretends cancelled bytes vanished.
        self.0.charge.resize(bytes)?;
        if self
            .0
            .charge
            .account
            .get()
            .saturating_add(self.0.other_retained.get())
            > self.0.limit.get()
        {
            self.0.cancellation.cancel();
            return Err("Host severe retained-output pressure prevents allocation".into());
        }
        Ok(())
    }

    /// Cancellation wakes real waiters independently of World/frame progress.
    pub fn cancellation(&self) -> IoCancellation {
        self.0.cancellation.clone()
    }
}

pub(crate) struct Charge {
    account: Rc<Cell<usize>>,
    bytes: Cell<usize>,
}

impl Charge {
    fn resize(&self, bytes: usize) -> Result<(), String> {
        let without = self.account.get() - self.bytes.get();
        self.account.set(
            without
                .checked_add(bytes)
                .ok_or("Bulk backing charge overflow")?,
        );
        self.bytes.set(bytes);
        Ok(())
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.account.set(self.account.get() - self.bytes.get());
    }
}

pub(crate) struct Backing {
    bytes: Arc<Vec<u8>>,
    _charge: Rc<Charge>,
}

pub(crate) enum Source {
    Bytes(Rc<Backing>),
    Reader {
        reader: Box<dyn IoReader>,
        accounting: ReaderAccounting,
    },
}

type StorageRegistry = Rc<RefCell<BTreeMap<IoStorageId, Weak<Charge>>>>;

pub(crate) struct ReaderAccounting {
    identity: IoStorageId,
    charge: Rc<Charge>,
    registry: StorageRegistry,
}

impl ReaderAccounting {
    fn observe(&mut self, storage: IoReaderStorage) -> Result<(), String> {
        if storage.identity == self.identity {
            return self.charge.resize(storage.bytes);
        }
        // Rebind before releasing the previous owner. Other readers may still
        // retain its charge and must never have it resized for a replacement.
        let charge = storage_charge(&self.registry, &self.charge.account, storage)?;
        self.identity = storage.identity;
        self.charge = charge;
        Ok(())
    }
}

fn storage_charge(
    registry: &StorageRegistry,
    account: &Rc<Cell<usize>>,
    storage: IoReaderStorage,
) -> Result<Rc<Charge>, String> {
    let mut registry = registry.borrow_mut();
    registry.retain(|_, charge| charge.strong_count() != 0);
    if let Some(charge) = registry.get(&storage.identity).and_then(Weak::upgrade) {
        charge.resize(storage.bytes)?;
        return Ok(charge);
    }
    let charge = Rc::new(Charge {
        account: account.clone(),
        bytes: Cell::new(0),
    });
    charge.resize(storage.bytes)?;
    registry.insert(storage.identity, Rc::downgrade(&charge));
    Ok(charge)
}

#[derive(Default)]
struct ReadState {
    requested: u64,
    sent: u64,
    acknowledged: u64,
    eof: bool,
    error: Option<String>,
    pending: BTreeMap<u64, SharedReplyReservation>,
}

struct RevocationCredit {
    lease: ReliableOutputLease,
    budget: SharedReplyBudget,
}

impl RevocationCredit {
    fn into_record(self) -> Option<SharedReplyReservation> {
        if self.budget.0.status() != OutputStatus::Open {
            return None;
        }
        Some(Rc::new(RefCell::new(ReplyReservation::from_observation(
            self.lease,
            self.budget.0,
            256,
        ))))
    }
}

pub(crate) struct BulkReadLease {
    pub(crate) order: u64,
    descriptor: BulkReadDescriptor,
    state: Rc<RefCell<ReadState>>,
    sender: async_channel::Sender<(u64, u64)>,
    task: Option<TaskHandle<()>>,
    outbox: SessionOutbox,
    revocation: Option<RevocationCredit>,
    _charge: Rc<Charge>,
}

pub(crate) struct BulkReadDestination<'a> {
    pub(crate) connection: u64,
    pub(crate) scheduler: HostScheduler,
    pub(crate) outbox: SessionOutbox,
    pub(crate) budget: SharedReplyBudget,
    pub(crate) leases: &'a mut BTreeMap<u64, BulkReadLease>,
}

impl BulkReadService {
    pub(crate) fn set_policy(&mut self, policy: BulkReadPolicy) {
        self.policy = policy;
        self.limit.set(policy.severe_retained_bytes.get());
    }

    pub(crate) fn observe_other_retained(&self, bytes: usize) {
        self.other_retained.set(bytes);
    }

    pub(crate) fn reserve_output(&mut self, bytes: usize) -> Result<BulkOutputAllocation, String> {
        self.limit.set(self.policy.severe_retained_bytes.get());
        if self
            .retained()
            .saturating_add(self.other_retained.get())
            .saturating_add(bytes)
            > self.limit.get()
        {
            return Err("Host severe retained-output pressure prevents allocation".into());
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or("Bulk allocation identity space exhausted")?;
        let state = Rc::new(AllocationState {
            charge: self.charge(bytes)?,
            cancellation: IoCancellation::default(),
            limit: self.limit.clone(),
            other_retained: self.other_retained.clone(),
        });
        self.allocations
            .retain(|_, allocation| allocation.strong_count() != 0);
        self.allocations.insert(self.next, Rc::downgrade(&state));
        Ok(BulkOutputAllocation(state))
    }

    pub(crate) fn cancel_pending_output(&mut self) -> bool {
        while let Some((_, weak)) = self.allocations.pop_first() {
            if let Some(allocation) = weak.upgrade()
                && !allocation.cancellation.is_cancelled()
            {
                allocation.cancellation.cancel();
                return true;
            }
        }
        false
    }

    pub(crate) fn adopt_output(
        &mut self,
        bytes: Arc<Vec<u8>>,
        allocation: BulkOutputAllocation,
    ) -> Result<(Source, u64), String> {
        if allocation.0.cancellation.is_cancelled() {
            return Err("Bulk output allocation revoked".into());
        }
        let length = u64::try_from(bytes.len()).map_err(|_| "Bulk length exceeds wire range")?;
        let key = IoStorageId::of_backing(&bytes);
        self.backings
            .retain(|_, backing| backing.strong_count() != 0);
        self.storage
            .borrow_mut()
            .retain(|_, charge| charge.strong_count() != 0);
        if let Some(backing) = self.backings.get(&key).and_then(Weak::upgrade) {
            return Ok((Source::Bytes(backing), length));
        }
        allocation.0.charge.resize(bytes.capacity())?;
        let charge = self
            .storage
            .borrow()
            .get(&key)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| allocation.0.charge.clone());
        charge.resize(bytes.capacity())?;
        let backing = Rc::new(Backing {
            _charge: charge,
            bytes,
        });
        self.storage
            .borrow_mut()
            .insert(key, Rc::downgrade(&backing._charge));
        self.backings.insert(key, Rc::downgrade(&backing));
        Ok((Source::Bytes(backing), length))
    }

    pub(crate) fn retained(&self) -> usize {
        self.retained.get()
    }

    fn charge(&self, bytes: usize) -> Result<Rc<Charge>, String> {
        let total = self
            .retained
            .get()
            .checked_add(bytes)
            .ok_or("Bulk backing charge overflow")?;
        self.retained.set(total);
        Ok(Rc::new(Charge {
            account: self.retained.clone(),
            bytes: Cell::new(bytes),
        }))
    }

    fn storage_charge(&self, storage: IoReaderStorage) -> Result<Rc<Charge>, String> {
        storage_charge(&self.storage, &self.retained, storage)
    }

    pub(crate) fn additional_storage(&self, storage: IoReaderStorage) -> usize {
        if self
            .storage
            .borrow()
            .get(&storage.identity)
            .and_then(Weak::upgrade)
            .is_some()
        {
            0
        } else {
            storage.bytes
        }
    }

    pub(crate) fn additional_backing(&self, bytes: &Arc<Vec<u8>>) -> usize {
        if self
            .storage
            .borrow()
            .get(&IoStorageId::of_backing(bytes))
            .and_then(Weak::upgrade)
            .is_some()
        {
            0
        } else {
            bytes.capacity()
        }
    }

    pub(crate) fn bytes(&mut self, bytes: Arc<Vec<u8>>) -> Result<(Source, u64), String> {
        let length = u64::try_from(bytes.len()).map_err(|_| "Bulk length exceeds wire range")?;
        let key = IoStorageId::of_backing(&bytes);
        self.backings
            .retain(|_, backing| backing.strong_count() != 0);
        let backing = match self.backings.get(&key).and_then(Weak::upgrade) {
            Some(backing) => backing,
            None => {
                let backing = Rc::new(Backing {
                    _charge: self.storage_charge(IoReaderStorage {
                        identity: key,
                        bytes: bytes.capacity(),
                        mapped_bytes: 0,
                    })?,
                    bytes,
                });
                self.backings.insert(key, Rc::downgrade(&backing));
                backing
            }
        };
        Ok((Source::Bytes(backing), length))
    }

    pub(crate) fn reader(&mut self, reader: Box<dyn IoReader>) -> Result<Source, String> {
        let storage = reader
            .retained_storage()
            .ok_or("Bulk reader does not report owned storage accounting")?;
        Ok(Source::Reader {
            reader,
            accounting: ReaderAccounting {
                identity: storage.identity,
                charge: self.storage_charge(storage)?,
                registry: self.storage.clone(),
            },
        })
    }

    pub(crate) fn publish(
        &mut self,
        source: Source,
        length: Option<u64>,
        destination: BulkReadDestination<'_>,
    ) -> Result<BulkReadDescriptor, String> {
        let BulkReadDestination {
            connection,
            scheduler,
            outbox,
            budget,
            leases,
        } = destination;
        let charge = self.charge(LEASE_BYTES)?;
        let revocation = RevocationCredit {
            lease: budget
                .0
                .reserve(OutputCharge {
                    entries: 0,
                    bytes: 256 + crate::reliable_output::RESPONSE_METADATA_BYTES,
                })
                .map_err(|_| "Bulk revocation notice capacity exhausted")?,
            budget,
        };
        if self.retained() > self.policy.severe_retained_bytes.get() {
            return Err("Host severe retained-output pressure prevents publication".into());
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or("Bulk read identity space exhausted")?;
        let descriptor = BulkReadDescriptor {
            reference: BulkReadReference {
                connection,
                read: self.next,
            },
            length,
        };
        let state = Rc::new(RefCell::new(ReadState::default()));
        let (sender, receiver) = async_channel::bounded(bulk_read::PIPELINE_CHUNKS);
        let worker_state = state.clone();
        let worker_outbox = outbox.clone();
        let task = scheduler.spawn(async move {
            run(source, descriptor, receiver, worker_state, worker_outbox).await;
        });
        leases.insert(
            self.next,
            BulkReadLease {
                order: self.next,
                descriptor,
                state,
                sender,
                task: Some(task),
                outbox,
                revocation: Some(revocation),
                _charge: charge,
            },
        );
        Ok(descriptor)
    }
}

impl BulkReadLease {
    pub(crate) fn read(
        &mut self,
        id: u64,
        offset: u64,
        reservation: SharedReplyReservation,
    ) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if state.eof || offset != state.requested || state.pending.contains_key(&id) {
            return Err("Bulk read offset or correlation mismatch".into());
        }
        if state.pending.len() >= bulk_read::PIPELINE_CHUNKS
            || state.requested.saturating_sub(state.acknowledged)
                >= (bulk_read::CHUNK_BYTES * bulk_read::PIPELINE_CHUNKS) as u64
        {
            return Err("Bulk read acknowledgement window exhausted".into());
        }
        if self.descriptor.length.is_none() && !state.pending.is_empty() {
            return Err("Unknown-length bulk reads require the preceding chunk".into());
        }
        let step = self
            .descriptor
            .length
            .map_or(bulk_read::CHUNK_BYTES as u64, |length| {
                length
                    .saturating_sub(offset)
                    .min(bulk_read::CHUNK_BYTES as u64)
            });
        let next = offset
            .checked_add(step)
            .ok_or("Bulk read offset overflow")?;
        self.sender
            .try_send((id, offset))
            .map_err(|_| "Bulk read task is unavailable")?;
        state.requested = next;
        state.pending.insert(id, reservation);
        Ok(())
    }

    pub(crate) fn acknowledge(&mut self, consumed: u64, eof: bool) -> Result<bool, String> {
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if consumed < state.acknowledged
            || consumed > state.sent
            || (eof && (!state.eof || consumed != state.sent || !state.pending.is_empty()))
        {
            return Err("Invalid bulk consumed-prefix acknowledgement".into());
        }
        state.acknowledged = consumed;
        Ok(eof)
    }

    pub(crate) fn notify_revocation(&mut self, reason: &str) {
        self.revoke(reason);
        if let Some(reservation) = self
            .revocation
            .take()
            .and_then(RevocationCredit::into_record)
        {
            emit(
                &self.outbox,
                self.descriptor.reference,
                0,
                reservation,
                BulkReadResponse::Error(reason),
            );
        }
    }

    pub(crate) fn revoke(&mut self, reason: &str) {
        self.task.take();
        self.sender.close();
        let pending = {
            let mut state = self.state.borrow_mut();
            state.error = Some(reason.into());
            std::mem::take(&mut state.pending)
        };
        for (id, reservation) in pending {
            emit(
                &self.outbox,
                self.descriptor.reference,
                id,
                reservation,
                BulkReadResponse::Error(reason),
            );
        }
    }
}

fn emit(
    outbox: &SessionOutbox,
    reference: BulkReadReference,
    id: u64,
    reservation: SharedReplyReservation,
    response: BulkReadResponse<'_>,
) {
    if !outbox.is_live() {
        return;
    }
    let bytes = bulk_read::response(reference, id, response).expect("bounded bulk response");
    reservation.borrow_mut().encoded(bytes.capacity());
    outbox.push_back(ReliableResponse {
        bytes,
        reservation,
    });
}

async fn encode_reader_chunk(
    reader: &mut dyn IoReader,
    accounting: &mut ReaderAccounting,
    reference: BulkReadReference,
    id: u64,
    offset: u64,
) -> Result<(Vec<u8>, usize, bool), String> {
    let minimum = NonZeroUsize::new(bulk_read::CHUNK_BYTES).unwrap();
    // Poll the provider boundary here to observe newly allocated lookahead even
    // on Pending. The normal IoReadFuture lends only after readiness and cannot
    // expose its exclusively borrowed reader for this accounting observation.
    std::future::poll_fn(|cx| {
        let ready = reader.poll_ready(cx, minimum);
        let storage = reader
            .retained_storage()
            .ok_or_else(|| "Bulk reader accounting became unavailable".to_string());
        if let Err(error) = storage.and_then(|storage| accounting.observe(storage)) {
            return std::task::Poll::Ready(Err(error));
        }
        ready
    })
    .await?;
    let window = reader.window();
    if window.bytes().len() < minimum.get() && !window.is_final() {
        return Err("IO provider returned a short nonfinal bulk window".into());
    }
    let count = window.bytes().len().min(bulk_read::CHUNK_BYTES);
    let eof = window.is_final() && count == window.bytes().len();
    let bytes = bulk_read::response(
        reference,
        id,
        BulkReadResponse::Chunk {
            offset,
            eof,
            bytes: &window.bytes()[..count],
        },
    )
    .map_err(|error| error.to_string())?;
    // The physical frame now owns its copy; consumption can release input storage.
    window.consume(count)?;
    Ok((bytes, count, eof))
}

async fn run(
    mut source: Source,
    descriptor: BulkReadDescriptor,
    receiver: async_channel::Receiver<(u64, u64)>,
    state: Rc<RefCell<ReadState>>,
    outbox: SessionOutbox,
) {
    loop {
        let request = next_request(&mut source, &receiver).await;
        let (id, offset) = match request {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(error) => {
                let pending = {
                    let mut shared = state.borrow_mut();
                    shared.error = Some(error.clone());
                    std::mem::take(&mut shared.pending)
                };
                for (id, reservation) in pending {
                    emit(
                        &outbox,
                        descriptor.reference,
                        id,
                        reservation,
                        BulkReadResponse::Error(&error),
                    );
                }
                break;
            }
        };
        let previous_error = state.borrow().error.clone();
        let result: Result<(Vec<u8>, usize, bool), String> = if let Some(error) = previous_error {
            Err(error)
        } else {
            match &mut source {
                Source::Bytes(backing) => usize::try_from(offset)
                    .map_err(|_| "Bulk offset exceeds address space".into())
                    .and_then(|start| {
                        let end = start
                            .saturating_add(bulk_read::CHUNK_BYTES)
                            .min(backing.bytes.len());
                        let payload = backing
                            .bytes
                            .get(start..end)
                            .ok_or("Bulk offset exceeds backing")?;
                        let eof = end == backing.bytes.len();
                        let bytes = bulk_read::response(
                            descriptor.reference,
                            id,
                            BulkReadResponse::Chunk {
                                offset,
                                eof,
                                bytes: payload,
                            },
                        )
                        .map_err(|error| error.to_string())?;
                        Ok((bytes, payload.len(), eof))
                    }),
                Source::Reader {
                    reader,
                    accounting,
                } => {
                    let result = encode_reader_chunk(
                        reader.as_mut(),
                        accounting,
                        descriptor.reference,
                        id,
                        offset,
                    )
                    .await;
                    match reader.retained_storage() {
                        Some(storage) => accounting.observe(storage).and(result),
                        None => Err("Bulk reader accounting became unavailable".into()),
                    }
                }
            }
        };
        {
            let mut shared = state.borrow_mut();
            let Some(reservation) = shared.pending.remove(&id) else {
                break;
            };
            match result {
                Ok((bytes, count, eof)) => {
                    let Some(end) = offset.checked_add(count as u64) else {
                        shared.error = Some("Bulk read offset overflow".into());
                        emit(
                            &outbox,
                            descriptor.reference,
                            id,
                            reservation,
                            BulkReadResponse::Error("Bulk read offset overflow"),
                        );
                        continue;
                    };
                    if descriptor
                        .length
                        .is_some_and(|length| end > length || (eof && end != length))
                    {
                        shared.error = Some("Bulk source length changed".into());
                        drop(bytes);
                        emit(
                            &outbox,
                            descriptor.reference,
                            id,
                            reservation,
                            BulkReadResponse::Error("Bulk source length changed"),
                        );
                    } else {
                        shared.sent = end;
                        shared.eof = eof;
                        if descriptor.length.is_none() {
                            shared.requested = end;
                        }
                        if outbox.is_live() {
                            reservation.borrow_mut().encoded(bytes.capacity());
                            outbox.push_back(ReliableResponse {
                                bytes,
                                reservation,
                            });
                        }
                    }
                }
                Err(error) => {
                    shared.error = Some(error.clone());
                    emit(
                        &outbox,
                        descriptor.reference,
                        id,
                        reservation,
                        BulkReadResponse::Error(&error),
                    );
                }
            }
        }
        ipp_core::services::asset_management::decode::yield_decode().await;
    }
}

/// Observe owned storage independently of source readiness. A published reader
/// can fill or replace its backing before the client asks for the next chunk.
/// Register before taking the snapshot so an asynchronous change cannot leave
/// the task parked with a stale charge. No window is acquired or consumed here.
async fn next_request(
    source: &mut Source,
    receiver: &async_channel::Receiver<(u64, u64)>,
) -> Result<Option<(u64, u64)>, String> {
    use std::future::Future;

    let mut request = std::pin::pin!(receiver.recv());
    std::future::poll_fn(|cx| {
        if let Source::Reader {
            reader,
            accounting,
        } = source
        {
            reader.register_storage_waker(cx.waker());
            let Some(storage) = reader.retained_storage() else {
                return std::task::Poll::Ready(Err(
                    "Bulk reader accounting became unavailable".into()
                ));
            };
            if let Err(error) = accounting.observe(storage) {
                return std::task::Poll::Ready(Err(error));
            }
        }
        request.as_mut().poll(cx).map(|request| Ok(request.ok()))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_output_adoption_reclaims_dead_registry_entries() {
        let mut service = BulkReadService::default();
        let mut external_owners = Vec::new();
        for _ in 0..64 {
            let bytes = Arc::new(vec![7; 256]);
            let allocation = service.reserve_output(bytes.capacity()).unwrap();
            let (source, _) = service.adopt_output(bytes.clone(), allocation).unwrap();
            assert_eq!(service.retained(), bytes.capacity());
            assert_eq!(service.backings.len(), 1);
            assert_eq!(service.storage.borrow().len(), 1);
            drop(source);
            assert_eq!(service.retained(), 0);
            // Keep independently owned backing addresses distinct: stale weak
            // registry entries must be reclaimed, not accidentally overwritten
            // when the allocator recycles an earlier publication's address.
            external_owners.push(bytes);
        }
    }
}
