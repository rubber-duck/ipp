//! Bound publication for numeric operators that require a combined output guard.
//! Copy values and independent lanes may be staged, but publication never enters
//! generic mutation, preparation or lifecycle hooks or copies resource ownership.

use crate::{
    ComponentValue, EntityId, ErrorReason, components::registry::ComponentStorage,
    components::schema::ComponentLifecycle, components::*,
    world::component_binding::ComponentBinding,
};

#[derive(Clone, Copy, Debug)]
pub(super) enum AnimationNumericOutput {
    Scalar(ComponentBinding<Scalar>),
    Transform(ComponentBinding<Transform>),
    Unlit(ComponentBinding<UnlitMaterial>),
    Pbr(ComponentBinding<PbrMaterial>),
    Light(ComponentBinding<Light>),
    Camera(ComponentBinding<Camera>),
    CustomMaterial(ComponentBinding<CustomMaterial>),
    #[cfg(feature = "surfaces")]
    Surface(ComponentBinding<crate::components::Surface>),
    #[cfg(feature = "gui")]
    GuiRoot(ComponentBinding<crate::systems::gui::GuiRoot>),
    LinearDriver(ComponentBinding<LinearDriver>),
    BoundingGeometry(ComponentBinding<BoundingGeometry>),
    PickingGeometry(ComponentBinding<PickingGeometry>),
    #[cfg(feature = "mesh-poses")]
    MeshPose(ComponentBinding<MeshPose>),
    #[cfg(feature = "particles")]
    ParticleEmitter(ComponentBinding<ParticleEmitter>),
    #[cfg(feature = "particles")]
    ParticleSprite(ComponentBinding<ParticleSprite>),
    #[cfg(feature = "particles")]
    ParticlePlayback(ComponentBinding<ParticlePlayback>),
}

impl AnimationNumericOutput {
    pub(super) fn patch_only(component: u16) -> bool {
        match component {
            ComponentValue::CUSTOM_MATERIAL
            | ComponentValue::LINEAR_DRIVER
            | ComponentValue::BOUNDING_GEOMETRY
            | ComponentValue::PICKING_GEOMETRY => true,
            #[cfg(feature = "surfaces")]
            ComponentValue::SURFACE => true,
            #[cfg(feature = "gui")]
            ComponentValue::GUI_ROOT => true,
            #[cfg(feature = "mesh-poses")]
            ComponentValue::MESH_POSE => true,
            #[cfg(feature = "particles")]
            ComponentValue::PARTICLE_EMITTER
            | ComponentValue::PARTICLE_SPRITE
            | ComponentValue::PARTICLE_PLAYBACK => true,
            _ => false,
        }
    }

    pub(super) fn bind(
        storage: &ComponentStorage,
        (entity, component): (EntityId, u16),
    ) -> Option<Self> {
        let index = entity.index() as usize;
        // SAFETY: Every pointer originates from the typed stable cell. Animation
        // drops these outputs with its drivers before target incarnation destruction.
        // Publication exclusively borrows this owning storage and writes numeric data.
        unsafe {
            Some(match component {
                ComponentValue::SCALAR => {
                    Self::Scalar(ComponentBinding::new(storage.scalar_ptr(index)?))
                }
                ComponentValue::TRANSFORM => {
                    Self::Transform(ComponentBinding::new(storage.transform_ptr(index)?))
                }
                ComponentValue::UNLIT_MATERIAL => {
                    Self::Unlit(ComponentBinding::new(storage.unlit_material_ptr(index)?))
                }
                ComponentValue::PBR_MATERIAL => {
                    Self::Pbr(ComponentBinding::new(storage.pbr_material_ptr(index)?))
                }
                ComponentValue::LIGHT => {
                    Self::Light(ComponentBinding::new(storage.light_ptr(index)?))
                }
                ComponentValue::CAMERA => {
                    Self::Camera(ComponentBinding::new(storage.camera_ptr(index)?))
                }
                ComponentValue::CUSTOM_MATERIAL => {
                    Self::CustomMaterial(ComponentBinding::new(storage.custom_material_ptr(index)?))
                }
                #[cfg(feature = "surfaces")]
                ComponentValue::SURFACE => {
                    Self::Surface(ComponentBinding::new(storage.surface_ptr(index)?))
                }
                #[cfg(feature = "gui")]
                ComponentValue::GUI_ROOT => {
                    Self::GuiRoot(ComponentBinding::new(storage.gui_root_ptr(index)?))
                }
                ComponentValue::LINEAR_DRIVER => {
                    Self::LinearDriver(ComponentBinding::new(storage.linear_driver_ptr(index)?))
                }
                ComponentValue::BOUNDING_GEOMETRY => Self::BoundingGeometry(ComponentBinding::new(
                    storage.bounding_geometry_ptr(index)?,
                )),
                ComponentValue::PICKING_GEOMETRY => Self::PickingGeometry(ComponentBinding::new(
                    storage.picking_geometry_ptr(index)?,
                )),
                #[cfg(feature = "mesh-poses")]
                ComponentValue::MESH_POSE => {
                    Self::MeshPose(ComponentBinding::new(storage.mesh_pose_ptr(index)?))
                }
                #[cfg(feature = "particles")]
                ComponentValue::PARTICLE_EMITTER => Self::ParticleEmitter(ComponentBinding::new(
                    storage.particle_emitter_ptr(index)?,
                )),
                #[cfg(feature = "particles")]
                ComponentValue::PARTICLE_SPRITE => {
                    Self::ParticleSprite(ComponentBinding::new(storage.particle_sprite_ptr(index)?))
                }
                #[cfg(feature = "particles")]
                ComponentValue::PARTICLE_PLAYBACK => Self::ParticlePlayback(ComponentBinding::new(
                    storage.particle_playback_ptr(index)?,
                )),
                _ => return None,
            })
        }
    }

    pub(super) fn write_properties(
        self,
        key: (EntityId, u16),
        properties: &super::component_values::PropertyScratch,
        storage: &mut ComponentStorage,
    ) -> Result<(), ErrorReason> {
        use crate::components::schema::{FieldValue, SchemaComponent};
        let fields = || properties.iter().filter(|((target, _), _)| *target == key);
        macro_rules! write_numeric_fields {
            ($binding:expr) => {{
                // Guard the complete sampled patch before publishing any lane.
                // Resource fields and private evaluation buffers remain untouched.
                for ((_, offset), value) in fields() {
                    let range = super::numeric_fields::range(key.1, *offset)
                        .expect("bound independent numeric field");
                    if !matches!(value, FieldValue::F32(value) if range.contains(*value)) {
                        return Err(ErrorReason::InvalidValue);
                    }
                }
                let component = $binding.get_mut(storage);
                for ((_, offset), value) in fields() {
                    component.set_field(*offset, value.clone())
                        .expect("numeric field and type established at binding");
                }
            }};
        }
        match self {
            Self::LinearDriver(binding) => write_numeric_fields!(binding),
            Self::BoundingGeometry(binding) => write_numeric_fields!(binding),
            Self::PickingGeometry(binding) => write_numeric_fields!(binding),
            #[cfg(feature = "mesh-poses")]
            Self::MeshPose(binding) => write_numeric_fields!(binding),
            #[cfg(feature = "particles")]
            Self::ParticleEmitter(binding) => write_numeric_fields!(binding),
            #[cfg(feature = "particles")]
            Self::ParticleSprite(binding) => write_numeric_fields!(binding),
            #[cfg(feature = "particles")]
            Self::ParticlePlayback(binding) => {
                let mut time = binding.get(storage).time;
                for ((_, offset), value) in fields() {
                    debug_assert_eq!(*offset, std::mem::offset_of!(ParticlePlayback, time) as u32);
                    let FieldValue::F32(value) = value else {
                        unreachable!("bound particle clock type");
                    };
                    if !value.is_finite() {
                        return Err(ErrorReason::InvalidValue);
                    }
                    time = *value;
                }
                binding.get_mut(storage).time = time;
            }
            Self::CustomMaterial(binding) => {
                // Source strings, asset properties and descriptor ownership cannot
                // be targeted by this program. Only numeric policy guards remain.
                for ((_, offset), value) in fields() {
                    if *offset == std::mem::offset_of!(CustomMaterial, alpha_mode) as u32 {
                        if !matches!(value, FieldValue::U32(v) if *v <= 2) {
                            return Err(ErrorReason::InvalidValue);
                        }
                    } else if *offset == std::mem::offset_of!(CustomMaterial, alpha_cutoff) as u32
                        && !matches!(value, FieldValue::F32(v) if v.is_finite() && (0.0..=1.0).contains(v))
                    {
                        return Err(ErrorReason::InvalidValue);
                    }
                }
                let component = binding.get_mut(storage);
                for ((_, offset), value) in fields() {
                    if crate::components::dynamic_properties::is_dynamic_field(*offset) {
                        // The component stays bound while its numeric buffer may
                        // grow. Resolve its stable property identity against the
                        // current buffer; never retain a pointer across relocation.
                        component
                            .properties
                            .set_field(*offset, value.clone())
                            .map_err(|_| ErrorReason::InvalidField)?;
                    } else {
                        component
                            .set_field(*offset, value.clone())
                            .map_err(|_| ErrorReason::InvalidField)?;
                    }
                }
            }
            #[cfg(feature = "surfaces")]
            Self::Surface(binding) => {
                let component = binding.get_mut(storage);
                for ((_, offset), value) in fields() {
                    component
                        .properties
                        .set_field(*offset, value.clone())
                        .map_err(|_| ErrorReason::InvalidField)?;
                }
            }
            #[cfg(feature = "gui")]
            Self::GuiRoot(binding) => {
                let component = binding.get_mut(storage);
                for ((_, offset), value) in fields() {
                    component
                        .properties
                        .set_field(*offset, value.clone())
                        .map_err(|_| ErrorReason::InvalidField)?;
                }
            }
            _ => unreachable!("bound patch output"),
        }
        Ok(())
    }

    pub(super) fn write(
        self,
        value: ComponentValue,
        storage: &mut ComponentStorage,
    ) -> Result<(), ErrorReason> {
        macro_rules! write {
            ($binding:expr, $value:expr) => {{
                // This is the operator's numeric result guard (for example additive
                // overflow or near < far), not structural validation or preparation.
                $value.validate()?;
                *$binding.get_mut(storage) = $value;
            }};
        }
        match (self, value) {
            (Self::Scalar(binding), ComponentValue::Scalar(value)) => write!(binding, value),
            (Self::Transform(binding), ComponentValue::Transform(value)) => write!(binding, value),
            (Self::Unlit(binding), ComponentValue::UnlitMaterial(value)) => write!(binding, value),
            (Self::Pbr(binding), ComponentValue::PbrMaterial(value)) => write!(binding, value),
            (Self::Light(binding), ComponentValue::Light(value)) => write!(binding, value),
            (Self::Camera(binding), ComponentValue::Camera(value)) => write!(binding, value),
            _ => unreachable!("numeric output type resolved when the controller bound"),
        }
        Ok(())
    }
}
