//! Direct ownership of headless services and independently updatable worlds.

use std::collections::BTreeMap;

use crate::systems::{
    SystemFactories, SystemFactory, SystemId, SystemScheduleError, compiled_system_factories,
};
use crate::{ErrorReason, World, WorldConstructionError, WorldContext, WorldLimits};
use std::sync::Arc;

pub(crate) mod attachments;
mod frame;
pub(crate) mod ingress;
mod persistence;
pub(crate) mod publication;
mod queries;
mod reference_fields;
pub(crate) mod reference_resolution;
pub(crate) mod references;
mod service_access;
mod world_lifecycle;

#[cfg(test)]
mod test_support;

pub use crate::systems::world_attachment::WorldAttachment;
pub use attachments::{
    RootBindingGeneration, RootOutputBinding, WorldAttachmentEffect, WorldAttachmentRetirement,
    WorldAttachmentToken,
};
pub use frame::HostFrameReport;
pub use ingress::HostIngressView;
pub use publication::{
    OutputPublicationObservation, PublishedWorldAttachment, WorldDerivedChunk, WorldOutputBuilder,
    WorldPublication, WorldPublicationId,
};
pub use queries::{
    PublishedSceneContribution, PublishedSceneHit, ViewDescriptor, ViewPickHit, ViewQueryTarget,
};
pub use reference_resolution::{OutputReferenceToken, WorldReferenceToken};
pub use references::{
    AttachmentPlacement, OutputKind, OutputRef, OutputTarget, WorldAttachmentMode,
    WorldFrameContext, WorldRef, WorldViewport,
};

/// Host-local world identity. Never reused during the Host lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldId(pub u64);

/// Headless runtime owned directly by a Host alongside its platform services.
/// Worlds are destroyed before services, including during ordinary Rust drop.
pub struct HostRuntime {
    #[cfg(feature = "instrumentation")]
    profile_context: usize,
    worlds: BTreeMap<WorldId, World>,
    next_world: u64,
    identity_namespace: u64,
    system_factories: SystemFactories,
    io: crate::services::io::IoService,
    data: crate::services::data::DataService,
    assets: crate::services::asset_management::service::AssetManagementService,
    topology: attachments::topology::HostTopology,
    publications: publication::HostPublications,
    frame: u64,
}
