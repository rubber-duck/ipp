//! Host control requests and the World catalog: creating, opening, attaching and destroying
//! Worlds for a connection's sessions.

use super::*;

/// World creation has no default selection; a request must name its Systems.
pub(crate) const WORLD_SELECTION_REQUIRED: &str =
    "World creation requires a System selection: name the Systems the World uses";

impl<P: HostServices> Host<P> {
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
                if let HostRequestBody::AssetExport(body) = request.body {
                    match self.begin_asset_export(&mut connection, request.request_id, body) {
                        Ok(None) => {}
                        response => {
                            let body = response
                                .map(|response| {
                                    HostResponseBody::AssetExport(
                                        response.expect("immediate asset response"),
                                    )
                                })
                                .unwrap_or_else(HostResponseBody::Error);
                            if let Err(error) = connection.reply(request.request_id, body) {
                                failures.push((id, error));
                            }
                        }
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
                            ipp_protocol::host::gui_input::GuiPhysicalResponse::Rejected(
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
                                    Some(ipp_protocol::host::gui_input::GuiPhysicalResponse::Rejected(
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
            ipp_protocol::host::presentation::PresentationRequest::Frame {
                after_outputs,
                ..
            },
        ) = &request.body
        {
            1024 + after_outputs.len()
                * (std::mem::size_of::<ipp_protocol::host::presentation::PresentedSource>()
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
            HostRequestBody::AssetExport(_) => {
                Err("Asset exports require the async operation boundary".into())
            }
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
                        .map(ipp_protocol::host::presentation::RootBinding::from)
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
        let next = crate::session::ingress::next_ingress_id()?;
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
