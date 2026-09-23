use super::*;

fn policy(direct_distance: f32) -> SurfaceCachePolicy {
    SurfaceCachePolicy::new(&SurfaceCache {
        direct_distance,
        texels_per_metre: 512.0,
        max_refresh_hz: 30.0,
    })
    .unwrap()
}

#[test]
fn validation_rejects_non_finite_non_positive_and_excessive_values() {
    let valid = SurfaceCache::default();
    assert!(SurfaceCachePolicy::new(&valid).is_ok());
    assert!(
        SurfaceCachePolicy::new(&SurfaceCache {
            direct_distance: 0.0,
            ..valid
        })
        .is_ok()
    );

    for invalid in [
        SurfaceCache {
            direct_distance: -1.0,
            ..valid
        },
        SurfaceCache {
            direct_distance: f32::NAN,
            ..valid
        },
        SurfaceCache {
            direct_distance: SURFACE_CACHE_MAX_DIRECT_DISTANCE * 2.0,
            ..valid
        },
        SurfaceCache {
            texels_per_metre: 0.0,
            ..valid
        },
        SurfaceCache {
            texels_per_metre: f32::INFINITY,
            ..valid
        },
        SurfaceCache {
            texels_per_metre: SURFACE_CACHE_MAX_TEXELS_PER_METRE * 2.0,
            ..valid
        },
        SurfaceCache {
            max_refresh_hz: -30.0,
            ..valid
        },
        SurfaceCache {
            max_refresh_hz: f32::NAN,
            ..valid
        },
        SurfaceCache {
            max_refresh_hz: SURFACE_CACHE_MAX_REFRESH_HZ * 2.0,
            ..valid
        },
    ] {
        assert_eq!(
            SurfaceCachePolicy::new(&invalid),
            Err(ErrorReason::InvalidValue),
            "{invalid:?}"
        );
    }
}

#[test]
fn initial_bands_double_with_distance_and_end_in_an_unbounded_band() {
    let policy = policy(2.0);
    assert_eq!(policy.initial_band(0.0), 0);
    assert_eq!(policy.initial_band(1.99), 0);
    assert_eq!(policy.initial_band(2.0), 1);
    assert_eq!(policy.initial_band(3.99), 1);
    assert_eq!(policy.initial_band(4.0), 2);
    assert_eq!(policy.initial_band(8.0), 3);
    assert_eq!(policy.initial_band(16.0), 4);
    assert_eq!(policy.initial_band(32.0), SURFACE_CACHE_MAX_BANDS);
    assert_eq!(policy.initial_band(1.0e9), SURFACE_CACHE_MAX_BANDS);

    for distance in [f32::NAN, f32::INFINITY, -1.0] {
        assert_eq!(policy.initial_band(distance), 0, "{distance}");
    }
}

#[test]
fn zero_direct_distance_caches_at_full_density_everywhere() {
    let policy = policy(0.0);
    for distance in [0.0, 0.5, 100.0, 1.0e6] {
        assert_eq!(policy.initial_band(distance), 1);
        assert_eq!(policy.band(distance, 0), 1);
        assert_eq!(policy.band(distance, SURFACE_CACHE_MAX_BANDS), 1);
    }
}

#[test]
fn hysteresis_keeps_the_previous_band_near_its_boundaries() {
    let policy = policy(2.0);

    // Entering band 1 from direct requires 10% past the 2 m boundary.
    assert_eq!(policy.band(2.1, 0), 0);
    assert_eq!(policy.band(2.25, 0), 1);

    // Returning to direct requires 10% inside the boundary.
    assert_eq!(policy.band(1.9, 1), 1);
    assert_eq!(policy.band(1.79, 1), 0);

    // The 4 m boundary between bands 1 and 2 behaves the same both ways.
    assert_eq!(policy.band(4.3, 1), 1);
    assert_eq!(policy.band(4.45, 1), 2);
    assert_eq!(policy.band(3.7, 2), 2);
    assert_eq!(policy.band(3.59, 2), 1);

    // Oscillating inside the window never changes the selection.
    let mut band = policy.initial_band(4.0);
    for distance in [3.7, 4.3, 3.65, 4.35, 4.0] {
        band = policy.band(distance, band);
        assert_eq!(band, 2, "{distance}");
    }

    // Large jumps skip intermediate bands in either direction.
    assert_eq!(policy.band(40.0, 0), SURFACE_CACHE_MAX_BANDS);
    assert_eq!(policy.band(0.5, SURFACE_CACHE_MAX_BANDS), 0);

    // Out-of-range history is clamped to the last band.
    assert_eq!(policy.band(40.0, u8::MAX), SURFACE_CACHE_MAX_BANDS);
}

#[test]
fn density_and_refresh_caps_never_increase_with_distance() {
    for policy in [
        policy(2.0),
        SurfaceCachePolicy::new(&SurfaceCache {
            direct_distance: 1.0,
            texels_per_metre: 4.0,
            max_refresh_hz: 0.25,
        })
        .unwrap(),
    ] {
        let mut density = f32::INFINITY;
        let mut interval = 0.0;
        for band in 1..=SURFACE_CACHE_MAX_BANDS + 2 {
            let next_density = policy.texels_per_metre_at(band);
            let next_interval = policy.refresh_interval_at(band);
            assert!(next_density > 0.0 && next_density <= density, "{band}");
            assert!(
                next_interval.is_finite() && next_interval >= interval,
                "{band}"
            );
            density = next_density;
            interval = next_interval;
        }
    }

    let policy = policy(2.0);
    assert_eq!(policy.texels_per_metre_at(1), 512.0);
    assert_eq!(policy.texels_per_metre_at(2), 256.0);
    assert_eq!(policy.texels_per_metre_at(5), 32.0);
    assert_eq!(policy.refresh_interval_at(0), 0.0);
    assert_eq!(policy.refresh_interval_at(1), 1.0 / 30.0);
    assert_eq!(policy.refresh_interval_at(2), 1.0 / 15.0);
    assert_eq!(policy.refresh_interval_at(5), 1.0 / 1.875);
}

#[test]
fn floors_stop_halving_without_raising_low_authored_values() {
    let policy = SurfaceCachePolicy::new(&SurfaceCache {
        direct_distance: 1.0,
        texels_per_metre: 40.0,
        max_refresh_hz: 1.5,
    })
    .unwrap();
    assert_eq!(policy.texels_per_metre_at(2), 20.0);
    assert_eq!(policy.texels_per_metre_at(3), TEXELS_PER_METRE_FLOOR);
    assert_eq!(policy.refresh_interval_at(2), 1.0 / 0.75);
    assert_eq!(
        policy.refresh_interval_at(3),
        1.0 / f64::from(REFRESH_HZ_FLOOR)
    );

    let low = SurfaceCachePolicy::new(&SurfaceCache {
        direct_distance: 1.0,
        texels_per_metre: 4.0,
        max_refresh_hz: 0.25,
    })
    .unwrap();
    assert_eq!(low.texels_per_metre_at(SURFACE_CACHE_MAX_BANDS), 4.0);
    assert_eq!(low.refresh_interval_at(SURFACE_CACHE_MAX_BANDS), 4.0);
}
