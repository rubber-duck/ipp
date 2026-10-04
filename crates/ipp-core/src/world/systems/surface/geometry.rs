//! Headless geometry shared by presentation, bounds and input.
//!
//! Content coordinates use metres, a top-left origin and +Y down. Offsets
//! follow the local front normal. Geometry owns neither Canvas nor GPU state.

use crate::{
    ErrorReason,
    systems::geometry::{GeometryRay, GeometryShape},
};
use std::{any::Any, fmt::Debug, ops::Deref, sync::Arc};

/// Whether intersections must lie in the authored rectangle or may continue beyond it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceDomain {
    /// Only the authored content rectangle is eligible.
    Content,
    /// Continue the unambiguous mapping beyond the authored rectangle.
    Continuation,
}

/// One sample of the content and normal-offset mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceSample {
    /// Entity-local position in metres.
    pub position: [f64; 3],
    /// Unit entity-local front normal.
    pub front_normal: [f64; 3],
}

/// One inverse-mapped ray intersection, retaining the caller's ray parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceIntersection {
    /// Parameter along the supplied ray, even under nonuniform placement.
    pub distance: f64,
    /// Content coordinates in metres.
    pub content: [f64; 2],
    /// Unit entity-local front normal at the intersection.
    pub front_normal: [f64; 3],
}

/// Build-time extension contract for a completed two-dimensional Surface.
pub trait Surface: Any + Debug + Send + Sync {
    /// Physical content extent in metres.
    fn physical_extent(&self) -> [f64; 2];

    /// Metres separating consecutive occupied content ranks.
    fn layer_spacing(&self) -> f32;

    /// Sample the mapping; finite content outside the rectangle is allowed where unambiguous.
    fn sample(&self, content: [f64; 2], offset: f64) -> Result<SurfaceSample, ErrorReason>;

    /// Invert all ray intersections, sorted by distance. Facing is a consumer policy.
    fn ray_intersections(
        &self,
        ray: &GeometryRay,
        offset: f64,
        domain: SurfaceDomain,
    ) -> Result<Vec<SurfaceIntersection>, ErrorReason>;

    /// Validate the entire occupied offset interval, including shell collapse/inversion.
    fn validate_offsets(&self, offsets: [f64; 2]) -> Result<(), ErrorReason>;

    /// Conservative local enclosure of the entire content rectangle and offset interval.
    fn bounds(&self, offsets: [f64; 2]) -> Result<GeometryShape, ErrorReason>;

    /// Conservative Euclidean deviation in metres from any triangle interpolant
    /// over a rectangular content patch, including its normal offset.
    /// `patch` is [min_x, min_y, max_x, max_y] in metres; the renderer owns subdivision.
    fn approximation_error(&self, patch: [f64; 4], offset: f64) -> Result<f64, ErrorReason>;

    /// Exact content-to-local affine map, when available, in column-major order.
    fn exact_affine(&self, offset: f64) -> Option<[f64; 16]>;

    /// Implementation type for immutable geometry equality.
    fn as_any(&self) -> &dyn Any;

    /// Whether two completed values describe exactly the same geometry.
    fn equivalent(&self, other: &dyn Surface) -> bool;
}

/// Immutable completed geometry with no component pointers or mutable World access.
#[derive(Clone, Debug)]
pub struct SurfaceGeometry {
    component: u16,
    geometry: Arc<dyn Surface>,
}

impl SurfaceGeometry {
    pub(crate) fn new<S: Surface>(component: u16, geometry: S) -> Self {
        Self {
            component,
            geometry: Arc::new(geometry),
        }
    }

    /// Registered component identity supplying this geometry.
    pub fn component(&self) -> u16 {
        self.component
    }
}

impl Deref for SurfaceGeometry {
    type Target = dyn Surface;

    fn deref(&self) -> &Self::Target {
        self.geometry.as_ref()
    }
}

impl PartialEq for SurfaceGeometry {
    fn eq(&self, other: &Self) -> bool {
        self.component == other.component
            && (Arc::ptr_eq(&self.geometry, &other.geometry)
                || self.geometry.equivalent(other.geometry.as_ref()))
    }
}

pub(super) fn validate_offsets(offsets: [f64; 2]) -> Result<(), ErrorReason> {
    if offsets.iter().all(|value| value.is_finite()) && offsets[0] <= offsets[1] {
        Ok(())
    } else {
        Err(ErrorReason::InvalidGeometry)
    }
}

pub(super) fn finite_content(content: [f64; 2]) -> Result<(), ErrorReason> {
    if content.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(ErrorReason::InvalidGeometry)
    }
}

pub(super) fn contains(extent: [f64; 2], content: [f64; 2]) -> bool {
    content
        .iter()
        .enumerate()
        .all(|(axis, value)| *value >= 0.0 && *value <= extent[axis])
}

pub(super) fn validate_ray(ray: &GeometryRay) -> Result<(), ErrorReason> {
    if ray
        .origin
        .iter()
        .chain(&ray.direction)
        .all(|value| value.is_finite())
        && ray.direction.iter().any(|value| *value != 0.0)
    {
        Ok(())
    } else {
        Err(ErrorReason::InvalidGeometry)
    }
}
