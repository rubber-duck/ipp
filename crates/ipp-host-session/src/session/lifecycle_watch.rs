//! Fixed-session target membership, sole typed ACKs and lease-preserving delivery.

use crate::*;
use ipp_core::services::reliable_output::{
    OutputCharge, OutputFailure, OutputStatus, ReliableOutputLease,
};
use ipp_core::systems::lifecycle_publisher::*;
use ipp_protocol::world::lifecycle_watch::{
    self as wire, LifecycleWatchChange, LifecycleWatchRequest,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};

/// Host rejection reason for a watch on a World that does not select the publisher.
pub(crate) const LIFECYCLE_WATCH_UNSUPPORTED: &str =
    "Lifecycle watches are unsupported: this World does not select ipp.lifecycle-publisher";

#[cfg(test)]
#[path = "lifecycle_watch_tests.rs"]
mod tests;

pub(crate) struct SessionLifecycleWatch {
    output: LifecycleWatchOutput,
    identity: Option<u64>,
    members: BTreeMap<u64, Registration>,
    pending_adds: BTreeMap<u64, PendingAdd>,
    /// The last value record handed to the outbox, until its transport completes it.
    delivering_value: Option<Weak<RefCell<reliable_output::ReplyReservation>>>,
    #[cfg(test)]
    registration_visits: usize,
}

struct Registration {
    member: LifecycleWatchMember,
    _metadata: ReliableOutputLease,
}

struct PendingAdd {
    generations: Vec<u64>,
    _metadata: ReliableOutputLease,
}

impl SessionLifecycleWatch {
    pub(crate) fn close(&self) {
        self.output.close();
    }

    /// Value records are state and wait in Core while the transport has not completed the
    /// last value record handed off, so a newer value replaces an undelivered one there.
    fn values_waiting(&self) -> bool {
        self.delivering_value
            .as_ref()
            .is_some_and(|reservation| reservation.strong_count() != 0)
    }

    fn discard_add(&mut self, request: u64) {
        if let Some(pending) = self.pending_adds.remove(&request) {
            for generation in &pending.generations {
                #[cfg(test)]
                {
                    self.registration_visits += 1;
                }
                self.members.remove(generation);
            }
        }
    }

    fn acknowledge_add(
        &mut self,
        request: u64,
        baselines: &[LifecycleMembershipBaseline],
    ) -> Result<(), String> {
        let pending = self
            .pending_adds
            .remove(&request)
            .ok_or("Missing lifecycle add request")?;
        if pending.generations.len() != baselines.len() {
            return Err("Lifecycle add baseline count mismatch".into());
        }
        for (generation, baseline) in pending.generations.iter().zip(baselines) {
            #[cfg(test)]
            {
                self.registration_visits += 1;
            }
            let entry = self
                .members
                .get(generation)
                .ok_or("Missing lifecycle add member")?;
            if entry.member.id() != baseline.member || entry.member.target() != baseline.target {
                return Err("Lifecycle add baseline identity mismatch".into());
            }
        }
        Ok(())
    }
}

impl<P: HostServices> WorldSessionContext<'_, P> {
    pub(crate) fn lifecycle_diagnostics(
        &self,
        query: ipp_protocol::world::lifecycle_diagnostics::LifecycleDiagnosticQuery,
    ) -> ResponseBody {
        let result = (|| {
            if query.world != self.world.world_ref().into() {
                return Err("Lifecycle diagnostic World does not match its session");
            }
            let watch = self
                .session
                .lifecycle_watch
                .as_ref()
                .ok_or("Missing lifecycle endpoint")?;
            if watch.identity != Some(query.output) {
                return Err("Stale lifecycle diagnostic endpoint");
            }
            let traffic = watch.output.traffic().ok_or("Lifecycle tracking ended")?;
            let system = self
                .world
                .system::<LifecyclePublisherSystem>(LifecyclePublisherSystem::ID)
                .ok_or("Lifecycle publisher is not selected")?;
            Ok(
                ipp_protocol::world::lifecycle_diagnostics::LifecycleDiagnosticSample {
                    endpoint: query,
                    work: system.target_work(),
                    traffic,
                },
            )
        })();
        match result {
            Ok(sample) => ResponseBody::LifecycleDiagnostics(sample),
            Err(message) => ResponseBody::Error {
                code: 1,
                message: message.into(),
            },
        }
    }

    pub(crate) fn prepare_lifecycle_watch(
        &mut self,
        request: u64,
        control: LifecycleWatchRequest,
    ) -> Result<(), String> {
        let original = self.session.request_origins[&request].0;
        let result = self.prepare_lifecycle_membership(request, control);
        if result.is_err()
            && let Some(watch) = &mut self.session.lifecycle_watch
        {
            watch.discard_add(original);
        }
        result
    }

    fn prepare_lifecycle_membership(
        &mut self,
        request: u64,
        control: LifecycleWatchRequest,
    ) -> Result<(), String> {
        let world = self.world.world_ref();
        if control.world != world.into() {
            return Err("Lifecycle World does not match its session".into());
        }
        // Refused before any endpoint or member exists, so nothing needs undoing.
        if !self
            .world
            .system_ids()
            .any(|id| id == LifecyclePublisherSystem::ID)
        {
            return Err(LIFECYCLE_WATCH_UNSUPPORTED.into());
        }
        if self.session.lifecycle_watch.is_none() {
            let mut encoding = wire::LIFECYCLE_WATCH_ENCODING;
            encoding.acknowledgement_bytes += reliable_output::RESPONSE_METADATA_BYTES;
            encoding.event_bytes += reliable_output::RESPONSE_METADATA_BYTES;
            encoding.value_bytes += reliable_output::RESPONSE_METADATA_BYTES;
            self.session.lifecycle_watch = Some(SessionLifecycleWatch {
                output: LifecycleWatchOutput::new(
                    self.session.reply_budget.0.clone(),
                    world,
                    self.session.id,
                    encoding,
                )
                .map_err(|error| format!("Lifecycle output unavailable: {error:?}"))?,
                identity: None,
                members: BTreeMap::new(),
                pending_adds: BTreeMap::new(),
                delivering_value: None,
                #[cfg(test)]
                registration_visits: 0,
            });
        }
        let watch = self
            .session
            .lifecycle_watch
            .as_mut()
            .expect("initialized lifecycle output");
        let original = self.session.request_origins[&request].0;
        let count = match &control.change {
            LifecycleWatchChange::Add(targets) => targets.len(),
            LifecycleWatchChange::Remove {
                generations,
                ..
            } => generations.len(),
        };
        let _preparation = watch
            .output
            .account()
            .reserve(OutputCharge {
                entries: 0,
                bytes: count
                    .checked_mul(std::mem::size_of::<LifecycleWatchMember>() * 2)
                    .ok_or("Lifecycle request allocation overflow")?,
            })
            .map_err(|error| format!("Lifecycle request unavailable: {error:?}"))?;
        let mut members = Vec::with_capacity(count);
        let action = match control.change {
            LifecycleWatchChange::Add(targets) => {
                let metadata = watch
                    .output
                    .account()
                    .reserve(OutputCharge {
                        entries: 0,
                        bytes: count
                            .checked_mul(std::mem::size_of::<u64>())
                            .and_then(|bytes| {
                                bytes.checked_add(
                                    std::mem::size_of::<PendingAdd>()
                                        + reliable_output::RESPONSE_METADATA_BYTES,
                                )
                            })
                            .ok_or("Lifecycle add allocation overflow")?,
                    })
                    .map_err(|error| format!("Lifecycle add unavailable: {error:?}"))?;
                watch.pending_adds.insert(
                    original,
                    PendingAdd {
                        generations: Vec::with_capacity(count),
                        _metadata: metadata,
                    },
                );
                let pending = watch
                    .pending_adds
                    .get_mut(&original)
                    .expect("prepared add request");
                for (target, kinds) in targets {
                    let metadata = watch
                        .output
                        .account()
                        .reserve(OutputCharge {
                            entries: 0,
                            bytes: std::mem::size_of::<Registration>()
                                + reliable_output::RESPONSE_METADATA_BYTES,
                        })
                        .map_err(|error| format!("Lifecycle member unavailable: {error:?}"))?;
                    let member = watch
                        .output
                        .new_member(target, kinds)
                        .map_err(|error| error.to_string())?;
                    watch.identity = Some(member.id().output);
                    pending.generations.push(member.id().generation);
                    members.push(member.clone());
                    watch.members.insert(
                        member.id().generation,
                        Registration {
                            member,
                            _metadata: metadata,
                        },
                    );
                }
                LifecycleMembershipAction::Add
            }
            LifecycleWatchChange::Remove {
                output,
                generations,
            } => {
                if watch.identity != Some(output) {
                    return Err("Stale lifecycle endpoint".into());
                }
                for generation in generations {
                    members.push(
                        watch
                            .members
                            .get(&generation)
                            .ok_or("Stale lifecycle member")?
                            .member
                            .clone(),
                    );
                }
                LifecycleMembershipAction::Remove
            }
        };
        let reservation = self
            .session
            .reply_reservations
            .remove(&request)
            .ok_or("Missing lifecycle reservation")?;
        let lease = Rc::try_unwrap(reservation)
            .map_err(|_| "Aliased lifecycle reservation")?
            .into_inner()
            .into_lease();
        let command = match LifecycleMembershipCommand::prepare(
            &watch.output,
            action,
            members,
            original,
            lease,
        ) {
            Ok(command) => command,
            Err(error) => {
                self.session.reply_reservations.insert(
                    request,
                    Rc::new(RefCell::new(
                        reliable_output::ReplyReservation::from_observation(
                            error.lease,
                            self.session.reply_budget.0.clone(),
                            256,
                        ),
                    )),
                );
                return Err(format!(
                    "Lifecycle membership admission failed: {:?}",
                    error.reason
                ));
            }
        };
        self.session.request_origins.remove(&request);
        // The publisher is selected, so only an exhausted World queue refuses the command.
        // Its drop still ACKs the request as cancelled; the exhaustion fails the connection.
        if let Err(reason) = self.world.enqueue_system_command(
            LifecyclePublisherSystem::ID,
            self.session.id,
            command,
        ) {
            self.session
                .pending_errors
                .insert(0, format!("Lifecycle watch refused: {reason}"));
        }
        Ok(())
    }
}

impl<P: HostServices> Host<P> {
    pub(crate) fn drain_lifecycle_watches(&mut self) -> Vec<(u64, String)> {
        let mut failures = Vec::new();
        for (&id, session) in &mut self.sessions {
            let Some(watch) = &mut session.lifecycle_watch else {
                continue;
            };
            // Decided once per drain: a transport that completed the last value record
            // receives every record queued since, including this frame's values. While it
            // lags, draining stops at the oldest value record, keeping Core's order, and
            // later values replace undelivered ones.
            let values_waiting = watch.values_waiting();
            let result = (|| {
                loop {
                    if values_waiting && watch.output.front_is_value() {
                        break;
                    }
                    let Some(delivery) = watch.output.pop_front() else {
                        break;
                    };
                    let (record, lease) = delivery.into_parts();
                    if record.session != id {
                        return Err("Lifecycle session mismatch".into());
                    }
                    if let LifecycleWatchRecordBody::Acknowledgement {
                        request,
                        action,
                        result,
                        ..
                    } = &record.body
                    {
                        match (action, result) {
                            (
                                LifecycleMembershipAction::Add,
                                LifecycleMembershipResult::Applied(baselines),
                            ) => {
                                watch.acknowledge_add(*request, baselines)?;
                            }
                            (LifecycleMembershipAction::Add, _) => {
                                watch.discard_add(*request);
                            }
                            (
                                LifecycleMembershipAction::Remove,
                                LifecycleMembershipResult::Applied(baselines),
                            ) => {
                                for baseline in baselines {
                                    watch.members.remove(&baseline.member.generation);
                                }
                            }
                            _ => {}
                        }
                    }
                    let output = watch
                        .identity
                        .ok_or("Missing lifecycle endpoint identity")?;
                    let size =
                        wire::encoded_size(output, &record).map_err(|error| error.to_string())?;
                    let encoding = wire::LIFECYCLE_WATCH_ENCODING;
                    let bound = match &record.body {
                        LifecycleWatchRecordBody::Acknowledgement {
                            result: LifecycleMembershipResult::Applied(baselines),
                            ..
                        } => baselines.iter().try_fold(
                            wire::LIFECYCLE_ACK_BYTES,
                            |bytes, baseline| {
                                bytes.checked_add(encoding.baseline_capacity(&baseline.target)?)
                            },
                        ),
                        LifecycleWatchRecordBody::Acknowledgement {
                            ..
                        } => Some(wire::LIFECYCLE_ACK_BYTES),
                        LifecycleWatchRecordBody::Event {
                            ..
                        } => Some(encoding.event_bytes),
                        LifecycleWatchRecordBody::Value {
                            values,
                            ..
                        } => encoding.value_capacity(values.as_deref()),
                    }
                    .ok_or("Lifecycle record exceeds its encoding bound")?;
                    let value = matches!(&record.body, LifecycleWatchRecordBody::Value { .. });
                    if size > bound
                        || size + reliable_output::RESPONSE_METADATA_BYTES > lease.charge().bytes
                    {
                        return Err("Lifecycle reply exceeded its pre-mutation credit".into());
                    }
                    let mut bytes = Vec::with_capacity(size);
                    wire::encode_into(output, &record, &mut bytes)
                        .map_err(|error| error.to_string())?;
                    drop(record);
                    let reservation = reliable_output::ReplyReservation::from_observation(
                        lease,
                        session.reply_budget.0.clone(),
                        bytes.capacity(),
                    );
                    let reservation = Rc::new(RefCell::new(reservation));
                    if value {
                        watch.delivering_value = Some(Rc::downgrade(&reservation));
                    }
                    session.outbox.push_back(QueuedResponse {
                        bytes,
                        reservation,
                    });
                }
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                session.reply_budget.0.fail(OutputFailure::InvalidPayload);
                failures.push((id, error));
            } else if let OutputStatus::Failed(reason) = session.reply_budget.0.status() {
                let usage = session.reply_budget.0.usage();
                let congestion = if reason == OutputFailure::Capacity {
                    "connection congestion: "
                } else {
                    ""
                };
                failures.push((
                    id,
                    format!(
                        "{congestion}Lifecycle delivery unavailable: {reason:?} entries={} bytes={}",
                        usage.entries, usage.bytes
                    ),
                ));
            }
        }
        failures
            .into_iter()
            .map(|(session, error)| (self.connection_for_session(session), error))
            .collect()
    }
}
