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
                entity_bindings: Vec::new(),
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
                        },
                        AnimationFrozenTransitionValue {
                            target: EntityId::from_bits(9),
                            property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                                component: crate::ComponentValue::TRANSFORM,
                                offsets: vec![12, 16, 20, 24],
                            }),
                            value: AnimationValue::Rotation([0.0, 0.0, 1.0, 0.0]),
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
        contributions: vec![
            AnimationPersistentContribution {
                controller: AnimationControllerId::from_bits(1),
                target: EntityId::from_bits(9),
                property: property(0),
                value: AnimationValue::Field(FieldValue::F32(-2.5)),
            },
            AnimationPersistentContribution {
                controller: AnimationControllerId::from_bits(2),
                target: EntityId::from_bits(9),
                property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                    component: crate::ComponentValue::TRANSFORM,
                    offsets: vec![12, 16, 20, 24],
                }),
                value: AnimationValue::Rotation([0.0, 1.0, 0.0, 0.0]),
            },
        ],
    }
}

fn unchecked_bytes(state: &AnimationPersistentState) -> Vec<u8> {
    let mut writer = WorldBinaryWriter::new(1 << 20);
    writer.u32(8).unwrap();
    encode(&mut writer, state).unwrap();
    writer.bytes
}

#[test]
fn version_eight_round_trips_transitions_directional_starts_and_contributions() {
    let state = live_and_frozen_state();
    let bytes = state.encode(1 << 20).unwrap();
    assert_eq!(
        AnimationPersistentState::decode(&bytes, 1 << 20).unwrap(),
        state
    );
}

#[test]
fn controller_state_rejects_legacy_versions_and_keeps_large_binding_tables() {
    let mut state = AnimationPersistentState {
        next_id: 2,
        controllers: vec![controller(1, false)],
        transitions: Vec::new(),
        directional_starts: Vec::new(),
        contributions: Vec::new(),
    };
    state.controllers[0].description.drivers[0].property = AnimationTrackTarget::EntityLink;
    state.controllers[0].description.drivers[0].weight = 1.0;
    state.controllers[0].description.drivers[0].reference_time = 0.0;
    state.controllers[0].description.drivers[0].entity_bindings =
        vec![EntityId::from_bits(9); 4097];
    let mut bytes = state.encode(1 << 20).unwrap();
    assert_eq!(
        AnimationPersistentState::decode(&bytes, 1 << 20).unwrap(),
        state
    );
    for legacy in [6_u32, 7] {
        bytes[..4].copy_from_slice(&legacy.to_le_bytes());
        assert!(AnimationPersistentState::decode(&bytes, 1 << 20).is_err());
    }
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
    assert!(decode(&mut reader).is_err());
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
    frozen_values_mut(&mut duplicate)[0].value = AnimationValue::Rotation([0.0, 0.0, 0.0, 1.0]);
    assert!(duplicate.encode(1 << 20).is_err());

    frozen_values_mut(&mut duplicate)[0].value =
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

#[test]
fn contributions_require_a_controller_a_unique_field_and_a_finite_float_or_rotation() {
    let mut orphan = live_and_frozen_state();
    orphan.contributions[0].controller = AnimationControllerId::from_bits(7);
    assert!(orphan.encode(1 << 20).is_err());

    let mut duplicate = live_and_frozen_state();
    duplicate
        .contributions
        .push(duplicate.contributions[0].clone());
    assert!(duplicate.encode(1 << 20).is_err());

    // Another controller may contribute to the same field.
    let mut shared = live_and_frozen_state();
    let mut other = shared.contributions[0].clone();
    other.controller = AnimationControllerId::from_bits(2);
    shared.contributions.push(other);
    assert_eq!(
        AnimationPersistentState::decode(&shared.encode(1 << 20).unwrap(), 1 << 20).unwrap(),
        shared
    );

    for value in [
        AnimationValue::Field(FieldValue::F32(f32::NAN)),
        AnimationValue::Field(FieldValue::U32(3)),
        AnimationValue::Field(FieldValue::Bool(true)),
    ] {
        let mut invalid = live_and_frozen_state();
        invalid.contributions[0].value = value;
        assert!(invalid.encode(1 << 20).is_err());
    }

    let mut structural = live_and_frozen_state();
    structural.contributions[0].property = AnimationTrackTarget::EntityLink;
    assert!(structural.encode(1 << 20).is_err());
}
