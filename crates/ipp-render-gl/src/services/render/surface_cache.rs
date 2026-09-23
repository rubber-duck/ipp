//! Renderer-owned whole-Surface texture caches for opted-in Surfaces.
//!
//! Each cached Surface's composed content is rasterized into one context-owned
//! SRGB8_ALPHA8 image covering its root content rectangle `[0, 0, w, h]`, with
//! premultiplied linear colour produced by the ordinary Surface blend state over
//! a transparent clear. The image is composited every frame at the current
//! Surface placement; its resolution and refresh cadence follow
//! [`ipp_core::SurfaceCachePolicy`]. Entries are keyed by World and generational
//! entity identity, bounded by a context-wide byte budget counted as four bytes
//! per texel, and rebuilt from current evaluated inputs after release or
//! context loss.
//!
//! The store and scheduling are delivered by ipp-s1ge.2.3; until then every
//! Surface presents directly and the diagnostics stay empty.

use ipp_core::{EntityId, WorldId};
use std::collections::BTreeMap;

/// Default context-wide byte budget for resident Surface cache images.
pub const DEFAULT_SURFACE_CACHE_BUDGET_BYTES: usize = 32 << 20;

/// How an opted-in Surface was presented by the last completed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfaceCachePresentation {
    /// Direct presentation because the Surface is inside its direct distance.
    Near,
    /// Direct presentation because its GuiRoot has live focus, hover, press or capture.
    Interaction,
    /// Direct presentation after budget pressure or a recoverable allocation failure.
    Fallback,
    /// Direct presentation because this context cannot provide cache targets.
    Unavailable,
    /// Outside the view; neither drawn nor repainted.
    Culled,
    /// Composited from the existing image without repainting.
    Reused,
    /// Repainted into its image, then composited.
    Repainted,
}

impl SurfaceCachePresentation {
    /// Stable numeric code used by diagnostic exports, in declaration order.
    pub const fn code(self) -> u32 {
        match self {
            Self::Near => 0,
            Self::Interaction => 1,
            Self::Fallback => 2,
            Self::Unavailable => 3,
            Self::Culled => 4,
            Self::Reused => 5,
            Self::Repainted => 6,
        }
    }

    /// Whether the Surface was drawn directly rather than from its image.
    pub const fn is_direct(self) -> bool {
        matches!(
            self,
            Self::Near | Self::Interaction | Self::Fallback | Self::Unavailable
        )
    }
}

/// Read-only state of one opted-in Surface's cache on this context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceCacheDiagnostic {
    /// Live generational entity identity within its World.
    pub entity: EntityId,
    /// Presentation selected by the last completed frame.
    pub presentation: SurfaceCachePresentation,
    /// Selected distance band; 0 is direct.
    pub band: u8,
    /// Resident image width and height in texels; zero without an image.
    pub size: [u32; 2],
    /// Repaints since the entry was created.
    pub repaints: u32,
    /// Frames composited from an unchanged image since the entry was created.
    pub reuses: u32,
    /// World time in seconds of the last repaint.
    pub painted_at: f64,
    /// Resident image bytes, four per texel.
    pub resident_bytes: u32,
}

/// Context-wide Surface cache store shared by every World on one context.
#[derive(Debug)]
pub(crate) struct SurfaceTextureCache {
    budget_bytes: usize,
    diagnostics: BTreeMap<(WorldId, EntityId), SurfaceCacheDiagnostic>,
}

impl Default for SurfaceTextureCache {
    fn default() -> Self {
        Self {
            budget_bytes: DEFAULT_SURFACE_CACHE_BUDGET_BYTES,
            diagnostics: BTreeMap::new(),
        }
    }
}

impl SurfaceTextureCache {
    /// Resident image budget; a lowered budget evicts at the next frame.
    pub(crate) fn budget(&self) -> usize {
        self.budget_bytes
    }

    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
    }

    /// Release every entry of one World, leaving other Worlds untouched.
    pub(crate) fn forget_world(&mut self, world: WorldId) {
        self.diagnostics.retain(|(owner, _), _| *owner != world);
    }

    /// Release every entry after unload or context loss.
    pub(crate) fn clear(&mut self) {
        self.diagnostics.clear();
    }

    /// Append one World's entries in entity order.
    pub(crate) fn diagnostics(&self, world: WorldId, out: &mut Vec<SurfaceCacheDiagnostic>) {
        out.extend(
            self.diagnostics
                .range((world, EntityId::from_bits(0))..=(world, EntityId::from_bits(u64::MAX)))
                .map(|(_, diagnostic)| *diagnostic),
        );
    }

    /// Context-wide entry count and resident bytes after a completed frame.
    pub(crate) fn resident(&self) -> (u32, u32) {
        let bytes = self
            .diagnostics
            .values()
            .map(|diagnostic| diagnostic.resident_bytes)
            .fold(0_u32, u32::saturating_add);
        (self.diagnostics.len() as u32, bytes)
    }
}
