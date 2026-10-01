//! Connection-owned reliable output accounting, independent of queues and codecs.
//!
//! One account bounds everything a physical connection retains for reliable delivery by bytes.
//! Owners charge each record's payload, peak encoding, copied transport storage and their own
//! per-record bookkeeping, so the byte limit also bounds how many records can accumulate; there
//! is no separate record limit. Reply leases may use a reserve that ordinary output cannot, so an
//! owner that reserves each reply before admitting correlated work can always answer it.

use std::cell::RefCell;
use std::rc::Rc;

/// Simultaneously retained output records and allocation bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputCharge {
    /// Retained queued or in-flight records. Counted for request admission, ownership checks
    /// and diagnostics, never limited by the account: each record's bytes include bookkeeping.
    pub entries: usize,
    /// Retained allocation capacity, including simultaneous encoding storage.
    pub bytes: usize,
}

impl OutputCharge {
    /// Replace `previous` with `next` in this total without overflow.
    fn replace(self, previous: Self, next: Self) -> Option<Self> {
        Some(Self {
            entries: (self.entries - previous.entries).checked_add(next.entries)?,
            bytes: (self.bytes - previous.bytes).checked_add(next.bytes)?,
        })
    }

    /// This charge if it retains a record, otherwise nothing.
    fn record(self) -> Self {
        if self.entries == 0 {
            Self::default()
        } else {
            self
        }
    }
}

/// Host-selected connection limits, independent of individual message limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputLimits {
    /// Maximum simultaneously retained bytes across all leases.
    pub bytes: usize,
    /// Part of `bytes` that only [`OutputClass::Reply`] leases may use.
    pub reply_reserve: usize,
}

/// Which part of the byte limit a lease may grow into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputClass {
    /// Events, notices, progress and metadata; leaves the reply reserve untouched.
    Ordinary,
    /// A reply to admitted correlated work, including its transport copies.
    Reply,
}

/// A connection failure to be handled by its Host outside publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFailure {
    /// Reliable delivery exhausted its bounded allocation capacity.
    Capacity,
    /// Retained or encoded content cannot satisfy its declared representation.
    InvalidPayload,
}

/// Closing never erases a previously recorded failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStatus {
    /// Reservations may grow within the configured limits.
    Open,
    /// The owner closed delivery; outstanding leases remain charged.
    Closed,
    /// The first failure remains visible even after close.
    Failed(OutputFailure),
}

/// A failed reservation leaves both the account and existing leases unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputReserveError {
    /// Limits or arithmetic would be exceeded.
    Capacity,
    /// The owner closed the account.
    Closed,
    /// The account previously failed.
    Failed(OutputFailure),
}

struct AccountState {
    limits: OutputLimits,
    usage: OutputCharge,
    replies: OutputCharge,
    records: OutputCharge,
    status: OutputStatus,
}

/// Shared accounting and a sticky failure latch; contains no delivery callbacks.
#[derive(Clone)]
pub struct ReliableOutputAccount(Rc<RefCell<AccountState>>);

impl ReliableOutputAccount {
    /// Create an open account with no outstanding reservations.
    pub fn new(limits: OutputLimits) -> Self {
        Self(Rc::new(RefCell::new(AccountState {
            limits,
            usage: OutputCharge::default(),
            replies: OutputCharge::default(),
            records: OutputCharge::default(),
            status: OutputStatus::Open,
        })))
    }

    /// Atomically reserve ordinary output; zero-entry metadata leases are valid.
    pub fn reserve(&self, charge: OutputCharge) -> Result<ReliableOutputLease, OutputReserveError> {
        self.reserve_as(OutputClass::Ordinary, charge)
    }

    /// Atomically reserve credit that may grow only within `class`'s share of the limit.
    pub fn reserve_as(
        &self,
        class: OutputClass,
        charge: OutputCharge,
    ) -> Result<ReliableOutputLease, OutputReserveError> {
        self.adjust(class, OutputCharge::default(), charge, true)?;
        Ok(ReliableOutputLease {
            account: self.clone(),
            charge,
            class,
        })
    }

    /// Includes leases retained after close or failure.
    pub fn usage(&self) -> OutputCharge {
        self.0.borrow().usage
    }

    /// The part of [`Self::usage`] held by [`OutputClass::Reply`] leases, such as replies to
    /// admitted requests that are not yet physically complete.
    pub fn reply_usage(&self) -> OutputCharge {
        self.0.borrow().replies
    }

    /// The part of [`Self::usage`] held by leases for retained records (nonzero entries), as
    /// opposed to endpoint, membership or transport metadata.
    pub fn record_usage(&self) -> OutputCharge {
        self.0.borrow().records
    }

    /// Inspect liveness or the sticky first failure without draining it.
    pub fn status(&self) -> OutputStatus {
        self.0.borrow().status
    }

    /// Latch the first failure without cleanup or external callbacks.
    pub fn fail(&self, reason: OutputFailure) {
        let mut state = self.0.borrow_mut();
        if !matches!(state.status, OutputStatus::Failed(_)) {
            state.status = OutputStatus::Failed(reason);
        }
    }

    /// Stop new reservations without erasing failures or outstanding usage.
    pub fn close(&self) {
        let mut state = self.0.borrow_mut();
        if state.status == OutputStatus::Open {
            state.status = OutputStatus::Closed;
        }
    }

    fn adjust(
        &self,
        class: OutputClass,
        previous: OutputCharge,
        next: OutputCharge,
        reservation: bool,
    ) -> Result<(), OutputReserveError> {
        let mut state = self.0.borrow_mut();
        let growing = next.bytes > previous.bytes;
        if reservation || growing || next.entries > previous.entries {
            match state.status {
                OutputStatus::Open => {}
                OutputStatus::Closed => return Err(OutputReserveError::Closed),
                OutputStatus::Failed(reason) => return Err(OutputReserveError::Failed(reason)),
            }
        }

        let usage = state
            .usage
            .replace(previous, next)
            .ok_or(OutputReserveError::Capacity)?;
        let replies = match class {
            OutputClass::Ordinary => state.replies,
            OutputClass::Reply => state
                .replies
                .replace(previous, next)
                .ok_or(OutputReserveError::Capacity)?,
        };
        let records = state
            .records
            .replace(previous.record(), next.record())
            .ok_or(OutputReserveError::Capacity)?;

        // Shrinking stays valid even while reply leases hold total usage above the ordinary share.
        let ceiling = match class {
            OutputClass::Ordinary => state
                .limits
                .bytes
                .saturating_sub(state.limits.reply_reserve),
            OutputClass::Reply => state.limits.bytes,
        };
        if growing && usage.bytes > ceiling {
            return Err(OutputReserveError::Capacity);
        }

        state.usage = usage;
        state.replies = replies;
        state.records = records;
        Ok(())
    }
}

/// A non-Clone reservation transferred between retention and transport ownership.
pub struct ReliableOutputLease {
    account: ReliableOutputAccount,
    charge: OutputCharge,
    class: OutputClass,
}

impl ReliableOutputLease {
    /// This lease's current contribution to account usage.
    pub fn charge(&self) -> OutputCharge {
        self.charge
    }

    /// The share of the account limit this lease may grow into.
    pub fn class(&self) -> OutputClass {
        self.class
    }

    /// Compare exact account lifetime rather than limits or current usage.
    pub fn belongs_to(&self, account: &ReliableOutputAccount) -> bool {
        Rc::ptr_eq(&self.account.0, &account.0)
    }

    /// Atomically resize within this lease's class; shrinking remains valid after close/failure.
    pub fn resize(&mut self, charge: OutputCharge) -> Result<(), OutputReserveError> {
        self.account
            .adjust(self.class, self.charge, charge, false)?;
        self.charge = charge;
        Ok(())
    }
}

impl Drop for ReliableOutputLease {
    fn drop(&mut self) {
        let mut state = self.account.0.borrow_mut();
        state.usage.entries -= self.charge.entries;
        state.usage.bytes -= self.charge.bytes;
        if self.class == OutputClass::Reply {
            state.replies.entries -= self.charge.entries;
            state.replies.bytes -= self.charge.bytes;
        }
        let record = self.charge.record();
        state.records.entries -= record.entries;
        state.records.bytes -= record.bytes;
    }
}

#[cfg(test)]
#[path = "reliable_output_tests.rs"]
mod tests;
