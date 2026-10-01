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
//! Each exact Canvas output owns a paint revision including primitive identities and
//! painter order. Hash reuse additionally requires the same inherited clip as its
//! last successful submission. A changed output incarnation owns fresh cache state.

use std::collections::BTreeSet;

use ipp_core::EntityId;

/// Key of the one retained Surface in a Canvas output's caches.
///
/// Retained GUI, glyph and analytic text caches are held per exact output, and a
/// World canvas names no producer entity, so every Canvas output keys its paint with
/// this one value.
pub(crate) const CANVAS_SURFACE: EntityId = EntityId::from_bits(0);

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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePaint {
    /// Core paint revision; zero when unknown.
    pub revision: u64,
    /// Incoming opacity from containing Canvas Worlds.
    pub opacity: f32,
    /// The revision and identity order match the Surface's last successful draw, so
    /// hashes computed under `revision` still describe each primitive identity.
    pub reusable: bool,
}

impl SurfacePaint {
    /// Unknown paint: every input is hashed.
    #[cfg(test)]
    pub const UNKNOWN: Self = Self {
        revision: 0,
        opacity: 1.0,
        reusable: false,
    };

    /// Whether work hashed under `revision` may be reused without hashing again.
    pub fn reuses(self, revision: u64) -> bool {
        self.reusable && revision == self.revision
    }
}

/// Last successful immutable paint and inherited clip of one exact Canvas output.
#[derive(Default)]
pub struct SurfacePaintTracker {
    last: Option<(u64, ipp_core::systems::canvas::CanvasClip, f32)>,
}

impl SurfacePaintTracker {
    pub fn paint(
        &self,
        revision: u64,
        clip: ipp_core::systems::canvas::CanvasClip,
        opacity: f32,
    ) -> SurfacePaint {
        SurfacePaint {
            revision,
            opacity,
            reusable: revision != 0 && self.last == Some((revision, clip, opacity)),
        }
    }

    pub fn drawn(
        &mut self,
        revision: u64,
        clip: ipp_core::systems::canvas::CanvasClip,
        opacity: f32,
    ) {
        self.last = (revision != 0).then_some((revision, clip, opacity));
    }
}

#[cfg(test)]
#[path = "retained_surfaces_tests.rs"]
mod tests;
