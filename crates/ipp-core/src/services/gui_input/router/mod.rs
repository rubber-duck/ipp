//! Ordinary composed physical input. The owning Host supplies the selected view,
//! delivery permits and event order; this module neither draws nor advances time.

mod focus;
mod group;
mod keyboard;
pub(crate) mod keyboard_panels;
mod overlay;
mod routing;

use super::{GuiDeliveryPermit, GuiInputError};
use crate::systems::gui::local::GuiEntityTarget;

pub use routing::{GuiInputRouter, GuiRoutingContext};

/// Bounded platform cancellation; never a committed control effect or frame.
#[derive(Default)]
pub struct GuiRoutingCancellation {
    pub(super) pointers: [u64; super::GUI_INPUT_MAX_POINTERS],
    pub(super) count: usize,
    /// Native focus must end.
    pub focus: bool,
}

impl GuiRoutingCancellation {
    /// Exact cancelled pointer identities in deterministic order.
    pub fn pointers(&self) -> &[u64] {
        &self.pointers[..self.count]
    }

    /// Whether the platform has any ownership to release.
    pub fn is_empty(&self) -> bool {
        self.count == 0 && !self.focus
    }
}

/// Platform input coordinates are normalized to the selected physical viewport.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum GuiPhysicalInput {
    /// Native edits retain the exact observed focus/value fence across asynchronous OS work.
    Text {
        /// Original native buffer fence.
        fence: crate::systems::gui::local::GuiTextFence,
        /// Ordered single-line editing operation.
        edit: crate::systems::gui::local::GuiTextEdit,
    },
    PointerDown {
        pointer: u64,
        point: [f32; 2],
        button: GuiPhysicalButton,
    },
    PointerMove {
        pointer: u64,
        point: [f32; 2],
    },
    PointerUp {
        pointer: u64,
        point: [f32; 2],
        button: GuiPhysicalButton,
    },
    PointerCancel {
        pointer: u64,
    },
    /// `shift` is whether Shift was held, for the control the wheel reaches.
    Wheel {
        point: [f32; 2],
        delta: [f32; 2],
        shift: bool,
    },
    /// `shift` is whether Shift was held, for the focused control's key handling.
    Key {
        key: GuiPhysicalKey,
        shift: bool,
    },
    Blur,
}

/// Primary activates controls; a secondary press on a control is a context
/// request; other presses only request scene-gesture admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GuiPhysicalButton {
    Primary,
    Secondary,
    Auxiliary,
}

/// Logical keys; platform composition is handled separately from activation.
/// `ContextMenu` is the Menu key; Shift+`F10` is the other context request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GuiPhysicalKey {
    Tab,
    BackTab,
    Enter,
    Space,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    ContextMenu,
    F10,
}

/// Routing admission, not a committed action or a completed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GuiRoutingDisposition {
    Routed {
        target: GuiEntityTarget,
    },
    Miss,
    Blocked,
    Unhandled,
}

/// Each child permit belongs to the one already-reserved physical request.
/// The adapter aggregates its terminal prefix without a second effect queue.
pub trait GuiRoutingDelivery {
    /// Reserve a child terminal before accepting its command. No World is borrowed.
    fn command(&mut self) -> Result<Box<dyn GuiDeliveryPermit>, GuiInputError>;
}
