use super::output::SubscriptionStatus;
use super::*;
use crate::services::gui_input::GuiInputError;
use crate::services::reliable_output::{OutputCharge, OutputFailure, ReliableOutputLease};
use crate::systems::gui::local::GuiLocalEffectKind;

struct Registration {
    subscription: GuiObservationSubscription,
    next: Option<Box<Registration>>,
    _metadata: ReliableOutputLease,
}

/// One publisher/ordinal per selected GuiSystem, independent of input service instances.
#[derive(Default)]
pub(in crate::world::systems::gui) struct GuiEffectPublisher {
    ordinal: u64,
    registrations: Option<Box<Registration>>,
}

impl GuiEffectPublisher {
    pub fn candidate_id(
        &self,
        world: WorldRef,
        kind: &GuiLocalEffectKind,
    ) -> Result<Option<GuiEffectId>, GuiInputError> {
        if Self::class(kind).is_none() {
            return Ok(None);
        }
        Ok(Some(GuiEffectId {
            world,
            ordinal: self.ordinal.checked_add(1).ok_or(GuiInputError::Capacity)?,
        }))
    }

    fn class(kind: &GuiLocalEffectKind) -> Option<GuiObservationClasses> {
        match kind {
            GuiLocalEffectKind::Pressed | GuiLocalEffectKind::Submitted(_) => {
                Some(GuiObservationClasses::Application)
            }
            GuiLocalEffectKind::FocusChanged {
                changed: true,
                ..
            } => Some(GuiObservationClasses::Feedback),
            GuiLocalEffectKind::InteractionChanged(effect) if effect.changed => {
                Some(GuiObservationClasses::Feedback)
            }
            _ => None,
        }
    }

    pub fn publish(&mut self, effect: &GuiLocalEffect) {
        let Some(id) = effect.id else {
            return;
        };
        assert_eq!(Some(id.ordinal), self.ordinal.checked_add(1));
        assert_eq!(id.world, effect.target.world);
        self.ordinal = id.ordinal;
        self.prune();
        let class = Self::class(&effect.kind).expect("only actual records have ordinals");
        let mut current = self.registrations.as_deref();
        while let Some(registration) = current {
            let subscription = &registration.subscription.0;
            if subscription.world == id.world
                && (subscription.classes == GuiObservationClasses::All
                    || subscription.classes == class)
                && let Some(output) = subscription.output.upgrade()
            {
                output.observe(subscription.id, effect);
            }
            current = registration.next.as_deref();
        }
    }

    pub fn command(&mut self, world: WorldRef, command: &GuiObservationCommand) {
        if !command.pending() {
            return;
        }
        if !command.output.is_live() {
            command.finish(GuiObservationControlResult::Cancelled);
            return;
        }
        if world != command.world {
            command.finish(GuiObservationControlResult::Rejected(
                GuiObservationRejection::StaleWorld,
            ));
            return;
        }
        self.prune();
        let subscription = &command.subscription.0;
        if command.subscribe {
            let rejection = match subscription.status.get() {
                SubscriptionStatus::Active => Some(GuiObservationRejection::AlreadySubscribed),
                SubscriptionStatus::Closed => Some(GuiObservationRejection::StaleSubscription),
                SubscriptionStatus::Pending => None,
            };
            if let Some(reason) = rejection {
                command.finish(GuiObservationControlResult::Rejected(reason));
                return;
            }
            let metadata = match command.output.account.reserve(OutputCharge {
                entries: 0,
                bytes: std::mem::size_of::<Registration>(),
            }) {
                Ok(metadata) => metadata,
                Err(_) => {
                    command.output.account.fail(OutputFailure::Capacity);
                    command.finish(GuiObservationControlResult::Cancelled);
                    return;
                }
            };
            let mut registration = Box::new(Registration {
                subscription: command.subscription.clone(),
                next: None,
                _metadata: metadata,
            });
            if command.finish(GuiObservationControlResult::Subscribed) {
                subscription.status.set(SubscriptionStatus::Active);
                registration.next = self.registrations.take();
                self.registrations = Some(registration);
            }
        } else {
            subscription.status.set(SubscriptionStatus::Closed);
            self.prune();
            command.finish(GuiObservationControlResult::Unsubscribed);
        }
    }

    pub fn prune(&mut self) {
        let mut cursor = &mut self.registrations;
        while let Some(mut registration) = cursor.take() {
            if registration.subscription.is_active() {
                *cursor = Some(registration);
                cursor = &mut cursor.as_mut().expect("retained registration").next;
            } else {
                *cursor = registration.next.take();
            }
        }
    }
}

impl Drop for GuiEffectPublisher {
    fn drop(&mut self) {
        while let Some(mut registration) = self.registrations.take() {
            registration
                .subscription
                .0
                .status
                .set(SubscriptionStatus::Closed);
            self.registrations = registration.next.take();
        }
    }
}

#[cfg(test)]
#[path = "publisher_tests.rs"]
mod tests;
