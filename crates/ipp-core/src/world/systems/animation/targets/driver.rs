//! Typed per-property drivers and immutable resource-index bindings.
//!
//! A driver of a linear or rotation field computes a contribution, its weighted
//! change from the clip's reference sample; the controller adds it to the field.
//! A driver of any other field type writes its sample.

use crate::systems::animation::clip::AnimationSampleSegment;
use crate::systems::animation::controller::contribution;
use crate::systems::animation::{
    AnimationClip, AnimationDriverDescription, AnimationSample, AnimationTrack, AnimationTrackData,
    AnimationTrackTarget, AnimationValue,
};
use crate::{ComponentValue, EntityId, ErrorReason, services::asset_management::AssetKey};
use std::{any::Any, fmt::Debug};

/// Stable lifetime identity and exact property coverage of one driven target.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::world) struct AnimationTargetIdentity {
    pub entity: EntityId,
    pub incarnation: u64,
    pub property: AnimationTrackTarget,
}

/// Resolved runtime access is distinct from the serializable client declaration.
/// Internal targets never call generated component fields or field setters.
#[derive(Debug)]
pub(in crate::world) enum AnimationRuntimeTarget {
    Property,
    JointLocal {
        source: AssetKey,
        joints: Box<[u32]>,
    },
}

impl AnimationRuntimeTarget {
    fn bind(
        property: &AnimationTrackTarget,
        source: Option<AssetKey>,
    ) -> Result<Self, ErrorReason> {
        match property {
            AnimationTrackTarget::EntityLink => Err(ErrorReason::InvalidField),
            AnimationTrackTarget::DynamicProperty {
                ..
            }
            | AnimationTrackTarget::AnimationProperty(_) => Ok(Self::Property),
            AnimationTrackTarget::Joints(joints) => Ok(Self::JointLocal {
                source: source.ok_or(ErrorReason::InvalidAsset)?,
                joints: joints.clone().into_boxed_slice(),
            }),
        }
    }
}

/// A typed curve binding of one target property.
///
/// Target identity is validated at binding and mutation boundaries; target storage
/// is borrowed only within the exclusive World phase. Typed curve ownership and
/// cached segments are revoked before source unload, and target lifecycle hooks
/// drop the binding before its component incarnation is destroyed.
#[derive(Debug)]
pub struct AnimationDriver<T: AnimationSample> {
    pub(in crate::world) description: AnimationDriverDescription,
    pub(in crate::world) identity: AnimationTargetIdentity,
    pub(in crate::world) clip: AssetKey,
    /// A value of the target's type.
    template: AnimationValue,
    contributes: bool,
    interval: std::cell::Cell<usize>,
    duration: f64,
    track: Option<std::sync::Arc<AnimationTrack<T>>>,
    /// The sample contributions are measured from.
    reference: Option<AnimationValue>,
    segment: std::cell::RefCell<Option<AnimationSampleSegment<T>>>,
    cache_segment: bool,
    discrete: bool,
    discrete_interval: std::cell::Cell<Option<usize>>,
    pub(in crate::world) runtime_target: AnimationRuntimeTarget,
}

/// Compiled destination of one dynamic value: a named dynamic property kept by
/// its validated descriptor, or a row property kept by its offset.
#[derive(Clone, Copy, Debug)]
pub(in crate::world) enum DynamicValueDestination {
    Property(crate::world::direct_bindings::property_binding::PropertyBinding),
    Row(super::row_property_destination::RowPropertyDestination),
}

impl DynamicValueDestination {
    /// Bind a single non-asset dynamic property or numeric row property.
    ///
    /// # Safety
    /// Callers keep the destination only while its component incarnation and,
    /// for dynamic properties, the property identity stay live; animation
    /// lifecycle hooks drop it before either departs.
    unsafe fn bind(
        storage: &crate::components::registry::ComponentStorage,
        entity: EntityId,
        component: u16,
        key: u32,
    ) -> Option<Self> {
        if crate::components::rows::row_region(key).is_some() {
            return super::row_property_destination::RowPropertyDestination::bind(
                storage, entity, component, key,
            )
            .map(Self::Row);
        }

        // SAFETY: The caller revokes this cell/descriptor binding before reuse.
        unsafe {
            crate::world::direct_bindings::property_binding::PropertyBinding::bind(
                storage, entity, component, key,
            )
        }
        .filter(|binding| binding.writable())
        .map(Self::Property)
    }

    fn write(
        self,
        storage: &mut crate::components::registry::ComponentStorage,
        value: crate::DynamicValue,
    ) -> Result<(), ErrorReason> {
        match self {
            Self::Property(binding) => {
                binding.validate(storage, &value)?;
                binding.write_validated(storage, value);
            }
            Self::Row(destination) => return destination.write(storage, value),
        }
        Ok(())
    }
}

pub(in crate::world) trait AnimationDriverBinding: Debug {
    fn description(&self) -> &AnimationDriverDescription;

    fn identity(&self) -> &AnimationTargetIdentity;

    fn clip(&self) -> AssetKey;

    fn duration(&self) -> f64;

    /// A value of the target's type.
    fn template(&self) -> &AnimationValue;

    /// Whether this driver adds a contribution rather than writing its sample.
    fn contributes(&self) -> bool;

    /// The output a crossfade writes this driver's target through.
    fn transition_output(
        &self,
        storage: &crate::components::registry::ComponentStorage,
    ) -> Option<AnimationTransitionOutput>;

    fn discrete(&self) -> bool;

    fn reset_discrete(&self);

    fn unchanged_discrete(&self, time: f64) -> bool;

    fn mark_discrete(&self, time: f64);

    fn suspend_track(&mut self);

    #[cfg(feature = "instrumentation")]
    fn segment_bytes(&self) -> usize;

    fn resolve_track(&mut self, clip: &AnimationClip) -> Result<(), ErrorReason>;

    /// The clip's value at controller time `time`.
    fn sample(&self, time: f64) -> AnimationValue;

    /// The weighted change from the reference sample to the sample at `time`.
    fn contribution(&self, time: f64) -> Result<AnimationValue, ErrorReason>;

    fn bound_pose_track(&self) -> &AnimationTrack<Vec<crate::components::Transform>>;

    fn runtime_target(&self) -> &AnimationRuntimeTarget;

    fn as_any(&self) -> &dyn Any;

    fn skeleton_source(&self) -> Option<AssetKey>;
}

#[derive(Clone, Debug)]
pub(in crate::world) enum AnimationTransitionOutput {
    F32(
        crate::world::direct_bindings::component_binding::ComponentBinding<f32>,
        AnimationTargetIdentity,
    ),
    Rotation(
        crate::world::direct_bindings::component_binding::ComponentBinding<[f32; 4]>,
        AnimationTargetIdentity,
    ),
    Dynamic(DynamicValueDestination, AnimationTargetIdentity),
}

impl AnimationTransitionOutput {
    pub(in crate::world::systems::animation) fn validate(
        &self,
        value: &AnimationValue,
    ) -> Result<(), ErrorReason> {
        validate_transition_value(self.identity(), value)
    }

    pub(in crate::world::systems::animation) fn identity(&self) -> &AnimationTargetIdentity {
        match self {
            Self::F32(_, identity) | Self::Rotation(_, identity) | Self::Dynamic(_, identity) => {
                identity
            }
        }
    }

    /// The target's current value.
    pub(in crate::world::systems::animation) fn read(
        &self,
        storage: &crate::components::registry::ComponentStorage,
    ) -> Result<AnimationValue, ErrorReason> {
        Ok(match self {
            Self::F32(source, _) => AnimationValue::Field(
                crate::components::schema::FieldValue::F32(*source.get(storage)),
            ),
            Self::Rotation(source, _) => AnimationValue::Rotation(*source.get(storage)),
            Self::Dynamic(DynamicValueDestination::Property(binding), _) => {
                AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(
                    binding.read(storage).ok_or(ErrorReason::InvalidField)?,
                ))
            }
            Self::Dynamic(_, identity) => {
                let property = identity
                    .property
                    .property()
                    .ok_or(ErrorReason::InvalidField)?;
                let &[offset] = property.offsets.as_slice() else {
                    return Err(ErrorReason::InvalidField);
                };
                AnimationValue::Field(
                    storage
                        .field(property.component, identity.entity.index() as usize, offset)
                        .ok_or(ErrorReason::MissingComponent)?,
                )
            }
        })
    }

    pub(in crate::world::systems::animation) fn write(
        &self,
        storage: &mut crate::components::registry::ComponentStorage,
        value: AnimationValue,
    ) -> Result<(), ErrorReason> {
        match (self, value) {
            (
                Self::F32(destination, _),
                AnimationValue::Field(crate::components::schema::FieldValue::F32(value)),
            ) => {
                *destination.get_mut(storage) = value;
            }
            (Self::Rotation(destination, _), AnimationValue::Rotation(value)) => {
                *destination.get_mut(storage) = value;
            }
            (
                Self::Dynamic(destination, _),
                AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value)),
            ) => {
                destination.write(storage, value)?;
            }
            _ => return Err(ErrorReason::InvalidField),
        }
        Ok(())
    }
}

pub(in crate::world::systems::animation) fn bind_frozen_transition_output(
    identity: AnimationTargetIdentity,
    value: &AnimationValue,
    storage: &crate::components::registry::ComponentStorage,
) -> Option<AnimationTransitionOutput> {
    match value {
        AnimationValue::Field(crate::components::schema::FieldValue::F32(_)) => {
            Some(AnimationTransitionOutput::F32(
                super::numeric_binding::bind_frozen_f32(&identity, storage)?,
                identity,
            ))
        }
        AnimationValue::Rotation(_) => Some(AnimationTransitionOutput::Rotation(
            super::numeric_binding::bind_frozen_rotation(&identity, storage)?,
            identity,
        )),
        AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value))
            if !matches!(
                value,
                crate::DynamicValue::Bool(_) | crate::DynamicValue::Asset(_)
            ) =>
        {
            let property = identity.property.property()?;
            let &[key] = property.offsets.as_slice() else {
                return None;
            };
            // SAFETY: World lifecycle invalidation drops the transition before its
            // property identity, row or component incarnation departs, and
            // publication holds exclusive mutable access to component storage.
            let destination = unsafe {
                DynamicValueDestination::bind(storage, identity.entity, property.component, key)?
            };
            Some(AnimationTransitionOutput::Dynamic(destination, identity))
        }
        _ => None,
    }
}

pub(in crate::world::systems::animation) fn frozen_transition_target_supported(
    target: &AnimationTrackTarget,
    value: &AnimationValue,
) -> bool {
    match value {
        AnimationValue::Field(crate::components::schema::FieldValue::F32(_)) => target
            .property()
            .and_then(|property| property.offsets.as_slice().first().copied())
            .is_some_and(|offset| {
                target.property().unwrap().offsets.len() == 1
                    && super::numeric_binding::frozen_f32_field_supported(
                        target.component_target(),
                        offset,
                    )
            }),
        AnimationValue::Rotation(_) => target.property().is_some_and(|property| {
            property.component == ComponentValue::TRANSFORM
                && property.offsets
                    == [
                        std::mem::offset_of!(crate::components::Transform, qx) as u32,
                        std::mem::offset_of!(crate::components::Transform, qy) as u32,
                        std::mem::offset_of!(crate::components::Transform, qz) as u32,
                        std::mem::offset_of!(crate::components::Transform, qw) as u32,
                    ]
        }),
        AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value)) => {
            (matches!(target, AnimationTrackTarget::DynamicProperty { .. })
                || target.property().is_some_and(|property| {
                    matches!(property.offsets.as_slice(), [offset] if crate::components::rows::row_region(*offset).is_some())
                }))
                && !matches!(
                    value,
                    crate::DynamicValue::Bool(_) | crate::DynamicValue::Asset(_)
                )
        }
        AnimationValue::Pose(values) => {
            matches!(target, AnimationTrackTarget::Joints(joints) if joints.len() == 1 && values.len() == 1)
        }
        _ => false,
    }
}

fn validate_transition_value(
    identity: &AnimationTargetIdentity,
    value: &AnimationValue,
) -> Result<(), ErrorReason> {
    match value {
        AnimationValue::Field(crate::components::schema::FieldValue::F32(value)) => {
            if !value.is_finite() {
                return Err(ErrorReason::InvalidValue);
            }
            if let Some(property) = identity.property.property()
                && let [offset] = property.offsets.as_slice()
                && !transition_field_valid(property.component, *offset, *value)
            {
                return Err(ErrorReason::InvalidValue);
            }
        }
        AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value)) => {
            value.validate().map_err(|_| ErrorReason::InvalidValue)?;
        }
        AnimationValue::Rotation(value) if value.iter().any(|value| !value.is_finite()) => {
            return Err(ErrorReason::InvalidValue);
        }
        _ => {}
    }
    Ok(())
}

fn transition_field_valid(component: u16, offset: u32, value: f32) -> bool {
    crate::world::direct_bindings::numeric_properties::fixed_range(component, offset)
        .is_some_and(|range| range.contains(value))
}

impl<T: AnimationSample> AnimationDriverBinding for AnimationDriver<T> {
    fn description(&self) -> &AnimationDriverDescription {
        &self.description
    }

    fn identity(&self) -> &AnimationTargetIdentity {
        &self.identity
    }

    fn clip(&self) -> AssetKey {
        self.clip
    }

    fn duration(&self) -> f64 {
        self.duration
    }

    fn template(&self) -> &AnimationValue {
        &self.template
    }

    fn contributes(&self) -> bool {
        self.contributes
    }

    fn transition_output(
        &self,
        storage: &crate::components::registry::ComponentStorage,
    ) -> Option<AnimationTransitionOutput> {
        bind_frozen_transition_output(self.identity.clone(), &self.template, storage)
    }

    fn discrete(&self) -> bool {
        self.discrete
    }

    fn reset_discrete(&self) {
        self.discrete_interval.set(None);
    }

    fn unchanged_discrete(&self, time: f64) -> bool {
        if !self.discrete {
            return false;
        }
        let time = self.local_time(time);
        let Some(track) = self.track.as_deref() else {
            return false;
        };
        let upper = track.keys.partition_point(|key| key.time <= time);
        self.discrete_interval.get() == Some(upper)
    }

    fn mark_discrete(&self, time: f64) {
        if !self.discrete {
            return;
        }
        let time = self.local_time(time);
        let Some(track) = self.track.as_deref() else {
            self.reset_discrete();
            return;
        };
        self.discrete_interval
            .set(Some(track.keys.partition_point(|key| key.time <= time)));
    }

    fn suspend_track(&mut self) {
        self.track = None;
        self.reference = None;
        *self.segment.get_mut() = None;
    }

    #[cfg(feature = "instrumentation")]
    fn segment_bytes(&self) -> usize {
        if self.cache_segment {
            std::mem::size_of::<AnimationSampleSegment<T>>()
        } else {
            0
        }
    }

    fn resolve_track(&mut self, clip: &AnimationClip) -> Result<(), ErrorReason> {
        if self.track.is_none() {
            let track = clip
                .shared_track::<T>(self.description.track as usize)
                .ok_or(ErrorReason::InvalidField)?;
            self.cache_segment = matches!(track.value_kind(), 1 | 8);
            if self.contributes {
                let reference = if self.description.additive {
                    f64::from(self.description.reference_time)
                } else {
                    0.0
                };
                self.reference = Some(track.sample(reference).into_value());
            }
            self.track = Some(track);
        }
        Ok(())
    }

    fn sample(&self, time: f64) -> AnimationValue {
        let time = self.local_time(time);
        let track = self.track.as_deref().expect("prepared animation driver");
        if self.cache_segment {
            track.sample_segment(time, &mut self.segment.borrow_mut())
        } else {
            track.sample_cached(time, &self.interval)
        }
        .into_value()
    }

    fn contribution(&self, time: f64) -> Result<AnimationValue, ErrorReason> {
        let reference = self.reference.as_ref().ok_or(ErrorReason::InvalidField)?;
        contribution::delta(
            &self.sample(time),
            reference,
            f64::from(self.description.weight),
        )
    }

    fn bound_pose_track(&self) -> &AnimationTrack<Vec<crate::components::Transform>> {
        (self.track.as_deref().expect("prepared animation driver") as &dyn Any)
            .downcast_ref()
            .expect("bound joint driver")
    }

    fn runtime_target(&self) -> &AnimationRuntimeTarget {
        &self.runtime_target
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn skeleton_source(&self) -> Option<AssetKey> {
        match &self.runtime_target {
            AnimationRuntimeTarget::JointLocal {
                source,
                ..
            } => Some(*source),
            AnimationRuntimeTarget::Property => None,
        }
    }
}

impl<T: AnimationSample> AnimationDriver<T> {
    fn local_time(&self, time: f64) -> f64 {
        if self.description.repeat {
            time.rem_euclid(self.duration)
        } else {
            time
        }
    }
}

/// Bind a typed driver. `template` is a value of the target's type; a driver of a
/// linear or rotation type contributes, any other writes its samples.
pub(in crate::world) fn make_driver(
    description: AnimationDriverDescription,
    incarnation: u64,
    resolved_property: AnimationTrackTarget,
    clip: AssetKey,
    duration: f64,
    template: AnimationValue,
    skeleton_source: Option<AssetKey>,
) -> Result<Box<dyn AnimationDriverBinding>, ErrorReason> {
    let contributes = contribution::contributes(&template);
    // Discrete resource fields write only when the selected key changes.
    // Pose-source writes rebase unkeyed joints at their declaration-order
    // position, so Skeleton drivers keep the skeletal evaluator's staging contract.
    let skeleton = resolved_property.component() == Some(ComponentValue::SKELETON);
    let discrete = !contributes
        && !skeleton
        && description.weight == 1.0
        && resolved_property
            .property()
            .is_some_and(|property| property.offsets.len() == 1)
        && matches!(
            template,
            AnimationValue::Field(
                crate::components::schema::FieldValue::String(_)
                    | crate::components::schema::FieldValue::Bytes(_)
                    | crate::components::schema::FieldValue::Entity(_)
                    | crate::components::schema::FieldValue::Dynamic(crate::DynamicValue::Asset(_))
            )
        );
    macro_rules! typed {
        ($type:ty) => {{
            <$type>::from_value(template.clone())?;
            Box::new(AnimationDriver::<$type> {
                identity: AnimationTargetIdentity {
                    entity: description.target,
                    incarnation,
                    property: resolved_property.clone(),
                },
                runtime_target: AnimationRuntimeTarget::bind(
                    &description.property,
                    skeleton_source,
                )?,
                description,
                clip,
                template: template.clone(),
                contributes,
                interval: std::cell::Cell::new(0),
                duration,
                track: None,
                reference: None,
                segment: std::cell::RefCell::new(None),
                cache_segment: false,
                discrete,
                discrete_interval: std::cell::Cell::new(None),
            }) as Box<dyn AnimationDriverBinding>
        }};
    }

    Ok(match template.kind() {
        11 => typed!(crate::DynamicValue),
        1 => typed!(f32),
        2 => typed!(EntityId),
        3 => typed!(u32),
        4 => typed!(u64),
        5 => typed!(std::sync::Arc<str>),
        6 => typed!(Vec<u8>),
        7 => typed!(bool),
        8 => typed!([f32; 4]),
        9 => typed!(Vec<crate::components::Transform>),
        _ => return Err(ErrorReason::InvalidField),
    })
}
