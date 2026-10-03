//! Host connection control, separate from World-scoped authoring envelopes.

use crate::presentation::{PresentationRequest, PresentationResponse, RootBinding};
use crate::references::{OutputReference, WorldReference};
use crate::wire::*;

use crate::{
    MAX_MESSAGE_BYTES, ProtocolError,
    codec::{Reader, Writer},
};
use ipp_core::services::world_serialization::{WorldGraphNodeDescriptor, WorldGraphNodeId};
use ipp_core::{
    WorldCapacityHints, WorldDescriptor, WorldId, WorldMetadata, WorldPersistentId, WorldSelector,
    WorldSystemCapacityHints,
};
use std::collections::BTreeMap;

/// Host request marker and independent control format revision.
pub const HOST_REQUEST_MAGIC: &[u8; 8] = &host_magic(HOST_REQUEST_MAGIC_HEX);
/// Host response marker; World replies keep their existing fenced envelope.
pub const HOST_RESPONSE_MAGIC: &[u8; 8] = &host_magic(HOST_RESPONSE_MAGIC_HEX);

/// Metadata and rename pages fit even when every name reaches its wire byte limit.
pub const MAX_GRAPH_METADATA_PAGE: usize = 8;
/// Binding pages bound delivery independently of graph size and retained World count.
pub const MAX_GRAPH_BINDING_PAGE: usize = 1024;

/// Owned wire-facing creation request; remote names resolve against Host factories.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldCreateOptions {
    /// Host-unique symbolic name, or empty for automatic naming.
    pub symbolic_id: String,
    /// Storage reservations independent of selected Systems.
    pub capacity_hints: WorldCapacityHints,
    /// Registered factory names the World instantiates, exactly. The wire keeps
    /// absence representable only so the Host can refuse it: there is no
    /// default selection.
    pub selected_systems: Option<Vec<String>>,
    /// Initial canvas extent and density, accepted only when the selection
    /// includes the Canvas System; absent selects the defaults.
    pub canvas: Option<ipp_core::CanvasState>,
}

impl WorldCreateOptions {
    /// Automatically named options with default reservations for this selection.
    pub fn new(selected_systems: Vec<String>) -> Self {
        Self {
            symbolic_id: String::new(),
            capacity_hints: WorldCapacityHints::default(),
            selected_systems: Some(selected_systems),
            canvas: None,
        }
    }
}

/// Selected World admission, independent of the compiled target schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldManifest {
    /// Resolved System identities in execution order.
    pub systems: Vec<String>,
    /// Admitted compiled component identities.
    pub components: Vec<u16>,
    /// Stable operation identities in the Host control contract.
    pub operations: Vec<u8>,
}

impl WorldManifest {
    /// Copy a published World's immutable manifest into an owned wire reply.
    pub fn from_core(manifest: &ipp_core::systems::WorldManifest) -> Self {
        use ipp_core::systems::WorldOperation;
        Self {
            systems: manifest
                .systems()
                .iter()
                .map(|system| system.0.to_owned())
                .collect(),
            components: manifest.components().collect(),
            operations: manifest
                .operations()
                .map(|operation| match operation {
                    WorldOperation::EntityLinks => 0,
                    // Operation 1 is retired; retired identifiers are never reused.
                    WorldOperation::Animation => 2,
                    WorldOperation::JointAnimation => 3,
                    WorldOperation::Constraints => 4,
                    WorldOperation::LookAt => 5,
                    WorldOperation::Geometry => 6,
                    WorldOperation::Rendering => 7,
                    WorldOperation::Camera => 8,
                    WorldOperation::Surface => 9,
                    WorldOperation::Gui => 10,
                    WorldOperation::Particles => 11,
                    WorldOperation::Canvas => 12,
                })
                .collect(),
        }
    }
}

fn read_system_selection(reader: &mut Reader<'_>) -> Result<Option<Vec<String>>, ProtocolError> {
    if !reader.boolean()? {
        return Ok(None);
    }
    let count = reader.count(1024)?;
    let mut selected = Vec::with_capacity(count);
    for _ in 0..count {
        let name = reader.string()?;
        if name.is_empty() || selected.contains(&name) {
            return Err(ProtocolError::Malformed("selected system"));
        }
        selected.push(name);
    }
    Ok(Some(selected))
}

fn write_system_selection(
    writer: &mut Writer,
    selected: &Option<Vec<String>>,
) -> Result<(), ProtocolError> {
    writer.u8(u8::from(selected.is_some()))?;
    if let Some(selected) = selected {
        writer.count(selected.len(), 1024)?;
        for name in selected {
            writer.string(name)?;
        }
    }
    Ok(())
}

fn read_manifest(reader: &mut Reader<'_>) -> Result<WorldManifest, ProtocolError> {
    let count = reader.count(1024)?;
    let systems = (0..count)
        .map(|_| reader.string())
        .collect::<Result<Vec<_>, _>>()?;
    let count = reader.count(1024)?;
    let components = (0..count)
        .map(|_| reader.u16())
        .collect::<Result<Vec<_>, _>>()?;
    let count = reader.count(32)?;
    let operations = (0..count)
        .map(|_| reader.u8())
        .collect::<Result<Vec<_>, _>>()?;
    if operations.iter().any(|&operation| operation > 12) {
        return Err(ProtocolError::Malformed("World operation"));
    }
    Ok(WorldManifest {
        systems,
        components,
        operations,
    })
}

fn write_manifest(writer: &mut Writer, manifest: &WorldManifest) -> Result<(), ProtocolError> {
    writer.count(manifest.systems.len(), 1024)?;
    for system in &manifest.systems {
        writer.string(system)?;
    }
    writer.count(manifest.components.len(), 1024)?;
    for component in &manifest.components {
        writer.u16(*component)?;
    }
    writer.count(manifest.operations.len(), 32)?;
    for operation in &manifest.operations {
        writer.u8(*operation)?;
    }
    Ok(())
}

/// Accept a standalone World session's hello and append that World's admission manifest.
pub fn accept_world_hello(
    bytes: &[u8],
    session: u64,
    manifest: &ipp_core::systems::WorldManifest,
) -> Result<Vec<u8>, ProtocolError> {
    let mut reply = crate::accept_hello(bytes, session)?;
    let mut writer = Writer::new(Vec::new());
    write_manifest(&mut writer, &WorldManifest::from_core(manifest))?;
    reply.extend(writer.0);
    Ok(reply)
}

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
    /// Optional Host measurement controls; production explicitly reports unavailability.
    Profile(crate::profiling::ProfileRequest),
    /// List the next page of published Worlds in identity order.
    ListWorlds {
        /// Exclusive runtime-identity cursor; zero starts discovery.
        after: u64,
    },
    /// Create without opening a session; temporary Worlds are destroyed on disconnect.
    CreateWorld {
        /// Creation or load configuration.
        options: WorldCreateOptions,
        /// Destroy this World when the creating connection closes.
        temporary: bool,
    },
    /// Open another independent session for a published World.
    OpenWorld(WorldReference),
    /// Resolve discovery metadata to an exact live World lifetime.
    ResolveWorld(WorldSelector),
    /// Explicitly bind the current output producer incarnation.
    BindOutput {
        /// Exact producer World lifetime.
        world: WorldReference,
        /// Acknowledged entity identity within the producer World.
        entity: u64,
        /// Compiled output producer kind.
        kind: ipp_core::OutputKind,
    },
    /// Validate an existing token without rebinding a replacement producer.
    ResolveOutput(OutputReference),
    /// Select presentation independently of authoring sessions.
    SetRootOutput {
        /// Exact output producer lifetime.
        output: OutputReference,
        /// Host presentation extent and pixel density.
        viewport: ipp_core::WorldViewport,
    },
    /// Withdraw presentation without destroying the selected World.
    ClearRootOutput(RootBinding),
    /// Observe configuration independently of publication availability.
    GetRootOutputBinding(WorldReference),
    /// Surface selection and actual completed presentation, independently of sessions.
    Presentation(PresentationRequest),
    /// Ordered physical input owned by an exact presentation context, not a World session.
    GuiInput(crate::gui_input::GuiPhysicalRequest),
    /// Rename without invalidating runtime identity or current attachments.
    RenameWorld {
        /// Target World identity or published descriptor.
        world: WorldSelector,
        /// Host-unique editable World name.
        symbolic_id: String,
    },
    /// Explicitly destroy a World and invalidate its attached sessions.
    DestroyWorld(WorldReference),
    /// Release one session without affecting peer sessions or World lifetime.
    DetachWorld {
        /// Connection-owned World session.
        session: u64,
    },
    /// Update reservations on the selected session's World.
    SetCapacityHints {
        /// Originating connection-owned World session.
        session: u64,
        /// Sparse reservation changes.
        hints: ipp_core::WorldCapacityHintsPatch,
    },
    /// Start a synchronous authored save ordered after the originating session's edits.
    SaveWorld {
        /// Originating connection-owned World session.
        session: u64,
    },
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
    /// Inspect the complete uploaded graph without creating Worlds or reserving names.
    InspectWorldLoad {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Sequential descriptor page offset.
        offset: u32,
    },
    /// Supply explicit node-name replacements for this inspected transfer.
    SetWorldLoadNames {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Bounded page of unique graph-local identities and names.
        names: BTreeMap<WorldGraphNodeId, String>,
    },
    /// Publish the complete graph and retain all fresh identities until acknowledgement.
    FinishWorldLoad {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Optional replacement for the root name.
        symbolic_id: Option<String>,
        /// Storage hints independent of saved System selection.
        capacity_hints: ipp_core::WorldCapacityHintsPatch,
    },
    /// Read the complete graph-local to fresh runtime identity journal in bounded pages.
    ReadWorldLoadBindings {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Sequential binding page offset.
        offset: u32,
    },
    /// Transfer cleanup ownership only after all fresh identities have been delivered.
    AcknowledgeWorldLoad {
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
    /// Optional profiling status or an immutable diagnostic artifact page.
    Profile(crate::profiling::ProfileResponse),
    /// Current root configuration; not a presentation fence.
    RootBinding(Option<RootBinding>),
    /// Surface-scoped presentation control or completion.
    Presentation(PresentationResponse),
    /// Physical input context or settled routing result; never a frame completion.
    GuiInput(crate::gui_input::GuiPhysicalResponse),
    /// Creation succeeded independently of opening an authoring session.
    Created {
        /// Published discovery metadata.
        world: WorldDescriptor,
        /// Exact live World identity for opening and cleanup.
        reference: WorldReference,
    },
    /// Validated exact runtime World lifetime.
    WorldReference(WorldReference),
    /// Validated exact output producer lifetime.
    OutputReference(OutputReference),
    /// Page of published Worlds; zero next cursor means there are no more.
    Worlds {
        /// Published World descriptors in runtime identity order.
        worlds: Vec<WorldDescriptor>,
        /// Next page cursor; zero indicates the final page.
        next: u64,
    },
    /// A fresh logical World session on the existing transport.
    Attached {
        /// Exact World lifetime selected by this session.
        reference: WorldReference,
        /// Target World identity or published descriptor.
        world: WorldDescriptor,
        /// Fresh logical World session identity.
        session: u64,
        /// Actual selected World admission, distinct from the compiled contract.
        manifest: WorldManifest,
    },
    /// Updated World metadata/configuration.
    World(WorldDescriptor),
    /// Completed lifecycle operation.
    Complete,
    /// Operation rejected without closing the Host connection.
    Error(String),
    /// Bounded metadata preview of the exact uploaded graph.
    WorldGraphPage {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Graph-local root identity.
        root: WorldGraphNodeId,
        /// Total node count across all pages.
        total: u32,
        /// Sequential page offset.
        offset: u32,
        /// Graph-local identities and durable metadata.
        nodes: Vec<WorldGraphNodeDescriptor>,
    },
    /// Graph publication succeeded; cancellation still destroys all transfer-owned Worlds.
    WorldGraphLoaded {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Fresh exact root identity, independently observable before opening a session.
        root: WorldReference,
        /// Number of created identities to collect before acknowledging.
        total: u32,
    },
    /// Bounded portion of the complete created-World journal.
    WorldGraphBindings {
        /// Connection-scoped transfer identity.
        job: u64,
        /// Sequential page offset.
        offset: u32,
        /// Graph-local identities and their fresh exact runtime identities.
        bindings: Vec<(WorldGraphNodeId, WorldReference)>,
    },
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
        HOST_REQUEST_READ_WORLD_SAVE => HostRequestBody::ReadWorldSave {
            job: reader.u64()?,
            offset: reader.u64()?,
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
    Ok(())
}

/// Validate and decode a result for the selected Host connection.
pub fn decode_host_response(bytes: &[u8], connection: u64) -> Result<HostResponse, ProtocolError> {
    let mut reader = host_reader(bytes, HOST_RESPONSE_MAGIC, connection)?;
    let request_id = reader.u64()?;
    let body = match reader.u8()? {
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
    let mut writer = Writer::new(Vec::new());
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
