use super::*;

impl<P: HostServices> WorldSessionContext<'_, P> {
    /// Admit one request in Host order without evaluating the World.
    pub(super) fn prepare_request(&mut self, dt: f64) -> Result<(), String> {
        if let Some(error) = self.session.pending_errors.remove(&0) {
            return Err(error);
        }
        if !self.session.ready {
            return Ok(());
        }
        if !dt.is_finite() || dt < 0.0 || !(self.world.time() + dt).is_finite() {
            return Err("invalid host frame delta".to_owned());
        }

        let mut replies = std::mem::take(&mut self.session.replies);
        replies.reserve(1);
        'request: {
            let Some(request) = self.session.pending.pop_front() else {
                break 'request;
            };
            // An assembled batch leaves the connection's buffered budget for the World queue.
            self.session.batch_leases.remove(&request.request_id);
            if let Some(error) = self.session.pending_errors.remove(&request.request_id) {
                replies.push((request.request_id, WorldSessionReply::Rejected(error)));
                break 'request;
            }
            let reply = match request.body {
                #[cfg(feature = "gui")]
                RequestBody::GuiObservation(control) => {
                    match self.prepare_gui_observation(request.request_id, control) {
                        Ok(()) => break 'request,
                        Err(error) => WorldSessionReply::Rejected(error),
                    }
                }
                RequestBody::AttachmentReceipt {
                    ..
                } => {
                    unreachable!("Host receipt boundary")
                }
                RequestBody::LifecycleWatch(control) => {
                    match self.prepare_lifecycle_watch(request.request_id, control) {
                        Ok(()) => break 'request,
                        Err(error) => WorldSessionReply::Rejected(error),
                    }
                }
                #[cfg(feature = "diagnostics")]
                RequestBody::LifecycleDiagnostics(query) => {
                    WorldSessionReply::LifecycleDiagnostics(query)
                }
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
                    self.session.request_origins.remove(&request.request_id);
                    break 'request;
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
                RequestBody::SubmitBatch(page) => {
                    let mut batch = ipp_core::Batch {
                        id: request.request_id,
                        operations: page.operations,
                    };
                    // Unconditional: `diagnostic!` expands whenever `ipp-core`
                    // diagnostics is enabled, which feature unification can do
                    // without this crate's own `diagnostics` feature.
                    let (_batch_id, _operations) = (page.batch_id, batch.operations.len());
                    #[cfg(feature = "diagnostics")]
                    if _operations != 0 {
                        ipp_core::diagnostic!(
                            Debug,
                            "[IPP {}] buffer.processing session={} request={} batch={} operations={} client_batch={}",
                            P::NAME,
                            self.session.id,
                            request.request_id,
                            request.request_id,
                            _operations,
                            _batch_id
                        );
                    }
                    // Bound the outcome's symbol reports like its aliases, before
                    // anything applies: too many is refused here, and their text
                    // joins the reply admission of the receipt sink with the
                    // aliases the batch defines and the adoptions it can report.
                    let mut symbols = ipp_core::BatchSymbolReports::default();
                    let (mut aliases, mut adoptions) = (0, 0);
                    for command in &mut batch.operations {
                        symbols.add(command);
                        aliases += usize::from(attachment_receipts::defines_alias(command));
                        adoptions += usize::from(attachment_receipts::may_adopt(command));
                    }
                    let result = if symbols.reports() > ipp_protocol::BATCH_OUTCOME_ALIASES {
                        Err(format!(
                            "Batch names more than {} distinct symbolic references, which one batch outcome cannot report",
                            ipp_protocol::BATCH_OUTCOME_ALIASES
                        ))
                    } else {
                        attachment_receipts::ReceiptSink::new(
                            self.session.receipts.clone(),
                            self.session.reply_reservations[&request.request_id].clone(),
                            aliases,
                            adoptions,
                            &symbols,
                        )
                        .and_then(|(sink, reservation)| {
                            self.session
                                .reply_reservations
                                .insert(request.request_id, reservation);
                            self.world.enqueue_with_effect_sink(batch, Box::new(sink))
                        })
                        .map_err(|error| error.to_string())
                    };
                    match result {
                        Ok(()) => WorldSessionReply::Batch {
                            #[cfg(feature = "diagnostics")]
                            operations: _operations,
                        },
                        Err(error) => {
                            ipp_core::diagnostic!(
                                Warn,
                                "[IPP {}] buffer.reject session={} request={} batch={} operations={} client_batch={} reason={}",
                                P::NAME,
                                self.session.id,
                                request.request_id,
                                request.request_id,
                                _operations,
                                _batch_id,
                                error
                            );
                            WorldSessionReply::Rejected(error)
                        }
                    }
                }
                RequestBody::Inspect(query) => {
                    let required = match query.collection {
                        3 => Some(ipp_core::systems::WorldOperation::Animation),
                        4 => Some(ipp_core::systems::WorldOperation::Rendering),
                        #[cfg(feature = "gui")]
                        6 | 7 => Some(ipp_core::systems::WorldOperation::Gui),
                        #[cfg(feature = "surfaces")]
                        8 => Some(ipp_core::systems::WorldOperation::Canvas),
                        _ => None,
                    };
                    if required.is_some_and(|operation| {
                        !self.world.manifest().supports_operation(operation)
                    }) {
                        WorldSessionReply::Rejected(
                            "Unsupported inspection collection for this World".to_owned(),
                        )
                    } else {
                        WorldSessionReply::Inspect(query)
                    }
                }
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
                    self.session.request_origins.remove(&request.request_id);
                    break 'request;
                }
                #[cfg(feature = "surfaces")]
                RequestBody::CanvasStateUpdateCommand(update) => {
                    if let Err(_reason) = self.world.enqueue_canvas_state_update(update) {
                        ipp_core::diagnostic!(
                            Warn,
                            "[IPP {}] command.reject session={} reason={}",
                            P::NAME,
                            self.session.id,
                            _reason
                        );
                    }
                    self.session.request_origins.remove(&request.request_id);
                    break 'request;
                }
                RequestBody::GeometryPickQuery(query) => {
                    if !self
                        .world
                        .manifest()
                        .supports_operation(ipp_core::systems::WorldOperation::Camera)
                        || !self
                            .world
                            .manifest()
                            .supports_operation(ipp_core::systems::WorldOperation::Geometry)
                    {
                        WorldSessionReply::Rejected(
                            "Unsupported camera/geometry query for this World".to_owned(),
                        )
                    } else {
                        WorldSessionReply::GeometryPick(query)
                    }
                }
                RequestBody::CameraProjectQuery(query) => {
                    if !self
                        .world
                        .manifest()
                        .supports_operation(ipp_core::systems::WorldOperation::Camera)
                    {
                        WorldSessionReply::Rejected(
                            "Unsupported camera projection for this World".to_owned(),
                        )
                    } else {
                        WorldSessionReply::CameraProject(query)
                    }
                }
                RequestBody::CameraNavigate(_) => unreachable!("Host camera admission boundary"),
            };
            replies.push((request.request_id, reply));
        }

        self.session.replies = replies;
        self.session.prepared = true;
        Ok(())
    }
}
