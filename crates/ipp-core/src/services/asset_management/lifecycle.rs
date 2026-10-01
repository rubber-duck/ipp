//! Resource-owned transitions and the synchronous Host release barrier.

use super::{AssetKey, AssetLoadProgress, AssetLoadStatus, AssetManagementService, AssetSource};

/// A requested change to retained resource storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssetReleaseKind {
    /// Drop graphics storage only; CPU payload and identity remain available.
    Graphics,
    /// Drop the payload while preserving the resource identity for recovery.
    Unload,
    /// Destroy the resource identity and make its slot reusable.
    Remove,
    /// Explicit destruction defeats renewed demand and publication retention.
    Revoke,
}

#[derive(Clone, Copy)]
pub(super) struct PendingAssetRelease {
    pub(super) kind: AssetReleaseKind,
    pub(super) revision: u64,
    pub(super) cancelable: bool,
}

impl PendingAssetRelease {
    pub(super) fn cancelable_orphan(self) -> bool {
        self.kind == AssetReleaseKind::Remove && self.cancelable
    }
}

/// Applied resource transitions distinguish identity lifetime from residency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetLifecycleKind {
    /// Graphics residency changes without invalidating CPU availability.
    GraphicsInvalidated,
    /// The resource reports new loading or residency state.
    StatusChanged,
    /// The identity has been invalidated after all consumers released it.
    Removed,
}

/// Owned semantic observation, independent of asynchronous client delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetLifecycleEvent {
    /// Representation observation after the transition.
    pub representation: super::AssetRepresentationStatus,
    /// Generational identity of the affected resource.
    pub key: AssetKey,
    /// Immutable resource source, including its producer namespace.
    pub source: AssetSource,
    /// Applied transition type.
    pub kind: AssetLifecycleKind,
    /// Resource status after the transition.
    pub status: AssetLoadStatus,
    pub(crate) release_revision: Option<u64>,
}

impl AssetManagementService {
    /// The current status still belongs to storage awaiting the Host release barrier.
    pub(crate) fn has_pending_release(&self, key: AssetKey) -> bool {
        self.pending_releases.contains_key(&key)
    }

    /// A Host enables the barrier before exposing its services to Worlds or renderers.
    pub(crate) fn require_lifecycle_barrier(&mut self) {
        self.lifecycle_barrier = true;
    }

    pub(super) fn defer_release(&mut self, key: AssetKey, kind: AssetReleaseKind) -> bool {
        if !self.lifecycle_barrier {
            return false;
        }
        if self.get(key).is_some_and(|provider| {
            kind == AssetReleaseKind::Remove
                || kind == AssetReleaseKind::Revoke
                || provider.decoded_available()
                || *provider.status() != AssetLoadStatus::Unloaded
        }) {
            if let Some(provider) = self.get(key) {
                let recipients: std::collections::BTreeSet<_> = self
                    .consumers
                    .iter()
                    .filter_map(|(&world, consumer)| {
                        (consumer.observed_sources.contains(provider.source())
                            || (consumer.owned.contains(&key) && !consumer.internal.contains(&key)))
                        .then_some(world)
                    })
                    .collect();
                // Later Worlds may reconcile after earlier consumers drained
                // their ordinary events. Keep every recipient until delivery.
                self.lifecycle_recipients
                    .entry(key)
                    .or_default()
                    .extend(recipients);
            }
            let previous = self.pending_releases.get(&key).copied();
            if previous.is_none_or(|pending| kind > pending.kind) {
                let revision = self
                    .next_release_revision
                    .checked_add(1)
                    .expect("asset release revision space exhausted");
                self.next_release_revision = revision;
                self.pending_releases.insert(
                    key,
                    PendingAssetRelease {
                        kind,
                        revision,
                        cancelable: kind == AssetReleaseKind::Remove && previous.is_none(),
                    },
                );
            }
        }
        true
    }

    /// Deterministic release requests. Payloads and identities remain intact while borrowed.
    pub(crate) fn pending_releases(&self) -> Vec<(AssetReleaseKind, AssetLifecycleEvent)> {
        self.pending_releases
            .iter()
            .filter_map(|(&key, &release)| {
                let provider = self.get(key)?;
                Some((
                    release.kind,
                    AssetLifecycleEvent {
                        key,
                        source: provider.source().clone(),
                        kind: match release.kind {
                            AssetReleaseKind::Graphics => AssetLifecycleKind::GraphicsInvalidated,
                            AssetReleaseKind::Unload => AssetLifecycleKind::StatusChanged,
                            AssetReleaseKind::Remove | AssetReleaseKind::Revoke => {
                                AssetLifecycleKind::Removed
                            }
                        },
                        status: AssetLoadStatus::Unloaded,
                        // Pending release is only used for synchronous invalidation. Public
                        // graphics observations come from the provider after release.
                        representation: Default::default(),
                        release_revision: Some(release.revision),
                    },
                ))
            })
            .collect()
    }

    /// Commit only the release snapshot whose full invalidation barrier completed.
    /// A stronger request remains pending for another Host drain.
    pub(crate) fn finish_release(&mut self, event: &AssetLifecycleEvent) -> bool {
        let Some(release) = self.pending_releases.get(&event.key).copied() else {
            return false;
        };
        if event.release_revision != Some(release.revision)
            || self
                .get(event.key)
                .is_none_or(|provider| provider.source() != &event.source)
        {
            return false;
        }
        self.pending_releases.remove(&event.key);
        match release.kind {
            AssetReleaseKind::Graphics => self.invalidate_graphics_released(event.key),
            AssetReleaseKind::Unload => self.unload_released(event.key),
            AssetReleaseKind::Remove | AssetReleaseKind::Revoke => {
                let mut committed = event.clone();
                committed.kind = AssetLifecycleKind::Removed;
                self.remove_slot_released(event.key);
                self.push_lifecycle(committed);
            }
        }
        true
    }

    pub(super) fn record_lifecycle(&mut self, progress: &AssetLoadProgress) {
        if self.lifecycle_barrier {
            self.push_lifecycle(AssetLifecycleEvent {
                key: progress.key,
                source: progress.source.clone(),
                kind: AssetLifecycleKind::StatusChanged,
                status: progress.status.clone(),
                representation: progress.representation,
                release_revision: None,
            });
        }
    }

    fn push_lifecycle(&mut self, event: AssetLifecycleEvent) {
        if matches!(event.status, AssetLoadStatus::Progress { .. })
            && self.lifecycle_events.back().is_some_and(|previous| {
                previous.key == event.key
                    && matches!(previous.status, AssetLoadStatus::Progress { .. })
            })
        {
            self.lifecycle_events.pop_back();
        }
        // This is the owning Host phase's synchronous transition journal. Never
        // drop an applied transition: only asynchronous publisher queues overflow.
        self.lifecycle_events.push_back(event);
    }

    pub(crate) fn finish_lifecycle_event(&mut self, event: &AssetLifecycleEvent) {
        if event.kind == AssetLifecycleKind::Removed
            || event.status == AssetLoadStatus::Unloaded
                && self.get(event.key).is_some()
                && !self.pending_releases.contains_key(&event.key)
        {
            self.lifecycle_recipients.remove(&event.key);
        }
    }

    pub(crate) fn take_lifecycle_events(&mut self) -> Vec<AssetLifecycleEvent> {
        self.lifecycle_events.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synchronous_journal_preserves_applied_transitions_beyond_client_backlog_limits() {
        let mut assets = AssetManagementService::empty();
        assets.require_lifecycle_barrier();
        let key = AssetKey {
            slot: 0,
            generation: 1,
        };
        let source = AssetSource {
            kind: super::super::AssetTypeId(1),
            uri: "memory:journal".into(),
            variant: 0,
        };
        let expected: Vec<_> = (0..40)
            .map(|attempt| AssetLoadStatus::Failed(format!("attempt {attempt}")))
            .collect();
        for status in &expected {
            assets.record_lifecycle(&AssetLoadProgress {
                representation: Default::default(),
                key,
                source: source.clone(),
                status: status.clone(),
            });
        }
        let actual: Vec<_> = assets
            .take_lifecycle_events()
            .into_iter()
            .map(|event| event.status)
            .collect();
        assert_eq!(actual, expected);
        assert!(assets.take_lifecycle_events().is_empty());
    }
}
