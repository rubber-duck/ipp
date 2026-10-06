//! Host connection control, separate from World-scoped authoring envelopes.

pub mod asset_export;
pub mod gui_input;
pub mod presentation;
pub mod profiling;

mod requests;
mod responses;
mod worlds;

#[cfg(test)]
mod codec_tests;

pub use requests::{decode_host_request, encode_host_request};
pub use responses::{
    decode_host_response, encode_host_response, encode_host_response_into,
    encoded_host_response_size,
};
pub use worlds::{WorldCreateOptions, WorldManifest, accept_world_hello};

use crate::contract::wire_manifest::{HOST_REQUEST_MAGIC_HEX, HOST_RESPONSE_MAGIC_HEX, host_magic};
use crate::host::presentation::{PresentationRequest, PresentationResponse, RootBinding};
use crate::references::{OutputReference, WorldReference};
use ipp_core::services::world_serialization::{WorldGraphNodeDescriptor, WorldGraphNodeId};
use ipp_core::{WorldDescriptor, WorldSelector};
use std::collections::BTreeMap;

/// Host request marker and independent control format revision.
pub const HOST_REQUEST_MAGIC: &[u8; 8] = &host_magic(HOST_REQUEST_MAGIC_HEX);
/// Host response marker; World replies keep their existing fenced envelope.
pub const HOST_RESPONSE_MAGIC: &[u8; 8] = &host_magic(HOST_RESPONSE_MAGIC_HEX);

/// Metadata and rename pages fit even when every name reaches its wire byte limit.
pub const MAX_GRAPH_METADATA_PAGE: usize = 8;
/// Binding pages bound delivery independently of graph size and retained World count.
pub const MAX_GRAPH_BINDING_PAGE: usize = 1024;

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
    /// Authorized original source and explicit typed semantic export controls.
    AssetExport(crate::host::asset_export::AssetExportRequest),
    /// Optional Host measurement controls; production explicitly reports unavailability.
    Profile(crate::host::profiling::ProfileRequest),
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
    GuiInput(crate::host::gui_input::GuiPhysicalRequest),
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
    /// Authority or delivery reference for an explicit asset export operation.
    AssetExport(crate::host::asset_export::AssetExportResponse),
    /// Optional profiling status or an immutable diagnostic artifact page.
    Profile(crate::host::profiling::ProfileResponse),
    /// Current root configuration; not a presentation fence.
    RootBinding(Option<RootBinding>),
    /// Surface-scoped presentation control or completion.
    Presentation(PresentationResponse),
    /// Physical input context or settled routing result; never a frame completion.
    GuiInput(crate::host::gui_input::GuiPhysicalResponse),
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
    /// Detached completed output published through the common bulk data plane.
    Read {
        /// Connection-owned immutable read reference.
        reference: crate::bulk_read::BulkReadReference,
        /// Exact length when known.
        length: Option<u64>,
    },
}
