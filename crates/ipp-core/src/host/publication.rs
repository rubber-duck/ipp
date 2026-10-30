//! Immutable evaluated chunks, explicit output producers and exact resource leases.

use crate::services::asset_management::{AssetKey, AssetPublicationId};
use crate::services::web_content::{
    WebContentImageDescription, WebContentImageId, WebContentImageValidity,
    WebContentPublicationLease,
};
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
    pub(crate) content: Arc<WorldPublicationContent>,
}

#[derive(Clone, Default)]
pub(crate) struct WorldPublicationContent {
    pub(crate) chunks: BTreeMap<SystemId, WorldDerivedChunk>,
    pub(crate) outputs: BTreeMap<OutputRef, WorldDerivedChunk>,
    pub(crate) resources: BTreeSet<AssetKey>,
    pub(crate) web_images: BTreeSet<WebContentImageId>,
}

impl WorldPublication {
    /// Read a selected evaluator's completed derived data.
    pub fn chunk(&self, system: SystemId) -> Option<&WorldDerivedChunk> {
        self.content.chunks.get(&system)
    }

    /// Read a captured output; Host output access additionally checks live binding validity.
    pub fn output(&self, selection: OutputRef) -> Option<&WorldDerivedChunk> {
        self.content.outputs.get(&selection)
    }

    /// Exact resource identities retained by this publication.
    pub fn resources(&self) -> impl Iterator<Item = AssetKey> + '_ {
        self.content.resources.iter().copied()
    }
}

/// Scoped producer sink. Typed immutable chunks share unchanged completed storage.
pub struct WorldOutputBuilder<'a> {
    pub(crate) previous: Option<&'a WorldPublication>,
    pub(crate) version: u64,
    pub(crate) changed: Option<WorldPublicationContent>,
    pub(crate) scratch: &'a mut PublicationScratch,
}

#[derive(Default)]
pub(crate) struct PublicationScratch {
    candidates: BTreeMap<SystemId, Option<Box<dyn Any + Send + Sync>>>,
    chunks: Vec<SystemId>,
    outputs: Vec<OutputRef>,
    resources: Vec<AssetKey>,
    web_images: Vec<WebContentImageId>,
}

impl PublicationScratch {
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.outputs.clear();
        self.resources.clear();
        self.web_images.clear();
    }
}

impl WorldOutputBuilder<'_> {
    /// Refill derived scratch storage, cloning only a changed immutable chunk.
    pub(crate) fn update_chunk<T: Any + Send + Sync + Default + Clone + PartialEq>(
        &mut self,
        producer: SystemId,
        update: impl FnOnce(&mut T, &mut Self) -> Result<(), ErrorReason>,
    ) -> Result<(), ErrorReason> {
        let mut candidate = self
            .scratch
            .candidates
            .entry(producer)
            .or_default()
            .take()
            .unwrap_or_else(|| Box::<T>::default());
        let data = candidate
            .downcast_mut::<T>()
            .expect("stable producer format");
        let result = update(data, self);
        if result.is_ok() {
            self.scratch.chunks.push(producer);
            if self
                .previous
                .and_then(|previous| previous.chunk(producer))
                .and_then(|previous| previous.data::<T>())
                != Some(data)
            {
                let chunk = WorldDerivedChunk {
                    version: self.version,
                    data: Arc::new(data.clone()),
                };
                self.changed().chunks.insert(producer, chunk);
            }
        }
        *self.scratch.candidates.get_mut(&producer).unwrap() = Some(candidate);
        result
    }

    fn changed(&mut self) -> &mut WorldPublicationContent {
        self.changed.get_or_insert_with(|| {
            self.previous
                .map(|previous| (*previous.content).clone())
                .unwrap_or_default()
        })
    }

    /// Publish owned typed derived data for one evaluator.
    pub fn chunk<T: Any + Send + Sync + PartialEq>(&mut self, producer: SystemId, data: T) {
        self.scratch.chunks.push(producer);
        let previous = self.previous.and_then(|previous| previous.chunk(producer));
        if previous.is_some_and(|previous| previous.data::<T>() == Some(&data)) {
            return;
        }
        let chunk = WorldDerivedChunk {
            version: self.version,
            data: Arc::new(data),
        };
        self.changed().chunks.insert(producer, chunk);
    }

    /// Publish a concrete output bound to its exact producer incarnation.
    pub fn output<T: Any + Send + Sync + PartialEq>(&mut self, selection: OutputRef, data: T) {
        self.scratch.outputs.push(selection);
        let previous = self
            .previous
            .and_then(|previous| previous.output(selection));
        if previous.is_some_and(|previous| previous.data::<T>() == Some(&data)) {
            return;
        }
        let chunk = WorldDerivedChunk {
            version: self.version,
            data: Arc::new(data),
        };
        self.changed().outputs.insert(selection, chunk);
    }

    /// Require an exact decoded resource and its immutable recovery source.
    pub fn retain(&mut self, resource: AssetKey) {
        self.scratch.resources.push(resource);
    }

    /// Retain one exact immutable web image in this completed output.
    pub fn retain_web_image(&mut self, image: WebContentImageId) {
        self.scratch.web_images.push(image);
    }

    pub(crate) fn finish(mut self) -> Arc<WorldPublicationContent> {
        self.scratch.chunks.sort_unstable();
        self.scratch.chunks.dedup();
        self.scratch.outputs.sort_unstable();
        self.scratch.outputs.dedup();
        self.scratch.resources.sort_unstable();
        self.scratch.resources.dedup();
        self.scratch.web_images.sort_unstable();
        self.scratch.web_images.dedup();
        let previous = self.previous.map(|previous| &*previous.content);
        let same_keys = previous.is_some_and(|previous| {
            previous.chunks.keys().eq(self.scratch.chunks.iter())
                && previous.outputs.keys().eq(self.scratch.outputs.iter())
                && previous.resources.iter().eq(self.scratch.resources.iter())
                && previous
                    .web_images
                    .iter()
                    .eq(self.scratch.web_images.iter())
        });
        if self.changed.is_none() && same_keys {
            return self.previous.unwrap().content.clone();
        }
        self.changed();
        let mut content = self.changed.take().unwrap();
        content
            .chunks
            .retain(|key, _| self.scratch.chunks.binary_search(key).is_ok());
        content
            .outputs
            .retain(|key, _| self.scratch.outputs.binary_search(key).is_ok());
        if !content.resources.iter().eq(self.scratch.resources.iter()) {
            content.resources = self.scratch.resources.iter().copied().collect();
        }
        if !content.web_images.iter().eq(self.scratch.web_images.iter()) {
            content.web_images = self.scratch.web_images.iter().copied().collect();
        }
        Arc::new(content)
    }
}

pub(crate) struct RetainedPublication {
    pub data: WorldPublication,
    pub lease: AssetPublicationId,
    pub web_lease: WebContentPublicationLease,
    pub available: bool,
    pub readers: std::rc::Rc<()>,
    pub lease_readers: Option<std::rc::Rc<()>>,
    pub retained: bool,
}

pub(crate) struct PublicationReadLease {
    _reader: std::rc::Rc<()>,
}

pub(crate) struct HostPublications {
    pub identity: u64,
    pub next: u64,
    pub completed: super::publication_storage::PublicationEntries,
    pub recycled: Vec<RetainedPublication>,
    pub pending: Vec<WorldPublicationId>,
    pub published_tokens: Vec<((u64, u64), super::WorldAttachmentToken)>,
    pub published_attachments: Vec<(
        super::attachments::topology::AttachmentAnchor,
        crate::WorldId,
    )>,
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
            completed: Default::default(),
            recycled: Vec::new(),
            pending: Vec::new(),
            published_tokens: Vec::new(),
            published_attachments: Vec::new(),
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

    /// Validate exact image authority under a still-available publication lease.
    pub fn publication_web_image(
        &self,
        publication: WorldPublicationId,
        image: WebContentImageId,
    ) -> Option<WebContentImageDescription> {
        let entry = self
            .publications
            .completed
            .get(&publication)
            .filter(|entry| entry.available)?;
        self.web_content.publication_image(entry.web_lease, image)
    }

    /// Obtain a revocation witness under an exact, still-available publication
    /// lease. The witness can outlive ordinary publication and image retirement.
    pub fn publication_web_image_validity(
        &self,
        publication: WorldPublicationId,
        image: WebContentImageId,
    ) -> Option<WebContentImageValidity> {
        let entry = self
            .publications
            .completed
            .get(&publication)
            .filter(|entry| entry.available)?;
        self.web_content
            .publication_image_validity(entry.web_lease, image)
    }

    /// Exact selected image identities, including images since revoked or lost.
    /// Render cache stamps must pair each identity with its current validity.
    pub fn publication_web_images(
        &self,
        publication: WorldPublicationId,
    ) -> Option<&BTreeSet<WebContentImageId>> {
        Some(&self.publication(publication)?.content.web_images)
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
        let mut pending = std::mem::take(&mut self.publications.pending);
        pending.clear();
        pending.extend(self.publications.latest.values().copied());
        for (id, entry) in self.publications.completed.iter() {
            if std::rc::Rc::strong_count(&entry.readers) > 1 {
                pending.push(*id);
            }
        }
        for entry in self.publications.completed.values_mut() {
            entry.retained = false;
        }
        while let Some(id) = pending.pop() {
            let Some(entry) = self.publications.completed.get_mut(&id) else {
                continue;
            };
            if entry.retained {
                continue;
            }
            entry.retained = true;
            if entry.available {
                let entry = self.publications.completed.get(&id).unwrap();
                pending.extend(
                    entry
                        .data
                        .attachments
                        .iter()
                        .filter_map(|edge| self.attached_publication(edge).map(|child| child.id)),
                );
            }
        }
        pending.extend(
            self.publications
                .completed
                .iter()
                .filter_map(|(id, entry)| (!entry.retained).then_some(*id)),
        );
        for id in pending.drain(..) {
            let mut entry = self.publications.completed.remove(&id).unwrap();
            if let Some(readers) = entry.lease_readers.take()
                && std::rc::Rc::strong_count(&readers) == 1
            {
                self.assets.release_publication(entry.lease);
                self.web_content.release_publication(entry.web_lease);
            }
            if self.worlds.contains_key(&entry.data.world.id)
                && self.publications.recycled.len() < self.worlds.len() * 2
            {
                // Reuse capacity without retaining retired attachment receipts.
                entry.data.attachments.clear();
                self.publications.recycled.push(entry);
            }
        }
        self.publications
            .recycled
            .retain(|entry| self.worlds.contains_key(&entry.data.world.id));
        self.publications.pending = pending;
        let mut tokens = std::mem::take(&mut self.publications.published_tokens);
        let mut published = std::mem::take(&mut self.publications.published_attachments);
        tokens.clear();
        published.clear();
        for entry in self
            .publications
            .completed
            .values()
            .filter(|entry| entry.available)
        {
            for edge in &entry.data.attachments {
                if self.attachment_binding_valid(edge) {
                    tokens.push((edge.token.identity(), edge.token.clone()));
                    published.push((
                        super::attachments::topology::AttachmentAnchor {
                            world: entry.data.world.id,
                            entity: edge.anchor,
                        },
                        edge.child.id,
                    ));
                }
            }
        }
        tokens.sort_unstable_by_key(|(identity, _)| *identity);
        tokens.dedup_by_key(|(identity, _)| *identity);
        published.sort_unstable();
        published.dedup();
        self.topology.retain_published_tokens(&tokens);
        self.topology.retire_detaches(&published);
        self.publications.published_tokens = tokens;
        self.publications.published_attachments = published;
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
