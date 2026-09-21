//! Independent numeric lanes on components which also own resources or relations.

use crate::{ComponentValue, components::*};
use std::mem::offset_of;

#[derive(Clone, Copy)]
pub(super) enum AnimationNumericRange {
    Finite,
    #[cfg(feature = "particles")]
    Nonnegative,
    Positive,
    Unit,
    #[cfg(feature = "particles")]
    Angle,
}

impl AnimationNumericRange {
    pub(super) fn contains(self, value: f32) -> bool {
        value.is_finite()
            && match self {
                Self::Finite => true,
                #[cfg(feature = "particles")]
                Self::Nonnegative => value >= 0.0,
                Self::Positive => value > 0.0,
                Self::Unit => (0.0..=1.0).contains(&value),
                #[cfg(feature = "particles")]
                Self::Angle => (0.0..=std::f32::consts::PI).contains(&value),
            }
    }
}

/// These lanes cannot change a resource, relation, descriptor or simulation epoch.
/// Coupled values and resource selections must use their owning operator instead.
pub(super) fn range(component: u16, offset: u32) -> Option<AnimationNumericRange> {
    use AnimationNumericRange::*;

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
        #[cfg(feature = "mesh-poses")]
        ComponentValue::MESH_POSE if fields!(MeshPose; weight) => Some(Unit),
        #[cfg(feature = "particles")]
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
        #[cfg(feature = "particles")]
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
