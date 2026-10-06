//! Host-owned GUI delivery and composed-input authority, independent of transports.
//! Reservations do not enqueue commands or execute GUI actions.

mod delivery;
mod pointer;
mod proof;
pub mod query;
pub mod router;
mod service;
mod ticket;

pub use delivery::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputError,
    GuiInputReservationError, GuiRequestKey,
};
pub use pointer::{GuiPointerLease, GuiPointerLeaseId};
pub use proof::GuiRoutedInputProof;
pub use service::{
    GuiContextBinding, GuiInputContext, GuiInputLimits, GuiInputService, GuiInputSession,
    GuiInputSessionId, GuiNativeCancellation,
};
pub use ticket::{GuiInputCommand, GuiPreparedEffect};

/// Pointers one input context tracks at once, for routed presses and hovers and for native
/// text captures alike.
///
/// A pointer beyond the cap is refused with `GuiInputError::Capacity` and the pointers already
/// tracked continue. Thirty-two covers every finger of several hands plus mice and pens, and
/// keeps a context's cancellation, which lists them all, one small fixed-size response. The
/// wire contract bounds that cancellation list by this constant.
pub const GUI_INPUT_MAX_POINTERS: usize = 32;

#[cfg(test)]
mod routing_test_support;

#[cfg(test)]
mod service_tests;

#[cfg(test)]
mod test_support;
