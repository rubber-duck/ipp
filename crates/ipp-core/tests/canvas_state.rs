//! Canvas System state: creation seed, ordered System command, query and persistence.

#![cfg(feature = "surfaces")]

mod support;

use ipp_core::services::world_serialization::{
    WorldGraphSnapshot, WorldLoadOptions, WorldPersistenceLimits,
};
use ipp_core::{
    CanvasState, CanvasStateUpdate, ErrorReason, HostRuntime, WorldConstructionError,
    WorldCreateOptions, WorldId, WorldLimits,
};
use support::selection::{CANVAS, SPATIAL};

fn canvas_world(host: &mut HostRuntime, canvas: Option<CanvasState>) -> WorldId {
    host.create_world_with_options(
        WorldLimits::default(),
        WorldCreateOptions {
            canvas,
            ..WorldCreateOptions::new(CANVAS.iter().copied())
        },
    )
    .unwrap()
}

fn state(host: &mut HostRuntime, world: WorldId) -> CanvasState {
    host.world_mut(world).unwrap().canvas_state().unwrap().state
}

#[test]
fn creation_seeds_the_state_and_absence_selects_the_defaults() {
    let mut host = HostRuntime::new();
    let seeded = CanvasState {
        extent: [320.0, 200.0],
        units_per_metre: 400.0,
    };
    let world = canvas_world(&mut host, Some(seeded));
    let record = host.world_mut(world).unwrap().canvas_state().unwrap();
    assert_eq!(record.state, seeded);
    assert_eq!(record.evaluated, None);

    let default = canvas_world(&mut host, None);
    assert_eq!(state(&mut host, default), CanvasState::default());
    assert_eq!(
        CanvasState::default(),
        CanvasState {
            extent: [1.0, 1.0],
            units_per_metre: 1.0,
        }
    );
}

#[test]
fn creation_refuses_state_for_an_unselected_system_or_invalid_values() {
    let mut host = HostRuntime::new();
    let refused = host.create_world_with_options(
        WorldLimits::default(),
        WorldCreateOptions {
            canvas: Some(CanvasState::default()),
            ..WorldCreateOptions::new(SPATIAL.iter().copied())
        },
    );
    assert!(matches!(
        refused,
        Err(WorldConstructionError::Canvas(
            ErrorReason::UnsupportedDependency
        ))
    ));
    for invalid in [
        CanvasState {
            extent: [0.0, 1.0],
            units_per_metre: 1.0,
        },
        CanvasState {
            extent: [1.0, f32::INFINITY],
            units_per_metre: 1.0,
        },
        CanvasState {
            extent: [1.0, 1.0],
            units_per_metre: -2.0,
        },
        CanvasState {
            extent: [1.0, 1.0],
            units_per_metre: f32::NAN,
        },
    ] {
        let refused = host.create_world_with_options(
            WorldLimits::default(),
            WorldCreateOptions {
                canvas: Some(invalid),
                ..WorldCreateOptions::new(CANVAS.iter().copied())
            },
        );
        assert!(matches!(
            refused,
            Err(WorldConstructionError::Canvas(ErrorReason::InvalidValue))
        ));
    }
    assert!(host.list_worlds().is_empty());

    let spatial = host.create_world(WorldLimits::default(), SPATIAL).unwrap();
    let mut world = host.world_mut(spatial).unwrap();
    assert_eq!(
        world.canvas_state().unwrap_err(),
        ErrorReason::UnsupportedDependency
    );
    assert_eq!(
        world
            .enqueue_canvas_state_update(CanvasStateUpdate::default())
            .unwrap_err(),
        ErrorReason::UnsupportedDependency
    );
}

#[test]
fn sparse_updates_apply_in_order_and_an_invalid_update_has_no_effect() {
    let mut host = HostRuntime::new();
    let world = canvas_world(&mut host, None);
    let mut context = host.world_mut(world).unwrap();
    for update in [
        CanvasStateUpdate {
            extent: Some([640.0, 480.0]),
            units_per_metre: None,
        },
        CanvasStateUpdate {
            extent: None,
            units_per_metre: Some(250.0),
        },
    ] {
        context.enqueue_canvas_state_update(update).unwrap();
    }
    // Commands wait for the ordered mutation boundary.
    assert_eq!(
        context.canvas_state().unwrap().state,
        CanvasState::default()
    );
    context.step(0.0).unwrap();
    let applied = CanvasState {
        extent: [640.0, 480.0],
        units_per_metre: 250.0,
    };
    assert_eq!(context.canvas_state().unwrap().state, applied);

    for invalid in [
        CanvasStateUpdate {
            extent: Some([10.0, 0.0]),
            units_per_metre: Some(2.0),
        },
        CanvasStateUpdate {
            extent: None,
            units_per_metre: Some(f32::NAN),
        },
    ] {
        context.enqueue_canvas_state_update(invalid).unwrap();
        context.step(0.0).unwrap();
        assert_eq!(context.canvas_state().unwrap().state, applied);
    }
}

#[test]
fn state_persists_and_a_missing_payload_restores_the_defaults() {
    let mut host = HostRuntime::new();
    let saved = CanvasState {
        extent: [96.0, 48.0],
        units_per_metre: 12.5,
    };
    let world = canvas_world(&mut host, Some(saved));
    let limits = WorldPersistenceLimits::default();
    let bytes = host.save_world(world, 7, limits).unwrap();
    let mut snapshot = WorldGraphSnapshot::decode(&bytes, 7, limits).unwrap();
    let payload = snapshot.nodes[0]
        .world
        .systems
        .get(ipp_core::systems::canvas::CanvasSystem::ID.0)
        .unwrap()
        .clone();
    let expected: Vec<u8> = [96.0f32, 48.0, 12.5]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    assert_eq!(payload, expected);

    let load = |host: &mut HostRuntime, bytes: &[u8], name: &str| {
        host.load_world(
            bytes,
            7,
            WorldLoadOptions {
                symbolic_id: Some(name.into()),
                ..Default::default()
            },
            WorldLimits::default(),
            limits,
        )
        .map(|result| result.root.id())
    };
    let restored = load(&mut host, &bytes, "restored").unwrap();
    assert_eq!(state(&mut host, restored), saved);

    let systems = &mut snapshot.nodes[0].world.systems;
    systems.remove(ipp_core::systems::canvas::CanvasSystem::ID.0);
    let bytes = snapshot.encode(7, limits).unwrap();
    let defaults = load(&mut host, &bytes, "defaults").unwrap();
    assert_eq!(state(&mut host, defaults), CanvasState::default());

    for invalid in [
        payload[..8].to_vec(),
        [payload.clone(), vec![0]].concat(),
        [0.0f32, 1.0, 1.0]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    ] {
        snapshot.nodes[0].world.systems.insert(
            ipp_core::systems::canvas::CanvasSystem::ID.0.to_owned(),
            invalid,
        );
        let bytes = snapshot.encode(7, limits).unwrap();
        assert!(load(&mut host, &bytes, "invalid").is_err());
        assert!(
            host.list_worlds()
                .iter()
                .all(|world| world.metadata.symbolic_id != "invalid")
        );
    }
}
