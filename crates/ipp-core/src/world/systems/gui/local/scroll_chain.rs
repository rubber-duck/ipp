use crate::services::gui_input::{GuiInputError, GuiRequestKey};
use std::cell::Cell;
use std::rc::Rc;

struct State {
    remaining: Cell<[f32; 2]>,
    failed: Cell<bool>,
    pending: Cell<Option<GuiRequestKey>>,
    allocated: Cell<usize>,
    completed: Cell<usize>,
    sealed: Cell<bool>,
}

/// One physical delta consumed from the innermost viewport outward.
/// It retains no control value and is updated only after an admitted local commit.
#[derive(Clone)]
pub struct GuiScrollChain(Rc<State>);

impl GuiScrollChain {
    /// Start a finite logical delta; adapters read its final remainder after settlement.
    pub fn new(delta: [f32; 2]) -> Result<Self, GuiInputError> {
        if !delta.iter().all(|value| value.is_finite()) {
            return Err(GuiInputError::Local(
                super::GuiLocalActionError::InvalidValue,
            ));
        }
        Ok(Self(Rc::new(State {
            remaining: Cell::new(delta),
            failed: Cell::new(false),
            pending: Cell::new(None),
            allocated: Cell::new(0),
            completed: Cell::new(0),
            sealed: Cell::new(false),
        })))
    }

    /// Remaining movement; a cancelled predecessor never authorizes scene fallback.
    pub fn remaining(&self) -> Result<[f32; 2], GuiInputError> {
        if self.0.failed.get() {
            Err(GuiInputError::Cancelled)
        } else {
            Ok(self.0.remaining.get())
        }
    }

    /// Release the prepared chain only after every ticket and FIFO position exists.
    pub fn seal(&self) {
        self.0.sealed.set(true);
    }

    pub(super) fn register(&self) -> Result<usize, GuiInputError> {
        if self.0.sealed.get() || self.0.failed.get() {
            return Err(GuiInputError::Cancelled);
        }
        let ordinal = self.0.allocated.get();
        self.0
            .allocated
            .set(ordinal.checked_add(1).ok_or(GuiInputError::Capacity)?);
        Ok(ordinal)
    }

    pub(super) fn ready(&self, ordinal: usize) -> bool {
        self.0.failed.get() || (self.0.sealed.get() && self.0.completed.get() == ordinal)
    }

    pub(super) fn enter(
        &self,
        ordinal: usize,
        key: GuiRequestKey,
    ) -> Result<[f32; 2], GuiInputError> {
        if !self.ready(ordinal) {
            return Err(GuiInputError::Cancelled);
        }
        let delta = self.remaining()?;
        if self.0.pending.replace(Some(key)).is_some() {
            self.0.failed.set(true);
            return Err(GuiInputError::Cancelled);
        }
        Ok(delta)
    }

    pub(super) fn commit(&self, consumed: [f32; 2]) {
        let remaining = self.0.remaining.get();
        self.0
            .remaining
            .set(std::array::from_fn(|axis| remaining[axis] - consumed[axis]));
        self.0.pending.set(None);
        self.0.completed.set(self.0.completed.get() + 1);
    }

    pub(super) fn abandon(&self) {
        self.0.failed.set(true);
        self.0.pending.set(None);
    }
}
