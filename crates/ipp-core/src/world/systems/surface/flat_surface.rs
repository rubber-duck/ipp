use super::{FlatSurface, Surface, SurfaceDomain, SurfaceIntersection, SurfaceSample, geometry};
use crate::{
    ErrorReason,
    systems::geometry::{GeometryRay, GeometryShape},
};

impl Surface for FlatSurface {
    fn physical_extent(&self) -> [f64; 2] {
        [f64::from(self.width), f64::from(self.height)]
    }

    fn layer_spacing(&self) -> f32 {
        self.layer_spacing
    }

    fn sample(&self, content: [f64; 2], offset: f64) -> Result<SurfaceSample, ErrorReason> {
        geometry::finite_content(content)?;
        self.validate_offsets([offset, offset])?;
        let extent = self.physical_extent();
        Ok(SurfaceSample {
            position: [
                content[0] - extent[0] * 0.5,
                extent[1] * 0.5 - content[1],
                offset,
            ],
            front_normal: [0.0, 0.0, 1.0],
        })
    }

    fn ray_intersections(
        &self,
        ray: &GeometryRay,
        offset: f64,
        domain: SurfaceDomain,
    ) -> Result<Vec<SurfaceIntersection>, ErrorReason> {
        geometry::validate_ray(ray)?;
        self.validate_offsets([offset, offset])?;
        if ray.direction[2] == 0.0 {
            return Ok(Vec::new());
        }
        let distance = (offset - ray.origin[2]) / ray.direction[2];
        let extent = self.physical_extent();
        let content = [
            ray.origin[0] + distance * ray.direction[0] + extent[0] * 0.5,
            extent[1] * 0.5 - ray.origin[1] - distance * ray.direction[1],
        ];
        if !distance.is_finite()
            || !content.iter().all(|value| value.is_finite())
            || (domain == SurfaceDomain::Content && !geometry::contains(extent, content))
        {
            return Ok(Vec::new());
        }
        Ok(vec![SurfaceIntersection {
            distance,
            content,
            front_normal: [0.0, 0.0, 1.0],
        }])
    }

    fn validate_offsets(&self, offsets: [f64; 2]) -> Result<(), ErrorReason> {
        geometry::validate_offsets(offsets)
    }

    fn bounds(&self, offsets: [f64; 2]) -> Result<GeometryShape, ErrorReason> {
        self.validate_offsets(offsets)?;
        let extent = self.physical_extent();
        Ok(GeometryShape::Box {
            min: [-extent[0] * 0.5, -extent[1] * 0.5, offsets[0]],
            max: [extent[0] * 0.5, extent[1] * 0.5, offsets[1]],
        })
    }

    fn approximation_error(&self, patch: [f64; 4], offset: f64) -> Result<f64, ErrorReason> {
        geometry::finite_content([patch[0], patch[1]])?;
        geometry::finite_content([patch[2], patch[3]])?;
        if patch[0] > patch[2] || patch[1] > patch[3] {
            return Err(ErrorReason::InvalidGeometry);
        }
        self.validate_offsets([offset, offset])?;
        Ok(0.0)
    }

    fn exact_affine(&self, offset: f64) -> Option<[f64; 16]> {
        self.validate_offsets([offset, offset]).ok()?;
        let extent = self.physical_extent();
        Some([
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            -extent[0] * 0.5,
            extent[1] * 0.5,
            offset,
            1.0,
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn equivalent(&self, other: &dyn Surface) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
}
