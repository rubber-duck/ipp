//! Connection lifecycle: opening, ingress admission and hysteresis, output order, maintenance
//! and closing.

use super::persistence::TRANSFER_INACTIVITY;
use super::*;
use std::time::Duration;

const CONGESTED: &str = "connection congestion: Host ingress capacity exhausted";

impl HostConnectionState {
    fn pending_requests(&self, sessions: &BTreeMap<u64, WorldSession>) -> usize {
        self.pending.len()
            + self.presentation_pending
            + self
                .sessions
                .iter()
                .filter_map(|id| sessions.get(id))
                .map(|session| session.pending.len())
                .sum::<usize>()
    }

    /// Whether a World message naming `session` is live work of this connection.
    ///
    /// A session the connection held and has ended answers `Ok(false)`: the message
    /// was sent before the client read that end, and the end already settled the
    /// session's work, so it is fenced without a reply and the connection stays
    /// usable. A session the connection never held fails the connection.
    pub(crate) fn holds_session(&self, session: u64) -> Result<bool, String> {
        if self.sessions.contains(&session) {
            Ok(true)
        } else if self.ended_sessions.contains(&session) {
            Ok(false)
        } else {
            Err("SessionMismatch".into())
        }
    }

    /// End a session of this connection; its later messages are stale.
    pub(super) fn end_session(&mut self, session: u64) {
        if self.sessions.remove(&session) {
            self.ended_sessions.insert(session);
        }
        self.batches.release_session(session);
    }

    /// Client-controlled request count: queued requests, or admitted correlated requests whose
    /// replies are not yet physically complete, whichever is larger. Uncorrelated output is
    /// bounded by bytes instead and never counts here.
    pub(crate) fn admitted_requests(&self, sessions: &BTreeMap<u64, WorldSession>) -> usize {
        self.pending_requests(sessions)
            .max(self.reply_budget.0.reply_usage().entries)
    }

    /// Correlated replies admitted and not yet physically delivered.
    #[cfg(test)]
    pub(crate) fn reply_entries(&self) -> usize {
        self.reply_budget.0.reply_usage().entries
    }

    /// Admit one correlated request's reply before accepting the request itself.
    pub(crate) fn reserve_reply(&self, bytes: usize) -> Result<SharedReplyReservation, String> {
        ReplyReservation::new(self.reply_budget.clone(), OutputClass::Reply, bytes)
            .map(|reservation| Rc::new(RefCell::new(reservation)))
            .map_err(|error| {
                let usage = self.reply_budget.0.usage();
                format!(
                    "connection congestion: reply capacity exhausted: {error} entries={} bytes={}",
                    usage.entries, usage.bytes
                )
            })
    }

    pub(in crate::services) fn reply(
        &mut self,
        request_id: u64,
        body: HostResponseBody,
    ) -> Result<(), String> {
        let response = HostResponse {
            connection: self.id,
            request_id,
            body,
        };
        let mut response = response;
        let size = host::encoded_host_response_size(&response)
            .or_else(|error| {
                response.body =
                    HostResponseBody::Error(format!("Host response unavailable: {error}"));
                host::encoded_host_response_size(&response)
            })
            .map_err(|error| error.to_string())?;
        let reservation = if request_id == 0 {
            Rc::new(RefCell::new(
                ReplyReservation::new(self.reply_budget.clone(), OutputClass::Ordinary, size)
                    .map_err(|error| {
                        format!("connection congestion: Host notice capacity exhausted: {error}")
                    })?,
            ))
        } else {
            self.reply_reservations
                .remove(&request_id)
                .ok_or("Host reply has no reserved output")?
        };
        reservation
            .borrow_mut()
            .reserve_bytes(size)
            .map_err(|error| error.to_string())?;
        let bytes = host::encode_host_response(&response).map_err(|error| error.to_string())?;
        reservation.borrow_mut().encoded(bytes.capacity());
        self.outbox.push_back(ReliableResponse {
            bytes,
            reservation,
        });
        Ok(())
    }
}

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
                bulk_reads: Default::default(),
                #[cfg(feature = "instrumentation")]
                bulk_test_inputs: Default::default(),
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
            ingress >= crate::MAX_PENDING / 2 || backlog != 0 && backlog >= room / 2
        } else {
            ingress >= crate::MAX_PENDING * 3 / 4 || backlog != 0 && backlog >= room * 3 / 4
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
            let reply = ipp_protocol::contract::accept_hello(bytes, id)
                .map_err(|error| error.to_string())?;
            reservation.borrow_mut().encoded(reply.capacity());
            connection.outbox.push_back(ReliableResponse {
                bytes: reply,
                reservation,
            });
            connection.ready = true;
            return Ok(());
        }
        #[cfg(feature = "instrumentation")]
        if bytes.starts_with(b"IPDT") {
            return self.receive_bulk_test(id, bytes);
        }
        if bytes.starts_with(ipp_protocol::bulk_read::REQUEST_MAGIC) {
            return self.receive_bulk_read(id, bytes);
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
        if bytes == ipp_protocol::contract::CONTRACT_REQUEST {
            // An admitted request whose reply is charged until physical delivery.
            if congested {
                return Err(CONGESTED.into());
            }
            let reservation = connection.reserve_reply(256)?;
            let _ = connection;
            let descriptor = self.publish_connection_bytes(id, {
                static BACKING: std::sync::OnceLock<std::sync::Arc<Vec<u8>>> =
                    std::sync::OnceLock::new();
                BACKING
                    .get_or_init(|| {
                        std::sync::Arc::new(ipp_protocol::contract::export_contract().to_vec())
                    })
                    .clone()
            })?;
            let reply = ipp_protocol::bulk_read::contract_descriptor(descriptor)
                .map_err(|error| error.to_string())?;
            reservation.borrow_mut().encoded(reply.capacity());
            self.connections
                .states
                .get(&id)
                .expect("live connection")
                .outbox
                .push_back(ReliableResponse {
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
                || self
                    .connections
                    .exports
                    .contains_pending_request(id, request.request_id)
            {
                return Err("duplicate outstanding Host request".into());
            }
            let ingress_bytes = match &request.body {
                HostRequestBody::Presentation(
                    ipp_protocol::host::presentation::PresentationRequest::Frame {
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
                body @ (ipp_protocol::host::profiling::ProfileRequest::Read {
                    ..
                }
                | ipp_protocol::host::profiling::ProfileRequest::Release(_)),
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
                    ipp_protocol::host::profiling::ProfileResponse::status(
                        ipp_protocol::host::profiling::ProfileStatus::Unavailable,
                    )
                };
                return connection.reply(request.request_id, HostResponseBody::Profile(response));
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
        self.close_asset_exports(id);
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

    /// Supply elapsed monotonic Host time independently of simulation advancement.
    /// Paused simulations still expire stalled transfers and congested connections.
    pub fn maintain_connections(&mut self, now: Duration) {
        self.connections.now = self.connections.now.max(now);
        self.maintain_bulk_pressure(0);
        self.expire_command_batches();
        self.expire_dataset_transfers();
        self.expire_queued_presentations();
        self.presentation.expire(self.connections.now);
        for (id, error) in self.publish_presentation_responses() {
            if let Some(connection) = self.connections.states.get_mut(&id) {
                connection.failure = Some(error);
            }
        }
        let expired: Vec<_> = self
            .connections
            .states
            .iter()
            .filter_map(|(&id, connection)| {
                connection
                    .transfer
                    .as_ref()
                    .is_some_and(|transfer| {
                        self.connections.now.saturating_sub(transfer.progress)
                            >= TRANSFER_INACTIVITY
                    })
                    .then_some(id)
            })
            .collect();
        for id in expired {
            let mut connection = self
                .connections
                .states
                .remove(&id)
                .expect("expired connection");
            self.cancel_world_transfer(&mut connection);
            self.connections.states.insert(id, connection);
        }
    }
}
