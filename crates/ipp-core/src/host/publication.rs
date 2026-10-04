//! Immutable evaluated chunks, explicit output producers and exact resource leases.

use crate::services::asset_management::{AssetKey, AssetPublicationId};
use crate::systems::SystemId;
use crate::{EntityId, ErrorReason, OutputRef, WorldAttachmentMode, WorldRef};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Exact completed-publication identity, fenced to its owning Host.
pub struct WorldPublicationId {
    pub(crate) host: u64,
    pub(crate) revision: u64,
}

/// A requested exact output and its observed completed source, if included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputPublicationObservation {
    /// Exact World, entity and output producer lifetime; never follows replacement.
    pub output: OutputRef,
    /// Completed source whose content participated in the observation.
    pub publication: Option<WorldPublicationId>,
}

/// Owned final inputs for one evaluator. The concrete producer owns its typed format.
#[derive(Clone)]
pub struct WorldDerivedChunk {
    version: u64,
    data: Arc<dyn Any + Send + Sync>,
}

impl WorldDerivedChunk {
    /// Revision of this immutable chunk; unchanged values share the preceding chunk.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Borrow the concrete producer's owned immutable format.
    pub fn data<T: Any>(&self) -> Option<&T> {
        self.data.downcast_ref()
    }
}

/// A completed edge, including the current parent-local affine placement.
#[derive(Clone, Debug, PartialEq)]
pub struct PublishedWorldAttachment {
    /// Exact producer-write identity for path validation and conditional cleanup.
    pub token: super::WorldAttachmentToken,
    /// Parent-local attachment entity.
    pub anchor: EntityId,
    /// Exact independently living child World.
    pub child: WorldRef,
    /// Explicit spatial or Surface domain selection.
    pub mode: WorldAttachmentMode,
    /// Selected child output, absent for spatial contributions.
    pub output: Option<OutputRef>,
    /// Parent output owning derived placement, distinct from the selected child output.
    /// Consumers may present this edge only inside that exact output's content.
    pub placement_output: Option<OutputRef>,
    /// Final parent-local affine placement.
    pub placement: [f64; 16],
    /// Physical Surface dimensions, never child Canvas logical units.
    pub surface_extent: Option<[f64; 2]>,
    /// Complete immutable Surface mapping.
    pub surface_geometry: Option<crate::systems::surface::SurfaceGeometry>,
    /// Exact provider lifetime; replacement invalidates retained paths.
    pub surface_incarnation: Option<u64>,
    /// Validated effective parent Surface policy, independent of child paint.
    /// Only optional Canvas caching consumes this policy, not Camera target selection.
    pub surface_cache_policy: Option<crate::systems::surface::SurfaceCachePolicy>,
    /// The parent Surface's `layer_spacing` in metres per canvas layer id;
    /// zero without a Surface. It separates layers only where the edge is
    /// presented in a camera's 3D domain.
    pub layer_spacing: f32,
    /// Last completed child contribution, absent before readiness.
    pub publication: Option<WorldPublicationId>,
}

/// Completed data contains neither component borrows nor mutable World access.
pub struct WorldPublication {
    /// Unique immutable publication identity.
    pub id: WorldPublicationId,
    /// Producing World lifetime.
    pub world: WorldRef,
    /// Completed World evaluation tick.
    pub tick: u64,
    /// Completed World simulation time.
    pub time: f64,
    /// Completed edges assembled after descendant evaluation.
    pub attachments: Vec<PublishedWorldAttachment>,
    pub(crate) chunks: BTreeMap<SystemId, WorldDerivedChunk>,
    pub(crate) outputs: BTreeMap<OutputRef, WorldDerivedChunk>,
    pub(crate) resources: BTreeSet<AssetKey>,
}

impl WorldPublication {
    /// Read a selected evaluator's completed derived data.
    pub fn chunk(&self, system: SystemId) -> Option<&WorldDerivedChunk> {
        self.chunks.get(&system)
    }

    /// Read a captured output; Host output access additionally checks live binding validity.
    pub fn output(&self, selection: OutputRef) -> Option<&WorldDerivedChunk> {
        self.outputs.get(&selection)
    }

    /// Exact resource identities retained by this publication.
    pub fn resources(&self) -> impl Iterator<Item = AssetKey> + '_ {
        self.resources.iter().copied()
    }
}

/// Scoped producer sink. Typed immutable chunks share unchanged completed storage.
pub struct WorldOutputBuilder<'a> {
    pub(crate) previous: Option<&'a WorldPublication>,
    pub(crate) version: u64,
    pub(crate) chunks: BTreeMap<SystemId, WorldDerivedChunk>,
    pub(crate) outputs: BTreeMap<OutputRef, WorldDerivedChunk>,
    pub(crate) resources: BTreeSet<AssetKey>,
}

impl WorldOutputBuilder<'_> {
    /// Publish owned typed derived data for one evaluator.
    pub fn chunk<T: Any + Send + Sync + PartialEq>(&mut self, producer: SystemId, data: T) {
        let previous = self
            .previous
            .and_then(|previous| previous.chunks.get(&producer));
        self.chunks
            .insert(producer, Self::share(previous, self.version, data));
    }

    /// Publish a concrete output bound to its exact producer incarnation.
    pub fn output<T: Any + Send + Sync + PartialEq>(&mut self, selection: OutputRef, data: T) {
        let previous = self
            .previous
            .and_then(|previous| previous.outputs.get(&selection));
        self.outputs
            .insert(selection, Self::share(previous, self.version, data));
    }

    /// Require an exact decoded resource and its immutable recovery source.
    pub fn retain(&mut self, resource: AssetKey) {
        self.resources.insert(resource);
    }

    fn share<T: Any + Send + Sync + PartialEq>(
        previous: Option<&WorldDerivedChunk>,
        version: u64,
        data: T,
    ) -> WorldDerivedChunk {
        if let Some(previous) = previous.filter(|previous| previous.data::<T>() == Some(&data)) {
            return previous.clone();
        }
        WorldDerivedChunk {
            version,
            data: Arc::new(data),
        }
    }
}

pub(crate) struct RetainedPublication {
    pub data: WorldPublication,
    pub lease: AssetPublicationId,
    pub available: bool,
    pub readers: std::rc::Rc<()>,
}

pub(crate) struct PublicationReadLease {
    _reader: std::rc::Rc<()>,
}

pub(crate) struct HostPublications {
    pub identity: u64,
    pub next: u64,
    pub completed: BTreeMap<WorldPublicationId, RetainedPublication>,
    pub latest: BTreeMap<crate::WorldId, WorldPublicationId>,
}

impl Default for HostPublications {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            identity: NEXT
                .fetch_update(
                    std::sync::atomic::Ordering::Relaxed,
                    std::sync::atomic::Ordering::Relaxed,
                    |identity| identity.checked_add(1),
                )
                .expect("Host publication identity space exhausted"),
            next: 0,
            completed: BTreeMap::new(),
            latest: BTreeMap::new(),
        }
    }
}

impl super::HostRuntime {
    /// Capture the next evaluation cut without evaluating or rebinding an output.
    pub fn output_evaluation_cut(&self, output: OutputRef) -> Result<u64, ErrorReason> {
        let world = self
            .worlds
            .get(&output.world().id())
            .ok_or(ErrorReason::InvalidEntity)?;

        if !world.output_valid(output) {
            return Err(ErrorReason::InvalidEntity);
        }

        world.next_evaluation_tick().ok_or(ErrorReason::Capacity)
    }

    /// Read only a still-available completed contribution, never live component state.
    pub fn publication(&self, id: WorldPublicationId) -> Option<&WorldPublication> {
        self.publications
            .completed
            .get(&id)
            .filter(|entry| entry.available)
            .map(|entry| &entry.data)
    }

    /// Last available completed output, including one retained after a failed publication.
    pub fn latest_publication(&self, world: crate::WorldId) -> Option<WorldPublicationId> {
        let id = *self.publications.latest.get(&world)?;
        self.publication(id).map(|_| id)
    }

    fn attachment_binding_valid(&self, edge: &PublishedWorldAttachment) -> bool {
        if self.attachment_retirement(&edge.token) != Ok(super::WorldAttachmentRetirement::Pending)
        {
            return false;
        }

        edge.placement_output.is_none_or(|owner| {
            owner.world() == edge.token.parent()
                && self
                    .worlds
                    .get(&owner.world().id())
                    .is_some_and(|world| world.output_valid(owner))
        })
    }

    /// Resolve an edge while its child and both output lifetimes remain usable.
    /// Retired write identities never recover, even if their output becomes valid again.
    /// This validates availability, not permission to present it in another output's domain.
    pub fn attached_publication(
        &self,
        edge: &PublishedWorldAttachment,
    ) -> Option<&WorldPublication> {
        if !self.attachment_binding_valid(edge) {
            return None;
        }
        let publication = self.publication(edge.publication?)?;
        if publication.world != edge.child {
            return None;
        }
        if edge.mode != WorldAttachmentMode::Spatial {
            edge.surface_extent?;
            let geometry = edge.surface_geometry.as_ref()?;
            let parent = self
                .worlds
                .get(&edge.token.parent().id())?
                .ingress_world_view();
            crate::systems::surface::publication_is_current(
                parent,
                edge.anchor,
                geometry,
                edge.surface_incarnation,
            )
            .then_some(())?;
            self.output(publication.id, edge.output?)?;
        }
        Some(publication)
    }

    /// Borrow only an exact resource retained by this still-valid publication.
    pub fn publication_resource(
        &self,
        publication: WorldPublicationId,
        resource: AssetKey,
    ) -> Option<&crate::services::asset_management::AssetProvider> {
        let entry = self
            .publications
            .completed
            .get(&publication)
            .filter(|entry| entry.available)?;
        self.assets.publication_resource(entry.lease, resource)
    }

    pub(crate) fn invalidate_publication_resources(
        &mut self,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        if event.kind == crate::services::asset_management::AssetLifecycleKind::GraphicsInvalidated
        {
            return;
        }
        let affected: BTreeSet<_> = self
            .assets
            .affected_publications(event)
            .into_iter()
            .collect();
        for entry in self.publications.completed.values_mut() {
            if affected.contains(&entry.lease) {
                entry.available = false;
            }
        }
        if !affected.is_empty() {
            self.retire_publications();
        }
    }

    pub(crate) fn retire_publications(&mut self) {
        let mut retained = BTreeSet::new();
        let mut pending: Vec<_> = self.publications.latest.values().copied().collect();
        pending.extend(
            self.publications
                .completed
                .iter()
                .filter_map(|(id, entry)| {
                    (std::rc::Rc::strong_count(&entry.readers) > 1).then_some(*id)
                }),
        );
        while let Some(id) = pending.pop() {
            if !retained.insert(id) {
                continue;
            }
            if let Some(entry) = self
                .publications
                .completed
                .get(&id)
                .filter(|entry| entry.available)
            {
                pending.extend(
                    entry
                        .data
                        .attachments
                        .iter()
                        .filter_map(|edge| self.attached_publication(edge).map(|child| child.id)),
                );
            }
        }
        let retired: Vec<_> = self
            .publications
            .completed
            .keys()
            .filter(|id| !retained.contains(id))
            .copied()
            .collect();
        for id in retired {
            if let Some(entry) = self.publications.completed.remove(&id) {
                self.assets.release_publication(entry.lease);
            }
        }
        let published_tokens = self
            .publications
            .completed
            .values()
            .filter(|entry| entry.available)
            .flat_map(|entry| {
                entry
                    .data
                    .attachments
                    .iter()
                    .filter(|edge| self.attachment_binding_valid(edge))
                    .map(|edge| (edge.token.identity(), edge.token.clone()))
            })
            .collect();
        self.topology.retain_published_tokens(published_tokens);
        let published = self
            .publications
            .completed
            .values()
            .filter(|entry| entry.available)
            .flat_map(|entry| {
                entry
                    .data
                    .attachments
                    .iter()
                    .filter(|edge| self.attachment_binding_valid(edge))
                    .map(|edge| {
                        (
                            super::topology::AttachmentAnchor {
                                world: entry.data.world.id,
                                entity: edge.anchor,
                            },
                            edge.child.id,
                        )
                    })
            })
            .collect();
        self.topology.retire_detaches(&published);
    }

    pub(crate) fn retain_publication_read(
        &self,
        id: WorldPublicationId,
    ) -> Option<PublicationReadLease> {
        let entry = self
            .publications
            .completed
            .get(&id)
            .filter(|entry| entry.available)?;
        Some(PublicationReadLease {
            _reader: entry.readers.clone(),
        })
    }

    pub(crate) fn next_publication(&mut self) -> Result<WorldPublicationId, ErrorReason> {
        self.publications.next = self
            .publications
            .next
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        Ok(WorldPublicationId {
            host: self.publications.identity,
            revision: self.publications.next,
        })
    }
}
