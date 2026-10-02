//! The sole sampler of GUI skin transitions: one Host-time clock per
//! transitioning part, eased between the origin and destination GUI requested.

use crate::ComponentValue;
use crate::systems::gui::motion::{GuiMotionOwner, notify_sample};
use crate::systems::{SystemCommitContext, SystemRuntimeAccess};
use std::collections::BTreeMap;

/// The transition a clock times and its elapsed Host seconds.
#[derive(Clone, Copy)]
struct GuiMotionClock {
    transition: u64,
    elapsed: f64,
}

#[derive(Default)]
pub(super) struct GuiMotionAnimations {
    /// Clocks of the transitions under way; a settled transition has none.
    clocks: BTreeMap<GuiMotionOwner, GuiMotionClock>,
    pub statistics: crate::systems::gui::motion::GuiMotionSamplingWork,
}

impl GuiMotionAnimations {
    /// Drop the clocks of parts whose control or `GuiBehavior` leaves before
    /// its storage can be reused.
    pub fn before_commit(&mut self, context: &SystemCommitContext<'_>) {
        if self.clocks.is_empty() {
            return;
        }

        for (entity, component) in context.changed_components() {
            if context.retains_component(entity, component) {
                continue;
            }

            self.clocks.retain(|owner, _| {
                owner.entity != entity
                    || (component != ComponentValue::GUI_BEHAVIOR && component != owner.control)
            });
        }
    }

    /// Restart the clock of every transition GUI started this frame, advance
    /// every clock by the Host frame and write each eased sample.
    pub fn update(
        &mut self,
        context: &mut SystemRuntimeAccess<'_>,
        dt: f64,
        started: &[GuiMotionOwner],
    ) {
        self.statistics = Default::default();
        for &owner in started {
            let Some(channels) = owner.channels(context.world) else {
                continue;
            };
            self.statistics.bindings += 1;
            self.clocks.insert(
                owner,
                GuiMotionClock {
                    transition: channels.transition,
                    elapsed: 0.0,
                },
            );
        }

        let mut settled = Vec::new();
        let mut sampled = Vec::new();
        for (&owner, clock) in &mut self.clocks {
            self.statistics.owners += 1;
            let Some(channels) = owner
                .channels_mut(context.world)
                .filter(|channels| channels.transition == clock.transition)
            else {
                settled.push(owner);
                continue;
            };

            // Durations are authored in single precision, so the transition
            // ends once the elapsed time reaches them at that precision.
            let duration = f64::from(channels.duration);
            clock.elapsed = (clock.elapsed + dt).min(duration);
            let done = clock.elapsed as f32 >= channels.duration;
            channels.values = if done {
                channels.destination
            } else {
                let progress = channels.easing.progress(clock.elapsed / duration);
                channels.origin.mix(&channels.destination, progress as f32)
            };
            channels.settled = done;
            self.statistics.samples += 1;
            sampled.push(owner.entity);
            if done {
                settled.push(owner);
            }
        }

        for owner in settled {
            self.clocks.remove(&owner);
        }

        sampled.dedup();
        for entity in sampled {
            notify_sample(context, entity);
        }
    }
}
