//! Short-lived authoritative reads for an owning System's command validation.

use super::publication::{HostPublications, RetainedPublication};
use super::reference_resolution::ReferenceWorlds;
use super::topology::HostTopology;
use super::*;
use crate::services::asset_management::{AssetKey, AssetManagementService, AssetProvider};
use crate::systems::SystemWorldView;
use std::collections::BTreeSet;

#[cfg(test)]
#[path = "ingress_tests.rs"]
mod tests;

/// Read-only reborrow at one command boundary. This is neither a guard nor presentation authority.
/// The owning System validates before its first mutation and drops this view before writing.
pub struct HostIngressView<'a> {
    pub(crate) local: SystemWorldView<'a>,
    pub(crate) local_outputs: crate::world::composition::WorldOutputs,
    pub(crate) declared: &'a BTreeSet<WorldRef>,
    pub(crate) foreign: Option<&'a ReferenceWorlds<'a>>,
    pub(crate) topology: &'a HostTopology,
    pub(crate) publications: &'a HostPublications,
    pub(crate) assets: &'a AssetManagementService,
}

impl HostIngressView<'_> {
    /// Current completed source for a declared World. This does not authorize an output or path.
    pub fn latest_publication(&self, world: WorldRef) -> Option<&WorldPublication> {
        self.world(world)?;
        self.publication(*self.publications.latest.get(&world.id())?)
    }

    /// Current target or an exact foreign lifetime declared by this command, never a sibling command.
    pub fn world(&self, world: WorldRef) -> Option<SystemWorldView<'_>> {
        if self.local.world_ref() == world {
            return Some(self.local);
        }
        self.declared.contains(&world).then_some(())?;
        Some(self.foreign?.get(world)?.ingress_world_view())
    }

    /// Authoritative root identity and dimensions; output availability is checked separately.
    pub fn root_binding(&self, world: WorldRef) -> Option<RootOutputBinding> {
        self.world(world)?;
        self.topology.roots.get(&world.id()).copied()
    }

    /// Exact selected producer membership and current active component incarnation.
    pub fn output_is_live(&self, output: OutputRef) -> bool {
        if output.world() == self.local.world_ref() {
            return self.local.output_matches(output, self.local_outputs);
        }
        self.declared.contains(&output.world())
            && self
                .foreign
                .and_then(|foreign| foreign.get(output.world()))
                .is_some_and(|world| world.output_valid(output))
    }

    /// Current producer edge, not a superseded token retained by historical presentation.
    pub fn attachment_is_current(&self, token: &WorldAttachmentToken) -> bool {
        let Some(parent) = self.world(token.parent()) else {
            return false;
        };
        let Some(child) = token.child().filter(|child| self.world(*child).is_some()) else {
            return false;
        };
        let anchor = token.location();
        token.identity().0 == self.topology.identity
            && self.topology.tokens.get(&anchor) == Some(token)
            && self.topology.desired.get(&anchor) == Some(&child)
            && self.topology.incoming.get(&child.id()) == Some(&anchor)
            && parent.entity_link_valid(token.anchor())
            && parent.component_is_active(token.anchor(), crate::ComponentValue::WORLD_ATTACHMENT)
    }

    fn retained(&self, id: WorldPublicationId) -> Option<&RetainedPublication> {
        let entry = self
            .publications
            .completed
            .get(&id)
            .filter(|entry| entry.available)?;
        self.world(entry.data.world)?;
        entry
            .data
            .resources
            .iter()
            .all(|key| {
                self.assets
                    .publication_resource(entry.lease, *key)
                    .is_some()
            })
            .then_some(entry)
    }

    /// Still-available source data, not necessarily the latest publication. No old Arc implies readiness.
    pub fn publication(&self, id: WorldPublicationId) -> Option<&WorldPublication> {
        Some(&self.retained(id)?.data)
    }

    /// Current output lifetime plus still-available immutable source data.
    pub fn output(&self, id: WorldPublicationId, output: OutputRef) -> Option<&WorldDerivedChunk> {
        self.output_is_live(output).then_some(())?;
        let publication = self.publication(id)?;
        (publication.world == output.world()).then_some(())?;
        publication.output(output)
    }

    /// Available contribution through a current edge. The owning System additionally validates
    /// the containing output, complete path, context and its local producer eligibility.
    pub fn attached_publication(
        &self,
        edge: &PublishedWorldAttachment,
    ) -> Option<&WorldPublication> {
        self.attachment_is_current(&edge.token).then_some(())?;
        (edge.token.child() == Some(edge.child)).then_some(())?;
        if let Some(owner) = edge.placement_output {
            (owner.world() == edge.token.parent() && self.output_is_live(owner)).then_some(())?;
        }
        let publication = self.publication(edge.publication?)?;
        (publication.world == edge.child).then_some(())?;
        if edge.mode != WorldAttachmentMode::Spatial {
            edge.surface_extent?;
            self.output(publication.id, edge.output?)?;
        }
        Some(publication)
    }

    /// Exact CPU/source lease access; pending explicit release is already unavailable.
    pub fn publication_resource(
        &self,
        id: WorldPublicationId,
        key: AssetKey,
    ) -> Option<&AssetProvider> {
        let entry = self.retained(id)?;
        self.assets.publication_resource(entry.lease, key)
    }
}
