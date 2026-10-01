use super::output::{OutputState, SubscriptionStatus};
use super::*;
use crate::services::reliable_output::{OutputCharge, OutputReserveError, ReliableOutputLease};
use std::cell::RefCell;
use std::rc::Rc;

/// Preparation failed before ownership of the sole control-result lease transferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiObservationPrepareFailure {
    /// Output, World, subscription or nonzero request identity does not match.
    InvalidIdentity,
    /// The supplied lease is not one entry from this output's connection account.
    InvalidLease,
    /// Checked allocation-size arithmetic overflowed.
    InvalidPayload,
    /// Delivery is closed, failed, or lacks required capacity.
    Output(OutputReserveError),
}

/// Pre-admission failure returns the original reserved control-result capacity.
pub struct GuiObservationPrepareError {
    /// Why no command was prepared.
    pub reason: GuiObservationPrepareFailure,
    /// Original reservation for adapter handling; never silently replaced.
    pub lease: ReliableOutputLease,
}

impl std::fmt::Debug for GuiObservationPrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.reason.fmt(formatter)
    }
}

/// Inert fenced command for GuiSystem's ordinary ordered command queue.
pub struct GuiObservationCommand {
    pub(super) output: Rc<OutputState>,
    pub(super) subscription: GuiObservationSubscription,
    pub(super) world: WorldRef,
    pub(super) subscribe: bool,
    request: u64,
    lease: RefCell<Option<ReliableOutputLease>>,
    _metadata: ReliableOutputLease,
}

impl GuiObservationCommand {
    /// Prepare without activating. Enqueue through GuiSystem without another command reply.
    pub fn prepare_subscribe(
        output: &GuiObservationOutput,
        world: WorldRef,
        subscription: &GuiObservationSubscription,
        request: u64,
        lease: ReliableOutputLease,
    ) -> Result<Self, GuiObservationPrepareError> {
        Self::prepare(output, world, subscription, request, lease, true)
    }

    /// Prepare without removing. The ordered receiver appends the sole result to the FIFO.
    pub fn prepare_unsubscribe(
        output: &GuiObservationOutput,
        world: WorldRef,
        subscription: &GuiObservationSubscription,
        request: u64,
        lease: ReliableOutputLease,
    ) -> Result<Self, GuiObservationPrepareError> {
        Self::prepare(output, world, subscription, request, lease, false)
    }

    fn prepare(
        output: &GuiObservationOutput,
        world: WorldRef,
        subscription: &GuiObservationSubscription,
        request: u64,
        mut lease: ReliableOutputLease,
        subscribe: bool,
    ) -> Result<Self, GuiObservationPrepareError> {
        let validation = (|| {
            if request == 0
                || subscription.0.world != world
                || !subscription.0.output.belongs_to(&output.0)
            {
                return Err(GuiObservationPrepareFailure::InvalidIdentity);
            }
            if !lease.belongs_to(&output.0.account) || lease.charge().entries != 1 {
                return Err(GuiObservationPrepareFailure::InvalidLease);
            }
            output
                .0
                .admission()
                .map_err(GuiObservationPrepareFailure::Output)?;
            let mut charge = output
                .0
                .control_charge()
                .ok_or(GuiObservationPrepareFailure::InvalidPayload)?;
            charge.bytes = charge.bytes.max(lease.charge().bytes);
            let metadata = output
                .0
                .account
                .reserve(OutputCharge {
                    entries: 0,
                    bytes: std::mem::size_of::<Self>(),
                })
                .map_err(GuiObservationPrepareFailure::Output)?;
            lease
                .resize(charge)
                .map_err(GuiObservationPrepareFailure::Output)?;
            Ok(metadata)
        })();
        let metadata = match validation {
            Ok(metadata) => metadata,
            Err(reason) => {
                return Err(GuiObservationPrepareError {
                    reason,
                    lease,
                });
            }
        };
        Ok(Self {
            output: output.0.clone(),
            subscription: subscription.clone(),
            world,
            subscribe,
            request,
            lease: RefCell::new(Some(lease)),
            _metadata: metadata,
        })
    }

    /// Exact declared World, for adapter queue selection and lifetime validation.
    pub fn world(&self) -> WorldRef {
        self.world
    }

    pub(super) fn pending(&self) -> bool {
        self.lease.borrow().is_some()
    }

    pub(super) fn finish(&self, result: GuiObservationControlResult) -> bool {
        let Some(lease) = self.lease.borrow_mut().take() else {
            return false;
        };
        self.output.retain(
            GuiObservationRecord::Control {
                world: self.world,
                subscription: self.subscription.id(),
                request: self.request,
                result,
            },
            lease,
        )
    }
}

impl Drop for GuiObservationCommand {
    fn drop(&mut self) {
        if self.pending() {
            if self.subscribe && self.subscription.0.status.get() == SubscriptionStatus::Pending {
                self.subscription.0.status.set(SubscriptionStatus::Closed);
            }
            self.finish(GuiObservationControlResult::Cancelled);
        }
    }
}
