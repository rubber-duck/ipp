//! Host response codec, independent of simulation frame envelopes.

use super::requests::host_reader;
use super::worlds::{read_manifest, read_world, write_manifest, write_world};
use super::{
    HOST_RESPONSE_MAGIC, HostResponse, HostResponseBody, MAX_GRAPH_BINDING_PAGE,
    MAX_GRAPH_METADATA_PAGE,
};
use crate::ProtocolError;
use crate::codec::Writer;
use crate::contract::wire_manifest::*;
use ipp_core::services::world_serialization::{WorldGraphNodeDescriptor, WorldGraphNodeId};
use ipp_core::{WorldMetadata, WorldPersistentId};

/// Encode a Host result independently of simulation frame envelopes.
pub fn encode_host_response(response: &HostResponse) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = Vec::with_capacity(encoded_host_response_size(response)?);
    encode_host_response_into(response, &mut bytes)?;
    Ok(bytes)
}

/// Measure the response without allocating its encoded storage.
pub fn encoded_host_response_size(response: &HostResponse) -> Result<usize, ProtocolError> {
    let mut writer = Writer::measuring();
    write_host_response(response, &mut writer)?;
    Ok(writer.len())
}

/// Encode into exclusively reserved storage. Failure publishes no partial response.
pub fn encode_host_response_into(
    response: &HostResponse,
    bytes: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    bytes.clear();
    let mut writer = Writer::new(std::mem::take(bytes));
    let result = write_host_response(response, &mut writer);
    *bytes = writer.0;
    if result.is_err() {
        bytes.clear();
    }
    result
}

fn write_host_response(response: &HostResponse, writer: &mut Writer) -> Result<(), ProtocolError> {
    if response.connection == 0 {
        return Err(ProtocolError::SessionMismatch);
    }
    writer.raw(HOST_RESPONSE_MAGIC)?;
    writer.u64(response.connection)?;
    writer.u64(response.request_id)?;
    match &response.body {
        HostResponseBody::AssetExport(body) => {
            writer.u8(HOST_RESPONSE_ASSET_EXPORT)?;
            writer.asset_export_response(body)?;
        }
        HostResponseBody::RootBinding(binding) => {
            writer.u8(HOST_RESPONSE_ROOT_BINDING)?;
            writer.u8(u8::from(binding.is_some()))?;
            if let Some(binding) = binding {
                writer.root_binding(*binding)?;
            }
        }
        HostResponseBody::Presentation(response) => {
            writer.u8(HOST_RESPONSE_PRESENTATION)?;
            writer.presentation_response(response)?;
        }
        HostResponseBody::Profile(response) => {
            writer.u8(HOST_RESPONSE_PROFILE)?;
            writer.profile_response(response)?;
        }
        HostResponseBody::GuiInput(response) => {
            writer.u8(HOST_RESPONSE_GUI_INPUT)?;
            writer.gui_physical_response(response)?;
        }
        HostResponseBody::Worlds {
            worlds,
            next,
        } => {
            writer.u8(HOST_RESPONSE_WORLDS)?;
            writer.count(worlds.len(), 32)?;
            for world in worlds {
                write_world(writer, world)?;
            }
            writer.u64(*next)?;
        }
        HostResponseBody::Created {
            world,
            reference,
        } => {
            writer.u8(HOST_RESPONSE_CREATED)?;
            write_world(writer, world)?;
            writer.world_reference(*reference)?;
        }
        HostResponseBody::Attached {
            reference,
            world,
            session,
            manifest,
        } => {
            writer.u8(HOST_RESPONSE_ATTACHED)?;
            write_world(writer, world)?;
            writer.u64(*session)?;
            write_manifest(writer, manifest)?;
            writer.world_reference(*reference)?;
        }
        HostResponseBody::WorldReference(reference) => {
            writer.u8(HOST_RESPONSE_WORLD_REFERENCE)?;
            writer.world_reference(*reference)?;
        }
        HostResponseBody::OutputReference(reference) => {
            writer.u8(HOST_RESPONSE_OUTPUT_REFERENCE)?;
            writer.output_reference(*reference)?;
        }
        HostResponseBody::World(world) => {
            writer.u8(HOST_RESPONSE_WORLD)?;
            write_world(writer, world)?;
        }
        HostResponseBody::Complete => writer.u8(HOST_RESPONSE_COMPLETE)?,
        HostResponseBody::Error(error) => {
            writer.u8(HOST_RESPONSE_ERROR)?;
            writer.string(error)?;
        }
        HostResponseBody::WorldGraphPage {
            job,
            root,
            total,
            offset,
            nodes,
        } => {
            writer.u8(HOST_RESPONSE_WORLD_GRAPH_PAGE)?;
            writer.u64(*job)?;
            writer.u32(root.0)?;
            writer.u32(*total)?;
            writer.u32(*offset)?;
            writer.count(nodes.len(), MAX_GRAPH_METADATA_PAGE)?;
            for node in nodes {
                writer.u32(node.id.0)?;
                writer.string(&node.metadata.symbolic_id)?;
                writer.raw(&node.metadata.persistent_id.0.to_le_bytes())?;
            }
        }
        HostResponseBody::WorldGraphLoaded {
            job,
            root,
            total,
        } => {
            writer.u8(HOST_RESPONSE_WORLD_GRAPH_LOADED)?;
            writer.u64(*job)?;
            writer.world_reference(*root)?;
            writer.u32(*total)?;
        }
        HostResponseBody::WorldGraphBindings {
            job,
            offset,
            bindings,
        } => {
            writer.u8(HOST_RESPONSE_WORLD_GRAPH_BINDINGS)?;
            writer.u64(*job)?;
            writer.u32(*offset)?;
            writer.count(bindings.len(), MAX_GRAPH_BINDING_PAGE)?;
            for (node, world) in bindings {
                writer.u32(node.0)?;
                writer.world_reference(*world)?;
            }
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
        HostResponseBody::Read {
            reference,
            length,
        } => {
            writer.u8(HOST_RESPONSE_READ)?;
            writer.u64(reference.connection)?;
            writer.u64(reference.read)?;
            writer.u8(u8::from(length.is_some()))?;
            if let Some(length) = length {
                writer.u64(*length)?;
            }
        }
    }
    Ok(())
}

/// Validate and decode a result for the selected Host connection.
pub fn decode_host_response(bytes: &[u8], connection: u64) -> Result<HostResponse, ProtocolError> {
    let mut reader = host_reader(bytes, HOST_RESPONSE_MAGIC, connection)?;
    let request_id = reader.u64()?;
    let body = match reader.u8()? {
        HOST_RESPONSE_ASSET_EXPORT => {
            HostResponseBody::AssetExport(reader.asset_export_response()?)
        }
        HOST_RESPONSE_ROOT_BINDING => HostResponseBody::RootBinding(if reader.boolean()? {
            Some(reader.root_binding()?)
        } else {
            None
        }),
        HOST_RESPONSE_PRESENTATION => {
            HostResponseBody::Presentation(reader.presentation_response()?)
        }
        HOST_RESPONSE_PROFILE => HostResponseBody::Profile(reader.profile_response()?),
        HOST_RESPONSE_GUI_INPUT => HostResponseBody::GuiInput(reader.gui_physical_response()?),
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
        HOST_RESPONSE_CREATED => HostResponseBody::Created {
            world: read_world(&mut reader)?,
            reference: reader.world_reference()?,
        },
        HOST_RESPONSE_ATTACHED => HostResponseBody::Attached {
            world: read_world(&mut reader)?,
            session: reader.u64()?,
            manifest: read_manifest(&mut reader)?,
            reference: reader.world_reference()?,
        },
        HOST_RESPONSE_WORLD_REFERENCE => {
            HostResponseBody::WorldReference(reader.world_reference()?)
        }
        HOST_RESPONSE_OUTPUT_REFERENCE => {
            HostResponseBody::OutputReference(reader.output_reference()?)
        }
        HOST_RESPONSE_WORLD => HostResponseBody::World(read_world(&mut reader)?),
        HOST_RESPONSE_COMPLETE => HostResponseBody::Complete,
        HOST_RESPONSE_ERROR => HostResponseBody::Error(reader.string()?),
        HOST_RESPONSE_WORLD_GRAPH_PAGE => {
            let job = reader.u64()?;
            let root = WorldGraphNodeId(reader.u32()?);
            let total = reader.u32()?;
            let offset = reader.u32()?;
            let count = reader.count(MAX_GRAPH_METADATA_PAGE)?;
            let mut nodes = Vec::with_capacity(count);
            for _ in 0..count {
                nodes.push(WorldGraphNodeDescriptor {
                    id: WorldGraphNodeId(reader.u32()?),
                    metadata: WorldMetadata {
                        symbolic_id: reader.string()?,
                        persistent_id: WorldPersistentId(u128::from_le_bytes(
                            reader.take(16)?.try_into().expect("fixed u128"),
                        )),
                    },
                });
            }
            HostResponseBody::WorldGraphPage {
                job,
                root,
                total,
                offset,
                nodes,
            }
        }
        HOST_RESPONSE_WORLD_GRAPH_LOADED => HostResponseBody::WorldGraphLoaded {
            job: reader.u64()?,
            root: reader.world_reference()?,
            total: reader.u32()?,
        },
        HOST_RESPONSE_WORLD_GRAPH_BINDINGS => {
            let job = reader.u64()?;
            let offset = reader.u32()?;
            let count = reader.count(MAX_GRAPH_BINDING_PAGE)?;
            let mut bindings = Vec::with_capacity(count);
            for _ in 0..count {
                bindings.push((WorldGraphNodeId(reader.u32()?), reader.world_reference()?));
            }
            HostResponseBody::WorldGraphBindings {
                job,
                offset,
                bindings,
            }
        }
        HOST_RESPONSE_DETACHED => HostResponseBody::Detached {
            session: reader.u64()?,
            reason: reader.string()?,
        },
        HOST_RESPONSE_TRANSFER => HostResponseBody::Transfer {
            job: reader.u64()?,
        },
        HOST_RESPONSE_READ => HostResponseBody::Read {
            reference: crate::bulk_read::BulkReadReference {
                connection: reader.u64()?,
                read: reader.u64()?,
            },
            length: if reader.boolean()? {
                Some(reader.u64()?)
            } else {
                None
            },
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
