use super::*;

impl<P: HostServices> Host<P> {
    /// Initialize Host services once, independently of world creation.
    pub fn new() -> Result<Self, String> {
        Self::with_system_factories(ipp_core::systems::compiled_system_factories())
    }

    /// Register reusable system factories before initializing platform services or Worlds.
    pub fn with_system_factories(
        factories: Vec<std::sync::Arc<dyn ipp_core::systems::SystemFactory>>,
    ) -> Result<Self, String> {
        let mut runtime = ipp_core::HostRuntime::with_system_factories(factories)
            .map_err(|error| error.to_string())?;
        let services = P::initialize(&mut runtime)?;
        Ok(Self {
            connections: Default::default(),
            sessions: Default::default(),
            runtime,
            services,
            frame_scratch: Default::default(),
            response_buffers: Vec::with_capacity(MAX_OUTBOX),
        })
    }

    /// Create a world and its initial logical connection. IDs are caller-fenced.
    pub fn open_session(&mut self, id: u64) -> Result<WorldId, String> {
        self.open_session_with_limits(id, Default::default())
    }

    /// Construct a protocol world under the Host's selected entity/ingress limits.
    pub fn open_session_with_limits(
        &mut self,
        id: u64,
        limits: ipp_core::WorldLimits,
    ) -> Result<WorldId, String> {
        if id == 0 || self.sessions.contains_key(&id) {
            return Err("session identity is zero or already live".into());
        }
        let world = self
            .runtime
            .create_world(limits)
            .map_err(|error| error.to_string())?;
        self.sessions
            .insert(id, WorldSession::new(id, world, false, true));
        Ok(world)
    }

    /// Borrow a logical session and the world selected by its Host routing record.
    pub fn session_mut(&mut self, id: u64) -> Option<WorldSessionContext<'_, P>> {
        if ipp_core::allocation_followup_enabled() {
            let slots = self.sessions.len().saturating_mul(2 * MAX_OUTBOX);
            self.response_buffers
                .reserve(slots.saturating_sub(self.response_buffers.len()));
        }
        let session = self.sessions.get_mut(&id)?;
        Some(WorldSessionContext {
            world: self.runtime.world_mut(session.world)?,
            session,
            services: &mut self.services,
            response_buffers: &mut self.response_buffers,
        })
    }

    /// Recycle storage only after all transport reads and copies have finished.
    pub fn recycle_response_buffer(&mut self, mut bytes: Vec<u8>) {
        bytes.clear();
        if bytes.capacity() != 0
            && ipp_core::allocation_followup_enabled()
            && self.response_buffers.len() < self.response_buffers.capacity()
        {
            self.response_buffers.push(bytes);
        }
    }

    /// Observe the world chosen for a logical session without accessing its state.
    pub fn session_world(&self, id: u64) -> Option<WorldId> {
        self.sessions.get(&id).map(|session| session.world)
    }

    /// The default connection policy destroys its private world on disconnect.
    pub fn close_session(&mut self, id: u64) -> bool {
        let Some(private) = self.sessions.get(&id).map(|session| session.private_world) else {
            return false;
        };
        if let Some(world) = self.detach_world_session(id)
            && private
        {
            self.runtime.destroy_world(world);
        }
        true
    }

    /// Run a Host frame, preserving independent progress when one session fails.
    pub fn tick_worlds(&mut self, dt: f64) -> Result<Vec<(u64, String)>, String> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(208, "host.tick");

        if !dt.is_finite() || dt < 0.0 {
            return Err("invalid host frame delta".into());
        }
        let mut failures = self.process_host_requests();
        failures.extend(self.process_command_batches());
        self.services.service_resources(&mut self.runtime)?;
        let reuse = ipp_core::allocation_optimizations_enabled();
        let mut scratch = std::mem::take(&mut self.frame_scratch);
        scratch.sessions.clear();
        scratch.prepared.clear();
        scratch.worlds.clear();
        scratch.evaluating.clear();
        scratch.sessions.extend(self.sessions.keys().copied());
        for &id in &scratch.sessions {
            if self.command_batch_owner(self.sessions[&id].world).is_some() {
                continue;
            }
            let result = self
                .session_mut(id)
                .expect("live session")
                .prepare_frame(dt);
            match result {
                Ok(()) => {
                    if self.sessions[&id].ready {
                        scratch.prepared.push(id);
                    }
                }
                Err(error) => failures.push((self.connection_for_session(id), error)),
            }
        }
        scratch.worlds.extend(self.runtime.world_ids());
        for &world in &scratch.worlds {
            if self.command_batch_owner(world).is_some() {
                continue;
            }
            if self
                .sessions
                .values()
                .any(|session| session.world == world && session.private_world && !session.ready)
            {
                continue;
            }
            let result = self
                .runtime
                .world_mut(world)
                .expect("live World")
                .prepare_update(dt);
            match result {
                Ok(()) => scratch.evaluating.push(world),
                Err(error) => {
                    let affected: Vec<_> = self
                        .sessions
                        .iter()
                        .filter(|(_, session)| session.world == world)
                        .map(|(&id, _)| id)
                        .collect();
                    for id in affected {
                        let connection = self.connection_for_session(id);
                        if let Err(error) = self
                            .session_mut(id)
                            .expect("live session")
                            .record_runtime_failure(
                                ipp_protocol::RuntimeFailureScope::World,
                                false,
                                error.to_string(),
                            )
                        {
                            failures.push((connection, error));
                        }
                    }
                }
            }
        }
        self.services.progress_assets(&mut self.runtime);
        for &world in &scratch.evaluating {
            let result = (|| {
                let mut world = self.runtime.world_mut(world).expect("live World");
                world.set_render_viewport(self.services.render_viewport());
                #[allow(unused_mut)]
                let mut report = world.step(dt).map_err(|error| error.to_string())?;
                let mut failure = if let Some(reason) = world.fault() {
                    Some((
                        ipp_protocol::RuntimeFailureScope::World,
                        true,
                        reason.to_string(),
                    ))
                } else {
                    self.services
                        .present(&mut world)
                        .err()
                        .map(|failure| (failure.scope, false, failure.message))
                };
                match world.take_asset_events() {
                    Ok(events) => report.resource_changes.extend(events),
                    Err(error) => {
                        failure = Some((ipp_protocol::RuntimeFailureScope::Resource, false, error))
                    }
                }
                report.assets.extend(world.take_asset_outcomes());
                Ok::<_, String>((report, failure))
            })();
            for &id in &scratch.prepared {
                if self.sessions[&id].world != world {
                    continue;
                }
                let result = match &result {
                    Ok((report, failure)) => {
                        let mut session = self.session_mut(id).expect("live session");
                        if let Some((scope, faulted, message)) = failure {
                            session
                                .record_runtime_failure(*scope, *faulted, message.clone())
                                .and_then(|()| session.publish_report(report))
                        } else {
                            session.session.last_failure = None;
                            session.publish_report(report)
                        }
                    }
                    Err(error) => self
                        .session_mut(id)
                        .expect("live session")
                        .record_runtime_failure(
                            ipp_protocol::RuntimeFailureScope::World,
                            false,
                            error.clone(),
                        ),
                };
                if let Err(error) = result {
                    failures.push((self.connection_for_session(id), error));
                }
            }
        }
        self.runtime.flush_resource_lifecycle();
        if reuse {
            self.frame_scratch = scratch;
        }
        Ok(failures)
    }

    /// Single-session embedding convenience; multi-world transports use `tick_worlds`.
    pub fn tick(&mut self, dt: f64) -> Result<(), String> {
        let failures = self.tick_worlds(dt)?;
        failures
            .into_iter()
            .next()
            .map_or(Ok(()), |(_, error)| Err(error))
    }

    /// Progress Host-owned provider and loader work without stepping any World.
    pub fn progress_resources(&mut self) -> Result<(), String> {
        self.service_resources()?;
        self.services.progress_resources(&mut self.runtime);
        // Loader progress can open dependent readers. Expose that provider work
        // before returning so an external pump does not need another World frame.
        self.service_resources()?;
        Ok(())
    }

    /// Expose Host-owned provider requests without polling shared loaders.
    pub fn service_resources(&mut self) -> Result<(), String> {
        self.services.service_resources(&mut self.runtime)?;
        Ok(())
    }

    /// Read Host resource state without borrowing any world.
    pub fn runtime(&self) -> &ipp_core::HostRuntime {
        &self.runtime
    }

    /// Split independent Host-owned facilities for platform lifecycle operations.
    pub fn parts_mut(&mut self) -> (&mut ipp_core::HostRuntime, &mut P) {
        (&mut self.runtime, &mut self.services)
    }

    /// Dequeue provider work from the Host service's own output queue.
    pub fn take_resource_request(&mut self) -> Option<Vec<u8>> {
        self.services.take_resource_request()
    }

    /// Host-owned core facilities, for lifecycle operations between updates.
    pub fn runtime_mut(&mut self) -> &mut ipp_core::HostRuntime {
        &mut self.runtime
    }

    /// Platform services share the Host lifetime.
    pub fn services_mut(&mut self) -> &mut P {
        &mut self.services
    }
}

#[cfg(test)]
impl<P: HostServices> Host<P> {
    pub(crate) fn test_session(&mut self) -> WorldSessionContext<'_, P> {
        assert_eq!(self.sessions.len(), 1);
        let id = *self.sessions.keys().next().unwrap();
        self.session_mut(id).unwrap()
    }

    pub(crate) fn test_limits(&mut self, limits: ipp_core::WorldLimits) {
        assert_eq!(self.test_session().world().tick(), 0);
        assert!(self.test_session().world().entities().is_empty());
        let id = *self.sessions.keys().next().unwrap();
        self.close_session(id);
        self.open_session_with_limits(id, limits).unwrap();
        self.test_session()
            .receive(&ipp_protocol::bootstrap())
            .unwrap();
        self.test_session().take_response().unwrap();
    }
}

impl<P: HostServices> Host<P> {
    pub(crate) fn detach_world_session(&mut self, id: u64) -> Option<WorldId> {
        let session = self.sessions.remove(&id)?;
        if session
            .command_batch
            .as_ref()
            .is_some_and(|batch| batch.started.is_some())
            && let Some(mut world) = self.runtime.world_mut(session.world)
        {
            world.finish_command_stream();
        }
        self.services.detach_world(session.world);
        if let Some(mut world) = self.runtime.world_mut(session.world) {
            world.release_system_session(session.id);
        }
        if let Some(mut world) = self.runtime.world_mut(session.world) {
            world.release_state_overlay_owners(session.owners);
        }
        if let Some(mut world) = self.runtime.world_mut(session.world) {
            for (source, _) in session
                .client_sources
                .into_iter()
                .filter(|(_, record)| record.active)
            {
                world
                    .asset_resources_mut()
                    .release_client_source(session.world, &source);
            }
        }
        Some(session.world)
    }
}
