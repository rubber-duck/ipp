use super::*;
use ipp_core::services::reliable_output::OutputClass;

impl<'a, P: HostServices> WorldSessionContext<'a, P> {
    /// Decode and retain a complete request without accessing live ECS state.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<(), String> {
        if !self.session.ready {
            let mut reservation = attachment_receipts::ReplyReservation::new(
                self.session.reply_budget.clone(),
                OutputClass::Reply,
                ipp_protocol::MAX_MESSAGE_BYTES,
            )
            .map_err(|error| error.to_string())?;
            let reply = ipp_protocol::host::accept_world_bootstrap(
                bytes,
                self.session.id,
                self.world.manifest(),
            )
            .map_err(|error| {
                ipp_core::diagnostic!(
                    Warn,
                    "[IPP {}] session.reject session={} reason=bootstrap",
                    P::NAME,
                    self.session.id
                );
                error.to_string()
            })?;
            reservation.encoded(reply.capacity());
            self.session.outbox.push_back(QueuedResponse {
                bytes: reply,
                reservation: std::rc::Rc::new(std::cell::RefCell::new(reservation)),
            });
            self.session.ready = true;
            ipp_core::diagnostic!(
                Info,
                "[IPP {}] session.bootstrap session={}",
                P::NAME,
                self.session.id
            );
            return Ok(());
        }

        // Mirrors connection admission: queued requests and unfinished replies are counted.
        if self.session.pending.len() >= MAX_PENDING
            || self.session.reply_budget.0.reply_usage().entries >= MAX_PENDING
        {
            return Err("connection congestion: ingress capacity exhausted".into());
        }
        let mut buffer = self.world.take_command_buffer();
        let request = ipp_protocol::decode_request_with_buffer(bytes, self.session.id, &mut buffer);
        self.world.recycle_command_buffer(buffer);
        self.receive_decoded(
            request.map_err(|error| {
                ipp_core::diagnostic!(
                    Warn,
                    "[IPP {}] buffer.reject session={} reason=decode",
                    P::NAME,
                    self.session.id
                );
                error.to_string()
            })?,
            None,
        )
    }

    /// Admit a decoded request after queued Host controls and check its session fence.
    ///
    /// Only a Host connection assembles paged batches; a session receiving
    /// requests directly accepts complete single-page batches.
    pub(super) fn receive_decoded(
        &mut self,
        request: Request,
        reservation: Option<attachment_receipts::SharedReplyReservation>,
    ) -> Result<(), String> {
        if let RequestBody::SubmitBatch(ipp_protocol::BatchPage {
            last: false,
            ..
        }) = request.body
        {
            return Err("Paged batches require a Host connection".into());
        }
        self.receive_admitted(request, reservation, None, None)
    }

    /// Queue a request, or a batch assembled by its connection, in session order.
    ///
    /// `lease` keeps an assembled batch's page bytes charged to its connection until
    /// the batch leaves the queue; `rejection` answers the batch with its first failure.
    pub(super) fn receive_admitted(
        &mut self,
        mut request: Request,
        reservation: Option<attachment_receipts::SharedReplyReservation>,
        lease: Option<command_batches::BatchBytesLease>,
        rejection: Option<String>,
    ) -> Result<(), String> {
        if !self.session.ready || request.session != self.session.id {
            return Err("SessionMismatch".into());
        }
        if self.session.request_origins.len() >= MAX_PENDING {
            return Err("session request budget exhausted; await replies".into());
        }
        {
            let original = request.request_id;
            let internal = next_ingress_id()?;
            let batch_id = match &request.body {
                RequestBody::SubmitBatch(page) => Some(u64::from(page.batch_id)),
                _ => None,
            };
            if original != 0 {
                let reservation = match reservation {
                    Some(reservation) => reservation,
                    None => std::rc::Rc::new(std::cell::RefCell::new(
                        attachment_receipts::ReplyReservation::new(
                            self.session.reply_budget.clone(),
                            OutputClass::Reply,
                            256,
                        )
                        .map_err(|error| error.to_string())?,
                    )),
                };
                self.session
                    .reply_reservations
                    .insert(internal, reservation);
            }
            self.session
                .request_origins
                .insert(internal, (original, batch_id));
            request.request_id = internal;
        }
        // Batch diagnostics name the core batch identity, which is the internal
        // request identity, so Host and core lines of one batch correlate; the
        // client's reusable page identity follows as `client_batch`.
        #[cfg(feature = "diagnostics")]
        if let RequestBody::SubmitBatch(page) = &request.body {
            ipp_core::diagnostic!(
                Debug,
                "[IPP {}] buffer.receive session={} request={} batch={} operations={} client_batch={}",
                P::NAME,
                self.session.id,
                request.request_id,
                request.request_id,
                page.operations.len(),
                page.batch_id
            );
        }
        let rejection = rejection.map_or_else(
            || services::connection::scope::scope_request(&self.world, &mut request.body).err(),
            Some,
        );
        if let Some(error) = rejection {
            self.session
                .pending_errors
                .insert(request.request_id, error);
        }
        if let Some(lease) = lease {
            self.session.batch_leases.insert(request.request_id, lease);
        }
        self.session.pending.push_back(request);
        #[cfg(feature = "diagnostics")]
        if let Some(Request {
            request_id,
            body: RequestBody::SubmitBatch(page),
            ..
        }) = self.session.pending.back()
        {
            ipp_core::diagnostic!(
                Debug,
                "[IPP {}] buffer.queued session={} request={} batch={} operations={} client_batch={}",
                P::NAME,
                self.session.id,
                request_id,
                request_id,
                page.operations.len(),
                page.batch_id
            );
        }
        Ok(())
    }

    /// Dequeue one encoded protocol response.
    pub fn take_response(&mut self) -> Option<ReliableResponse> {
        self.session.take_response()
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

impl WorldSession {
    pub(super) fn new(id: u64, world: WorldId, ready: bool, private_world: bool) -> Self {
        Self {
            id,
            world,
            ready,
            private_world,
            batch_leases: Default::default(),
            last_failure: None,
            pending: VecDeque::with_capacity(MAX_PENDING),
            replies: Vec::with_capacity(MAX_PENDING),
            prepared: false,
            outbox: Default::default(),
            progress_leases: Default::default(),
            connection_progress_leases: Default::default(),
            progress: None,
            receipts: Default::default(),
            lifecycle_watch: None,
            #[cfg(feature = "gui")]
            gui_observations: None,
            reply_budget: Default::default(),
            reply_reservations: Default::default(),
            request_origins: Default::default(),
            pending_errors: Default::default(),
            client_sources: Default::default(),
            source_transfers: Default::default(),
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
