//! Exact producer-write receipts with caller-owned, monotone retirement observations.

use super::{HostRuntime, WorldRef, topology::AttachmentAnchor};
use crate::{EntityId, ErrorReason};
use std::{
    cmp::Ordering,
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
};

#[derive(Debug)]
struct AttachmentReceipt {
    host: u64,
    revision: u64,
    parent: WorldRef,
    anchor: EntityId,
    incarnation: u64,
    child: Option<WorldRef>,
    retired: AtomicBool,
}

/// An exact applied attachment write and its owned retirement observation.
/// Clones retain only bounded identity metadata, never a World, Host or publication.
/// Transport adapters retain these handles in their bounded session registries until release.
#[derive(Clone, Debug)]
pub struct WorldAttachmentToken(Arc<AttachmentReceipt>);

impl WorldAttachmentToken {
    pub(super) fn new(
        host: u64,
        revision: u64,
        parent: WorldRef,
        anchor: EntityId,
        incarnation: u64,
        child: Option<WorldRef>,
    ) -> Self {
        Self(Arc::new(AttachmentReceipt {
            host,
            revision,
            parent,
            anchor,
            incarnation,
            child,
            retired: AtomicBool::new(child.is_none()),
        }))
    }

    /// Host-qualified opaque identity for a transport's retained-handle registry.
    /// This is not a constructor: unknown or released wire identities must be rejected.
    pub fn identity(&self) -> (u64, u64) {
        (self.0.host, self.0.revision)
    }

    /// Exact parent World lifetime of the applied producer write.
    pub fn parent(&self) -> WorldRef {
        self.0.parent
    }

    /// Generational attachment entity, independent of component replacement.
    pub fn anchor(&self) -> EntityId {
        self.0.anchor
    }

    /// Exact producer component lifetime; field writes additionally change the token identity.
    pub fn incarnation(&self) -> u64 {
        self.0.incarnation
    }

    /// Exact child selected by this write, not a subsequent replacement.
    pub fn child(&self) -> Option<WorldRef> {
        self.0.child
    }

    pub(super) fn location(&self) -> AttachmentAnchor {
        AttachmentAnchor {
            world: self.parent().id(),
            entity: self.anchor(),
        }
    }

    pub(super) fn retire(&self) {
        self.0.retired.store(true, AtomicOrdering::Release);
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<AttachmentReceipt>() + 2 * std::mem::size_of::<usize>()
    }
}

impl PartialEq for WorldAttachmentToken {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl Eq for WorldAttachmentToken {}

impl PartialOrd for WorldAttachmentToken {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WorldAttachmentToken {
    fn cmp(&self, other: &Self) -> Ordering {
        self.identity().cmp(&other.identity())
    }
}

impl Hash for WorldAttachmentToken {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity().hash(state);
    }
}

/// Retirement concerns this exact write's published paths and reservation only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldAttachmentRetirement {
    /// An active edge, reachable completed edge or retiring incoming reservation remains.
    Pending,
    /// The exact edge cannot become usable again, including after output correction or replacement.
    Retired,
}

/// Applied attachment effects retain their exact observation even after later batch failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldAttachmentEffect {
    /// A producer write, including a successful same-value assignment.
    Written(WorldAttachmentToken),
    /// The matching producer component was removed; observe this token's retirement.
    Detached(WorldAttachmentToken),
    /// Another producer replaced the target; observe the original token without mutating it.
    Superseded(WorldAttachmentToken),
}

impl HostRuntime {
    /// Observe an owned receipt, including after its parent or child was destroyed.
    /// Foreign Host receipts fail; adapters must resolve unknown wire IDs in their session registry.
    pub fn attachment_retirement(
        &self,
        token: &WorldAttachmentToken,
    ) -> Result<WorldAttachmentRetirement, ErrorReason> {
        if token.0.host != self.publications.identity {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(if token.0.retired.load(AtomicOrdering::Acquire) {
            WorldAttachmentRetirement::Retired
        } else {
            WorldAttachmentRetirement::Pending
        })
    }
}

#[cfg(test)]
#[path = "attachment_tokens_tests.rs"]
mod tests;
