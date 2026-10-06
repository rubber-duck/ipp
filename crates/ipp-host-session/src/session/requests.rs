//! Per-request dispatch at the Host mutation boundary, in global request order.

use super::attachment_receipts;
use crate::*;
use std::collections::BTreeSet;

impl<P: HostServices> WorldSessionContext<'_, P> {
    /// Admit one request in Host order without evaluating the World.
    pub(crate) fn prepare_request(&mut self, dt: f64) -> Result<(), String> {
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
                    let (batch_id, operations) = (page.batch_id, batch.operations.len());
                    if operations != 0 {
                        ipp_core::diagnostic!(
                            Debug,
                            "[IPP {}] buffer.processing session={} request={} batch={} operations={} client_batch={}",
                            P::NAME,
                            self.session.id,
                            request.request_id,
                            request.request_id,
                            operations,
                            batch_id
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
                            operations,
                        },
                        Err(error) => {
                            ipp_core::diagnostic!(
                                Warn,
                                "[IPP {}] buffer.reject session={} request={} batch={} operations={} client_batch={} reason={}",
                                P::NAME,
                                self.session.id,
                                request.request_id,
                                request.request_id,
                                operations,
                                batch_id,
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
                        6 | 7 => Some(ipp_core::systems::WorldOperation::Gui),
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
                RequestBody::GuiPreferencesUpdateCommand(update) => {
                    if let Err(_reason) = self.world.enqueue_gui_preferences_update(update) {
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
                    let manifest = self.world.manifest();
                    let supports_canvas =
                        manifest.supports_operation(ipp_core::systems::WorldOperation::Canvas);
                    let supports_camera_geometry = manifest
                        .supports_operation(ipp_core::systems::WorldOperation::Camera)
                        && manifest.supports_operation(ipp_core::systems::WorldOperation::Geometry);
                    if !supports_canvas && !supports_camera_geometry {
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

impl<P: HostServices> Host<P> {
    /// Admit ready session requests in global request order at the Host mutation boundary.
    pub(crate) fn admit_session_requests(&mut self, dt: f64) -> Vec<(u64, String)> {
        let mut deferred = BTreeSet::new();
        let mut failed = BTreeSet::new();
        let mut failures = Vec::new();
        loop {
            let next = self
                .sessions
                .values()
                .filter(|session| session.ready && !failed.contains(&session.id))
                .filter_map(|session| {
                    if deferred.contains(&session.id) {
                        return None;
                    }
                    let request = session.pending.front()?;
                    Some((request.request_id, session.id, 0))
                })
                .min();
            let Some((_, id, position)) = next else {
                break;
            };
            match self.admit_session_request(id, position, dt) {
                Ok(true) => {
                    deferred.clear();
                }
                Ok(false) => {
                    deferred.insert(id);
                }
                Err(error) => {
                    failures.push((self.connection_for_session(id), error));
                    failed.insert(id);
                }
            }
        }
        failures
    }

    fn admit_session_request(&mut self, id: u64, position: usize, dt: f64) -> Result<bool, String> {
        let body = &self.sessions[&id].pending[position].body;
        if matches!(body, RequestBody::CameraNavigate(_)) {
            return self.admit_camera_navigation(id, position);
        }
        if matches!(body, RequestBody::AttachmentReceipt { .. })
            && !self.sessions[&id].replies.is_empty()
        {
            return Ok(false);
        }
        if !matches!(body, RequestBody::AttachmentReceipt { .. }) {
            self.runtime
                .world_ref(self.sessions[&id].world)
                .ok_or("World is closed")?;
            self.session_mut(id)
                .expect("live session")
                .prepare_request(dt)?;
            return Ok(true);
        }
        let request = self
            .sessions
            .get_mut(&id)
            .unwrap()
            .pending
            .remove(position)
            .unwrap();
        let RequestBody::AttachmentReceipt {
            receipt,
            release,
        } = request.body
        else {
            unreachable!("receipt boundary");
        };
        let result = if release {
            self.sessions[&id]
                .receipts
                .borrow_mut()
                .release(receipt)
                .map(|()| None)
        } else {
            self.sessions[&id]
                .receipts
                .borrow()
                .resolve(receipt)
                .and_then(|token| self.runtime.attachment_retirement(&token))
                .map(|state| Some(state == ipp_core::WorldAttachmentRetirement::Retired))
        };
        let response = match result {
            Ok(retired) => ResponseBody::AttachmentReceipt {
                receipt,
                retired,
            },
            Err(error) => ResponseBody::Error {
                code: 1,
                message: error.to_string(),
            },
        };
        self.session_mut(id)
            .expect("live session")
            .queue_response(request.request_id, response)?;
        Ok(true)
    }
}
