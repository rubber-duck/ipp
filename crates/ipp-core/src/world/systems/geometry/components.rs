use super::{GEOMETRY_TYPE, GeometryDefinition, GeometryEvaluationState};
use crate::{
    EntityId, ErrorReason,
    components::schema::ComponentLifecycle,
    services::asset_management::service::{AssetDemandSelection, validate_source},
};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;

/// Private evaluated storage of one bounding or picking component incarnation.
#[derive(Debug, Default)]
pub struct GeometryRuntimeState {
    // Prepared programs grow independently of authored component/Command layout.
    // This allocation is retained across evaluations and numeric display edits.
    pub(crate) evaluation: Option<Box<GeometryEvaluationState>>,
}

macro_rules! geometry_component {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[repr(C)]
        #[derive(Debug, SchemaComponent)]
        pub struct $name {
            /// Inline IPPG definition; empty selects the source or mesh-derived geometry.
            pub geometry: Vec<u8>,
            /// Immutable geometry source; empty uses inline data or the visual mesh.
            pub source: String,
            /// Immutable geometry variant.
            pub variant: u32,
            /// Skeleton entity for joint-pair parts; zero selects this entity's rig.
            pub skeleton: EntityId,
            /// Draw these evaluated shapes independently of picking participation.
            pub is_rendered: bool,
            /// Draw contours instead of filled primitives.
            pub outline: bool,
            /// Local contour stroke diameter.
            pub stroke: f32,
            /// Use this component's RGB instead of the global geometry color.
            pub has_color_override: bool,
            /// Linear red override in 0..=1.
            pub r: f32,
            /// Linear green override in 0..=1.
            pub g: f32,
            /// Linear blue override in 0..=1.
            pub b: f32,
            /// Evaluated data owned by this component incarnation, absent from the schema.
            #[schema(ignore)]
            pub runtime: GeometryRuntimeState,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    geometry: Vec::new(),
                    source: String::new(),
                    variant: 0,
                    skeleton: EntityId::from_bits(0),
                    is_rendered: false,
                    outline: false,
                    stroke: 0.04,
                    has_color_override: false,
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    runtime: Default::default(),
                }
            }
        }

        impl Clone for $name {
            fn clone(&self) -> Self {
                Self {
                    geometry: self.geometry.clone(),
                    source: self.source.clone(),
                    variant: self.variant,
                    skeleton: self.skeleton,
                    is_rendered: self.is_rendered,
                    outline: self.outline,
                    stroke: self.stroke,
                    has_color_override: self.has_color_override,
                    r: self.r,
                    g: self.g,
                    b: self.b,
                    runtime: Default::default(),
                }
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.geometry == other.geometry
                    && self.source == other.source
                    && self.variant == other.variant
                    && self.skeleton == other.skeleton
                    && self.is_rendered == other.is_rendered
                    && self.outline == other.outline
                    && self.stroke == other.stroke
                    && self.has_color_override == other.has_color_override
                    && self.r == other.r
                    && self.g == other.g
                    && self.b == other.b
            }
        }

        impl ComponentLifecycle for $name {
            fn preserve_runtime(&mut self, previous: &mut Self) {
                if self.geometry == previous.geometry
                    && self.source == previous.source
                    && self.variant == previous.variant
                    && self.skeleton == previous.skeleton
                {
                    self.runtime = std::mem::take(&mut previous.runtime);
                }
            }

            fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
                &[crate::components::schema::ComponentAssetReference {
                    kind: 6,
                    source_offset: std::mem::offset_of!(Self, source) as u32,
                    variant_offset: std::mem::offset_of!(Self, variant) as u32,
                }]
            }

            fn accepts_null_entity(offset: u32) -> bool {
                offset == std::mem::offset_of!(Self, skeleton) as u32
            }

            fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
                if offset == std::mem::offset_of!(Self, source) as u32 {
                    validate_source(&self.source)?;
                }
                if offset == std::mem::offset_of!(Self, geometry) as u32
                    && !self.geometry.is_empty()
                {
                    GeometryDefinition::decode(&self.geometry)?;
                }
                Ok(())
            }

            fn validate(&self) -> Result<(), ErrorReason> {
                validate_source(&self.source)?;
                if !self.geometry.is_empty() {
                    GeometryDefinition::decode(&self.geometry)?;
                    if !self.source.is_empty() {
                        return Err(ErrorReason::InvalidGeometry);
                    }
                }
                if !self.stroke.is_finite()
                    || self.stroke <= 0.0
                    || [self.r, self.g, self.b]
                        .iter()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                {
                    return Err(ErrorReason::InvalidValue);
                }
                Ok(())
            }

            fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
                if !self.source.is_empty() {
                    demand.insert(AssetDemandSelection::new(
                        GEOMETRY_TYPE,
                        &self.source,
                        self.variant,
                    ));
                }
            }
        }
    };
}

geometry_component!(
    BoundingGeometry,
    "Conservative enclosure for culling, using shared primitive and skeletal geometry."
);

geometry_component!(
    PickingGeometry,
    "Explicit picking shapes; component presence enables geometry picking."
);
