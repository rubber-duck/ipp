//! Host connection control, separate from World-scoped authoring envelopes.

use crate::wire::*;

use crate::{
    MAX_MESSAGE_BYTES, ProtocolError,
    codec::{Reader, Writer},
};
use ipp_core::{
    WorldCapacityHints, WorldCreateOptions, WorldDescriptor, WorldId, WorldMetadata,
    WorldPersistentId, WorldSelector, WorldSystemCapacityHints,
};
use std::collections::BTreeMap;

/// Host request marker and independent control format revision.
pub const HOST_REQUEST_MAGIC: &[u8; 8] = &host_magic(HOST_REQUEST_MAGIC_HEX);
/// Host response marker; World replies keep their existing fenced envelope.
pub const HOST_RESPONSE_MAGIC: &[u8; 8] = &host_magic(HOST_RESPONSE_MAGIC_HEX);

/// One correlated operation addressed to the physical Host connection.
#[derive(Clone, Debug, PartialEq)]
pub struct HostRequest {
    /// Negotiated Host connection identity.
    pub connection: u64,
    /// Nonzero correlation unique while this connection is live.
    pub request_id: u64,
    /// Host lifecycle or persistence operation.
    pub body: HostRequestBody,
}

/// Host-visible discovery and lifecycle. World authoring remains in ordinary requests.
#[derive(Clone, Debug, PartialEq)]
pub enum HostRequestBody {
    /// List the next page of published Worlds in identity order.
    ListWorlds {
        /// Exclusive runtime-identity cursor; zero starts discovery.
        after: u64,
    },
    /// Create and attach; temporary Worlds are destroyed when this connection closes.
    CreateWorld {
        /// Creation or load configuration.
        options: WorldCreateOptions,
        /// Destroy this World when the creating connection closes.
        temporary: bool,
    },
    /// Attach to a published World; one connection has at most one active World session.
    AttachWorld(WorldSelector),
    /// Rename without invalidating runtime identity or current attachments.
    RenameWorld {
        /// Target World identity or published descriptor.
        world: WorldSelector,
        /// Host-unique editable World name.
        symbolic_id: String,
    },
    /// Explicitly destroy a World and invalidate its attached sessions.
    DestroyWorld(WorldSelector),
    /// Release the current session and return to Host-only state.
    DetachWorld,
    /// Update reservations on the attached World.
    SetCapacityHints(ipp_core::WorldCapacityHintsPatch),
    /// Start a synchronous authored save of the attached World.
    SaveWorld,
    /// Read an ordered chunk of a ready save; Pending is a normal result.
    ReadWorldSave {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Expected byte offset in the complete file.
        offset: u64,
    },
    /// Reserve a private inbound file transfer; it does not create a World.
    BeginWorldLoad {
        /// Owned payload or expected complete byte length.
        bytes: u64,
        /// Creation or load configuration.
        options: ipp_core::services::world_serialization::WorldLoadOptions,
    },
    /// Append one owned, ordered file chunk.
    WriteWorldLoad {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Expected byte offset in the complete file.
        offset: u64,
        /// Owned payload or expected complete byte length.
        bytes: Vec<u8>,
    },
    /// Validate and publish a new World from the complete private transfer, then attach.
    FinishWorldLoad {
        /// Connection-scoped transfer identity.
        job: u64,
    },
    /// Cancel and discard the current unpublished save or load.
    CancelWorldTransfer {
        /// Connection-scoped transfer identity.
        job: u64,
    },
}

/// Correlated Host result or unsolicited attachment invalidation.
#[derive(Clone, Debug, PartialEq)]
pub struct HostResponse {
    /// Original Host connection identity, independent of attached World sessions.
    pub connection: u64,
    /// Caller correlation, or zero for an attachment invalidation.
    pub request_id: u64,
    /// Result or explicit failure; semantic errors leave the Host connection usable.
    pub body: HostResponseBody,
}

/// Host results have no simulation tick and do not imply resource/render readiness.
#[derive(Clone, Debug, PartialEq)]
pub enum HostResponseBody {
    /// Page of published Worlds; zero next cursor means there are no more.
    Worlds {
        /// Published World descriptors in runtime identity order.
        worlds: Vec<WorldDescriptor>,
        /// Next page cursor; zero indicates the final page.
        next: u64,
    },
    /// A fresh logical World session on the existing transport.
    Attached {
        /// Target World identity or published descriptor.
        world: WorldDescriptor,
        /// Fresh logical World session identity.
        session: u64,
    },
    /// Updated World metadata/configuration.
    World(WorldDescriptor),
    /// Completed lifecycle operation.
    Complete,
    /// Operation rejected without closing the Host connection.
    Error(String),
    /// The current World session ended; Host operations remain available.
    Detached {
        /// Fresh logical World session identity.
        session: u64,
        /// Explanation of the completed detachment.
        reason: String,
    },
    /// A save/load transfer was accepted under this connection's lifetime.
    Transfer {
        /// Connection-scoped transfer identity.
        job: u64,
    },
    /// Ordered bytes of a completed file, with exact total length.
    SaveChunk {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Expected byte offset in the complete file.
        offset: u64,
        /// Exact completed file byte length.
        total: u64,
        /// Owned payload or expected complete byte length.
        bytes: Vec<u8>,
    },
}

/// Validate and decode one connection-scoped Host request.
pub fn decode_host_request(bytes: &[u8], connection: u64) -> Result<HostRequest, ProtocolError> {
    let mut reader = host_reader(bytes, HOST_REQUEST_MAGIC, connection)?;
    let request_id = reader.u64()?;
    if request_id == 0 {
        return Err(ProtocolError::Malformed("zero Host request"));
    }
    let body = match reader.u8()? {
        HOST_REQUEST_LIST_WORLDS => HostRequestBody::ListWorlds {
            after: reader.u64()?,
        },
        HOST_REQUEST_CREATE_WORLD => HostRequestBody::CreateWorld {
            options: WorldCreateOptions {
                symbolic_id: reader.string()?,
                capacity_hints: read_hints_patch(&mut reader)?
                    .apply(&WorldCapacityHints::default()),
            },
            temporary: reader.boolean()?,
        },
        HOST_REQUEST_ATTACH_WORLD => HostRequestBody::AttachWorld(read_selector(&mut reader)?),
        HOST_REQUEST_RENAME_WORLD => HostRequestBody::RenameWorld {
            world: read_selector(&mut reader)?,
            symbolic_id: reader.string()?,
        },
        HOST_REQUEST_DESTROY_WORLD => HostRequestBody::DestroyWorld(read_selector(&mut reader)?),
        HOST_REQUEST_DETACH_WORLD => HostRequestBody::DetachWorld,
        HOST_REQUEST_SET_CAPACITY_HINTS => {
            HostRequestBody::SetCapacityHints(read_hints_patch(&mut reader)?)
        }
        HOST_REQUEST_SAVE_WORLD => HostRequestBody::SaveWorld,
        HOST_REQUEST_READ_WORLD_SAVE => HostRequestBody::ReadWorldSave {
            job: reader.u64()?,
            offset: reader.u64()?,
        },
        HOST_REQUEST_BEGIN_WORLD_LOAD => {
            let bytes = reader.u64()?;
            let symbolic_id = if reader.boolean()? {
                Some(reader.string()?)
            } else {
                None
            };
            let capacity_hints = read_hints_patch(&mut reader)?;
            HostRequestBody::BeginWorldLoad {
                bytes,
                options: ipp_core::services::world_serialization::WorldLoadOptions {
                    symbolic_id,
                    capacity_hints,
                },
            }
        }
        HOST_REQUEST_WRITE_WORLD_LOAD => HostRequestBody::WriteWorldLoad {
            job: reader.u64()?,
            offset: reader.u64()?,
            bytes: reader.bytes()?,
        },
        HOST_REQUEST_FINISH_WORLD_LOAD => HostRequestBody::FinishWorldLoad {
            job: reader.u64()?,
        },
        HOST_REQUEST_CANCEL_WORLD_TRANSFER => HostRequestBody::CancelWorldTransfer {
            job: reader.u64()?,
        },
        tag => return Err(ProtocolError::Unsupported(tag)),
    };
    if reader.at != bytes.len() {
        return Err(ProtocolError::Malformed("trailing Host bytes"));
    }
    Ok(HostRequest {
        connection,
        request_id,
        body,
    })
}

/// Encode one bounded Host operation.
pub fn encode_host_request(request: &HostRequest) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = host_writer(HOST_REQUEST_MAGIC, request.connection, request.request_id)?;
    if request.request_id == 0 {
        return Err(ProtocolError::Malformed("zero Host request"));
    }
    match &request.body {
        HostRequestBody::ListWorlds {
            after,
        } => {
            writer.u8(HOST_REQUEST_LIST_WORLDS)?;
            writer.u64(*after)?;
        }
        HostRequestBody::CreateWorld {
            options,
            temporary,
        } => {
            writer.u8(HOST_REQUEST_CREATE_WORLD)?;
            writer.string(&options.symbolic_id)?;
            write_hints_patch(&mut writer, &options.capacity_hints.clone().into())?;
            writer.u8(u8::from(*temporary))?;
        }
        HostRequestBody::AttachWorld(world) => {
            writer.u8(HOST_REQUEST_ATTACH_WORLD)?;
            write_selector(&mut writer, world)?;
        }
        HostRequestBody::RenameWorld {
            world,
            symbolic_id,
        } => {
            writer.u8(HOST_REQUEST_RENAME_WORLD)?;
            write_selector(&mut writer, world)?;
            writer.string(symbolic_id)?;
        }
        HostRequestBody::DestroyWorld(world) => {
            writer.u8(HOST_REQUEST_DESTROY_WORLD)?;
            write_selector(&mut writer, world)?;
        }
        HostRequestBody::DetachWorld => writer.u8(HOST_REQUEST_DETACH_WORLD)?,
        HostRequestBody::SetCapacityHints(hints) => {
            writer.u8(HOST_REQUEST_SET_CAPACITY_HINTS)?;
            write_hints_patch(&mut writer, hints)?;
        }
        HostRequestBody::SaveWorld => writer.u8(HOST_REQUEST_SAVE_WORLD)?,
        HostRequestBody::ReadWorldSave {
            job,
            offset,
        } => {
            writer.u8(HOST_REQUEST_READ_WORLD_SAVE)?;
            writer.u64(*job)?;
            writer.u64(*offset)?;
        }
        HostRequestBody::BeginWorldLoad {
            bytes,
            options,
        } => {
            writer.u8(HOST_REQUEST_BEGIN_WORLD_LOAD)?;
            writer.u64(*bytes)?;
            writer.u8(u8::from(options.symbolic_id.is_some()))?;
            if let Some(symbol) = &options.symbolic_id {
                writer.string(symbol)?;
            }
            write_hints_patch(&mut writer, &options.capacity_hints)?;
        }
        HostRequestBody::WriteWorldLoad {
            job,
            offset,
            bytes,
        } => {
            writer.u8(HOST_REQUEST_WRITE_WORLD_LOAD)?;
            writer.u64(*job)?;
            writer.u64(*offset)?;
            writer.bytes(bytes)?;
        }
        HostRequestBody::FinishWorldLoad {
            job,
        } => {
            writer.u8(HOST_REQUEST_FINISH_WORLD_LOAD)?;
            writer.u64(*job)?;
        }
        HostRequestBody::CancelWorldTransfer {
            job,
        } => {
            writer.u8(HOST_REQUEST_CANCEL_WORLD_TRANSFER)?;
            writer.u64(*job)?;
        }
    }
    Ok(writer.0)
}

/// Encode a Host result independently of simulation frame envelopes.
pub fn encode_host_response(response: &HostResponse) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = host_writer(
        HOST_RESPONSE_MAGIC,
        response.connection,
        response.request_id,
    )?;
    match &response.body {
        HostResponseBody::Worlds {
            worlds,
            next,
        } => {
            writer.u8(HOST_RESPONSE_WORLDS)?;
            writer.count(worlds.len(), 32)?;
            for world in worlds {
                write_world(&mut writer, world)?;
            }
            writer.u64(*next)?;
        }
        HostResponseBody::Attached {
            world,
            session,
        } => {
            writer.u8(HOST_RESPONSE_ATTACHED)?;
            write_world(&mut writer, world)?;
            writer.u64(*session)?;
        }
        HostResponseBody::World(world) => {
            writer.u8(HOST_RESPONSE_WORLD)?;
            write_world(&mut writer, world)?;
        }
        HostResponseBody::Complete => writer.u8(HOST_RESPONSE_COMPLETE)?,
        HostResponseBody::Error(error) => {
            writer.u8(HOST_RESPONSE_ERROR)?;
            writer.string(error)?;
        }
        HostResponseBody::Detached {
            session,
            reason,
        } => {
            writer.u8(HOST_RESPONSE_DETACHED)?;
            writer.u64(*session)?;
            writer.string(reason)?;
        }
        HostResponseBody::Transfer {
            job,
        } => {
            writer.u8(HOST_RESPONSE_TRANSFER)?;
            writer.u64(*job)?;
        }
        HostResponseBody::SaveChunk {
            job,
            offset,
            total,
            bytes,
        } => {
            writer.u8(HOST_RESPONSE_SAVE_CHUNK)?;
            writer.u64(*job)?;
            writer.u64(*offset)?;
            writer.u64(*total)?;
            writer.bytes(bytes)?;
        }
    }
    Ok(writer.0)
}

/// Validate and decode a result for the selected Host connection.
pub fn decode_host_response(bytes: &[u8], connection: u64) -> Result<HostResponse, ProtocolError> {
    let mut reader = host_reader(bytes, HOST_RESPONSE_MAGIC, connection)?;
    let request_id = reader.u64()?;
    let body = match reader.u8()? {
        HOST_RESPONSE_WORLDS => {
            let count = reader.count(32)?;
            let mut worlds = Vec::new();
            for _ in 0..count {
                worlds.push(read_world(&mut reader)?);
            }
            HostResponseBody::Worlds {
                worlds,
                next: reader.u64()?,
            }
        }
        HOST_RESPONSE_ATTACHED => HostResponseBody::Attached {
            world: read_world(&mut reader)?,
            session: reader.u64()?,
        },
        HOST_RESPONSE_WORLD => HostResponseBody::World(read_world(&mut reader)?),
        HOST_RESPONSE_COMPLETE => HostResponseBody::Complete,
        HOST_RESPONSE_ERROR => HostResponseBody::Error(reader.string()?),
        HOST_RESPONSE_DETACHED => HostResponseBody::Detached {
            session: reader.u64()?,
            reason: reader.string()?,
        },
        HOST_RESPONSE_TRANSFER => HostResponseBody::Transfer {
            job: reader.u64()?,
        },
        HOST_RESPONSE_SAVE_CHUNK => HostResponseBody::SaveChunk {
            job: reader.u64()?,
            offset: reader.u64()?,
            total: reader.u64()?,
            bytes: reader.bytes()?,
        },
        tag => return Err(ProtocolError::Unsupported(tag)),
    };
    if reader.at != bytes.len() {
        return Err(ProtocolError::Malformed("trailing Host bytes"));
    }
    Ok(HostResponse {
        connection,
        request_id,
        body,
    })
}

fn host_reader<'a>(
    bytes: &'a [u8],
    magic: &[u8; 8],
    connection: u64,
) -> Result<Reader<'a>, ProtocolError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::Limit("Host message"));
    }
    let mut reader = Reader {
        bytes,
        at: 0,
    };
    if reader.take(8)? != magic {
        return Err(ProtocolError::Malformed("Host envelope"));
    }
    if connection == 0 || reader.u64()? != connection {
        return Err(ProtocolError::SessionMismatch);
    }
    Ok(reader)
}

fn host_writer(magic: &[u8; 8], connection: u64, request_id: u64) -> Result<Writer, ProtocolError> {
    if connection == 0 {
        return Err(ProtocolError::SessionMismatch);
    }
    let mut writer = Writer(Vec::new());
    writer.raw(magic)?;
    writer.u64(connection)?;
    writer.u64(request_id)?;
    Ok(writer)
}

fn read_selector(reader: &mut Reader<'_>) -> Result<WorldSelector, ProtocolError> {
    match reader.u8()? {
        WORLD_SELECTOR_ID => Ok(WorldSelector::Id(WorldId(reader.u64()?))),
        WORLD_SELECTOR_SYMBOL => Ok(WorldSelector::SymbolicId(reader.string()?)),
        tag => Err(ProtocolError::Unsupported(tag)),
    }
}

fn write_selector(writer: &mut Writer, selector: &WorldSelector) -> Result<(), ProtocolError> {
    match selector {
        WorldSelector::Id(id) => {
            writer.u8(WORLD_SELECTOR_ID)?;
            writer.u64(id.0)
        }
        WorldSelector::SymbolicId(symbol) => {
            writer.u8(WORLD_SELECTOR_SYMBOL)?;
            writer.string(symbol)
        }
    }
}

fn read_hints(reader: &mut Reader<'_>) -> Result<WorldCapacityHints, ProtocolError> {
    let entities = reader.u32()? as usize;
    let count = reader.count(1024)?;
    let mut systems = BTreeMap::new();
    for _ in 0..count {
        let system = reader.string()?;
        let count = reader.count(1024)?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let key = reader.string()?;
            let value = reader.u32()? as usize;
            if values.insert(key, value).is_some() {
                return Err(ProtocolError::Malformed("duplicate capacity hint"));
            }
        }
        if systems
            .insert(system, WorldSystemCapacityHints(values))
            .is_some()
        {
            return Err(ProtocolError::Malformed("duplicate system hints"));
        }
    }
    Ok(WorldCapacityHints {
        entities,
        systems,
    })
}

fn write_hints(writer: &mut Writer, hints: &WorldCapacityHints) -> Result<(), ProtocolError> {
    writer.u32(
        u32::try_from(hints.entities).map_err(|_| ProtocolError::Limit("entity reservation"))?,
    )?;
    writer.count(hints.systems.len(), 1024)?;
    for (system, hints) in &hints.systems {
        writer.string(system)?;
        writer.count(hints.0.len(), 1024)?;
        for (key, &value) in &hints.0 {
            writer.string(key)?;
            writer.u32(
                u32::try_from(value).map_err(|_| ProtocolError::Limit("system reservation"))?,
            )?;
        }
    }
    Ok(())
}

fn write_world(writer: &mut Writer, world: &WorldDescriptor) -> Result<(), ProtocolError> {
    writer.u64(world.id.0)?;
    writer.string(&world.metadata.symbolic_id)?;
    writer.raw(&world.metadata.persistent_id.0.to_le_bytes())?;
    write_hints(writer, &world.capacity_hints)
}

fn read_world(reader: &mut Reader<'_>) -> Result<WorldDescriptor, ProtocolError> {
    let id = WorldId(reader.u64()?);
    let symbolic_id = reader.string()?;
    let persistent_id = WorldPersistentId(u128::from_le_bytes(
        reader.take(16)?.try_into().expect("checked length"),
    ));
    Ok(WorldDescriptor {
        id,
        metadata: WorldMetadata {
            symbolic_id,
            persistent_id,
        },
        capacity_hints: read_hints(reader)?,
    })
}

fn read_hints_patch(
    reader: &mut Reader<'_>,
) -> Result<ipp_core::WorldCapacityHintsPatch, ProtocolError> {
    let entities = if reader.boolean()? {
        Some(reader.u32()? as usize)
    } else {
        None
    };
    let count = reader.count(1024)?;
    let mut systems = BTreeMap::new();
    for _ in 0..count {
        let system = reader.string()?;
        let count = reader.count(1024)?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let key = reader.string()?;
            let value = reader.u32()? as usize;
            if values.insert(key, value).is_some() {
                return Err(ProtocolError::Malformed("duplicate capacity hint"));
            }
        }
        if systems
            .insert(system, WorldSystemCapacityHints(values))
            .is_some()
        {
            return Err(ProtocolError::Malformed("duplicate system hints"));
        }
    }
    Ok(ipp_core::WorldCapacityHintsPatch {
        entities,
        systems,
    })
}

fn write_hints_patch(
    writer: &mut Writer,
    hints: &ipp_core::WorldCapacityHintsPatch,
) -> Result<(), ProtocolError> {
    writer.u8(u8::from(hints.entities.is_some()))?;
    if let Some(entities) = hints.entities {
        writer.u32(
            u32::try_from(entities).map_err(|_| ProtocolError::Limit("entity reservation"))?,
        )?;
    }
    writer.count(hints.systems.len(), 1024)?;
    for (system, hints) in &hints.systems {
        writer.string(system)?;
        writer.count(hints.0.len(), 1024)?;
        for (key, &value) in &hints.0 {
            writer.string(key)?;
            writer.u32(
                u32::try_from(value).map_err(|_| ProtocolError::Limit("system reservation"))?,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
