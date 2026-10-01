//! Direct field bindings for crossfade outputs. No schema inspection at sampling.
//!
//! Only fixed `repr(C)` fields receive raw pointers here. Dynamic properties
//! and schema-row properties never do: their buffers and tables may relocate
//! while the component stays bound, so their destinations retain the stable
//! component cell plus a validated descriptor or row offset and resolve the
//! value on every write (see `driver::DynamicValueDestination`).

use crate::{
    ComponentValue,
    components::registry::ComponentStorage,
    components::{Camera, Light, PbrMaterial, Scalar, Transform, UnlitMaterial},
    world::component_binding::ComponentBinding,
};
use std::mem::offset_of;

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
        ComponentValue::MESH_POSE => storage.mesh_pose_ptr(index)?.cast::<u8>(),
        ComponentValue::PARTICLE_EMITTER => storage.particle_emitter_ptr(index)?.cast::<u8>(),
        ComponentValue::PARTICLE_SPRITE => storage.particle_sprite_ptr(index)?.cast::<u8>(),
        ComponentValue::PARTICLE_PLAYBACK => storage.particle_playback_ptr(index)?.cast::<u8>(),
        _ => return None,
    };
    // SAFETY: the property was previously accepted as a scalar transition target, so its repr(C)
    // offset identifies one f32 field. The stable component cell remains valid until lifecycle
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
        ComponentValue::MESH_POSE => super::numeric_fields::range(component, offset).is_some(),
        ComponentValue::PARTICLE_EMITTER | ComponentValue::PARTICLE_SPRITE => {
            super::numeric_fields::range(component, offset).is_some()
        }
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
    // SAFETY: Transform is repr(C), the checked quaternion fields are four adjacent f32 fields,
    // and occupied component storage stays stable until synchronous lifecycle invalidation.
    Some(unsafe { ComponentBinding::new(base.add(offset_of!(Transform, qx)).cast::<[f32; 4]>()) })
}
