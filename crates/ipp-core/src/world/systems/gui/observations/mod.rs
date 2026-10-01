//! Ordered World-local applied effects and independently lived observer outputs.

mod command;
mod output;
mod publisher;

pub use command::{
    GuiObservationCommand, GuiObservationPrepareError, GuiObservationPrepareFailure,
};
pub use output::{GuiObservationDelivery, GuiObservationOutput, GuiObservationSubscription};
pub(in crate::world::systems::gui) use publisher::GuiEffectPublisher;

use super::local::GuiLocalEffect;
use crate::WorldRef;
use std::sync::Arc;

/// Monotonic applied-record identity within an exact World lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiEffectId {
    /// Exact World lifetime; restoration never shares this identity.
    pub world: WorldRef,
    /// Nonzero ordinal of an actual applied record, not a tick.
    pub ordinal: u64,
}

/// Application effects and explicit local feedback are distinct subscriptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiObservationClasses {
    /// Press and submission effects; values reach clients through field observation.
    Application,
    /// Changed logical focus and explicitly commanded pointer feedback.
    Feedback,
    /// Both classes, in their single World commit order.
    All,
}

/// Opaque output lifetime and subscription generation, unrelated to input sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiObservationSubscriptionId {
    /// Exact output lifetime, never a caller's reusable connection number.
    pub output: u64,
    /// Generation minted by that output; no retired-name map is retained.
    pub generation: u64,
}

/// Adapter-supplied upper bounds for the peak encoded allocation, not a wire policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiObservationEncoding {
    /// Maximum encoded allocation capacity for one control marker.
    pub control_bytes: usize,
    /// Fixed encoded allocation capacity for one effect record.
    pub effect_bytes: usize,
    /// Additional encoded capacity per pinned ancestry entry.
    pub ancestry_entry_bytes: usize,
    /// Additional encoded capacity per UTF-8 value byte.
    pub text_byte_bytes: usize,
}

/// Ordered receiver rejection, distinct from connection output failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiObservationRejection {
    /// The queued command reached a different World lifetime.
    StaleWorld,
    /// A previously closed generation cannot become active again.
    StaleSubscription,
    /// This exact generation is already active.
    AlreadySubscribed,
}

/// The sole correlated result for an ordered subscription command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiObservationControlResult {
    /// Retained before subscription activation.
    Subscribed,
    /// Retained after removal, behind already-retained effects.
    Unsubscribed,
    /// The prepared command was discarded before settlement.
    Cancelled,
    /// No requested registration change was made.
    Rejected(GuiObservationRejection),
}

/// One FIFO record; control results do not claim a World evaluation tick.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiObservationRecord {
    /// Sole correlated subscription result; carries no fabricated effect tick.
    Control {
        /// Exact requested World.
        world: WorldRef,
        /// Independent output/subscription lifetime.
        subscription: GuiObservationSubscriptionId,
        /// Adapter's control-request correlation.
        request: u64,
        /// Applied cut, cancellation, or rejection.
        result: GuiObservationControlResult,
    },
    /// Immutable applied effect selected at its World mutation boundary.
    Effect {
        /// The subscription active at that boundary.
        subscription: GuiObservationSubscriptionId,
        /// Actual identity, tick, pinned ancestry, source and momentary payload.
        effect: Arc<GuiLocalEffect>,
    },
}
