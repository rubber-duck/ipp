use super::*;

impl<P: HostServices> Host<P> {
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
                session: None,
                temporary_worlds: BTreeSet::new(),
                pending: if ipp_core::allocation_optimizations_enabled() {
                    VecDeque::with_capacity(crate::MAX_PENDING)
                } else {
                    VecDeque::new()
                },
                outbox: VecDeque::new(),
                failure: None,
                transfer: None,
            },
        );
        Ok(())
    }

    /// Per-connection admission hysteresis, independent of other senders.
    pub fn connection_accepts_input(&mut self, id: u64) -> bool {
        let Some(connection) = self.connections.states.get_mut(&id) else {
            return false;
        };
        let session = connection.session.and_then(|id| self.sessions.get(&id));
        let ingress = connection.pending.len() + session.map_or(0, |session| session.pending.len());
        let output = connection.outbox.len()
            + session.map_or(0, |session| {
                session.outbox.len() + session.request_origins.len()
            });
        if connection.throttled {
            connection.throttled =
                ingress >= crate::MAX_PENDING / 2 || output >= crate::MAX_OUTBOX / 2;
        } else {
            connection.throttled =
                ingress >= crate::MAX_PENDING * 3 / 4 || output >= crate::MAX_OUTBOX * 3 / 4;
        }
        !connection.throttled
    }

    /// Validate transport input and enqueue it without advancing simulation time.
    pub fn receive_connection(&mut self, id: u64, bytes: &[u8]) -> Result<(), String> {
        let connection = self
            .connections
            .states
            .get_mut(&id)
            .ok_or("Connection is closed")?;
        if !connection.ready {
            let reply =
                ipp_protocol::accept_bootstrap(bytes, id).map_err(|error| error.to_string())?;
            connection.outbox.push_back(reply);
            connection.ready = true;
            return Ok(());
        }
        if bytes.starts_with(ipp_protocol::asset_source::REQUEST_MAGIC) {
            return self.receive_asset_source(id, bytes);
        }
        if connection.pending.len() >= crate::MAX_PENDING
            || !connection.can_reserve_reply(&self.sessions)
            || bytes.len() > ipp_protocol::MAX_MESSAGE_BYTES
        {
            return Err("connection congestion: Host ingress capacity exhausted".into());
        }
        if bytes.starts_with(host::HOST_REQUEST_MAGIC) {
            let request =
                host::decode_host_request(bytes, id).map_err(|error| error.to_string())?;
            connection
                .pending
                .push_back(HostConnectionIngress::Control(request));
        } else {
            let session = connection
                .session
                .ok_or("Connection is not attached to a World")?;
            if bytes.get(..8) != Some(session.to_le_bytes().as_slice()) {
                return Err("SessionMismatch".into());
            }
            if ipp_core::allocation_optimizations_enabled() {
                let world_id = self
                    .sessions
                    .get(&session)
                    .ok_or("Session is closed")?
                    .world;
                let mut world = self.runtime.world_mut(world_id).ok_or("World is closed")?;
                let mut buffer = world.take_command_buffer();
                let request = ipp_protocol::decode_request_with_buffer(bytes, session, &mut buffer);
                world.recycle_command_buffer(buffer);
                connection
                    .pending
                    .push_back(HostConnectionIngress::DecodedWorld(
                        request.map_err(|error| error.to_string())?,
                    ));
                return Ok(());
            }
            ipp_protocol::decode_request(bytes, session).map_err(|error| error.to_string())?;
            connection
                .pending
                .push_back(HostConnectionIngress::World(bytes.to_vec()));
        }
        Ok(())
    }

    /// Host control replies precede World observations; attachment arrives before its frames.
    pub fn take_connection_response(&mut self, id: u64) -> Option<Vec<u8>> {
        let connection = self.connections.states.get_mut(&id)?;
        if let Some(response) = connection.outbox.pop_front() {
            return Some(response);
        }
        let session = connection.session?;
        self.sessions.get_mut(&session)?.outbox.pop_front()
    }

    /// Observe the selected World without changing attachment state.
    pub fn connection_world(&self, id: u64) -> Option<WorldId> {
        let session = self.connections.states.get(&id)?.session?;
        self.session_world(session)
    }

    /// Borrow the attached session for platform presentation/data-plane operations.
    pub fn connection_session_mut(&mut self, id: u64) -> Option<crate::WorldSessionContext<'_, P>> {
        let session = self.connections.states.get(&id)?.session?;
        self.session_mut(session)
    }

    /// Disconnect releases scoped state and only Worlds explicitly created as temporary.
    pub fn close_connection(&mut self, id: u64) -> bool {
        let Some(mut connection) = self.connections.states.remove(&id) else {
            return false;
        };
        self.connections
            .persistence
            .release(connection.transfer.take());
        if let Some(session) = connection.session.take() {
            self.detach_world_session(session);
        }
        for world in connection.temporary_worlds {
            self.destroy_connected_world(world, None);
        }
        true
    }

    pub(crate) fn process_host_requests(&mut self) -> Vec<(u64, String)> {
        if ipp_core::allocation_optimizations_enabled()
            && self
                .connections
                .states
                .values()
                .all(|connection| connection.pending.is_empty() && connection.failure.is_none())
        {
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
            while let Some(ingress) = connection.pending.pop_front() {
                let request = match ingress {
                    HostConnectionIngress::DecodedWorld(request) => {
                        if let Some(mut session) = connection
                            .session
                            .and_then(|session| self.session_mut(session))
                            && let Err(error) = session.receive_decoded(request)
                        {
                            failures.push((id, error));
                            break;
                        }
                        continue;
                    }
                    HostConnectionIngress::World(bytes) => {
                        if let Some(mut session) = connection
                            .session
                            .and_then(|session| self.session_mut(session))
                            && let Err(error) = session.receive(&bytes)
                        {
                            failures.push((id, error));
                            break;
                        }
                        continue;
                    }
                    HostConnectionIngress::Control(request) => request,
                };
                // Prior World work commits in this frame before a subsequent Host
                // capture/lifecycle barrier. Later ingress stays ordered behind it.
                if connection
                    .session
                    .and_then(|session| self.sessions.get(&session))
                    .is_some_and(|session| {
                        !session.pending.is_empty()
                            || self.command_batch_owner(session.world).is_some()
                    })
                {
                    connection
                        .pending
                        .push_front(HostConnectionIngress::Control(request));
                    // A same-session Host barrier must not strand the buffers
                    // needed to finish the batch that is holding that barrier.
                    if connection.session.is_some_and(|id| {
                        self.sessions.get(&id).is_some_and(|session| {
                            self.command_batch_owner(session.world) == Some(id)
                        })
                    }) && let Some(position) = connection.pending.iter().position(|ingress| {
                        matches!(
                            ingress,
                            HostConnectionIngress::World(_)
                                | HostConnectionIngress::DecodedWorld(_)
                        )
                    }) {
                        let ingress = connection
                            .pending
                            .remove(position)
                            .expect("queued World input");
                        connection.pending.push_front(ingress);
                        continue;
                    }
                    break;
                }
                if matches!(
                    request.body,
                    HostRequestBody::SaveWorld | HostRequestBody::FinishWorldLoad { .. }
                ) {
                    if expensive {
                        connection
                            .pending
                            .push_front(HostConnectionIngress::Control(request));
                        break;
                    }
                    expensive = true;
                    self.connections.last_persistence_connection = id;
                }
                let body = self
                    .apply_host_request(&mut connection, request.body)
                    .unwrap_or_else(HostResponseBody::Error);
                if let Err(error) = connection.reply(request.request_id, body) {
                    failures.push((id, error));
                    break;
                }
            }
            self.connections.states.insert(id, connection);
        }
        failures
    }

    pub(super) fn apply_host_request(
        &mut self,
        connection: &mut HostConnectionState,
        request: HostRequestBody,
    ) -> Result<HostResponseBody, String> {
        match request {
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
                Self::require_unattached(connection)?;
                let id = self
                    .runtime
                    .create_world_with_options(Default::default(), options)
                    .map_err(|error| error.to_string())?;
                match self.attach_connection(connection, id) {
                    Ok(response) => {
                        if temporary {
                            connection.temporary_worlds.insert(id);
                        }
                        Ok(response)
                    }
                    Err(error) => {
                        self.runtime.destroy_world(id);
                        Err(error)
                    }
                }
            }
            HostRequestBody::AttachWorld(selector) => {
                Self::require_unattached(connection)?;
                let id = self
                    .runtime
                    .resolve_world(&selector)
                    .ok_or("World does not exist")?;
                self.attach_connection(connection, id)
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
            HostRequestBody::DestroyWorld(selector) => {
                let id = self
                    .runtime
                    .resolve_world(&selector)
                    .ok_or("World does not exist")?;
                self.destroy_connected_world(id, Some(connection));
                Ok(HostResponseBody::Complete)
            }
            HostRequestBody::DetachWorld => {
                self.connections
                    .persistence
                    .release(connection.transfer.take());
                if let Some(session) = connection.session.take() {
                    self.detach_world_session(session);
                }
                Ok(HostResponseBody::Complete)
            }
            HostRequestBody::SetCapacityHints(hints) => {
                let id = self
                    .session_world(
                        connection
                            .session
                            .ok_or("Connection is not attached to a World")?,
                    )
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

    pub(super) fn require_unattached(connection: &HostConnectionState) -> Result<(), String> {
        if connection.session.is_some() {
            Err("Detach the current World session before attaching another World".into())
        } else {
            Ok(())
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
        Self::require_unattached(connection)?;
        let descriptor = self.world_descriptor(world)?;
        let next = crate::next_ingress_id()?;
        if next >= 1 << 63 {
            return Err("World session identity space exhausted".into());
        }
        let session = (1 << 63) | next;
        self.services.attach_world(world)?;
        self.sessions
            .insert(session, WorldSession::new(session, world, true, false));
        connection.session = Some(session);
        Ok(HostResponseBody::Attached {
            world: descriptor,
            session,
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
            if connection
                .session
                .is_some_and(|session| sessions.contains(&session))
            {
                let session = connection.session.take().expect("matching session");
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
                .session
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
                .session
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
            .find_map(|(&id, connection)| (connection.session == Some(session)).then_some(id))
            .unwrap_or(session)
    }
}
