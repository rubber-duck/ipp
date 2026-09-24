//! Typed per-property restoration and immutable resource-index bindings.

use super::*;
use crate::{ComponentValue, services::asset_management::AssetKey};
use std::{any::Any, fmt::Debug};

/// Stable lifetime identity and exact property coverage for baseline inheritance.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::world) struct AnimationTargetIdentity {
    pub entity: EntityId,
    pub incarnation: u64,
    pub property: AnimationTrackTarget,
}

/// Resolved runtime access is distinct from the serializable client declaration.
/// Internal targets never call generated component fields or field setters.
#[derive(Debug)]
#[cfg(feature = "skeletal-animation")]
pub(in crate::world) enum AnimationRuntimeTarget {
    Property,
    #[cfg(feature = "skeletal-animation")]
    JointLocal {
        source: AssetKey,
        joints: Box<[u32]>,
    },
}

#[cfg(feature = "skeletal-animation")]
impl AnimationRuntimeTarget {
    fn bind(
        property: &AnimationTrackTarget,
        #[cfg(feature = "skeletal-animation")] source: Option<AssetKey>,
    ) -> Result<Self, ErrorReason> {
        match property {
            AnimationTrackTarget::DynamicProperty {
                ..
            }
            | AnimationTrackTarget::AnimationProperty(_) => Ok(Self::Property),
            #[cfg(feature = "skeletal-animation")]
            AnimationTrackTarget::Joints(joints) => Ok(Self::JointLocal {
                source: source.ok_or(ErrorReason::InvalidAsset)?,
                joints: joints.clone().into_boxed_slice(),
            }),
        }
    }

    #[cfg(feature = "skeletal-animation")]
    pub(in crate::world) fn write_joints(
        &self,
        storage: &mut crate::components::registry::ComponentStorage,
        entity: EntityId,
        value: AnimationValue,
    ) -> Result<(), ErrorReason> {
        let (
            Self::JointLocal {
                source,
                joints,
            },
            AnimationValue::Pose(values),
        ) = (self, value)
        else {
            return Err(ErrorReason::InvalidField);
        };
        let _ = (source, joints);
        self.write_joint_slice(storage, entity, &values)
    }

    pub(in crate::world) fn write_joint_slice(
        &self,
        storage: &mut crate::components::registry::ComponentStorage,
        entity: EntityId,
        values: &[crate::components::Transform],
    ) -> Result<(), ErrorReason> {
        let Self::JointLocal {
            source,
            joints,
        } = self
        else {
            return Err(ErrorReason::InvalidField);
        };
        if joints.len() != values.len() {
            return Err(ErrorReason::InvalidField);
        }
        for value in values {
            crate::components::schema::ComponentLifecycle::validate(value)?;
        }
        let pose = storage
            .skeleton_mut(entity.index() as usize)
            .and_then(|value| value.runtime.pose.as_mut())
            .filter(|pose| pose.valid && pose.source == *source)
            .ok_or(ErrorReason::MissingComponent)?;
        if joints
            .last()
            .is_some_and(|&joint| joint as usize >= pose.local.len())
        {
            return Err(ErrorReason::InvalidField);
        }
        for (&joint, &value) in joints.iter().zip(values) {
            *pose
                .local
                .get_mut(joint as usize)
                .ok_or(ErrorReason::InvalidField)? = value;
        }
        Ok(())
    }
}

/// A typed curve binding and its sparse restoration value.
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
    pub(in crate::world) original_value: T,
    interval: std::cell::Cell<usize>,
    duration: f64,
    track: Option<std::sync::Arc<AnimationTrack<T>>>,
    segment: std::cell::RefCell<Option<super::clip::AnimationSampleSegment<T>>>,
    cache_segment: bool,
    pub(super) destination: Option<crate::world::component_binding::ComponentBinding<T>>,
    transition_destination: Option<crate::world::component_binding::ComponentBinding<T>>,
    dynamic_destination: Option<DynamicValueDestination>,
    transition_dynamic_destination: Option<DynamicValueDestination>,
    discrete: bool,
    retain_discrete: bool,
    discrete_interval: std::cell::Cell<Option<usize>>,
    #[cfg(feature = "skeletal-animation")]
    pub(in crate::world) runtime_target: AnimationRuntimeTarget,
}

/// Compiled destination of one dynamic value: a named dynamic property kept by
/// its validated descriptor, or a row property kept by its offset.
#[derive(Clone, Copy, Debug)]
pub(in crate::world) enum DynamicValueDestination {
    CustomMaterial(
        crate::world::component_binding::ComponentBinding<crate::components::CustomMaterial>,
        crate::components::dynamic_properties::DynamicPropertyDescriptor,
    ),
    #[cfg(feature = "surfaces")]
    Surface(
        crate::world::component_binding::ComponentBinding<crate::components::Surface>,
        crate::components::dynamic_properties::DynamicPropertyDescriptor,
    ),
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
        let index = entity.index() as usize;
        if crate::components::rows::row_region(key).is_some() {
            return super::row_property_destination::RowPropertyDestination::bind(
                storage, entity, component, key,
            )
            .map(Self::Row);
        }

        let numeric =
            |descriptor: &crate::components::dynamic_properties::DynamicPropertyDescriptor| {
                descriptor.kind != crate::DynamicPropertyKind::Asset
            };
        // SAFETY: each pointer originates from the stable occupied cell whose
        // descriptor is captured with it; the caller's contract above bounds
        // their lifetime, and writes borrow the owning storage exclusively.
        unsafe {
            match component {
                ComponentValue::CUSTOM_MATERIAL => Some(Self::CustomMaterial(
                    crate::world::component_binding::ComponentBinding::new(
                        storage.custom_material_ptr(index)?,
                    ),
                    storage
                        .custom_material(index)?
                        .properties
                        .descriptor(key)
                        .filter(numeric)?,
                )),
                #[cfg(feature = "surfaces")]
                ComponentValue::SURFACE => Some(Self::Surface(
                    crate::world::component_binding::ComponentBinding::new(
                        storage.surface_ptr(index)?,
                    ),
                    storage
                        .surface(index)?
                        .properties
                        .descriptor(key)
                        .filter(numeric)?,
                )),
                _ => None,
            }
        }
    }

    fn write(
        self,
        storage: &mut crate::components::registry::ComponentStorage,
        value: crate::DynamicValue,
    ) -> Result<(), ErrorReason> {
        match self {
            Self::CustomMaterial(binding, descriptor) => binding
                .get_mut(storage)
                .properties
                .set_descriptor(descriptor, value),
            #[cfg(feature = "surfaces")]
            Self::Surface(binding, descriptor) => binding
                .get_mut(storage)
                .properties
                .set_descriptor(descriptor, value),
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

    fn bind_numeric(&mut self, storage: &crate::components::registry::ComponentStorage);

    fn has_numeric_binding(&self) -> bool;

    fn clear_numeric_binding(&mut self);

    fn bind_transition_output(&mut self, storage: &crate::components::registry::ComponentStorage);

    fn transition_output(&self) -> Option<AnimationTransitionOutput>;

    fn restore_numeric(&self, storage: &mut crate::components::registry::ComponentStorage) -> bool;

    fn sample_numeric(
        &self,
        time: f64,
        storage: &mut crate::components::registry::ComponentStorage,
    ) -> bool;

    fn discrete(&self) -> bool;

    fn retain_discrete(&self) -> bool;

    fn set_discrete_retention(&mut self, exclusive: bool);

    fn reset_discrete(&self);

    fn unchanged_discrete(&self, time: f64) -> bool;

    fn mark_discrete(&self, time: f64);

    fn original(&self) -> AnimationValue;

    fn suspend_track(&mut self);

    #[cfg(feature = "profiling")]
    fn segment_bytes(&self) -> usize;

    fn resolve_track(&mut self, clip: &AnimationClip) -> Result<(), ErrorReason>;

    fn sample_bound(
        &self,
        time: f64,
        current: AnimationValue,
    ) -> Result<AnimationValue, ErrorReason>;

    #[cfg(feature = "skeletal-animation")]
    fn bound_pose_track(&self) -> &AnimationTrack<Vec<crate::components::Transform>>;

    #[cfg(feature = "skeletal-animation")]
    fn joint_original(&self) -> Option<&[crate::components::Transform]>;

    #[cfg(feature = "skeletal-animation")]
    fn joint_original_mut(&mut self) -> Option<(&[u32], &mut Vec<crate::components::Transform>)>;

    fn refresh_original(&mut self, value: AnimationValue) -> Result<(), ErrorReason>;

    fn restore(&self, component: &mut ComponentValue) -> Result<(), ErrorReason>;

    #[cfg(feature = "skeletal-animation")]
    fn runtime_target(&self) -> &AnimationRuntimeTarget;

    fn as_any(&self) -> &dyn Any;

    #[cfg(feature = "skeletal-animation")]
    fn skeleton_source(&self) -> Option<AssetKey>;
}

#[derive(Clone, Debug)]
pub(in crate::world) enum AnimationTransitionOutput {
    F32(
        crate::world::component_binding::ComponentBinding<f32>,
        AnimationTargetIdentity,
    ),
    Rotation(
        crate::world::component_binding::ComponentBinding<[f32; 4]>,
        AnimationTargetIdentity,
    ),
    Dynamic(DynamicValueDestination, AnimationTargetIdentity),
}

impl AnimationTransitionOutput {
    pub(super) fn validate(&self, value: &AnimationValue) -> Result<(), ErrorReason> {
        let identity = match self {
            Self::F32(_, identity) | Self::Rotation(_, identity) | Self::Dynamic(_, identity) => {
                identity
            }
        };
        validate_transition_value(identity, value)
    }

    pub(super) fn write(
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

pub(super) fn bind_frozen_transition_output(
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

pub(super) fn frozen_transition_target_supported(
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
                        target.component(),
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
        #[cfg(feature = "skeletal-animation")]
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
            #[cfg(feature = "surfaces")]
            if let AnimationTrackTarget::DynamicProperty {
                component,
                name,
            } = &identity.property
                && *component == ComponentValue::SURFACE
            {
                crate::systems::surface::validate_animation_property(name, value)?;
            }
            // Row targets are checked before any channel publishes, so a
            // rejected value leaves every transition output unchanged.
            #[cfg(feature = "gui")]
            if let Some(property) = identity.property.property()
                && property.component == ComponentValue::GUI_ROOT
                && let [offset] = property.offsets.as_slice()
                && crate::components::rows::row_region(*offset).is_some()
            {
                if !crate::systems::gui::GuiRoot::numeric_animatable(*offset) {
                    return Err(ErrorReason::InvalidField);
                }
                crate::systems::gui::GuiRoot::validate_row_value(*offset, value)?;
            }
        }
        AnimationValue::Rotation(value) if value.iter().any(|value| !value.is_finite()) => {
            return Err(ErrorReason::InvalidValue);
        }
        _ => {}
    }
    Ok(())
}

fn transition_field_valid(component: u16, offset: u32, value: f32) -> bool {
    use crate::components::*;
    use std::mem::offset_of;
    if let Some(range) = super::numeric_fields::range(component, offset) {
        return range.contains(value);
    }
    macro_rules! fields {
        ($ty:ty; $($field:ident),+) => { [$(offset_of!($ty, $field) as u32),+].contains(&offset) };
    }
    match component {
        ComponentValue::TRANSFORM if fields!(Transform; sx, sy, sz) => value > 0.0,
        ComponentValue::TRANSFORM if fields!(Transform; x, y, z) => true,
        ComponentValue::UNLIT_MATERIAL if fields!(UnlitMaterial; r, g, b) => {
            (0.0..=1.0).contains(&value)
        }
        ComponentValue::PBR_MATERIAL if fields!(PbrMaterial; r, g, b, metallic, roughness) => {
            (0.0..=1.0).contains(&value)
        }
        ComponentValue::LIGHT if fields!(Light; r, g, b) => (0.0..=1.0).contains(&value),
        ComponentValue::LIGHT if fields!(Light; intensity, shadow_radius) => value >= 0.0,
        ComponentValue::LIGHT if fields!(Light; shadow_bias) => (0.0..=0.05).contains(&value),
        ComponentValue::CAMERA if fields!(Camera; fov_y) => {
            (0.001..=std::f32::consts::PI - 0.001).contains(&value)
        }
        ComponentValue::CAMERA if fields!(Camera; ortho_height) => value > 0.0,
        ComponentValue::SCALAR => true,
        _ => false,
    }
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

    fn bind_numeric(&mut self, storage: &crate::components::registry::ComponentStorage) {
        self.destination = super::numeric_binding::bind(self, storage);
        self.transition_destination = super::numeric_binding::bind_transition(self, storage);
        self.dynamic_destination = None;
        let property = self.identity.property.property();
        self.discrete = self.description.weight == 1.0
            && !self.description.additive
            && property.is_some_and(|p| p.offsets.len() == 1)
            && matches!(
                self.original(),
                AnimationValue::Field(
                    crate::components::schema::FieldValue::String(_)
                        | crate::components::schema::FieldValue::Bytes(_)
                        | crate::components::schema::FieldValue::Entity(_)
                        | crate::components::schema::FieldValue::Dynamic(
                            crate::DynamicValue::Asset(_)
                        )
                )
            );
        #[cfg(feature = "skeletal-animation")]
        if property.is_some_and(|p| p.component == ComponentValue::SKELETON) {
            // Pose-source writes rebase unkeyed joints at their declaration-order
            // position, so they keep the skeletal evaluator's staging contract.
            self.discrete = false;
        }
        self.reset_discrete();
        if self.description.weight == 1.0
            && !self.description.additive
            && std::any::TypeId::of::<T>() == std::any::TypeId::of::<crate::DynamicValue>()
            && let Some(property) = property
            && let [key] = property.offsets.as_slice()
        {
            // SAFETY: Animation invalidates departing property identities, rows and
            // component incarnations before reuse. Stable component pointers own
            // their buffers; only validated keys and offsets are retained across
            // buffer or table reallocation.
            self.dynamic_destination = unsafe {
                DynamicValueDestination::bind(
                    storage,
                    self.identity.entity,
                    property.component,
                    *key,
                )
            };
        }
    }

    fn has_numeric_binding(&self) -> bool {
        self.destination.is_some() || self.dynamic_destination.is_some()
    }

    fn clear_numeric_binding(&mut self) {
        self.destination = None;
        self.transition_destination = None;
        self.dynamic_destination = None;
        self.transition_dynamic_destination = None;
    }

    fn bind_transition_output(&mut self, storage: &crate::components::registry::ComponentStorage) {
        self.transition_destination = super::numeric_binding::bind_transition(self, storage);
        self.transition_dynamic_destination = None;
        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<crate::DynamicValue>()
            && let Some(property) = self.identity.property.property()
            && let [key] = property.offsets.as_slice()
        {
            // SAFETY: the retained binding points into stable occupied component storage. World
            // lifecycle hooks invalidate the controller before slot or row reuse, and animation
            // owns the only mutable access while publishing this output.
            self.transition_dynamic_destination = unsafe {
                DynamicValueDestination::bind(
                    storage,
                    self.identity.entity,
                    property.component,
                    *key,
                )
            };
        }
    }

    fn transition_output(&self) -> Option<AnimationTransitionOutput> {
        if let Some(destination) = self.transition_dynamic_destination {
            return Some(AnimationTransitionOutput::Dynamic(
                destination,
                self.identity.clone(),
            ));
        }
        let destination = self.transition_destination?;
        // SAFETY: bind_transition established the concrete track/sample type. The
        // TypeId check preserves alignment and representation, and animation drops
        // this binding synchronously before target incarnation replacement. Frame
        // evaluation holds exclusive access to the owning component storage.
        unsafe {
            if std::any::TypeId::of::<T>() == std::any::TypeId::of::<f32>() {
                return Some(AnimationTransitionOutput::F32(
                    destination.cast(),
                    self.identity.clone(),
                ));
            }
            if std::any::TypeId::of::<T>() == std::any::TypeId::of::<[f32; 4]>() {
                return Some(AnimationTransitionOutput::Rotation(
                    destination.cast(),
                    self.identity.clone(),
                ));
            }
        }
        None
    }

    fn restore_numeric(&self, storage: &mut crate::components::registry::ComponentStorage) -> bool {
        if let Some(destination) = self.dynamic_destination {
            let AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value)) =
                self.original()
            else {
                unreachable!()
            };
            // A rejected write changes nothing; the general path reports it.
            return destination.write(storage, value).is_ok();
        }
        let Some(destination) = self.destination else {
            return false;
        };
        *destination.get_mut(storage) = self.original_value.clone();
        true
    }

    fn sample_numeric(
        &self,
        time: f64,
        storage: &mut crate::components::registry::ComponentStorage,
    ) -> bool {
        if !self.has_numeric_binding() {
            return false;
        }
        let time = if self.description.repeat {
            time.rem_euclid(self.duration)
        } else {
            time
        };
        let track = self.track.as_deref().expect("compiled numeric track");
        let value = if self.cache_segment {
            track.sample_segment(time, &mut self.segment.borrow_mut())
        } else {
            track.sample_cached(time, &self.interval)
        };
        if let Some(destination) = self.destination {
            *destination.get_mut(storage) = value;
        } else if let Some(destination) = self.dynamic_destination {
            let AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value)) =
                value.into_value()
            else {
                unreachable!()
            };
            // A rejected row value changes nothing; the general path samples
            // again and reports the failure through ordinary validation.
            return destination.write(storage, value).is_ok();
        }
        true
    }

    fn discrete(&self) -> bool {
        self.discrete
    }

    fn retain_discrete(&self) -> bool {
        self.retain_discrete
    }

    fn set_discrete_retention(&mut self, exclusive: bool) {
        self.retain_discrete = self.discrete && exclusive;
    }

    fn reset_discrete(&self) {
        self.discrete_interval.set(None);
    }

    fn unchanged_discrete(&self, time: f64) -> bool {
        if !self.discrete {
            return false;
        }
        let time = if self.description.repeat {
            time.rem_euclid(self.duration)
        } else {
            time
        };
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
        let time = if self.description.repeat {
            time.rem_euclid(self.duration)
        } else {
            time
        };
        let Some(track) = self.track.as_deref() else {
            self.reset_discrete();
            return;
        };
        self.discrete_interval
            .set(Some(track.keys.partition_point(|key| key.time <= time)));
    }

    fn duration(&self) -> f64 {
        self.duration
    }

    fn suspend_track(&mut self) {
        self.track = None;
        *self.segment.get_mut() = None;
    }

    #[cfg(feature = "profiling")]
    fn segment_bytes(&self) -> usize {
        if self.cache_segment {
            std::mem::size_of::<super::clip::AnimationSampleSegment<T>>()
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
            self.track = Some(track);
        }
        Ok(())
    }

    fn sample_bound(
        &self,
        time: f64,
        current: AnimationValue,
    ) -> Result<AnimationValue, ErrorReason> {
        self.sample_track(
            self.track.as_deref().expect("prepared animation driver"),
            time,
            current,
        )
    }

    #[cfg(feature = "skeletal-animation")]
    fn bound_pose_track(&self) -> &AnimationTrack<Vec<crate::components::Transform>> {
        (self.track.as_deref().expect("prepared animation driver") as &dyn Any)
            .downcast_ref()
            .expect("bound joint driver")
    }

    fn original(&self) -> AnimationValue {
        self.original_value.to_value()
    }

    #[cfg(feature = "skeletal-animation")]
    fn joint_original(&self) -> Option<&[crate::components::Transform]> {
        (&self.original_value as &dyn Any)
            .downcast_ref::<Vec<crate::components::Transform>>()
            .map(Vec::as_slice)
    }

    #[cfg(feature = "skeletal-animation")]
    fn joint_original_mut(&mut self) -> Option<(&[u32], &mut Vec<crate::components::Transform>)> {
        let AnimationRuntimeTarget::JointLocal {
            joints,
            ..
        } = &self.runtime_target
        else {
            return None;
        };
        Some((
            joints,
            (&mut self.original_value as &mut dyn Any).downcast_mut()?,
        ))
    }

    fn refresh_original(&mut self, value: AnimationValue) -> Result<(), ErrorReason> {
        self.original_value = T::from_value(value)?;
        self.reset_discrete();
        Ok(())
    }

    fn restore(&self, component: &mut ComponentValue) -> Result<(), ErrorReason> {
        match &self.identity.property {
            AnimationTrackTarget::DynamicProperty {
                ..
            } => Err(ErrorReason::InvalidField),
            AnimationTrackTarget::AnimationProperty(property) => {
                self.original_value.to_value().write(property, component)
            }
            #[cfg(feature = "skeletal-animation")]
            AnimationTrackTarget::Joints(_) => Ok(()),
        }
    }

    #[cfg(feature = "skeletal-animation")]
    fn runtime_target(&self) -> &AnimationRuntimeTarget {
        &self.runtime_target
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    #[cfg(feature = "skeletal-animation")]
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
    pub(super) fn track_for_binding(&self) -> Option<&AnimationTrack<T>> {
        self.track.as_deref()
    }

    fn sample_track(
        &self,
        track: &AnimationTrack<T>,
        time: f64,
        current: AnimationValue,
    ) -> Result<AnimationValue, ErrorReason> {
        let time = if self.description.repeat {
            time.rem_euclid(self.duration)
        } else {
            time
        };
        let sample = if self.cache_segment {
            track.sample_segment(time, &mut self.segment.borrow_mut())
        } else {
            track.sample_cached(time, &self.interval)
        }
        .into_value();
        if !self.description.additive && self.description.weight == 1.0 {
            return Ok(sample);
        }
        let weight = f64::from(self.description.weight);
        if self.description.additive {
            additive(
                &current,
                &sample,
                &track
                    .sample(f64::from(self.description.reference_time))
                    .into_value(),
                weight,
            )
        } else {
            Ok(mix(&current, &sample, weight))
        }
    }
}

pub(in crate::world) fn make_driver(
    description: AnimationDriverDescription,
    incarnation: u64,
    resolved_property: AnimationTrackTarget,
    clip: AssetKey,
    duration: f64,
    original: AnimationValue,
    #[cfg(feature = "skeletal-animation")] skeleton_source: Option<AssetKey>,
) -> Result<Box<dyn AnimationDriverBinding>, ErrorReason> {
    macro_rules! typed {
        ($type:ty) => {
            Box::new(AnimationDriver::<$type> {
                identity: AnimationTargetIdentity {
                    entity: description.target,
                    incarnation,
                    property: resolved_property.clone(),
                },
                #[cfg(feature = "skeletal-animation")]
                runtime_target: AnimationRuntimeTarget::bind(
                    &description.property,
                    #[cfg(feature = "skeletal-animation")]
                    skeleton_source,
                )?,
                description,
                clip,
                original_value: <$type>::from_value(original)?,
                interval: std::cell::Cell::new(0),
                duration,
                track: None,
                segment: std::cell::RefCell::new(None),
                cache_segment: false,
                destination: None,
                transition_destination: None,
                dynamic_destination: None,
                transition_dynamic_destination: None,
                discrete: false,
                retain_discrete: false,
                discrete_interval: std::cell::Cell::new(None),
            }) as Box<dyn AnimationDriverBinding>
        };
    }

    Ok(match original.kind() {
        11 => typed!(crate::DynamicValue),
        1 => typed!(f32),
        2 => typed!(EntityId),
        3 => typed!(u32),
        4 => typed!(u64),
        5 => typed!(String),
        6 => typed!(Vec<u8>),
        7 => typed!(bool),
        8 => typed!([f32; 4]),
        #[cfg(feature = "skeletal-animation")]
        9 => typed!(Vec<crate::components::Transform>),
        _ => return Err(ErrorReason::InvalidField),
    })
}

impl AnimationSystemState {
    /// Whether [`Self::restore_underlying`] can change this component
    /// incarnation, so callers clone it only when restoration applies.
    #[cfg(feature = "gui")]
    pub(in crate::world) fn has_underlying(
        &self,
        entity: EntityId,
        incarnation: u64,
        component: u16,
    ) -> bool {
        let matches = |identity: &AnimationTargetIdentity| {
            identity.entity == entity
                && identity.incarnation == incarnation
                && identity.property.component() == component
        };

        self.pending_restorations
            .iter()
            .any(|(identity, _)| matches(identity) && identity.property.property().is_some())
            || self.controllers.values().any(|controller| {
                controller
                    .drivers
                    .iter()
                    .any(|driver| matches(driver.identity()))
            })
    }

    /// Restore only driver-controlled fields into a temporary component snapshot.
    pub(in crate::world) fn restore_underlying(
        &self,
        entity: EntityId,
        incarnation: u64,
        value: &mut ComponentValue,
    ) {
        for (identity, original) in &self.pending_restorations {
            if identity.entity == entity
                && identity.incarnation == incarnation
                && identity.property.component() == ComponentValue::type_id(value)
                && let Some(property) = identity.property.property()
            {
                let _ = original.clone().write(property, value);
            }
        }
        for controller in self.controllers.values() {
            for driver in &controller.drivers {
                let identity = driver.identity();
                if identity.entity == entity
                    && identity.incarnation == incarnation
                    && identity.property.component() == ComponentValue::type_id(value)
                {
                    let _ = driver.restore(value);
                }
            }
        }
    }
}
