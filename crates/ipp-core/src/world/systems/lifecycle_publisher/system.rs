use super::output::LifecycleMemberStatus;
use super::target_index::LifecycleTargetIndex;
use super::*;

const MAX_SESSIONS: usize = 128;
const MAX_SUBSCRIPTIONS: usize = 64;

/// Observations wait here only until the Host drains the session, which charges them to the
/// connection's reliable output account.
#[derive(Default)]
struct LifecycleSession {
    subscriptions: BTreeMap<u64, LifecycleFilter>,
    observations: VecDeque<LifecyclePublication>,
}

/// World-owned subscription policy; no storage references or delivery callbacks escape.
#[derive(Default)]
pub struct LifecyclePublisherSystem {
    sessions: BTreeMap<u64, LifecycleSession>,
    sequence: u64,
    targets: LifecycleTargetIndex,
}

impl LifecyclePublisherSystem {
    /// Stable composition and generic command/event routing identity.
    pub const ID: SystemId = SystemId("ipp.lifecycle-publisher");

    /// Copy cumulative index work without visiting members or observing World time.
    pub fn target_work(&self) -> LifecycleTargetWork {
        self.targets.work
    }

    fn apply(
        &mut self,
        session: u64,
        command: &LifecyclePublisherCommand,
    ) -> Result<(), ErrorReason> {
        if session == 0
            || (!self.sessions.contains_key(&session) && self.sessions.len() == MAX_SESSIONS)
        {
            return Err(ErrorReason::Capacity);
        }
        let state = self.sessions.entry(session).or_default();
        match command {
            LifecyclePublisherCommand::Subscribe {
                subscription,
                filter,
            } => {
                if *subscription == 0 || state.subscriptions.contains_key(subscription) {
                    return Err(ErrorReason::InvalidValue);
                }
                if state.subscriptions.len() == MAX_SUBSCRIPTIONS {
                    return Err(ErrorReason::Capacity);
                }
                filter.validate()?;
                state.subscriptions.insert(*subscription, filter.clone());
            }
            LifecyclePublisherCommand::Unsubscribe {
                subscription,
            } => {
                if *subscription == 0 {
                    return Err(ErrorReason::InvalidValue);
                }
                state.subscriptions.remove(subscription);
                state
                    .observations
                    .retain(|event| event.subscription != *subscription);
            }
        }
        Ok(())
    }

    fn observe(&mut self, tick: u64, observation: &LifecycleObservation) {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("World lifecycle sequence exhausted");

        for state in self.sessions.values_mut() {
            for (&subscription, filter) in &state.subscriptions {
                if filter.matches(observation) {
                    state.observations.push_back(LifecyclePublication {
                        subscription,
                        sequence: self.sequence,
                        tick,
                        observation: observation.clone(),
                    });
                }
            }
        }

        for member in self.targets.matching(observation) {
            if let Some(output) = member.0.output.upgrade() {
                output.observe(member.id(), self.sequence, tick, observation);
            }
        }
    }

    fn membership(
        &mut self,
        world: super::super::SystemWorldView<'_>,
        session: u64,
        command: &LifecycleMembershipCommand,
    ) {
        let Some(mut pending) = command.take() else {
            return;
        };

        let rejection = if command.world() != world.reference() {
            Some(LifecycleMembershipRejection::StaleWorld)
        } else if command.session() != session {
            Some(LifecycleMembershipRejection::StaleSession)
        } else if command.action == LifecycleMembershipAction::Add && !command.output.0.tracking() {
            Some(LifecycleMembershipRejection::TrackingEnded)
        } else if command.action == LifecycleMembershipAction::Add
            && !self.sessions.contains_key(&session)
            && self.sessions.len() == MAX_SESSIONS
        {
            Some(LifecycleMembershipRejection::Capacity)
        } else if command.action == LifecycleMembershipAction::Add {
            pending
                .members
                .iter()
                .find_map(|member| match member.0.status.get() {
                    LifecycleMemberStatus::Pending => None,
                    LifecycleMemberStatus::Active => {
                        Some(LifecycleMembershipRejection::AlreadyActive)
                    }
                    LifecycleMemberStatus::Removed => {
                        Some(LifecycleMembershipRejection::StaleMember)
                    }
                })
        } else {
            None
        };

        let result = if let Some(rejection) = rejection {
            LifecycleMembershipResult::Rejected(rejection)
        } else {
            for member in &pending.members {
                let lifetime = match command.action {
                    LifecycleMembershipAction::Add => {
                        self.sessions.entry(session).or_default();
                        self.targets.add(session, member.clone());
                        match member.target() {
                            LifecycleWatchTarget::Entity(entity) => {
                                LifecycleTargetLifetime::Entity {
                                    live: world.entity_is_live(entity),
                                }
                            }
                            LifecycleWatchTarget::Component(entity, component)
                            | LifecycleWatchTarget::Value(entity, component, _) => {
                                LifecycleTargetLifetime::Component {
                                    entity_live: world.entity_is_live(entity),
                                    incarnation: world.component_incarnation(entity, component),
                                }
                            }
                        }
                    }
                    LifecycleMembershipAction::Remove => {
                        self.targets.remove(session, member);
                        LifecycleTargetLifetime::Removed
                    }
                };
                pending.baselines.push(LifecycleMembershipBaseline {
                    member: member.id(),
                    target: member.target(),
                    lifetime,
                });
            }

            LifecycleMembershipResult::Applied(pending.baselines)
        };

        command.finish(
            Some((self.sequence, world.next_tick())),
            result,
            pending.lease,
        );
    }
}

/// Stateless composition factory.
#[derive(Default)]
pub struct LifecyclePublisherSystemFactory;

impl SystemFactory for LifecyclePublisherSystemFactory {
    fn id(&self) -> SystemId {
        LifecyclePublisherSystem::ID
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(LifecyclePublisherSystem::default()))
    }
}

impl System for LifecyclePublisherSystem {
    fn command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        session: u64,
        command: &dyn Any,
    ) -> Result<(), ErrorReason> {
        if let Some(command) = command.downcast_ref::<LifecycleMembershipCommand>() {
            self.membership(context.world.view(), session, command);
            return Ok(());
        }

        self.apply(
            session,
            command
                .downcast_ref::<LifecyclePublisherCommand>()
                .ok_or(ErrorReason::InvalidValue)?,
        )
    }

    fn lifecycle(
        &mut self,
        context: &SystemLifecycleContext<'_>,
        observation: &LifecycleObservation,
    ) {
        self.observe(context.world.next_tick(), observation);
    }

    fn asset_lifecycle(
        &mut self,
        context: &mut crate::systems::SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        if let Some(resource) = context.asset_snapshot(event) {
            self.observe(
                context.tick(),
                &LifecycleObservation::Asset {
                    resource,
                    kind: event.kind,
                },
            );
        }
    }

    fn drain_events(&mut self, session: u64) -> Vec<Box<dyn Any>> {
        let Some(state) = self.sessions.get_mut(&session) else {
            return Vec::new();
        };
        let mut output: Vec<Box<dyn Any>> = Vec::new();
        if !state.observations.is_empty() {
            output.push(Box::new(LifecyclePublisherOutput(
                state.observations.drain(..).collect(),
            )));
        }
        if state.subscriptions.is_empty() && !self.targets.has_session(session) {
            self.sessions.remove(&session);
        }
        output
    }

    fn release_session(&mut self, session: u64) {
        for output in self.targets.release_session(session) {
            output.retire();
        }
        self.sessions.remove(&session);
    }

    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn observe_frame(&mut self, world: super::super::SystemWorldView<'_>, tick: u64) {
        self.targets.observe_values(world, tick);
    }
}

#[cfg(test)]
#[path = "lifecycle_publisher_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "target_tests.rs"]
mod target_tests;
