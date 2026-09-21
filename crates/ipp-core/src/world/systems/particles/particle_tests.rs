use super::simulation::simulate;
use super::*;

fn identity() -> [f32; 16] {
    [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]
}

#[test]
fn births_have_stable_ids_and_subframe_ages() {
    let emitter = ParticleEmitter {
        rate: 20.0,
        lifetime: 3.0,
        shape: 1,
        speed_random: 0.7,
        spread: 0.8,
        acceleration_y: -1.0,
        drag: 0.3,
        ..Default::default()
    };
    let mut one = ParticleRuntimeState::default();
    let mut split = ParticleRuntimeState::default();
    simulate(&emitter, &mut one, 1.0, identity(), None);
    for _ in 0..10 {
        simulate(&emitter, &mut split, 0.1, identity(), None);
    }
    assert_eq!(one.particles.len(), 20);
    assert_eq!(split.particles.len(), 20);
    for (a, b) in one.particles.iter().zip(&split.particles) {
        assert_eq!(a.id, b.id);
        assert!((a.age - b.age).abs() < 1e-8);
        for i in 0..3 {
            assert!((a.position[i] - b.position[i]).abs() < 1e-5);
        }
    }
}

#[test]
fn disable_drains_restart_resets_and_clone_is_empty() {
    let mut emitter = ParticleEmitter {
        burst: 4,
        rate: 0.0,
        lifetime: 1.0,
        ..Default::default()
    };
    let mut state = ParticleRuntimeState::default();
    simulate(&emitter, &mut state, 0.0, identity(), None);
    assert_eq!(state.particles.len(), 4);
    assert!(state.clone().particles.is_empty());
    emitter.enabled = false;
    simulate(&emitter, &mut state, 1.0, identity(), None);
    assert!(state.particles.is_empty());
    emitter.enabled = true;
    emitter.restart += 1;
    simulate(&emitter, &mut state, 0.0, identity(), None);
    assert_eq!(state.particles[0].id, 0);
}

#[test]
fn delayed_world_birth_captures_transform_and_does_not_follow_emitter() {
    let emitter = ParticleEmitter {
        burst: 1,
        rate: 0.0,
        delay: 1.0,
        space: 1,
        speed: 0.0,
        ..Default::default()
    };
    let mut state = ParticleRuntimeState::default();
    let mut model = identity();
    model[12] = 4.0;
    simulate(&emitter, &mut state, 0.5, model, None);
    assert!(state.particles.is_empty());
    simulate(&emitter, &mut state, 0.5, model, None);
    assert_eq!(state.particles[0].position, [4.0, 0.0, 0.0]);
    model[12] = 10.0;
    simulate(&emitter, &mut state, 0.1, model, None);
    assert_eq!(state.particles[0].position, [4.0, 0.0, 0.0]);
}

fn sample(id: u64, x: f32) -> ParticleCacheSample {
    ParticleCacheSample {
        id,
        birth: 0.0,
        death: 2.0,
        position: [x, 0., 0.],
        velocity: [0.; 3],
        rotation: [0., 0., 0., 1.],
        size: 1.0,
    }
}

#[test]
fn cache_directory_and_ids_survive_scrubbing_and_reject_malformed_data() {
    let cache = ParticleCache {
        space: 1,
        frames: vec![
            ParticleCacheFrame {
                time: 0.0,
                samples: vec![sample(10, 0.0), sample(20, 50.0)],
            },
            ParticleCacheFrame {
                time: 1.0,
                samples: vec![sample(10, 2.0), sample(30, 100.0)],
            },
        ],
    };
    let bytes = cache.encode().unwrap();
    assert_eq!(ParticleCache::decode(&bytes).unwrap(), cache);
    let mut state = ParticleRuntimeState::default();
    cache.sample(0.5, &mut state);
    assert_eq!(state.particles[0].position[0], 1.0);
    assert_eq!(state.particles[1].position[0], 50.0);
    cache.sample(1.0, &mut state);
    assert_eq!(state.particles[1].id, 30);
    cache.sample(0.0, &mut state);
    assert_eq!(state.particles[1].id, 20);
    for len in [0, 15, bytes.len() - 1] {
        assert!(ParticleCache::decode(&bytes[..len]).is_err());
    }
    let mut bad = bytes.clone();
    bad[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(ParticleCache::decode(&bad).is_err());
}

#[test]
fn capacity_admission_is_chronological_across_host_steps() {
    let e = ParticleEmitter {
        capacity: 2,
        rate: 20.0,
        lifetime: 0.3,
        ..Default::default()
    };
    let mut whole = ParticleRuntimeState::default();
    let mut split = ParticleRuntimeState::default();
    simulate(&e, &mut whole, 1.0, identity(), None);
    for _ in 0..20 {
        simulate(&e, &mut split, 0.05, identity(), None);
    }
    assert_eq!(
        whole.particles.iter().map(|p| p.id).collect::<Vec<_>>(),
        split.particles.iter().map(|p| p.id).collect::<Vec<_>>()
    );
    assert!(whole.particles.len() <= 2);
    assert_eq!(whole.births, 20);
}
