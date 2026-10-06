//! Host request framing and codec.

use super::worlds::{
    read_hints_patch, read_selector, read_system_selection, write_hints_patch, write_selector,
    write_system_selection,
};
use super::{
    HOST_REQUEST_MAGIC, HostRequest, HostRequestBody, MAX_GRAPH_METADATA_PAGE, WorldCreateOptions,
};
use crate::codec::{Reader, Writer};
use crate::contract::wire_manifest::*;
use crate::{MAX_MESSAGE_BYTES, ProtocolError};
use ipp_core::WorldCapacityHints;
use ipp_core::services::world_serialization::WorldGraphNodeId;
use std::collections::BTreeMap;

/// Validate and decode one connection-scoped Host request.
pub fn decode_host_request(bytes: &[u8], connection: u64) -> Result<HostRequest, ProtocolError> {
    let mut reader = host_reader(bytes, HOST_REQUEST_MAGIC, connection)?;
    let request_id = reader.u64()?;
    if request_id == 0 {
        return Err(ProtocolError::Malformed("zero Host request"));
    }
    let body = match reader.u8()? {
        HOST_REQUEST_ASSET_EXPORT => HostRequestBody::AssetExport(reader.asset_export_request()?),
        HOST_REQUEST_LIST_WORLDS => HostRequestBody::ListWorlds {
            after: reader.u64()?,
        },
        HOST_REQUEST_CREATE_WORLD => HostRequestBody::CreateWorld {
            options: WorldCreateOptions {
                symbolic_id: reader.string()?,
                capacity_hints: read_hints_patch(&mut reader)?
                    .apply(&WorldCapacityHints::default()),
                selected_systems: read_system_selection(&mut reader)?,
                canvas: if reader.boolean()? {
                    Some(reader.canvas_state()?)
                } else {
                    None
                },
            },
            temporary: reader.boolean()?,
        },
        HOST_REQUEST_OPEN_WORLD => HostRequestBody::OpenWorld(reader.world_reference()?),
        HOST_REQUEST_RESOLVE_WORLD => HostRequestBody::ResolveWorld(read_selector(&mut reader)?),
        HOST_REQUEST_BIND_OUTPUT => HostRequestBody::BindOutput {
            world: reader.world_reference()?,
            entity: reader.u64()?,
            kind: reader.output_kind()?,
        },
        HOST_REQUEST_RESOLVE_OUTPUT => HostRequestBody::ResolveOutput(reader.output_reference()?),
        HOST_REQUEST_SET_ROOT_OUTPUT => HostRequestBody::SetRootOutput {
            output: reader.output_reference()?,
            viewport: ipp_core::WorldViewport {
                width: reader.u32()?,
                height: reader.u32()?,
                device_pixel_ratio: reader.f64()?,
            },
        },
        HOST_REQUEST_CLEAR_ROOT_OUTPUT => HostRequestBody::ClearRootOutput(reader.root_binding()?),
        HOST_REQUEST_GET_ROOT_OUTPUT_BINDING => {
            HostRequestBody::GetRootOutputBinding(reader.world_reference()?)
        }
        HOST_REQUEST_PRESENTATION => HostRequestBody::Presentation(reader.presentation_request()?),
        HOST_REQUEST_PROFILE => HostRequestBody::Profile(reader.profile_request()?),
        HOST_REQUEST_GUI_INPUT => HostRequestBody::GuiInput(reader.gui_physical_request()?),
        HOST_REQUEST_RENAME_WORLD => HostRequestBody::RenameWorld {
            world: read_selector(&mut reader)?,
            symbolic_id: reader.string()?,
        },
        HOST_REQUEST_DESTROY_WORLD => HostRequestBody::DestroyWorld(reader.world_reference()?),
        HOST_REQUEST_DETACH_WORLD => HostRequestBody::DetachWorld {
            session: reader.u64()?,
        },
        HOST_REQUEST_SET_CAPACITY_HINTS => HostRequestBody::SetCapacityHints {
            session: reader.u64()?,
            hints: read_hints_patch(&mut reader)?,
        },
        HOST_REQUEST_SAVE_WORLD => HostRequestBody::SaveWorld {
            session: reader.u64()?,
        },
        HOST_REQUEST_BEGIN_WORLD_LOAD => HostRequestBody::BeginWorldLoad {
            bytes: reader.u64()?,
        },
        HOST_REQUEST_WRITE_WORLD_LOAD => HostRequestBody::WriteWorldLoad {
            job: reader.u64()?,
            offset: reader.u64()?,
            bytes: reader.bytes()?,
        },
        HOST_REQUEST_FINISH_WORLD_LOAD => HostRequestBody::FinishWorldLoad {
            job: reader.u64()?,
            symbolic_id: if reader.boolean()? {
                Some(reader.string()?)
            } else {
                None
            },
            capacity_hints: read_hints_patch(&mut reader)?,
        },
        HOST_REQUEST_INSPECT_WORLD_LOAD => HostRequestBody::InspectWorldLoad {
            job: reader.u64()?,
            offset: reader.u32()?,
        },
        HOST_REQUEST_SET_WORLD_LOAD_NAMES => {
            let job = reader.u64()?;
            let count = reader.count(MAX_GRAPH_METADATA_PAGE)?;
            let mut names = BTreeMap::new();
            for _ in 0..count {
                if names
                    .insert(WorldGraphNodeId(reader.u32()?), reader.string()?)
                    .is_some()
                {
                    return Err(ProtocolError::Malformed("duplicate graph rename"));
                }
            }
            HostRequestBody::SetWorldLoadNames {
                job,
                names,
            }
        }
        HOST_REQUEST_READ_WORLD_LOAD_BINDINGS => HostRequestBody::ReadWorldLoadBindings {
            job: reader.u64()?,
            offset: reader.u32()?,
        },
        HOST_REQUEST_ACKNOWLEDGE_WORLD_LOAD => HostRequestBody::AcknowledgeWorldLoad {
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
        HostRequestBody::AssetExport(body) => {
            writer.u8(HOST_REQUEST_ASSET_EXPORT)?;
            writer.asset_export_request(body)?;
        }
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
            write_system_selection(&mut writer, &options.selected_systems)?;
            writer.u8(u8::from(options.canvas.is_some()))?;
            if let Some(canvas) = &options.canvas {
                writer.canvas_state(canvas)?;
            }
            writer.u8(u8::from(*temporary))?;
        }
        HostRequestBody::OpenWorld(world) => {
            writer.u8(HOST_REQUEST_OPEN_WORLD)?;
            writer.world_reference(*world)?;
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
            writer.world_reference(*world)?;
        }
        HostRequestBody::ResolveWorld(world) => {
            writer.u8(HOST_REQUEST_RESOLVE_WORLD)?;
            write_selector(&mut writer, world)?;
        }
        HostRequestBody::BindOutput {
            world,
            entity,
            kind,
        } => {
            writer.u8(HOST_REQUEST_BIND_OUTPUT)?;
            writer.world_reference(*world)?;
            writer.u64(*entity)?;
            writer.u8(match kind {
                ipp_core::OutputKind::Canvas => OUTPUT_CANVAS,
                ipp_core::OutputKind::Camera => OUTPUT_CAMERA,
            })?;
        }
        HostRequestBody::ResolveOutput(output) => {
            writer.u8(HOST_REQUEST_RESOLVE_OUTPUT)?;
            writer.output_reference(*output)?;
        }
        HostRequestBody::SetRootOutput {
            output,
            viewport,
        } => {
            writer.u8(HOST_REQUEST_SET_ROOT_OUTPUT)?;
            writer.output_reference(*output)?;
            writer.u32(viewport.width)?;
            writer.u32(viewport.height)?;
            writer.f64(viewport.device_pixel_ratio)?;
        }
        HostRequestBody::ClearRootOutput(binding) => {
            writer.u8(HOST_REQUEST_CLEAR_ROOT_OUTPUT)?;
            writer.root_binding(*binding)?;
        }
        HostRequestBody::GetRootOutputBinding(world) => {
            writer.u8(HOST_REQUEST_GET_ROOT_OUTPUT_BINDING)?;
            writer.world_reference(*world)?;
        }
        HostRequestBody::Presentation(request) => {
            writer.u8(HOST_REQUEST_PRESENTATION)?;
            writer.presentation_request(request)?;
        }
        HostRequestBody::Profile(request) => {
            writer.u8(HOST_REQUEST_PROFILE)?;
            writer.profile_request(request)?;
        }
        HostRequestBody::GuiInput(request) => {
            writer.u8(HOST_REQUEST_GUI_INPUT)?;
            writer.gui_physical_request(request)?;
        }
        HostRequestBody::DetachWorld {
            session,
        } => {
            writer.u8(HOST_REQUEST_DETACH_WORLD)?;
            writer.u64(*session)?;
        }
        HostRequestBody::SetCapacityHints {
            session,
            hints,
        } => {
            writer.u8(HOST_REQUEST_SET_CAPACITY_HINTS)?;
            writer.u64(*session)?;
            write_hints_patch(&mut writer, hints)?;
        }
        HostRequestBody::SaveWorld {
            session,
        } => {
            writer.u8(HOST_REQUEST_SAVE_WORLD)?;
            writer.u64(*session)?;
        }
        HostRequestBody::BeginWorldLoad {
            bytes,
        } => {
            writer.u8(HOST_REQUEST_BEGIN_WORLD_LOAD)?;
            writer.u64(*bytes)?;
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
            symbolic_id,
            capacity_hints,
        } => {
            writer.u8(HOST_REQUEST_FINISH_WORLD_LOAD)?;
            writer.u64(*job)?;
            writer.u8(u8::from(symbolic_id.is_some()))?;
            if let Some(symbol) = symbolic_id {
                writer.string(symbol)?;
            }
            write_hints_patch(&mut writer, capacity_hints)?;
        }
        HostRequestBody::InspectWorldLoad {
            job,
            offset,
        } => {
            writer.u8(HOST_REQUEST_INSPECT_WORLD_LOAD)?;
            writer.u64(*job)?;
            writer.u32(*offset)?;
        }
        HostRequestBody::SetWorldLoadNames {
            job,
            names,
        } => {
            writer.u8(HOST_REQUEST_SET_WORLD_LOAD_NAMES)?;
            writer.u64(*job)?;
            writer.count(names.len(), MAX_GRAPH_METADATA_PAGE)?;
            for (node, name) in names {
                writer.u32(node.0)?;
                writer.string(name)?;
            }
        }
        HostRequestBody::ReadWorldLoadBindings {
            job,
            offset,
        } => {
            writer.u8(HOST_REQUEST_READ_WORLD_LOAD_BINDINGS)?;
            writer.u64(*job)?;
            writer.u32(*offset)?;
        }
        HostRequestBody::AcknowledgeWorldLoad {
            job,
        } => {
            writer.u8(HOST_REQUEST_ACKNOWLEDGE_WORLD_LOAD)?;
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

pub(super) fn host_reader<'a>(
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
    let mut writer = Writer::new(Vec::new());
    writer.raw(magic)?;
    writer.u64(connection)?;
    writer.u64(request_id)?;
    Ok(writer)
}
