//! Bind-time proofs for direct numeric operators. No schema inspection at sampling.

use super::{AnimationDriver, AnimationInterpolation, AnimationSample, AnimationValue};
use crate::{
    ComponentValue,
    components::registry::ComponentStorage,
    components::{Camera, Light, PbrMaterial, Scalar, Transform, UnlitMaterial},
    world::component_binding::ComponentBinding,
};
use std::{any::TypeId, mem::offset_of};

pub(super) fn bind_frozen_f32(
    identity: &super::driver::AnimationTargetIdentity,
    storage: &ComponentStorage,
) -> Option<ComponentBinding<f32>> {
    let property = identity.property.property()?;
    let &[offset] = property.offsets.as_slice() else {
        return None;
    };
    if !frozen_f32_field_supported(property.component, offset) {
        return None;
    }
    let index = identity.entity.index() as usize;
    let base = match property.component {
        ComponentValue::SCALAR => storage.scalar_ptr(index)?.cast::<u8>(),
        ComponentValue::TRANSFORM => storage.transform_ptr(index)?.cast::<u8>(),
        ComponentValue::UNLIT_MATERIAL => storage.unlit_material_ptr(index)?.cast::<u8>(),
        ComponentValue::PBR_MATERIAL => storage.pbr_material_ptr(index)?.cast::<u8>(),
        ComponentValue::LIGHT => storage.light_ptr(index)?.cast::<u8>(),
        ComponentValue::CAMERA => storage.camera_ptr(index)?.cast::<u8>(),
        ComponentValue::LINEAR_DRIVER => storage.linear_driver_ptr(index)?.cast::<u8>(),
        ComponentValue::BOUNDING_GEOMETRY => storage.bounding_geometry_ptr(index)?.cast::<u8>(),
        ComponentValue::PICKING_GEOMETRY => storage.picking_geometry_ptr(index)?.cast::<u8>(),
        #[cfg(feature = "mesh-poses")]
        ComponentValue::MESH_POSE => storage.mesh_pose_ptr(index)?.cast::<u8>(),
        #[cfg(feature = "particles")]
        ComponentValue::PARTICLE_EMITTER => storage.particle_emitter_ptr(index)?.cast::<u8>(),
        #[cfg(feature = "particles")]
        ComponentValue::PARTICLE_SPRITE => storage.particle_sprite_ptr(index)?.cast::<u8>(),
        #[cfg(feature = "particles")]
        ComponentValue::PARTICLE_PLAYBACK => storage.particle_playback_ptr(index)?.cast::<u8>(),
        _ => return None,
    };
    // SAFETY: the property was previously accepted as a scalar transition target, so its repr(C)
    // offset identifies one f32 lane. The stable component cell remains valid until lifecycle
    // invalidation, and transition evaluation has exclusive storage access while writing.
    Some(unsafe { ComponentBinding::new(base.add(offset as usize).cast::<f32>()) })
}

pub(super) fn frozen_f32_field_supported(component: u16, offset: u32) -> bool {
    macro_rules! fields {
        ($ty:ty; $($field:ident),+) => {
            [$(offset_of!($ty, $field) as u32),+].contains(&offset)
        };
    }
    match component {
        ComponentValue::SCALAR => offset == offset_of!(Scalar, value) as u32,
        ComponentValue::TRANSFORM => {
            fields!(Transform; x, y, z, sx, sy, sz)
        }
        ComponentValue::UNLIT_MATERIAL => fields!(UnlitMaterial; r, g, b),
        ComponentValue::PBR_MATERIAL => {
            fields!(PbrMaterial; r, g, b, metallic, roughness)
        }
        ComponentValue::LIGHT => {
            fields!(Light; r, g, b, intensity, shadow_radius, shadow_bias)
        }
        ComponentValue::CAMERA => fields!(Camera; fov_y, ortho_height),
        ComponentValue::LINEAR_DRIVER
        | ComponentValue::BOUNDING_GEOMETRY
        | ComponentValue::PICKING_GEOMETRY => {
            super::numeric_fields::range(component, offset).is_some()
        }
        #[cfg(feature = "mesh-poses")]
        ComponentValue::MESH_POSE => super::numeric_fields::range(component, offset).is_some(),
        #[cfg(feature = "particles")]
        ComponentValue::PARTICLE_EMITTER | ComponentValue::PARTICLE_SPRITE => {
            super::numeric_fields::range(component, offset).is_some()
        }
        #[cfg(feature = "particles")]
        ComponentValue::PARTICLE_PLAYBACK => {
            offset == offset_of!(crate::components::ParticlePlayback, time) as u32
        }
        _ => false,
    }
}

pub(super) fn bind_frozen_rotation(
    identity: &super::driver::AnimationTargetIdentity,
    storage: &ComponentStorage,
) -> Option<ComponentBinding<[f32; 4]>> {
    let property = identity.property.property()?;
    if property.component != ComponentValue::TRANSFORM
        || property.offsets
            != [
                offset_of!(Transform, qx) as u32,
                offset_of!(Transform, qy) as u32,
                offset_of!(Transform, qz) as u32,
                offset_of!(Transform, qw) as u32,
            ]
    {
        return None;
    }
    let base = storage
        .transform_ptr(identity.entity.index() as usize)?
        .cast::<u8>();
    // SAFETY: Transform is repr(C), the checked quaternion fields are four adjacent f32 lanes,
    // and occupied component storage stays stable until synchronous lifecycle invalidation.
    Some(unsafe { ComponentBinding::new(base.add(offset_of!(Transform, qx)).cast::<[f32; 4]>()) })
}

pub(super) fn bind<T: AnimationSample>(
    driver: &AnimationDriver<T>,
    storage: &ComponentStorage,
) -> Option<ComponentBinding<T>> {
    if !crate::direct_numeric_updates_enabled()
        || driver.description.weight != 1.0
        || driver.description.additive
    {
        return None;
    }
    bind_transition(driver, storage)
}

pub(super) fn bind_transition<T: AnimationSample>(
    driver: &AnimationDriver<T>,
    storage: &ComponentStorage,
) -> Option<ComponentBinding<T>> {
    let property = driver.identity.property.property()?;
    let index = driver.identity.entity.index() as usize;
    let (base, offset) = match (property.component, property.offsets.as_slice()) {
        (ComponentValue::SCALAR, &[offset])
            if offset == offset_of!(Scalar, value) as u32
                && TypeId::of::<T>() == TypeId::of::<f32>() =>
        {
            (storage.scalar_ptr(index)?.cast::<u8>(), offset)
        }
        (ComponentValue::TRANSFORM, &[offset]) if TypeId::of::<T>() == TypeId::of::<f32>() => {
            let translation = [
                offset_of!(Transform, x),
                offset_of!(Transform, y),
                offset_of!(Transform, z),
            ];
            let scale = [
                offset_of!(Transform, sx),
                offset_of!(Transform, sy),
                offset_of!(Transform, sz),
            ];
            if scale.contains(&(offset as usize)) {
                let track = driver.track_for_binding()?;
                // Cubic interpolation stays in the convex hull of its handles.
                // Positive keys alone cannot prove positive scale for Bezier curves.
                let positive = |value: &T| matches!(value.to_value(), AnimationValue::Field(crate::components::schema::FieldValue::F32(v)) if v > 0.0);
                if !track.keys.iter().all(|key| {
                    positive(&key.value)
                        && match &key.interpolation {
                            AnimationInterpolation::Bezier {
                                value1,
                                value2,
                                ..
                            } => positive(value1) && positive(value2),
                            _ => true,
                        }
                }) {
                    return None;
                }
            } else if !translation.contains(&(offset as usize)) {
                return None;
            }
            (storage.transform_ptr(index)?.cast::<u8>(), offset)
        }
        (ComponentValue::TRANSFORM, offsets)
            if TypeId::of::<T>() == TypeId::of::<[f32; 4]>()
                && offsets
                    == [
                        offset_of!(Transform, qx) as u32,
                        offset_of!(Transform, qy) as u32,
                        offset_of!(Transform, qz) as u32,
                        offset_of!(Transform, qw) as u32,
                    ] =>
        {
            (
                storage.transform_ptr(index)?.cast::<u8>(),
                offset_of!(Transform, qx) as u32,
            )
        }
        (component, &[offset])
            if TypeId::of::<T>() == TypeId::of::<f32>()
                && super::numeric_fields::range(component, offset).is_some() =>
        {
            let range = super::numeric_fields::range(component, offset)?;
            let track = driver.track_for_binding()?;
            let allowed = |value: &T| matches!(value.to_value(), AnimationValue::Field(crate::components::schema::FieldValue::F32(value)) if range.contains(value));
            if !track.keys.iter().all(|key| {
                allowed(&key.value)
                    && match &key.interpolation {
                        AnimationInterpolation::Bezier {
                            value1,
                            value2,
                            ..
                        } => allowed(value1) && allowed(value2),
                        _ => true,
                    }
            }) {
                return None;
            }
            let pointer = match component {
                ComponentValue::LINEAR_DRIVER => storage.linear_driver_ptr(index)?.cast::<u8>(),
                ComponentValue::BOUNDING_GEOMETRY => {
                    storage.bounding_geometry_ptr(index)?.cast::<u8>()
                }
                ComponentValue::PICKING_GEOMETRY => {
                    storage.picking_geometry_ptr(index)?.cast::<u8>()
                }
                #[cfg(feature = "mesh-poses")]
                ComponentValue::MESH_POSE => storage.mesh_pose_ptr(index)?.cast::<u8>(),
                #[cfg(feature = "particles")]
                ComponentValue::PARTICLE_EMITTER => {
                    storage.particle_emitter_ptr(index)?.cast::<u8>()
                }
                #[cfg(feature = "particles")]
                ComponentValue::PARTICLE_SPRITE => storage.particle_sprite_ptr(index)?.cast::<u8>(),
                _ => unreachable!("independent numeric component"),
            };
            (pointer, offset)
        }
        (component, &[offset]) if TypeId::of::<T>() == TypeId::of::<f32>() => {
            let range = |low: f32, high: f32| {
                driver.track_for_binding().is_some_and(|track| track.keys.iter().all(|key| {
                    let allowed = |value: &T| matches!(value.to_value(), AnimationValue::Field(crate::components::schema::FieldValue::F32(v)) if v >= low && v <= high);
                    allowed(&key.value) && match &key.interpolation {
                        AnimationInterpolation::Bezier { value1, value2, .. } => allowed(value1) && allowed(value2),
                        _ => true,
                    }
                }))
            };
            macro_rules! fields {
                ($ty:ty; $($field:ident),+) => { [$(offset_of!($ty, $field) as u32),+].contains(&offset) };
            }
            let pointer = match component {
                ComponentValue::UNLIT_MATERIAL
                    if fields!(UnlitMaterial; r, g, b) && range(0.0, 1.0) =>
                {
                    storage.unlit_material_ptr(index)?.cast::<u8>()
                }
                ComponentValue::PBR_MATERIAL
                    if fields!(PbrMaterial; r, g, b, metallic, roughness) && range(0.0, 1.0) =>
                {
                    storage.pbr_material_ptr(index)?.cast::<u8>()
                }
                ComponentValue::LIGHT
                    if (fields!(Light; r, g, b) && range(0.0, 1.0))
                        || (fields!(Light; intensity, shadow_radius) && range(0.0, f32::MAX))
                        || (fields!(Light; shadow_bias) && range(0.0, 0.05)) =>
                {
                    storage.light_ptr(index)?.cast::<u8>()
                }
                ComponentValue::CAMERA
                    if (fields!(Camera; fov_y) && range(0.001, std::f32::consts::PI - 0.001))
                        || (fields!(Camera; ortho_height)
                            && range(f32::MIN_POSITIVE, f32::MAX)) =>
                {
                    storage.camera_ptr(index)?.cast::<u8>()
                }
                #[cfg(feature = "particles")]
                ComponentValue::PARTICLE_PLAYBACK
                    if offset == offset_of!(crate::components::ParticlePlayback, time) as u32
                        && range(0.0, f32::MAX) =>
                {
                    storage.particle_playback_ptr(index)?.cast::<u8>()
                }
                _ => return None,
            };
            (pointer, offset)
        }
        _ => return None,
    };
    // SAFETY: Exact type and repr(C) field offsets were checked above; quaternion
    // fields are four adjacent f32 lanes. The pointer originates from a stable cell,
    // never a temporary &mut. Animation's incarnation hooks drop every driver before
    // target removal/reuse, and all access borrows this World's component storage.
    Some(unsafe { ComponentBinding::new(base.add(offset as usize).cast::<T>()) })
}
