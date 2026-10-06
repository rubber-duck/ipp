//! Latest completed progress waits behind reliable responses until transport drain.
//! One physical-connection marker remains outstanding through final lease release;
//! independent round-robin cursors alternate deferred progress and reliable replies.

use super::ReplyReservation;
use crate::*;
use ipp_core::services::reliable_output::OutputClass;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy)]
pub(crate) struct WorldProgress {
    pub tick: u64,
    pub time: f64,
}

impl WorldSession {
    pub(crate) fn take_response(&mut self) -> Option<ReliableResponse> {
        if let Some(response) = self.outbox.pop_front() {
            return Some(response);
        }
        self.take_progress_response()
    }

    pub(crate) fn take_progress_response(&mut self) -> Option<ReliableResponse> {
        if !self.outbox.is_live()
            || self.outbox.has_responses()
            || self.connection_progress_leases.get() != 0
        {
            return None;
        }
        let progress = self.progress?;
        if progress.tick < self.outbox.latest_tick() {
            self.progress = None;
            return None;
        }
        let response = Response {
            session: self.id,
            request_id: 0,
            tick: progress.tick,
            body: ResponseBody::Frame {
                time: progress.time,
            },
        };
        let size = ipp_protocol::world::encoded_response_size(&response)
            .expect("fixed-size World progress encoding");
        // Progress is supersedable: without ordinary capacity it waits for a later drain.
        let mut reservation =
            ReplyReservation::new(self.reply_budget.clone(), OutputClass::Ordinary, size).ok()?;
        let bytes = ipp_protocol::world::encode_response(&response)
            .expect("fixed-size World progress encoding");
        reservation.encoded(bytes.capacity());
        reservation.mark_progress(&self.progress_leases, &self.connection_progress_leases);
        self.progress = None;
        self.outbox.observe_tick(progress.tick);
        Some(ReliableResponse {
            bytes,
            reservation: Rc::new(RefCell::new(reservation)),
        })
    }
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
