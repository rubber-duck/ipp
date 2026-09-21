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
    pub(in crate::world) fn read_joints(
        &self,
        storage: &crate::components::registry::ComponentStorage,
        entity: EntityId,
    ) -> Result<AnimationValue, ErrorReason> {
        let Self::JointLocal {
            source,
            joints,
        } = self
        else {
            return Err(ErrorReason::InvalidField);
        };
        let pose = storage
            .skeleton(entity.index() as usize)
            .and_then(|value| value.runtime.pose.as_ref())
            .filter(|pose| pose.valid && pose.source == *source)
            .ok_or(ErrorReason::MissingComponent)?;
        Ok(AnimationValue::Pose(
            joints
                .iter()
                .map(|&joint| {
                    pose.local
                        .get(joint as usize)
                        .copied()
                        .ok_or(ErrorReason::InvalidField)
                })
                .collect::<Result<Vec<_>, _>>()?,
        ))
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
    track_copied: bool,
    segment: std::cell::RefCell<Option<super::clip::AnimationSampleSegment<T>>>,
    cache_segment: bool,
    pub(super) destination: Option<crate::world::component_binding::ComponentBinding<T>>,
    transition_destination: Option<crate::world::component_binding::ComponentBinding<T>>,
    dynamic_destination: Option<DynamicPropertyDestination>,
    transition_dynamic_destination: Option<DynamicPropertyDestination>,
    discrete: bool,
    retain_discrete: bool,
    discrete_interval: std::cell::Cell<Option<usize>>,
    #[cfg(feature = "skeletal-animation")]
    pub(in crate::world) runtime_target: AnimationRuntimeTarget,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::world) enum DynamicPropertyDestination {
    CustomMaterial(
        crate::world::component_binding::ComponentBinding<crate::components::CustomMaterial>,
        crate::components::dynamic_properties::DynamicPropertyDescriptor,
    ),
    #[cfg(feature = "surfaces")]
    Surface(
        crate::world::component_binding::ComponentBinding<crate::components::Surface>,
        crate::components::dynamic_properties::DynamicPropertyDescriptor,
    ),
    #[cfg(feature = "gui")]
    GuiRoot(
        crate::world::component_binding::ComponentBinding<crate::systems::gui::GuiRoot>,
        crate::components::dynamic_properties::DynamicPropertyDescriptor,
    ),
}

impl DynamicPropertyDestination {
    fn write(
        self,
        storage: &mut crate::components::registry::ComponentStorage,
        value: crate::DynamicValue,
    ) {
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
            #[cfg(feature = "gui")]
            Self::GuiRoot(binding, descriptor) => binding
                .get_mut(storage)
                .properties
                .set_descriptor(descriptor, value),
        }
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
    fn copied_track_bytes(&self) -> usize;

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

    fn sample(
        &self,
        clip: &AnimationClip,
        time: f64,
        current: AnimationValue,
    ) -> Result<AnimationValue, ErrorReason>;

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
    Dynamic(DynamicPropertyDestination, AnimationTargetIdentity),
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
                destination.write(storage, value);
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
            let index = identity.entity.index() as usize;
            // SAFETY: the descriptor and stable component pointer are captured together. World
            // lifecycle invalidation drops the transition before slot reuse, and publication
            // holds exclusive mutable access to component storage.
            let destination = if property.component == ComponentValue::CUSTOM_MATERIAL {
                let component = storage.custom_material(index)?;
                DynamicPropertyDestination::CustomMaterial(
                    // SAFETY: see the stable-cell and invalidation argument above.
                    unsafe {
                        crate::world::component_binding::ComponentBinding::new(
                            storage.custom_material_ptr(index)?,
                        )
                    },
                    component.properties.descriptor(key).filter(|descriptor| {
                        descriptor.kind != crate::DynamicPropertyKind::Asset
                    })?,
                )
            } else {
                match property.component {
                    #[cfg(feature = "surfaces")]
                    ComponentValue::SURFACE => {
                        let component = storage.surface(index)?;
                        DynamicPropertyDestination::Surface(
                            // SAFETY: see the stable-cell and invalidation argument above.
                            unsafe {
                                crate::world::component_binding::ComponentBinding::new(
                                    storage.surface_ptr(index)?,
                                )
                            },
                            component.properties.descriptor(key).filter(|descriptor| {
                                descriptor.kind != crate::DynamicPropertyKind::Asset
                            })?,
                        )
                    }
                    #[cfg(feature = "gui")]
                    ComponentValue::GUI_ROOT => {
                        let component = storage.gui_root(index)?;
                        DynamicPropertyDestination::GuiRoot(
                            // SAFETY: see the stable-cell and invalidation argument above.
                            unsafe {
                                crate::world::component_binding::ComponentBinding::new(
                                    storage.gui_root_ptr(index)?,
                                )
                            },
                            component.properties.descriptor(key).filter(|descriptor| {
                                descriptor.kind != crate::DynamicPropertyKind::Asset
                            })?,
                        )
                    }
                    _ => return None,
                }
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
            matches!(target, AnimationTrackTarget::DynamicProperty { .. })
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
            #[cfg(feature = "gui")]
            if let AnimationTrackTarget::DynamicProperty {
                component,
                name,
            } = &identity.property
                && *component == ComponentValue::GUI_ROOT
            {
                crate::systems::gui::validate_gui_property_value(name, value)?;
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
        self.discrete = crate::compiled_property_bindings_enabled()
            && self.description.weight == 1.0
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
        if crate::compiled_property_bindings_enabled()
            && crate::direct_numeric_updates_enabled()
            && self.description.weight == 1.0
            && !self.description.additive
            && std::any::TypeId::of::<T>() == std::any::TypeId::of::<crate::DynamicValue>()
            && let Some(property) = property
            && let [key] = property.offsets.as_slice()
        {
            let index = self.identity.entity.index() as usize;
            // SAFETY: Animation invalidates departing property identities and component
            // incarnations before reuse. Stable component pointers own their buffers;
            // only validated offsets are retained across buffer reallocation.
            unsafe {
                if property.component == ComponentValue::CUSTOM_MATERIAL
                    && let Some(material) = storage.custom_material(index)
                    && let Some(descriptor) = material
                        .properties
                        .descriptor(*key)
                        .filter(|d| d.kind != crate::DynamicPropertyKind::Asset)
                {
                    self.dynamic_destination = Some(DynamicPropertyDestination::CustomMaterial(
                        crate::world::component_binding::ComponentBinding::new(
                            storage.custom_material_ptr(index).unwrap(),
                        ),
                        descriptor,
                    ));
                }
                #[cfg(feature = "surfaces")]
                if property.component == ComponentValue::SURFACE
                    && let Some(surface) = storage.surface(index)
                    && let Some(descriptor) = surface
                        .properties
                        .descriptor(*key)
                        .filter(|d| d.kind != crate::DynamicPropertyKind::Asset)
                {
                    self.dynamic_destination = Some(DynamicPropertyDestination::Surface(
                        crate::world::component_binding::ComponentBinding::new(
                            storage.surface_ptr(index).unwrap(),
                        ),
                        descriptor,
                    ));
                }
                #[cfg(feature = "gui")]
                if property.component == ComponentValue::GUI_ROOT
                    && let Some(gui_root) = storage.gui_root(index)
                    && let Some(descriptor) = gui_root
                        .properties
                        .descriptor(*key)
                        .filter(|d| d.kind != crate::DynamicPropertyKind::Asset)
                {
                    self.dynamic_destination = Some(DynamicPropertyDestination::GuiRoot(
                        crate::world::component_binding::ComponentBinding::new(
                            storage.gui_root_ptr(index).unwrap(),
                        ),
                        descriptor,
                    ));
                }
            }
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
            let index = self.identity.entity.index() as usize;
            // SAFETY: the retained binding points into stable occupied component storage. World
            // lifecycle hooks invalidate the controller before slot reuse, and animation owns the
            // only mutable access while publishing this output.
            unsafe {
                if property.component == ComponentValue::CUSTOM_MATERIAL
                    && let Some(material) = storage.custom_material(index)
                    && let Some(descriptor) = material
                        .properties
                        .descriptor(*key)
                        .filter(|descriptor| descriptor.kind != crate::DynamicPropertyKind::Asset)
                {
                    self.transition_dynamic_destination =
                        Some(DynamicPropertyDestination::CustomMaterial(
                            crate::world::component_binding::ComponentBinding::new(
                                storage.custom_material_ptr(index).unwrap(),
                            ),
                            descriptor,
                        ));
                }
                #[cfg(feature = "surfaces")]
                if property.component == ComponentValue::SURFACE
                    && let Some(surface) = storage.surface(index)
                    && let Some(descriptor) = surface
                        .properties
                        .descriptor(*key)
                        .filter(|descriptor| descriptor.kind != crate::DynamicPropertyKind::Asset)
                {
                    self.transition_dynamic_destination =
                        Some(DynamicPropertyDestination::Surface(
                            crate::world::component_binding::ComponentBinding::new(
                                storage.surface_ptr(index).unwrap(),
                            ),
                            descriptor,
                        ));
                }
                #[cfg(feature = "gui")]
                if property.component == ComponentValue::GUI_ROOT
                    && let Some(gui_root) = storage.gui_root(index)
                    && let Some(descriptor) = gui_root
                        .properties
                        .descriptor(*key)
                        .filter(|descriptor| descriptor.kind != crate::DynamicPropertyKind::Asset)
                {
                    self.transition_dynamic_destination =
                        Some(DynamicPropertyDestination::GuiRoot(
                            crate::world::component_binding::ComponentBinding::new(
                                storage.gui_root_ptr(index).unwrap(),
                            ),
                            descriptor,
                        ));
                }
            }
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
            destination.write(storage, value);
            return true;
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
            destination.write(storage, value);
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
    fn copied_track_bytes(&self) -> usize {
        if self.track_copied {
            self.track
                .as_ref()
                .map_or(0, |track| track.resident_bytes())
        } else {
            0
        }
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
            self.track_copied = crate::animation_track_copy_enabled();
            self.cache_segment =
                crate::animation_segment_copy_enabled() && matches!(track.value_kind(), 1 | 8);
            self.track = Some(if self.track_copied {
                std::sync::Arc::new((*track).clone())
            } else {
                track
            });
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

    fn sample(
        &self,
        clip: &AnimationClip,
        time: f64,
        current: AnimationValue,
    ) -> Result<AnimationValue, ErrorReason> {
        let track = clip
            .typed_track::<T>(self.description.track as usize)
            .ok_or(ErrorReason::InvalidField)?;
        self.sample_track(track, time, current)
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
        } else if crate::animation_update_reuse_enabled() {
            track.sample_cached(time, &self.interval)
        } else {
            track.sample(time)
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
                track_copied: false,
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
