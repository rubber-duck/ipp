use super::*;

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;

impl<P: HostServices> Host<P> {
    /// Adapter-owned optional profiling control at a quiescent Host boundary.
    /// A trusted adapter supplies a private owner ID; transport requests use
    /// the negotiated connection identity. Never call during frame evaluation.
    pub fn profile_control(
        &mut self,
        connection: u64,
        request: ipp_protocol::host::profiling::ProfileRequest,
    ) -> ipp_protocol::host::profiling::ProfileResponse {
        #[cfg(feature = "instrumentation")]
        {
            self.profiling.request(
                self.runtime.identity(),
                P::NAME,
                connection,
                request,
                &mut self.services,
            )
        }
        #[cfg(not(feature = "instrumentation"))]
        {
            let _ = (connection, request);
            ipp_protocol::host::profiling::ProfileResponse::status(
                ipp_protocol::host::profiling::ProfileStatus::Unavailable,
            )
        }
    }

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
        let scheduler = services::task_scheduler::TaskSchedulerService::new();
        let schedulers = scheduler.schedulers();
        runtime.set_asset_load_scheduler(std::rc::Rc::new(schedulers.host()));
        let services = P::initialize(&mut runtime, &schedulers)?;
        Ok(Self {
            #[cfg(feature = "instrumentation")]
            profiling: Default::default(),
            scheduler,
            connections: Default::default(),
            sessions: Default::default(),
            runtime,
            services,
            frame_scratch: Default::default(),
            presentation_time: 0.0,
            presentation: Default::default(),
        })
    }

    /// Create a world with exactly these Systems and its initial logical
    /// connection. IDs are caller-fenced.
    pub fn open_session(
        &mut self,
        id: u64,
        selected: &[ipp_core::systems::SystemId],
    ) -> Result<WorldId, String> {
        self.open_session_with_limits(id, Default::default(), selected)
    }

    /// Construct a protocol world with exactly these Systems under the Host's
    /// selected entity/ingress limits.
    pub fn open_session_with_limits(
        &mut self,
        id: u64,
        limits: ipp_core::WorldLimits,
        selected: &[ipp_core::systems::SystemId],
    ) -> Result<WorldId, String> {
        if id == 0 || self.sessions.contains_key(&id) {
            return Err("session identity is zero or already live".into());
        }
        let world = self
            .runtime
            .create_world(limits, selected)
            .map_err(|error| error.to_string())?;
        self.sessions
            .insert(id, WorldSession::new(id, world, false, true));
        Ok(world)
    }

    /// Borrow a logical session and the world selected by its Host routing record.
    pub fn session_mut(&mut self, id: u64) -> Option<WorldSessionContext<'_, P>> {
        let session = self.sessions.get_mut(&id)?;
        let world = self.runtime.world_mut(session.world)?;
        Some(WorldSessionContext {
            world,
            session,
            services: &mut self.services,
        })
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
        if !dt.is_finite() || dt < 0.0 {
            return Err("invalid host frame delta".into());
        }
        let presentation_time = self.presentation_time + dt;
        if !presentation_time.is_finite() {
            return Err("Host presentation time exhausted".into());
        }

        self.services.prepare_task_poll(&mut self.runtime)?;
        self.scheduler.poll_ready();
        self.progress_asset_exports();
        let mut failures = self.process_host_requests();
        #[cfg(feature = "instrumentation")]
        let _profile = self.runtime.profile_scope();
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(208, "host.tick");

        failures.extend(self.admit_session_requests(dt));
        self.services.service_resources(&mut self.runtime)?;
        let mut scratch = std::mem::take(&mut self.frame_scratch);
        scratch.sessions.clear();
        scratch.prepared.clear();
        scratch.sessions.extend(self.sessions.keys().copied());
        for &id in &scratch.sessions {
            if let Some(error) = self
                .sessions
                .get_mut(&id)
                .unwrap()
                .pending_errors
                .remove(&0)
            {
                failures.push((self.connection_for_session(id), error));
            } else if self.sessions[&id].ready {
                scratch.prepared.push(id);
            }
        }
        self.services.progress_assets(&mut self.runtime);
        let frame = self.runtime.frame(dt).map_err(|error| error.to_string())?;
        self.services.record_frame(&mut self.runtime, &frame);
        self.complete_view_queries(&frame);
        self.presentation_time = presentation_time;
        let mut presentation_failures = std::collections::BTreeMap::new();
        for failure in self.services.route_input(&mut self.runtime, &frame) {
            let world = failure.output.world();
            if self.runtime.world_ref(world.id()) == Some(world) {
                presentation_failures.insert(
                    world.id(),
                    HostPresentationFailure {
                        scope: ipp_protocol::world::RuntimeFailureScope::Context,
                        message: failure.message,
                    },
                );
            }
        }
        if let Some((world, error)) = self.presentation.draw(
            &mut self.runtime,
            &mut self.services,
            self.presentation_time,
            self.connections.now,
        ) {
            presentation_failures.insert(world, error);
        }
        failures.extend(self.publish_presentation_responses());
        for (world, result) in frame.worlds {
            let mut result = result.map(|mut report| {
                if let Some(mut context) = self.runtime.world_mut(world) {
                    match context.take_asset_events() {
                        Ok(events) => report.resource_changes.extend(events),
                        Err(message) => {
                            presentation_failures.insert(
                                world,
                                HostPresentationFailure {
                                    scope: ipp_protocol::world::RuntimeFailureScope::Resource,
                                    message,
                                },
                            );
                        }
                    }
                    report.assets.extend(context.take_asset_outcomes());
                }
                report
            });
            for &id in &scratch.prepared {
                if self.sessions[&id].world != world {
                    continue;
                }
                let result = match &mut result {
                    Ok(report) => {
                        let mut session = self.session_mut(id).expect("live session");
                        let failure = presentation_failures
                            .get(&world)
                            .map(|error| (error.scope, false, error.message.clone()))
                            .or_else(|| {
                                frame.publication_errors.get(&world).map(|error| {
                                    (
                                        ipp_protocol::world::RuntimeFailureScope::World,
                                        session.world.fault().is_some(),
                                        error.clone(),
                                    )
                                })
                            });
                        if let Some((scope, faulted, message)) = failure {
                            session
                                .record_runtime_failure(scope, faulted, message)
                                .and_then(|()| session.publish_report(report))
                        } else {
                            session.session.last_failure = None;
                            session.publish_report(report)
                        }
                    }
                    Err(error) => {
                        let mut session = self.session_mut(id).expect("live session");
                        session.fail_view_queries(*error).and_then(|()| {
                            session.record_runtime_failure(
                                ipp_protocol::world::RuntimeFailureScope::World,
                                session.world.fault().is_some(),
                                error.to_string(),
                            )
                        })
                    }
                };
                if let Err(error) = result {
                    let connection = self.connection_for_session(id);
                    let mut sessions = 0;
                    let mut progress = 0;
                    let mut queued_progress = 0;
                    let mut queued = 0;
                    let mut reserved = 0;
                    for (&peer, session) in &self.sessions {
                        if self.connection_for_session(peer) != connection {
                            continue;
                        }
                        sessions += 1;
                        progress += session.progress_leases.get();
                        queued_progress += session.outbox.progress_len();
                        queued += session.outbox.len();
                        reserved += session.reply_reservations.len();
                    }
                    let usage = self.sessions[&id].reply_budget.0.usage();
                    failures.push((
                        connection,
                        format!("{error} sessions={sessions} frame_leases={progress} frame_queued={queued_progress} world_queued={queued} world_reserved={reserved} physical_entries={} physical_bytes={}", usage.entries, usage.bytes),
                    ));
                }
            }
        }
        self.runtime.flush_resource_lifecycle();
        failures.extend(self.drain_lifecycle_watches());
        failures.extend(self.drain_gui_observations());
        self.frame_scratch = scratch;
        #[cfg(feature = "instrumentation")]
        self.profiling.completed_boundary();

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
        self.services.prepare_task_poll(&mut self.runtime)?;
        self.scheduler.poll_ready();
        self.progress_asset_exports();
        self.services.service_resources(&mut self.runtime)?;
        self.services.progress_resources(&mut self.runtime);
        // Loader progress can open dependent readers. Expose that provider work
        // before returning so an external pump does not need another World frame.
        self.services.service_resources(&mut self.runtime)?;
        self.progress_asset_exports();
        Ok(())
    }

    /// Expose Host-owned provider requests without polling shared loaders.
    pub fn service_resources(&mut self) -> Result<(), String> {
        self.services.prepare_task_poll(&mut self.runtime)?;
        self.scheduler.poll_ready();
        self.progress_asset_exports();
        self.services.service_resources(&mut self.runtime)?;
        self.progress_asset_exports();
        Ok(())
    }

    /// Scheduling contexts for owned service operations, independent of Worlds.
    pub fn task_schedulers(&self) -> services::task_scheduler::TaskSchedulers {
        self.scheduler.schedulers()
    }

    /// Request platform updates when tasks become ready; callbacks must only enqueue.
    pub fn set_task_wakeup(&mut self, wakeup: std::sync::Arc<dyn Fn() + Send + Sync>) {
        self.scheduler.set_wakeup(wakeup);
    }

    /// Cancel and drain asynchronous tasks before platform services are released.
    pub fn shutdown_tasks(&mut self) {
        // Platform preparation failure still releases logical task state; graphics
        // adapters fence destruction when their context is permanently unavailable.
        let _ = self.services.prepare_task_poll(&mut self.runtime);
        self.scheduler.shutdown();
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

impl<P: HostServices> Host<P> {
    pub(crate) fn detach_world_session(&mut self, id: u64) -> Option<WorldId> {
        let session = self.sessions.remove(&id)?;
        if let Some(watch) = &session.lifecycle_watch {
            watch.close();
        }
        session.outbox.close();
        if session.private_world {
            session.reply_budget.0.close();
        }
        if let Some(observations) = &session.gui_observations {
            observations.close();
        }
        session.receipts.borrow_mut().close();
        if let Some(mut world) = self.runtime.world_mut(session.world) {
            world.release_system_session(session.id);
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

impl<P: HostServices> Drop for Host<P> {
    fn drop(&mut self) {
        self.shutdown_tasks();
    }
}
