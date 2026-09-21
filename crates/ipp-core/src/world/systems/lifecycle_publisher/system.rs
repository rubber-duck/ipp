use super::*;

const MAX_SESSIONS: usize = 128;
const MAX_SUBSCRIPTIONS: usize = 64;
const MAX_OBSERVATIONS: usize = 128;

#[derive(Default)]
struct LifecycleSession {
    subscriptions: BTreeMap<u64, LifecycleFilter>,
    observations: VecDeque<LifecyclePublication>,
    overflow: Option<u64>,
}

/// World-owned subscription policy; no storage references or delivery callbacks escape.
#[derive(Default)]
pub struct LifecyclePublisherSystem {
    sessions: BTreeMap<u64, LifecycleSession>,
    sequence: u64,
}

impl LifecyclePublisherSystem {
    /// Stable composition and generic command/event routing identity.
    pub const ID: SystemId = SystemId("ipp.lifecycle-publisher");

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
                if state.subscriptions.len() == MAX_SUBSCRIPTIONS || state.overflow.is_some() {
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
            let matching: Vec<_> = state
                .subscriptions
                .iter()
                .filter_map(|(&id, filter)| filter.matches(observation).then_some(id))
                .collect();
            if matching.len() > MAX_OBSERVATIONS.saturating_sub(state.observations.len()) {
                state.overflow = Some((state.observations.len() + matching.len()) as u64);
                state.observations.clear();
                state.subscriptions.clear();
                continue;
            }
            for subscription in matching {
                state.observations.push_back(LifecyclePublication {
                    subscription,
                    sequence: self.sequence,
                    tick,
                    observation: observation.clone(),
                });
            }
        }
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
        _: &mut SystemCommandContext<'_>,
        session: u64,
        command: &dyn Any,
    ) -> Result<(), ErrorReason> {
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
            output.push(Box::new(LifecyclePublisherOutput::Events(
                state.observations.drain(..).collect(),
            )));
        }
        if let Some(dropped) = state.overflow.take() {
            output.push(Box::new(LifecyclePublisherOutput::Overflow {
                dropped,
            }));
        }
        if state.subscriptions.is_empty() {
            self.sessions.remove(&session);
        }
        output
    }

    fn release_session(&mut self, session: u64) {
        self.sessions.remove(&session);
    }

    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}
}

#[cfg(test)]
#[path = "lifecycle_publisher_tests.rs"]
mod tests;
