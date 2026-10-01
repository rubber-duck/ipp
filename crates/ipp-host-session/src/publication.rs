use super::*;
use ipp_core::services::reliable_output::OutputClass;

/// Payload-free name for capacity failure reasons.
fn response_kind(body: &ResponseBody) -> &'static str {
    match body {
        ResponseBody::Frame {
            ..
        } => "frame",
        ResponseBody::Resources {
            ..
        } => "resources",
        ResponseBody::LifecycleEvents(_) => "lifecycle",
        ResponseBody::RuntimeFailure {
            ..
        } => "runtime-failure",
        ResponseBody::Batch(_) => "batch",
        ResponseBody::Error {
            ..
        } => "error",
        _ => "other",
    }
}

impl<P: HostServices> WorldSessionContext<'_, P> {
    pub(super) fn record_runtime_failure(
        &mut self,
        scope: ipp_protocol::RuntimeFailureScope,
        faulted: bool,
        mut message: String,
    ) -> Result<(), String> {
        if message.len() > ipp_protocol::MAX_FAILURE_MESSAGE_BYTES {
            let mut end = ipp_protocol::MAX_FAILURE_MESSAGE_BYTES;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        let failure = (scope, faulted, message.clone());
        if self.session.last_failure.as_ref() != Some(&failure) {
            self.queue_response(
                0,
                ResponseBody::RuntimeFailure {
                    scope,
                    faulted,
                    message,
                },
            )?;
            self.session.last_failure = Some(failure);
        }
        Ok(())
    }

    pub(super) fn publish_report(
        &mut self,
        report: &mut ipp_core::WorldUpdateReport,
    ) -> Result<(), String> {
        self.session.prepared = false;
        let mut replies = std::mem::take(&mut self.session.replies);
        let origins: std::collections::BTreeSet<_> =
            self.session.request_origins.keys().copied().collect();
        for change in report.render_state_changes.iter().cloned() {
            self.queue_response(0, ResponseBody::RenderStateUpdatedEvent(change))?;
        }
        if !report.playback_events.is_empty() {
            self.queue_response(
                0,
                ResponseBody::PlaybackEvents(report.playback_events.clone()),
            )?;
        }
        let mut controller_outcomes = report
            .animation_controller_outcomes
            .iter()
            .filter(|outcome| origins.contains(&outcome.request_id))
            .cloned()
            .collect::<VecDeque<_>>();
        let mut system_outcomes = report
            .system_command_outcomes
            .iter()
            .filter(|outcome| outcome.session == self.session.id)
            .cloned()
            .collect::<VecDeque<_>>();
        let mut outcomes = report
            .outcomes
            .extract_if(.., |outcome| origins.contains(&outcome.batch_id));
        let result = (|| {
            for (request_id, reply) in replies.drain(..) {
                let body = match reply {
                    WorldSessionReply::CameraNavigate => {
                        let outcome = system_outcomes
                            .pop_front()
                            .ok_or("core did not publish navigation outcome")?;
                        if outcome.request_id != request_id {
                            return Err("navigation outcome correlation".into());
                        }
                        match outcome.result {
                            Ok(()) => ResponseBody::CameraNavigated,
                            Err(reason) => ResponseBody::Error {
                                code: 1,
                                message: reason.to_string(),
                            },
                        }
                    }
                    WorldSessionReply::LifecycleSubscription => {
                        let outcome = system_outcomes
                            .pop_front()
                            .ok_or("core did not publish subscription outcome")?;
                        if outcome.request_id != request_id {
                            return Err("subscription outcome correlation".into());
                        }
                        match outcome.result {
                            Ok(()) => ResponseBody::LifecycleSubscription,
                            Err(reason) => ResponseBody::Error {
                                code: 1,
                                message: reason.to_string(),
                            },
                        }
                    }
                    WorldSessionReply::AnimationController => {
                        let outcome = controller_outcomes
                            .pop_front()
                            .ok_or_else(|| "core did not publish controller outcome".to_owned())?;
                        if outcome.request_id != request_id {
                            return Err("controller outcome correlation".to_owned());
                        }
                        match outcome.result {
                            Ok(id) => ResponseBody::AnimationController(id),
                            Err(reason) => ResponseBody::Error {
                                code: 1,
                                message: reason.to_string(),
                            },
                        }
                    }
                    WorldSessionReply::Batch {
                        #[cfg(feature = "diagnostics")]
                        operations,
                    } => {
                        let outcome = outcomes.next().ok_or_else(|| {
                            "core did not publish the submitted batch outcome".to_owned()
                        })?;
                        #[cfg(feature = "diagnostics")]
                        if operations != 0 {
                            if let Err(error) = &outcome.result {
                                ipp_core::diagnostic!(
                                    Warn,
                                    "[IPP {}] buffer.reject session={} request={} batch={} operations={} operation={:?} reason={}",
                                    P::NAME,
                                    self.session.id,
                                    request_id,
                                    outcome.batch_id,
                                    operations,
                                    error.operation,
                                    error.reason
                                );
                            } else {
                                ipp_core::diagnostic!(
                                    Debug,
                                    "[IPP {}] buffer.complete session={} request={} batch={} operations={}",
                                    P::NAME,
                                    self.session.id,
                                    request_id,
                                    outcome.batch_id,
                                    operations
                                );
                            }
                        }
                        ResponseBody::Batch(self.session.receipts.borrow().outcome(outcome)?)
                    }
                    WorldSessionReply::Inspect(query) => {
                        self.inspection_page(query, request_id, report.tick, report.time)
                    }
                    #[cfg(feature = "diagnostics")]
                    WorldSessionReply::LifecycleDiagnostics(query) => {
                        self.lifecycle_diagnostics(query)
                    }
                    WorldSessionReply::CompletedView(body) => body,
                    WorldSessionReply::GeometryPick(_) | WorldSessionReply::CameraProject(_) => {
                        return Err("Host did not resolve the completed-view query".into());
                    }
                    WorldSessionReply::Rejected(message) => ResponseBody::Error {
                        code: 1,
                        message,
                    },
                };
                self.queue_response(request_id, body)?;
            }
            Ok::<_, String>(())
        })();
        self.session.replies = replies;
        result?;
        if !system_outcomes.is_empty() {
            return Err("core published an uncorrelated system command outcome".into());
        }
        if outcomes.next().is_some() {
            return Err("core published an uncorrelated batch outcome".to_owned());
        }
        let mut resources = Vec::new();
        let mut resource_bytes = 29;
        for resource in report.resource_changes.iter().cloned() {
            // Include the largest status encoding so long references still fit
            // the transport frame. These chunks do not limit total asset demand.
            let size = 40
                + resource.source.len()
                + match &resource.status {
                    ipp_core::AssetResourceStatus::Failed(error) => error.len(),
                    _ => 0,
                };
            if !resources.is_empty()
                && (resources.len() == 128
                    || resource_bytes + size > ipp_protocol::MAX_MESSAGE_BYTES)
            {
                self.queue_response(
                    0,
                    ResponseBody::Resources {
                        resources,
                    },
                )?;
                resources = Vec::new();
                resource_bytes = 29;
            }
            resources.push(resource);
            resource_bytes += size;
        }
        if !resources.is_empty() {
            self.queue_response(
                0,
                ResponseBody::Resources {
                    resources,
                },
            )?;
        }
        // Every drained observation is charged to the connection's reliable output account;
        // exhaustion fails this connection rather than dropping observations.
        for LifecyclePublisherOutput(events) in
            self.world.drain_system_events::<LifecyclePublisherOutput>(
                LifecyclePublisherSystem::ID,
                self.session.id,
            )
        {
            let mut events = events.into_iter().peekable();
            while events.peek().is_some() {
                let page = events
                    .by_ref()
                    .take(ipp_protocol::MAX_LIFECYCLE_PUBLICATIONS)
                    .collect();
                self.queue_response(
                    0,
                    ResponseBody::LifecycleEvents(LifecyclePublisherOutput(page)),
                )?;
            }
        }
        self.session.progress = Some(progress::WorldProgress {
            tick: report.tick,
            time: report.time,
        });
        Ok(())
    }

    pub(super) fn queue_response(
        &mut self,
        request_id: u64,
        mut body: ResponseBody,
    ) -> Result<(), String> {
        if !self.session.outbox.is_live() {
            return Err("World session output is closed".into());
        }

        let reservation = self.session.reply_reservations.remove(&request_id);
        let is_batch = matches!(body, ResponseBody::Batch(_));
        let request_id = if request_id == 0 {
            0
        } else {
            let (original, batch_id) = self
                .session
                .request_origins
                .remove(&request_id)
                .ok_or("World response has no session correlation")?;
            match &mut body {
                ResponseBody::Batch(outcome) => {
                    outcome.outcome.batch_id =
                        batch_id.ok_or("Batch response has no caller batch identity")?;
                }
                ResponseBody::GeometryPickResultEvent(outcome) => outcome.request_id = original,
                ResponseBody::CameraProjectResultEvent(outcome) => outcome.request_id = original,
                _ => {}
            }
            original
        };
        let mut response = Response {
            session: self.session.id,
            request_id,
            tick: self.world.tick(),
            body,
        };
        #[cfg(feature = "diagnostics")]
        if matches!(response.body, ResponseBody::LifecycleDiagnostics(_)) {
            response.tick = 0;
        }
        let size = ipp_protocol::encoded_response_size(&response)
            .or_else(|error| {
                if request_id == 0 || is_batch {
                    return Err(error);
                }
                response.body = ResponseBody::Error {
                    code: 3,
                    message: format!("response unavailable: {error}"),
                };
                ipp_protocol::encoded_response_size(&response)
            })
            .map_err(|error| error.to_string())?;
        let reservation = match reservation {
            Some(reservation) => Ok(reservation),
            None => attachment_receipts::ReplyReservation::new(
                self.session.reply_budget.clone(),
                OutputClass::Ordinary,
                size,
            )
            .map(|reservation| std::rc::Rc::new(std::cell::RefCell::new(reservation))),
        }
        .and_then(|reservation| {
            reservation.borrow_mut().reserve_bytes(size)?;
            Ok(reservation)
        });
        let reservation = match reservation {
            Ok(reservation) => reservation,
            Err(error) => {
                let usage = self.session.reply_budget.0.usage();
                return Err(format!(
                    "connection congestion: shared reliable output capacity exhausted: {error} body={} session={} request={request_id} size={size} entries={} bytes={} account={:?}",
                    response_kind(&response.body),
                    self.session.id,
                    usage.entries,
                    usage.bytes,
                    self.session.reply_budget.0.status(),
                ));
            }
        };
        let mut bytes = Vec::with_capacity(size);
        ipp_protocol::encode_response_into(&response, &mut bytes)
            .map_err(|error| error.to_string())?;
        self.session.outbox.observe_tick(response.tick);
        drop(response);
        reservation.borrow_mut().encoded(bytes.capacity());
        self.session.outbox.push_back(QueuedResponse {
            bytes,
            reservation,
        });
        Ok(())
    }
}
