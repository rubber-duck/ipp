//! Frame-to-frame state shared by retained Surface work: which retained work a
//! completed frame made stale, and whether a Surface's prepared paint is unchanged
//! since its last drawn frame.
//!
//! # Stale work
//!
//! Destroyed Surfaces release everything. A submitted Surface drew every primitive it
//! still owns, so its unused work is stale. Live Surfaces skipped by culling, or
//! composited from an unchanged cache image, keep their retained work for the frame
//! they draw again.
//!
//! # Unchanged paint
//!
//! Core publishes a World-monotonic paint revision beside each Surface item that
//! changes whenever its primitives or clip size paint differently; zero means the
//! revision is unknown. Retained box, glyph and analytic text work keyed by primitive
//! identity may reuse the input hashes it computed under the same revision, which
//! skips re-hashing every primitive on unchanged frames.
//!
//! The revision ignores primitive identities, so identical paint can move between
//! identities, for example when two identical controls swap places. Reuse therefore
//! also requires the Surface's identity order to match its last successful draw.

use std::collections::{BTreeMap, BTreeSet};

use ipp_core::systems::surface::SurfacePrimitiveIdentity;
use ipp_core::{EntityId, SurfaceRenderItem};

/// Surface participation in one completed frame, deciding which retained work is stale.
pub struct RetainedSurfaceSubmission<'a> {
    /// Every live Surface entity in the World.
    pub live: &'a BTreeSet<EntityId>,
    /// Surfaces whose primitives this frame submitted.
    pub submitted: &'a BTreeSet<EntityId>,
}

impl RetainedSurfaceSubmission<'_> {
    /// Whether work retained for `entity` is stale, given whether this frame used it.
    pub fn is_stale(&self, entity: EntityId, used: bool) -> bool {
        !self.live.contains(&entity) || (!used && self.submitted.contains(&entity))
    }
}

/// Paint identity of one Surface for the current frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfacePaint {
    /// Core paint revision; zero when unknown.
    pub revision: u64,
    /// The revision and identity order match the Surface's last successful draw, so
    /// hashes computed under `revision` still describe each primitive identity.
    pub reusable: bool,
}

impl SurfacePaint {
    /// Unknown paint: every input is hashed.
    pub const UNKNOWN: Self = Self {
        revision: 0,
        reusable: false,
    };

    /// Whether work hashed under `revision` may be reused without hashing again.
    pub fn reuses(self, revision: u64) -> bool {
        self.reusable && revision == self.revision
    }
}

/// Last successfully drawn paint revision and identity order of each Surface of one
/// World.
#[derive(Default)]
pub struct SurfacePaintTracker {
    surfaces: BTreeMap<EntityId, (u64, Vec<SurfacePrimitiveIdentity>)>,
}

impl SurfacePaintTracker {
    /// Paint identity of `item` for this frame.
    pub fn paint(&self, item: &SurfaceRenderItem) -> SurfacePaint {
        let reusable = item.paint_revision != 0
            && self
                .surfaces
                .get(&item.entity)
                .is_some_and(|(revision, identities)| {
                    *revision == item.paint_revision
                        && identities.len() == item.primitives.len()
                        && identities
                            .iter()
                            .zip(&item.primitives)
                            .all(|(identity, primitive)| *identity == primitive.style().identity)
                });

        SurfacePaint {
            revision: item.paint_revision,
            reusable,
        }
    }

    /// Record a completed draw of `item`, so an unchanged next frame may reuse it.
    pub fn drawn(&mut self, item: &SurfaceRenderItem, paint: SurfacePaint) {
        if item.paint_revision == 0 {
            self.surfaces.remove(&item.entity);
            return;
        }

        if paint.reusable {
            return;
        }

        let (revision, identities) = self.surfaces.entry(item.entity).or_default();
        *revision = item.paint_revision;
        identities.clear();
        identities.extend(
            item.primitives
                .iter()
                .map(|primitive| primitive.style().identity),
        );
    }

    /// Forget Surfaces that are no longer live.
    pub fn retain(&mut self, live: &BTreeSet<EntityId>) {
        self.surfaces.retain(|entity, _| live.contains(entity));
    }
}

#[cfg(test)]
#[path = "retained_surfaces_tests.rs"]
mod tests;
