//! Completed-publication retention over exact resource incarnations.

use super::{AssetKey, AssetLifecycleEvent, AssetManagementService, AssetProvider};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_SERVICE_IDENTITY: AtomicU64 = AtomicU64::new(1);

pub(super) fn next_service_identity() -> u64 {
    NEXT_SERVICE_IDENTITY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |identity| {
            identity.checked_add(1)
        })
        .expect("asset service identity space exhausted")
}

/// Opaque consumer identity, fenced to one Host asset service instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetPublicationId {
    service: u64,
    sequence: u64,
}

impl AssetManagementService {
    /// Retain decoded resources and their provider-owned immutable recovery sources.
    /// Acquire the replacement publication before releasing the previous one.
    pub fn retain_publication(
        &mut self,
        keys: impl IntoIterator<Item = AssetKey>,
    ) -> Result<AssetPublicationId, String> {
        let keys: BTreeSet<_> = keys.into_iter().collect();
        for &key in &keys {
            let provider = self.get(key).ok_or("Unknown publication resource")?;
            if provider.data().is_none() || !provider.requires_recovery() {
                return Err("Publication resource has no immutable decoded data".into());
            }
            if self
                .pending_releases
                .get(&key)
                .is_some_and(|release| !release.cancelable_orphan())
            {
                return Err("Publication resource is being invalidated".into());
            }
        }
        let sequence = self
            .next_publication
            .checked_add(1)
            .ok_or("Publication identity space exhausted")?;
        let publication = AssetPublicationId {
            service: self.publication_identity,
            sequence,
        };

        self.require_lifecycle_barrier();
        self.next_publication = sequence;
        for &key in &keys {
            self.remove_idle(key);
            if self
                .pending_releases
                .get(&key)
                .is_some_and(|release| release.cancelable_orphan())
            {
                self.pending_releases.remove(&key);
            }
            self.publication_users
                .entry(key)
                .or_default()
                .insert(publication);
        }
        self.publications.insert(publication, keys);
        Ok(publication)
    }

    /// Borrow only the exact resource incarnation retained by this publication.
    /// Explicit release requests make it unavailable before their storage is freed.
    pub fn publication_resource(
        &self,
        publication: AssetPublicationId,
        key: AssetKey,
    ) -> Option<&AssetProvider> {
        if publication.service != self.publication_identity
            || !self.publications.get(&publication)?.contains(&key)
            || self.pending_releases.contains_key(&key)
        {
            return None;
        }
        self.get(key)
    }

    /// Consumers to invalidate synchronously before applying a pending release.
    pub fn affected_publications(&self, event: &AssetLifecycleEvent) -> Vec<AssetPublicationId> {
        self.publication_users
            .get(&event.key)
            .into_iter()
            .flat_map(|users| users.iter().copied())
            .collect()
    }

    /// Retire one publication without changing any other consumer's demand.
    pub fn release_publication(&mut self, publication: AssetPublicationId) -> bool {
        if publication.service != self.publication_identity {
            return false;
        }
        let Some(keys) = self.publications.remove(&publication) else {
            return false;
        };
        for key in keys {
            let Some(users) = self.publication_users.get_mut(&key) else {
                continue;
            };
            users.remove(&publication);
            if users.is_empty() {
                self.publication_users.remove(&key);
                if !self.is_required(key) {
                    self.retain_idle_or_remove(key);
                }
            }
        }
        self.prune_idle();
        true
    }

    pub(super) fn is_required(&self, key: AssetKey) -> bool {
        self.used.contains(&key)
            || self.owned.contains(&key)
            || self.publication_users.contains_key(&key)
    }

    pub(super) fn forget_publication_key(&mut self, key: AssetKey) {
        let Some(users) = self.publication_users.remove(&key) else {
            return;
        };
        for publication in users {
            if let Some(keys) = self.publications.get_mut(&publication) {
                keys.remove(&key);
            }
        }
    }
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
