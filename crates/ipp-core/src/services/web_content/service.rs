use super::*;
use std::collections::{BTreeMap, BTreeSet};

struct Source {
    id: WebContentSourceId,
    validity: Arc<AtomicBool>,
}

struct Target {
    source_name: Arc<str>,
    logical_size: [f32; 2],
    request: Option<WebContentRequestId>,
    image: Option<WebContentImageId>,
    unavailable: bool,
    validity: Arc<AtomicBool>,
}

struct Image {
    request: WebContentRequestId,
    target: WebContentTarget,
    source: WebContentSourceId,
    description: WebContentImageDescription,
    complete: bool,
    publications: usize,
    validity: WebContentImageValidity,
}

/// Host-owned request authority and exact publication retention. It owns no GPU
/// handles, browser objects, callbacks or component values.
pub struct WebContentService {
    namespace: u64,
    next: u64,
    sources: BTreeMap<Arc<str>, Source>,
    targets: BTreeMap<WebContentTarget, Target>,
    requests: BTreeMap<WebContentRequestId, WebContentRequest>,
    reservations: BTreeMap<WebContentRequestId, WebContentImageId>,
    images: BTreeMap<WebContentImageId, Image>,
    publications: BTreeMap<WebContentPublicationLease, BTreeSet<WebContentImageId>>,
    renderer_validity: Arc<AtomicBool>,
}

impl Default for WebContentService {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            namespace: NEXT
                .fetch_update(
                    std::sync::atomic::Ordering::Relaxed,
                    std::sync::atomic::Ordering::Relaxed,
                    |value| value.checked_add(1),
                )
                .expect("web-content service identity space exhausted"),
            next: 0,
            sources: BTreeMap::new(),
            targets: BTreeMap::new(),
            requests: BTreeMap::new(),
            reservations: BTreeMap::new(),
            images: BTreeMap::new(),
            publications: BTreeMap::new(),
            renderer_validity: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl Drop for WebContentService {
    fn drop(&mut self) {
        self.renderer_validity.store(false, Ordering::Release);
    }
}

impl WebContentService {
    fn identity(&mut self) -> u64 {
        self.next = self
            .next
            .checked_add(1)
            .expect("web-content identity space exhausted");
        self.next
    }

    /// Register an adapter-owned source name. Replacing a registration revokes
    /// all old requests and images, including images in frozen publications.
    pub fn register_source(&mut self, name: impl Into<Arc<str>>) -> WebContentSourceId {
        let name = name.into();
        if let Some(old) = self.sources.get(&name).map(|source| source.id) {
            self.revoke_source(old);
        }
        let id = WebContentSourceId(self.namespace, self.identity());
        self.sources.insert(
            name.clone(),
            Source {
                id,
                validity: Arc::new(AtomicBool::new(true)),
            },
        );
        let targets: Vec<_> = self
            .targets
            .iter()
            .filter_map(|(&key, target)| (target.source_name == name).then_some(key))
            .collect();
        for target in targets {
            self.start_request(target);
        }
        id
    }

    /// Withdraw exactly this registration; a stale owner cannot revoke its replacement.
    pub fn revoke_source(&mut self, source: WebContentSourceId) -> bool {
        let Some(name) = self
            .sources
            .iter()
            .find_map(|(name, entry)| (entry.id == source).then(|| name.clone()))
        else {
            return false;
        };
        self.sources
            .remove(&name)
            .expect("registered source")
            .validity
            .store(false, Ordering::Release);
        let targets: Vec<_> = self
            .targets
            .iter()
            .filter_map(|(&key, target)| (target.source_name == name).then_some(key))
            .collect();
        for target in targets {
            self.cancel_request(target);
            if let Some(state) = self.targets.get_mut(&target) {
                state.unavailable = true;
            }
        }
        self.images.retain(|_, image| image.source != source);
        true
    }

    /// Current requests. Adapters reconcile these identities and release canceled producers.
    pub fn requests(&self) -> impl Iterator<Item = &WebContentRequest> {
        self.requests.values()
    }

    /// Reserve authority before private GPU allocation/import. At most one import
    /// reservation exists per request; a failed import must discard it.
    pub fn reserve_image(
        &mut self,
        request: WebContentRequestId,
        description: WebContentImageDescription,
    ) -> Result<WebContentImageId, WebContentError> {
        let live = self
            .requests
            .get(&request)
            .ok_or(WebContentError::StaleRequest)?;
        if description.pixel_size.contains(&0) {
            return Err(WebContentError::InvalidImage);
        }
        if self.reservations.contains_key(&request) {
            return Err(WebContentError::ReservationPending);
        }
        let (target, source) = (live.target, live.source);
        let validity = WebContentImageValidity {
            source: self.sources[&live.source_name].validity.clone(),
            target: self.targets[&target].validity.clone(),
            renderer: self.renderer_validity.clone(),
        };
        let id = WebContentImageId(self.namespace, self.identity());
        self.images.insert(
            id,
            Image {
                request,
                target,
                source,
                description,
                complete: false,
                publications: 0,
                validity,
            },
        );
        self.reservations.insert(request, id);
        Ok(id)
    }

    /// Commit only a matching full-image reservation after import has completed.
    /// This changes service state; it never evaluates or mutates a World.
    pub fn complete_image(
        &mut self,
        request: WebContentRequestId,
        image: WebContentImageId,
    ) -> Result<(), WebContentError> {
        let target = self
            .requests
            .get(&request)
            .ok_or(WebContentError::StaleRequest)?
            .target;
        if self.reservations.get(&request) != Some(&image) {
            return Err(WebContentError::InvalidImage);
        }
        let value = self
            .images
            .get_mut(&image)
            .ok_or(WebContentError::InvalidImage)?;
        value.complete = true;
        self.reservations.remove(&request);
        let state = self.targets.get_mut(&target).expect("live request target");
        state.image = Some(image);
        state.unavailable = false;
        self.collect_images();
        Ok(())
    }

    /// Abandon private import staging. Completed images cannot be discarded through this API.
    pub fn discard_image(&mut self, image: WebContentImageId) {
        if let Some(value) = self.images.get(&image).filter(|value| !value.complete) {
            self.reservations.remove(&value.request);
            self.images.remove(&image);
        }
    }

    /// Report a missing accelerated capability without fabricating an image.
    pub fn unavailable(&mut self, request: WebContentRequestId) -> Result<(), WebContentError> {
        let target = self
            .requests
            .get(&request)
            .ok_or(WebContentError::StaleRequest)?
            .target;
        let state = self.targets.get_mut(&target).expect("live request target");
        state.unavailable = true;
        state.image = None;
        self.collect_images();
        Ok(())
    }

    /// Metadata for a completed image whose authority remains valid.
    pub fn image_description(
        &self,
        image: WebContentImageId,
    ) -> Option<WebContentImageDescription> {
        self.images
            .get(&image)
            .filter(|value| value.complete)
            .map(|value| value.description)
    }

    /// A revocation witness for a completed, retained image. Cloning this witness
    /// does not retain the image or prevent its GPU storage from retiring.
    pub fn image_validity(&self, image: WebContentImageId) -> Option<WebContentImageValidity> {
        self.images
            .get(&image)
            .filter(|value| value.complete)
            .map(|value| value.validity.clone())
    }

    /// Includes import reservations, the latest pending/current image and exact
    /// publication leases. Renderer retirement must not sweep only live components.
    pub fn is_image_retained(&self, image: WebContentImageId) -> bool {
        self.images.contains_key(&image)
    }

    /// Retire all GPU image authority and rotate every live request. A later
    /// completion cannot revive the old context's images or impersonate frozen pixels.
    pub fn renderer_invalidated(&mut self) {
        self.renderer_validity.store(false, Ordering::Release);
        self.renderer_validity = Arc::new(AtomicBool::new(true));
        self.images.clear();
        self.reservations.clear();
        let targets: Vec<_> = self.targets.keys().copied().collect();
        for target in targets {
            self.cancel_request(target);
            self.start_request(target);
        }
    }

    pub(crate) fn reconcile(
        &mut self,
        target: WebContentTarget,
        source_name: Arc<str>,
        logical_size: [f32; 2],
    ) -> WebContentStatus {
        let unchanged = self.targets.get(&target).is_some_and(|state| {
            state.source_name == source_name && state.logical_size == logical_size
        });
        if !unchanged {
            let validity = self.targets.get(&target).map_or_else(
                || Arc::new(AtomicBool::new(true)),
                |state| state.validity.clone(),
            );
            self.cancel_request(target);
            self.targets.insert(
                target,
                Target {
                    source_name,
                    logical_size,
                    request: None,
                    image: None,
                    unavailable: false,
                    validity,
                },
            );
            self.start_request(target);
        }
        if self
            .targets
            .get(&target)
            .is_some_and(|state| state.request.is_none())
        {
            self.start_request(target);
        }
        self.status(target)
    }

    /// Current service state; reading it never advances or evaluates a World.
    pub fn status(&self, target: WebContentTarget) -> WebContentStatus {
        let Some(state) = self.targets.get(&target) else {
            return WebContentStatus::Unavailable;
        };
        if let Some(image) = state
            .image
            .filter(|image| self.image_description(*image).is_some())
        {
            WebContentStatus::Ready(image)
        } else if state.unavailable || state.request.is_none() {
            WebContentStatus::Unavailable
        } else {
            WebContentStatus::Pending
        }
    }

    pub(crate) fn cancel_request(&mut self, target: WebContentTarget) {
        if let Some(state) = self.targets.get_mut(&target) {
            if let Some(request) = state.request.take() {
                self.requests.remove(&request);
                if let Some(image) = self.reservations.remove(&request) {
                    self.images.remove(&image);
                }
            }
            state.image = None;
        }
        self.collect_images();
    }

    pub(crate) fn invalidate_target(&mut self, target: WebContentTarget) {
        if let Some(state) = self.targets.get(&target) {
            state.validity.store(false, Ordering::Release);
        }
        self.cancel_request(target);
        self.targets.remove(&target);
        self.images.retain(|_, image| image.target != target);
    }

    pub(crate) fn invalidate_world(&mut self, world: WorldRef) {
        let targets: Vec<_> = self
            .targets
            .keys()
            .filter(|target| target.world == world)
            .copied()
            .collect();
        for target in targets {
            self.invalidate_target(target);
        }
        self.images.retain(|_, image| image.target.world != world);
    }

    pub(crate) fn retain_publication(
        &mut self,
        images: &BTreeSet<WebContentImageId>,
    ) -> WebContentPublicationLease {
        let lease = WebContentPublicationLease(self.identity());
        let retained = images
            .iter()
            .filter_map(|&id| {
                let image = self.images.get_mut(&id).filter(|image| image.complete)?;
                image.publications += 1;
                Some(id)
            })
            .collect();
        self.publications.insert(lease, retained);
        lease
    }

    pub(crate) fn release_publication(&mut self, lease: WebContentPublicationLease) {
        if let Some(images) = self.publications.remove(&lease) {
            for id in images {
                if let Some(image) = self.images.get_mut(&id) {
                    image.publications -= 1;
                }
            }
        }
        self.collect_images();
    }

    pub(crate) fn publication_image(
        &self,
        lease: WebContentPublicationLease,
        image: WebContentImageId,
    ) -> Option<WebContentImageDescription> {
        self.publications
            .get(&lease)?
            .contains(&image)
            .then_some(())?;
        self.image_description(image)
    }

    pub(crate) fn publication_image_validity(
        &self,
        lease: WebContentPublicationLease,
        image: WebContentImageId,
    ) -> Option<WebContentImageValidity> {
        self.publications
            .get(&lease)?
            .contains(&image)
            .then_some(())?;
        self.image_validity(image)
    }

    fn start_request(&mut self, target: WebContentTarget) {
        let state = self.targets.get(&target).expect("registered target");
        if state
            .logical_size
            .iter()
            .any(|size| !size.is_finite() || *size <= 0.0)
        {
            return;
        }
        let Some(source) = self.sources.get(&state.source_name).map(|source| source.id) else {
            return;
        };
        let (source_name, logical_size) = (state.source_name.clone(), state.logical_size);
        let id = WebContentRequestId(self.namespace, self.identity());
        self.requests.insert(
            id,
            WebContentRequest {
                id,
                target,
                source,
                source_name,
                logical_size,
            },
        );
        let state = self.targets.get_mut(&target).expect("registered target");
        state.request = Some(id);
        state.unavailable = false;
    }

    fn collect_images(&mut self) {
        self.images.retain(|id, image| {
            !image.complete
                || image.publications != 0
                || self
                    .targets
                    .get(&image.target)
                    .is_some_and(|target| target.image == Some(*id))
        });
    }
}
