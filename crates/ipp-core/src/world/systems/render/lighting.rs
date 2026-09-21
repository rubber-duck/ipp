//! Opaque direct-light material and punctual light declarations.

use crate::{ErrorReason, components::schema::ComponentLifecycle};
use ipp_schema_derive::SchemaComponent;

/// Opaque metallic-roughness material, using linear RGB and geometric flat normals.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct PbrMaterial {
    /// Linear base-color red in 0..=1.
    pub r: f32,
    /// Linear base-color green in 0..=1.
    pub g: f32,
    /// Linear base-color blue in 0..=1.
    pub b: f32,
    /// Metallic fraction in 0..=1 (zero is dielectric).
    pub metallic: f32,
    /// Perceptual roughness in 0..=1; shading clamps to 0.045 for stability.
    pub roughness: f32,
    /// Include this opaque mesh in the spotlight depth pass.
    pub cast_shadows: bool,
    /// Attenuate direct spot illumination using the depth map.
    pub receive_shadows: bool,
}

impl Default for PbrMaterial {
    fn default() -> Self {
        Self {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            metallic: 0.0,
            roughness: 0.5,
            cast_shadows: true,
            receive_shadows: true,
        }
    }
}

impl ComponentLifecycle for PbrMaterial {
    fn validate(&self) -> Result<(), ErrorReason> {
        if [self.r, self.g, self.b, self.metallic, self.roughness]
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// Direct light; effective Transform supplies position and local -Z direction.
/// Scale does not change intensity, direction, cone or range.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct Light {
    /// 0 = directional, 1 = point, 2 = spot.
    pub kind: u32,
    /// Linear red in 0..=1.
    pub r: f32,
    /// Linear green in 0..=1.
    pub g: f32,
    /// Linear blue in 0..=1.
    pub b: f32,
    /// Nonnegative illuminance (directional) or luminous intensity (point/spot).
    pub intensity: f32,
    /// Positive finite attenuation cutoff in metres for point/spot lights.
    pub range: f32,
    /// Spot inner half-angle in radians, 0 <= inner < outer.
    pub inner_cone: f32,
    /// Spot outer half-angle in radians, strictly between 0 and pi/2.
    pub outer_cone: f32,
    /// Request the optional single spotlight shadow map; invalid for other kinds.
    pub cast_shadows: bool,
    /// Positive spotlight shadow near distance, less than range.
    pub shadow_near: f32,
    /// Comparison bias in normalized depth, in 0..=0.05.
    pub shadow_bias: f32,
    /// Spotlight emitter radius in metres; zero retains the small antialiasing filter.
    pub shadow_radius: f32,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            kind: 0,
            r: 1.0,
            g: 1.0,
            b: 1.0,
            intensity: 1.0,
            range: 20.0,
            inner_cone: 0.35,
            outer_cone: 0.6,
            cast_shadows: false,
            shadow_near: 0.1,
            shadow_bias: 0.001,
            shadow_radius: 0.0,
        }
    }
}

impl ComponentLifecycle for Light {
    fn validate(&self) -> Result<(), ErrorReason> {
        if self.kind > 2
            || ![self.r, self.g, self.b]
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            || ![
                self.intensity,
                self.range,
                self.inner_cone,
                self.outer_cone,
                self.shadow_near,
                self.shadow_bias,
                self.shadow_radius,
            ]
            .iter()
            .all(|v| v.is_finite())
            || self.shadow_radius < 0.0
            || self.intensity < 0.0
            || self.range <= 0.0
            || self.inner_cone < 0.0
            || self.inner_cone >= self.outer_cone
            || self.outer_cone >= std::f32::consts::FRAC_PI_2
            || (self.cast_shadows && self.kind != 2)
            || self.shadow_near <= 0.0
            || self.shadow_near >= self.range
            || !(0.0..=0.05).contains(&self.shadow_bias)
        {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lighting_values_reject_nonfinite_invalid_cones_and_unsupported_shadow_kinds() {
        assert!(Light::default().validate().is_ok());
        assert!(PbrMaterial::default().validate().is_ok());
        assert!(
            Light {
                shadow_radius: 0.25,
                ..Light::default()
            }
            .validate()
            .is_ok()
        );
        for light in [
            Light {
                shadow_radius: -0.1,
                ..Light::default()
            },
            Light {
                shadow_radius: f32::NAN,
                ..Light::default()
            },
            Light {
                shadow_radius: f32::INFINITY,
                ..Light::default()
            },
            Light {
                kind: 3,
                ..Light::default()
            },
            Light {
                intensity: -1.0,
                ..Light::default()
            },
            Light {
                intensity: f32::INFINITY,
                ..Light::default()
            },
            Light {
                r: f32::NAN,
                ..Light::default()
            },
            Light {
                inner_cone: 0.6,
                ..Light::default()
            },
            Light {
                outer_cone: std::f32::consts::FRAC_PI_2,
                ..Light::default()
            },
            Light {
                cast_shadows: true,
                ..Light::default()
            },
            Light {
                shadow_near: 20.0,
                ..Light::default()
            },
            Light {
                shadow_bias: 0.051,
                ..Light::default()
            },
        ] {
            assert_eq!(light.validate(), Err(ErrorReason::InvalidValue));
        }
        for material in [
            PbrMaterial {
                metallic: 1.01,
                ..PbrMaterial::default()
            },
            PbrMaterial {
                roughness: -0.1,
                ..PbrMaterial::default()
            },
            PbrMaterial {
                r: f32::NAN,
                ..PbrMaterial::default()
            },
        ] {
            assert_eq!(material.validate(), Err(ErrorReason::InvalidValue));
        }
    }
}
