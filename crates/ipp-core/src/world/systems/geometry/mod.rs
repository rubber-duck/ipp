//! Shared headless shapes for bounding, picking and their visualization.
//!
//! Rays retain their parameter through affine transforms. GeometryPlane operations use
//! support intervals, including under nonuniform scale and shear. Compounds are
//! unions: ray intersections skip surfaces internal to overlapping parts.

mod components;
pub use components::{BoundingGeometry, PickingGeometry};

mod enclosure;
#[cfg(feature = "particles")]
mod particle_bounds;
mod program;
pub use enclosure::GeometryEnclosure;

mod spatial;
pub use spatial::{
    GeometryPreparedBounds, GeometryQueryResults, GeometryQueryScratch, GeometrySpatialBackend,
    GeometrySpatialIndex,
};
mod update;

pub(crate) use update::GeometryEvaluationState;
pub(in crate::world) use update::{evaluation_mesh_demand, update_evaluation_mesh_demand};

mod shape;
mod transform;

mod definition;

mod visualization;

pub use visualization::GeometryPrimitiveVisual;

pub(crate) use visualization::arrow_shoulder;

pub use shape::{CompoundGeometryShape, GeometryShape, TransformedGeometryShape};
pub use transform::GeometryShapeTransform;

pub use definition::{GEOMETRY_TYPE, GeometryDefinition, GeometryShapePart};

pub(crate) use definition::geometry_asset_loader;

mod picking;

pub(in crate::world) use picking::{GeometryQueryAccess, GeometryQueryCommand};

pub mod queries;

mod system_state;
pub use system_state::GeometrySystemState;

mod system;
pub use system::{GeometrySystem, GeometrySystemFactory};

mod bounds;
pub use bounds::{
    GeometryBounds, GeometryPlane, GeometryPlaneIntersection, GeometryRay, GeometryRayHit,
    GeometryRayInterval, frustum_planes,
};
pub(crate) use bounds::{dot, finite3, scale, subtract};

/// Geometry inputs borrowed from one completed World evaluation for a render submission.
/// References expire before mutation; missing or unproven culling geometry keeps draws visible.
pub struct RenderGeometry<'a> {
    /// The visual mesh's World-space enclosure, distinct from authored culling geometry.
    pub mesh_bounds: Option<[[f64; 3]; 2]>,
    /// A trustworthy evaluated enclosure shared by camera and shadow frustum tests.
    pub culling: Option<&'a CompoundGeometryShape>,
}
