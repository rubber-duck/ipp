use std::{cell::RefCell, collections::LinkedList, rc::Rc};

use crate::QueuedResponse;

#[derive(Clone)]
pub(crate) struct SessionOutbox(Rc<RefCell<State>>);

struct State {
    live: bool,
    reserved: usize,
    responses: LinkedList<QueuedResponse>,
    latest_tick: u64,
}

impl Default for SessionOutbox {
    fn default() -> Self {
        Self(Rc::new(RefCell::new(State {
            live: true,
            reserved: 0,
            responses: LinkedList::new(),
            latest_tick: 0,
        })))
    }
}

impl SessionOutbox {
    pub(crate) fn len(&self) -> usize {
        let state = self.0.borrow();
        state.responses.len() + state.reserved
    }

    pub(crate) fn is_live(&self) -> bool {
        self.0.borrow().live
    }

    pub(crate) fn has_responses(&self) -> bool {
        !self.0.borrow().responses.is_empty()
    }

    pub(crate) fn progress_len(&self) -> usize {
        self.0
            .borrow()
            .responses
            .iter()
            .filter(|response| response.reservation.borrow().is_progress())
            .count()
    }

    pub(crate) fn observe_tick(&self, tick: u64) {
        let mut state = self.0.borrow_mut();
        state.latest_tick = state.latest_tick.max(tick);
    }

    pub(crate) fn latest_tick(&self) -> u64 {
        self.0.borrow().latest_tick
    }

    pub(crate) fn push_back(&self, response: QueuedResponse) {
        self.0.borrow_mut().responses.push_back(response);
    }

    pub(crate) fn pop_front(&self) -> Option<QueuedResponse> {
        self.0.borrow_mut().responses.pop_front()
    }

    pub(crate) fn close(&self) {
        let responses = {
            let mut state = self.0.borrow_mut();
            state.live = false;
            std::mem::take(&mut state.responses)
        };
        drop(responses);
    }

    #[cfg(feature = "gui")]
    pub(crate) fn reserve(&self) -> Result<ReplySlot, ipp_core::ErrorReason> {
        let mut state = self.0.borrow_mut();
        if !state.live {
            return Err(ipp_core::ErrorReason::Capacity);
        }
        state.reserved += 1;
        Ok(ReplySlot(self.clone()))
    }
}

#[cfg(feature = "gui")]
pub(crate) struct ReplySlot(SessionOutbox);

#[cfg(feature = "gui")]
impl ReplySlot {
    pub(crate) fn is_live(&self) -> bool {
        self.0.is_live()
    }

    pub(crate) fn settle(self, response: QueuedResponse) {
        if self.is_live() {
            self.0.push_back(response);
        }
    }
}

#[cfg(feature = "gui")]
impl Drop for ReplySlot {
    fn drop(&mut self) {
        self.0.0.borrow_mut().reserved -= 1;
    }
}
