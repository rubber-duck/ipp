//! Common output publication, input admission and severe-pressure revocation.

use super::*;
use crate::services::bulk_read::{BulkOutputAllocation, BulkReadPolicy};
use ipp_protocol::bulk_read::{self, BulkReadDescriptor, BulkReadOperation, BulkReadResponse};
use std::sync::Arc;

impl<P: HostServices> Host<P> {
    /// Observe retained output backing separately from physical delivery memory.
    pub fn bulk_read_usage(&self) -> crate::services::bulk_read::BulkReadUsage {
        crate::services::bulk_read::BulkReadUsage {
            backing_bytes: self.connections.bulk.retained(),
            leases: self
                .connections
                .states
                .values()
                .map(|state| state.bulk_reads.len())
                .sum(),
            delivery_bytes: self
                .connections
                .states
                .values()
                .map(|state| state.reply_budget.0.usage().bytes)
                .sum(),
        }
    }

    /// Configure a retained-output policy, independently of cache eviction.
    pub fn set_bulk_read_policy(&mut self, policy: BulkReadPolicy) {
        self.connections.bulk.set_policy(policy);
        self.maintain_bulk_pressure(0);
    }

    /// A platform observed actual severe memory pressure. Revoke retained unread
    /// leases; delivery allocations retain their physical completion credit.
    pub fn signal_severe_memory_pressure(&mut self) {
        self.revoke_bulk_leases(true, 0);
    }

    /// Reserve retained capacity before asynchronous encoding or GPU readback.
    pub fn reserve_bulk_output(&mut self, bytes: usize) -> Result<BulkOutputAllocation, String> {
        self.maintain_bulk_pressure(bytes);
        self.connections.bulk.reserve_output(bytes)
    }

    /// Transfer an operation's existing allocation charge into detached output.
    pub fn publish_connection_bytes_from_allocation(
        &mut self,
        connection: u64,
        bytes: Arc<Vec<u8>>,
        allocation: BulkOutputAllocation,
    ) -> Result<BulkReadDescriptor, String> {
        if !self.connections.states.contains_key(&connection) {
            return Err("Connection is closed".into());
        }
        let (source, length) = self.connections.bulk.adopt_output(bytes, allocation)?;
        self.publish_bulk_source(connection, source, Some(length))
    }

    /// Publish an already authorized immutable output to one connection.
    pub fn publish_connection_bytes(
        &mut self,
        connection: u64,
        bytes: Arc<Vec<u8>>,
    ) -> Result<BulkReadDescriptor, String> {
        if !self.connections.states.contains_key(&connection) {
            return Err("Connection is closed".into());
        }
        self.maintain_bulk_pressure(
            self.connections
                .bulk
                .additional_backing(&bytes)
                .saturating_add(2048 + 1280),
        );
        let (source, length) = self.connections.bulk.bytes(bytes)?;
        self.publish_bulk_source(connection, source, Some(length))
    }

    /// Publish an exact owned input without bounding its total encoded length.
    /// The reader must report retained_storage; unknown accounting is rejected.
    /// Changes during readiness polling are charged even while Pending. Providers
    /// changing storage asynchronously must notify the registered storage waker,
    /// including while the client has not requested a source window.
    pub fn publish_connection_reader(
        &mut self,
        connection: u64,
        reader: Box<dyn ipp_core::services::io::IoReader>,
        length: Option<u64>,
    ) -> Result<BulkReadDescriptor, String> {
        if !self.connections.states.contains_key(&connection) {
            return Err("Connection is closed".into());
        }
        let storage = reader
            .retained_storage()
            .ok_or("Bulk reader does not report owned storage accounting")?;
        self.maintain_bulk_pressure(
            self.connections
                .bulk
                .additional_storage(storage)
                .saturating_add(2048 + 1280),
        );
        let source = self.connections.bulk.reader(reader)?;
        self.publish_bulk_source(connection, source, length)
    }

    fn publish_bulk_source(
        &mut self,
        connection: u64,
        source: crate::services::bulk_read::Source,
        length: Option<u64>,
    ) -> Result<BulkReadDescriptor, String> {
        let state = self
            .connections
            .states
            .get_mut(&connection)
            .ok_or("Connection is closed")?;
        if self.connections.bulk.retained()
            > self.connections.bulk.policy.severe_retained_bytes.get()
        {
            return Err("Host severe retained-output pressure prevents publication".into());
        }
        self.connections.bulk.publish(
            source,
            length,
            crate::services::bulk_read::BulkReadDestination {
                connection,
                scheduler: self.scheduler.schedulers().host(),
                outbox: state.outbox.clone(),
                budget: state.reply_budget.clone(),
                leases: &mut state.bulk_reads,
            },
        )
    }

    fn bulk_pressure_bytes(&self) -> usize {
        let other = self
            .connections
            .persistence
            .reserved
            .saturating_add(
                self.presentation
                    .retained_output_bytes(&self.connections.bulk),
            )
            .saturating_add(
                self.connections
                    .states
                    .values()
                    .map(|state| state.reply_budget.0.usage().bytes)
                    .sum::<usize>(),
            );
        self.connections.bulk.observe_other_retained(other);
        self.connections.bulk.retained().saturating_add(other)
    }

    pub(crate) fn maintain_bulk_pressure(&mut self, additional: usize) {
        self.revoke_bulk_leases(false, additional);
    }

    fn revoke_bulk_leases(&mut self, all: bool, additional: usize) {
        while all
            || self.bulk_pressure_bytes().saturating_add(additional)
                > self.connections.bulk.policy.severe_retained_bytes.get()
        {
            let oldest = self
                .connections
                .states
                .iter()
                .flat_map(|(&connection, state)| {
                    state
                        .bulk_reads
                        .iter()
                        .map(move |(&read, lease)| (lease.order, connection, read))
                })
                .min();
            let Some((_, connection, read)) = oldest else {
                if self.connections.bulk.cancel_pending_output() {
                    continue;
                }
                break;
            };
            let state = self
                .connections
                .states
                .get_mut(&connection)
                .expect("live connection");
            if let Some(mut lease) = state.bulk_reads.remove(&read) {
                lease.notify_revocation("Bulk read revoked by severe Host memory pressure");
            }
        }
    }

    pub(super) fn receive_bulk_read(
        &mut self,
        connection: u64,
        bytes: &[u8],
    ) -> Result<(), String> {
        let request = bulk_read::decode(bytes, connection).map_err(|error| error.to_string())?;
        let state = self
            .connections
            .states
            .get_mut(&connection)
            .ok_or("Connection is closed")?;
        // Chunk payloads use the ordinary share. Small acknowledgement/release
        // replies may use reserved reply room, so retained lease metadata can
        // never prevent a client from abandoning output and restoring capacity.
        let reservation = if matches!(request.operation, BulkReadOperation::Read { .. }) {
            match ReplyReservation::new(
                state.reply_budget.clone(),
                OutputClass::Ordinary,
                bulk_read::FRAME_BYTES,
            ) {
                Ok(reservation) => Rc::new(RefCell::new(reservation)),
                Err(_) => {
                    let reservation = state.reserve_reply(256)?;
                    let bytes = bulk_read::response(
                        request.reference,
                        request.id,
                        BulkReadResponse::Error("Bulk response capacity exhausted"),
                    )
                    .map_err(|error| error.to_string())?;
                    reservation.borrow_mut().encoded(bytes.capacity());
                    state.outbox.push_back(ReliableResponse {
                        bytes,
                        reservation,
                    });
                    return Ok(());
                }
            }
        } else {
            state.reserve_reply(256)?
        };
        let result = if let Some(lease) = state.bulk_reads.get_mut(&request.reference.read) {
            match request.operation {
                BulkReadOperation::Read {
                    offset,
                } => match lease.read(request.id, offset, reservation.clone()) {
                    Ok(()) => return Ok(()),
                    Err(error) => Err(error),
                },
                BulkReadOperation::Acknowledge {
                    consumed,
                    eof,
                } => lease.acknowledge(consumed, eof).map(|release| {
                    if release {
                        state.bulk_reads.remove(&request.reference.read);
                    }
                }),
                BulkReadOperation::Release => {
                    if let Some(mut lease) = state.bulk_reads.remove(&request.reference.read) {
                        lease.revoke("Bulk read released");
                    }
                    Ok(())
                }
            }
        } else if matches!(request.operation, BulkReadOperation::Release) {
            Ok(())
        } else {
            Err("Bulk read is stale, unavailable or revoked".into())
        };
        let response = match &result {
            Ok(()) => BulkReadResponse::Complete,
            Err(error) => BulkReadResponse::Error(error),
        };
        let bytes = bulk_read::response(request.reference, request.id, response)
            .map_err(|error| error.to_string())?;
        reservation.borrow_mut().encoded(bytes.capacity());
        state.outbox.push_back(ReliableResponse {
            bytes,
            reservation,
        });
        Ok(())
    }
}
