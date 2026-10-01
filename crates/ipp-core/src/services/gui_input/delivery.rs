use super::GuiInputSessionId;
use crate::WorldRef;
use crate::systems::gui::local::{GuiEntityTarget, GuiLocalActionError, GuiLocalEffect};

/// Exact correlation scope; the session identity is minted by the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GuiRequestKey {
    /// Target World, which need not have an authoring session.
    pub world: WorldRef,
    /// Originating session lifetime, not a wire session number.
    pub session: GuiInputSessionId,
    /// Adapter correlation within that lifetime.
    pub request_id: u64,
}

/// Adapter failure before any effect mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiDeliveryError {
    /// The adapter's originating delivery owner was revoked.
    SessionClosed,
    /// Exact effect serialization cannot fit the reserved delivery budget.
    Capacity,
}

/// Foundation admission and command-boundary failure, without a fabricated effect tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiInputError {
    /// A handle belongs to another service or a closed session.
    SessionClosed,
    /// A bounded service budget is exhausted.
    Capacity,
    /// The same correlation is already pending in this exact scope.
    DuplicateRequest,
    /// The World lifetime or selected GUI capability is unavailable.
    Unavailable,
    /// Root output, viewport, context or binding generation changed.
    StaleContext,
    /// The current attachment path or retained source is no longer eligible.
    StalePath,
    /// Cancellation won before the mutation boundary.
    Cancelled,
    /// Adapter reservation failed before mutation.
    Delivery(GuiDeliveryError),
    /// The owning GUI System rejected local identity, role, eligibility or value.
    Local(GuiLocalActionError),
}

impl std::fmt::Display for GuiInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for GuiInputError {}

/// The one terminal disposition for a reserved request.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiDeliveryTerminal {
    /// Native transient/value edit; any application effect was published before settlement.
    NativeApplied(crate::systems::gui::local::GuiNativeTextState),
    /// Exact immutable effect prepared before its local mutation.
    Applied(GuiLocalEffect),
    /// A value operation applied without a momentary effect; the written
    /// control fields carry its result.
    Written {
        /// Exact control lifetime the operation wrote.
        target: GuiEntityTarget,
        /// Reservation provenance, as an effect would carry it.
        source: crate::systems::gui::local::GuiLocalEffectSource,
        /// Whether any value field changed.
        changed: bool,
    },
    /// No effect was committed.
    Rejected(GuiInputError),
    /// The pending payload was discarded; no destruction cause is invented.
    Cancelled,
}

/// Adapter-owned existing outbox capacity, not a second core response queue.
///
/// The factory reserves a response slot and bounded rejection bytes before handing
/// this permit to the service. Dropping a permit releases any unused reservation.
/// Callbacks must not retain Host/World borrows or synchronously execute GUI actions.
pub trait GuiDeliveryPermit {
    /// Reserve the native-buffer response before changing text, selection or composition.
    /// Semantic-only adapters reject this physical-only terminal by default.
    fn prepare_native(
        &mut self,
        _: &crate::systems::gui::local::GuiNativeTextState,
    ) -> Result<(), GuiDeliveryError> {
        Err(GuiDeliveryError::Capacity)
    }

    /// None checks delivery liveness before local focus revalidation. Some reserves
    /// the exact effect/value/ancestry bytes before any mutation for that effect.
    /// Failure preserves capacity for the bounded rejection terminal.
    fn prepare(&mut self, effect: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError>;

    /// Infallibly consume the existing reservation. Called without service borrows.
    /// The adapter must also respect its own session revocation fence.
    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal);
}

/// Pre-reservation failure returns the adapter's capacity intact. No ticket was accepted.
/// The adapter settles its bounded rejection, or drops the permit if delivery was revoked.
pub struct GuiInputReservationError {
    /// Admission failure without an effect tick.
    pub reason: GuiInputError,
    pub(super) permit: Box<dyn GuiDeliveryPermit>,
}

impl GuiInputReservationError {
    /// Recover the same outbox reservation; never allocate a replacement response slot.
    pub fn into_parts(self) -> (GuiInputError, Box<dyn GuiDeliveryPermit>) {
        (self.reason, self.permit)
    }
}

impl std::fmt::Debug for GuiInputReservationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.reason.fmt(formatter)
    }
}

impl std::fmt::Display for GuiInputReservationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.reason, formatter)
    }
}

impl std::error::Error for GuiInputReservationError {}

pub(super) fn target_key(
    target: GuiEntityTarget,
    session: GuiInputSessionId,
    request_id: u64,
) -> GuiRequestKey {
    GuiRequestKey {
        world: target.world,
        session,
        request_id,
    }
}
