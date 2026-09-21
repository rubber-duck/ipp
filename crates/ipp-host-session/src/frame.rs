use super::*;

impl<P: HostServices> WorldSessionContext<'_, P> {
    /// Complete exactly one host-clock frame, including idle frames.
    pub(super) fn prepare_frame(&mut self, dt: f64) -> Result<(), String> {
        if let Some(error) = self.session.pending_errors.remove(&0) {
            return Err(error);
        }
        if self.session.prepared || !self.session.ready {
            return Ok(());
        }
        if self.session.pending.len() + self.session.outbox.len() + EVENT_RESERVE > MAX_OUTBOX {
            return Err("connection congestion: reliable output capacity exhausted".to_owned());
        }
        if !dt.is_finite() || dt < 0.0 || !(self.world.time() + dt).is_finite() {
            return Err("invalid host frame delta".to_owned());
        }

        let mut replies = if ipp_core::allocation_optimizations_enabled() {
            std::mem::take(&mut self.session.replies)
        } else {
            Vec::new()
        };
        replies.clear();
        replies.reserve(self.session.pending.len());
        while !self.session.pending.front().is_some_and(|request| {
            matches!(
                request.body,
                RequestBody::BeginBatch | RequestBody::EndBatch(_)
            ) || is_command_batch_continuation(&request.body)
        }) {
            let Some(request) = self.session.pending.pop_front() else {
                break;
            };
            if let Some(error) = self.session.pending_errors.remove(&request.request_id) {
                replies.push((request.request_id, WorldSessionReply::Rejected(error)));
                continue;
            }
            let reply = match request.body {
                #[cfg(feature = "surfaces")]
                RequestBody::SurfaceCommand(command) => {
                    match self.world.enqueue_system_command_with_reply(
                        ipp_core::systems::surface::SurfaceSystem::ID,
                        self.session.id,
                        request.request_id,
                        command,
                    ) {
                        Ok(()) => WorldSessionReply::SurfaceCommand,
                        Err(error) => WorldSessionReply::Rejected(error.to_string()),
                    }
                }
                #[cfg(feature = "gui")]
                RequestBody::GuiCommands {
                    batch_id: None,
                    commands,
                } => {
                    match self.world.enqueue_gui_commands_with_reply(
                        self.session.id,
                        request.request_id,
                        commands,
                    ) {
                        Ok(()) => WorldSessionReply::GuiCommands,
                        Err(error) => WorldSessionReply::Rejected(error.to_string()),
                    }
                }
                #[cfg(feature = "gui")]
                RequestBody::GuiInspect(query) => WorldSessionReply::GuiInspect(query),
                #[cfg(feature = "gui")]
                RequestBody::GuiInput(command) => {
                    match self.world.enqueue_gui_input_command_with_reply(
                        self.session.id,
                        request.request_id,
                        *command,
                    ) {
                        Ok(()) => WorldSessionReply::GuiInput,
                        Err(error) => WorldSessionReply::Rejected(error.to_string()),
                    }
                }
                #[cfg(feature = "gui")]
                RequestBody::GuiSemanticSnapshot(query) => {
                    WorldSessionReply::GuiSemanticSnapshot(query)
                }
                #[cfg(feature = "gui")]
                RequestBody::GuiSemanticAction(action_request) => {
                    self.resolve_semantic_action(request.request_id, &action_request)
                }
                RequestBody::BeginBatch | RequestBody::EndBatch(_) | RequestBody::BatchChunk(_) => {
                    unreachable!("Host command batch boundary")
                }
                #[cfg(feature = "gui")]
                RequestBody::GuiCommands {
                    batch_id: Some(_),
                    ..
                } => unreachable!("Host command batch continuation"),
                RequestBody::LifecycleSubscription(command) => {
                    match self.world.enqueue_system_command_with_reply(
                        LifecyclePublisherSystem::ID,
                        self.session.id,
                        request.request_id,
                        command,
                    ) {
                        Ok(()) => WorldSessionReply::LifecycleSubscription,
                        Err(error) => WorldSessionReply::Rejected(error.to_string()),
                    }
                }
                RequestBody::AnimationPlaybackCommand {
                    controller,
                    control,
                } => {
                    if let Err(_reason) = self.world.enqueue_playback(controller, control) {
                        ipp_core::diagnostic!(
                            Warn,
                            "[IPP host] playback.enqueue.reject reason={_reason}"
                        );
                    }
                    continue;
                }
                RequestBody::AnimationController(command) => {
                    match self
                        .world
                        .enqueue_animation_controller(request.request_id, command)
                    {
                        Ok(()) => WorldSessionReply::AnimationController,
                        Err(error) => WorldSessionReply::Rejected(error.to_string()),
                    }
                }
                RequestBody::Batch(batch) => {
                    #[cfg(feature = "diagnostics")]
                    let (batch_id, operations) = (batch.id, batch.operations.len());
                    #[cfg(feature = "diagnostics")]
                    if operations != 0 {
                        ipp_core::diagnostic!(
                            Debug,
                            "[IPP {}] buffer.processing session={} request={} batch={} operations={}",
                            P::NAME,
                            self.session.id,
                            request.request_id,
                            batch_id,
                            operations
                        );
                    }
                    match self.world.enqueue(batch) {
                        Ok(()) => WorldSessionReply::Batch {
                            #[cfg(feature = "diagnostics")]
                            operations,
                        },
                        Err(error) => {
                            ipp_core::diagnostic!(
                                Warn,
                                "[IPP {}] buffer.reject session={} request={} batch={} operations={} reason={}",
                                P::NAME,
                                self.session.id,
                                request.request_id,
                                batch_id,
                                operations,
                                error
                            );
                            WorldSessionReply::Rejected(error.to_string())
                        }
                    }
                }
                RequestBody::Inspect(query) => WorldSessionReply::Inspect(query),
                RequestBody::RenderStateUpdateCommand(patch) => {
                    if let Err(_reason) = self.world.enqueue_render_state_update(patch) {
                        ipp_core::diagnostic!(
                            Warn,
                            "[IPP {}] command.reject session={} reason={}",
                            P::NAME,
                            self.session.id,
                            _reason
                        );
                    }
                    continue;
                }
                RequestBody::CameraActivateCommand {
                    entity,
                } => {
                    if let Err(_reason) = self.world.enqueue_camera_activate(entity) {
                        ipp_core::diagnostic!(
                            Warn,
                            "[IPP {}] command.reject session={} reason={}",
                            P::NAME,
                            self.session.id,
                            _reason
                        );
                    }
                    continue;
                }
                RequestBody::CameraNavigateCommand(motion) => {
                    if let Err(_reason) = self.world.enqueue_camera_navigate(motion) {
                        ipp_core::diagnostic!(
                            Warn,
                            "[IPP {}] command.reject session={} reason={}",
                            P::NAME,
                            self.session.id,
                            _reason
                        );
                    }
                    continue;
                }
                RequestBody::GeometryPickQuery(query) => WorldSessionReply::GeometryPick(
                    self.world.enqueue_geometry_pick(request.request_id, query),
                ),
                RequestBody::CameraProjectQuery(query) => WorldSessionReply::CameraProject(
                    self.world.enqueue_camera_project(request.request_id, query),
                ),
            };
            replies.push((request.request_id, reply));
        }

        self.session.replies = replies;
        self.session.prepared = true;
        Ok(())
    }

    /// Evaluate and publish a prepared session frame after shared service progress.
    pub fn tick(&mut self, dt: f64) -> Result<(), String> {
        self.prepare_frame(dt)?;
        if !self.session.ready {
            return Ok(());
        }
        self.world
            .prepare_update(dt)
            .map_err(|error| error.to_string())?;

        self.world
            .set_render_viewport(self.services.render_viewport());

        #[allow(unused_mut)]
        let mut report = self.world.step(dt).map_err(|error| error.to_string())?;
        if let Some(reason) = self.world.fault() {
            self.record_runtime_failure(
                ipp_protocol::RuntimeFailureScope::World,
                true,
                reason.to_string(),
            )?;
        } else if let Err(error) = self.services.present(&mut self.world) {
            self.record_runtime_failure(error.scope, false, error.message)?;
        } else {
            self.session.last_failure = None;
        }
        {
            report
                .resource_changes
                .extend(self.world.take_asset_events()?);
        }

        report.assets.extend(self.world.take_asset_outcomes());
        self.publish_report(&report)
    }
}
