use super::*;
use crate::services::reliable_output::{
    OutputCharge, OutputFailure, OutputReserveError, OutputStatus, ReliableOutputAccount,
    ReliableOutputLease,
};
use std::cell::{Cell, RefCell};
use std::mem::size_of;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct OutputState {
    pub account: ReliableOutputAccount,
    pub encoding: GuiObservationEncoding,
    identity: u64,
    serial: Cell<u64>,
    live: Cell<bool>,
    queue: RefCell<OutputQueue>,
    allocation: Rc<ReliableOutputLease>,
}

pub(super) struct OutputWeak {
    output: Weak<OutputState>,
    _allocation: Rc<ReliableOutputLease>,
}

impl OutputWeak {
    fn new(output: &Rc<OutputState>) -> Self {
        Self {
            output: Rc::downgrade(output),
            _allocation: output.allocation.clone(),
        }
    }

    pub fn upgrade(&self) -> Option<Rc<OutputState>> {
        self.output.upgrade()
    }

    pub fn belongs_to(&self, output: &Rc<OutputState>) -> bool {
        self.output.as_ptr() == Rc::as_ptr(output)
    }
}

/// Independently closeable World-session FIFO, sharing its connection's account.
#[derive(Clone)]
pub struct GuiObservationOutput(pub(super) Rc<OutputState>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SubscriptionStatus {
    Pending,
    Active,
    Closed,
}

pub(super) struct SubscriptionState {
    pub output: OutputWeak,
    pub id: GuiObservationSubscriptionId,
    pub world: WorldRef,
    pub classes: GuiObservationClasses,
    pub status: Cell<SubscriptionStatus>,
    _metadata: ReliableOutputLease,
}

/// Strong identity handles retain their metadata charge even after cancellation.
#[derive(Clone)]
pub struct GuiObservationSubscription(pub(super) Rc<SubscriptionState>);

impl GuiObservationSubscription {
    /// Stable output/generation identity carried by control and effect records.
    pub fn id(&self) -> GuiObservationSubscriptionId {
        self.0.id
    }

    /// Read-only liveness; closing the output/account makes this immediately false.
    pub fn is_active(&self) -> bool {
        self.0.status.get() == SubscriptionStatus::Active
            && self
                .0
                .output
                .upgrade()
                .is_some_and(|output| output.is_live())
    }
}

/// Non-Clone retained delivery; transfer its lease into the Host outbox without a gap.
pub struct GuiObservationDelivery {
    record: GuiObservationRecord,
    lease: ReliableOutputLease,
}

impl GuiObservationDelivery {
    /// Borrow the exact retained record without releasing its accounting.
    pub fn record(&self) -> &GuiObservationRecord {
        &self.record
    }

    /// Includes retained payload and the adapter's declared peak encoding allocation.
    pub fn charge(&self) -> OutputCharge {
        self.lease.charge()
    }

    /// Encode while both parts remain owned. Release payload before shrinking peak credit.
    pub fn into_parts(self) -> (GuiObservationRecord, ReliableOutputLease) {
        (self.record, self.lease)
    }
}

struct QueueNode {
    delivery: GuiObservationDelivery,
    next: Option<Box<QueueNode>>,
}

#[derive(Default)]
struct OutputQueue {
    incoming: Option<Box<QueueNode>>,
    outgoing: Option<Box<QueueNode>>,
}

impl OutputQueue {
    fn push(&mut self, delivery: GuiObservationDelivery) {
        self.incoming = Some(Box::new(QueueNode {
            delivery,
            next: self.incoming.take(),
        }));
    }

    fn pop(&mut self) -> Option<GuiObservationDelivery> {
        if self.outgoing.is_none() {
            while let Some(mut node) = self.incoming.take() {
                self.incoming = node.next.take();
                node.next = self.outgoing.take();
                self.outgoing = Some(node);
            }
        }
        let mut node = self.outgoing.take()?;
        self.outgoing = node.next.take();
        Some(node.delivery)
    }
}

impl Drop for OutputQueue {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

impl GuiObservationOutput {
    /// Charge endpoint metadata to the same account used by connection replies and events.
    pub fn new(
        account: ReliableOutputAccount,
        encoding: GuiObservationEncoding,
    ) -> Result<Self, OutputReserveError> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let metadata = account.reserve(OutputCharge {
            entries: 0,
            bytes: size_of::<OutputState>()
                + size_of::<ReliableOutputLease>()
                + 4 * size_of::<usize>(),
        })?;
        let identity = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| OutputReserveError::Capacity)?;
        Ok(Self(Rc::new(OutputState {
            account,
            encoding,
            identity,
            serial: Cell::new(0),
            live: Cell::new(true),
            queue: RefCell::default(),
            allocation: Rc::new(metadata),
        })))
    }

    /// Mint an inert whole-World subscription; only the ordered command activates it.
    pub fn new_subscription(
        &self,
        world: WorldRef,
        classes: GuiObservationClasses,
    ) -> Result<GuiObservationSubscription, OutputReserveError> {
        self.0.admission()?;
        let generation = self
            .0
            .serial
            .get()
            .checked_add(1)
            .ok_or(OutputReserveError::Capacity)?;
        let metadata = self.0.account.reserve(OutputCharge {
            entries: 0,
            bytes: size_of::<SubscriptionState>() + 2 * size_of::<usize>(),
        })?;
        self.0.serial.set(generation);
        Ok(GuiObservationSubscription(Rc::new(SubscriptionState {
            output: OutputWeak::new(&self.0),
            id: GuiObservationSubscriptionId {
                output: self.0.identity,
                generation,
            },
            world,
            classes,
            status: Cell::new(SubscriptionStatus::Pending),
            _metadata: metadata,
        })))
    }

    /// Transfer a retained record and its lease; failed outputs do not continue delivery.
    pub fn pop_front(&self) -> Option<GuiObservationDelivery> {
        self.0
            .is_live()
            .then(|| self.0.queue.borrow_mut().pop())
            .flatten()
    }

    /// Revoke this endpoint and release queued records, not other connection outputs.
    pub fn close(&self) {
        self.0.live.set(false);
        let queue = std::mem::take(&mut *self.0.queue.borrow_mut());
        drop(queue);
    }

    /// The shared connection account and failure latch inspected by the adapter.
    pub fn account(&self) -> &ReliableOutputAccount {
        &self.0.account
    }
}

impl OutputState {
    pub fn admission(&self) -> Result<(), OutputReserveError> {
        if !self.live.get() {
            return Err(OutputReserveError::Closed);
        }
        match self.account.status() {
            OutputStatus::Open => Ok(()),
            OutputStatus::Closed => Err(OutputReserveError::Closed),
            OutputStatus::Failed(reason) => Err(OutputReserveError::Failed(reason)),
        }
    }

    pub fn is_live(&self) -> bool {
        self.admission().is_ok()
    }

    pub fn control_charge(&self) -> Option<OutputCharge> {
        Some(OutputCharge {
            entries: 1,
            bytes: size_of::<QueueNode>().checked_add(self.encoding.control_bytes)?,
        })
    }

    pub fn retain(&self, record: GuiObservationRecord, lease: ReliableOutputLease) -> bool {
        if !self.is_live() {
            return false;
        }
        self.queue.borrow_mut().push(GuiObservationDelivery {
            record,
            lease,
        });
        true
    }

    pub fn observe(&self, subscription: GuiObservationSubscriptionId, effect: &GuiLocalEffect) {
        if !self.is_live() {
            return;
        }
        let Some(bytes) = self.effect_bytes(effect) else {
            self.account.fail(OutputFailure::InvalidPayload);
            return;
        };
        match self.account.reserve(OutputCharge {
            entries: 1,
            bytes,
        }) {
            Ok(lease) => {
                self.retain(
                    GuiObservationRecord::Effect {
                        subscription,
                        effect: Arc::new(effect.clone()),
                    },
                    lease,
                );
            }
            Err(OutputReserveError::Capacity) => self.account.fail(OutputFailure::Capacity),
            Err(_) => {}
        }
    }

    fn effect_bytes(&self, effect: &GuiLocalEffect) -> Option<usize> {
        let mut bytes = size_of::<QueueNode>()
            .checked_add(size_of::<GuiLocalEffect>())?
            .checked_add(4 * size_of::<usize>())?
            .checked_add(
                effect
                    .ancestry
                    .len()
                    .checked_mul(size_of::<crate::EntityId>())?,
            )?;
        let mut text_length = 0;
        if let super::super::local::GuiLocalEffectKind::Submitted(text)
        | super::super::local::GuiLocalEffectKind::Rejected(text)
        | super::super::local::GuiLocalEffectKind::Discarded(text) = &effect.kind
        {
            bytes = bytes.checked_add(text.len())?;
            text_length = text.len();
        }
        bytes
            .checked_add(self.encoding.effect_bytes)?
            .checked_add(
                effect
                    .ancestry
                    .len()
                    .checked_mul(self.encoding.ancestry_entry_bytes)?,
            )?
            .checked_add(text_length.checked_mul(self.encoding.text_byte_bytes)?)
    }
}
