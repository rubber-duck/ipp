use super::*;

impl<P: HostServices> WorldSessionContext<'_, P> {
    pub(super) fn record_runtime_failure(
        &mut self,
        scope: ipp_protocol::RuntimeFailureScope,
        faulted: bool,
        mut message: String,
    ) -> Result<(), String> {
        if message.len() > 2048 {
            let mut end = 2048;
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
        report: &ipp_core::WorldUpdateReport,
    ) -> Result<(), String> {
        self.session.prepared = false;
        let mut replies = std::mem::take(&mut self.session.replies);
        let origins: std::collections::BTreeSet<_> =
            self.session.request_origins.keys().copied().collect();
        let diagnostics: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|diagnostic| self.session.owners.contains(&diagnostic.owner))
            .cloned()
            .collect();
        for change in report.camera_state_changes.iter().cloned() {
            self.queue_response(0, ResponseBody::CameraStateChangedEvent(change))?;
        }
        for change in report.render_state_changes.iter().cloned() {
            self.queue_response(0, ResponseBody::RenderStateUpdatedEvent(change))?;
        }
        if !report.playback_events.is_empty() {
            self.queue_response(
                0,
                ResponseBody::PlaybackEvents(report.playback_events.clone()),
            )?;
        }
        // Committed GUI observations travel as unsolicited, chunked wire
        // messages: broadcast effects/conflicts/cancellations plus
        // supplier-only unhandled input for scene fallback. The helper
        // filters by session so raw input never leaks across sessions.
        #[cfg(feature = "gui")]
        for body in ipp_protocol::gui_observation_bodies(report, self.session.id) {
            self.queue_response(0, body)?;
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
            .iter()
            .filter(|outcome| origins.contains(&outcome.batch_id))
            .cloned();
        let mut geometry_picks = report
            .geometry_picks
            .iter()
            .filter(|outcome| origins.contains(&outcome.request_id))
            .cloned();
        let mut camera_projections = report
            .camera_projections
            .iter()
            .filter(|outcome| origins.contains(&outcome.request_id))
            .cloned();
        let result = (|| {
            for (request_id, reply) in replies.drain(..) {
                let body = match reply {
                    #[cfg(feature = "surfaces")]
                    WorldSessionReply::SurfaceCommand => {
                        let outcome = system_outcomes
                            .pop_front()
                            .ok_or("core did not publish Surface outcome")?;
                        if outcome.request_id != request_id {
                            return Err("Surface outcome correlation".into());
                        }
                        match outcome.result {
                            Ok(()) => ResponseBody::SurfaceCommand,
                            Err(reason) => ResponseBody::Error {
                                code: 1,
                                message: reason.to_string(),
                            },
                        }
                    }
                    #[cfg(feature = "gui")]
                    WorldSessionReply::GuiCommands => {
                        let outcome = system_outcomes
                            .pop_front()
                            .ok_or("core did not publish GUI outcome")?;
                        if outcome.request_id != request_id {
                            return Err("GUI outcome correlation".into());
                        }
                        ResponseBody::GuiCommands {
                            applied: u32::try_from(outcome.applied)
                                .map_err(|_| "GUI applied prefix exceeds u32")?,
                            error: outcome.result.err(),
                        }
                    }
                    #[cfg(feature = "gui")]
                    WorldSessionReply::GuiInspect(query) => {
                        match self.world.inspect_gui(
                            query.entity,
                            query.node_id,
                            query.max_depth,
                            query.limit,
                        ) {
                            Ok(response) => ResponseBody::GuiInspect(response),
                            Err(error) => ResponseBody::Error {
                                code: 1,
                                message: error.to_string(),
                            },
                        }
                    }
                    #[cfg(feature = "gui")]
                    WorldSessionReply::GuiInput => {
                        let outcome = system_outcomes
                            .pop_front()
                            .ok_or("core did not publish GUI input outcome")?;
                        if outcome.request_id != request_id {
                            return Err("GUI input outcome correlation".into());
                        }
                        match outcome.result {
                            Ok(()) => {
                                let mut routed =
                                    report.gui_unhandled_inputs.iter().filter(|input| {
                                        input.session == self.session.id
                                            && input.source_request_id == request_id
                                    });
                                let unhandled = routed.next().map(|input| input.reason.clone());
                                if routed.next().is_some() {
                                    return Err(
                                        "GUI input published duplicate routing dispositions".into(),
                                    );
                                }
                                ResponseBody::GuiInput {
                                    tick: report.tick,
                                    unhandled,
                                }
                            }
                            Err(reason) => ResponseBody::Error {
                                code: 1,
                                message: reason.to_string(),
                            },
                        }
                    }
                    #[cfg(feature = "gui")]
                    WorldSessionReply::GuiSemanticSnapshot(query) => {
                        match self.gui_semantic_snapshot(&query) {
                            Ok(tree) => ResponseBody::GuiSemanticSnapshot(tree),
                            Err(error) => ResponseBody::Error {
                                code: 1,
                                message: error,
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
                        ResponseBody::Batch(outcome)
                    }
                    WorldSessionReply::Inspect(query) => {
                        self.inspection_page(query, request_id, report.tick, report.time)
                    }
                    WorldSessionReply::GeometryPick(result) => {
                        let outcome = match result {
                            Ok(()) => geometry_picks.next().ok_or_else(|| {
                                "core did not publish the submitted geometry outcome".to_owned()
                            })?,
                            Err(reason) => ipp_core::GeometryPickOutcome {
                                request_id,
                                tick: report.tick,
                                camera: self.world.active_camera(),
                                result: Err(reason),
                            },
                        };
                        ResponseBody::GeometryPickResultEvent(outcome)
                    }
                    WorldSessionReply::CameraProject(result) => {
                        let outcome = match result {
                            Ok(()) => camera_projections.next().ok_or_else(|| {
                                "core did not publish the submitted projection".to_owned()
                            })?,
                            Err(reason) => ipp_core::CameraProjectOutcome {
                                request_id,
                                tick: report.tick,
                                camera: self.world.active_camera(),
                                result: Err(reason),
                            },
                        };
                        ResponseBody::CameraProjectResultEvent(outcome)
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
        if ipp_core::allocation_optimizations_enabled() {
            self.session.replies = replies;
        }
        result?;
        if !system_outcomes.is_empty() {
            return Err("core published an uncorrelated system command outcome".into());
        }
        if outcomes.next().is_some() {
            return Err("core published an uncorrelated batch outcome".to_owned());
        }
        if geometry_picks.next().is_some() {
            return Err("core published an uncorrelated geometry outcome".to_owned());
        }
        if camera_projections.next().is_some() {
            return Err("core published an uncorrelated projection outcome".to_owned());
        }
        if !diagnostics.is_empty() {
            self.queue_response(
                0,
                ResponseBody::Lifecycle {
                    diagnostics,
                },
            )?;
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
        for output in self.world.drain_system_events::<LifecyclePublisherOutput>(
            LifecyclePublisherSystem::ID,
            self.session.id,
        ) {
            self.queue_response(0, ResponseBody::LifecycleEvents(output))?;
        }
        self.session
            .owners
            .retain(|owner| self.world.state_overlay_owner_is_live(*owner));
        self.queue_response(
            0,
            ResponseBody::Frame {
                time: report.time,
            },
        )
    }

    pub(super) fn queue_response(
        &mut self,
        request_id: u64,
        mut body: ResponseBody,
    ) -> Result<(), String> {
        if self.session.outbox.len() >= MAX_OUTBOX {
            return Err("connection congestion: reliable output capacity exhausted".into());
        }

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
                    outcome.batch_id =
                        batch_id.ok_or("Batch response has no caller batch identity")?;
                    {
                        self.session.owners.extend(
                            outcome
                                .state_overlays
                                .iter()
                                .filter(|alias| {
                                    alias.kind == ipp_core::StateOverlayHandleKind::Owner
                                })
                                .map(|alias| alias.id),
                        );
                    }
                }
                ResponseBody::GeometryPickResultEvent(outcome) => outcome.request_id = original,
                ResponseBody::CameraProjectResultEvent(outcome) => outcome.request_id = original,
                _ => {}
            }
            original
        };
        let mut bytes = if ipp_core::allocation_followup_enabled() {
            self.response_buffers
                .pop()
                .unwrap_or_else(|| Vec::with_capacity(4096))
        } else {
            Vec::new()
        };
        let result = ipp_protocol::encode_response_into(
            &Response {
                session: self.session.id,
                request_id,
                tick: self.world.tick(),
                body,
            },
            &mut bytes,
        )
        .or_else(|error| {
            if request_id == 0 {
                return Err(error);
            }
            ipp_protocol::encode_response_into(
                &Response {
                    session: self.session.id,
                    request_id,
                    tick: self.world.tick(),
                    body: ResponseBody::Error {
                        code: 3,
                        message: format!("response unavailable: {error}"),
                    },
                },
                &mut bytes,
            )
        });
        if let Err(error) = result {
            self.recycle_response_buffer(bytes);
            return Err(error.to_string());
        }
        self.session.outbox.push_back(bytes);
        Ok(())
    }
}
