use super::*;
use std::cell::Cell;

#[test]
fn interval_reuse_matches_stateless_sampling_through_seeks_and_endpoints() {
    let track = AnimationTrack::<f32> {
        target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::SCALAR,
            offsets: vec![0],
        }),
        keys: vec![
            AnimationKeyframe {
                time: 0.2,
                value: 3.0,
                interpolation: AnimationInterpolation::Linear,
            },
            AnimationKeyframe {
                time: 0.7,
                value: -2.0,
                interpolation: AnimationInterpolation::Step,
            },
            AnimationKeyframe {
                time: 1.3,
                value: 5.0,
                interpolation: AnimationInterpolation::Bezier {
                    time1: 1.5,
                    value1: 8.0,
                    time2: 2.4,
                    value2: -1.0,
                },
            },
            AnimationKeyframe {
                time: 3.0,
                value: 9.0,
                interpolation: AnimationInterpolation::Step,
            },
        ],
    };
    let cursor = Cell::new(0);
    let mut segment = None;
    let times = [
        -1.0, 0.0, 0.2, 0.21, 0.5, 0.7, 0.9, 1.3, 1.5, 2.0, 3.0, 10.0, 0.0, 2.3, 0.4,
    ];
    for time in times.into_iter().chain(times.into_iter().rev()) {
        assert_eq!(
            track.sample_cached(time, &cursor),
            track.sample(time),
            "time {time}"
        );
        assert_eq!(
            track.sample_segment(time, &mut segment),
            track.sample(time),
            "segment at {time}"
        );
    }
    let mut single = track.clone();
    single.keys.truncate(1);
    // A recreated payload can have a different address; a cursor never borrows it.
    for time in times {
        assert_eq!(single.sample_cached(time, &cursor), single.sample(time));
    }
}

#[test]
fn rotation_segment_cache_preserves_normalization_and_shortest_arc() {
    let track = AnimationTrack::<[f32; 4]> {
        target: AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::TRANSFORM,
            offsets: vec![12, 16, 20, 24],
        }),
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: [0.0, 0.0, 0.0, 2.0],
                interpolation: AnimationInterpolation::Linear,
            },
            AnimationKeyframe {
                time: 1.0,
                value: [0.0, 0.0, -2.0, -2.0],
                interpolation: AnimationInterpolation::Step,
            },
        ],
    };
    let mut segment = None;
    for time in [0.0, 0.1, 0.2, 0.9, 1.0, 3.0, 0.5, 0.0] {
        let actual = track.sample_segment(time, &mut segment);
        assert_eq!(actual, track.sample(time));
        assert!((actual.into_iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-6);
    }
}
