use super::*;

const CONGESTED: &str = "connection congestion: Host ingress capacity exhausted";

/// World creation has no default selection; a request must name its Systems.
pub(crate) const WORLD_SELECTION_REQUIRED: &str =
    "World creation requires a System selection: name the Systems the World uses";

impl<P: HostServices> Host<P> {
    pub(crate) fn expire_queued_presentations(&mut self) {
        use ipp_protocol::presentation::{
            PresentationError, PresentationRequest, PresentationResponse,
        };
        let now = self.connections.now;
        for connection in self.connections.states.values_mut() {
            let mut expired = Vec::new();
            connection.pending.retain(|ingress| {
                if let HostConnectionIngress::Control {
                    request:
                        HostRequest {
                            request_id,
                            body:
                                HostRequestBody::Presentation(PresentationRequest::Frame {
                                    ..
                                }),
                            ..
                        },
                    received_at,
                } = ingress
                    && super::super::presentation::frame_deadline(*received_at) <= now
                {
                    expired.push(*request_id);
                    false
                } else {
                    true
                }
            });
            for request in expired {
                if let Err(error) = connection.reply(
                    request,
                    HostResponseBody::Presentation(PresentationResponse::Error(
                        PresentationError::Timeout,
                    )),
                ) {
                    connection.failure = Some(error);
                }
            }
        }
    }

    pub(crate) fn publish_presentation_responses(&mut self) -> Vec<(u64, String)> {
        let mut failures = Vec::new();
        for (id, request, response) in self.presentation.take_completed() {
            if let Some(connection) = self.connections.states.get_mut(&id) {
                connection.presentation_pending -= 1;
                if let Err(error) =
                    connection.reply(request, HostResponseBody::Presentation(response))
                {
                    failures.push((id, error));
                }
            }
        }
        failures
    }

    /// Open only a transport connection. World creation requires an explicit Host request.
    pub fn open_connection(&mut self, id: u64) -> Result<(), String> {
        if id == 0 || self.connections.states.contains_key(&id) {
            return Err("Connection identity is zero or already live".into());
        }
        self.connections.states.insert(
            id,
            HostConnectionState {
                id,
                ready: false,
                throttled: false,
                sessions: BTreeSet::new(),
                ended_sessions: BTreeSet::new(),
                last_output_session: 0,
                last_progress_session: 0,
                progress_turn: true,
                temporary_worlds: BTreeSet::new(),
                pending: VecDeque::with_capacity(crate::MAX_PENDING),
                outbox: Default::default(),
                reply_reservations: BTreeMap::new(),
                failure: None,
                transfer: None,
                reply_budget: Default::default(),
                progress_leases: Default::default(),
                presentation_pending: 0,
                batches: Default::default(),
                datasets: Default::default(),
            },
        );
        Ok(())
    }

    /// Charge bounded transport allocations to the connection until their actual destruction.
    pub fn reserve_connection_output_bytes(
        &self,
        id: u64,
        bytes: usize,
    ) -> Result<ipp_core::services::reliable_output::ReliableOutputLease, String> {
        self.connections
            .states
            .get(&id)
            .ok_or("Connection is closed")?
            .reply_budget
            .0
            .reserve(ipp_core::services::reliable_output::OutputCharge {
                entries: 0,
                bytes,
            })
            .map_err(|error| format!("connection output allocation unavailable: {error:?}"))
    }

    /// Per-connection admission hysteresis, independent of other senders.
    ///
    /// Admitted requests are counted against [`crate::MAX_PENDING`]. Output congestion is the
    /// bytes of undelivered records measured against the room the ordinary share leaves them
    /// after long-lived endpoint, membership and transport metadata, which a reader cannot drain.
    /// Throttle at three quarters of either bound and resume once both are below half.
    pub fn connection_accepts_input(&mut self, id: u64) -> bool {
        use crate::reliable_output::ORDINARY_OUTPUT_BYTES;

        let Some(connection) = self.connections.states.get_mut(&id) else {
            return false;
        };
        // An admitted source update needs its next chunk before its final reply can
        // complete. Count hysteresis must not withhold those continuations behind
        // other long-lived reservations; output-byte pressure still withholds input.
        let ingress = if connection.datasets.has_transfers() {
            0
        } else {
            connection.admitted_requests(&self.sessions)
        };
        let account = &connection.reply_budget.0;
        let backlog = account.record_usage().bytes;
        let metadata = account.usage().bytes - backlog;
        let room = ORDINARY_OUTPUT_BYTES.saturating_sub(metadata);
        connection.throttled = if connection.throttled {
            ingress >= crate::MAX_PENDING / 2 || backlog >= room / 2
        } else {
            ingress >= crate::MAX_PENDING * 3 / 4 || backlog >= room * 3 / 4
        };
        !connection.throttled
    }

    /// Validate transport input and enqueue it without advancing simulation time.
    ///
    /// World requests are decoded here, into the World's recycled command buffers.
    pub fn receive_connection(&mut self, id: u64, bytes: &[u8]) -> Result<(), String> {
        let connection = self
            .connections
            .states
            .get_mut(&id)
            .ok_or("Connection is closed")?;
        if !connection.ready {
            let reservation = connection.reserve_reply(256)?;
            let reply = ipp_protocol::accept_hello(bytes, id).map_err(|error| error.to_string())?;
            reservation.borrow_mut().encoded(reply.capacity());
            connection.outbox.push_back(ReliableResponse {
                bytes: reply,
                reservation,
            });
            connection.ready = true;
            return Ok(());
        }
        if bytes.starts_with(ipp_protocol::dataset::REQUEST_MAGIC) {
            return self.receive_dataset(id, bytes);
        }
        if bytes.starts_with(ipp_protocol::asset_source::REQUEST_MAGIC) {
            return self.receive_asset_source(id, bytes);
        }
        if bytes.len() > ipp_protocol::MAX_MESSAGE_BYTES {
            return Err(CONGESTED.into());
        }
        // Batch pages are admitted by their batch: only a page that opens one counts.
        let congested = connection.admitted_requests(&self.sessions) >= crate::MAX_PENDING;
        if bytes == ipp_protocol::CONTRACT_REQUEST {
            // An admitted request whose reply is charged until physical delivery.
            if congested {
                return Err(CONGESTED.into());
            }
            let reply = ipp_protocol::contract_reply();
            let reservation = connection.reserve_reply(reply.capacity())?;
            reservation.borrow_mut().encoded(reply.capacity());
            ipp_core::diagnostic!(
                Info,
                "[IPP {}] connection.contract connection={} bytes={}",
                P::NAME,
                id,
                reply.len()
            );
            connection.outbox.push_back(ReliableResponse {
                bytes: reply,
                reservation,
            });
            return Ok(());
        }
        if bytes.starts_with(host::HOST_REQUEST_MAGIC) {
            if congested {
                return Err(CONGESTED.into());
            }
            let request =
                host::decode_host_request(bytes, id).map_err(|error| error.to_string())?;
            if connection
                .reply_reservations
                .contains_key(&request.request_id)
            {
                return Err("duplicate outstanding Host request".into());
            }
            let ingress_bytes = match &request.body {
                HostRequestBody::Presentation(
                    ipp_protocol::presentation::PresentationRequest::Frame {
                        after_outputs,
                        ..
                    },
                ) => {
                    256 + after_outputs.capacity()
                        * std::mem::size_of::<ipp_protocol::references::OutputReference>()
                }
                _ => 256,
            };
            let reservation = connection.reserve_reply(ingress_bytes)?;
            connection
                .reply_reservations
                .insert(request.request_id, reservation);
            if let HostRequestBody::Profile(
                body @ (ipp_protocol::profiling::ProfileRequest::Read {
                    ..
                }
                | ipp_protocol::profiling::ProfileRequest::Release(_)),
            ) = request.body
            {
                #[cfg(feature = "instrumentation")]
                let response = self.profiling.request(
                    self.runtime.identity(),
                    P::NAME,
                    id,
                    body,
                    &mut self.services,
                );
                #[cfg(not(feature = "instrumentation"))]
                let response = {
                    let _ = body;
                    ipp_protocol::profiling::ProfileResponse::status(
                        ipp_protocol::profiling::ProfileStatus::Unavailable,
                    )
                };
                return connection.reply(request.request_id, HostResponseBody::Profile(response));
            }
            // A completed capture is an immutable snapshot: reading or releasing it needs no
            // frame boundary, like asset source transfers. Transfers answer at ingress, so they
            // keep their arrival order and never wait behind requests queued for the next frame.
            if let HostRequestBody::Presentation(
                body @ (ipp_protocol::presentation::PresentationRequest::ReadCapture {
                    ..
                }
                | ipp_protocol::presentation::PresentationRequest::ReleaseCapture(_)),
            ) = request.body
            {
                let response = self
                    .presentation
                    .request(
                        &mut self.runtime,
                        &mut self.services,
                        id,
                        request.request_id,
                        body,
                        self.connections.now,
                    )
                    .expect("capture transfers complete immediately");
                return connection
                    .reply(request.request_id, HostResponseBody::Presentation(response));
            }
            connection
                .pending
                .push_back(HostConnectionIngress::Control {
                    request,
                    received_at: self.connections.now,
                });
        } else {
            let session = u64::from_le_bytes(
                bytes
                    .get(..8)
                    .ok_or("Truncated session")?
                    .try_into()
                    .unwrap(),
            );
            if !connection.holds_session(session)? {
                ipp_core::diagnostic!(
                    Debug,
                    "[IPP {}] session.stale connection={} session={}",
                    P::NAME,
                    id,
                    session
                );
                return Ok(());
            }
            let world_id = self
                .sessions
                .get(&session)
                .ok_or("Session is closed")?
                .world;
            let mut buffer = self
                .runtime
                .world_mut(world_id)
                .ok_or("World is closed")?
                .take_command_buffer();
            let request = super::messages::decode_world(bytes, Some(session), &mut buffer);
            self.runtime
                .world_mut(world_id)
                .ok_or("World is closed")?
                .recycle_command_buffer(buffer);
            self.receive_world_request(id, request, congested)?;
        }
        Ok(())
    }

    /// Receive a message prepared by [`super::HostConnectionMessage::decode`],
    /// possibly on a transport thread, without advancing simulation time.
    ///
    /// Equivalent to [`Self::receive_connection`] with the message's bytes.
    pub fn receive_connection_message(
        &mut self,
        id: u64,
        message: super::HostConnectionMessage,
    ) -> Result<(), String> {
        let page = match message.0 {
            super::messages::HostConnectionMessageBody::Encoded(bytes) => {
                return self.receive_connection(id, &bytes);
            }
            super::messages::HostConnectionMessageBody::BatchPage(page) => page.into_arrival(),
        };
        let connection = self
            .connections
            .states
            .get(&id)
            .ok_or("Connection is closed")?;
        if !connection.ready {
            return Err("IPP hello must precede World requests".into());
        }
        if !connection.holds_session(page.session)? {
            ipp_core::diagnostic!(
                Debug,
                "[IPP {}] session.stale connection={} session={}",
                P::NAME,
                id,
                page.session
            );
            return Ok(());
        }
        self.receive_world_request(
            id,
            super::messages::DecodedWorldRequest::BatchPage(page),
            false,
        )
    }

    /// Queue a decoded World request of a live session of this connection.
    ///
    /// `congested` refuses requests other than batch pages, which are admitted
    /// by their batch.
    fn receive_world_request(
        &mut self,
        id: u64,
        request: super::messages::DecodedWorldRequest,
        congested: bool,
    ) -> Result<(), String> {
        let page = match request {
            super::messages::DecodedWorldRequest::BatchPage(page) => page,
            super::messages::DecodedWorldRequest::Failed(error) => return Err(error),
            super::messages::DecodedWorldRequest::Request(request) => {
                let request = *request;
                if congested {
                    return Err(CONGESTED.into());
                }
                let connection = self
                    .connections
                    .states
                    .get_mut(&id)
                    .expect("receiving connection");
                let ingress = HostConnectionIngress::DecodedWorld {
                    reservation: (request.request_id != 0)
                        .then(|| connection.reserve_reply(256))
                        .transpose()?,
                    request,
                    lease: None,
                    rejection: None,
                };
                connection.pending.push_back(ingress);
                return Ok(());
            }
        };
        let Some(batch) = self.receive_batch_page(id, page)? else {
            return Ok(());
        };
        self.connections
            .states
            .get_mut(&id)
            .expect("receiving connection")
            .pending
            .push_back(HostConnectionIngress::DecodedWorld {
                request: batch.request,
                reservation: batch.reservation,
                lease: batch.lease,
                rejection: batch.rejection,
            });
        Ok(())
    }

    /// Host control replies precede World observations; attachment arrives before its frames.
    pub fn take_connection_response(&mut self, id: u64) -> Option<ReliableResponse> {
        let connection = self.connections.states.get_mut(&id)?;
        if let Some(response) = connection.outbox.pop_front() {
            return Some(response);
        }
        for progress in [connection.progress_turn, !connection.progress_turn] {
            if progress && connection.progress_leases.get() != 0 {
                continue;
            }
            let cursor = if progress {
                connection.last_progress_session
            } else {
                connection.last_output_session
            };
            let ordered = connection
                .sessions
                .range((
                    std::ops::Bound::Excluded(cursor),
                    std::ops::Bound::Unbounded,
                ))
                .chain(connection.sessions.range(..=cursor));
            for &id in ordered {
                let response = self.sessions.get_mut(&id).and_then(|session| {
                    if progress {
                        session.take_progress_response()
                    } else {
                        session.outbox.pop_front()
                    }
                });
                if let Some(response) = response {
                    if progress {
                        connection.last_progress_session = id;
                    } else {
                        connection.last_output_session = id;
                    }
                    connection.progress_turn = !progress;
                    return Some(response);
                }
            }
        }
        None
    }

    /// Disconnect releases scoped state and only Worlds explicitly created as temporary.
    pub fn close_connection(&mut self, id: u64) -> bool {
        let Some(mut connection) = self.connections.states.remove(&id) else {
            return false;
        };
        #[cfg(feature = "instrumentation")]
        self.profiling.disconnect(id);

        std::mem::take(&mut connection.datasets).disconnect(&mut self.runtime);
        connection.outbox.close();
        if let Some(input) = self.services.gui_input() {
            input.close_connection(&mut self.runtime, id);
        }
        connection.reply_budget.0.close();
        self.cancel_world_transfer(&mut connection);
        self.presentation.disconnect(id);
        for session in connection.sessions {
            self.detach_world_session(session);
        }
        for world in connection.temporary_worlds {
            self.destroy_connected_world(world, None);
        }
        true
    }

    pub(crate) fn process_host_requests(&mut self) -> Vec<(u64, String)> {
        #[cfg(feature = "instrumentation")]
        self.profiling.cleanup(&mut self.services);
        self.expire_queued_presentations();
        if self.connections.states.values().all(|connection| {
            connection.pending.is_empty()
                && connection.failure.is_none()
                && matches!(
                    connection.reply_budget.0.status(),
                    ipp_core::services::reliable_output::OutputStatus::Open
                )
        }) {
            return Vec::new();
        }
        let mut ids: Vec<_> = self.connections.states.keys().copied().collect();
        let split = ids.partition_point(|id| *id <= self.connections.last_persistence_connection);
        ids.rotate_left(split);
        let mut expensive = false;
        let mut failures = Vec::new();
        for id in ids {
            let mut connection = self
                .connections
                .states
                .remove(&id)
                .expect("live connection");
            if let Some(error) = connection.failure.take() {
                failures.push((id, error));
            }
            if !matches!(
                connection.reply_budget.0.status(),
                ipp_core::services::reliable_output::OutputStatus::Open
            ) {
                failures.push((
                    id,
                    "connection congestion: reliable output account failed".into(),
                ));
                self.connections.states.insert(id, connection);
                continue;
            }
            let mut blocked = BTreeSet::new();
            let queued = connection.pending.len();
            for _ in 0..queued {
                let Some(ingress) = connection.pending.pop_front() else {
                    break;
                };
                let (request, received_at) = match ingress {
                    HostConnectionIngress::DecodedWorld {
                        request,
                        reservation,
                        lease,
                        rejection,
                    } => {
                        if blocked.contains(&request.session) {
                            connection
                                .pending
                                .push_back(HostConnectionIngress::DecodedWorld {
                                    request,
                                    reservation,
                                    lease,
                                    rejection,
                                });
                            continue;
                        }
                        if connection.sessions.contains(&request.session)
                            && let Some(mut session) = self.session_mut(request.session)
                            && let Err(error) =
                                session.receive_admitted(request, reservation, lease, rejection)
                        {
                            failures.push((id, error));
                            break;
                        }
                        continue;
                    }
                    HostConnectionIngress::Control {
                        request,
                        received_at,
                    } => (request, received_at),
                };
                let affected = self.control_sessions(&connection, &request.body);
                if affected.iter().any(|id| {
                    blocked.contains(id)
                        || self
                            .sessions
                            .get(id)
                            .is_some_and(|session| !session.pending.is_empty())
                }) {
                    blocked.extend(affected);
                    connection
                        .pending
                        .push_back(HostConnectionIngress::Control {
                            request,
                            received_at,
                        });
                    continue;
                }
                if matches!(
                    request.body,
                    HostRequestBody::SaveWorld { .. }
                        | HostRequestBody::FinishWorldLoad { .. }
                        | HostRequestBody::InspectWorldLoad { .. }
                ) {
                    if expensive {
                        connection
                            .pending
                            .push_back(HostConnectionIngress::Control {
                                request,
                                received_at,
                            });
                        blocked.extend(affected);
                        continue;
                    }
                    expensive = true;
                    self.connections.last_persistence_connection = id;
                }
                let capacity = self.reply_reservations_for_control(&connection, &request);
                if let Err(error) = capacity {
                    if let Err(error) =
                        connection.reply(request.request_id, HostResponseBody::Error(error))
                    {
                        failures.push((id, error));
                    }
                    continue;
                }
                if let HostRequestBody::GuiInput(body) = request.body {
                    let reservation = connection
                        .reply_reservations
                        .get(&request.request_id)
                        .expect("reserved Host request")
                        .clone();
                    let response = match self.services.gui_input() {
                        Some(input) => input.request(
                            &mut self.runtime,
                            id,
                            request.request_id,
                            body,
                            &connection.outbox,
                            connection.reply_budget.clone(),
                            reservation,
                        ),
                        None => Ok(Some(
                            ipp_protocol::gui_input::GuiPhysicalResponse::Rejected(
                                "Unsupported".into(),
                            ),
                        )),
                    };
                    match response {
                        Ok(None) => {
                            connection.reply_reservations.remove(&request.request_id);
                        }
                        response => {
                            let response = response
                                .unwrap_or_else(|error| {
                                    Some(ipp_protocol::gui_input::GuiPhysicalResponse::Rejected(
                                        error,
                                    ))
                                })
                                .expect("immediate physical response");
                            if let Err(error) = connection
                                .reply(request.request_id, HostResponseBody::GuiInput(response))
                            {
                                failures.push((id, error));
                            }
                        }
                    }
                    continue;
                }
                let body = if let HostRequestBody::Presentation(body) = request.body {
                    match self.presentation.request(
                        &mut self.runtime,
                        &mut self.services,
                        id,
                        request.request_id,
                        body,
                        received_at,
                    ) {
                        Some(response) => HostResponseBody::Presentation(response),
                        None => {
                            connection.presentation_pending += 1;
                            continue;
                        }
                    }
                } else {
                    self.apply_host_request(&mut connection, request.body)
                        .unwrap_or_else(HostResponseBody::Error)
                };
                if let Err(error) = connection.reply(request.request_id, body) {
                    failures.push((id, error));
                    break;
                }
            }
            self.connections.states.insert(id, connection);
        }
        failures
    }

    fn reply_reservations_for_control(
        &self,
        connection: &HostConnectionState,
        request: &HostRequest,
    ) -> Result<(), String> {
        if matches!(request.body, HostRequestBody::GuiInput(_)) {
            return connection
                .reply_reservations
                .get(&request.request_id)
                .ok_or("Missing physical reply reservation")?
                .borrow_mut()
                .reserve_bytes(512)
                .map_err(|error| error.to_string());
        }
        let bytes = if let HostRequestBody::Presentation(
            ipp_protocol::presentation::PresentationRequest::Frame {
                after_outputs,
                ..
            },
        ) = &request.body
        {
            1024 + after_outputs.len()
                * (std::mem::size_of::<ipp_protocol::presentation::PresentedSource>()
                    + std::mem::size_of::<ipp_core::OutputPublicationObservation>()
                    + std::mem::size_of::<(ipp_core::OutputRef, u64)>())
        } else {
            ipp_protocol::MAX_MESSAGE_BYTES
        };
        connection
            .reply_reservations
            .get(&request.request_id)
            .ok_or("Missing Host control reservation")?
            .borrow_mut()
            .reserve_bytes(bytes)
            .map_err(|error| format!("Host reply capacity unavailable: {error}"))
    }

    pub(super) fn apply_host_request(
        &mut self,
        connection: &mut HostConnectionState,
        request: HostRequestBody,
    ) -> Result<HostResponseBody, String> {
        match request {
            HostRequestBody::Profile(request) => Ok(HostResponseBody::Profile(
                self.profile_control(connection.id, request),
            )),
            HostRequestBody::GuiInput(_) => {
                Err("physical input request requires correlation".into())
            }
            HostRequestBody::Presentation(_) => {
                Err("presentation request requires correlation".into())
            }
            HostRequestBody::GetRootOutputBinding(reference) => {
                let world = reference
                    .resolve(&self.runtime)
                    .map_err(|error| error.to_string())?;
                let binding = self
                    .runtime
                    .root_output_binding(world)
                    .map_err(|error| error.to_string())?;
                Ok(HostResponseBody::RootBinding(binding.map(Into::into)))
            }
            HostRequestBody::ListWorlds {
                after,
            } => {
                let mut worlds: Vec<_> = self
                    .runtime
                    .list_worlds()
                    .into_iter()
                    .filter(|world| world.id.0 > after)
                    .take(33)
                    .collect();
                let next = if worlds.len() > 32 {
                    worlds.truncate(32);
                    worlds.last().expect("full page").id.0
                } else {
                    0
                };
                Ok(HostResponseBody::Worlds {
                    worlds,
                    next,
                })
            }
            HostRequestBody::CreateWorld {
                options,
                temporary,
            } => {
                let selected_systems = options
                    .selected_systems
                    .ok_or(WORLD_SELECTION_REQUIRED)?
                    .into_iter()
                    .map(|name| {
                        self.runtime
                            .system_ids()
                            .find(|system| system.0 == name)
                            .ok_or_else(|| format!("Unknown registered system: {name}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let id = self
                    .runtime
                    .create_world_with_options(
                        Default::default(),
                        ipp_core::WorldCreateOptions {
                            symbolic_id: options.symbolic_id,
                            capacity_hints: options.capacity_hints,
                            selected_systems,
                            canvas: options.canvas,
                        },
                    )
                    .map_err(|error| error.to_string())?;
                if temporary {
                    connection.temporary_worlds.insert(id);
                }
                Ok(HostResponseBody::Created {
                    world: self.world_descriptor(id)?,
                    reference: self.runtime.world_ref(id).ok_or("World is closed")?.into(),
                })
            }
            HostRequestBody::OpenWorld(reference) => {
                let world = reference
                    .resolve(&self.runtime)
                    .map_err(|error| error.to_string())?;
                self.attach_connection(connection, world.id())
            }
            HostRequestBody::ResolveWorld(selector) => {
                let id = self
                    .runtime
                    .resolve_world(&selector)
                    .ok_or("World does not exist")?;
                Ok(HostResponseBody::WorldReference(
                    self.runtime.world_ref(id).ok_or("World is closed")?.into(),
                ))
            }
            HostRequestBody::BindOutput {
                world,
                entity,
                kind,
            } => {
                let world = world
                    .resolve(&self.runtime)
                    .map_err(|error| error.to_string())?;
                let output = self
                    .runtime
                    .bind_output(world, ipp_core::EntityId::from_bits(entity), kind)
                    .map_err(|error| error.to_string())?;
                Ok(HostResponseBody::OutputReference(output.into()))
            }
            HostRequestBody::ResolveOutput(reference) => {
                let output = reference
                    .resolve(&self.runtime)
                    .map_err(|error| error.to_string())?;
                Ok(HostResponseBody::OutputReference(output.into()))
            }
            HostRequestBody::SetRootOutput {
                output,
                viewport,
            } => {
                let output = output
                    .resolve(&self.runtime)
                    .map_err(|error| error.to_string())?;
                self.runtime
                    .set_root_output(output, viewport)
                    .map_err(|error| error.to_string())?;
                Ok(HostResponseBody::RootBinding(
                    self.runtime
                        .root_output_binding(output.world())
                        .map_err(|error| error.to_string())?
                        .map(Into::into),
                ))
            }
            HostRequestBody::ClearRootOutput(expected) => {
                if let Ok(world) = expected.output.world.resolve(&self.runtime)
                    && self
                        .runtime
                        .root_output_binding(world)
                        .ok()
                        .flatten()
                        .map(ipp_protocol::presentation::RootBinding::from)
                        == Some(expected)
                {
                    self.runtime.clear_root_output(world.id());
                }
                Ok(HostResponseBody::Complete)
            }
            HostRequestBody::RenameWorld {
                world,
                symbolic_id,
            } => {
                let id = self
                    .runtime
                    .resolve_world(&world)
                    .ok_or("World does not exist")?;
                self.runtime.rename_world(id, symbolic_id)?;
                Ok(HostResponseBody::World(self.world_descriptor(id)?))
            }
            HostRequestBody::DestroyWorld(reference) => {
                let world = reference
                    .resolve(&self.runtime)
                    .map_err(|error| error.to_string())?;
                self.destroy_connected_world(world.id(), Some(connection));
                Ok(HostResponseBody::Complete)
            }
            HostRequestBody::DetachWorld {
                session,
            } => {
                Self::require_session(connection, session)?;
                if connection
                    .transfer
                    .as_ref()
                    .is_some_and(|transfer| transfer.origin == Some(session))
                {
                    self.connections
                        .persistence
                        .release(connection.transfer.take());
                }
                connection.end_session(session);
                self.detach_world_session(session);
                Ok(HostResponseBody::Complete)
            }
            HostRequestBody::SetCapacityHints {
                session,
                hints,
            } => {
                Self::require_session(connection, session)?;
                let id = self
                    .session_world(session)
                    .ok_or("World session is closed")?;
                let mut world = self.runtime.world_mut(id).ok_or("World does not exist")?;
                let hints = hints.apply(world.capacity_hints());
                world
                    .set_capacity_hints(hints)
                    .map_err(|error| error.to_string())?;
                drop(world);
                Ok(HostResponseBody::World(self.world_descriptor(id)?))
            }
            request => self.apply_persistence_request(connection, request),
        }
    }

    pub(super) fn require_session(
        connection: &HostConnectionState,
        session: u64,
    ) -> Result<(), String> {
        if connection.sessions.contains(&session) {
            Ok(())
        } else {
            Err("Unknown or foreign World session".into())
        }
    }

    pub(super) fn world_descriptor(&self, id: WorldId) -> Result<WorldDescriptor, String> {
        self.runtime
            .list_worlds()
            .into_iter()
            .find(|world| world.id == id)
            .ok_or_else(|| "World does not exist".into())
    }

    pub(super) fn attach_connection(
        &mut self,
        connection: &mut HostConnectionState,
        world: WorldId,
    ) -> Result<HostResponseBody, String> {
        let descriptor = self.world_descriptor(world)?;
        let manifest = host::WorldManifest::from_core(
            self.runtime
                .world_manifest(world)
                .ok_or("World manifest is unavailable")?,
        );
        let next = crate::next_ingress_id()?;
        if next >= 1 << 63 {
            return Err("World session identity space exhausted".into());
        }
        let session = (1 << 63) | next;
        self.sessions
            .insert(session, WorldSession::new(session, world, true, false));
        let state = self.sessions.get_mut(&session).expect("new session");
        state.reply_budget = connection.reply_budget.clone();
        state.connection_progress_leases = connection.progress_leases.clone();
        connection.sessions.insert(session);
        Ok(HostResponseBody::Attached {
            reference: self
                .runtime
                .world_ref(world)
                .ok_or("World is closed")?
                .into(),
            world: descriptor,
            session,
            manifest,
        })
    }

    pub(super) fn destroy_connected_world(
        &mut self,
        world: WorldId,
        current: Option<&mut HostConnectionState>,
    ) {
        let sessions: BTreeSet<_> = self
            .sessions
            .iter()
            .filter_map(|(&id, session)| (session.world == world).then_some(id))
            .collect();
        for session in &sessions {
            self.detach_world_session(*session);
        }
        let invalidate = |connection: &mut HostConnectionState| {
            connection.temporary_worlds.remove(&world);
            for session in connection
                .sessions
                .intersection(&sessions)
                .copied()
                .collect::<Vec<_>>()
            {
                connection.end_session(session);
                if let Err(error) = connection.reply(
                    0,
                    HostResponseBody::Detached {
                        session,
                        reason: "World was destroyed".into(),
                    },
                ) {
                    connection.failure = Some(error);
                }
            }
        };
        for connection in self.connections.states.values_mut() {
            if connection
                .transfer
                .as_ref()
                .and_then(|transfer| transfer.origin)
                .is_some_and(|session| sessions.contains(&session))
            {
                self.connections
                    .persistence
                    .release(connection.transfer.take());
            }
            invalidate(connection);
        }
        if let Some(connection) = current {
            if connection
                .transfer
                .as_ref()
                .and_then(|transfer| transfer.origin)
                .is_some_and(|session| sessions.contains(&session))
            {
                self.connections
                    .persistence
                    .release(connection.transfer.take());
            }
            invalidate(connection);
        }
        self.runtime.destroy_world(world);
    }
}

impl<P: HostServices> Host<P> {
    pub(crate) fn connection_for_session(&self, session: u64) -> u64 {
        self.connections
            .states
            .iter()
            .find_map(|(&id, connection)| connection.sessions.contains(&session).then_some(id))
            .unwrap_or(session)
    }

    fn control_sessions(
        &self,
        connection: &HostConnectionState,
        request: &HostRequestBody,
    ) -> BTreeSet<u64> {
        let world = match request {
            HostRequestBody::SaveWorld {
                session,
            }
            | HostRequestBody::SetCapacityHints {
                session,
                ..
            } => {
                return connection
                    .sessions
                    .contains(session)
                    .then_some(*session)
                    .into_iter()
                    .collect();
            }
            HostRequestBody::RenameWorld {
                world,
                ..
            } => self.runtime.resolve_world(world),
            HostRequestBody::BindOutput {
                world,
                ..
            }
            | HostRequestBody::GetRootOutputBinding(world) => Some(WorldId(world.id)),
            HostRequestBody::ClearRootOutput(binding) => Some(WorldId(binding.output.world.id)),
            HostRequestBody::Presentation(_) => None,
            HostRequestBody::GuiInput(_) => None,
            HostRequestBody::SetRootOutput {
                output,
                ..
            }
            | HostRequestBody::ResolveOutput(output) => Some(WorldId(output.world.id)),
            _ => None,
        };
        connection
            .sessions
            .iter()
            .copied()
            .filter(|id| world.is_some() && self.session_world(*id) == world)
            .collect()
    }
}
