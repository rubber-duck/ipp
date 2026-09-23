//! Renderer-owned whole-Surface texture caches for opted-in Surfaces.
//!
//! Each cached Surface's composed content is rasterized into one context-owned
//! SRGB8_ALPHA8 image covering its root content rectangle `[0, 0, w, h]`, with
//! premultiplied linear colour produced by the ordinary Surface blend state over
//! a transparent clear. The image is composited every frame at the current
//! Surface placement; its resolution and refresh cadence follow
//! [`ipp_core::SurfaceCachePolicy`].
//!
//! # Store
//!
//! [`SurfaceTextureCache`] belongs to the Host's RenderService and is shared by
//! every World on its context. Entries are keyed by World and generational
//! entity identity, so a recreated entity or replacement World never meets an
//! image from a previous lifetime. Each entry remembers its band, image and
//! size, the paint and resource revisions it last painted, the World time of
//! that paint and of its last cached presentation, its cumulative counters and
//! the presentation selected by the last planned frame.
//!
//! # Frame decisions
//!
//! Planning runs once per World frame, before any GPU work:
//!
//! 1. Culled Surfaces do nothing and keep their image.
//! 2. Interaction, the direct band, a device limit of zero, a zero budget, an
//!    allocation back-off or an unusable content size present directly.
//! 3. Otherwise the Surface is cached. It repaints when it has no image, its
//!    resolution changed, its resource revision changed, a resource its last
//!    repaint had to skip is now resident, or its image is out of date and
//!    either was not presented cached by the previous frame or the band's
//!    refresh interval has elapsed since the last paint. Everything else
//!    reuses the image, so paint edits coalesce until the next refresh.
//!
//! A repaint that skips a primitive because its resource or GPU data is not
//! resident (after context recovery, or while it is still loading) leaves the
//! image incomplete and records the missing resources. Direct presentation
//! skips the same primitives, so the image stays current until one of them
//! becomes resident; the first frame that sees it resident repaints regardless
//! of the refresh interval. Analytic glyph fallback draws, so it is complete;
//! when the glyph atlas deferred entries past its per-frame population bound,
//! the image is refined on the next frame instead, until its text samples the
//! atlas as direct presentation does.
//!
//! Resolution is the band's texel density times the content size, scaled
//! uniformly to fit `min(device limit, SURFACE_CACHE_MAX_DIMENSION)`. The
//! refresh clock is World time supplied by the Host, never frame counts.
//!
//! # Memory
//!
//! Images count four bytes per texel against a context-wide budget
//! ([`DEFAULT_SURFACE_CACHE_BUDGET_BYTES`] by default). Surfaces presented cached
//! this frame are admitted in entity order, images already at their size first;
//! a Surface that does not fit presents directly as a fallback. Images not
//! presented this frame are then evicted, least recently presented first with
//! ties broken by World and entity. Recoverable allocation or repaint failures
//! release the image and present directly for [`SURFACE_CACHE_RETRY_SECONDS`] of
//! World time. Images not presented cached for [`SURFACE_CACHE_IDLE_SECONDS`]
//! are released at the end of a frame, and entries of Surfaces that are no
//! longer live or opted in are removed after every completed frame. Failed
//! and cameraless frames keep every entry. Context loss and unload release
//! every entry; the next frame repaints from current evaluated inputs.

use super::RenderError;
use ipp_core::{EntityId, SurfaceCachePolicy, WorldId, services::asset_management::AssetKey};
use std::collections::BTreeMap;

/// Default context-wide byte budget for resident Surface cache images.
pub const DEFAULT_SURFACE_CACHE_BUDGET_BYTES: usize = 32 << 20;

/// Largest cache image dimension, further capped by the device limit.
pub const SURFACE_CACHE_MAX_DIMENSION: u32 = 2048;

/// World seconds without cached presentation after which an image is released.
pub const SURFACE_CACHE_IDLE_SECONDS: f64 = 10.0;

/// World seconds a Surface presents directly after a recoverable cache failure.
pub const SURFACE_CACHE_RETRY_SECONDS: f64 = 1.0;

/// How an opted-in Surface was presented by the last completed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfaceCachePresentation {
    /// Direct presentation because the Surface is inside its direct distance.
    Near,
    /// Direct presentation because its GuiRoot has live focus, hover, press or capture.
    Interaction,
    /// Direct presentation after budget pressure or a recoverable allocation or
    /// repaint failure.
    Fallback,
    /// Direct presentation because this context cannot provide cache targets
    /// or the Surface has no usable content size.
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

/// Device operations the store needs to own cache images.
pub(crate) trait SurfaceCacheTargets {
    /// Context-owned image handle.
    type Target;

    /// Allocate a transparent image of `size` texels.
    fn create(&mut self, size: [u32; 2]) -> Result<Self::Target, RenderError>;

    /// Reallocate an image in place; on failure the caller deletes it.
    fn resize(&mut self, target: &mut Self::Target, size: [u32; 2]) -> Result<(), RenderError>;

    /// Release an image, tolerating handles invalidated by context loss.
    fn delete(&mut self, target: Self::Target);
}

/// Evaluated inputs of one visible or culled opted-in Surface for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceCacheInput {
    pub entity: EntityId,
    pub policy: SurfaceCachePolicy,
    /// Content rectangle width and height in metres.
    pub clip_size: [f32; 2],
    pub paint_revision: u64,
    pub resource_revision: u64,
    /// Live GUI interaction on the Surface's root.
    pub interaction: bool,
    /// A resource the image's last repaint skipped is resident now.
    pub missing_resident: bool,
    /// Inside the camera frustum this frame.
    pub visible: bool,
    /// Camera-to-anchor distance in metres.
    pub distance: f32,
}

/// Work selected for one opted-in Surface in the current frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurfaceCacheAction {
    /// Outside the view; no drawing.
    Culled,
    /// Draw the Surface's primitives directly in the main pass.
    Direct,
    /// Composite the existing image in the main pass.
    Reuse,
    /// Repaint the image before the main pass, then composite it.
    Repaint,
}

/// Per-frame cache work, published into [`super::RenderStats`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SurfaceCacheFrameCounts {
    pub repaints: u32,
    pub reuses: u32,
    pub direct: u32,
    pub fallbacks: u32,
    pub allocations: u32,
}

/// Revisions an image was last painted from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceCachePaint {
    paint_revision: u64,
    resource_revision: u64,
    /// No primitive was skipped for a missing resource or missing GPU data.
    complete: bool,
    /// Text was drawn analytically while atlas population was deferred.
    refine: bool,
}

struct SurfaceCacheImage<T> {
    target: T,
    size: [u32; 2],
}

impl<T> SurfaceCacheImage<T> {
    fn bytes(&self) -> usize {
        image_bytes(self.size)
    }
}

struct SurfaceCacheEntry<T> {
    band: u8,
    image: Option<SurfaceCacheImage<T>>,
    /// `None` until a repaint of the current image storage completes.
    painted: Option<SurfaceCachePaint>,
    /// Resources the last repaint skipped because they were not resident.
    missing: Vec<AssetKey>,
    painted_at: f64,
    /// World time of the last cached presentation, for idle release.
    cached_at: f64,
    /// Context-wide plan stamp of the last cached presentation, for eviction order.
    used: u64,
    /// World time before which the Surface presents directly after a failure.
    retry_at: f64,
    /// Composited by the previous planned frame of this World.
    shown: bool,
    /// Plan stamp of the last frame that listed this Surface as live and opted in.
    live: u64,
    presentation: SurfaceCachePresentation,
    repaints: u32,
    reuses: u32,
    // Frame-local selection.
    action: SurfaceCacheAction,
    desired: [u32; 2],
    revisions: (u64, u64),
}

impl<T> SurfaceCacheEntry<T> {
    fn new(band: u8) -> Self {
        Self {
            band,
            image: None,
            painted: None,
            missing: Vec::new(),
            painted_at: 0.0,
            cached_at: 0.0,
            used: 0,
            retry_at: f64::NEG_INFINITY,
            shown: false,
            live: 0,
            presentation: SurfaceCachePresentation::Near,
            repaints: 0,
            reuses: 0,
            action: SurfaceCacheAction::Direct,
            desired: [0, 0],
            revisions: (0, 0),
        }
    }

    fn cached(&self) -> bool {
        matches!(
            self.action,
            SurfaceCacheAction::Reuse | SurfaceCacheAction::Repaint
        )
    }
}

/// Context-wide Surface cache store shared by every World on one context.
pub(crate) struct SurfaceTextureCache<T> {
    budget_bytes: usize,
    resident_bytes: usize,
    entries: BTreeMap<(WorldId, EntityId), SurfaceCacheEntry<T>>,
    /// Incremented by every plan; orders least-recent use across Worlds.
    stamp: u64,
    /// World whose frame is planned and not yet finished.
    planned: Option<WorldId>,
    counts: SurfaceCacheFrameCounts,
    /// Reused budget ordering scratch.
    order: Vec<(bool, u64, (WorldId, EntityId))>,
}

impl<T> Default for SurfaceTextureCache<T> {
    fn default() -> Self {
        Self {
            budget_bytes: DEFAULT_SURFACE_CACHE_BUDGET_BYTES,
            resident_bytes: 0,
            entries: BTreeMap::new(),
            stamp: 0,
            planned: None,
            counts: SurfaceCacheFrameCounts::default(),
            order: Vec::new(),
        }
    }
}

impl<T> SurfaceTextureCache<T> {
    /// Resident image budget; a lowered budget evicts at the next frame.
    pub(crate) fn budget(&self) -> usize {
        self.budget_bytes
    }

    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
    }

    /// Whether any World has an entry; lets default frames skip cache work.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Select this frame's presentation for every opted-in Surface of `world`,
    /// apply the budget and allocate or resize images.
    ///
    /// `inputs` lists every live opted-in Surface, visible or not. Only context
    /// loss is returned; other allocation failures present directly.
    pub(crate) fn plan<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        time: f64,
        limit: u32,
        inputs: &[SurfaceCacheInput],
        targets: &mut A,
    ) -> Result<(), RenderError> {
        self.stamp += 1;
        self.planned = Some(world);
        self.counts = SurfaceCacheFrameCounts::default();
        let limit = limit.min(SURFACE_CACHE_MAX_DIMENSION);

        for input in inputs {
            self.select(world, time, limit, input);
        }

        self.apply_budget(targets);
        let allocated = self.allocate(world, time, targets);

        for entry in self.entries.range_mut(world_range(world)).map(|(_, e)| e) {
            if entry.live != self.stamp {
                continue;
            }

            match entry.action {
                SurfaceCacheAction::Direct => self.counts.direct += 1,
                SurfaceCacheAction::Reuse => {
                    self.counts.reuses += 1;
                    entry.reuses = entry.reuses.saturating_add(1);
                }
                SurfaceCacheAction::Culled | SurfaceCacheAction::Repaint => {}
            }

            if entry.presentation == SurfaceCachePresentation::Fallback {
                self.counts.fallbacks += 1;
            }
        }

        allocated
    }

    fn select(&mut self, world: WorldId, time: f64, limit: u32, input: &SurfaceCacheInput) {
        use SurfaceCachePresentation as P;

        let stamp = self.stamp;
        let budget = self.budget_bytes;
        let entry = self
            .entries
            .entry((world, input.entity))
            .or_insert_with(|| SurfaceCacheEntry::new(input.policy.initial_band(input.distance)));

        entry.live = stamp;
        entry.band = input.policy.band(input.distance, entry.band);
        entry.revisions = (input.paint_revision, input.resource_revision);
        let shown = std::mem::replace(&mut entry.shown, false);

        let direct = if !input.visible {
            entry.action = SurfaceCacheAction::Culled;
            entry.presentation = P::Culled;
            return;
        } else if input.interaction {
            Some(P::Interaction)
        } else if entry.band == 0 {
            Some(P::Near)
        } else if limit == 0 {
            Some(P::Unavailable)
        } else if budget == 0 || time < entry.retry_at {
            Some(P::Fallback)
        } else {
            None
        };

        let desired = direct.map_or_else(
            || {
                cache_size(
                    input.clip_size,
                    input.policy.texels_per_metre_at(entry.band),
                    limit,
                )
            },
            |_| None,
        );
        let (Some(desired), None) = (desired, direct) else {
            entry.action = SurfaceCacheAction::Direct;
            entry.presentation = direct.unwrap_or(P::Unavailable);
            return;
        };

        entry.desired = desired;
        let resized = entry
            .image
            .as_ref()
            .is_none_or(|image| image.size != desired);
        let repaint = match entry.painted {
            _ if resized => true,
            None => true,
            Some(painted) if painted.resource_revision != input.resource_revision => true,
            // A skipped resource arrived, or deferred glyphs are being populated:
            // repaint now, whatever the cadence.
            Some(painted) if !painted.complete && input.missing_resident => true,
            Some(painted) if painted.refine => true,
            // Up to date. An incomplete image matches direct presentation, which
            // skips the same primitives, until a missing resource arrives.
            Some(painted) if painted.paint_revision == input.paint_revision => false,
            // Out-of-date content may stay on screen only while it was on screen.
            Some(_) if !shown => true,
            Some(_) => {
                time - entry.painted_at >= input.policy.refresh_interval_at(entry.band) - 1e-9
            }
        };

        if repaint {
            entry.action = SurfaceCacheAction::Repaint;
            entry.presentation = P::Repainted;
        } else {
            entry.action = SurfaceCacheAction::Reuse;
            entry.presentation = P::Reused;
        }
    }

    /// Admit this frame's cached Surfaces within the budget, then evict images
    /// not presented this frame, least recently presented first.
    fn apply_budget<A: SurfaceCacheTargets<Target = T>>(&mut self, targets: &mut A) {
        let stamp = self.stamp;
        let mut demand = 0;
        let mut idle = 0;
        for entry in self.entries.values() {
            if entry.live == stamp && entry.cached() {
                demand += image_bytes(entry.desired);
            } else if let Some(image) = &entry.image {
                idle += image.bytes();
            }
        }

        if demand + idle <= self.budget_bytes {
            return;
        }

        // Images already at their size first, each group in entity order.
        self.order.clear();
        for (key, entry) in &self.entries {
            if entry.live == stamp && entry.cached() {
                let grows = entry
                    .image
                    .as_ref()
                    .is_none_or(|image| image.size != entry.desired);
                self.order.push((grows, 0, *key));
            }
        }
        self.order.sort_unstable();

        let mut admitted = 0;
        for index in 0..self.order.len() {
            let key = self.order[index].2;
            let entry = self.entries.get_mut(&key).expect("ordered entry");
            let bytes = image_bytes(entry.desired);
            if admitted + bytes <= self.budget_bytes {
                admitted += bytes;
                continue;
            }

            entry.action = SurfaceCacheAction::Direct;
            entry.presentation = SurfaceCachePresentation::Fallback;
            entry.painted = None;
            if let Some(image) = entry.image.take() {
                self.resident_bytes -= image.bytes();
                targets.delete(image.target);
            }
        }

        // Least recently presented first, ties by World and entity.
        self.order.clear();
        for (key, entry) in &self.entries {
            if !(entry.live == stamp && entry.cached()) && entry.image.is_some() {
                self.order.push((false, entry.used, *key));
            }
        }
        self.order
            .sort_unstable_by_key(|&(_, used, key)| (used, key));

        let mut idle = self
            .order
            .iter()
            .map(|(_, _, key)| self.entries[key].image.as_ref().map_or(0, |i| i.bytes()))
            .sum::<usize>();
        for index in 0..self.order.len() {
            if admitted + idle <= self.budget_bytes {
                break;
            }

            let key = self.order[index].2;
            let entry = self.entries.get_mut(&key).expect("ordered entry");
            if let Some(image) = entry.image.take() {
                idle -= image.bytes();
                self.resident_bytes -= image.bytes();
                entry.painted = None;
                targets.delete(image.target);
            }
        }
    }

    fn allocate<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        time: f64,
        targets: &mut A,
    ) -> Result<(), RenderError> {
        let stamp = self.stamp;
        let mut resident = self.resident_bytes;
        let mut lost = false;
        for entry in self.entries.range_mut(world_range(world)).map(|(_, e)| e) {
            if entry.live != stamp || entry.action != SurfaceCacheAction::Repaint {
                continue;
            }

            let desired = entry.desired;
            let allocated = match entry.image.as_mut() {
                Some(image) if image.size == desired => continue,
                Some(image) => targets.resize(&mut image.target, desired).map(|()| None),
                None => targets.create(desired).map(Some),
            };

            match allocated {
                Ok(created) => {
                    if let Some(target) = created {
                        entry.image = Some(SurfaceCacheImage {
                            target,
                            size: desired,
                        });
                    } else if let Some(image) = entry.image.as_mut() {
                        resident -= image.bytes();
                        image.size = desired;
                    }

                    resident += image_bytes(desired);
                    entry.painted = None;
                    self.counts.allocations += 1;
                }
                Err(error) => {
                    if let Some(image) = entry.image.take() {
                        resident -= image.bytes();
                        targets.delete(image.target);
                    }

                    entry.painted = None;
                    entry.action = SurfaceCacheAction::Direct;
                    entry.presentation = SurfaceCachePresentation::Fallback;
                    entry.retry_at = time + SURFACE_CACHE_RETRY_SECONDS;
                    if error == RenderError::ContextLost {
                        lost = true;
                        break;
                    }
                }
            }
        }

        self.resident_bytes = resident;
        if lost {
            Err(RenderError::ContextLost)
        } else {
            Ok(())
        }
    }

    /// Work selected for one Surface by the current plan; `None` when it is not
    /// opted in or not planned.
    pub(crate) fn action(&self, world: WorldId, entity: EntityId) -> Option<SurfaceCacheAction> {
        self.entries
            .get(&(world, entity))
            .filter(|entry| entry.live == self.stamp && self.planned == Some(world))
            .map(|entry| entry.action)
    }

    /// Whether the current plan repaints any image of `world`.
    pub(crate) fn repaints_planned(&self, world: WorldId) -> bool {
        self.planned == Some(world)
            && self.world_entries(world).any(|entry| {
                entry.live == self.stamp && entry.action == SurfaceCacheAction::Repaint
            })
    }

    /// Whether the current plan composites any image of `world`.
    pub(crate) fn composites_planned(&self, world: WorldId) -> bool {
        self.planned == Some(world)
            && self
                .world_entries(world)
                .any(|entry| entry.live == self.stamp && entry.cached())
    }

    /// The image and its size for a Surface planned to repaint or reuse.
    pub(crate) fn image(&self, world: WorldId, entity: EntityId) -> Option<(&T, [u32; 2])> {
        self.entries
            .get(&(world, entity))
            .and_then(|entry| entry.image.as_ref())
            .map(|image| (&image.target, image.size))
    }

    /// Record a completed repaint. `missing` lists the resources whose
    /// primitives were skipped because they were not resident; the image is
    /// repainted as soon as a plan reports one of them resident. `refine`
    /// repaints it on the next planned frame.
    pub(crate) fn repainted(
        &mut self,
        world: WorldId,
        entity: EntityId,
        time: f64,
        missing: &[AssetKey],
        refine: bool,
    ) {
        let Some(entry) = self.entries.get_mut(&(world, entity)) else {
            return;
        };

        entry.painted = Some(SurfaceCachePaint {
            paint_revision: entry.revisions.0,
            resource_revision: entry.revisions.1,
            complete: missing.is_empty(),
            refine,
        });
        entry.missing.clear();
        entry.missing.extend_from_slice(missing);
        entry.missing.sort_unstable();
        entry.missing.dedup();
        entry.painted_at = time;
        entry.repaints = entry.repaints.saturating_add(1);
        self.counts.repaints += 1;
    }

    /// Resources the Surface's current image skipped; empty for a complete
    /// image or no image. The caller reports their residency in the next plan.
    pub(crate) fn missing(&self, world: WorldId, entity: EntityId) -> &[AssetKey] {
        match self.entries.get(&(world, entity)) {
            Some(entry) if entry.painted.is_some_and(|painted| !painted.complete) => &entry.missing,
            _ => &[],
        }
    }

    /// Record a composite: the image is on screen for the stale-image rule and
    /// counts as used for eviction and idle release.
    pub(crate) fn presented(&mut self, world: WorldId, entity: EntityId, time: f64) {
        if let Some(entry) = self.entries.get_mut(&(world, entity)) {
            entry.shown = true;
            entry.cached_at = time;
            entry.used = self.stamp;
        }
    }

    /// Release the image after a recoverable repaint or composite failure and
    /// present directly until the retry interval elapses.
    pub(crate) fn failed<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        entity: EntityId,
        time: f64,
        targets: &mut A,
    ) {
        let Some(entry) = self.entries.get_mut(&(world, entity)) else {
            return;
        };

        if let Some(image) = entry.image.take() {
            self.resident_bytes -= image.bytes();
            targets.delete(image.target);
        }

        // A reused image that failed to composite was not presented.
        if entry.action == SurfaceCacheAction::Reuse {
            entry.reuses = entry.reuses.saturating_sub(1);
            self.counts.reuses = self.counts.reuses.saturating_sub(1);
        }

        entry.painted = None;
        entry.shown = false;
        entry.retry_at = time + SURFACE_CACHE_RETRY_SECONDS;
        entry.action = SurfaceCacheAction::Direct;
        entry.presentation = SurfaceCachePresentation::Fallback;
        self.counts.direct += 1;
        self.counts.fallbacks += 1;
    }

    /// End the World frame. A completed plan removes entries of Surfaces no
    /// longer live or opted in and releases idle images, then returns the
    /// frame's counts; otherwise every entry is kept and nothing counts.
    pub(crate) fn finish_frame<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        time: f64,
        completed: bool,
        targets: &mut A,
    ) -> Option<SurfaceCacheFrameCounts> {
        let planned = self.planned.take() == Some(world);
        if !(planned && completed) {
            // Nothing planned was necessarily presented.
            for (_, entry) in self.entries.range_mut(world_range(world)) {
                entry.shown = false;
            }
            return None;
        }

        let stamp = self.stamp;
        let mut released = 0;
        self.entries.retain(|&(owner, _), entry| {
            if owner != world {
                return true;
            }

            let live = entry.live == stamp;
            let idle = !entry.shown && time - entry.cached_at >= SURFACE_CACHE_IDLE_SECONDS;
            if (!live || idle)
                && let Some(image) = entry.image.take()
            {
                released += image.bytes();
                entry.painted = None;
                targets.delete(image.target);
            }

            live
        });
        self.resident_bytes -= released;

        Some(self.counts)
    }

    /// Release every entry of one World, leaving other Worlds untouched.
    pub(crate) fn forget_world<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        targets: &mut A,
    ) {
        let mut released = 0;
        self.entries.retain(|&(owner, _), entry| {
            if owner != world {
                return true;
            }

            if let Some(image) = entry.image.take() {
                released += image.bytes();
                targets.delete(image.target);
            }

            false
        });
        self.resident_bytes -= released;
        if self.planned == Some(world) {
            self.planned = None;
        }
    }

    /// Release every entry after unload or context loss.
    pub(crate) fn clear<A: SurfaceCacheTargets<Target = T>>(&mut self, targets: &mut A) {
        for (_, entry) in std::mem::take(&mut self.entries) {
            if let Some(image) = entry.image {
                targets.delete(image.target);
            }
        }

        self.resident_bytes = 0;
        self.planned = None;
    }

    /// Append one World's entries in entity order.
    pub(crate) fn diagnostics(&self, world: WorldId, out: &mut Vec<SurfaceCacheDiagnostic>) {
        out.extend(
            self.entries
                .range(world_range(world))
                .map(|(&(_, entity), entry)| SurfaceCacheDiagnostic {
                    entity,
                    presentation: entry.presentation,
                    band: entry.band,
                    size: entry.image.as_ref().map_or([0, 0], |image| image.size),
                    repaints: entry.repaints,
                    reuses: entry.reuses,
                    painted_at: entry.painted_at,
                    resident_bytes: entry.image.as_ref().map_or(0, |image| image.bytes() as u32),
                }),
        );
    }

    /// Context-wide resident image count and bytes.
    pub(crate) fn resident(&self) -> (u32, u32) {
        let images = self
            .entries
            .values()
            .filter(|entry| entry.image.is_some())
            .count();
        (
            images as u32,
            u32::try_from(self.resident_bytes).unwrap_or(u32::MAX),
        )
    }

    fn world_entries(&self, world: WorldId) -> impl Iterator<Item = &SurfaceCacheEntry<T>> {
        self.entries
            .range(world_range(world))
            .map(|(_, entry)| entry)
    }
}

fn world_range(world: WorldId) -> std::ops::RangeInclusive<(WorldId, EntityId)> {
    (world, EntityId::from_bits(0))..=(world, EntityId::from_bits(u64::MAX))
}

/// Resident bytes of an image, four per texel.
fn image_bytes(size: [u32; 2]) -> usize {
    4 * size[0] as usize * size[1] as usize
}

/// Texels within this fraction of a whole count round down to it, so a size and
/// density whose product is whole in decimal (2.4 m at 80 texels per metre) is
/// not enlarged by the `f32` representation error of its factors.
const SURFACE_CACHE_SIZE_TOLERANCE: f64 = 1e-4;

/// Image size for a content rectangle at a texel density, scaled uniformly to
/// fit `limit`; `None` for an empty or non-finite rectangle.
pub(crate) fn cache_size(
    clip_size: [f32; 2],
    texels_per_metre: f32,
    limit: u32,
) -> Option<[u32; 2]> {
    let width = f64::from(clip_size[0]) * f64::from(texels_per_metre);
    let height = f64::from(clip_size[1]) * f64::from(texels_per_metre);
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) || limit == 0 {
        return None;
    }

    let scale = (f64::from(limit) / width.max(height)).min(1.0);
    let axis = |texels: f64| {
        (texels * scale - SURFACE_CACHE_SIZE_TOLERANCE)
            .ceil()
            .clamp(1.0, f64::from(limit)) as u32
    };
    Some([axis(width), axis(height)])
}

#[cfg(test)]
#[path = "surface_cache_tests.rs"]
mod tests;
