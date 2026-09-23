use super::*;
use std::collections::BTreeSet;

const WORLD: WorldId = WorldId(1);

/// Counting image allocator with failure injection.
#[derive(Default)]
struct Targets {
    next: u32,
    live: BTreeSet<u32>,
    creates: u32,
    resizes: u32,
    fail_create: Option<RenderError>,
    fail_resize: Option<RenderError>,
}

impl SurfaceCacheTargets for Targets {
    type Target = u32;

    fn create(&mut self, size: [u32; 2]) -> Result<u32, RenderError> {
        assert!(size[0] >= 1 && size[1] >= 1);
        self.creates += 1;
        if let Some(error) = self.fail_create.clone() {
            return Err(error);
        }

        self.next += 1;
        self.live.insert(self.next);
        Ok(self.next)
    }

    fn resize(&mut self, target: &mut u32, _: [u32; 2]) -> Result<(), RenderError> {
        assert!(self.live.contains(target));
        self.resizes += 1;
        match self.fail_resize.clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn delete(&mut self, target: u32) {
        assert!(self.live.remove(&target), "each image is deleted once");
    }
}

/// Direct below 4 m, 100 texels per metre and 10 Hz in the first band.
fn policy() -> SurfaceCachePolicy {
    SurfaceCachePolicy {
        direct_distance: 4.0,
        texels_per_metre: 100.0,
        max_refresh_hz: 10.0,
    }
}

fn entity(index: u64) -> EntityId {
    EntityId::from_bits(index)
}

fn input(index: u64, distance: f32, paint: u64, resource: u64) -> SurfaceCacheInput {
    SurfaceCacheInput {
        entity: entity(index),
        policy: policy(),
        clip_size: [1.0, 0.5],
        paint_revision: paint,
        resource_revision: resource,
        interaction: false,
        visible: true,
        distance,
    }
}

struct Store {
    cache: SurfaceTextureCache<u32>,
    targets: Targets,
}

impl Store {
    fn new() -> Self {
        Self {
            cache: SurfaceTextureCache::default(),
            targets: Targets::default(),
        }
    }

    /// Plan, repaint and composite like the service, then finish.
    fn frame_in(
        &mut self,
        world: WorldId,
        time: f64,
        inputs: &[SurfaceCacheInput],
    ) -> SurfaceCacheFrameCounts {
        self.cache
            .plan(world, time, 4096, inputs, &mut self.targets)
            .unwrap();
        for input in inputs {
            match self.cache.action(world, input.entity) {
                Some(SurfaceCacheAction::Repaint) => {
                    self.cache.repainted(world, input.entity, time, true);
                    self.cache.presented(world, input.entity, time);
                }
                Some(SurfaceCacheAction::Reuse) => {
                    self.cache.presented(world, input.entity, time);
                }
                _ => {}
            }
        }

        self.cache
            .finish_frame(world, time, true, &mut self.targets)
            .expect("completed frame counts")
    }

    fn frame(&mut self, time: f64, inputs: &[SurfaceCacheInput]) -> SurfaceCacheFrameCounts {
        self.frame_in(WORLD, time, inputs)
    }

    fn diagnostic(&self, index: u64) -> SurfaceCacheDiagnostic {
        let mut out = Vec::new();
        self.cache.diagnostics(WORLD, &mut out);
        out.into_iter()
            .find(|diagnostic| diagnostic.entity == entity(index))
            .expect("entry")
    }
}

fn counts(
    repaints: u32,
    reuses: u32,
    direct: u32,
    fallbacks: u32,
    allocations: u32,
) -> SurfaceCacheFrameCounts {
    SurfaceCacheFrameCounts {
        repaints,
        reuses,
        direct,
        fallbacks,
        allocations,
    }
}

#[test]
fn cache_size_preserves_aspect_within_the_limit() {
    assert_eq!(cache_size([1.0, 0.5], 100.0, 2048), Some([100, 50]));
    assert_eq!(cache_size([40.0, 10.0], 100.0, 2048), Some([2048, 512]));
    assert_eq!(cache_size([30.0, 40.0], 100.0, 1000), Some([750, 1000]));
    assert_eq!(cache_size([1.0, 0.000_001], 100.0, 2048), Some([100, 1]));
    assert_eq!(cache_size([0.0, 1.0], 100.0, 2048), None);
    assert_eq!(cache_size([f32::NAN, 1.0], 100.0, 2048), None);
    assert_eq!(cache_size([1.0, 1.0], 100.0, 0), None);
}

#[test]
fn first_frame_repaints_and_unchanged_frames_reuse() {
    let mut store = Store::new();

    assert_eq!(
        store.frame(0.0, &[input(1, 5.0, 1, 1)]),
        counts(1, 0, 0, 0, 1)
    );
    assert_eq!(
        store.frame(0.5, &[input(1, 5.0, 1, 1)]),
        counts(0, 1, 0, 0, 0)
    );
    assert_eq!(
        store.frame(9.0, &[input(1, 5.0, 1, 1)]),
        counts(0, 1, 0, 0, 0)
    );

    let diagnostic = store.diagnostic(1);
    assert_eq!(diagnostic.presentation, SurfaceCachePresentation::Reused);
    assert_eq!(diagnostic.band, 1);
    assert_eq!(diagnostic.size, [100, 50]);
    assert_eq!((diagnostic.repaints, diagnostic.reuses), (1, 2));
    assert_eq!(diagnostic.resident_bytes, 4 * 100 * 50);
    assert_eq!(store.cache.resident(), (1, 4 * 100 * 50));
    assert_eq!(store.targets.creates, 1);
}

#[test]
fn paint_edits_coalesce_until_the_refresh_interval() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1)]);

    // Two edits inside the 0.1 s interval keep the displayed image.
    assert_eq!(store.frame(0.04, &[input(1, 5.0, 2, 1)]).reuses, 1);
    assert_eq!(store.frame(0.08, &[input(1, 5.0, 3, 1)]).reuses, 1);

    // The next eligible refresh paints the latest revision once.
    assert_eq!(store.frame(0.1, &[input(1, 5.0, 3, 1)]).repaints, 1);
    assert_eq!(store.frame(0.3, &[input(1, 5.0, 3, 1)]).reuses, 1);
    assert_eq!(store.diagnostic(1).repaints, 2);
    assert_eq!(store.diagnostic(1).painted_at, 0.1);
}

#[test]
fn farther_bands_refresh_less_often() {
    let mut near = Store::new();
    let mut far = Store::new();
    let mut repaints = [0, 0];
    for frame in 0..=60_u32 {
        let time = f64::from(frame) / 60.0;
        let paint = u64::from(frame) + 1;
        repaints[0] += near.frame(time, &[input(1, 5.0, paint, 1)]).repaints;
        repaints[1] += far.frame(time, &[input(1, 20.0, paint, 1)]).repaints;
    }

    assert_eq!(near.diagnostic(1).band, 1);
    assert_eq!(far.diagnostic(1).band, 3);
    // One paint per elapsed interval of 1/10 s and 1/2.5 s, plus the first.
    assert_eq!(repaints, [11, 3]);
}

#[test]
fn continuous_animation_is_not_starved_by_uneven_frames() {
    let mut store = Store::new();
    let mut time = 0.0;
    let mut repaints = 0;
    for frame in 0..200_u64 {
        repaints += store.frame(time, &[input(1, 5.0, frame + 1, 1)]).repaints;
        time += if frame % 3 == 0 {
            0.031
        } else {
            0.007
        };
    }

    // 200 frames advance 3.03 s: at most one repaint per 0.1 s and at least
    // one per 0.1 s plus the longest frame.
    assert!((23..=31).contains(&repaints), "{repaints} repaints");
}

#[test]
fn frozen_world_time_never_repaints_displayed_images() {
    let mut store = Store::new();
    store.frame(2.0, &[input(1, 5.0, 1, 1)]);
    for paint in 2..20 {
        assert_eq!(store.frame(2.0, &[input(1, 5.0, paint, 1)]).repaints, 0);
    }

    assert_eq!(store.frame(2.1, &[input(1, 5.0, 20, 1)]).repaints, 1);
}

#[test]
fn resource_revisions_bypass_the_refresh_interval() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1)]);

    assert_eq!(
        store.frame(0.01, &[input(1, 5.0, 1, 2)]),
        counts(1, 0, 0, 0, 0)
    );
    assert_eq!(store.frame(0.02, &[input(1, 5.0, 1, 2)]).reuses, 1);
}

#[test]
fn incomplete_images_retry_at_the_band_cadence() {
    let mut store = Store::new();
    let inputs = [input(1, 5.0, 1, 1)];
    store
        .cache
        .plan(WORLD, 0.0, 4096, &inputs, &mut store.targets)
        .unwrap();
    store.cache.repainted(WORLD, entity(1), 0.0, false);
    store.cache.presented(WORLD, entity(1), 0.0);
    store
        .cache
        .finish_frame(WORLD, 0.0, true, &mut store.targets);

    assert_eq!(store.frame(0.05, &inputs).reuses, 1);
    assert_eq!(store.frame(0.1, &inputs).repaints, 1);
    assert_eq!(store.frame(0.3, &inputs).reuses, 1);
}

#[test]
fn band_changes_resize_with_hysteresis() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1)]);

    // Band 1 is [4, 8): 8.5 m stays inside the 10 % margin.
    assert_eq!(
        store.frame(0.1, &[input(1, 8.5, 1, 1)]),
        counts(0, 1, 0, 0, 0)
    );
    assert_eq!(store.diagnostic(1).size, [100, 50]);

    // 9 m leaves band 1; the image halves in density and repaints before use.
    assert_eq!(
        store.frame(0.2, &[input(1, 9.0, 1, 1)]),
        counts(1, 0, 0, 0, 1)
    );
    assert_eq!(store.diagnostic(1).band, 2);
    assert_eq!(store.diagnostic(1).size, [50, 25]);
    assert_eq!(store.targets.resizes, 1);

    // Oscillating around 8 m keeps band 2 until 7.2 m.
    for distance in [7.5, 8.4, 7.3, 8.0] {
        assert_eq!(store.frame(0.3, &[input(1, distance, 1, 1)]).reuses, 1);
    }

    assert_eq!(store.frame(0.4, &[input(1, 7.0, 1, 1)]).allocations, 1);
    assert_eq!(store.diagnostic(1).band, 1);
    assert_eq!(store.targets.creates, 1);
}

#[test]
fn near_interaction_and_unavailable_present_directly() {
    let mut store = Store::new();
    assert_eq!(
        store.frame(0.0, &[input(1, 3.0, 1, 1)]),
        counts(0, 0, 1, 0, 0)
    );
    assert_eq!(
        store.diagnostic(1).presentation,
        SurfaceCachePresentation::Near
    );

    let mut interacting = input(1, 5.0, 1, 1);
    interacting.interaction = true;
    assert_eq!(store.frame(0.1, &[interacting]), counts(0, 0, 1, 0, 0));
    assert_eq!(
        store.diagnostic(1).presentation,
        SurfaceCachePresentation::Interaction
    );
    assert_eq!(store.targets.creates, 0);

    store
        .cache
        .plan(WORLD, 0.2, 0, &[input(1, 5.0, 1, 1)], &mut store.targets)
        .unwrap();
    assert_eq!(
        store.cache.action(WORLD, entity(1)),
        Some(SurfaceCacheAction::Direct)
    );
    store
        .cache
        .finish_frame(WORLD, 0.2, true, &mut store.targets);
    assert_eq!(
        store.diagnostic(1).presentation,
        SurfaceCachePresentation::Unavailable
    );
}

#[test]
fn promotion_returns_to_the_image_only_while_it_is_current() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1)]);

    let mut interacting = input(1, 5.0, 1, 1);
    interacting.interaction = true;
    store.frame(0.01, &[interacting]);

    // Unchanged content reuses its image after interaction ends.
    assert_eq!(store.frame(0.02, &[input(1, 5.0, 1, 1)]).reuses, 1);

    // Content changed while direct repaints before the image shows again,
    // even inside the refresh interval.
    interacting.paint_revision = 2;
    store.frame(0.03, &[interacting]);
    assert_eq!(store.frame(0.04, &[input(1, 5.0, 2, 1)]).repaints, 1);
}

#[test]
fn culled_surfaces_do_no_work_and_repaint_stale_content_on_return() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1)]);

    let mut hidden = input(1, 5.0, 2, 1);
    hidden.visible = false;
    assert_eq!(store.frame(0.01, &[hidden]), counts(0, 0, 0, 0, 0));
    assert_eq!(
        store.diagnostic(1).presentation,
        SurfaceCachePresentation::Culled
    );

    assert_eq!(store.frame(0.02, &[input(1, 5.0, 2, 1)]).repaints, 1);
}

#[test]
fn budget_fallback_admits_entities_in_order() {
    let mut store = Store::new();
    store.cache.set_budget(2 * 4 * 100 * 50);
    let inputs = [
        input(1, 5.0, 1, 1),
        input(2, 5.0, 1, 1),
        input(3, 5.0, 1, 1),
    ];

    assert_eq!(store.frame(0.0, &inputs), counts(2, 0, 1, 1, 2));
    assert_eq!(
        store.diagnostic(3).presentation,
        SurfaceCachePresentation::Fallback
    );
    assert_eq!(store.cache.resident(), (2, 2 * 4 * 100 * 50));

    // A stable over-budget set does not churn allocations.
    assert_eq!(store.frame(0.1, &inputs), counts(0, 2, 1, 1, 0));
    assert_eq!(store.targets.creates, 2);
}

#[test]
fn budget_pressure_evicts_the_least_recently_presented_image() {
    let mut store = Store::new();
    store.cache.set_budget(2 * 4 * 100 * 50);
    let hidden = |index| SurfaceCacheInput {
        visible: false,
        ..input(index, 5.0, 1, 1)
    };

    store.frame(0.0, &[input(1, 5.0, 1, 1), input(2, 5.0, 1, 1)]);
    store.frame(0.1, &[hidden(1), input(2, 5.0, 1, 1)]);

    // Surface 3 needs room: 1 was presented longest ago, so it goes first.
    let frame = store.frame(0.2, &[hidden(1), hidden(2), input(3, 5.0, 1, 1)]);
    assert_eq!(frame, counts(1, 0, 0, 0, 1));
    assert_eq!(store.diagnostic(1).size, [0, 0]);
    assert_eq!(store.diagnostic(2).size, [100, 50]);
    assert_eq!(store.cache.resident(), (2, 2 * 4 * 100 * 50));
    assert_eq!(store.targets.live.len(), 2);

    // Lowering the budget releases idle images at the next frame first.
    store.cache.set_budget(4 * 100 * 50);
    store.frame(0.3, &[hidden(1), hidden(2), input(3, 5.0, 1, 1)]);
    assert_eq!(store.diagnostic(2).size, [0, 0]);
    assert_eq!(store.cache.resident(), (1, 4 * 100 * 50));

    // Zero disables caching.
    store.cache.set_budget(0);
    assert_eq!(
        store.frame(0.4, &[input(3, 5.0, 1, 1)]),
        counts(0, 0, 1, 1, 0)
    );
    assert_eq!(store.cache.resident(), (0, 0));
    assert!(store.targets.live.is_empty());
}

#[test]
fn idle_images_are_released_after_ten_seconds() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1)]);
    // Direct presentation keeps the image for a quick return to the band.
    store.frame(1.0, &[input(1, 3.0, 1, 1)]);
    store.frame(9.9, &[input(1, 3.0, 1, 1)]);
    assert_eq!(store.cache.resident().0, 1);

    store.frame(10.0, &[input(1, 3.0, 1, 1)]);
    assert_eq!(store.cache.resident(), (0, 0));
    assert!(store.targets.live.is_empty());
    assert_eq!(
        store.diagnostic(1).presentation,
        SurfaceCachePresentation::Near
    );
}

#[test]
fn surfaces_leaving_the_plan_release_their_entries_after_completed_frames() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1), input(2, 5.0, 1, 1)]);

    // A failed frame keeps everything.
    store
        .cache
        .plan(WORLD, 0.1, 4096, &[input(1, 5.0, 1, 1)], &mut store.targets)
        .unwrap();
    assert_eq!(
        store
            .cache
            .finish_frame(WORLD, 0.1, false, &mut store.targets),
        None
    );
    assert_eq!(store.cache.resident().0, 2);

    store.frame(0.2, &[input(1, 5.0, 1, 1)]);
    let mut out = Vec::new();
    store.cache.diagnostics(WORLD, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(store.targets.live.len(), 1);
}

#[test]
fn failed_frames_do_not_count_as_presented() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1)]);
    store
        .cache
        .plan(
            WORLD,
            0.01,
            4096,
            &[input(1, 5.0, 2, 1)],
            &mut store.targets,
        )
        .unwrap();
    store
        .cache
        .finish_frame(WORLD, 0.01, false, &mut store.targets);

    // Stale content was not on screen, so it repaints before showing.
    assert_eq!(store.frame(0.02, &[input(1, 5.0, 2, 1)]).repaints, 1);
}

#[test]
fn worlds_share_the_budget_and_forget_only_their_own_entries() {
    let mut store = Store::new();
    store.cache.set_budget(4 * 100 * 50);
    let other = WorldId(2);

    store.frame_in(WORLD, 0.0, &[input(1, 5.0, 1, 1)]);
    let frame = store.frame_in(other, 0.0, &[input(1, 5.0, 1, 1)]);
    assert_eq!(frame.repaints, 1);
    assert_eq!(store.diagnostic(1).size, [0, 0]);
    assert_eq!(store.cache.resident(), (1, 4 * 100 * 50));

    store.cache.set_budget(DEFAULT_SURFACE_CACHE_BUDGET_BYTES);
    store.frame_in(WORLD, 0.1, &[input(1, 5.0, 1, 1)]);
    assert_eq!(store.cache.resident().0, 2);

    store.cache.forget_world(other, &mut store.targets);
    assert_eq!(store.cache.resident(), (1, 4 * 100 * 50));
    let mut out = Vec::new();
    store.cache.diagnostics(other, &mut out);
    assert!(out.is_empty());

    store.cache.clear(&mut store.targets);
    assert_eq!(store.cache.resident(), (0, 0));
    assert!(store.targets.live.is_empty());
}

#[test]
fn allocation_failures_fall_back_and_retry_after_the_back_off() {
    let mut store = Store::new();
    store.targets.fail_create = Some(RenderError::RenderDevice("injected".into()));

    assert_eq!(
        store.frame(0.0, &[input(1, 5.0, 1, 1)]),
        counts(0, 0, 1, 1, 0)
    );
    assert_eq!(
        store.diagnostic(1).presentation,
        SurfaceCachePresentation::Fallback
    );

    store.targets.fail_create = None;
    assert_eq!(
        store.frame(0.5, &[input(1, 5.0, 1, 1)]),
        counts(0, 0, 1, 1, 0)
    );
    assert_eq!(store.targets.creates, 1);
    assert_eq!(
        store.frame(1.0, &[input(1, 5.0, 1, 1)]),
        counts(1, 0, 0, 0, 1)
    );

    // A failed resize releases the image.
    store.targets.fail_resize = Some(RenderError::RenderDevice("injected".into()));
    assert_eq!(store.frame(1.1, &[input(1, 9.0, 1, 1)]).fallbacks, 1);
    assert!(store.targets.live.is_empty());
    assert_eq!(store.cache.resident(), (0, 0));
}

#[test]
fn context_loss_during_allocation_fails_the_plan() {
    let mut store = Store::new();
    store.targets.fail_create = Some(RenderError::ContextLost);

    assert_eq!(
        store
            .cache
            .plan(WORLD, 0.0, 4096, &[input(1, 5.0, 1, 1)], &mut store.targets),
        Err(RenderError::ContextLost)
    );
    assert_eq!(
        store
            .cache
            .finish_frame(WORLD, 0.0, false, &mut store.targets),
        None
    );
}

#[test]
fn a_failed_composite_is_not_counted_as_a_reuse() {
    let mut store = Store::new();
    store.frame(0.0, &[input(1, 5.0, 1, 1)]);
    store
        .cache
        .plan(WORLD, 0.1, 4096, &[input(1, 5.0, 1, 1)], &mut store.targets)
        .unwrap();
    store
        .cache
        .failed(WORLD, entity(1), 0.1, &mut store.targets);

    let frame = store
        .cache
        .finish_frame(WORLD, 0.1, true, &mut store.targets)
        .unwrap();
    assert_eq!(frame, counts(0, 0, 1, 1, 0));
    assert_eq!(store.diagnostic(1).reuses, 0);
    assert!(store.targets.live.is_empty());
}

#[test]
fn device_limits_cap_the_image_size() {
    let mut store = Store::new();
    let mut large = input(1, 5.0, 1, 1);
    large.clip_size = [100.0, 50.0];
    store
        .cache
        .plan(WORLD, 0.0, 1024, &[large], &mut store.targets)
        .unwrap();
    store
        .cache
        .finish_frame(WORLD, 0.0, true, &mut store.targets);
    assert_eq!(store.diagnostic(1).size, [1024, 512]);

    // Larger device limits still stop at the service maximum.
    store.frame(0.1, &[large]);
    assert_eq!(store.diagnostic(1).size, [2048, 1024]);
}
