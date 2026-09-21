use super::*;
use crate::components::schema::FieldValue;

fn summary() -> AnimationControllerTransitionState {
    AnimationControllerTransitionState {
        duration: 2.0,
        elapsed: 0.5,
        easing: AnimationTransitionEasing::Smoothstep,
        pending: false,
    }
}

fn property(offset: u32) -> AnimationTrackTarget {
    AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: crate::ComponentValue::SCALAR,
        offsets: vec![offset],
    })
}

fn controller(id: u64, transition: bool) -> AnimationControllerSnapshot {
    AnimationControllerSnapshot {
        id: AnimationControllerId::from_bits(id),
        description: AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: "asset://7/11".into(),
                variant: 2,
                track: 3,
                target: EntityId::from_bits(9),
                property: property(0),
                weight: 0.75,
                additive: false,
                reference_time: 0.25,
                repeat: true,
            }],
            speed: -1.5,
            looping: true,
        },
        state: AnimationPlaybackStatus::Playing,
        time: 1.25,
        transition: transition.then(summary),
    }
}

fn live_and_frozen_state() -> AnimationPersistentState {
    let live = controller(1, true);
    let frozen = controller(2, true);
    let mut live_source = live.clone();
    live_source.transition = None;
    live_source.time = 0.75;
    let mut bindings = frozen.clone();
    bindings.transition = None;
    AnimationPersistentState {
        next_id: 3,
        controllers: vec![live, frozen],
        transitions: vec![
            AnimationPersistentTransition {
                id: AnimationControllerId::from_bits(1),
                source: AnimationPersistentTransitionSource::Live(live_source),
                start_time: AnimationTransitionStartTime::MatchPhase,
            },
            AnimationPersistentTransition {
                id: AnimationControllerId::from_bits(2),
                source: AnimationPersistentTransitionSource::Frozen {
                    values: vec![
                        AnimationFrozenTransitionValue {
                            target: EntityId::from_bits(9),
                            property: property(0),
                            value: AnimationValue::Field(FieldValue::F32(3.0)),
                            baseline: AnimationValue::Field(FieldValue::F32(4.0)),
                        },
                        AnimationFrozenTransitionValue {
                            target: EntityId::from_bits(9),
                            property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                                component: crate::ComponentValue::TRANSFORM,
                                offsets: vec![12, 16, 20, 24],
                            }),
                            value: AnimationValue::Rotation([0.0, 0.0, 0.0, 1.0]),
                            baseline: AnimationValue::Rotation([0.0, 0.0, 1.0, 0.0]),
                        },
                    ],
                    bindings,
                    reference_time: 0.75,
                    reference_duration: 2.0,
                },
                start_time: AnimationTransitionStartTime::Seek(0.25),
            },
        ],
        directional_starts: vec![AnimationControllerId::from_bits(1)],
    }
}

fn unchecked_bytes(state: &AnimationPersistentState) -> Vec<u8> {
    let mut writer = WorldBinaryWriter::new(1 << 20);
    writer.u32(4).unwrap();
    encode(&mut writer, state).unwrap();
    writer.bytes
}

#[test]
fn version_five_round_trips_transitions_and_directional_starts() {
    let state = live_and_frozen_state();
    let bytes = state.encode(1 << 20).unwrap();
    assert_eq!(
        AnimationPersistentState::decode(&bytes, 1 << 20).unwrap(),
        state
    );
}

#[test]
fn direct_snapshot_encoding_rejects_nested_transition_sources() {
    let mut state = live_and_frozen_state();
    let AnimationPersistentTransitionSource::Live(source) = &mut state.transitions[0].source else {
        unreachable!();
    };
    source.transition = Some(summary());
    let bytes = unchecked_bytes(&state);
    assert!(AnimationPersistentState::decode(&bytes, 1 << 20).is_err());
}

#[test]
fn one_reader_budget_applies_to_direct_source_snapshots_and_frozen_values() {
    let bytes = unchecked_bytes(&live_and_frozen_state());
    let mut reader = WorldBinaryReader::new(&bytes[4..], 1);
    assert!(decode(&mut reader, 4).is_err());
}

#[test]
fn malformed_transition_metadata_and_duplicate_sidecars_are_rejected() {
    let mut nonfinite = live_and_frozen_state();
    nonfinite.controllers[0]
        .transition
        .as_mut()
        .unwrap()
        .elapsed = f64::NAN;
    let bytes = unchecked_bytes(&nonfinite);
    assert!(AnimationPersistentState::decode(&bytes, 1 << 20).is_err());

    let mut nonfinite = live_and_frozen_state();
    let AnimationPersistentTransitionSource::Frozen {
        reference_time,
        ..
    } = &mut nonfinite.transitions[1].source
    else {
        unreachable!();
    };
    *reference_time = f64::NAN;
    let bytes = unchecked_bytes(&nonfinite);
    assert!(AnimationPersistentState::decode(&bytes, 1 << 20).is_err());

    let mut duplicate = live_and_frozen_state();
    duplicate.transitions.push(duplicate.transitions[0].clone());
    let bytes = unchecked_bytes(&duplicate);
    assert!(AnimationPersistentState::decode(&bytes, 1 << 20).is_err());
}

#[test]
fn frozen_values_require_unique_keys_matching_finite_types_and_shapes() {
    let mut duplicate = live_and_frozen_state();
    let repeated = frozen_values_mut(&mut duplicate)[0].clone();
    frozen_values_mut(&mut duplicate).push(repeated);
    assert!(duplicate.encode(1 << 20).is_err());

    frozen_values_mut(&mut duplicate).pop();
    frozen_values_mut(&mut duplicate)[0].baseline = AnimationValue::Rotation([0.0, 0.0, 0.0, 1.0]);
    assert!(duplicate.encode(1 << 20).is_err());

    frozen_values_mut(&mut duplicate)[0].baseline =
        AnimationValue::Field(FieldValue::F32(f32::INFINITY));
    assert!(duplicate.encode(1 << 20).is_err());

    let mut malformed = live_and_frozen_state();
    let value = &mut frozen_values_mut(&mut malformed)[0];
    value.property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: crate::ComponentValue::SCALAR,
        offsets: vec![u32::MAX],
    });
    assert!(malformed.encode(1 << 20).is_err());
}

fn frozen_values_mut(
    state: &mut AnimationPersistentState,
) -> &mut Vec<super::AnimationFrozenTransitionValue> {
    let AnimationPersistentTransitionSource::Frozen {
        values,
        ..
    } = &mut state.transitions[1].source
    else {
        unreachable!();
    };
    values
}
