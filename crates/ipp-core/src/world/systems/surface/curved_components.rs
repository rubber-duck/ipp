//! Curved components use one signed curvature in inverse metres. Positive
//! curvature faces outside, negative faces inside, and zero is exactly flat.
//! Radius helpers derive abs(curvature)=1/radius; no second radius is stored.

use crate::{ErrorReason, components::schema::ComponentLifecycle};
use ipp_schema_derive::SchemaComponent;

macro_rules! curved_component {
    ($name:ident, $axes:expr, $description:literal) => {
        #[doc = $description]
        #[repr(C)]
        #[derive(Clone, Debug, PartialEq, SchemaComponent)]
        pub struct $name {
            /// Physical horizontal content extent in metres.
            pub width: f32,
            /// Physical vertical content extent in metres.
            pub height: f32,
            /// Signed inverse radius in metres^-1: outside positive, inside negative, zero flat.
            pub curvature: f32,
            /// Metres separating consecutive occupied ranks along front normals.
            pub layer_spacing: f32,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    width: 1.0,
                    height: 1.0,
                    curvature: 1.0,
                    layer_spacing: 0.0,
                }
            }
        }

        impl ComponentLifecycle for $name {
            fn required_components() -> &'static [u16] {
                &[
                    crate::ComponentValue::TRANSFORM,
                    crate::ComponentValue::BOUNDING_GEOMETRY,
                ]
            }

            fn validate(&self) -> Result<(), ErrorReason> {
                super::curved_geometry::CurvedSurfaceParameters::new(
                    self.width,
                    self.height,
                    self.curvature,
                    $axes,
                )
                .validate()
                .map_err(|_| ErrorReason::InvalidValue)?;

                if !self.layer_spacing.is_finite() {
                    return Err(ErrorReason::InvalidValue);
                }

                Ok(())
            }
        }
    };
}

curved_component!(
    CylinderSurface,
    super::curved_geometry::CurvatureAxes::Horizontal,
    "Cylindrical segment with horizontal arc-length coordinates and a straight vertical axis."
);
curved_component!(
    SphereSurface,
    super::curved_geometry::CurvatureAxes::Radial,
    "Spherical segment with a centre-based equidistant content chart."
);
