//! SkeletonJoint-local TRS follows the same interpolation and additive rules as properties.

use super::{AnimationValue, math};
use crate::{ErrorReason, components::Transform};

pub(in crate::world) fn mix(a: &[Transform], b: &[Transform], weight: f64) -> Vec<Transform> {
    debug_assert_eq!(a.len(), b.len());

    a.iter()
        .zip(b)
        .map(|(a, b)| mix_joint(a, b, weight))
        .collect()
}

pub(in crate::world) fn additive(
    base: &[Transform],
    sample: &[Transform],
    reference: &[Transform],
    weight: f64,
) -> Result<Vec<Transform>, ErrorReason> {
    if base.len() != sample.len() || base.len() != reference.len() {
        return Err(ErrorReason::InvalidValue);
    }

    base.iter()
        .zip(sample)
        .zip(reference)
        .map(|((a, b), c)| additive_joint(a, b, c, weight))
        .collect()
}

fn rotation(transform: &Transform) -> [f32; 4] {
    [transform.qx, transform.qy, transform.qz, transform.qw]
}

pub(super) fn mix_joint(a: &Transform, b: &Transform, weight: f64) -> Transform {
    let lerp = |a, b| (f64::from(a) * (1.0 - weight) + f64::from(b) * weight) as f32;
    let [qx, qy, qz, qw] = math::slerp(rotation(a), rotation(b), weight);

    Transform {
        x: lerp(a.x, b.x),
        y: lerp(a.y, b.y),
        z: lerp(a.z, b.z),
        qx,
        qy,
        qz,
        qw,
        sx: lerp(a.sx, b.sx),
        sy: lerp(a.sy, b.sy),
        sz: lerp(a.sz, b.sz),
    }
}

fn additive_joint(
    a: &Transform,
    b: &Transform,
    c: &Transform,
    weight: f64,
) -> Result<Transform, ErrorReason> {
    let add = |a, b, c| (f64::from(a) + (f64::from(b) - f64::from(c)) * weight) as f32;
    let AnimationValue::Rotation([qx, qy, qz, qw]) = math::additive(
        &AnimationValue::Rotation(rotation(a)),
        &AnimationValue::Rotation(rotation(b)),
        &AnimationValue::Rotation(rotation(c)),
        weight,
    )?
    else {
        unreachable!("rotation inputs")
    };

    Ok(Transform {
        x: add(a.x, b.x, c.x),
        y: add(a.y, b.y, c.y),
        z: add(a.z, b.z, c.z),
        qx,
        qy,
        qz,
        qw,
        sx: add(a.sx, b.sx, c.sx),
        sy: add(a.sy, b.sy, c.sy),
        sz: add(a.sz, b.sz, c.sz),
    })
}

pub(super) fn sample_transition_joints(
    driver: &dyn super::driver::AnimationDriverBinding,
    track: &super::AnimationTrack<Vec<Transform>>,
    duration: f64,
    time: f64,
    current: &[Transform],
    output: &mut [Transform],
) -> Result<(), ErrorReason> {
    if current.len() != output.len() {
        return Err(ErrorReason::InvalidField);
    }
    let description = driver.description();
    let time = if description.repeat {
        time.rem_euclid(duration)
    } else {
        time
    };
    let sample = JointSegment::new(track, time);
    let reference = description
        .additive
        .then(|| JointSegment::new(track, f64::from(description.reference_time)));
    for (index, (current, output)) in current.iter().zip(output).enumerate() {
        let sampled = sample.joint(index);
        *output = if let Some(reference) = &reference {
            additive_joint(
                current,
                &sampled,
                &reference.joint(index),
                f64::from(description.weight),
            )?
        } else if description.weight == 1.0 {
            sampled
        } else {
            mix_joint(current, &sampled, f64::from(description.weight))
        };
    }
    Ok(())
}

/// Borrow a segment once; sampling each joint then uses only stack values.
enum JointSegment<'a> {
    Held(&'a [Transform]),
    Linear(&'a [Transform], &'a [Transform], f64),
    Bezier(
        &'a [Transform],
        &'a [Transform],
        &'a [Transform],
        &'a [Transform],
        f64,
    ),
}

impl<'a> JointSegment<'a> {
    fn new(track: &'a super::AnimationTrack<Vec<Transform>>, time: f64) -> Self {
        let upper = track.keys.partition_point(|key| key.time <= time);
        if upper == 0 {
            return Self::Held(&track.keys[0].value);
        }
        let a = &track.keys[upper - 1];
        let Some(b) = track.keys.get(upper) else {
            return Self::Held(&a.value);
        };
        if time == a.time {
            return Self::Held(&a.value);
        }
        match &a.interpolation {
            super::AnimationInterpolation::Step => Self::Held(&a.value),
            super::AnimationInterpolation::Linear => {
                Self::Linear(&a.value, &b.value, (time - a.time) / (b.time - a.time))
            }
            super::AnimationInterpolation::Bezier {
                time1,
                value1,
                time2,
                value2,
            } => {
                let duration = b.time - a.time;
                let u = math::bezier_parameter(
                    (time - a.time) / duration,
                    (time1 - a.time) / duration,
                    (time2 - a.time) / duration,
                );
                Self::Bezier(&a.value, value1, value2, &b.value, u)
            }
        }
    }

    fn joint(&self, index: usize) -> Transform {
        match *self {
            Self::Held(values) => {
                let mut value = values[index];
                [value.qx, value.qy, value.qz, value.qw] = math::normalize(rotation(&value));
                value
            }
            Self::Linear(a, b, u) => mix_joint(&a[index], &b[index], u),
            Self::Bezier(a, b, c, d, u) => {
                let ab = mix_joint(&a[index], &b[index], u);
                let bc = mix_joint(&b[index], &c[index], u);
                let cd = mix_joint(&c[index], &d[index], u);
                mix_joint(&mix_joint(&ab, &bc, u), &mix_joint(&bc, &cd, u), u)
            }
        }
    }
}

pub(super) fn sample_joints(
    driver: &dyn super::driver::AnimationDriverBinding,
    track: &super::AnimationTrack<Vec<Transform>>,
    duration: f64,
    time: f64,
    storage: &mut crate::components::registry::ComponentStorage,
) -> Result<(), ErrorReason> {
    use crate::components::schema::ComponentLifecycle;
    let super::driver::AnimationRuntimeTarget::JointLocal {
        source,
        joints,
    } = driver.runtime_target()
    else {
        return Err(ErrorReason::InvalidField);
    };
    let description = driver.description();
    let time = if description.repeat {
        time.rem_euclid(duration)
    } else {
        time
    };
    let sample = JointSegment::new(track, time);
    let reference = description
        .additive
        .then(|| JointSegment::new(track, f64::from(description.reference_time)));
    let pose = storage
        .skeleton_mut(description.target.index() as usize)
        .and_then(|value| value.runtime.pose.as_mut())
        .filter(|pose| pose.valid && pose.source == *source)
        .ok_or(ErrorReason::MissingComponent)?;
    for (index, &joint) in joints.iter().enumerate() {
        let current = pose
            .local
            .get(joint as usize)
            .ok_or(ErrorReason::InvalidField)?;
        let sample = sample.joint(index);
        let result = if let Some(reference) = &reference {
            additive_joint(
                current,
                &sample,
                &reference.joint(index),
                f64::from(description.weight),
            )?
        } else if description.weight == 1.0 {
            sample
        } else {
            mix_joint(current, &sample, f64::from(description.weight))
        };
        result.validate()?;
        pose.evaluation[index] = result;
    }
    // Validate the complete contribution before publishing any joint of it.
    for (index, &joint) in joints.iter().enumerate() {
        pose.local[joint as usize] = pose.evaluation[index];
        pose.sampled[joint as usize] = true;
    }
    Ok(())
}

impl super::AnimationReadAccess<'_> {
    pub(super) fn refresh_joint_original(
        &self,
        driver: &mut dyn super::driver::AnimationDriverBinding,
    ) -> Result<(), ErrorReason> {
        let super::driver::AnimationRuntimeTarget::JointLocal {
            source,
            ..
        } = driver.runtime_target()
        else {
            return Err(ErrorReason::InvalidField);
        };
        let skeleton = self
            .state
            .input_skeleton(&self.world.components, driver.identity().entity)
            .ok_or(ErrorReason::MissingComponent)?;
        let asset = self
            .bound_skeleton_data(*source, &skeleton)
            .ok_or(ErrorReason::InvalidAsset)?;
        let pose = if skeleton.pose_source.is_empty() {
            None
        } else {
            let (_, pose) = self
                .source_data::<crate::PoseAsset>(
                    crate::POSE_TYPE,
                    &skeleton.pose_source,
                    skeleton.pose_variant,
                )
                .ok_or(ErrorReason::InvalidAsset)?;
            if pose.joints().len() != asset.joints().len() {
                return Err(ErrorReason::InvalidAsset);
            }
            Some(pose)
        };
        // The immutable bound target and mutable original are disjoint driver fields.
        let (joints, original) = driver
            .joint_original_mut()
            .ok_or(ErrorReason::InvalidField)?;
        if joints
            .last()
            .is_some_and(|&joint| joint as usize >= asset.joints().len())
        {
            return Err(ErrorReason::InvalidField);
        }
        let overrides =
            crate::services::asset_management::skeleton::override_iter(&skeleton.joints)?;
        if overrides
            .clone()
            .any(|(joint, _)| joint >= asset.joints().len())
        {
            return Err(ErrorReason::InvalidValue);
        }
        original.resize(joints.len(), Transform::default());
        for (value, &joint) in original.iter_mut().zip(joints) {
            *value = pose.map_or_else(
                || asset.joints()[joint as usize].rest,
                |pose| pose.joints()[joint as usize],
            );
        }
        for (joint, transform) in overrides {
            if let Ok(index) = joints.binary_search(&(joint as u32)) {
                original[index] = transform;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "pose_tests.rs"]
mod tests;
