//! Direct ownership of headless services and independently updatable worlds.

use std::collections::BTreeMap;

use crate::systems::{
    SystemFactories, SystemFactory, SystemId, SystemScheduleError, compiled_system_factories,
};
use crate::{ErrorReason, World, WorldConstructionError, WorldContext, WorldLimits};
use std::sync::Arc;

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
    topology: topology::HostTopology,
    publications: publication::HostPublications,
    frame: u64,
}

impl Default for HostRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HostRuntime {
    fn drop(&mut self) {
        while let Some(id) = self.worlds.keys().next().copied() {
            self.destroy_world(id);
        }
        #[cfg(feature = "instrumentation")]
        crate::profiling::retire_world(self.profile_context);
    }
}

mod graph_capture;
mod graph_restore;
mod persistence;

mod runtime;

pub(crate) mod attachment;
mod attachment_tokens;
mod reference_fields;
pub(crate) mod reference_resolution;
pub use crate::systems::world_attachment::WorldAttachment;
pub use attachment::{
    AttachmentPlacement, OutputKind, OutputRef, OutputTarget, WorldAttachmentMode,
    WorldFrameContext, WorldRef, WorldViewport,
};
pub use attachment_tokens::{
    WorldAttachmentEffect, WorldAttachmentRetirement, WorldAttachmentToken,
};
pub use reference_resolution::{OutputReferenceToken, WorldReferenceToken};

pub(crate) mod publication;
pub(crate) mod topology;
pub use publication::{
    OutputPublicationObservation, PublishedWorldAttachment, WorldDerivedChunk, WorldOutputBuilder,
    WorldPublication, WorldPublicationId,
};
mod frame;
pub use frame::HostFrameReport;
mod root_binding;
pub use root_binding::{RootBindingGeneration, RootOutputBinding};
pub(crate) mod ingress;
pub use ingress::HostIngressView;
mod scene;
pub use scene::{PublishedSceneContribution, PublishedSceneHit};
mod plot_queries;
mod view_queries;
pub use view_queries::{ViewDescriptor, ViewPickHit, ViewQueryTarget};
