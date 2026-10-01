use super::output::LifecycleMemberStatus;
use super::*;
use crate::services::reliable_output::{OutputCharge, OutputReserveError, ReliableOutputLease};
use std::cell::RefCell;
use std::mem::size_of;
use std::rc::Rc;

/// Preparation failed before the sole reply reservation transferred to the command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleMembershipPrepareFailure {
    /// Zero correlation, empty/unsorted page, or a member from a different endpoint.
    InvalidIdentity,
    /// Reply reservation belongs to a different account or is not exactly one entry.
    InvalidLease,
    /// Checked capacity arithmetic or payload allocation failed.
    InvalidPayload,
    /// The connection account cannot retain the request and reply payloads.
    Output(OutputReserveError),
}

/// Return the original Host reservation when preparation fails before admission.
pub struct LifecycleMembershipPrepareError {
    /// Failure before any membership changes.
    pub reason: LifecycleMembershipPrepareFailure,
    /// The sole reply reservation remains owned by the caller.
    pub lease: ReliableOutputLease,
}

impl std::fmt::Debug for LifecycleMembershipPrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.reason.fmt(formatter)
    }
}

pub(super) struct LifecyclePendingMembership {
    pub members: Vec<LifecycleWatchMember>,
    pub baselines: Vec<LifecycleMembershipBaseline>,
    pub lease: ReliableOutputLease,
}

/// Inert, owned membership page for the publisher's ordinary ordered command ingress.
/// Enqueue with no generic command reply: this command owns its sole correlated ACK.
pub struct LifecycleMembershipCommand {
    pub(super) output: LifecycleWatchOutput,
    pub(super) action: LifecycleMembershipAction,
    pub(super) request: u64,
    pending: RefCell<Option<LifecyclePendingMembership>>,
    _metadata: ReliableOutputLease,
}

impl LifecycleMembershipCommand {
    /// Reserve owned request storage and the entire baseline reply before admission.
    /// Members must be sorted by generation, with no duplicates. Hosts byte-page both
    /// request and reply representations; the total live set has no fixed count limit.
    pub fn prepare(
        output: &LifecycleWatchOutput,
        action: LifecycleMembershipAction,
        members: Vec<LifecycleWatchMember>,
        request: u64,
        mut lease: ReliableOutputLease,
    ) -> Result<Self, LifecycleMembershipPrepareError> {
        let prepared = (|| {
            if request == 0
                || members.is_empty()
                || members.windows(2).any(|pair| pair[0].id() >= pair[1].id())
                || members
                    .iter()
                    .any(|member| member.0.output.as_ptr() != Rc::as_ptr(&output.0))
            {
                return Err(LifecycleMembershipPrepareFailure::InvalidIdentity);
            }

            if !lease.belongs_to(output.account()) || lease.charge().entries != 1 {
                return Err(LifecycleMembershipPrepareFailure::InvalidLease);
            }

            let metadata_bytes = size_of::<Self>()
                .checked_add(
                    members
                        .capacity()
                        .checked_mul(size_of::<LifecycleWatchMember>())
                        .ok_or(LifecycleMembershipPrepareFailure::InvalidPayload)?,
                )
                .ok_or(LifecycleMembershipPrepareFailure::InvalidPayload)?;
            let metadata = output
                .account()
                .reserve(OutputCharge {
                    entries: 0,
                    bytes: metadata_bytes,
                })
                .map_err(LifecycleMembershipPrepareFailure::Output)?;

            // Baselines echo their targets, including every field offset of a value target.
            let value_fields = members
                .iter()
                .map(|member| match &member.0.target {
                    LifecycleWatchTarget::Value(_, _, fields) => fields.len(),
                    LifecycleWatchTarget::Entity(_) | LifecycleWatchTarget::Component(_, _) => 0,
                })
                .try_fold(0usize, usize::checked_add)
                .ok_or(LifecycleMembershipPrepareFailure::InvalidPayload)?;

            let mut charge = output
                .acknowledgement_charge(members.len(), value_fields)
                .ok_or(LifecycleMembershipPrepareFailure::InvalidPayload)?;
            charge.bytes = charge.bytes.max(lease.charge().bytes);
            lease
                .resize(charge)
                .map_err(LifecycleMembershipPrepareFailure::Output)?;

            let mut baselines = Vec::new();
            baselines
                .try_reserve_exact(members.len())
                .map_err(|_| LifecycleMembershipPrepareFailure::InvalidPayload)?;

            let mut charge = output
                .acknowledgement_charge(baselines.capacity(), value_fields)
                .ok_or(LifecycleMembershipPrepareFailure::InvalidPayload)?;
            charge.bytes = charge.bytes.max(lease.charge().bytes);
            lease
                .resize(charge)
                .map_err(LifecycleMembershipPrepareFailure::Output)?;

            Ok((metadata, baselines))
        })();

        match prepared {
            Ok((metadata, baselines)) => Ok(Self {
                output: output.clone(),
                action,
                request,
                pending: RefCell::new(Some(LifecyclePendingMembership {
                    members,
                    baselines,
                    lease,
                })),
                _metadata: metadata,
            }),
            Err(reason) => Err(LifecycleMembershipPrepareError {
                reason,
                lease,
            }),
        }
    }

    /// Fixed World used by the adapter to select ingress.
    pub fn world(&self) -> crate::WorldRef {
        self.output.0.world
    }

    /// Fixed authoring session; reusing this command in another session is rejected.
    pub fn session(&self) -> u64 {
        self.output.0.session
    }

    pub(super) fn take(&self) -> Option<LifecyclePendingMembership> {
        self.pending.borrow_mut().take()
    }

    pub(super) fn finish(
        &self,
        cut: Option<(u64, u64)>,
        result: LifecycleMembershipResult,
        lease: ReliableOutputLease,
    ) {
        self.output.0.retain(
            LifecycleWatchRecordBody::Acknowledgement {
                request: self.request,
                action: self.action,
                cut,
                result,
            },
            lease,
        );
    }
}

impl Drop for LifecycleMembershipCommand {
    fn drop(&mut self) {
        if let Some(pending) = self.take() {
            if self.action == LifecycleMembershipAction::Add {
                for member in &pending.members {
                    if member.0.status.get() == LifecycleMemberStatus::Pending {
                        member.0.status.set(LifecycleMemberStatus::Removed);
                    }
                }
            }

            self.finish(None, LifecycleMembershipResult::Cancelled, pending.lease);
        }
    }
}
