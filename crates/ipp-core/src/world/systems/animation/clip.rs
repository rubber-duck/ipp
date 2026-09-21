//! Immutable, bounded property and pose keyframes and time/value Bézier segments.

use super::math;
use crate::{
    ComponentValue, EntityId, ErrorReason,
    components::schema::{FieldKind, FieldValue},
    services::asset_management::*,
};
use std::{any::Any, collections::BTreeSet, fmt::Debug};

/// Compiled immutable animation format identity.
pub const ANIMATION_TYPE: AssetTypeId = AssetTypeId(10);

/// A typed property value, complete xyzw rotation or joint-local pose.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationValue {
    /// An ordinary exposed field.
    Field(FieldValue),
    /// Complete quaternion, normalized during sampling.
    Rotation([f32; 4]),
    /// SkeletonJoint-local TRS samples in the pose track's joint order.
    #[cfg(feature = "skeletal-animation")]
    Pose(Vec<crate::components::Transform>),
}

/// Outgoing interpolation from this key to the next.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationInterpolation<T = AnimationValue> {
    /// Hold the previous key until the next exact key time.
    Step,
    /// Linear numeric or shortest-arc spherical rotation interpolation.
    Linear,
    /// Cubic curve with absolute local times and typed value handles.
    Bezier {
        /// First control time.
        time1: f64,
        /// First control value.
        value1: T,
        /// Second control time.
        time2: f64,
        /// Second control value.
        value2: T,
    },
}

/// One key in animation-local seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationKeyframe<T = AnimationValue> {
    /// Nonnegative local key time.
    pub time: f64,
    /// Owned typed sample.
    pub value: T,
    /// Outgoing segment; the final key uses Step.
    pub interpolation: AnimationInterpolation<T>,
}

/// Component-local property binding resolved against the player's target.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationProperty {
    /// Compiled target component identity.
    pub component: u16,
    /// One exact exposed field offset, or four xyzw offsets for a rotation.
    pub offsets: Vec<u32>,
}

/// Stable target within the player's target entity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnimationTrackTarget {
    /// Named dynamic component property, resolved to its identity when bound.
    DynamicProperty {
        /// Compiled component identity with dynamic-property capability.
        component: u16,
        /// Named property resolved to a lifetime identity during preparation.
        name: String,
    },
    /// Target-generated component field offsets.
    AnimationProperty(AnimationProperty),
    /// Ascending skeleton joint ordinals, independent of CPU field layout.
    #[cfg(feature = "skeletal-animation")]
    Joints(Vec<u32>),
}

impl AnimationTrackTarget {
    /// Target component identity, independent of any World binding.
    pub fn component(&self) -> u16 {
        match self {
            Self::DynamicProperty {
                component,
                ..
            } => *component,
            Self::AnimationProperty(property) => property.component,
            #[cfg(feature = "skeletal-animation")]
            Self::Joints(_) => ComponentValue::SKELETON,
        }
    }

    pub(crate) fn property(&self) -> Option<&AnimationProperty> {
        match self {
            Self::DynamicProperty {
                ..
            } => None,
            Self::AnimationProperty(property) => Some(property),
            #[cfg(feature = "skeletal-animation")]
            Self::Joints(_) => None,
        }
    }

    /// Exact field offsets or ordered joint ordinals.
    pub fn indices(&self) -> &[u32] {
        match self {
            Self::DynamicProperty {
                ..
            } => &[],
            Self::AnimationProperty(property) => &property.offsets,
            #[cfg(feature = "skeletal-animation")]
            Self::Joints(joints) => joints,
        }
    }

    pub(crate) fn owned_bytes(&self) -> usize {
        match self {
            Self::DynamicProperty {
                name,
                ..
            } => name.capacity(),
            Self::AnimationProperty(property) => property.offsets.capacity() * 4,
            #[cfg(feature = "skeletal-animation")]
            Self::Joints(joints) => joints.capacity() * 4,
        }
    }

    fn is_pose(&self) -> bool {
        match self {
            Self::DynamicProperty {
                ..
            }
            | Self::AnimationProperty(_) => false,
            #[cfg(feature = "skeletal-animation")]
            Self::Joints(_) => true,
        }
    }
}

/// A reusable typed curve, with optional source property metadata for authoring tools.
/// The target metadata never contains a world entity or a live binding.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationTrack<T = AnimationValue> {
    /// Source property hint retained by the IPPA interchange format.
    pub target: AnimationTrackTarget,
    /// Contiguous, strictly increasing typed keyframes.
    pub keys: Vec<AnimationKeyframe<T>>,
}

/// Concrete values supported by compiled typed animation tracks and drivers.
pub trait AnimationSample: Clone + Debug + PartialEq + Send + Sync + 'static {
    /// Convert a boundary value after checking its exact type.
    fn from_value(value: AnimationValue) -> Result<Self, ErrorReason>;

    /// Convert one temporary value for schema validation or interchange encoding.
    fn to_value(&self) -> AnimationValue {
        self.clone().into_value()
    }

    /// Move one sample across the schema boundary without cloning owned values.
    fn into_value(self) -> AnimationValue;

    /// Owned allocation bytes, measured without constructing a sample.
    fn retained_bytes(&self) -> usize {
        0
    }

    /// Normalize a held sample while cloning its owned data only once.
    fn sampled(&self) -> Self {
        self.clone()
    }

    /// Interpolate one typed sample.
    fn interpolate(a: &Self, b: &Self, weight: f64) -> Self {
        Self::from_value(math::mix(&a.to_value(), &b.to_value(), weight))
            .expect("same typed animation values")
    }

    /// Evaluate a typed cubic segment.
    fn bezier(a: &Self, b: &Self, c: &Self, d: &Self, weight: f64) -> Self {
        Self::from_value(math::bezier_value(
            &a.to_value(),
            &b.to_value(),
            &c.to_value(),
            &d.to_value(),
            weight,
        ))
        .expect("same typed animation values")
    }
}

macro_rules! field_sample {
    ($type:ty, $variant:ident $(, $bytes:expr)?) => {
        impl AnimationSample for $type {
            fn from_value(value: AnimationValue) -> Result<Self, ErrorReason> {
                match value {
                    AnimationValue::Field(FieldValue::$variant(value)) => Ok(value),
                    _ => Err(ErrorReason::InvalidField),
                }
            }

            fn into_value(self) -> AnimationValue {
                AnimationValue::Field(FieldValue::$variant(self))
            }

            $(fn retained_bytes(&self) -> usize { $bytes(self) })?
        }
    };
}

field_sample!(crate::DynamicValue, Dynamic);
impl AnimationSample for f32 {
    fn from_value(value: AnimationValue) -> Result<Self, ErrorReason> {
        match value {
            AnimationValue::Field(FieldValue::F32(value)) => Ok(value),
            _ => Err(ErrorReason::InvalidField),
        }
    }

    fn into_value(self) -> AnimationValue {
        AnimationValue::Field(FieldValue::F32(self))
    }

    fn interpolate(a: &Self, b: &Self, weight: f64) -> Self {
        if weight <= 0.0 {
            return *a;
        }
        if weight >= 1.0 {
            return *b;
        }
        (f64::from(*a) * (1.0 - weight) + f64::from(*b) * weight) as f32
    }
}
field_sample!(u32, U32);
field_sample!(u64, U64);
field_sample!(bool, Bool);
field_sample!(EntityId, Entity);
field_sample!(String, String, |value: &String| value.capacity());
field_sample!(Vec<u8>, Bytes, |value: &Vec<u8>| value.capacity());

impl AnimationSample for [f32; 4] {
    fn from_value(value: AnimationValue) -> Result<Self, ErrorReason> {
        match value {
            AnimationValue::Rotation(value) => Ok(value),
            _ => Err(ErrorReason::InvalidField),
        }
    }

    fn into_value(self) -> AnimationValue {
        AnimationValue::Rotation(self)
    }

    fn sampled(&self) -> Self {
        math::normalize(*self)
    }

    fn interpolate(a: &Self, b: &Self, weight: f64) -> Self {
        math::slerp(*a, *b, weight)
    }
}

#[cfg(feature = "skeletal-animation")]
impl AnimationSample for Vec<crate::components::Transform> {
    fn from_value(value: AnimationValue) -> Result<Self, ErrorReason> {
        match value {
            AnimationValue::Pose(value) => Ok(value),
            _ => Err(ErrorReason::InvalidField),
        }
    }

    fn into_value(self) -> AnimationValue {
        AnimationValue::Pose(self)
    }

    fn retained_bytes(&self) -> usize {
        self.capacity() * std::mem::size_of::<crate::components::Transform>()
    }

    fn sampled(&self) -> Self {
        self.iter()
            .map(|joint| {
                let mut joint = *joint;
                [joint.qx, joint.qy, joint.qz, joint.qw] =
                    math::normalize([joint.qx, joint.qy, joint.qz, joint.qw]);
                joint
            })
            .collect()
    }

    fn interpolate(a: &Self, b: &Self, weight: f64) -> Self {
        super::pose::mix(a, b, weight)
    }
}

/// Erased track boundary. Resident key arrays remain in `AnimationTrack<T>`.
pub trait AnimationTrackData: Any + Debug + Send + Sync {
    /// Typed downcast used once when a driver binds, and checked on resource lookup.
    fn as_any(&self) -> &dyn Any;

    /// Source property metadata; controller drivers supply the actual target.
    fn target(&self) -> &AnimationTrackTarget;

    /// Number of immutable keyframes.
    fn key_count(&self) -> usize;

    /// Exact sample type tag from the interchange format.
    fn value_kind(&self) -> u8;

    /// Sample through an erased inspection boundary.
    fn sample_value(&self, time: f64) -> AnimationValue;

    /// Temporary interchange representation; never retained in the asset.
    fn interchange(&self) -> AnimationTrack;

    /// Allocated bytes retained by this concrete track.
    fn resident_bytes(&self) -> usize;
}

/// Driver-owned working set: only the current interpolation segment.
#[derive(Clone, Debug)]
pub(super) struct AnimationSampleSegment<T> {
    lower: f64,
    upper: f64,
    index: usize,
    key: AnimationKeyframe<T>,
    next: Option<(f64, T)>,
}

impl<T: AnimationSample> AnimationSampleSegment<T> {
    fn sample(&self, time: f64) -> T {
        sample_key(
            &self.key,
            self.next.as_ref().map(|(time, value)| (*time, value)),
            time,
        )
    }
}

fn sample_key<T: AnimationSample>(
    a: &AnimationKeyframe<T>,
    next: Option<(f64, &T)>,
    time: f64,
) -> T {
    let Some((end, b)) = next else {
        return a.value.sampled();
    };
    if time == a.time {
        return a.value.sampled();
    }
    match &a.interpolation {
        AnimationInterpolation::Step => a.value.sampled(),
        AnimationInterpolation::Linear => {
            T::interpolate(&a.value, b, (time - a.time) / (end - a.time))
        }
        AnimationInterpolation::Bezier {
            time1,
            value1,
            time2,
            value2,
        } => {
            let duration = end - a.time;
            let u = math::bezier_parameter(
                (time - a.time) / duration,
                (time1 - a.time) / duration,
                (time2 - a.time) / duration,
            );
            T::bezier(&a.value, value1, value2, b, u)
        }
    }
}

impl<T: AnimationSample> AnimationTrack<T> {
    /// Sample a typed curve, holding endpoints outside the keyed interval.
    pub fn sample(&self, time: f64) -> T {
        let upper = self.keys.partition_point(|key| key.time <= time);
        self.sample_interval(time, upper)
    }

    /// Reuse the last interval for coherent playback; arbitrary seeks retain
    /// logarithmic lookup and identical endpoint/interpolation behavior.
    pub(super) fn sample_cached(&self, time: f64, cursor: &std::cell::Cell<usize>) -> T {
        let previous = cursor.get();
        let contains = |upper: usize| {
            upper <= self.keys.len()
                && (upper == 0 || self.keys[upper - 1].time <= time)
                && self.keys.get(upper).is_none_or(|key| time < key.time)
        };
        let upper = if contains(previous) {
            previous
        } else if previous < self.keys.len() && contains(previous + 1) {
            previous + 1
        } else {
            self.keys.partition_point(|key| key.time <= time)
        };
        cursor.set(upper);
        self.sample_interval(time, upper)
    }

    pub(super) fn sample_segment(
        &self,
        time: f64,
        segment: &mut Option<AnimationSampleSegment<T>>,
    ) -> T {
        if let Some(cached) = segment
            && cached.lower <= time
            && time < cached.upper
        {
            return cached.sample(time);
        }
        let upper = if let Some(cached) = segment
            && cached.index < self.keys.len()
            && cached.upper <= time
            && self
                .keys
                .get(cached.index + 1)
                .is_none_or(|key| time < key.time)
        {
            cached.index + 1
        } else {
            self.keys.partition_point(|key| key.time <= time)
        };
        let key = self.keys[upper.saturating_sub(1)].clone();
        let next = (upper != 0).then(|| self.keys.get(upper)).flatten();
        let cached = AnimationSampleSegment {
            index: upper,
            lower: if upper == 0 {
                f64::NEG_INFINITY
            } else {
                key.time
            },
            upper: self.keys.get(upper).map_or(f64::INFINITY, |key| key.time),
            key,
            next: next.map(|key| (key.time, key.value.clone())),
        };
        let value = cached.sample(time);
        *segment = Some(cached);
        value
    }

    fn sample_interval(&self, time: f64, upper: usize) -> T {
        if upper == 0 {
            return self.keys[0].value.sampled();
        }
        sample_key(
            &self.keys[upper - 1],
            self.keys.get(upper).map(|key| (key.time, &key.value)),
            time,
        )
    }
}

impl<T: AnimationSample> AnimationTrackData for AnimationTrack<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn target(&self) -> &AnimationTrackTarget {
        &self.target
    }

    fn key_count(&self) -> usize {
        self.keys.len()
    }

    fn value_kind(&self) -> u8 {
        self.keys
            .first()
            .map_or(0, |key| key.value.to_value().kind())
    }

    fn sample_value(&self, time: f64) -> AnimationValue {
        self.sample(time).into_value()
    }

    fn interchange(&self) -> AnimationTrack {
        AnimationTrack {
            target: self.target.clone(),
            keys: self
                .keys
                .iter()
                .map(|key| AnimationKeyframe {
                    time: key.time,
                    value: key.value.to_value(),
                    interpolation: match &key.interpolation {
                        AnimationInterpolation::Step => AnimationInterpolation::Step,
                        AnimationInterpolation::Linear => AnimationInterpolation::Linear,
                        AnimationInterpolation::Bezier {
                            time1,
                            value1,
                            time2,
                            value2,
                        } => AnimationInterpolation::Bezier {
                            time1: *time1,
                            value1: value1.to_value(),
                            time2: *time2,
                            value2: value2.to_value(),
                        },
                    },
                })
                .collect(),
        }
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.target.owned_bytes()
            + self.keys.capacity() * std::mem::size_of::<AnimationKeyframe<T>>()
            + self
                .keys
                .iter()
                .map(|key| {
                    key.value.retained_bytes()
                        + match &key.interpolation {
                            AnimationInterpolation::Bezier {
                                value1,
                                value2,
                                ..
                            } => value1.retained_bytes() + value2.retained_bytes(),
                            _ => 0,
                        }
                })
                .sum::<usize>()
    }
}

/// Convert typed or interchange tracks into an owned heterogeneous clip.
pub trait IntoAnimationTrack {
    /// Produce a concrete typed track allocation.
    fn into_animation_track(self) -> Result<Box<dyn AnimationTrackData>, ErrorReason>;
}

impl<T: AnimationSample> IntoAnimationTrack for AnimationTrack<T> {
    fn into_animation_track(self) -> Result<Box<dyn AnimationTrackData>, ErrorReason> {
        Ok(Box::new(self))
    }
}

impl IntoAnimationTrack for Box<dyn AnimationTrackData> {
    fn into_animation_track(self) -> Result<Box<dyn AnimationTrackData>, ErrorReason> {
        Ok(self)
    }
}

impl AnimationTrack {
    fn typed<T: AnimationSample>(self) -> Result<Box<dyn AnimationTrackData>, ErrorReason> {
        let mut track = AnimationTrack::<T> {
            target: self.target,
            keys: self
                .keys
                .into_iter()
                .map(|key| {
                    Ok(AnimationKeyframe {
                        time: key.time,
                        value: T::from_value(key.value)?,
                        interpolation: match key.interpolation {
                            AnimationInterpolation::Step => AnimationInterpolation::Step,
                            AnimationInterpolation::Linear => AnimationInterpolation::Linear,
                            AnimationInterpolation::Bezier {
                                time1,
                                value1,
                                time2,
                                value2,
                            } => AnimationInterpolation::Bezier {
                                time1,
                                value1: T::from_value(value1)?,
                                time2,
                                value2: T::from_value(value2)?,
                            },
                        },
                    })
                })
                .collect::<Result<_, ErrorReason>>()?,
        };
        // Collect can reuse the much larger interchange-enum allocation in place.
        // Immutable scalar/rotation tracks must not retain that spare byte capacity.
        track.keys.shrink_to_fit();
        Ok(Box::new(track))
    }
}

impl IntoAnimationTrack for AnimationTrack {
    fn into_animation_track(self) -> Result<Box<dyn AnimationTrackData>, ErrorReason> {
        let kind = self
            .keys
            .first()
            .ok_or(ErrorReason::InvalidAsset)?
            .value
            .kind();
        match kind {
            11 => self.typed::<crate::DynamicValue>(),
            1 => self.typed::<f32>(),
            2 => self.typed::<EntityId>(),
            3 => self.typed::<u32>(),
            4 => self.typed::<u64>(),
            5 => self.typed::<String>(),
            6 => self.typed::<Vec<u8>>(),
            7 => self.typed::<bool>(),
            8 => self.typed::<[f32; 4]>(),
            #[cfg(feature = "skeletal-animation")]
            9 => self.typed::<Vec<crate::components::Transform>>(),
            _ => Err(ErrorReason::InvalidAsset),
        }
        .map_err(|_| ErrorReason::InvalidAsset)
    }
}

/// Immutable source payload. Prepared drivers share typed tracks until the source
/// release barrier.
#[derive(Debug)]
pub struct AnimationClip {
    duration: f64,
    tracks: Vec<std::sync::Arc<dyn AnimationTrackData>>,
    bytes: usize,
}

impl PartialEq for AnimationClip {
    fn eq(&self, other: &Self) -> bool {
        self.encode() == other.encode()
    }
}

impl AnimationClip {
    /// Validate and retain an owned keyframe source.
    pub fn new<T: IntoAnimationTrack>(duration: f64, tracks: Vec<T>) -> Result<Self, ErrorReason> {
        let owned_tracks = tracks
            .into_iter()
            .map(IntoAnimationTrack::into_animation_track)
            .collect::<Result<Vec<_>, _>>()?;
        let tracks: Vec<_> = owned_tracks
            .iter()
            .map(|track| track.interchange())
            .collect();
        if !duration.is_finite()
            || duration <= 0.0
            || tracks.is_empty()
            || u32::try_from(tracks.len()).is_err()
        {
            return Err(ErrorReason::InvalidAsset);
        }
        let pose_tracks = tracks.iter().any(|track| {
            track.target.is_pose()
                || track.keys.iter().any(|key| key.value.kind() == 11)
                || matches!(track.target, AnimationTrackTarget::DynamicProperty { .. })
        });
        let mut bytes = 20usize;
        for track in &tracks {
            if u32::try_from(track.keys.len()).is_err() || track.keys.is_empty() {
                return Err(ErrorReason::InvalidAsset);
            }
            let kind = track.keys[0].value.kind();
            match &track.target {
                AnimationTrackTarget::DynamicProperty {
                    component,
                    name,
                } => {
                    crate::DynamicProperties::validate_name(name)
                        .map_err(|_| ErrorReason::InvalidAsset)?;
                    if !ComponentValue::supports_dynamic_properties(*component) || kind != 11 {
                        return Err(ErrorReason::InvalidAsset);
                    }
                    let expected = match &track.keys[0].value {
                        AnimationValue::Field(FieldValue::Dynamic(v)) => v.kind(),
                        _ => unreachable!(),
                    };
                    for key in &track.keys {
                        let valid = |v: &AnimationValue| matches!(v, AnimationValue::Field(FieldValue::Dynamic(v)) if v.kind() == expected);
                        if !valid(&key.value)
                            || matches!(&key.interpolation, AnimationInterpolation::Bezier { value1, value2, .. } if !valid(value1) || !valid(value2))
                        {
                            return Err(ErrorReason::InvalidAsset);
                        }
                    }
                }
                AnimationTrackTarget::AnimationProperty(property) => {
                    if !matches!(property.offsets.len(), 1 | 4)
                        || (kind == 8) != (property.offsets.len() == 4)
                        || kind == 9
                    {
                        return Err(ErrorReason::InvalidAsset);
                    }
                }
                #[cfg(feature = "skeletal-animation")]
                AnimationTrackTarget::Joints(joints) => {
                    if joints.is_empty()
                        || joints.len() > crate::MAX_JOINTS
                        || joints
                            .iter()
                            .any(|&joint| joint as usize >= crate::MAX_JOINTS)
                        || joints.windows(2).any(|pair| pair[0] >= pair[1])
                        || kind != 9
                    {
                        return Err(ErrorReason::InvalidAsset);
                    }
                    for key in &track.keys {
                        let valid = |value: &AnimationValue| matches!(value, AnimationValue::Pose(pose) if pose.len() == joints.len());
                        if !valid(&key.value)
                            || matches!(&key.interpolation,
                            AnimationInterpolation::Bezier { value1, value2, .. } if !valid(value1) || !valid(value2))
                        {
                            return Err(ErrorReason::InvalidAsset);
                        }
                    }
                }
            }
            let mut properties = BTreeSet::new();
            for &index in track.target.indices() {
                if !properties.insert(index) {
                    return Err(ErrorReason::InvalidAsset);
                }
            }
            bytes += if let AnimationTrackTarget::DynamicProperty {
                name,
                ..
            } = &track.target
            {
                10 + name.len()
            } else if track.target.is_pose() {
                8
            } else {
                7
            } + 4 * track.target.indices().len()
                + usize::from(pose_tracks);
            for (index, key) in track.keys.iter().enumerate() {
                if !key.time.is_finite()
                    || !(0.0..=duration).contains(&key.time)
                    || !key.value.same_type(&track.keys[0].value)
                {
                    return Err(ErrorReason::InvalidAsset);
                }
                key.value.validate()?;
                bytes = bytes
                    .checked_add(9 + key.value.encoded_bytes())
                    .ok_or(ErrorReason::Capacity)?;
                let next = track.keys.get(index + 1);
                if next.is_some_and(|next| key.time >= next.time) {
                    return Err(ErrorReason::InvalidAsset);
                }
                match &key.interpolation {
                    AnimationInterpolation::Step => {}
                    AnimationInterpolation::Linear if next.is_some() && key.value.numeric() => {}
                    AnimationInterpolation::Bezier {
                        time1,
                        value1,
                        time2,
                        value2,
                    } => {
                        let next = next.ok_or(ErrorReason::InvalidAsset)?;
                        if !key.value.numeric()
                            || !time1.is_finite()
                            || !time2.is_finite()
                            || !(key.time..=next.time).contains(time1)
                            || !(time1..=&next.time).contains(&time2)
                            || !value1.same_type(&key.value)
                            || !value2.same_type(&key.value)
                        {
                            return Err(ErrorReason::InvalidAsset);
                        }
                        value1.validate()?;
                        value2.validate()?;
                        bytes = bytes
                            .checked_add(16 + value1.encoded_bytes() + value2.encoded_bytes())
                            .ok_or(ErrorReason::Capacity)?;
                    }
                    _ => return Err(ErrorReason::InvalidAsset),
                }
            }
        }

        Ok(Self {
            duration,
            tracks: owned_tracks.into_iter().map(std::sync::Arc::from).collect(),
            bytes,
        })
    }

    /// Positive duration in local seconds.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Immutable tracks in authored order; reload never reorders their indices.
    pub fn tracks(&self) -> &[std::sync::Arc<dyn AnimationTrackData>] {
        &self.tracks
    }

    /// Resolve an exact track type through its stable source index.
    pub fn typed_track<T: AnimationSample>(&self, index: usize) -> Option<&AnimationTrack<T>> {
        self.tracks.get(index)?.as_any().downcast_ref()
    }

    pub(super) fn shared_track<T: AnimationSample>(
        &self,
        index: usize,
    ) -> Option<std::sync::Arc<AnimationTrack<T>>> {
        let track: std::sync::Arc<dyn Any + Send + Sync> = self.tracks.get(index)?.clone();
        track.downcast().ok()
    }

    /// Inspect one sample through the schema boundary.
    pub fn sample(&self, track: usize, time: f64) -> AnimationValue {
        self.tracks[track].sample_value(time)
    }

    /// Encode IPPA v1 property clips or v2 joint/pose clips.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes);
        let version = if self.tracks().iter().any(|track| {
            matches!(track.target(), AnimationTrackTarget::DynamicProperty { .. })
                || track.value_kind() == 11
        }) {
            3u32
        } else if self.tracks().iter().any(|track| track.target().is_pose()) {
            2u32
        } else {
            1
        };
        out.extend_from_slice(b"IPPA");
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&self.duration().to_le_bytes());
        out.extend_from_slice(&(self.tracks().len() as u32).to_le_bytes());
        for track in self.tracks() {
            let track = track.interchange();
            if version >= 2 {
                out.push(
                    if matches!(track.target, AnimationTrackTarget::DynamicProperty { .. }) {
                        2
                    } else {
                        u8::from(track.target.is_pose())
                    },
                );
            }
            match &track.target {
                AnimationTrackTarget::DynamicProperty {
                    component,
                    name,
                } => {
                    out.extend(component.to_le_bytes());
                    out.extend((name.len() as u32).to_le_bytes());
                    out.extend(name.as_bytes());
                }
                AnimationTrackTarget::AnimationProperty(property) => {
                    out.extend_from_slice(&property.component.to_le_bytes());
                    out.push(property.offsets.len() as u8);
                }
                #[cfg(feature = "skeletal-animation")]
                AnimationTrackTarget::Joints(joints) => {
                    out.extend_from_slice(&(joints.len() as u32).to_le_bytes())
                }
            }
            for index in track.target.indices() {
                out.extend_from_slice(&index.to_le_bytes());
            }
            out.extend_from_slice(&(track.keys.len() as u32).to_le_bytes());
            for key in &track.keys {
                out.extend_from_slice(&key.time.to_le_bytes());
                key.value.encode(&mut out);
                match &key.interpolation {
                    AnimationInterpolation::Step => out.push(0),
                    AnimationInterpolation::Linear => out.push(1),
                    AnimationInterpolation::Bezier {
                        time1,
                        value1,
                        time2,
                        value2,
                    } => {
                        out.push(2);
                        out.extend_from_slice(&time1.to_le_bytes());
                        value1.encode(&mut out);
                        out.extend_from_slice(&time2.to_le_bytes());
                        value2.encode(&mut out);
                    }
                }
            }
        }
        out
    }

    /// Decode exact owned data, rejecting unknown tags, trailing bytes and malformed lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        let mut r = AnimationClipReader {
            bytes,
            cursor: 0,
        };
        if r.take(4)? != b"IPPA" {
            return Err(ErrorReason::InvalidAsset);
        }
        let version = r.u32()?;
        if version != 1 && version != 3 && !(cfg!(feature = "skeletal-animation") && version == 2) {
            return Err(ErrorReason::InvalidAsset);
        }
        let duration = r.f64()?;
        let count = r.u32()? as usize;
        let mut tracks = Vec::new();
        for _ in 0..count {
            let tag = if version == 1 {
                0
            } else {
                r.byte()?
            };
            let target = match tag {
                2 if version >= 3 => {
                    let component = u16::from_le_bytes(r.array()?);
                    let length = r.u32()? as usize;
                    let name = std::str::from_utf8(r.take(length)?)
                        .map_err(|_| ErrorReason::InvalidAsset)?
                        .to_owned();
                    AnimationTrackTarget::DynamicProperty {
                        component,
                        name,
                    }
                }
                0 => {
                    let component = u16::from_le_bytes(r.array()?);
                    let n = r.byte()?;
                    if !matches!(n, 1 | 4) {
                        return Err(ErrorReason::InvalidAsset);
                    }
                    let mut offsets = Vec::new();
                    for _ in 0..n {
                        offsets.push(r.u32()?);
                    }
                    AnimationTrackTarget::AnimationProperty(AnimationProperty {
                        component,
                        offsets,
                    })
                }
                #[cfg(feature = "skeletal-animation")]
                1 => {
                    let n = r.u32()? as usize;
                    if n == 0 || n > crate::MAX_JOINTS {
                        return Err(ErrorReason::InvalidAsset);
                    }
                    let mut joints = Vec::with_capacity(n);
                    for _ in 0..n {
                        joints.push(r.u32()?);
                    }
                    AnimationTrackTarget::Joints(joints)
                }
                _ => return Err(ErrorReason::InvalidAsset),
            };
            let count = r.u32()? as usize;
            let mut keys = Vec::new();
            for _ in 0..count {
                let time = r.f64()?;
                let value = r.value()?;
                let interpolation = match r.byte()? {
                    0 => AnimationInterpolation::Step,
                    1 => AnimationInterpolation::Linear,
                    2 => AnimationInterpolation::Bezier {
                        time1: r.f64()?,
                        value1: r.value()?,
                        time2: r.f64()?,
                        value2: r.value()?,
                    },
                    _ => return Err(ErrorReason::InvalidAsset),
                };
                keys.push(AnimationKeyframe {
                    time,
                    value,
                    interpolation,
                });
            }
            tracks.push(AnimationTrack {
                target,
                keys,
            });
        }
        if r.cursor != bytes.len() {
            return Err(ErrorReason::InvalidAsset);
        }
        Self::new(duration, tracks)
    }
}

impl AnimationValue {
    pub(crate) fn same_type(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Field(FieldValue::Dynamic(a)), Self::Field(FieldValue::Dynamic(b))) => {
                a.kind() == b.kind()
            }
            _ => self.kind() == other.kind(),
        }
    }

    pub(crate) fn kind(&self) -> u8 {
        match self {
            Self::Field(v) => v.kind() as u8,
            Self::Rotation(_) => 8,
            #[cfg(feature = "skeletal-animation")]
            Self::Pose(_) => 9,
        }
    }

    pub(crate) fn numeric(&self) -> bool {
        match self {
            Self::Field(FieldValue::Dynamic(value)) => !matches!(
                value,
                crate::DynamicValue::Bool(_) | crate::DynamicValue::Asset(_)
            ),
            _ => matches!(self.kind(), 1 | 3 | 4 | 8 | 9),
        }
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        match self {
            Self::Field(FieldValue::Dynamic(value)) => {
                value.validate().map_err(|_| ErrorReason::InvalidAsset)
            }
            Self::Field(FieldValue::String(value)) if u32::try_from(value.len()).is_err() => {
                Err(ErrorReason::InvalidAsset)
            }
            Self::Field(FieldValue::Bytes(value)) if u32::try_from(value.len()).is_err() => {
                Err(ErrorReason::InvalidAsset)
            }
            Self::Field(FieldValue::F32(v)) if !v.is_finite() => Err(ErrorReason::InvalidAsset),
            Self::Rotation(q)
                if q.iter().any(|v| !v.is_finite()) || q.iter().all(|v| *v == 0.0) =>
            {
                Err(ErrorReason::InvalidAsset)
            }
            #[cfg(feature = "skeletal-animation")]
            Self::Pose(joints) => {
                use crate::components::schema::ComponentLifecycle;
                if joints.is_empty() || joints.len() > crate::MAX_JOINTS {
                    return Err(ErrorReason::InvalidAsset);
                }
                for joint in joints {
                    joint.validate().map_err(|_| ErrorReason::InvalidAsset)?;
                    crate::systems::camera::model_matrix(joint)
                        .map_err(|_| ErrorReason::InvalidAsset)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn write(
        &self,
        property: &AnimationProperty,
        component: &mut ComponentValue,
    ) -> Result<(), ErrorReason> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(194, "animation.write");

        let mut write = |offset, value| {
            component
                .set_field(offset, value)
                .map_err(|error| match error {
                    crate::components::schema::FieldError::NonFinite => ErrorReason::InvalidValue,
                    _ => ErrorReason::InvalidField,
                })
        };
        match self {
            Self::Field(value) => {
                if let Some(&offset) = property.offsets.first() {
                    write(offset, value.clone())?;
                }
            }
            Self::Rotation(q) => {
                for (&offset, value) in property.offsets.iter().zip(math::normalize(*q)) {
                    write(offset, FieldValue::F32(value))?;
                }
            }
            #[cfg(feature = "skeletal-animation")]
            Self::Pose(_) => return Err(ErrorReason::InvalidField),
        }
        Ok(())
    }

    pub(crate) fn read(
        property: &AnimationProperty,
        component: &ComponentValue,
    ) -> Result<Self, ErrorReason> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = crate::profiling::AllocationScope::new(193, "animation.read");

        Self::read_fields(property, |offset| component.field(offset).ok())
    }

    pub(crate) fn read_fields(
        property: &AnimationProperty,
        mut field: impl FnMut(u32) -> Option<FieldValue>,
    ) -> Result<Self, ErrorReason> {
        if let [offset] = property.offsets.as_slice() {
            return field(*offset)
                .map(Self::Field)
                .ok_or(ErrorReason::InvalidField);
        }
        use crate::components::Transform;
        if property.component != ComponentValue::TRANSFORM
            || property.offsets
                != [
                    std::mem::offset_of!(Transform, qx) as u32,
                    std::mem::offset_of!(Transform, qy) as u32,
                    std::mem::offset_of!(Transform, qz) as u32,
                    std::mem::offset_of!(Transform, qw) as u32,
                ]
        {
            return Err(ErrorReason::InvalidField);
        }

        let mut q = [0.0; 4];
        for (out, &offset) in q.iter_mut().zip(&property.offsets) {
            let FieldValue::F32(value) = field(offset).ok_or(ErrorReason::InvalidField)? else {
                return Err(ErrorReason::InvalidField);
            };
            *out = value;
        }
        Ok(Self::Rotation(q))
    }

    fn encoded_bytes(&self) -> usize {
        1 + match self {
            Self::Field(FieldValue::Dynamic(value)) => 4 + value.encode().len(),
            Self::Field(FieldValue::F32(_) | FieldValue::U32(_)) => 4,
            Self::Field(FieldValue::U64(_) | FieldValue::Entity(_)) => 8,
            Self::Field(FieldValue::Bool(_)) => 1,
            Self::Field(FieldValue::String(v)) => 4 + v.len(),
            Self::Field(FieldValue::Bytes(v)) => 4 + v.len(),
            Self::Rotation(_) => 16,
            #[cfg(feature = "skeletal-animation")]
            Self::Pose(pose) => 4 + pose.len() * 40,
        }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.kind());
        match self {
            Self::Field(FieldValue::Dynamic(value)) => {
                let bytes = value.encode();
                out.extend((bytes.len() as u32).to_le_bytes());
                out.extend(bytes);
            }
            Self::Field(FieldValue::F32(v)) => out.extend_from_slice(&v.to_le_bytes()),
            Self::Field(FieldValue::U32(v)) => out.extend_from_slice(&v.to_le_bytes()),
            Self::Field(FieldValue::U64(v)) => out.extend_from_slice(&v.to_le_bytes()),
            Self::Field(FieldValue::Entity(v)) => out.extend_from_slice(&v.to_bits().to_le_bytes()),
            Self::Field(FieldValue::Bool(v)) => out.push(u8::from(*v)),
            Self::Field(FieldValue::String(v)) => {
                out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                out.extend_from_slice(v.as_bytes());
            }
            Self::Field(FieldValue::Bytes(v)) => {
                out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                out.extend_from_slice(v);
            }
            #[cfg(feature = "skeletal-animation")]
            Self::Pose(pose) => {
                out.extend_from_slice(&(pose.len() as u32).to_le_bytes());
                for joint in pose {
                    crate::services::asset_management::skeleton::encode_transform(out, joint);
                }
            }
            Self::Rotation(q) => {
                for v in q {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}

struct AnimationClipReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> AnimationClipReader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ErrorReason> {
        let end = self
            .cursor
            .checked_add(n)
            .ok_or(ErrorReason::InvalidAsset)?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or(ErrorReason::InvalidAsset)?;
        self.cursor = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ErrorReason> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    fn byte(&mut self) -> Result<u8, ErrorReason> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, ErrorReason> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn f64(&mut self) -> Result<f64, ErrorReason> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    fn value(&mut self) -> Result<AnimationValue, ErrorReason> {
        let tag = self.byte()?;
        let value = match tag {
            11 => {
                let length = self.u32()? as usize;
                FieldValue::Dynamic(
                    crate::DynamicValue::decode(self.take(length)?)
                        .map_err(|_| ErrorReason::InvalidAsset)?,
                )
            }
            1 => FieldValue::F32(f32::from_le_bytes(self.array()?)),
            2 => FieldValue::Entity(EntityId::from_bits(u64::from_le_bytes(self.array()?))),
            3 => FieldValue::U32(self.u32()?),
            4 => FieldValue::U64(u64::from_le_bytes(self.array()?)),
            5 | 6 => {
                let len = self.u32()? as usize;
                let bytes = self.take(len)?;
                if tag == FieldKind::String as u8 {
                    FieldValue::String(
                        std::str::from_utf8(bytes)
                            .map_err(|_| ErrorReason::InvalidAsset)?
                            .to_owned(),
                    )
                } else {
                    FieldValue::Bytes(bytes.to_vec())
                }
            }
            7 => FieldValue::Bool(match self.byte()? {
                0 => false,
                1 => true,
                _ => return Err(ErrorReason::InvalidAsset),
            }),
            8 => {
                return Ok(AnimationValue::Rotation([
                    f32::from_le_bytes(self.array()?),
                    f32::from_le_bytes(self.array()?),
                    f32::from_le_bytes(self.array()?),
                    f32::from_le_bytes(self.array()?),
                ]));
            }
            #[cfg(feature = "skeletal-animation")]
            9 => {
                let count = self.u32()? as usize;
                if count == 0 || count > crate::MAX_JOINTS {
                    return Err(ErrorReason::InvalidAsset);
                }
                let mut pose = Vec::with_capacity(count);
                for _ in 0..count {
                    pose.push(crate::services::asset_management::skeleton::transform(
                        self.take(40)?,
                    )?);
                }
                return Ok(AnimationValue::Pose(pose));
            }
            _ => return Err(ErrorReason::InvalidAsset),
        };
        Ok(AnimationValue::Field(value))
    }
}

impl Asset for AnimationClip {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.tracks.capacity() * std::mem::size_of::<std::sync::Arc<dyn AnimationTrackData>>()
            + self
                .tracks
                .iter()
                .map(|track| track.resident_bytes())
                .sum::<usize>()
    }
}

pub(crate) fn animation_asset_loader() -> impl AssetLoader<Data = AnimationClip> {
    BufferedAssetLoader::new(|bytes| {
        AnimationClip::decode(bytes).map_err(|error| error.to_string())
    })
}

impl crate::services::asset_management::writer::AssetEncoder for AnimationClip {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        if self.bytes > max_bytes {
            return Err("Asset output byte budget exhausted".into());
        }
        Ok(self.encode())
    }
}

#[cfg(test)]
#[path = "clip_sampling_tests.rs"]
mod sampling_tests;
