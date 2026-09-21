//! Connection-owned transfers coordinated by the Host persistence service.

use super::{Host, HostConnectionState, HostServices};
use ipp_core::services::world_serialization::{WorldLoadOptions, WorldPersistenceLimits};
use ipp_protocol::host::{HostRequestBody, HostResponseBody};
use std::time::Duration;

const STAGING_BYTES: usize = 256 << 20;
const TRANSFER_INACTIVITY: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct HostPersistenceService {
    next_job: u64,
    pub(super) reserved: usize,
    limits: WorldPersistenceLimits,
}

pub(super) struct HostWorldTransfer {
    id: u64,
    reservation: usize,
    state: HostWorldTransferState,
    progress: Duration,
}

enum HostWorldTransferState {
    Ready {
        bytes: Vec<u8>,
        offset: usize,
    },
    Loading {
        expected: usize,
        bytes: Vec<u8>,
        options: WorldLoadOptions,
    },
}

impl HostPersistenceService {
    fn charge(&mut self, bytes: usize) -> Result<(), String> {
        if bytes > STAGING_BYTES.saturating_sub(self.reserved) {
            return Err("Host persistence staging budget exhausted".into());
        }
        self.reserved += bytes;
        Ok(())
    }

    fn reserve(&mut self, bytes: usize) -> Result<(u64, usize), String> {
        let id = self
            .next_job
            .checked_add(1)
            .ok_or("Transfer identity space exhausted")?;
        self.charge(bytes)?;
        self.next_job = id;
        Ok((id, bytes))
    }

    pub(super) fn release(&mut self, transfer: Option<HostWorldTransfer>) {
        if let Some(transfer) = transfer {
            self.reserved -= transfer.reservation;
        }
    }
}

impl<P: HostServices> Host<P> {
    pub(super) fn apply_persistence_request(
        &mut self,
        connection: &mut HostConnectionState,
        request: HostRequestBody,
    ) -> Result<HostResponseBody, String> {
        let result = self.run_persistence_request(connection, request);
        if result.is_err() {
            self.connections
                .persistence
                .release(connection.transfer.take());
        }
        result
    }

    fn run_persistence_request(
        &mut self,
        connection: &mut HostConnectionState,
        request: HostRequestBody,
    ) -> Result<HostResponseBody, String> {
        let now = self.connections.now;
        match request {
            HostRequestBody::SaveWorld => {
                if connection.transfer.is_some() {
                    return Err("A World transfer is already active on this connection".into());
                }
                let world = self
                    .session_world(connection.session.ok_or("Attach a World before saving")?)
                    .ok_or("World session is closed")?;
                let limits = self.connections.persistence.limits;
                let (id, reservation) =
                    self.connections.persistence.reserve(limits.max_bytes * 3)?;
                match self
                    .runtime
                    .save_world(world, ipp_protocol::schema_hash(), limits)
                {
                    Ok(bytes) => {
                        self.connections.persistence.reserved -= reservation - bytes.capacity();
                        let reservation = bytes.capacity();
                        connection.transfer = Some(HostWorldTransfer {
                            id,
                            reservation,
                            progress: now,
                            state: HostWorldTransferState::Ready {
                                bytes,
                                offset: 0,
                            },
                        });
                        Ok(HostResponseBody::Transfer {
                            job: id,
                        })
                    }
                    Err(error) => {
                        self.connections.persistence.reserved -= reservation;
                        Err(error)
                    }
                }
            }
            HostRequestBody::ReadWorldSave {
                job,
                offset,
            } => {
                let transfer = connection
                    .transfer
                    .as_mut()
                    .filter(|transfer| transfer.id == job)
                    .ok_or("Unknown World transfer")?;
                match &mut transfer.state {
                    HostWorldTransferState::Ready {
                        bytes,
                        offset: expected,
                    } => {
                        if offset != *expected as u64 {
                            return Err("World save offset mismatch".into());
                        }
                        let end = bytes.len().min(expected.saturating_add(64 << 10));
                        let response = HostResponseBody::SaveChunk {
                            job,
                            offset,
                            total: bytes.len() as u64,
                            bytes: bytes[*expected..end].to_vec(),
                        };
                        if end > *expected {
                            transfer.progress = now;
                        }
                        *expected = end;
                        if end == bytes.len() {
                            self.connections
                                .persistence
                                .release(connection.transfer.take());
                        }
                        Ok(response)
                    }
                    _ => Err("Transfer is a World load".into()),
                }
            }
            HostRequestBody::BeginWorldLoad {
                bytes,
                options,
            } => {
                Self::require_unattached(connection)?;
                if connection.transfer.is_some() {
                    return Err("A World transfer is already active on this connection".into());
                }
                let expected =
                    usize::try_from(bytes).map_err(|_| "World file size exceeds address space")?;
                if expected < 32 || expected > self.connections.persistence.limits.max_bytes {
                    return Err("World file byte budget exhausted".into());
                }
                let (id, reservation) = self.connections.persistence.reserve(0)?;
                let bytes = Vec::new();
                connection.transfer = Some(HostWorldTransfer {
                    id,
                    reservation,
                    progress: now,
                    state: HostWorldTransferState::Loading {
                        expected,
                        bytes,
                        options,
                    },
                });
                Ok(HostResponseBody::Transfer {
                    job: id,
                })
            }
            HostRequestBody::WriteWorldLoad {
                job,
                offset,
                bytes: chunk,
            } => {
                let transfer = connection
                    .transfer
                    .as_mut()
                    .filter(|transfer| transfer.id == job)
                    .ok_or("Unknown World transfer")?;
                let HostWorldTransferState::Loading {
                    expected,
                    bytes,
                    ..
                } = &mut transfer.state
                else {
                    return Err("Transfer is not a World load".into());
                };
                if offset != bytes.len() as u64
                    || chunk.is_empty()
                    || chunk.len() > expected.saturating_sub(bytes.len())
                {
                    return Err("World load chunk offset or length mismatch".into());
                }
                let required = bytes.len() + chunk.len();
                if required > bytes.capacity() {
                    let capacity = required
                        .max(bytes.capacity().saturating_mul(2))
                        .min(*expected);
                    let additional = capacity - bytes.capacity();
                    self.connections.persistence.charge(additional)?;
                    if let Err(error) = bytes.try_reserve_exact(capacity - bytes.len()) {
                        self.connections.persistence.reserved -= additional;
                        return Err(error.to_string());
                    }
                    transfer.reservation += additional;
                    debug_assert_eq!(transfer.reservation, bytes.capacity());
                }
                bytes.extend_from_slice(&chunk);
                transfer.progress = now;
                Ok(HostResponseBody::Complete)
            }
            HostRequestBody::FinishWorldLoad {
                job,
            } => {
                Self::require_unattached(connection)?;
                let transfer = connection
                    .transfer
                    .as_ref()
                    .filter(|transfer| transfer.id == job)
                    .ok_or("Unknown World transfer")?;
                let HostWorldTransferState::Loading {
                    expected,
                    bytes,
                    ..
                } = &transfer.state
                else {
                    return Err("Transfer is not a World load".into());
                };
                if bytes.len() != *expected {
                    return Err("World load is incomplete".into());
                }
                let scratch = self.connections.persistence.limits.max_bytes * 2;
                self.connections.persistence.charge(scratch)?;
                let transfer = connection.transfer.take().expect("validated transfer");
                let HostWorldTransferState::Loading {
                    bytes,
                    options,
                    ..
                } = transfer.state
                else {
                    unreachable!()
                };
                let result = self.runtime.load_world(
                    &bytes,
                    ipp_protocol::schema_hash(),
                    options,
                    Default::default(),
                    self.connections.persistence.limits,
                );
                self.connections.persistence.reserved -= transfer.reservation + scratch;
                let world = result?;
                match self.attach_connection(connection, world) {
                    Ok(response) => Ok(response),
                    Err(error) => {
                        self.runtime.destroy_world(world);
                        Err(error)
                    }
                }
            }
            HostRequestBody::CancelWorldTransfer {
                job,
            } => {
                if connection
                    .transfer
                    .as_ref()
                    .is_none_or(|transfer| transfer.id != job)
                {
                    return Err("Unknown World transfer".into());
                }
                self.connections
                    .persistence
                    .release(connection.transfer.take());
                Ok(HostResponseBody::Complete)
            }
            _ => unreachable!("ordinary Host control is handled by the connection service"),
        }
    }
}

impl<P: HostServices> Host<P> {
    /// Supply elapsed monotonic Host time independently of simulation advancement.
    /// Paused simulations still expire stalled transfers and congested connections.
    pub fn maintain_connections(&mut self, now: Duration) {
        self.connections.now = self.connections.now.max(now);
        self.expire_command_batches();
        for connection in self.connections.states.values_mut() {
            if connection.transfer.as_ref().is_some_and(|transfer| {
                self.connections.now.saturating_sub(transfer.progress) >= TRANSFER_INACTIVITY
            }) {
                self.connections
                    .persistence
                    .release(connection.transfer.take());
            }
        }
    }
}
