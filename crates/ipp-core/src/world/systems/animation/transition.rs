//! Prepared, allocation-free crossfade evaluation over sparse numeric and joint targets.
//!
//! A controller crossfade blends contributions: each side evaluates what its
//! drivers add, the blend moves every field by the change of the blended total,
//! and the destination controller remembers what it applied. A frozen side is a
//! captured contribution that fades out. GUI skin motion uses the same program
//! with absolute channels that fade from a captured value to the clip's value.

use super::{
    contribution::AnimationContributions, driver::AnimationDriverBinding,
    system_state::AnimationRuntimeFrozenTransitionValue, *,
};
use crate::ComponentValue;
use crate::components::Transform;
use crate::components::registry::ComponentStorage;
use crate::components::schema::ComponentLifecycle;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TransitionChannelKey {
    Property(super::driver::AnimationTargetIdentity),
    Joint {
        entity: EntityId,
        incarnation: u64,
        joint: u32,
    },
}

#[derive(Debug)]
enum TransitionOutput {
    Value(super::driver::AnimationTransitionOutput),
    Joint(EntityId, u32),
}

#[derive(Clone, Debug, PartialEq)]
enum TransitionChannelValue {
    Property(AnimationValue),
    Joint(Transform),
}

#[derive(Debug)]
struct TransitionChannel {
    key: TransitionChannelKey,
    /// Where each side starts: the empty contribution, or GUI motion's value.
    baseline: TransitionChannelValue,
    source: TransitionChannelValue,
    destination: TransitionChannelValue,
    output: TransitionOutput,
}

#[derive(Debug)]
enum TransitionOperation {
    Property {
        driver: usize,
        channel: usize,
    },
    Constant {
        channel: usize,
        value: AnimationValue,
    },
    Pose {
        driver: usize,
        channels: Vec<usize>,
        current: Vec<Transform>,
        output: Vec<Transform>,
    },
}

/// Mutation-boundary program; frame evaluation only copies retained values and samples tracks.
#[derive(Debug)]
pub(super) struct AnimationTransitionProgram {
    channels: Vec<TransitionChannel>,
    source_operations: Vec<TransitionOperation>,
    destination_operations: Vec<TransitionOperation>,
    numeric_targets: Vec<(EntityId, u16)>,
    frozen_source: bool,
    /// Channels carry contributions rather than absolute values.
    contributions: bool,
    /// Per-channel values staged by one evaluation before any is written, with
    /// the value a contributed field held before.
    staged: Vec<(TransitionChannelValue, Option<AnimationValue>)>,
}

impl AnimationTransitionProgram {
    pub(super) fn set_constant_destination(
        &mut self,
        entity: EntityId,
        incarnation: u64,
        property: AnimationTrackTarget,
        value: AnimationValue,
    ) -> Result<(), ErrorReason> {
        let key = TransitionChannelKey::Property(super::driver::AnimationTargetIdentity {
            entity,
            incarnation,
            property,
        });
        let index = self
            .channels
            .iter()
            .position(|channel| channel.key == key)
            .ok_or(ErrorReason::InvalidField)?;
        let TransitionOutput::Value(output) = &self.channels[index].output else {
            return Err(ErrorReason::InvalidField);
        };
        output.validate(&value)?;

        let operation = self.destination_operations.iter_mut().find(|operation| {
            matches!(operation, TransitionOperation::Property { channel, .. } if *channel == index)
        }).ok_or(ErrorReason::InvalidField)?;
        *operation = TransitionOperation::Constant {
            channel: index,
            value,
        };

        Ok(())
    }

    /// Crossfade between two controllers' contributions.
    pub(super) fn bind(
        source: Option<&mut AnimationController>,
        destination: &mut AnimationController,
        storage: &ComponentStorage,
    ) -> Result<Self, ErrorReason> {
        Self::bind_sides(source, destination, storage, true)
    }

    fn bind_sides(
        source: Option<&mut AnimationController>,
        destination: &mut AnimationController,
        storage: &ComponentStorage,
        contributions: bool,
    ) -> Result<Self, ErrorReason> {
        if let Some(source) = source.as_ref() {
            validate_drivers(&source.drivers)?;
        }
        validate_drivers(&destination.drivers)?;

        let mut keys =
            BTreeMap::<TransitionChannelKey, (TransitionChannelValue, TransitionOutput)>::new();
        if let Some(source) = source.as_deref() {
            collect_channels(&source.drivers, storage, contributions, &mut keys)?;
        }
        collect_channels(&destination.drivers, storage, contributions, &mut keys)?;
        let key_indices: BTreeMap<_, _> = keys
            .keys()
            .cloned()
            .enumerate()
            .map(|(index, key)| (key, index))
            .collect();
        let channels = keys
            .into_iter()
            .map(|(key, (baseline, output))| TransitionChannel {
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
        let mut program = Self {
            channels,
            source_operations,
            destination_operations,
            numeric_targets: Vec::new(),
            frozen_source: false,
            contributions,
            staged: Vec::new(),
        };
        program.refresh_numeric_targets();
        Ok(program)
    }

    /// Fade a captured contribution out while the destination fades in.
    pub(super) fn bind_frozen(
        _bindings: &mut AnimationController,
        destination: &mut AnimationController,
        frozen: &[AnimationRuntimeFrozenTransitionValue],
        storage: &ComponentStorage,
    ) -> Result<Self, ErrorReason> {
        Self::bind_frozen_values(destination, frozen, storage, true)
    }

    /// Fade from frozen values to the destination. With `contributions`, the
    /// values are contributions; otherwise they are absolute (GUI motion).
    pub(super) fn bind_frozen_values(
        destination: &mut AnimationController,
        frozen: &[AnimationRuntimeFrozenTransitionValue],
        storage: &ComponentStorage,
        contributions: bool,
    ) -> Result<Self, ErrorReason> {
        let mut program = Self::bind_sides(None, destination, storage, contributions)?;
        program.source_operations.clear();
        program.frozen_source = true;
        for value in frozen {
            let key = frozen_key(value)?;
            if !program.channels.iter().any(|channel| channel.key == key) {
                let output = match &key {
                    TransitionChannelKey::Property(identity) => TransitionOutput::Value(
                        super::driver::bind_frozen_transition_output(
                            identity.clone(),
                            &value.baseline,
                            storage,
                        )
                        .ok_or(ErrorReason::InvalidField)?,
                    ),
                    TransitionChannelKey::Joint {
                        entity,
                        joint,
                        ..
                    } => TransitionOutput::Joint(*entity, *joint),
                };
                program.channels.push(TransitionChannel {
                    key: key.clone(),
                    source: channel_value(&value.value)?,
                    destination: channel_value(&value.baseline)?,
                    baseline: channel_value(&value.baseline)?,
                    output,
                });
            }
            let channel = program
                .channels
                .iter_mut()
                .find(|channel| channel.key == key)
                .expect("frozen transition channel inserted");
            channel.baseline = channel_value(&value.baseline)?;
            channel.source = channel_value(&value.value)?;
        }
        program.channels.sort_by(|a, b| a.key.cmp(&b.key));
        let key_indices: BTreeMap<_, _> = program
            .channels
            .iter()
            .enumerate()
            .map(|(index, channel)| (channel.key.clone(), index))
            .collect();
        program.destination_operations = operations(&destination.drivers, &key_indices);
        program.refresh_numeric_targets();
        Ok(program)
    }

    pub(super) fn retarget_frozen(
        mut self,
        destination: &mut AnimationController,
        frozen: &[AnimationRuntimeFrozenTransitionValue],
        storage: &ComponentStorage,
    ) -> Result<Self, ErrorReason> {
        validate_drivers(&destination.drivers)?;
        let mut additions = BTreeMap::new();
        collect_channels(
            &destination.drivers,
            storage,
            self.contributions,
            &mut additions,
        )?;
        for (key, (baseline, output)) in additions {
            if let Some(channel) = self.channels.iter_mut().find(|channel| channel.key == key) {
                channel.destination = baseline;
                channel.output = output;
            } else {
                self.channels.push(TransitionChannel {
                    key,
                    source: baseline.clone(),
                    destination: baseline.clone(),
                    baseline,
                    output,
                });
            }
        }
        self.channels.sort_by(|a, b| a.key.cmp(&b.key));
        let key_indices: BTreeMap<_, _> = self
            .channels
            .iter()
            .enumerate()
            .map(|(index, channel)| (channel.key.clone(), index))
            .collect();
        self.source_operations.clear();
        self.destination_operations = operations(&destination.drivers, &key_indices);
        self.frozen_source = true;
        for value in frozen {
            let key = frozen_key(value)?;
            let channel = self
                .channels
                .iter_mut()
                .find(|channel| channel.key == key)
                .ok_or(ErrorReason::InvalidField)?;
            channel.baseline = channel_value(&value.baseline)?;
            channel.source = channel_value(&value.value)?;
        }
        self.refresh_numeric_targets();
        Ok(self)
    }

    fn refresh_numeric_targets(&mut self) {
        self.numeric_targets = self
            .channels
            .iter()
            .filter_map(|channel| match &channel.key {
                TransitionChannelKey::Property(identity) => {
                    Some((identity.entity, identity.property.component_target()))
                }
                TransitionChannelKey::Joint {
                    ..
                } => None,
            })
            .collect();
        self.numeric_targets.sort_unstable();
        self.numeric_targets.dedup();
    }

    /// Capture what the crossfade has in its fields, for an interrupting one to
    /// fade out. A controller crossfade's property channels hold what its
    /// destination applied; other channels hold their last blended value.
    pub(super) fn freeze(
        &self,
        applied: Option<&AnimationContributions>,
    ) -> Result<Vec<AnimationRuntimeFrozenTransitionValue>, ErrorReason> {
        self.channels
            .iter()
            .map(|channel| {
                let value = match (&channel.key, applied) {
                    (TransitionChannelKey::Property(identity), Some(applied))
                        if self.contributions =>
                    {
                        applied
                            .get(identity)
                            .map(super::contribution::AnimationApplied::value)
                            .unwrap_or_else(|| channel_animation_value(&channel.baseline))
                    }
                    _ => channel_animation_value(&channel.destination),
                };
                Ok(frozen_value(
                    channel,
                    value,
                    channel_animation_value(&channel.baseline),
                ))
            })
            .collect()
    }

    /// The captured outgoing values of a frozen crossfade.
    pub(super) fn persistent_frozen_values(&self) -> Vec<AnimationRuntimeFrozenTransitionValue> {
        self.channels
            .iter()
            .map(|channel| {
                frozen_value(
                    channel,
                    channel_animation_value(&channel.source),
                    channel_animation_value(&channel.baseline),
                )
            })
            .collect()
    }

    /// Blend both sides at `progress` and write the result. Every channel is
    /// validated before any is written. For a controller crossfade, fields move
    /// by the change of the blended contribution and `applied` records it.
    pub(super) fn evaluate(
        &mut self,
        source_controller: Option<&AnimationController>,
        destination_drivers: &[Box<dyn AnimationDriverBinding>],
        destination_time: f64,
        applied: &mut AnimationContributions,
        progress: f64,
        storage: &mut ComponentStorage,
    ) -> Result<(), ErrorReason> {
        for channel in &mut self.channels {
            if !self.frozen_source {
                channel.source = channel.baseline.clone();
            }
            channel.destination = channel.baseline.clone();
        }
        if let Some(source) = source_controller {
            evaluate_operations(
                &mut self.source_operations,
                &mut self.channels,
                &source.drivers,
                source.snapshot.time,
                true,
                self.contributions,
            )?;
        }
        evaluate_operations(
            &mut self.destination_operations,
            &mut self.channels,
            destination_drivers,
            destination_time,
            false,
            self.contributions,
        )?;
        for channel in &mut self.channels {
            channel.destination = match (&channel.source, &channel.destination) {
                (
                    TransitionChannelValue::Property(source_value),
                    TransitionChannelValue::Property(destination_value),
                ) => {
                    TransitionChannelValue::Property(mix(source_value, destination_value, progress))
                }
                (
                    TransitionChannelValue::Joint(source_value),
                    TransitionChannelValue::Joint(destination_value),
                ) => TransitionChannelValue::Joint(super::pose::mix_joint(
                    source_value,
                    destination_value,
                    progress,
                )),
                _ => return Err(ErrorReason::InvalidField),
            };
        }

        // Stage every written value, then publish only when all validate.
        self.staged.clear();
        for channel in &self.channels {
            let staged = match (&channel.destination, &channel.output) {
                (TransitionChannelValue::Property(value), TransitionOutput::Value(output)) => {
                    if self.contributions {
                        let TransitionChannelKey::Property(identity) = &channel.key else {
                            return Err(ErrorReason::InvalidField);
                        };
                        let current = output.read(storage)?;
                        let next = match applied.get(identity) {
                            Some(previous) => previous.moved(&current, value)?,
                            None => super::contribution::AnimationApplied::new(
                                channel_animation_value(&channel.baseline),
                            )
                            .moved(&current, value)?,
                        };
                        output.validate(&next)?;
                        (TransitionChannelValue::Property(next), Some(current))
                    } else {
                        output.validate(value)?;
                        (TransitionChannelValue::Property(value.clone()), None)
                    }
                }
                (TransitionChannelValue::Joint(value), TransitionOutput::Joint(entity, joint)) => {
                    let value = if self.contributions {
                        super::contribution::compose_joint(
                            joint_local(storage, *entity, *joint)?,
                            value,
                        )
                    } else {
                        *value
                    };
                    value.validate()?;
                    (TransitionChannelValue::Joint(value), None)
                }
                _ => return Err(ErrorReason::InvalidField),
            };
            self.staged.push(staged);
        }
        for (channel, (staged, before)) in self.channels.iter().zip(&self.staged) {
            match (staged, &channel.output) {
                (TransitionChannelValue::Property(value), TransitionOutput::Value(output)) => {
                    output.write(storage, value.clone())?;
                }
                (TransitionChannelValue::Joint(value), TransitionOutput::Joint(entity, joint)) => {
                    *joint_local_mut(storage, *entity, *joint)? = *value;
                }
                _ => return Err(ErrorReason::InvalidField),
            }
            if let (
                TransitionChannelKey::Property(identity),
                TransitionChannelValue::Property(total),
                TransitionChannelValue::Property(after),
                Some(before),
            ) = (&channel.key, &channel.destination, staged, before)
            {
                applied.landed(identity, before, after, total);
            }
        }
        Ok(())
    }

    pub(super) fn numeric_targets(&self) -> &[(EntityId, u16)] {
        &self.numeric_targets
    }

    pub(super) fn target_keys(&self) -> impl Iterator<Item = (EntityId, u16)> + '_ {
        self.channels.iter().map(|channel| match &channel.key {
            TransitionChannelKey::Property(identity) => {
                (identity.entity, identity.property.component_target())
            }
            TransitionChannelKey::Joint {
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
        self.channels.iter().any(|channel| {
            let alive = match &channel.key {
                TransitionChannelKey::Property(identity) => {
                    transition_property_alive(identity, staged, storage)
                }
                TransitionChannelKey::Joint {
                    entity,
                    incarnation,
                    ..
                } => staged
                    .entities
                    .get(entity)
                    .and_then(|record| record.input(ComponentValue::SKELETON))
                    .is_some_and(|input| input.incarnation == *incarnation),
            };
            let key = match &channel.key {
                TransitionChannelKey::Property(identity) => {
                    (identity.entity, identity.property.component_target())
                }
                TransitionChannelKey::Joint {
                    entity,
                    ..
                } => (*entity, ComponentValue::SKELETON),
            };
            staged.changed.contains_key(&key) && !alive
        })
    }
}

/// Whether a change departs the field of a captured value a pending crossfade
/// still has to fade out.
pub(super) fn frozen_values_invalidated(
    values: &[AnimationRuntimeFrozenTransitionValue],
    staged: &crate::world::WorldMutationState,
    storage: &ComponentStorage,
) -> bool {
    values.iter().any(|value| {
        let Ok(key) = frozen_key(value) else {
            return true;
        };
        let (component, alive) = match &key {
            TransitionChannelKey::Property(identity) => (
                identity.property.component_target(),
                transition_property_alive(identity, staged, storage),
            ),
            TransitionChannelKey::Joint {
                entity,
                incarnation,
                ..
            } => (
                ComponentValue::SKELETON,
                staged
                    .entities
                    .get(entity)
                    .and_then(|record| record.input(ComponentValue::SKELETON))
                    .is_some_and(|input| input.incarnation == *incarnation),
            ),
        };
        staged.changed.contains_key(&(value.target, component)) && !alive
    })
}

fn joint_local(
    storage: &ComponentStorage,
    entity: EntityId,
    joint: u32,
) -> Result<&Transform, ErrorReason> {
    storage
        .skeleton(entity.index() as usize)
        .and_then(|skeleton| skeleton.runtime.pose.as_ref())
        .filter(|pose| pose.valid)
        .ok_or(ErrorReason::MissingComponent)?
        .local
        .get(joint as usize)
        .ok_or(ErrorReason::InvalidField)
}

fn joint_local_mut(
    storage: &mut ComponentStorage,
    entity: EntityId,
    joint: u32,
) -> Result<&mut Transform, ErrorReason> {
    storage
        .skeleton_mut(entity.index() as usize)
        .and_then(|skeleton| skeleton.runtime.pose.as_mut())
        .filter(|pose| pose.valid)
        .ok_or(ErrorReason::MissingComponent)?
        .local
        .get_mut(joint as usize)
        .ok_or(ErrorReason::InvalidField)
}

fn frozen_value(
    channel: &TransitionChannel,
    value: AnimationValue,
    baseline: AnimationValue,
) -> AnimationRuntimeFrozenTransitionValue {
    let (target, incarnation, property) = match &channel.key {
        TransitionChannelKey::Property(identity) => (
            identity.entity,
            identity.incarnation,
            identity.property.clone(),
        ),
        TransitionChannelKey::Joint {
            entity,
            incarnation,
            joint,
        } => (
            *entity,
            *incarnation,
            AnimationTrackTarget::Joints(vec![*joint]),
        ),
    };
    AnimationRuntimeFrozenTransitionValue {
        target,
        incarnation,
        property,
        value,
        baseline,
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
        .and_then(|record| record.input(identity.property.component_target()))
        .is_some_and(|input| input.incarnation == identity.incarnation)
        && identity.property.indices().iter().all(|offset| {
            !super::binding::removable_field(*offset)
                || super::binding::present_field(state.input_field(
                    storage,
                    identity.entity,
                    identity.property.component_target(),
                    *offset,
                ))
        })
}

fn channel_value(value: &AnimationValue) -> Result<TransitionChannelValue, ErrorReason> {
    if let AnimationValue::Pose(values) = value {
        let [value] = values.as_slice() else {
            return Err(ErrorReason::InvalidField);
        };
        return Ok(TransitionChannelValue::Joint(*value));
    }
    Ok(TransitionChannelValue::Property(value.clone()))
}

fn frozen_key(
    value: &AnimationRuntimeFrozenTransitionValue,
) -> Result<TransitionChannelKey, ErrorReason> {
    if let AnimationTrackTarget::Joints(joints) = &value.property {
        let [joint] = joints.as_slice() else {
            return Err(ErrorReason::InvalidField);
        };
        return Ok(TransitionChannelKey::Joint {
            entity: value.target,
            incarnation: value.incarnation,
            joint: *joint,
        });
    }
    Ok(TransitionChannelKey::Property(
        super::driver::AnimationTargetIdentity {
            entity: value.target,
            incarnation: value.incarnation,
            property: value.property.clone(),
        },
    ))
}

fn channel_animation_value(value: &TransitionChannelValue) -> AnimationValue {
    match value {
        TransitionChannelValue::Property(value) => value.clone(),
        TransitionChannelValue::Joint(value) => AnimationValue::Pose(vec![*value]),
    }
}

fn validate_drivers(drivers: &[Box<dyn AnimationDriverBinding>]) -> Result<(), ErrorReason> {
    if drivers.iter().all(|driver| driver.contributes()) {
        Ok(())
    } else {
        Err(ErrorReason::InvalidField)
    }
}

/// One channel per driven property or joint, starting from the empty
/// contribution, or from the driver's template value for absolute channels.
fn collect_channels(
    drivers: &[Box<dyn AnimationDriverBinding>],
    storage: &ComponentStorage,
    contributions: bool,
    channels: &mut BTreeMap<TransitionChannelKey, (TransitionChannelValue, TransitionOutput)>,
) -> Result<(), ErrorReason> {
    for driver in drivers {
        let baseline = if contributions {
            super::contribution::identity_value(driver.template())
        } else {
            driver.template().clone()
        };
        if let AnimationTrackTarget::Joints(joints) = &driver.identity().property {
            for &joint in joints {
                channels
                    .entry(TransitionChannelKey::Joint {
                        entity: driver.identity().entity,
                        incarnation: driver.identity().incarnation,
                        joint,
                    })
                    .or_insert((
                        TransitionChannelValue::Joint(super::contribution::IDENTITY_JOINT),
                        TransitionOutput::Joint(driver.identity().entity, joint),
                    ));
            }
            continue;
        }
        let output = driver
            .transition_output(storage)
            .ok_or(ErrorReason::InvalidField)?;
        channels
            .entry(TransitionChannelKey::Property(driver.identity().clone()))
            .or_insert((
                TransitionChannelValue::Property(baseline),
                TransitionOutput::Value(output),
            ));
    }
    Ok(())
}

fn operations(
    drivers: &[Box<dyn AnimationDriverBinding>],
    channels: &BTreeMap<TransitionChannelKey, usize>,
) -> Vec<TransitionOperation> {
    drivers
        .iter()
        .enumerate()
        .map(|(driver_index, driver)| {
            if let AnimationTrackTarget::Joints(joints) = &driver.identity().property {
                let indices: Vec<_> = joints
                    .iter()
                    .map(|&joint| {
                        channels[&TransitionChannelKey::Joint {
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
                    channels: indices,
                };
            }
            TransitionOperation::Property {
                driver: driver_index,
                channel: channels[&TransitionChannelKey::Property(driver.identity().clone())],
            }
        })
        .collect()
}

fn evaluate_operations(
    operations: &mut [TransitionOperation],
    channels: &mut [TransitionChannel],
    drivers: &[Box<dyn AnimationDriverBinding>],
    time: f64,
    source_side: bool,
    contributions: bool,
) -> Result<(), ErrorReason> {
    for operation in operations {
        match operation {
            TransitionOperation::Constant {
                channel,
                value,
            } => {
                channels[*channel].destination = TransitionChannelValue::Property(value.clone());
            }
            TransitionOperation::Property {
                driver,
                channel,
            } => {
                let current = if source_side {
                    &channels[*channel].source
                } else {
                    &channels[*channel].destination
                };
                let TransitionChannelValue::Property(current) = current else {
                    return Err(ErrorReason::InvalidField);
                };
                let driver = &drivers[*driver];
                let value = TransitionChannelValue::Property(if contributions {
                    super::contribution::compose(current, &driver.contribution(time)?)?
                } else {
                    driver.sample(time)
                });
                if source_side {
                    channels[*channel].source = value;
                } else {
                    channels[*channel].destination = value;
                }
            }
            TransitionOperation::Pose {
                driver,
                channels: indices,
                current,
                output,
            } => {
                for (&channel, current) in indices.iter().zip(current.iter_mut()) {
                    let value = if source_side {
                        &channels[channel].source
                    } else {
                        &channels[channel].destination
                    };
                    let TransitionChannelValue::Joint(value) = value else {
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
                for (&channel, &value) in indices.iter().zip(output.iter()) {
                    if source_side {
                        channels[channel].source = TransitionChannelValue::Joint(value);
                    } else {
                        channels[channel].destination = TransitionChannelValue::Joint(value);
                    }
                }
            }
        }
    }
    Ok(())
}
