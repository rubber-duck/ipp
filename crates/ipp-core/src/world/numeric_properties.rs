//! Independent fixed numeric properties shared by compiled animation and constraints.

use crate::{ComponentValue, components::*};
use std::mem::offset_of;

#[derive(Clone, Copy, Debug)]
pub(in crate::world) enum NumericPropertyRange {
    Finite,
    Nonnegative,
    Positive,
    Unit,
    Angle,
    CameraAngle,
    ShadowBias,
}

impl NumericPropertyRange {
    pub(in crate::world) fn contains(self, value: f32) -> bool {
        value.is_finite()
            && match self {
                Self::Finite => true,
                Self::Nonnegative => value >= 0.0,
                Self::Positive => value > 0.0,
                Self::Unit => (0.0..=1.0).contains(&value),
                Self::Angle => (0.0..=std::f32::consts::PI).contains(&value),
                Self::CameraAngle => (0.001..=std::f32::consts::PI - 0.001).contains(&value),
                Self::ShadowBias => (0.0..=0.05).contains(&value),
            }
    }
}

/// These fields cannot change a resource, relation, descriptor or simulation epoch.
/// Coupled values and resource selections must use their owning operator instead.
pub(in crate::world) fn range(component: u16, offset: u32) -> Option<NumericPropertyRange> {
    use NumericPropertyRange::*;

    macro_rules! fields {
        ($ty:ty; $($field:ident),+) => {
            [$(offset_of!($ty, $field) as u32),+].contains(&offset)
        };
    }

    match component {
        ComponentValue::LINEAR_DRIVER if fields!(LinearDriver; scale, bias) => Some(Finite),
        ComponentValue::BOUNDING_GEOMETRY => {
            if fields!(BoundingGeometry; stroke) {
                Some(Positive)
            } else if fields!(BoundingGeometry; r, g, b) {
                Some(Unit)
            } else {
                None
            }
        }
        ComponentValue::PICKING_GEOMETRY => {
            if fields!(PickingGeometry; stroke) {
                Some(Positive)
            } else if fields!(PickingGeometry; r, g, b) {
                Some(Unit)
            } else {
                None
            }
        }
        ComponentValue::MESH_POSE if fields!(MeshPose; weight) => Some(Unit),
        ComponentValue::PARTICLE_EMITTER => {
            if fields!(ParticleEmitter; rate, duration, delay, extent_x, extent_y, extent_z,
                speed, rotation_random, drag)
            {
                Some(Nonnegative)
            } else if fields!(ParticleEmitter; lifetime, size) {
                Some(Positive)
            } else if fields!(ParticleEmitter; lifetime_random, speed_random, size_random) {
                Some(Unit)
            } else if fields!(ParticleEmitter; spread) {
                Some(Angle)
            } else if fields!(ParticleEmitter; spin, acceleration_x, acceleration_y, acceleration_z)
            {
                Some(Finite)
            } else {
                None
            }
        }
        ComponentValue::PARTICLE_SPRITE => {
            if fields!(ParticleSprite; r, g, b, opacity, end_opacity) {
                Some(Unit)
            } else if fields!(ParticleSprite; end_size) {
                Some(Nonnegative)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Fixed f32 fields whose writes cannot change resource/relationship ownership
/// or violate coupled invariants. Quaternion lanes and camera clip planes are
/// deliberately absent; their owning combined operators validate those values.
pub(in crate::world) fn fixed_range(component: u16, offset: u32) -> Option<NumericPropertyRange> {
    use NumericPropertyRange::*;

    if let Some(range) = range(component, offset) {
        return Some(range);
    }

    macro_rules! fields {
        ($ty:ty; $($field:ident),+) => {
            [$(offset_of!($ty, $field) as u32),+].contains(&offset)
        };
    }

    match component {
        ComponentValue::TRANSFORM if fields!(Transform; sx, sy, sz) => Some(Positive),
        ComponentValue::TRANSFORM if fields!(Transform; x, y, z) => Some(Finite),
        ComponentValue::UNLIT_MATERIAL if fields!(UnlitMaterial; r, g, b) => Some(Unit),
        ComponentValue::PBR_MATERIAL if fields!(PbrMaterial; r, g, b, metallic, roughness) => {
            Some(Unit)
        }
        ComponentValue::LIGHT if fields!(Light; r, g, b) => Some(Unit),
        ComponentValue::LIGHT if fields!(Light; intensity, shadow_radius) => Some(Nonnegative),
        ComponentValue::LIGHT if fields!(Light; shadow_bias) => Some(ShadowBias),
        ComponentValue::CAMERA if fields!(Camera; fov_y) => Some(CameraAngle),
        ComponentValue::CAMERA if fields!(Camera; ortho_height) => Some(Positive),
        ComponentValue::SCALAR if fields!(Scalar; value) => Some(Finite),
        _ => None,
    }
}
