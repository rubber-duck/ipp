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
//! every World on its context. Entries are keyed by the parent World and Surface
//! entity; the compositor additionally fences the exact parent World lifetime,
//! child output and attachment token before admitting reuse. Each entry remembers its band, image and
//! size, the paint and resource revisions it last painted, the Host presentation time of
//! that paint and of its last cached presentation, its cumulative counters and
//! the presentation selected by the last planned frame.
//!
//! # Frame decisions
//!
//! Planning reserves all opted-in images of the presented output graph together:
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
//! 4. A Surface whose paint changed and was repainted at its refresh cap on
//!    [`SURFACE_CACHE_ANIMATED_FRAMES`] consecutive frames presents directly
//!    as animated instead of its next due repaint: repainting every frame costs
//!    more than drawing directly. It keeps its image and returns to the rules
//!    above once its paint revision holds for [`SURFACE_CACHE_SETTLE_FRAMES`]
//!    frames, repainting the stale image first. Caps below the frame rate never
//!    repaint on consecutive frames, so their animated content stays cached.
//!
//! A repaint that skips a primitive because its resource or GPU data is not
//! resident (after context recovery, or while it is still loading) leaves the
//! image incomplete and records the missing resources. Direct presentation
//! skips the same primitives, so the image stays current until one of them
//! becomes resident; the first frame that sees it resident repaints regardless
//! of the refresh interval. Analytic glyph fallback draws, so it is complete.
//! A repaint that drew text runs analytically only because atlas population has
//! not reached their entries yet records how many; direct presentation draws
//! the same runs analytically until then. Once a plan reports fewer such runs,
//! the image is refined at its refresh cadence, so population progress by any
//! Surface never repaints an image faster than its cap, and text that stays
//! analytic for other reasons (no atlas band, or entries backing off after a
//! failure) never refines. Paint changes follow the ordinary rules above,
//! including the switch to direct presentation for animated paint.
//!
//! Resolution follows shared projected device-pixel demand and stable selection
//! in [`super::quality`], bounded by the device and dimension cap. The
//! refresh clock is Host presentation time supplied by the Host, never frame counts.
//!
//! # Memory
//!
//! Active raster dimensions are separate from allocation capacity. Growth reserves
//! about 10% headroom within device and budget limits; active quality changes
//! repaint without reallocating while they fit. Shrinking keeps capacity until
//! idle release or memory pressure, which reclaims headroom before denying active
//! quality. Images count four bytes per allocated texel against the budget the renderer
//! owns ([`SURFACE_CACHE_BUDGET_BYTES`]); hosts do not configure it. GL exposes no
//! memory size, so the budget is a documented heuristic while the device's target
//! limit bounds each image's dimensions. Surfaces presented cached
//! this frame are admitted in entity order, images already at their size first;
//! a Surface that does not fit presents directly as a fallback. Images not
//! presented this frame are then evicted, least recently presented first with
//! ties broken by World and entity. Recoverable allocation or repaint failures
//! release the image and present directly for [`SURFACE_CACHE_RETRY_SECONDS`] of
//! Host presentation time. Images not presented cached for [`SURFACE_CACHE_IDLE_SECONDS`]
//! are released at the end of a frame, and entries of Surfaces that are no
//! longer live or opted in are removed after every completed frame. Failed
//! and cameraless frames keep every entry. Context loss and unload release
//! every entry; the next frame repaints from current evaluated inputs.

use crate::RenderError;
use ipp_core::{EntityId, SurfaceCachePolicy, WorldId, services::asset_management::AssetKey};
use std::collections::{BTreeMap, BTreeSet};

/// Context-wide byte budget for resident Surface cache images: 32 MiB, two images at
/// the largest cache dimension or many smaller panels, a small share of the memory
/// of any device that runs WebGL 2 or GLES 3.
pub const SURFACE_CACHE_BUDGET_BYTES: usize = 32 << 20;

/// Largest cache image dimension, further capped by the device limit. At 2048
/// texels one image stays within 16 MiB, half of [`SURFACE_CACHE_BUDGET_BYTES`],
/// so a single oversized Surface cannot claim the whole budget; the WebGL bridge
/// reports its raw device limit and this service applies the cap for every device.
pub const SURFACE_CACHE_MAX_DIMENSION: u32 = 2048;

/// Host seconds without cached presentation after which an image is released.
pub const SURFACE_CACHE_IDLE_SECONDS: f64 = 10.0;

/// Host seconds a Surface presents directly after a recoverable cache failure.
pub const SURFACE_CACHE_RETRY_SECONDS: f64 = 1.0;

/// Consecutive planned frames whose paint changed and were each repainted at the
/// refresh cap, after which a cached Surface presents directly as
/// [`SurfaceCachePresentation::Animated`]. A repaint then costs more than drawing the
/// Surface directly, since every such frame also composites the new image.
pub const SURFACE_CACHE_ANIMATED_FRAMES: u32 = 4;

/// Consecutive planned frames without a paint change after which an animated
/// Surface returns to cached presentation, repainting its stale image first.
pub const SURFACE_CACHE_SETTLE_FRAMES: u32 = 8;

/// How an opted-in Surface was presented by the last completed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfaceCachePresentation {
    /// Direct presentation because the Surface is inside its direct distance.
    Near,
    /// Direct presentation because its GUI content has live focus, hover, press or capture.
    Interaction,
    /// Direct presentation after budget pressure or a recoverable allocation or
    /// repaint failure.
    Fallback,
    /// No usable image. Affine geometry presents directly; required projected
    /// presentation remains unavailable without substituting affine geometry.
    Unavailable,
    /// Direct presentation because the Surface's paint changed and was repainted at
    /// its refresh cap on [`SURFACE_CACHE_ANIMATED_FRAMES`] consecutive frames; it
    /// returns to its image once the paint holds for [`SURFACE_CACHE_SETTLE_FRAMES`].
    Animated,
    /// Outside the view; neither drawn nor repainted.
    Culled,
    /// Composited from the existing image without repainting.
    Reused,
    /// Repainted into its image, then composited.
    Repainted,
    /// Direct presentation because the Surface separates its canvas's layers
    /// along its normal, which one flat image cannot show.
    Layered,
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
            Self::Animated => 7,
            Self::Layered => 8,
        }
    }

    /// Whether the Surface was drawn directly rather than from its image.
    pub const fn is_direct(self) -> bool {
        matches!(
            self,
            Self::Near
                | Self::Interaction
                | Self::Fallback
                | Self::Unavailable
                | Self::Animated
                | Self::Layered
        )
    }
}

/// Read-only state of one Surface's optional or required presentation images.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceCacheDiagnostic {
    /// Live generational entity identity within its World.
    pub entity: EntityId,
    /// Presentation selected by the last completed frame.
    pub presentation: SurfaceCachePresentation,
    /// Selected distance/quality band; 0 selects near quality. Affine optional
    /// caching presents directly in band 0; projected geometry still uses images.
    pub band: u8,
    /// Common resident image dimensions in texels; zero without an image.
    pub size: [u32; 2],
    /// Allocated texture capacity, including growth headroom; zero without an image.
    pub capacity: [u32; 2],
    /// Image repaints since the entry was created, summed over separated layers.
    pub repaints: u32,
    /// Image reuses since the entry was created, summed over separated layers.
    pub reuses: u32,
    /// Host presentation time in seconds of the last repaint.
    pub painted_at: f64,
    /// Aggregate resident image bytes, four per texel across separated layers.
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
    /// Projected device-pixel demand, with uniform content density.
    pub pixel_demand: [f64; 2],
    pub paint_revision: u64,
    pub resource_revision: u64,
    /// Live GUI interaction on the Surface's root.
    pub interaction: bool,
    /// The Surface separates its canvas's layers along its normal; one
    /// flat image cannot present them.
    pub layered: bool,
    /// A resource the image's last repaint skipped is resident now.
    pub missing_resident: bool,
    /// Fewer of the Surface's text runs wait for atlas population than when its
    /// image was last repainted, so some can now sample the atlas.
    pub text_populated: bool,
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

/// Per-frame cache work, published into [`crate::RenderStatistics`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SurfaceCacheFrameCounts {
    pub repaints: u32,
    pub reuses: u32,
    pub direct: u32,
    pub fallbacks: u32,
    pub allocations: u32,
    /// Direct presentations of animated Surfaces; also counted in `direct`.
    pub animated: u32,
}

/// Revisions an image was last painted from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceCachePaint {
    paint_revision: u64,
    resource_revision: u64,
    /// No primitive was skipped for a missing resource or missing GPU data.
    complete: bool,
    /// Text runs drawn analytically because atlas population had not reached
    /// their entries yet.
    unpopulated: u32,
}

struct SurfaceCacheImage<T> {
    target: T,
    size: [u32; 2],
    capacity: [u32; 2],
}

impl<T> SurfaceCacheImage<T> {
    fn bytes(&self) -> usize {
        image_bytes(self.capacity)
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
    /// Host presentation time of the last cached presentation, for idle release.
    cached_at: f64,
    /// Context-wide plan stamp of the last cached presentation, for eviction order.
    used: u64,
    /// Host presentation time before which the Surface presents directly after a failure.
    retry_at: f64,
    /// Composited by the previous planned frame of this World.
    shown: bool,
    /// Consecutive planned frames repainted for a paint change at the refresh cap.
    repaint_streak: u32,
    /// Presenting directly while the paint keeps changing every frame.
    animated: bool,
    /// Consecutive planned frames without a paint change while animated.
    settled: u32,
    /// Paint revision seen by the previous planned frame.
    seen_paint: u64,
    /// Plan stamp of the last frame that listed this Surface as live and opted in.
    live: u64,
    presentation: SurfaceCachePresentation,
    repaints: u32,
    reuses: u32,
    // Frame-local selection.
    action: SurfaceCacheAction,
    desired: [u32; 2],
    desired_capacity: [u32; 2],
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
            repaint_streak: 0,
            animated: false,
            settled: 0,
            seen_paint: 0,
            live: 0,
            presentation: SurfaceCachePresentation::Near,
            repaints: 0,
            reuses: 0,
            action: SurfaceCacheAction::Direct,
            desired: [0, 0],
            desired_capacity: [0, 0],
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
    reserved_bytes: usize,
    resident_bytes: usize,
    /// Entries holding an image, maintained with `resident_bytes`.
    resident_images: usize,
    entries: BTreeMap<(WorldId, EntityId), SurfaceCacheEntry<T>>,
    /// Incremented by every plan; orders least-recent use across Worlds.
    stamp: u64,
    /// Worlds participating in the current output-graph plan.
    planned: BTreeSet<WorldId>,
    counts: SurfaceCacheFrameCounts,
    /// Reused budget ordering scratch.
    order: Vec<(bool, u64, (WorldId, EntityId))>,
}

impl<T> Default for SurfaceTextureCache<T> {
    fn default() -> Self {
        Self {
            budget_bytes: SURFACE_CACHE_BUDGET_BYTES,
            reserved_bytes: 0,
            resident_bytes: 0,
            resident_images: 0,
            entries: BTreeMap::new(),
            stamp: 0,
            planned: BTreeSet::new(),
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

    pub(crate) fn reserve_images(&mut self, bytes: usize) {
        self.reserved_bytes = bytes;
    }

    /// Testing override of the renderer-owned budget.
    #[cfg(any(test, feature = "instrumentation"))]
    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
    }

    /// Select this frame's presentation for every opted-in Surface of `world`,
    /// apply the budget and allocate or resize images.
    ///
    /// `inputs` lists every live opted-in Surface, visible or not. Only context
    /// loss is returned; other allocation failures present directly.
    #[cfg(test)]
    pub(crate) fn plan<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        time: f64,
        limit: u32,
        inputs: &[SurfaceCacheInput],
        targets: &mut A,
    ) -> Result<(), RenderError> {
        let inputs: Vec<_> = inputs.iter().map(|input| (world, *input)).collect();
        self.plan_outputs(&[world], time, limit, &inputs, targets)
    }

    pub(crate) fn plan_outputs<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        worlds: &[WorldId],
        time: f64,
        limit: u32,
        inputs: &[(WorldId, SurfaceCacheInput)],
        targets: &mut A,
    ) -> Result<(), RenderError> {
        self.stamp += 1;
        self.planned = worlds.iter().copied().collect();
        self.counts = SurfaceCacheFrameCounts::default();
        let limit = limit.min(SURFACE_CACHE_MAX_DIMENSION);
        for (world, input) in inputs {
            self.select(*world, time, limit, input);
        }
        self.apply_budget(targets);
        for world in worlds {
            self.allocate(*world, time, targets)?;
            self.count_plan(*world);
        }
        Ok(())
    }

    /// Count this plan's presentations for the frame statistics.
    fn count_plan(&mut self, world: WorldId) {
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

            match entry.presentation {
                SurfaceCachePresentation::Fallback => self.counts.fallbacks += 1,
                SurfaceCachePresentation::Animated => self.counts.animated += 1,
                _ => {}
            }
        }
    }

    fn select(&mut self, world: WorldId, time: f64, limit: u32, input: &SurfaceCacheInput) {
        use SurfaceCachePresentation as P;

        let stamp = self.stamp;
        let budget = self.budget_bytes.saturating_sub(self.reserved_bytes);
        let entry = self
            .entries
            .entry((world, input.entity))
            .or_insert_with(|| SurfaceCacheEntry::new(input.policy.initial_band(input.distance)));

        entry.live = stamp;
        entry.band = input.policy.band(input.distance, entry.band);
        entry.revisions = (input.paint_revision, input.resource_revision);
        let shown = std::mem::replace(&mut entry.shown, false);

        // Culled frames neither advance nor break the animated-paint tracking.
        if !input.visible {
            entry.action = SurfaceCacheAction::Culled;
            entry.presentation = P::Culled;
            return;
        }

        let paint_changed =
            std::mem::replace(&mut entry.seen_paint, input.paint_revision) != input.paint_revision;
        let direct = if input.layered {
            Some(P::Layered)
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
                super::quality::image_size(
                    input.pixel_demand,
                    input.policy.resolution_scale,
                    limit,
                    entry.image.as_ref().map(|image| image.size),
                )
            },
            |_| None,
        );
        let (Some(desired), None) = (desired, direct) else {
            entry.action = SurfaceCacheAction::Direct;
            entry.presentation = direct.unwrap_or(P::Unavailable);
            entry.repaint_streak = 0;
            entry.animated = false;
            return;
        };

        // An animated Surface draws directly until its paint holds, then repaints
        // its stale image through the ordinary rules below.
        if entry.animated {
            entry.settled = if paint_changed {
                0
            } else {
                entry.settled + 1
            };
            if entry.settled < SURFACE_CACHE_SETTLE_FRAMES {
                entry.action = SurfaceCacheAction::Direct;
                entry.presentation = P::Animated;
                return;
            }

            entry.animated = false;
            entry.repaint_streak = 0;
        }

        entry.desired = desired;
        entry.desired_capacity = entry
            .image
            .as_ref()
            .filter(|image| super::quality::fits(desired, image.capacity))
            .map_or_else(
                || super::quality::image_capacity(desired, limit),
                |image| image.capacity,
            );
        let resized = entry
            .image
            .as_ref()
            .is_none_or(|image| image.size != desired);
        // Whether an out-of-date image repaints because its refresh interval elapsed.
        let mut capped = false;
        let repaint = match entry.painted {
            _ if resized => true,
            None => true,
            Some(painted) if painted.resource_revision != input.resource_revision => true,
            // A skipped resource arrived: repaint now, whatever the cadence.
            Some(painted) if !painted.complete && input.missing_resident => true,
            // Up to date. An incomplete image matches direct presentation, which
            // skips the same primitives, until a missing resource arrives. Text
            // drawn analytically while it waited for atlas population refines at
            // the refresh cadence once some of it can sample the atlas.
            Some(painted) if painted.paint_revision == input.paint_revision => {
                painted.unpopulated > 0
                    && input.text_populated
                    && time - entry.painted_at
                        >= input.policy.refresh_interval_at(entry.band) - 1e-9
            }
            // Out-of-date content may stay on screen only while it was on screen.
            Some(_) if !shown => true,
            Some(_) => {
                let due =
                    time - entry.painted_at >= input.policy.refresh_interval_at(entry.band) - 1e-9;
                capped = due;
                due
            }
        };

        // Paint that changes and is repainted at the refresh cap on consecutive frames
        // costs more cached than direct: after that many repaints, the next one
        // presents directly instead until the paint settles.
        let capped_repaint = capped && paint_changed;
        if capped_repaint && entry.repaint_streak >= SURFACE_CACHE_ANIMATED_FRAMES {
            entry.animated = true;
            entry.settled = 0;
            entry.action = SurfaceCacheAction::Direct;
            entry.presentation = P::Animated;
            return;
        }

        entry.repaint_streak = if capped_repaint {
            entry.repaint_streak + 1
        } else {
            0
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
                demand += image_bytes(entry.desired_capacity);
            } else if let Some(image) = &entry.image {
                idle += image.bytes();
            }
        }

        if demand + idle <= self.budget_bytes.saturating_sub(self.reserved_bytes) {
            return;
        }

        // Under pressure remove optional headroom before denying active quality.
        // Reallocation invalidates the retained paint even when active size holds.
        {
            for entry in self.entries.values_mut() {
                if entry.live == stamp
                    && entry.cached()
                    && (demand > self.budget_bytes.saturating_sub(self.reserved_bytes)
                        || entry.image.is_none())
                {
                    entry.desired_capacity = entry.desired;
                    if entry
                        .image
                        .as_ref()
                        .is_some_and(|image| image.capacity != entry.desired_capacity)
                    {
                        entry.action = SurfaceCacheAction::Repaint;
                        entry.presentation = SurfaceCachePresentation::Repainted;
                        entry.painted = None;
                    }
                }
            }
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
            let bytes = image_bytes(entry.desired_capacity);
            if admitted + bytes <= self.budget_bytes.saturating_sub(self.reserved_bytes) {
                admitted += bytes;
                continue;
            }

            entry.action = SurfaceCacheAction::Direct;
            entry.presentation = SurfaceCachePresentation::Fallback;
            entry.painted = None;
            if let Some(image) = entry.image.take() {
                self.resident_bytes -= image.bytes();
                self.resident_images -= 1;
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
            if admitted + idle <= self.budget_bytes.saturating_sub(self.reserved_bytes) {
                break;
            }

            let key = self.order[index].2;
            let entry = self.entries.get_mut(&key).expect("ordered entry");
            if let Some(image) = entry.image.take() {
                idle -= image.bytes();
                self.resident_bytes -= image.bytes();
                self.resident_images -= 1;
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
            let capacity = entry.desired_capacity;
            let allocated = match entry.image.as_mut() {
                Some(image) if image.capacity == capacity => {
                    image.size = desired;
                    continue;
                }
                Some(image) => targets.resize(&mut image.target, capacity).map(|()| None),
                None => targets.create(capacity).map(Some),
            };

            match allocated {
                Ok(created) => {
                    if let Some(target) = created {
                        entry.image = Some(SurfaceCacheImage {
                            target,
                            size: desired,
                            capacity,
                        });
                        self.resident_images += 1;
                    } else if let Some(image) = entry.image.as_mut() {
                        resident -= image.bytes();
                        image.size = desired;
                        image.capacity = capacity;
                    }

                    resident += image_bytes(capacity);
                    entry.painted = None;
                    self.counts.allocations += 1;
                }
                Err(error) => {
                    if let Some(image) = entry.image.take() {
                        resident -= image.bytes();
                        self.resident_images -= 1;
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
            .filter(|entry| entry.live == self.stamp && self.planned.contains(&world))
            .map(|entry| entry.action)
    }

    /// The image and its size for a Surface planned to repaint or reuse.
    pub(crate) fn image(&self, world: WorldId, entity: EntityId) -> Option<(&T, [u32; 2])> {
        self.entries
            .get(&(world, entity))
            .and_then(|entry| entry.image.as_ref())
            .map(|image| (&image.target, image.size))
    }

    pub(crate) fn take_image(
        &mut self,
        world: WorldId,
        entity: EntityId,
    ) -> Option<(T, [u32; 2], [u32; 2])> {
        let image = self.entries.get_mut(&(world, entity))?.image.take()?;
        self.resident_bytes -= image.bytes();
        self.resident_images -= 1;
        Some((image.target, image.size, image.capacity))
    }

    pub(crate) fn put_image(
        &mut self,
        world: WorldId,
        entity: EntityId,
        target: T,
        size: [u32; 2],
        capacity: [u32; 2],
    ) {
        let entry = self
            .entries
            .get_mut(&(world, entity))
            .expect("planned cache entry");
        assert!(entry.image.is_none());
        entry.image = Some(SurfaceCacheImage {
            target,
            size,
            capacity,
        });
        self.resident_bytes += image_bytes(capacity);
        self.resident_images += 1;
    }

    pub(crate) fn forget_surface<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        entity: EntityId,
        targets: &mut A,
    ) {
        if let Some(image) = self
            .entries
            .remove(&(world, entity))
            .and_then(|entry| entry.image)
        {
            self.resident_bytes -= image.bytes();
            self.resident_images -= 1;
            targets.delete(image.target);
        }
    }

    /// Record a completed repaint. `missing` lists the resources whose
    /// primitives were skipped because they were not resident; the image is
    /// repainted as soon as a plan reports one of them resident. `unpopulated`
    /// counts text runs drawn analytically while they waited for atlas
    /// population; the image refines at its refresh cadence once a plan reports
    /// that fewer runs wait.
    pub(crate) fn repainted(
        &mut self,
        world: WorldId,
        entity: EntityId,
        time: f64,
        missing: &[AssetKey],
        unpopulated: u32,
    ) {
        let Some(entry) = self.entries.get_mut(&(world, entity)) else {
            return;
        };

        entry.painted = Some(SurfaceCachePaint {
            paint_revision: entry.revisions.0,
            resource_revision: entry.revisions.1,
            complete: missing.is_empty(),
            unpopulated,
        });
        entry.missing.clear();
        entry.missing.extend_from_slice(missing);
        entry.missing.sort_unstable();
        entry.missing.dedup();
        entry.painted_at = time;
        entry.repaints = entry.repaints.saturating_add(1);
        self.counts.repaints += 1;
    }

    /// Text runs the Surface's current image drew analytically while they waited
    /// for atlas population; zero without an image. The caller compares them with
    /// the runs waiting now in the next plan.
    pub(crate) fn unpopulated(&self, world: WorldId, entity: EntityId) -> u32 {
        self.entries
            .get(&(world, entity))
            .and_then(|entry| entry.painted)
            .map_or(0, |painted| painted.unpopulated)
    }

    /// Resources the Surface's current image skipped; empty for a complete
    /// image or no image. The caller reports their residency in the next plan.
    #[cfg(test)]
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
            self.resident_images -= 1;
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

    /// Finish one World's portion of the output-graph plan. A completed plan removes entries of Surfaces no
    /// longer live or opted in and releases idle images and returns `true`;
    /// otherwise every entry is kept.
    pub(crate) fn finish_frame<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        time: f64,
        completed: bool,
        targets: &mut A,
    ) -> bool {
        let planned = self.planned.remove(&world);
        if !(planned && completed) {
            // Nothing planned was necessarily presented.
            for (_, entry) in self.entries.range_mut(world_range(world)) {
                entry.shown = false;
            }
            return false;
        }

        let stamp = self.stamp;
        let mut released = 0;
        let mut images = 0;
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
                images += 1;
                entry.painted = None;
                targets.delete(image.target);
            }

            live
        });
        self.resident_bytes -= released;
        self.resident_images -= images;
        planned
    }

    /// The last completed plan's counts.
    pub(crate) fn counts(&self) -> SurfaceCacheFrameCounts {
        self.counts
    }

    /// Release every entry of one World, leaving other Worlds untouched.
    pub(crate) fn forget_world<A: SurfaceCacheTargets<Target = T>>(
        &mut self,
        world: WorldId,
        targets: &mut A,
    ) {
        let mut released = 0;
        let mut images = 0;
        self.entries.retain(|&(owner, _), entry| {
            if owner != world {
                return true;
            }

            if let Some(image) = entry.image.take() {
                released += image.bytes();
                images += 1;
                targets.delete(image.target);
            }

            false
        });
        self.resident_bytes -= released;
        self.resident_images -= images;
        self.planned.remove(&world);
    }

    /// Release every entry after unload or context loss.
    pub(crate) fn clear<A: SurfaceCacheTargets<Target = T>>(&mut self, targets: &mut A) {
        for (_, entry) in std::mem::take(&mut self.entries) {
            if let Some(image) = entry.image {
                targets.delete(image.target);
            }
        }

        self.resident_bytes = 0;
        self.resident_images = 0;
        self.planned.clear();
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
                    capacity: entry.image.as_ref().map_or([0, 0], |image| image.capacity),
                    repaints: entry.repaints,
                    reuses: entry.reuses,
                    painted_at: entry.painted_at,
                    resident_bytes: entry.image.as_ref().map_or(0, |image| image.bytes() as u32),
                }),
        );
    }

    /// Context-wide resident image count and bytes.
    pub(crate) fn resident(&self) -> (u32, u32) {
        (
            u32::try_from(self.resident_images).unwrap_or(u32::MAX),
            u32::try_from(self.resident_bytes).unwrap_or(u32::MAX),
        )
    }
}

fn world_range(world: WorldId) -> std::ops::RangeInclusive<(WorldId, EntityId)> {
    (world, EntityId::from_bits(0))..=(world, EntityId::from_bits(u64::MAX))
}

/// Resident bytes of an image, four per texel.
fn image_bytes(size: [u32; 2]) -> usize {
    4 * size[0] as usize * size[1] as usize
}

#[cfg(test)]
#[path = "texture_cache_tests.rs"]
mod tests;
