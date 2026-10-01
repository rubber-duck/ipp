//! Ordered GUI registration cuts and immutable effects sharing the physical outbox account.

use super::*;
use ipp_core::services::reliable_output::{
    OutputCharge, OutputFailure, OutputStatus, ReliableOutputLease,
};
use ipp_core::systems::gui::{GuiSystem, observations::*};
use ipp_protocol::gui::{GUI_OBSERVATION_ENCODING, GuiObservationRequest};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub(crate) struct SessionObservations {
    output: GuiObservationOutput,
    subscriptions: BTreeMap<(u64, u64), Registration>,
}

struct Registration {
    subscription: GuiObservationSubscription,
    _metadata: ReliableOutputLease,
}

fn encoded_bound(record: &GuiObservationRecord) -> Option<usize> {
    let encoding = GUI_OBSERVATION_ENCODING;
    match record {
        GuiObservationRecord::Control {
            ..
        } => Some(encoding.control_bytes),
        GuiObservationRecord::Effect {
            effect,
            ..
        } => {
            let text_bytes = match &effect.kind {
                ipp_core::systems::gui::local::GuiLocalEffectKind::Submitted(text) => text.len(),
                _ => 0,
            };
            encoding
                .effect_bytes
                .checked_add(
                    effect
                        .ancestry
                        .len()
                        .checked_mul(encoding.ancestry_entry_bytes)?,
                )?
                .checked_add(text_bytes.checked_mul(encoding.text_byte_bytes)?)
        }
    }
}

impl SessionObservations {
    pub(crate) fn close(&self) {
        self.output.close();
    }
}

impl<P: HostServices> WorldSessionContext<'_, P> {
    pub(crate) fn prepare_gui_observation(
        &mut self,
        request: u64,
        control: GuiObservationRequest,
    ) -> Result<(), String> {
        if !self
            .world
            .manifest()
            .supports_operation(ipp_core::systems::WorldOperation::Gui)
        {
            return Err("GUI is not selected".into());
        }
        let world = self.world.world_ref();
        let (GuiObservationRequest::Subscribe {
            world: requested,
            ..
        }
        | GuiObservationRequest::Unsubscribe {
            world: requested,
            ..
        }) = &control;
        if *requested != world.into() {
            return Err("GUI subscription World does not match its session".into());
        }
        if self.session.gui_observations.is_none() {
            let mut encoding = GUI_OBSERVATION_ENCODING;
            encoding.control_bytes += reliable_output::RESPONSE_METADATA_BYTES;
            encoding.effect_bytes += reliable_output::RESPONSE_METADATA_BYTES;
            self.session.gui_observations = Some(SessionObservations {
                output: GuiObservationOutput::new(self.session.reply_budget.0.clone(), encoding)
                    .map_err(|error| format!("GUI output unavailable: {error:?}"))?,
                subscriptions: BTreeMap::new(),
            });
        }
        let observations = self
            .session
            .gui_observations
            .as_mut()
            .expect("initialized observations");
        let registration = match control {
            GuiObservationRequest::Subscribe {
                classes,
                ..
            } => {
                let metadata = self
                    .session
                    .reply_budget
                    .0
                    .reserve(OutputCharge {
                        entries: 0,
                        bytes: reliable_output::RESPONSE_METADATA_BYTES,
                    })
                    .map_err(|error| format!("GUI registration unavailable: {error:?}"))?;
                Some(Registration {
                    subscription: observations
                        .output
                        .new_subscription(world, classes)
                        .map_err(|error| format!("GUI registration unavailable: {error:?}"))?,
                    _metadata: metadata,
                })
            }
            GuiObservationRequest::Unsubscribe {
                ..
            } => None,
        };
        let subscription = match &registration {
            Some(registration) => &registration.subscription,
            None => {
                let GuiObservationRequest::Unsubscribe {
                    subscription,
                    ..
                } = control
                else {
                    unreachable!()
                };
                &observations
                    .subscriptions
                    .get(&(subscription.output, subscription.generation))
                    .ok_or("Stale GUI subscription")?
                    .subscription
            }
        };
        let id = subscription.id();
        let original = self.session.request_origins[&request].0;
        let reservation = self
            .session
            .reply_reservations
            .remove(&request)
            .ok_or("Missing GUI control reservation")?;
        let reservation = Rc::try_unwrap(reservation)
            .map_err(|_| "GUI control reservation is aliased")?
            .into_inner();
        let lease = reservation.into_lease();
        let command = if registration.is_some() {
            GuiObservationCommand::prepare_subscribe(
                &observations.output,
                world,
                subscription,
                original,
                lease,
            )
        } else {
            GuiObservationCommand::prepare_unsubscribe(
                &observations.output,
                world,
                subscription,
                original,
                lease,
            )
        };
        let command = match command {
            Ok(command) => command,
            Err(error) => {
                self.session.reply_reservations.insert(
                    request,
                    Rc::new(RefCell::new(
                        attachment_receipts::ReplyReservation::from_observation(
                            error.lease,
                            self.session.reply_budget.0.clone(),
                            256,
                        ),
                    )),
                );
                return Err(format!(
                    "GUI registration admission failed: {:?}",
                    error.reason
                ));
            }
        };
        if let Some(registration) = registration {
            observations
                .subscriptions
                .insert((id.output, id.generation), registration);
        }
        self.session.request_origins.remove(&request);
        let _ = self
            .world
            .enqueue_system_command(GuiSystem::ID, self.session.id, command);
        Ok(())
    }
}

impl<P: HostServices> Host<P> {
    pub(crate) fn drain_gui_observations(&mut self) -> Vec<(u64, String)> {
        let mut failures = Vec::new();
        for (&id, session) in &mut self.sessions {
            let Some(observations) = &mut session.gui_observations else {
                continue;
            };
            let result = (|| {
                while let Some(delivery) = observations.output.pop_front() {
                    let (record, lease) = delivery.into_parts();
                    let request_id =
                        match &record {
                            GuiObservationRecord::Control {
                                subscription,
                                request,
                                ..
                            } => {
                                let key = (subscription.output, subscription.generation);
                                if observations.subscriptions.get(&key).is_some_and(
                                    |registration| !registration.subscription.is_active(),
                                ) {
                                    observations.subscriptions.remove(&key);
                                }
                                *request
                            }
                            GuiObservationRecord::Effect {
                                ..
                            } => 0,
                        };
                    let bound = encoded_bound(&record)
                        .ok_or("GUI observation encoding bound overflowed")?;
                    let response = Response {
                        session: id,
                        request_id,
                        tick: 0,
                        body: ResponseBody::GuiObservation(record),
                    };
                    let size = ipp_protocol::encoded_response_size(&response)
                        .map_err(|error| error.to_string())?;
                    if size > bound
                        || size + reliable_output::RESPONSE_METADATA_BYTES > lease.charge().bytes
                    {
                        return Err(
                            "GUI observation exceeded its reserved encoded allocation".into()
                        );
                    }
                    let bytes = ipp_protocol::encode_response(&response)
                        .map_err(|error| error.to_string())?;
                    drop(response);
                    let reservation = attachment_receipts::ReplyReservation::from_observation(
                        lease,
                        session.reply_budget.0.clone(),
                        bytes.capacity(),
                    );
                    session.outbox.push_back(QueuedResponse {
                        bytes,
                        reservation: Rc::new(RefCell::new(reservation)),
                    });
                }
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                session.reply_budget.0.fail(OutputFailure::InvalidPayload);
                failures.push((id, error));
            } else if let OutputStatus::Failed(reason) = session.reply_budget.0.status() {
                let usage = session.reply_budget.0.usage();
                failures.push((
                    id,
                    format!(
                        "connection congestion: GUI observation delivery failed: {reason:?} entries={} bytes={}",
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
