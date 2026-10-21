use super::*;

fn state(mode: AnimationMotionMode, velocity: f64) -> AnimationMotionState {
    AnimationMotionState {
        mode,
        velocity,
        min: 0.0,
        max: 10.0,
    }
}

#[test]
fn critical_spring_matches_closed_form_and_preserves_retarget_velocity() {
    let mut motion = state(
        AnimationMotionMode::Spring {
            target: 5.0,
            frequency: 2.0,
            damping: 1.0,
        },
        3.0,
    );
    let mut position = 2.0;
    let omega = 4.0 * std::f64::consts::PI;
    let expected = 5.0 + (-3.0 + (3.0 - 3.0 * omega) * 0.1) * (-omega * 0.1).exp();
    assert!(!motion.advance(&mut position, 0.1));
    assert!((position - expected).abs() < 1e-12);
    let velocity = motion.velocity;
    motion.mode = AnimationMotionMode::Spring {
        target: 1.0,
        frequency: 2.0,
        damping: 1.0,
    };
    motion.advance(&mut position, 0.0);
    assert!((motion.velocity - velocity).abs() < 1e-12);
}

#[test]
fn spring_and_decay_agree_across_frame_partitions() {
    for mode in [
        AnimationMotionMode::Spring {
            target: 5.0,
            frequency: 1.5,
            damping: 0.7,
        },
        AnimationMotionMode::Spring {
            target: 5.0,
            frequency: 1.5,
            damping: 1.0,
        },
        AnimationMotionMode::Spring {
            target: 5.0,
            frequency: 1.5,
            damping: 2.0,
        },
        AnimationMotionMode::Decay {
            friction: 4.0,
        },
    ] {
        let mut whole = state(mode, 2.0);
        let mut expected = 3.0;
        whole.advance(&mut expected, 0.25);
        for count in [15, 30, 60] {
            let mut split = state(mode, 2.0);
            let mut actual = 3.0;
            for _ in 0..count {
                split.advance(&mut actual, 0.25 / f64::from(count));
            }
            assert!((actual - expected).abs() < 1e-11);
            assert!((split.velocity - whole.velocity).abs() < 1e-11);
        }
    }
}

#[test]
fn decay_distance_bounds_and_long_frame_settling_are_finite() {
    let mut motion = state(
        AnimationMotionMode::Decay {
            friction: 4.0,
        },
        8.0,
    );
    let mut position = 3.0;
    assert!(motion.advance(&mut position, 100.0));
    assert!((position - 5.0).abs() < 1e-12);
    motion.velocity = -1e9;
    assert!(motion.advance(&mut position, 1.0));
    assert_eq!(position, 0.0);
    motion.mode = AnimationMotionMode::Spring {
        target: 4.0,
        frequency: 0.001,
        damping: 100.0,
    };
    assert!(motion.advance(&mut position, f64::MAX));
    assert_eq!((position, motion.velocity), (4.0, 0.0));
}

#[test]
fn hold_retains_release_velocity_and_invalid_motion_is_rejected() {
    let mut motion = state(AnimationMotionMode::Hold, 2.0);
    let mut position = 3.0;
    assert!(!motion.advance(&mut position, 4.0));
    assert_eq!((position, motion.velocity), (3.0, 2.0));
    for velocity in [f64::NAN, f64::INFINITY, 1e16] {
        motion.velocity = velocity;
        assert!(motion.validate(position).is_err());
    }
}
