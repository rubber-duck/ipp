//! Bounded command streaming and the per-World evaluation gate.

use std::time::Duration;

use super::*;

const BATCH_DEADLINE: Duration = Duration::from_secs(2);

pub(crate) struct HostCommandBatch {
    pub(crate) id: u64,
    pub(crate) started: Option<Duration>,
    pub(crate) components: std::collections::BTreeMap<u32, u16>,
}

impl<P: HostServices> Host<P> {
    pub(crate) fn command_batch_owner(&self, world: WorldId) -> Option<u64> {
        self.sessions.values().find_map(|session| {
            (session.world == world
                && session
                    .command_batch
                    .as_ref()
                    .is_some_and(|batch| batch.started.is_some()))
            .then_some(session.id)
        })
    }

    pub(crate) fn expire_command_batches(&mut self) {
        let now = self.connections.now;
        let expired: Vec<_> = self
            .sessions
            .values()
            .filter_map(|session| {
                let batch = session.command_batch.as_ref()?;
                (now.saturating_sub(batch.started?) >= BATCH_DEADLINE).then_some(session.id)
            })
            .collect();
        for id in expired {
            let mut context = self.session_mut(id).expect("live batch session");
            let batch = context.session.command_batch.take().expect("active batch");
            context.world.finish_command_stream();
            // Admission reserves unsolicited-event capacity independently of replies.
            if let Err(error) = context.queue_response(
                0,
                ResponseBody::BatchAborted {
                    batch_id: batch.id,
                    message: "Command batch exceeded its two-second deadline".into(),
                },
            ) {
                context.session.pending_errors.insert(0, error);
            }
        }
    }

    pub(crate) fn process_command_batches(&mut self) -> Vec<(u64, String)> {
        if self
            .sessions
            .values()
            .all(|session| session.pending.is_empty())
        {
            return Vec::new();
        }
        let mut failures = Vec::new();
        let sessions: Vec<_> = self.sessions.keys().copied().collect();
        for id in sessions {
            let world = self.sessions[&id].world;
            if self
                .command_batch_owner(world)
                .is_some_and(|owner| owner != id)
            {
                continue;
            }
            loop {
                let session = &self.sessions[&id];
                let active = session
                    .command_batch
                    .as_ref()
                    .is_some_and(|batch| batch.started.is_some());
                // Continuations bypass queued unrelated work, which stays ordered
                // behind the open batch instead of preventing it from completing.
                let position = if active {
                    session.pending.iter().position(|request| {
                        matches!(request.body, RequestBody::EndBatch(_))
                            || is_command_batch_continuation(&request.body)
                    })
                } else {
                    session.pending.front().and_then(|request| {
                        (matches!(
                            request.body,
                            RequestBody::BeginBatch | RequestBody::EndBatch(_)
                        ) || is_command_batch_continuation(&request.body))
                        .then_some(0)
                    })
                };
                let Some(position) = position else {
                    break;
                };
                let now = self.connections.now;
                let result = self
                    .session_mut(id)
                    .expect("live session")
                    .apply_stream_request(position, now);
                match result {
                    Err(error) => {
                        failures.push((self.connection_for_session(id), error));
                        break;
                    }
                    // A deferred chunk waits unscathed for earlier input to
                    // drain; retry it next frame instead of spinning here.
                    Ok(false) => break,
                    Ok(true) => {}
                }
            }
        }
        failures
    }
}

impl<P: HostServices> WorldSessionContext<'_, P> {
    /// Apply one queued stream request. Returns false when a chunk was
    /// deferred for earlier input to drain; the caller must stop for this
    /// frame so evaluation can make progress before the retry.
    fn apply_stream_request(&mut self, position: usize, now: Duration) -> Result<bool, String> {
        let request = self
            .session
            .pending
            .remove(position)
            .expect("queued request");
        let body = match request.body {
            RequestBody::BeginBatch => {
                if self.session.command_batch.is_some() {
                    ResponseBody::Error {
                        code: 1,
                        message: "A command batch is already allocated for this session".into(),
                    }
                } else {
                    let id = next_ingress_id()?;
                    self.session.command_batch = Some(HostCommandBatch {
                        id,
                        started: None,
                        components: Default::default(),
                    });
                    ResponseBody::BatchStarted(id)
                }
            }
            RequestBody::EndBatch(id) => {
                if self
                    .session
                    .command_batch
                    .as_ref()
                    .is_some_and(|batch| batch.id == id)
                {
                    self.session.command_batch = None;
                    self.world.finish_command_stream();
                    ResponseBody::BatchFinished(id)
                } else {
                    ResponseBody::Error {
                        code: 1,
                        message: "Unknown, completed, expired or foreign command batch".into(),
                    }
                }
            }
            body if is_command_batch_continuation(&body) => {
                return self.apply_stream_chunk(request.request_id, position, now, body);
            }
            _ => unreachable!("command streaming boundary"),
        };
        self.queue_response(request.request_id, body).map(|()| true)
    }

    fn apply_stream_chunk(
        &mut self,
        request_id: u64,
        position: usize,
        now: Duration,
        mut body: RequestBody,
    ) -> Result<bool, String> {
        let batch_id = self.session.request_origins[&request_id]
            .1
            .expect("batch correlation");
        if self
            .session
            .command_batch
            .as_ref()
            .is_none_or(|active| active.id != batch_id)
        {
            self.recycle_stream_body(body);
            return self
                .queue_response(
                    request_id,
                    ResponseBody::Error {
                        code: 1,
                        message: "Unknown, completed, expired or foreign command batch".into(),
                    },
                )
                .map(|()| true);
        }
        if let Some(error) = self.session.pending_errors.remove(&request_id) {
            self.abort_command_batch();
            self.recycle_stream_body(body);
            return self
                .queue_response(
                    request_id,
                    ResponseBody::Error {
                        code: 1,
                        message: error,
                    },
                )
                .map(|()| true);
        }
        if self.world.has_deferred_world_input() {
            // Earlier input (queued ingress or routed envelopes awaiting their
            // boundary) drains first. Scoping is not idempotent, so the owned
            // continuation waits untouched and the stream stays unstarted.
            let deferred = Request {
                session: self.session.id,
                request_id,
                body,
            };
            let at = position.min(self.session.pending.len());
            self.session.pending.insert(at, deferred);
            return Ok(false);
        }
        if let Err(error) =
            services::connection::scope::scope_request(self.session, &self.world, &mut body)
        {
            self.abort_command_batch();
            self.recycle_stream_body(body);
            return self
                .queue_response(
                    request_id,
                    ResponseBody::Error {
                        code: 1,
                        message: error,
                    },
                )
                .map(|()| true);
        }
        self.session
            .command_batch
            .as_mut()
            .expect("allocated batch")
            .started
            .get_or_insert(now);

        // The deferral gate already waited out transient backpressure, so a
        // late error ends the stream instead of retrying a scoped buffer.
        let response = match body {
            RequestBody::BatchChunk(batch) => match self.world.apply_command_chunk(batch) {
                Ok(outcome) => {
                    if outcome.result.is_err() {
                        self.abort_command_batch();
                    }
                    ResponseBody::Batch(outcome)
                }
                Err(error) => {
                    self.abort_command_batch();
                    ResponseBody::Error {
                        code: 1,
                        message: error.to_string(),
                    }
                }
            },
            #[cfg(feature = "gui")]
            RequestBody::GuiCommands {
                batch_id: Some(_),
                commands,
            } => match self
                .world
                .apply_gui_command_chunk(self.session.id, request_id, commands)
            {
                Ok(outcome) => {
                    if outcome.result.is_err() {
                        self.abort_command_batch();
                    }
                    ResponseBody::GuiCommands {
                        applied: u32::try_from(outcome.applied)
                            .map_err(|_| "GUI applied prefix exceeds u32")?,
                        error: outcome.result.err(),
                    }
                }
                Err(error) => {
                    self.abort_command_batch();
                    ResponseBody::Error {
                        code: 1,
                        message: error.to_string(),
                    }
                }
            },
            _ => unreachable!("command stream continuation"),
        };
        self.queue_response(request_id, response).map(|()| true)
    }

    fn abort_command_batch(&mut self) {
        self.session.command_batch = None;
        self.world.finish_command_stream();
    }

    fn recycle_stream_body(&mut self, body: RequestBody) {
        if let RequestBody::BatchChunk(batch) = body {
            self.world.recycle_command_buffer(batch.operations);
        }
    }
}
