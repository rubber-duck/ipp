//! Connection-owned transfers coordinated by the Host persistence service.

use super::{Host, HostConnectionState, HostServices};
use ipp_core::services::world_serialization::{
    WorldGraphDescriptor, WorldGraphLoadResult, WorldGraphNodeId, WorldLoadOptions,
    WorldPersistenceLimits, inspect_world_graph,
};
use ipp_protocol::host::{
    HostRequestBody, HostResponseBody, MAX_GRAPH_BINDING_PAGE, MAX_GRAPH_METADATA_PAGE,
};
use std::{collections::BTreeMap, ops::Bound, time::Duration};

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
    pub(super) origin: Option<u64>,
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
        descriptor: Option<WorldGraphDescriptor>,
        preview_offset: usize,
        names: BTreeMap<WorldGraphNodeId, String>,
    },
    Loaded {
        result: WorldGraphLoadResult,
        offset: usize,
        after: Option<WorldGraphNodeId>,
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
        let job = match &request {
            HostRequestBody::ReadWorldSave {
                job,
                ..
            }
            | HostRequestBody::WriteWorldLoad {
                job,
                ..
            }
            | HostRequestBody::InspectWorldLoad {
                job,
                ..
            }
            | HostRequestBody::SetWorldLoadNames {
                job,
                ..
            }
            | HostRequestBody::FinishWorldLoad {
                job,
                ..
            }
            | HostRequestBody::ReadWorldLoadBindings {
                job,
                ..
            }
            | HostRequestBody::AcknowledgeWorldLoad {
                job,
            }
            | HostRequestBody::CancelWorldTransfer {
                job,
            } => Some(*job),
            _ => None,
        };
        let result = self.run_persistence_request(connection, request);
        if result.is_err()
            && connection
                .transfer
                .as_ref()
                .is_some_and(|transfer| Some(transfer.id) == job)
        {
            self.cancel_world_transfer(connection);
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
            HostRequestBody::SaveWorld {
                session,
            } => {
                Self::require_session(connection, session)?;
                if connection.transfer.is_some() {
                    return Err("A World transfer is already active on this connection".into());
                }
                let world = self
                    .session_world(session)
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
                            origin: Some(session),
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
                        Err(error.to_string())
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
            } => {
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
                    origin: None,
                    reservation,
                    progress: now,
                    state: HostWorldTransferState::Loading {
                        expected,
                        bytes,
                        descriptor: None,
                        preview_offset: 0,
                        names: BTreeMap::new(),
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
                }
                bytes.extend_from_slice(&chunk);
                transfer.progress = now;
                Ok(HostResponseBody::Complete)
            }
            HostRequestBody::InspectWorldLoad {
                job,
                offset,
            } => {
                let transfer = connection
                    .transfer
                    .as_mut()
                    .filter(|transfer| transfer.id == job)
                    .ok_or("Unknown World transfer")?;
                let HostWorldTransferState::Loading {
                    expected,
                    bytes,
                    descriptor,
                    preview_offset,
                    ..
                } = &mut transfer.state
                else {
                    return Err("Transfer is not a World load".into());
                };
                if bytes.len() != *expected || offset as usize != *preview_offset {
                    return Err("World preview offset or length mismatch".into());
                }
                if descriptor.is_none() {
                    let limits = self.connections.persistence.limits;
                    let scratch = limits.max_bytes * 2;
                    self.connections.persistence.charge(scratch)?;
                    let result = inspect_world_graph(bytes, ipp_protocol::schema_hash(), limits);
                    self.connections.persistence.reserved -= scratch;
                    let mut inspected = match result {
                        Ok(value) => value,
                        Err(error) => return Err(error.to_string()),
                    };
                    inspected.nodes.sort_unstable_by_key(|node| node.id);
                    u32::try_from(inspected.nodes.len())
                        .map_err(|_| "Graph node count exceeds wire identity space")?;
                    let retained = inspected.nodes.capacity()
                        * std::mem::size_of::<
                            ipp_core::services::world_serialization::WorldGraphNodeDescriptor,
                        >()
                        + inspected
                            .nodes
                            .iter()
                            .map(|node| node.metadata.symbolic_id.capacity())
                            .sum::<usize>();
                    self.connections.persistence.charge(retained)?;
                    transfer.reservation += retained;
                    *descriptor = Some(inspected);
                }
                let descriptor = descriptor.as_ref().expect("inspected transfer");
                let end = descriptor
                    .nodes
                    .len()
                    .min(preview_offset.saturating_add(MAX_GRAPH_METADATA_PAGE));
                let response = HostResponseBody::WorldGraphPage {
                    job,
                    root: descriptor.root,
                    total: descriptor.nodes.len() as u32,
                    offset,
                    nodes: descriptor.nodes[*preview_offset..end].to_vec(),
                };
                if end > *preview_offset {
                    transfer.progress = now;
                }
                *preview_offset = end;
                Ok(response)
            }
            HostRequestBody::SetWorldLoadNames {
                job,
                names: replacements,
            } => {
                let transfer = connection
                    .transfer
                    .as_mut()
                    .filter(|transfer| transfer.id == job)
                    .ok_or("Unknown World transfer")?;
                let HostWorldTransferState::Loading {
                    descriptor: Some(descriptor),
                    preview_offset,
                    names,
                    ..
                } = &mut transfer.state
                else {
                    return Err("World graph must be inspected before renaming".into());
                };
                if *preview_offset != descriptor.nodes.len() || replacements.is_empty() {
                    return Err("Graph preview must be complete and rename page nonempty".into());
                }
                for node in replacements.keys() {
                    if names.contains_key(node)
                        || descriptor
                            .nodes
                            .binary_search_by_key(node, |node| node.id)
                            .is_err()
                    {
                        return Err("Unknown or repeated graph node rename".into());
                    }
                }
                let retained = replacements
                    .values()
                    .map(|name| {
                        name.capacity()
                            + std::mem::size_of::<(WorldGraphNodeId, String)>()
                            + 8 * std::mem::size_of::<usize>()
                    })
                    .sum::<usize>();
                self.connections.persistence.charge(retained)?;
                transfer.reservation += retained;
                names.extend(replacements);
                transfer.progress = now;
                Ok(HostResponseBody::Complete)
            }
            HostRequestBody::FinishWorldLoad {
                job,
                symbolic_id,
                capacity_hints,
            } => {
                let transfer = connection
                    .transfer
                    .as_mut()
                    .filter(|transfer| transfer.id == job)
                    .ok_or("Unknown World transfer")?;
                let HostWorldTransferState::Loading {
                    bytes,
                    descriptor: Some(descriptor),
                    preview_offset,
                    names,
                    ..
                } = &mut transfer.state
                else {
                    return Err("World graph must be inspected before publication".into());
                };
                if *preview_offset != descriptor.nodes.len() {
                    return Err("World graph preview is incomplete".into());
                }
                let journal = descriptor
                    .nodes
                    .len()
                    .checked_mul(
                        std::mem::size_of::<(WorldGraphNodeId, ipp_core::WorldRef)>()
                            + 8 * std::mem::size_of::<usize>(),
                    )
                    .ok_or("Graph journal exceeds address space")?;
                let scratch = self.connections.persistence.limits.max_bytes * 2;
                self.connections.persistence.charge(
                    scratch
                        .checked_add(journal)
                        .ok_or("Graph journal budget overflow")?,
                )?;
                let result = self.runtime.load_world(
                    bytes,
                    ipp_protocol::schema_hash(),
                    WorldLoadOptions {
                        symbolic_id,
                        capacity_hints,
                        world_names: std::mem::take(names),
                    },
                    Default::default(),
                    self.connections.persistence.limits,
                );
                self.connections.persistence.reserved -= scratch;
                match result {
                    Ok(result) => {
                        let root = result.root.into();
                        let total = result.created.len() as u32;
                        transfer.state = HostWorldTransferState::Loaded {
                            result,
                            offset: 0,
                            after: None,
                        };
                        self.connections.persistence.reserved -= transfer.reservation;
                        transfer.reservation = journal;
                        transfer.progress = now;
                        Ok(HostResponseBody::WorldGraphLoaded {
                            job,
                            root,
                            total,
                        })
                    }
                    Err(error) => {
                        self.connections.persistence.reserved -= journal;
                        Err(error.to_string())
                    }
                }
            }
            HostRequestBody::ReadWorldLoadBindings {
                job,
                offset: requested,
            } => {
                let transfer = connection
                    .transfer
                    .as_mut()
                    .filter(|transfer| transfer.id == job)
                    .ok_or("Unknown World transfer")?;
                let HostWorldTransferState::Loaded {
                    result,
                    offset,
                    after,
                } = &mut transfer.state
                else {
                    return Err("World graph is not published".into());
                };
                if requested as usize != *offset {
                    return Err("Graph binding offset mismatch".into());
                }
                let bindings: Vec<_> = result
                    .created
                    .range((
                        after.map_or(Bound::Unbounded, Bound::Excluded),
                        Bound::Unbounded,
                    ))
                    .take(MAX_GRAPH_BINDING_PAGE)
                    .map(|(node, world)| (*node, (*world).into()))
                    .collect();
                if let Some((node, _)) = bindings.last() {
                    *after = Some(*node);
                    *offset += bindings.len();
                    transfer.progress = now;
                }
                Ok(HostResponseBody::WorldGraphBindings {
                    job,
                    offset: requested,
                    bindings,
                })
            }
            HostRequestBody::AcknowledgeWorldLoad {
                job,
            } => {
                let transfer = connection
                    .transfer
                    .as_ref()
                    .filter(|transfer| transfer.id == job)
                    .ok_or("Unknown World transfer")?;
                let HostWorldTransferState::Loaded {
                    result,
                    offset,
                    ..
                } = &transfer.state
                else {
                    return Err("World graph is not published".into());
                };
                if *offset != result.created.len() {
                    return Err("Created World journal delivery is incomplete".into());
                }
                self.connections
                    .persistence
                    .release(connection.transfer.take());
                Ok(HostResponseBody::Complete)
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
                self.cancel_world_transfer(connection);
                Ok(HostResponseBody::Complete)
            }
            _ => unreachable!("ordinary Host control is handled by the connection service"),
        }
    }
}

impl<P: HostServices> Host<P> {
    pub(super) fn cancel_world_transfer(&mut self, connection: &mut HostConnectionState) {
        let Some(transfer) = connection.transfer.take() else {
            return;
        };
        if let HostWorldTransferState::Loaded {
            result,
            ..
        } = &transfer.state
        {
            for world in result.created.values() {
                if self.runtime.world_ref(world.id()) == Some(*world) {
                    self.destroy_connected_world(world.id(), Some(connection));
                }
            }
        }
        self.connections.persistence.release(Some(transfer));
    }

    /// Supply elapsed monotonic Host time independently of simulation advancement.
    /// Paused simulations still expire stalled transfers and congested connections.
    pub fn maintain_connections(&mut self, now: Duration) {
        self.connections.now = self.connections.now.max(now);
        self.expire_command_batches();
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
