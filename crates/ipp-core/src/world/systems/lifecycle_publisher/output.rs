use super::value_observation::value_heap_bytes;
use super::*;
use crate::WorldRef;
use crate::components::schema::FieldValue;
use crate::services::reliable_output::{
    OutputCharge, OutputFailure, OutputReserveError, OutputStatus, ReliableOutputAccount,
    ReliableOutputLease,
};
use std::cell::{Cell, RefCell};
use std::mem::size_of;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

/// One full node of the queue's B-tree: eleven entries, twelve child edges and its header.
const QUEUE_NODE_BYTES: usize =
    11 * size_of::<(u64, LifecycleWatchDelivery)>() + 16 * size_of::<usize>();

/// Bookkeeping charged for each queued record. Every B-tree node except the root holds at
/// least five entries, so a record never accounts for more than a fifth of a full node; the
/// endpoint reserves the root node once.
const QUEUE_ENTRY_BYTES: usize = QUEUE_NODE_BYTES.div_ceil(5);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LifecycleMemberStatus {
    Pending,
    Active,
    Removed,
}

pub(super) struct LifecycleMemberState {
    pub output: Weak<LifecycleOutputState>,
    pub id: LifecycleWatchId,
    pub target: LifecycleWatchTarget,
    pub kinds: LifecycleWatchKinds,
    pub status: Cell<LifecycleMemberStatus>,
    _metadata: ReliableOutputLease,
    _output_metadata: Rc<ReliableOutputLease>,
}

/// Inert member until an ordered add ACK; cloning shares one membership generation.
#[derive(Clone)]
pub struct LifecycleWatchMember(pub(super) Rc<LifecycleMemberState>);

impl LifecycleWatchMember {
    /// Generation carried by ACKs and events, never reusable after removal.
    pub fn id(&self) -> LifecycleWatchId {
        self.0.id
    }

    /// The exact entity or entity/type originally requested.
    pub fn target(&self) -> LifecycleWatchTarget {
        self.0.target.clone()
    }

    /// Tracking liveness, independent of the target's current existence.
    pub fn is_active(&self) -> bool {
        self.0.status.get() == LifecycleMemberStatus::Active
            && self
                .0
                .output
                .upgrade()
                .is_some_and(|output| output.tracking())
    }
}

/// Non-Clone owned payload and lease; popping transfers rather than releases credit.
pub struct LifecycleWatchDelivery {
    record: LifecycleWatchRecord,
    lease: ReliableOutputLease,
}

impl LifecycleWatchDelivery {
    /// Borrow the frozen payload while retaining its allocation charge.
    pub fn record(&self) -> &LifecycleWatchRecord {
        &self.record
    }

    /// Includes retained payload and the adapter's declared peak encoding allocation.
    pub fn charge(&self) -> OutputCharge {
        self.lease.charge()
    }

    /// Keep both parts until encoding finishes; then drop payload before shrinking credit.
    pub fn into_parts(self) -> (LifecycleWatchRecord, ReliableOutputLease) {
        (self.record, self.lease)
    }
}

/// Undelivered records in retention order. Every retained record takes a larger stamp, so a
/// value record that supersedes an undelivered one moves behind every earlier record.
#[derive(Default)]
struct OutputQueue {
    records: BTreeMap<u64, LifecycleWatchDelivery>,
    stamp: u64,
}

impl OutputQueue {
    fn push(&mut self, delivery: LifecycleWatchDelivery) -> u64 {
        self.stamp = self
            .stamp
            .checked_add(1)
            .expect("lifecycle output stamp exhausted");
        self.records.insert(self.stamp, delivery);
        self.stamp
    }

    fn pop(&mut self) -> Option<LifecycleWatchDelivery> {
        self.records.pop_first().map(|(_, delivery)| delivery)
    }

    /// Release observations of one member, or of every member, with their leases.
    /// ACKs remain.
    fn discard_observations(&mut self, member: Option<LifecycleWatchId>) {
        self.records
            .retain(|_, delivery| match delivery.record.body {
                LifecycleWatchRecordBody::Event {
                    member: id,
                    ..
                }
                | LifecycleWatchRecordBody::Value {
                    member: id,
                    ..
                } => member.is_some_and(|member| member != id),
                LifecycleWatchRecordBody::Acknowledgement {
                    ..
                } => true,
            });
    }
}

pub(super) struct LifecycleOutputState {
    pub account: ReliableOutputAccount,
    pub encoding: LifecycleWatchEncoding,
    pub world: WorldRef,
    pub session: u64,
    identity: u64,
    serial: Cell<u64>,
    live: Cell<bool>,
    /// Session release ended tracking; ACKs still settle while the endpoint is live.
    retired: Cell<bool>,
    queue: RefCell<OutputQueue>,
    allocation: Rc<ReliableOutputLease>,
    traffic: Cell<LifecycleWatchTraffic>,
}

/// A fixed World/session endpoint sharing the physical connection's output account.
#[derive(Clone)]
pub struct LifecycleWatchOutput(pub(super) Rc<LifecycleOutputState>);

impl LifecycleWatchOutput {
    /// Reserve endpoint metadata before accepting members.
    pub fn new(
        account: ReliableOutputAccount,
        world: WorldRef,
        session: u64,
        encoding: LifecycleWatchEncoding,
    ) -> Result<Self, OutputReserveError> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        if session == 0 {
            return Err(OutputReserveError::Closed);
        }

        let metadata = account.reserve(OutputCharge {
            entries: 0,
            bytes: size_of::<LifecycleOutputState>()
                + size_of::<ReliableOutputLease>()
                + 4 * size_of::<usize>()
                + QUEUE_NODE_BYTES,
        })?;

        let identity = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| OutputReserveError::Capacity)?;

        Ok(Self(Rc::new(LifecycleOutputState {
            account,
            encoding,
            world,
            session,
            identity,
            serial: Cell::new(0),
            live: Cell::new(true),
            retired: Cell::new(false),
            queue: RefCell::default(),
            allocation: Rc::new(metadata),
            traffic: Cell::default(),
        })))
    }

    /// Mint a fresh inert generation. Duplicate local users should share this handle.
    pub fn new_member(
        &self,
        target: LifecycleWatchTarget,
        kinds: LifecycleWatchKinds,
    ) -> Result<LifecycleWatchMember, ErrorReason> {
        if !target.valid() || !kinds.valid_for(&target) {
            return Err(ErrorReason::InvalidValue);
        }

        if !self.0.tracking() {
            return Err(ErrorReason::Capacity);
        }

        let generation = self
            .0
            .serial
            .get()
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        let metadata = self
            .0
            .account
            .reserve(OutputCharge {
                entries: 0,
                bytes: size_of::<LifecycleMemberState>() + 2 * size_of::<usize>(),
            })
            .map_err(|_| ErrorReason::Capacity)?;
        self.0.serial.set(generation);

        Ok(LifecycleWatchMember(Rc::new(LifecycleMemberState {
            output: Rc::downgrade(&self.0),
            id: LifecycleWatchId {
                output: self.0.identity,
                generation,
            },
            target,
            kinds,
            status: Cell::new(LifecycleMemberStatus::Pending),
            _metadata: metadata,
            _output_metadata: self.0.allocation.clone(),
        })))
    }

    /// Move one record and its charge into Host ownership, freeing only its Core queue slot.
    pub fn pop_front(&self) -> Option<LifecycleWatchDelivery> {
        self.0.queue.borrow_mut().pop()
    }

    /// Whether the oldest undelivered record is a value record. Value records are state:
    /// while one stays here, a newer value of its member replaces it, so a Host whose
    /// transport is behind can stop draining at it instead of queueing every frame's value.
    pub fn front_is_value(&self) -> bool {
        self.0
            .queue
            .borrow()
            .records
            .first_key_value()
            .is_some_and(|(_, delivery)| {
                matches!(delivery.record.body, LifecycleWatchRecordBody::Value { .. })
            })
    }

    /// Session teardown releases queued records; already handed-off leases remain charged.
    pub fn close(&self) {
        self.0.live.set(false);
        let records = std::mem::take(&mut self.0.queue.borrow_mut().records);
        drop(records);
    }

    /// The adapter must inspect failure and disconnect only this physical connection.
    pub fn account(&self) -> &ReliableOutputAccount {
        &self.0.account
    }

    /// Cumulative event retention, excluding ACKs and failed retention.
    /// A closed or failed endpoint cannot supply a live diagnostic sample.
    pub fn traffic(&self) -> Option<LifecycleWatchTraffic> {
        self.0.tracking().then(|| self.0.traffic.get())
    }

    /// Peak Core payload plus adapter encoding capacity for a reply with this many baselines,
    /// whose value targets name `value_fields` field offsets in total.
    /// Hosts page requests and replies using this checked bound and their own wire limits.
    pub fn acknowledgement_charge(
        &self,
        members: usize,
        value_fields: usize,
    ) -> Option<OutputCharge> {
        Some(OutputCharge {
            entries: 1,
            bytes: QUEUE_ENTRY_BYTES
                .checked_add(self.0.encoding.acknowledgement_bytes)?
                .checked_add(
                    members.checked_mul(
                        size_of::<LifecycleMembershipBaseline>()
                            .checked_add(self.0.encoding.baseline_bytes)?,
                    )?,
                )?
                .checked_add(value_fields.checked_mul(self.0.encoding.baseline_field_bytes)?)?,
        })
    }
}

impl LifecycleOutputState {
    pub fn tracking(&self) -> bool {
        self.live.get() && !self.retired.get() && self.account.status() == OutputStatus::Open
    }

    pub fn retain(&self, body: LifecycleWatchRecordBody, lease: ReliableOutputLease) {
        if !self.live.get() {
            return;
        }

        let event_bytes =
            matches!(&body, LifecycleWatchRecordBody::Event { .. }).then(|| lease.charge().bytes);

        self.queue.borrow_mut().push(LifecycleWatchDelivery {
            record: LifecycleWatchRecord {
                world: self.world,
                session: self.session,
                body,
            },
            lease,
        });
        if let Some(bytes) = event_bytes {
            let mut traffic = self.traffic.get();
            traffic.record(bytes);
            self.traffic.set(traffic);
        }
    }

    pub fn observe(
        &self,
        member: LifecycleWatchId,
        sequence: u64,
        tick: u64,
        observation: &LifecycleObservation,
    ) {
        let Some(bytes) = QUEUE_ENTRY_BYTES.checked_add(self.encoding.event_bytes) else {
            self.account.fail(OutputFailure::InvalidPayload);
            return;
        };

        match self.account.reserve(OutputCharge {
            entries: 1,
            bytes,
        }) {
            Ok(lease) => self.retain(
                LifecycleWatchRecordBody::Event {
                    member,
                    sequence,
                    tick,
                    observation: observation.clone(),
                },
                lease,
            ),
            Err(OutputReserveError::Capacity) => self.account.fail(OutputFailure::Capacity),
            Err(_) => {}
        }
    }

    /// Queue a member's current values, charged in bytes when observed. A member keeps at
    /// most one undelivered value record: a newer value replaces it, moves behind every
    /// record retained before it and takes over its lease, resized.
    ///
    /// Returns whether the record was retained. `pending` holds the stamp of the member's
    /// undelivered record, and is empty after a failure. Account exhaustion fails the
    /// connection, as for events; the replaced record is released either way.
    pub fn observe_value(
        &self,
        member: LifecycleWatchId,
        pending: &mut Option<u64>,
        tick: u64,
        values: Option<Vec<(u32, FieldValue)>>,
    ) -> bool {
        if !self.live.get() {
            return false;
        }

        let payload = values.as_ref().map_or(Some(0), |values| {
            values.iter().try_fold(
                values
                    .capacity()
                    .checked_mul(size_of::<(u32, FieldValue)>())?,
                |bytes, (_, value)| bytes.checked_add(value_heap_bytes(value)),
            )
        });
        let bytes = payload
            .and_then(|payload| payload.checked_add(QUEUE_ENTRY_BYTES))
            .and_then(|bytes| bytes.checked_add(self.encoding.value_capacity(values.as_deref())?));
        let Some(bytes) = bytes else {
            self.account.fail(OutputFailure::InvalidPayload);
            return false;
        };
        let charge = OutputCharge {
            entries: 1,
            bytes,
        };

        let mut queue = self.queue.borrow_mut();
        let superseded = pending
            .take()
            .and_then(|stamp| queue.records.remove(&stamp));
        let lease = match superseded {
            Some(LifecycleWatchDelivery {
                record,
                mut lease,
                ..
            }) => {
                drop(record);
                lease.resize(charge).map(|()| lease)
            }
            None => self.account.reserve(charge),
        };

        match lease {
            Ok(lease) => {
                *pending = Some(queue.push(LifecycleWatchDelivery {
                    record: LifecycleWatchRecord {
                        world: self.world,
                        session: self.session,
                        body: LifecycleWatchRecordBody::Value {
                            member,
                            tick,
                            values,
                        },
                    },
                    lease,
                }));
                true
            }
            Err(OutputReserveError::Capacity) => {
                self.account.fail(OutputFailure::Capacity);
                false
            }
            Err(_) => false,
        }
    }

    pub fn remove(&self, member: LifecycleWatchId) {
        self.queue.borrow_mut().discard_observations(Some(member));
    }

    pub fn retire(&self) {
        self.retired.set(true);
        self.queue.borrow_mut().discard_observations(None);
    }
}

#[cfg(test)]
#[path = "output_diagnostics_tests.rs"]
mod diagnostics_tests;
