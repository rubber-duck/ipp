use super::*;

impl<'a, P: HostServices> WorldSessionContext<'a, P> {
    /// Decode and retain a complete request without accessing live ECS state.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<(), String> {
        if !self.session.ready {
            let reply =
                ipp_protocol::accept_bootstrap(bytes, self.session.id).map_err(|error| {
                    ipp_core::diagnostic!(
                        Warn,
                        "[IPP {}] session.reject session={} reason=bootstrap",
                        P::NAME,
                        self.session.id
                    );
                    error.to_string()
                })?;
            self.session.outbox.push_back(reply);
            self.session.ready = true;
            ipp_core::diagnostic!(
                Info,
                "[IPP {}] session.bootstrap session={}",
                P::NAME,
                self.session.id
            );
            return Ok(());
        }

        if self.session.pending.len() >= MAX_PENDING {
            return Err("connection congestion: ingress capacity exhausted".into());
        }
        if self.session.outbox.len() + self.session.request_origins.len() + EVENT_RESERVE
            >= MAX_OUTBOX
        {
            return Err("connection congestion: reliable output reservation exhausted".into());
        }
        let mut buffer = if ipp_core::allocation_optimizations_enabled() {
            self.world.take_command_buffer()
        } else {
            Vec::new()
        };
        let request = ipp_protocol::decode_request_with_buffer(bytes, self.session.id, &mut buffer);
        if ipp_core::allocation_optimizations_enabled() {
            self.world.recycle_command_buffer(buffer);
        }
        self.receive_decoded(request.map_err(|error| {
            ipp_core::diagnostic!(
                Warn,
                "[IPP {}] buffer.reject session={} reason=decode",
                P::NAME,
                self.session.id
            );
            error.to_string()
        })?)
    }

    /// Admit a decoded request after queued Host controls and check its session fence.
    pub(super) fn receive_decoded(&mut self, mut request: Request) -> Result<(), String> {
        if !self.session.ready || request.session != self.session.id {
            return Err("SessionMismatch".into());
        }
        if self.session.pending.len() + self.session.outbox.len() >= MAX_PENDING {
            return Err("session pending/output budget exhausted; drain responses".into());
        }
        if request.request_id != 0 {
            let original = request.request_id;
            let internal = next_ingress_id()?;
            let batch_id = match &mut request.body {
                RequestBody::Batch(batch) | RequestBody::BatchChunk(batch) => {
                    let original = batch.id;
                    batch.id = internal;
                    Some(original)
                }
                #[cfg(feature = "gui")]
                RequestBody::GuiCommands {
                    batch_id: Some(batch_id),
                    ..
                } => Some(*batch_id),
                _ => None,
            };
            self.session
                .request_origins
                .insert(internal, (original, batch_id));
            request.request_id = internal;
        }
        #[cfg(feature = "diagnostics")]
        if let RequestBody::Batch(batch) = &request.body {
            ipp_core::diagnostic!(
                Debug,
                "[IPP {}] buffer.receive session={} request={} batch={} operations={}",
                P::NAME,
                self.session.id,
                request.request_id,
                batch.id,
                batch.operations.len()
            );
        }
        if !is_command_batch_continuation(&request.body)
            && let Err(error) = services::connection::scope::scope_request(
                self.session,
                &self.world,
                &mut request.body,
            )
        {
            self.session
                .pending_errors
                .insert(request.request_id, error);
        }
        self.session.pending.push_back(request);
        #[cfg(feature = "diagnostics")]
        if let Some(Request {
            request_id,
            body: RequestBody::Batch(batch),
            ..
        }) = self.session.pending.back()
        {
            ipp_core::diagnostic!(
                Debug,
                "[IPP {}] buffer.queued session={} request={} batch={} operations={}",
                P::NAME,
                self.session.id,
                request_id,
                batch.id,
                batch.operations.len()
            );
        }
        Ok(())
    }

    /// Dequeue one encoded protocol response.
    pub fn take_response(&mut self) -> Option<Vec<u8>> {
        self.session.outbox.pop_front()
    }

    /// Return exclusive encoded storage after the consumer has finished with it.
    pub fn recycle_response_buffer(&mut self, mut bytes: Vec<u8>) {
        bytes.clear();
        if bytes.capacity() != 0
            && ipp_core::allocation_followup_enabled()
            && self.response_buffers.len() < self.response_buffers.capacity()
        {
            self.response_buffers.push(bytes);
        }
    }

    /// Whether bootstrap compatibility has been accepted.
    pub fn is_ready(&self) -> bool {
        self.session.ready
    }

    /// Read the session world for host-only observations.
    pub fn world(&self) -> &WorldContext<'a> {
        &self.world
    }

    /// Mutably access the world at a host-controlled safe boundary.
    pub fn world_mut(&mut self) -> &mut WorldContext<'a> {
        &mut self.world
    }

    /// Enqueue one ordered GUI input from a host-owned event loop.
    ///
    /// Session-fenced to this session and ordered through the existing
    /// `GuiInputSystem` ingress: routing, focus/capture bookkeeping and
    /// envelope application stay in core.
    /// Hosts translate platform pointer/keyboard/touch events here without
    /// touching scene state; unhandled input remains observable for ordinary
    /// scene controls without duplicate dispatch.
    #[cfg(feature = "gui")]
    pub fn enqueue_gui_input(&mut self, command: ipp_core::GuiInputCommand) -> Result<(), String> {
        self.world
            .enqueue_gui_input_command(self.session.id, command)
            .map_err(|error| error.to_string())
    }

    /// Mutably access services-owned presentation or provider state.
    pub fn services_mut(&mut self) -> &mut P {
        self.services
    }

    /// Borrow the world and services together for host lifecycle operations.
    pub fn parts_mut(&mut self) -> (&mut WorldContext<'a>, &mut P) {
        (&mut self.world, &mut self.services)
    }

    /// Dequeue one host provider request.
    pub fn take_resource_request(&mut self) -> Option<Vec<u8>> {
        self.services.take_resource_request()
    }

    /// Deliver one host provider result; rejected input fails only its resource.
    pub fn complete_resource(&mut self, id: u64, result: Result<Vec<u8>, String>) {
        if let Err(error) = self.world.complete_resource(id, result) {
            self.world
                .asset_input_end(id, Err(format!("Asset provider input rejected: {error}")));
        }
    }

    /// Feed one bounded source chunk.
    pub fn asset_input_chunk(&self, id: u64, bytes: &[u8]) -> Result<bool, String> {
        self.world.asset_input_chunk(id, bytes)
    }

    /// Finish one bounded source stream.
    pub fn asset_input_end(&self, id: u64, result: Result<(), String>) {
        self.world.asset_input_end(id, result);
    }
}

pub(super) fn is_command_batch_continuation(body: &RequestBody) -> bool {
    if matches!(body, RequestBody::BatchChunk(_)) {
        return true;
    }
    #[cfg(feature = "gui")]
    if matches!(
        body,
        RequestBody::GuiCommands {
            batch_id: Some(_),
            ..
        }
    ) {
        return true;
    }
    false
}

impl WorldSession {
    pub(super) fn new(id: u64, world: WorldId, ready: bool, private_world: bool) -> Self {
        Self {
            id,
            world,
            ready,
            private_world,
            command_batch: None,
            last_failure: None,
            pending: if ipp_core::allocation_optimizations_enabled() {
                VecDeque::with_capacity(MAX_PENDING)
            } else {
                VecDeque::new()
            },
            replies: if ipp_core::allocation_optimizations_enabled() {
                Vec::with_capacity(MAX_PENDING)
            } else {
                Vec::new()
            },
            prepared: false,
            outbox: VecDeque::new(),
            request_origins: Default::default(),
            pending_errors: Default::default(),
            client_sources: Default::default(),
            source_transfers: Default::default(),
            owners: Default::default(),
        }
    }
}

pub(super) fn next_ingress_id() -> Result<u64, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_update(
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
        |value| value.checked_add(1),
    )
    .map_err(|_| "Host ingress identity space exhausted".into())
}
