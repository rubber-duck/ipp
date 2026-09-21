//! Prepared, allocation-free crossfade evaluation over sparse numeric and joint targets.

#![cfg_attr(
    not(feature = "skeletal-animation"),
    allow(
        unreachable_patterns,
        irrefutable_let_patterns,
        clippy::unnecessary_filter_map
    )
)]

use super::{driver::AnimationDriverBinding, *};
#[cfg(feature = "skeletal-animation")]
use crate::ComponentValue;
#[cfg(feature = "skeletal-animation")]
use crate::components::Transform;
use crate::components::registry::ComponentStorage;
#[cfg(feature = "skeletal-animation")]
use crate::components::schema::ComponentLifecycle;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TransitionLaneKey {
    Property(super::driver::AnimationTargetIdentity),
    #[cfg(feature = "skeletal-animation")]
    Joint {
        entity: EntityId,
        incarnation: u64,
        joint: u32,
    },
}

#[derive(Debug)]
enum TransitionOutput {
    Value(super::driver::AnimationTransitionOutput),
    #[cfg(feature = "skeletal-animation")]
    Joint(EntityId, u32),
}

#[derive(Clone, Debug)]
enum TransitionLaneValue {
    Property(AnimationValue),
    #[cfg(feature = "skeletal-animation")]
    Joint(Transform),
}

#[derive(Debug)]
struct TransitionLane {
    key: TransitionLaneKey,
    baseline: TransitionLaneValue,
    source: TransitionLaneValue,
    destination: TransitionLaneValue,
    output: TransitionOutput,
}

#[derive(Debug)]
enum TransitionOperation {
    Property {
        driver: usize,
        lane: usize,
    },
    #[cfg(feature = "skeletal-animation")]
    Pose {
        driver: usize,
        lanes: Vec<usize>,
        current: Vec<Transform>,
        output: Vec<Transform>,
    },
}

/// Mutation-boundary program; frame evaluation only copies retained values and samples tracks.
#[derive(Debug)]
pub(super) struct AnimationTransitionProgram {
    lanes: Vec<TransitionLane>,
    source_operations: Vec<TransitionOperation>,
    destination_operations: Vec<TransitionOperation>,
    numeric_targets: Vec<(EntityId, u16)>,
    frozen_source: bool,
}

impl AnimationTransitionProgram {
    pub(super) fn bind_source(
        source: &mut AnimationController,
        storage: &ComponentStorage,
    ) -> Result<Self, ErrorReason> {
        validate_drivers(&source.drivers)?;
        for driver in &mut source.drivers {
            driver.bind_transition_output(storage);
        }
        let mut keys = BTreeMap::new();
        collect_lanes(&source.drivers, &mut keys)?;
        let key_indices: BTreeMap<_, _> = keys
            .keys()
            .cloned()
            .enumerate()
            .map(|(index, key)| (key, index))
            .collect();
        let lanes = keys
            .into_iter()
            .map(|(key, (baseline, output))| TransitionLane {
                key,
                source: baseline.clone(),
                destination: baseline.clone(),
                baseline,
                output,
            })
            .collect();
        let source_operations = operations(&source.drivers, &key_indices);
        let mut numeric_targets: Vec<_> = key_indices
            .keys()
            .filter_map(|key| match key {
                TransitionLaneKey::Property(identity) => {
                    Some((identity.entity, identity.property.component()))
                }
                #[cfg(feature = "skeletal-animation")]
                TransitionLaneKey::Joint {
                    ..
                } => None,
            })
            .collect();
        numeric_targets.sort_unstable();
        numeric_targets.dedup();
        Ok(Self {
            lanes,
            source_operations,
            destination_operations: Vec::new(),
            numeric_targets,
            frozen_source: false,
        })
    }

    pub(super) fn bind_frozen_source(
        values: &[super::system_state::AnimationRuntimeFrozenTransitionValue],
        storage: &ComponentStorage,
    ) -> Result<Self, ErrorReason> {
        let mut lanes = Vec::with_capacity(values.len());
        for value in values {
            let key = frozen_key(value)?;
            let output = match &key {
                TransitionLaneKey::Property(identity) => TransitionOutput::Value(
                    super::driver::bind_frozen_transition_output(
                        identity.clone(),
                        &value.value,
                        storage,
                    )
                    .ok_or(ErrorReason::InvalidField)?,
                ),
                #[cfg(feature = "skeletal-animation")]
                TransitionLaneKey::Joint {
                    entity,
                    joint,
                    ..
                } => TransitionOutput::Joint(*entity, *joint),
            };
            lanes.push(TransitionLane {
                key,
                source: lane_value(&value.value)?,
                destination: lane_value(&value.value)?,
                baseline: lane_value(&value.baseline)?,
                output,
            });
        }
        lanes.sort_by(|a, b| a.key.cmp(&b.key));
        let mut numeric_targets = frozen_numeric_targets(values);
        numeric_targets.sort_unstable();
        Ok(Self {
            lanes,
            source_operations: Vec::new(),
            destination_operations: Vec::new(),
            numeric_targets,
            frozen_source: true,
        })
    }

    pub(super) fn bind(
        mut source: Option<&mut AnimationController>,
        destination: &mut AnimationController,
        storage: &ComponentStorage,
    ) -> Result<Self, ErrorReason> {
        if let Some(source) = source.as_ref() {
            validate_drivers(&source.drivers)?;
        }
        validate_drivers(&destination.drivers)?;
        if let Some(source) = source.as_deref_mut() {
            for driver in &mut source.drivers {
                driver.bind_transition_output(storage);
            }
        }
        for driver in &mut destination.drivers {
            driver.bind_transition_output(storage);
        }

        let mut keys =
            BTreeMap::<TransitionLaneKey, (TransitionLaneValue, TransitionOutput)>::new();
        if let Some(source) = source.as_deref() {
            collect_lanes(&source.drivers, &mut keys)?;
        }
        collect_lanes(&destination.drivers, &mut keys)?;
        let key_indices: BTreeMap<_, _> = keys
            .keys()
            .cloned()
            .enumerate()
            .map(|(index, key)| (key, index))
            .collect();
        let lanes = keys
            .into_iter()
            .map(|(key, (baseline, output))| TransitionLane {
                key,
                source: baseline.clone(),
                destination: baseline.clone(),
                baseline,
                output,
            })
            .collect();
        let source_operations = source
            .as_deref()
            .map_or_else(Vec::new, |source| operations(&source.drivers, &key_indices));
        let destination_operations = operations(&destination.drivers, &key_indices);
        let mut numeric_targets: Vec<_> = key_indices
            .keys()
            .filter_map(|key| match key {
                TransitionLaneKey::Property(identity) => {
                    Some((identity.entity, identity.property.component()))
                }
                #[cfg(feature = "skeletal-animation")]
                TransitionLaneKey::Joint {
                    ..
                } => None,
            })
            .collect();
        numeric_targets.sort_unstable();
        numeric_targets.dedup();
        Ok(Self {
            lanes,
            source_operations,
            destination_operations,
            numeric_targets,
            frozen_source: false,
        })
    }

    pub(super) fn bind_frozen(
        _bindings: &mut AnimationController,
        destination: &mut AnimationController,
        frozen: &[super::system_state::AnimationRuntimeFrozenTransitionValue],
        storage: &ComponentStorage,
    ) -> Result<Self, ErrorReason> {
        let mut program = Self::bind(None, destination, storage)?;
        program.source_operations.clear();
        program.frozen_source = true;
        for value in frozen {
            let key = match &value.property {
                #[cfg(feature = "skeletal-animation")]
                AnimationTrackTarget::Joints(joints) if joints.len() == 1 => {
                    TransitionLaneKey::Joint {
                        entity: value.target,
                        incarnation: value.incarnation,
                        joint: joints[0],
                    }
                }
                _ => TransitionLaneKey::Property(super::driver::AnimationTargetIdentity {
                    entity: value.target,
                    incarnation: value.incarnation,
                    property: value.property.clone(),
                }),
            };
            if !program.lanes.iter().any(|lane| lane.key == key) {
                let output = match &key {
                    TransitionLaneKey::Property(identity) => TransitionOutput::Value(
                        super::driver::bind_frozen_transition_output(
                            identity.clone(),
                            &value.baseline,
                            storage,
                        )
                        .ok_or(ErrorReason::InvalidField)?,
                    ),
                    #[cfg(feature = "skeletal-animation")]
                    TransitionLaneKey::Joint {
                        entity,
                        joint,
                        ..
                    } => TransitionOutput::Joint(*entity, *joint),
                };
                program.lanes.push(TransitionLane {
                    key: key.clone(),
                    source: lane_value(&value.value)?,
                    destination: lane_value(&value.baseline)?,
                    baseline: lane_value(&value.baseline)?,
                    output,
                });
            }
            let lane = program
                .lanes
                .iter_mut()
                .find(|lane| lane.key == key)
                .expect("frozen transition lane inserted");
            lane.baseline = lane_value(&value.baseline)?;
            lane.source = lane_value(&value.value)?;
        }
        program.lanes.sort_by(|a, b| a.key.cmp(&b.key));
        let key_indices: BTreeMap<_, _> = program
            .lanes
            .iter()
            .enumerate()
            .map(|(index, lane)| (lane.key.clone(), index))
            .collect();
        program.destination_operations = operations(&destination.drivers, &key_indices);
        program.numeric_targets = program
            .lanes
            .iter()
            .filter_map(|lane| match &lane.key {
                TransitionLaneKey::Property(identity) => {
                    Some((identity.entity, identity.property.component()))
                }
                #[cfg(feature = "skeletal-animation")]
                TransitionLaneKey::Joint {
                    ..
                } => None,
            })
            .collect();
        program.numeric_targets.sort_unstable();
        program.numeric_targets.dedup();
        Ok(program)
    }

    pub(super) fn retarget_frozen(
        mut self,
        destination: &mut AnimationController,
        frozen: &[super::system_state::AnimationRuntimeFrozenTransitionValue],
        storage: &ComponentStorage,
    ) -> Result<Self, ErrorReason> {
        for driver in &mut destination.drivers {
            driver.bind_transition_output(storage);
        }
        validate_drivers(&destination.drivers)?;
        let mut additions = BTreeMap::new();
        collect_lanes(&destination.drivers, &mut additions)?;
        for (key, (baseline, output)) in additions {
            if let Some(lane) = self.lanes.iter_mut().find(|lane| lane.key == key) {
                lane.destination = baseline;
                lane.output = output;
            } else {
                self.lanes.push(TransitionLane {
                    key,
                    source: baseline.clone(),
                    destination: baseline.clone(),
                    baseline,
                    output,
                });
            }
        }
        self.lanes.sort_by(|a, b| a.key.cmp(&b.key));
        let key_indices: BTreeMap<_, _> = self
            .lanes
            .iter()
            .enumerate()
            .map(|(index, lane)| (lane.key.clone(), index))
            .collect();
        self.source_operations.clear();
        self.destination_operations = operations(&destination.drivers, &key_indices);
        self.frozen_source = true;
        for value in frozen {
            let key = frozen_key(value)?;
            let lane = self
                .lanes
                .iter_mut()
                .find(|lane| lane.key == key)
                .ok_or(ErrorReason::InvalidField)?;
            lane.baseline = lane_value(&value.baseline)?;
            lane.source = lane_value(&value.value)?;
        }
        self.numeric_targets = self
            .lanes
            .iter()
            .filter_map(|lane| match &lane.key {
                TransitionLaneKey::Property(identity) => {
                    Some((identity.entity, identity.property.component()))
                }
                #[cfg(feature = "skeletal-animation")]
                TransitionLaneKey::Joint {
                    ..
                } => None,
            })
            .collect();
        self.numeric_targets.sort_unstable();
        self.numeric_targets.dedup();
        Ok(self)
    }

    pub(super) fn freeze(
        &self,
    ) -> Result<Vec<super::system_state::AnimationRuntimeFrozenTransitionValue>, ErrorReason> {
        self.lanes
            .iter()
            .map(|lane| {
                let (target, incarnation, property) = match &lane.key {
                    TransitionLaneKey::Property(identity) => (
                        identity.entity,
                        identity.incarnation,
                        identity.property.clone(),
                    ),
                    #[cfg(feature = "skeletal-animation")]
                    TransitionLaneKey::Joint {
                        entity,
                        incarnation,
                        joint,
                    } => (
                        *entity,
                        *incarnation,
                        AnimationTrackTarget::Joints(vec![*joint]),
                    ),
                };
                Ok(super::system_state::AnimationRuntimeFrozenTransitionValue {
                    target,
                    incarnation,
                    property,
                    value: lane_animation_value(&lane.destination),
                    baseline: lane_animation_value(&lane.baseline),
                })
            })
            .collect()
    }

    pub(super) fn persistent_frozen_values(
        &self,
    ) -> Vec<super::system_state::AnimationRuntimeFrozenTransitionValue> {
        self.lanes
            .iter()
            .map(|lane| {
                let (target, incarnation, property) = match &lane.key {
                    TransitionLaneKey::Property(identity) => (
                        identity.entity,
                        identity.incarnation,
                        identity.property.clone(),
                    ),
                    #[cfg(feature = "skeletal-animation")]
                    TransitionLaneKey::Joint {
                        entity,
                        incarnation,
                        joint,
                    } => (
                        *entity,
                        *incarnation,
                        AnimationTrackTarget::Joints(vec![*joint]),
                    ),
                };
                super::system_state::AnimationRuntimeFrozenTransitionValue {
                    target,
                    incarnation,
                    property,
                    value: lane_animation_value(&lane.source),
                    baseline: lane_animation_value(&lane.baseline),
                }
            })
            .collect()
    }

    pub(super) fn evaluate(
        &mut self,
        source_controller: Option<&AnimationController>,
        destination: &AnimationController,
        progress: f64,
        storage: &mut ComponentStorage,
    ) -> Result<(), ErrorReason> {
        for lane in &mut self.lanes {
            if !self.frozen_source {
                lane.source = lane.baseline.clone();
            }
            lane.destination = lane.baseline.clone();
        }
        if let Some(source) = source_controller {
            evaluate_operations(
                &mut self.source_operations,
                &mut self.lanes,
                &source.drivers,
                source.snapshot.time,
                true,
            )?;
        }
        evaluate_operations(
            &mut self.destination_operations,
            &mut self.lanes,
            &destination.drivers,
            destination.snapshot.time,
            false,
        )?;
        for lane in &mut self.lanes {
            lane.destination = match (&lane.source, &lane.destination) {
                (
                    TransitionLaneValue::Property(source_value),
                    TransitionLaneValue::Property(destination_value),
                ) => TransitionLaneValue::Property(source_mix(
                    source_value,
                    destination_value,
                    progress,
                )),
                #[cfg(feature = "skeletal-animation")]
                (
                    TransitionLaneValue::Joint(source_value),
                    TransitionLaneValue::Joint(destination_value),
                ) => TransitionLaneValue::Joint(super::pose::mix_joint(
                    source_value,
                    destination_value,
                    progress,
                )),
                _ => return Err(ErrorReason::InvalidField),
            };
            match (&lane.destination, &lane.output) {
                (TransitionLaneValue::Property(value), TransitionOutput::Value(output)) => {
                    output.validate(value)?;
                }
                #[cfg(feature = "skeletal-animation")]
                (TransitionLaneValue::Joint(value), TransitionOutput::Joint(_, _)) => {
                    value.validate()?;
                }
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        for lane in &self.lanes {
            match (&lane.destination, &lane.output) {
                (TransitionLaneValue::Property(value), TransitionOutput::Value(output)) => {
                    output.write(storage, value.clone())?
                }
                #[cfg(feature = "skeletal-animation")]
                (TransitionLaneValue::Joint(value), TransitionOutput::Joint(entity, joint)) => {
                    let pose = storage
                        .skeleton_mut(entity.index() as usize)
                        .and_then(|skeleton| skeleton.runtime.pose.as_mut())
                        .filter(|pose| pose.valid)
                        .ok_or(ErrorReason::MissingComponent)?;
                    *pose
                        .local
                        .get_mut(*joint as usize)
                        .ok_or(ErrorReason::InvalidField)? = *value;
                }
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        Ok(())
    }

    pub(super) fn prepare_source_hold(
        &mut self,
        source: &AnimationController,
    ) -> Result<(), ErrorReason> {
        for lane in &mut self.lanes {
            lane.source = lane.baseline.clone();
        }
        evaluate_operations(
            &mut self.source_operations,
            &mut self.lanes,
            &source.drivers,
            source.snapshot.time,
            true,
        )?;
        Ok(())
    }

    pub(super) fn write_source_hold(
        &self,
        storage: &mut ComponentStorage,
    ) -> Result<(), ErrorReason> {
        for lane in &self.lanes {
            match (&lane.source, &lane.output) {
                (TransitionLaneValue::Property(value), TransitionOutput::Value(output)) => {
                    output.validate(value)?;
                }
                #[cfg(feature = "skeletal-animation")]
                (TransitionLaneValue::Joint(value), TransitionOutput::Joint(_, _)) => {
                    value.validate()?;
                }
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        for lane in &self.lanes {
            match (&lane.source, &lane.output) {
                (TransitionLaneValue::Property(value), TransitionOutput::Value(output)) => {
                    output.write(storage, value.clone())?;
                }
                #[cfg(feature = "skeletal-animation")]
                (TransitionLaneValue::Joint(value), TransitionOutput::Joint(entity, joint)) => {
                    let pose = storage
                        .skeleton_mut(entity.index() as usize)
                        .and_then(|skeleton| skeleton.runtime.pose.as_mut())
                        .filter(|pose| pose.valid)
                        .ok_or(ErrorReason::MissingComponent)?;
                    *pose
                        .local
                        .get_mut(*joint as usize)
                        .ok_or(ErrorReason::InvalidField)? = *value;
                }
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        Ok(())
    }

    pub(super) fn write_composite_hold(
        &self,
        storage: &mut ComponentStorage,
    ) -> Result<(), ErrorReason> {
        for lane in &self.lanes {
            match (&lane.destination, &lane.output) {
                (TransitionLaneValue::Property(value), TransitionOutput::Value(output)) => {
                    output.validate(value)?;
                }
                #[cfg(feature = "skeletal-animation")]
                (TransitionLaneValue::Joint(value), TransitionOutput::Joint(_, _)) => {
                    value.validate()?;
                }
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        for lane in &self.lanes {
            match (&lane.destination, &lane.output) {
                (TransitionLaneValue::Property(value), TransitionOutput::Value(output)) => {
                    output.write(storage, value.clone())?;
                }
                #[cfg(feature = "skeletal-animation")]
                (TransitionLaneValue::Joint(value), TransitionOutput::Joint(entity, joint)) => {
                    let pose = storage
                        .skeleton_mut(entity.index() as usize)
                        .and_then(|skeleton| skeleton.runtime.pose.as_mut())
                        .filter(|pose| pose.valid)
                        .ok_or(ErrorReason::MissingComponent)?;
                    *pose
                        .local
                        .get_mut(*joint as usize)
                        .ok_or(ErrorReason::InvalidField)? = *value;
                }
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        Ok(())
    }

    pub(super) fn numeric_targets(&self) -> &[(EntityId, u16)] {
        &self.numeric_targets
    }

    pub(super) fn target_keys(&self) -> impl Iterator<Item = (EntityId, u16)> + '_ {
        self.lanes.iter().map(|lane| match &lane.key {
            TransitionLaneKey::Property(identity) => {
                (identity.entity, identity.property.component())
            }
            #[cfg(feature = "skeletal-animation")]
            TransitionLaneKey::Joint {
                entity,
                ..
            } => (*entity, ComponentValue::SKELETON),
        })
    }

    pub(super) fn invalidated_by(
        &self,
        staged: &crate::world::WorldMutationState,
        storage: &ComponentStorage,
    ) -> bool {
        self.lanes.iter().any(|lane| {
            let alive = match &lane.key {
                TransitionLaneKey::Property(identity) => {
                    transition_property_alive(identity, staged, storage)
                }
                #[cfg(feature = "skeletal-animation")]
                TransitionLaneKey::Joint {
                    entity,
                    incarnation,
                    ..
                } => staged
                    .entities
                    .get(entity)
                    .and_then(|record| record.input(ComponentValue::SKELETON))
                    .is_some_and(|input| input.incarnation == *incarnation),
            };
            let key = match &lane.key {
                TransitionLaneKey::Property(identity) => {
                    (identity.entity, identity.property.component())
                }
                #[cfg(feature = "skeletal-animation")]
                TransitionLaneKey::Joint {
                    entity,
                    ..
                } => (*entity, ComponentValue::SKELETON),
            };
            staged.changed.contains_key(&key) && !alive
        })
    }

    pub(super) fn restore(
        &self,
        storage: &mut ComponentStorage,
        state: &crate::world::WorldEntityState,
    ) -> Result<(), ErrorReason> {
        for lane in &self.lanes {
            let alive = match &lane.key {
                TransitionLaneKey::Property(identity) => {
                    transition_property_alive(identity, state, storage)
                }
                #[cfg(feature = "skeletal-animation")]
                TransitionLaneKey::Joint {
                    entity,
                    incarnation,
                    ..
                } => state
                    .entities
                    .get(entity)
                    .and_then(|record| record.input(crate::ComponentValue::SKELETON))
                    .is_some_and(|input| input.incarnation == *incarnation),
            };
            if !alive {
                continue;
            }
            match (&lane.baseline, &lane.output) {
                (TransitionLaneValue::Property(value), TransitionOutput::Value(output)) => {
                    output.write(storage, value.clone())?;
                }
                #[cfg(feature = "skeletal-animation")]
                (TransitionLaneValue::Joint(value), TransitionOutput::Joint(entity, joint)) => {
                    let pose = storage
                        .skeleton_mut(entity.index() as usize)
                        .and_then(|skeleton| skeleton.runtime.pose.as_mut())
                        .filter(|pose| pose.valid)
                        .ok_or(ErrorReason::MissingComponent)?;
                    *pose
                        .local
                        .get_mut(*joint as usize)
                        .ok_or(ErrorReason::InvalidField)? = *value;
                }
                _ => return Err(ErrorReason::InvalidField),
            }
        }
        Ok(())
    }

    pub(super) fn refresh_baselines(
        &mut self,
        source: Option<&AnimationController>,
        destination: &AnimationController,
    ) {
        let mut originals = BTreeMap::new();
        if let Some(source) = source {
            collect_originals(&source.drivers, &mut originals);
        }
        collect_originals(&destination.drivers, &mut originals);
        for lane in &mut self.lanes {
            if let Some(value) = originals.get(&lane.key) {
                lane.baseline = value.clone();
            }
        }
    }

    pub(super) fn refresh_frozen_baselines(
        &mut self,
        values: &[super::system_state::AnimationRuntimeFrozenTransitionValue],
    ) {
        let baselines: BTreeMap<_, _> = values
            .iter()
            .filter_map(|value| {
                frozen_key(value)
                    .and_then(|key| Ok((key, lane_value(&value.baseline)?)))
                    .ok()
            })
            .collect();
        for lane in &mut self.lanes {
            if let Some(value) = baselines.get(&lane.key) {
                lane.baseline = value.clone();
            }
        }
    }
}

fn transition_property_alive(
    identity: &super::driver::AnimationTargetIdentity,
    state: &crate::world::WorldEntityState,
    storage: &ComponentStorage,
) -> bool {
    state
        .entities
        .get(&identity.entity)
        .and_then(|record| record.input(identity.property.component()))
        .is_some_and(|input| input.incarnation == identity.incarnation)
        && identity.property.indices().iter().all(|offset| {
            !crate::components::dynamic_properties::is_dynamic_field(*offset)
                || state
                    .input_field(
                        storage,
                        identity.entity,
                        identity.property.component(),
                        *offset,
                    )
                    .is_some()
        })
}

pub(super) fn restore_frozen_values(
    values: &[super::system_state::AnimationRuntimeFrozenTransitionValue],
    storage: &mut ComponentStorage,
    state: &crate::world::WorldEntityState,
) -> Result<(), ErrorReason> {
    for value in values {
        if !state
            .entities
            .get(&value.target)
            .and_then(|record| record.input(value.property.component()))
            .is_some_and(|input| input.incarnation == value.incarnation)
        {
            continue;
        }
        #[cfg(feature = "skeletal-animation")]
        if let AnimationTrackTarget::Joints(joints) = &value.property {
            let AnimationValue::Pose(baseline) = &value.baseline else {
                return Err(ErrorReason::InvalidField);
            };
            let (&joint, baseline) = joints
                .first()
                .zip(baseline.first())
                .ok_or(ErrorReason::InvalidField)?;
            let pose = storage
                .skeleton_mut(value.target.index() as usize)
                .and_then(|skeleton| skeleton.runtime.pose.as_mut())
                .filter(|pose| pose.valid)
                .ok_or(ErrorReason::MissingComponent)?;
            *pose
                .local
                .get_mut(joint as usize)
                .ok_or(ErrorReason::InvalidField)? = *baseline;
            continue;
        }
        let identity = super::driver::AnimationTargetIdentity {
            entity: value.target,
            incarnation: value.incarnation,
            property: value.property.clone(),
        };
        let output =
            super::driver::bind_frozen_transition_output(identity, &value.baseline, storage)
                .ok_or(ErrorReason::InvalidField)?;
        output.validate(&value.baseline)?;
        output.write(storage, value.baseline.clone())?;
    }
    Ok(())
}

pub(super) fn frozen_numeric_targets(
    values: &[super::system_state::AnimationRuntimeFrozenTransitionValue],
) -> Vec<(EntityId, u16)> {
    let mut targets: Vec<_> = values
        .iter()
        .filter_map(|value| {
            #[cfg(feature = "skeletal-animation")]
            if matches!(value.property, AnimationTrackTarget::Joints(_)) {
                return None;
            }
            Some((value.target, value.property.component()))
        })
        .collect();
    targets.sort_unstable();
    targets.dedup();
    targets
}

fn collect_originals(
    drivers: &[Box<dyn AnimationDriverBinding>],
    originals: &mut BTreeMap<TransitionLaneKey, TransitionLaneValue>,
) {
    for driver in drivers {
        #[cfg(feature = "skeletal-animation")]
        if let AnimationTrackTarget::Joints(joints) = &driver.identity().property
            && let AnimationValue::Pose(values) = driver.original()
        {
            for (&joint, value) in joints.iter().zip(values) {
                originals
                    .entry(TransitionLaneKey::Joint {
                        entity: driver.identity().entity,
                        incarnation: driver.identity().incarnation,
                        joint,
                    })
                    .or_insert(TransitionLaneValue::Joint(value));
            }
            continue;
        }
        originals
            .entry(TransitionLaneKey::Property(driver.identity().clone()))
            .or_insert_with(|| TransitionLaneValue::Property(driver.original()));
    }
}

fn lane_value(value: &AnimationValue) -> Result<TransitionLaneValue, ErrorReason> {
    #[cfg(feature = "skeletal-animation")]
    if let AnimationValue::Pose(values) = value {
        let [value] = values.as_slice() else {
            return Err(ErrorReason::InvalidField);
        };
        return Ok(TransitionLaneValue::Joint(*value));
    }
    Ok(TransitionLaneValue::Property(value.clone()))
}

fn frozen_key(
    value: &super::system_state::AnimationRuntimeFrozenTransitionValue,
) -> Result<TransitionLaneKey, ErrorReason> {
    #[cfg(feature = "skeletal-animation")]
    if let AnimationTrackTarget::Joints(joints) = &value.property {
        let [joint] = joints.as_slice() else {
            return Err(ErrorReason::InvalidField);
        };
        return Ok(TransitionLaneKey::Joint {
            entity: value.target,
            incarnation: value.incarnation,
            joint: *joint,
        });
    }
    Ok(TransitionLaneKey::Property(
        super::driver::AnimationTargetIdentity {
            entity: value.target,
            incarnation: value.incarnation,
            property: value.property.clone(),
        },
    ))
}

fn lane_animation_value(value: &TransitionLaneValue) -> AnimationValue {
    match value {
        TransitionLaneValue::Property(value) => value.clone(),
        #[cfg(feature = "skeletal-animation")]
        TransitionLaneValue::Joint(value) => AnimationValue::Pose(vec![*value]),
    }
}

fn source_mix(
    source: &AnimationValue,
    destination: &AnimationValue,
    progress: f64,
) -> AnimationValue {
    mix(source, destination, progress)
}

fn validate_drivers(drivers: &[Box<dyn AnimationDriverBinding>]) -> Result<(), ErrorReason> {
    for driver in drivers {
        match driver.original() {
            AnimationValue::Field(crate::components::schema::FieldValue::F32(_))
            | AnimationValue::Rotation(_) => {}
            AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value))
                if !matches!(
                    value,
                    crate::DynamicValue::Bool(_) | crate::DynamicValue::Asset(_)
                ) => {}
            #[cfg(feature = "skeletal-animation")]
            AnimationValue::Pose(_) => {}
            _ => return Err(ErrorReason::InvalidField),
        }
    }
    Ok(())
}

fn collect_lanes(
    drivers: &[Box<dyn AnimationDriverBinding>],
    lanes: &mut BTreeMap<TransitionLaneKey, (TransitionLaneValue, TransitionOutput)>,
) -> Result<(), ErrorReason> {
    for driver in drivers {
        #[cfg(feature = "skeletal-animation")]
        if let AnimationTrackTarget::Joints(joints) = &driver.identity().property {
            let AnimationValue::Pose(values) = driver.original() else {
                return Err(ErrorReason::InvalidField);
            };
            for (&joint, value) in joints.iter().zip(values) {
                lanes
                    .entry(TransitionLaneKey::Joint {
                        entity: driver.identity().entity,
                        incarnation: driver.identity().incarnation,
                        joint,
                    })
                    .or_insert((
                        TransitionLaneValue::Joint(value),
                        TransitionOutput::Joint(driver.identity().entity, joint),
                    ));
            }
            continue;
        }
        let output = driver
            .transition_output()
            .ok_or(ErrorReason::InvalidField)?;
        lanes
            .entry(TransitionLaneKey::Property(driver.identity().clone()))
            .or_insert((
                TransitionLaneValue::Property(driver.original()),
                TransitionOutput::Value(output),
            ));
    }
    Ok(())
}

fn operations(
    drivers: &[Box<dyn AnimationDriverBinding>],
    lanes: &BTreeMap<TransitionLaneKey, usize>,
) -> Vec<TransitionOperation> {
    drivers
        .iter()
        .enumerate()
        .map(|(driver_index, driver)| {
            #[cfg(feature = "skeletal-animation")]
            if let AnimationTrackTarget::Joints(joints) = &driver.identity().property {
                let indices: Vec<_> = joints
                    .iter()
                    .map(|&joint| {
                        lanes[&TransitionLaneKey::Joint {
                            entity: driver.identity().entity,
                            incarnation: driver.identity().incarnation,
                            joint,
                        }]
                    })
                    .collect();
                return TransitionOperation::Pose {
                    driver: driver_index,
                    current: vec![Transform::default(); indices.len()],
                    output: vec![Transform::default(); indices.len()],
                    lanes: indices,
                };
            }
            TransitionOperation::Property {
                driver: driver_index,
                lane: lanes[&TransitionLaneKey::Property(driver.identity().clone())],
            }
        })
        .collect()
}

fn evaluate_operations(
    operations: &mut [TransitionOperation],
    lanes: &mut [TransitionLane],
    drivers: &[Box<dyn AnimationDriverBinding>],
    time: f64,
    source_side: bool,
) -> Result<(), ErrorReason> {
    for operation in operations {
        match operation {
            TransitionOperation::Property {
                driver,
                lane,
            } => {
                let current = if source_side {
                    &lanes[*lane].source
                } else {
                    &lanes[*lane].destination
                };
                let TransitionLaneValue::Property(current) = current else {
                    return Err(ErrorReason::InvalidField);
                };
                let value = TransitionLaneValue::Property(
                    drivers[*driver].sample_bound(time, current.clone())?,
                );
                if source_side {
                    lanes[*lane].source = value;
                } else {
                    lanes[*lane].destination = value;
                }
            }
            #[cfg(feature = "skeletal-animation")]
            TransitionOperation::Pose {
                driver,
                lanes: indices,
                current,
                output,
            } => {
                for (&lane, current) in indices.iter().zip(current.iter_mut()) {
                    let value = if source_side {
                        &lanes[lane].source
                    } else {
                        &lanes[lane].destination
                    };
                    let TransitionLaneValue::Joint(value) = value else {
                        return Err(ErrorReason::InvalidField);
                    };
                    *current = *value;
                }
                super::pose::sample_transition_joints(
                    drivers[*driver].as_ref(),
                    drivers[*driver].bound_pose_track(),
                    drivers[*driver].duration(),
                    time,
                    current,
                    output,
                )?;
                for (&lane, &value) in indices.iter().zip(output.iter()) {
                    if source_side {
                        lanes[lane].source = TransitionLaneValue::Joint(value);
                    } else {
                        lanes[lane].destination = TransitionLaneValue::Joint(value);
                    }
                }
            }
        }
    }
    Ok(())
}
