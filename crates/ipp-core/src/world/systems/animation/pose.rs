//! SkeletonJoint-local TRS follows the same interpolation and contribution rules
//! as properties. The Skeleton rebuilds its local pose every frame before
//! animation, so joint contributions apply in full each frame and are not kept.

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

/// Compose each joint's contribution at `time` onto `current`, into `output`.
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
    let (sample, reference, weight) = segments(driver, track, duration, time);
    for (index, (current, output)) in current.iter().zip(output).enumerate() {
        let contribution =
            joint_contribution(&sample.joint(index), &reference.joint(index), weight)?;
        *output = super::contribution::compose_joint(current, &contribution);
    }
    Ok(())
}

fn segments<'a>(
    driver: &dyn super::driver::AnimationDriverBinding,
    track: &'a super::AnimationTrack<Vec<Transform>>,
    duration: f64,
    time: f64,
) -> (JointSegment<'a>, JointSegment<'a>, f64) {
    let description = driver.description();
    let time = if description.repeat {
        time.rem_euclid(duration)
    } else {
        time
    };
    let reference = if description.additive {
        f64::from(description.reference_time)
    } else {
        0.0
    };
    (
        JointSegment::new(track, time),
        JointSegment::new(track, reference),
        f64::from(description.weight),
    )
}

/// The weighted change of one joint from its reference sample.
fn joint_contribution(
    sample: &Transform,
    reference: &Transform,
    weight: f64,
) -> Result<Transform, ErrorReason> {
    additive_joint(
        &super::contribution::IDENTITY_JOINT,
        sample,
        reference,
        weight,
    )
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

/// Apply a joint driver's contribution onto the Skeleton's pose for this frame.
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
    let (sample, reference, weight) = segments(driver, track, duration, time);
    let pose = storage
        .skeleton_mut(driver.description().target.index() as usize)
        .and_then(|value| value.runtime.pose.as_mut())
        .filter(|pose| pose.valid && pose.source == *source)
        .ok_or(ErrorReason::MissingComponent)?;
    for (index, &joint) in joints.iter().enumerate() {
        let current = pose
            .local
            .get(joint as usize)
            .ok_or(ErrorReason::InvalidField)?;
        let contribution =
            joint_contribution(&sample.joint(index), &reference.joint(index), weight)?;
        let result = super::contribution::compose_joint(current, &contribution);
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

#[cfg(test)]
#[path = "pose_tests.rs"]
mod tests;
