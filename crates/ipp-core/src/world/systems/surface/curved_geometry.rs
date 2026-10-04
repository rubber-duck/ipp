//! Shared stable curved charts. Centred content is x=u-width/2,y=height/2-v.
//! Cylinder uses a=k*x; sphere uses a=k*hypot(x,y). The base point is
//! [x*sinc(a), y (cylinder) or y*sinc(a), (cos(a)-1)/k]. Front normals are
//! [sin(a),0,cos(a)] or [k*x*sinc(a),k*y*sinc(a),cos(a)]. Normal offsets
//! multiply the curved radius by 1+k*d, which must stay strictly positive.
//! Principal charts exclude |a|>=pi, preventing seam/antipode ambiguity.

use super::{FlatSurface, Surface, SurfaceDomain, SurfaceIntersection, SurfaceSample, geometry};
use crate::{
    ErrorReason,
    systems::geometry::{GeometryRay, GeometryShape},
};

#[derive(Clone, Copy)]
pub(super) enum CurvatureAxes {
    Horizontal,
    Radial,
}

pub(super) struct CurvedSurfaceParameters {
    extent: [f64; 2],
    curvature: f64,
    axes: CurvatureAxes,
}

impl CurvedSurfaceParameters {
    pub fn new(width: f32, height: f32, curvature: f32, axes: CurvatureAxes) -> Self {
        Self {
            extent: [f64::from(width), f64::from(height)],
            curvature: f64::from(curvature),
            axes,
        }
    }

    pub fn validate(&self) -> Result<(), ErrorReason> {
        let radius = self.chart_radius();
        if self
            .extent
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
            && self.curvature.is_finite()
            && self.curvature.abs() * radius < std::f64::consts::PI
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidGeometry)
        }
    }

    fn chart_radius(&self) -> f64 {
        match self.axes {
            CurvatureAxes::Horizontal => self.extent[0] * 0.5,
            CurvatureAxes::Radial => self.extent[0].hypot(self.extent[1]) * 0.5,
        }
    }

    fn flat(&self) -> FlatSurface {
        FlatSurface {
            width: self.extent[0] as f32,
            height: self.extent[1] as f32,
            layer_spacing: 0.0,
        }
    }

    pub fn validate_offsets(&self, offsets: [f64; 2]) -> Result<(), ErrorReason> {
        self.validate()?;
        geometry::validate_offsets(offsets)?;

        if offsets.iter().all(|offset| {
            let factor = 1.0 + self.curvature * offset;
            factor.is_finite() && factor > 0.0
        }) {
            Ok(())
        } else {
            Err(ErrorReason::InvalidGeometry)
        }
    }

    pub fn sample(&self, content: [f64; 2], offset: f64) -> Result<SurfaceSample, ErrorReason> {
        self.validate_offsets([offset, offset])?;
        geometry::finite_content(content)?;

        if self.curvature == 0.0 {
            return self.flat().sample(content, offset);
        }

        let x = content[0] - self.extent[0] * 0.5;
        let y = self.extent[1] * 0.5 - content[1];
        let radius = match self.axes {
            CurvatureAxes::Horizontal => x.abs(),
            CurvatureAxes::Radial => x.hypot(y),
        };
        let angle = self.curvature
            * match self.axes {
                CurvatureAxes::Horizontal => x,
                CurvatureAxes::Radial => radius,
            };
        if angle.abs() >= std::f64::consts::PI {
            return Err(ErrorReason::InvalidGeometry);
        }

        let sinc = sinc(angle);
        let normal = [
            self.curvature * x * sinc,
            match self.axes {
                CurvatureAxes::Horizontal => 0.0,
                CurvatureAxes::Radial => self.curvature * y * sinc,
            },
            angle.cos(),
        ];

        // Half-angle evaluation preserves the continuous limit for tiny k.
        let z = -2.0 * (angle * 0.5).sin().powi(2) / self.curvature;
        Ok(SurfaceSample {
            position: [
                x * sinc + offset * normal[0],
                match self.axes {
                    CurvatureAxes::Horizontal => y,
                    CurvatureAxes::Radial => y * sinc + offset * normal[1],
                },
                z + offset * normal[2],
            ],
            front_normal: normal,
        })
    }

    pub fn ray_intersections(
        &self,
        ray: &GeometryRay,
        offset: f64,
        domain: SurfaceDomain,
    ) -> Result<Vec<SurfaceIntersection>, ErrorReason> {
        self.validate_offsets([offset, offset])?;
        geometry::validate_ray(ray)?;

        if self.curvature == 0.0 {
            return self.flat().ray_intersections(ray, offset, domain);
        }

        let k = self.curvature;
        let axes: &[usize] = match self.axes {
            CurvatureAxes::Horizontal => &[0, 2],
            CurvatureAxes::Radial => &[0, 1, 2],
        };

        // k*(x²[+y²]+z²-d²)+2*(z-d)=0 avoids giant centre subtraction.
        let a = k * axes
            .iter()
            .map(|&axis| ray.direction[axis].powi(2))
            .sum::<f64>();
        let b = 2.0
            * (k * axes
                .iter()
                .map(|&axis| ray.origin[axis] * ray.direction[axis])
                .sum::<f64>()
                + ray.direction[2]);
        let c = k
            * (axes
                .iter()
                .map(|&axis| ray.origin[axis].powi(2))
                .sum::<f64>()
                - offset.powi(2))
            + 2.0 * (ray.origin[2] - offset);

        let factor = 1.0 + k * offset;
        let mut intersections = Vec::new();
        for distance in roots(a, b, c)? {
            let point: [f64; 3] =
                std::array::from_fn(|axis| ray.origin[axis] + distance * ray.direction[axis]);
            let normal = [
                k * point[0] / factor,
                match self.axes {
                    CurvatureAxes::Horizontal => 0.0,
                    CurvatureAxes::Radial => k * point[1] / factor,
                },
                (1.0 + k * point[2]) / factor,
            ];

            let centred = match self.axes {
                CurvatureAxes::Horizontal => {
                    let angle = (k * point[0]).atan2(1.0 + k * point[2]);
                    if angle.abs() >= std::f64::consts::PI {
                        continue;
                    }
                    [angle / k, point[1]]
                }
                CurvatureAxes::Radial => {
                    let radial = (k * point[0]).hypot(k * point[1]);
                    let angle = radial.atan2(1.0 + k * point[2]);
                    if angle >= std::f64::consts::PI {
                        continue;
                    }
                    let scale = if radial == 0.0 {
                        1.0 / factor
                    } else {
                        angle / radial
                    };
                    [point[0] * scale, point[1] * scale]
                }
            };
            let content = [
                centred[0] + self.extent[0] * 0.5,
                self.extent[1] * 0.5 - centred[1],
            ];
            if !content.iter().all(|value| value.is_finite())
                || (domain == SurfaceDomain::Content && !geometry::contains(self.extent, content))
            {
                continue;
            }
            let length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
            if !length.is_finite() || length == 0.0 {
                continue;
            }

            intersections.push(SurfaceIntersection {
                distance,
                content,
                front_normal: normal.map(|value| value / length),
            });
        }

        Ok(intersections)
    }

    pub fn bounds(&self, offsets: [f64; 2]) -> Result<GeometryShape, ErrorReason> {
        self.validate_offsets(offsets)?;
        if self.curvature == 0.0 {
            return self.flat().bounds(offsets);
        }

        let angle = self.curvature.abs() * self.chart_radius();
        let factor = offsets
            .map(|offset| 1.0 + self.curvature * offset)
            .into_iter()
            .fold(0.0_f64, f64::max);
        let horizontal =
            (angle.min(std::f64::consts::FRAC_PI_2).sin() / self.curvature.abs()) * factor;
        let half = match self.axes {
            CurvatureAxes::Horizontal => [horizontal, self.extent[1] * 0.5],
            CurvatureAxes::Radial => [
                horizontal.min(self.extent[0] * 0.5 * factor),
                horizontal.min(self.extent[1] * 0.5 * factor),
            ],
        };

        let base_z = -2.0 * (angle * 0.5).sin().powi(2) / self.curvature;
        let z = offsets
            .into_iter()
            .flat_map(|offset| [offset, base_z + offset * angle.cos()]);
        let (min_z, max_z) = z.fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), value| {
            (min.min(value), max.max(value))
        });

        Ok(GeometryShape::Box {
            min: [-half[0], -half[1], min_z],
            max: [half[0], half[1], max_z],
        })
    }

    pub fn approximation_error(&self, patch: [f64; 4], offset: f64) -> Result<f64, ErrorReason> {
        self.validate_offsets([offset, offset])?;
        for content in [
            [patch[0], patch[1]],
            [patch[2], patch[1]],
            [patch[0], patch[3]],
            [patch[2], patch[3]],
        ] {
            self.sample(content, offset)?;
        }
        if patch[0] > patch[2] || patch[1] > patch[3] {
            return Err(ErrorReason::InvalidGeometry);
        }

        let dx = patch[2] - patch[0];
        let dy = patch[3] - patch[1];
        let diameter_squared = match self.axes {
            CurvatureAxes::Horizontal => dx * dx,
            CurvatureAxes::Radial => dx * dx + dy * dy,
        };

        // Cylinder ||D²p[h,h]||<=|k|(1+kd)|h|². For sphere, the
        // integral sinc(a)=integral_0^1 cos(t*a)dt bounds the XY Hessian
        // by (2+2*pi)/3*|k| and Z by |k| on the principal chart. Eight
        // bounds their combined norm. Taylor's remainder bounds every
        // convex triangle interpolant by half this Hessian times diameter².
        let hessian = match self.axes {
            CurvatureAxes::Horizontal => 1.0,
            CurvatureAxes::Radial => 8.0,
        } * self.curvature.abs()
            * (1.0 + self.curvature * offset);
        Ok(0.5 * hessian * diameter_squared)
    }

    pub fn exact_affine(&self, offset: f64) -> Option<[f64; 16]> {
        self.validate_offsets([offset, offset]).ok()?;
        (self.curvature == 0.0)
            .then(|| self.flat().exact_affine(offset))
            .flatten()
    }
}

fn sinc(angle: f64) -> f64 {
    if angle.abs() < 1e-4 {
        let square = angle * angle;
        1.0 - square / 6.0 + square * square / 120.0
    } else {
        angle.sin() / angle
    }
}

fn roots(a: f64, b: f64, c: f64) -> Result<Vec<f64>, ErrorReason> {
    if ![a, b, c].iter().all(|value| value.is_finite()) {
        return Err(ErrorReason::InvalidGeometry);
    }
    if a == 0.0 {
        return Ok(if b == 0.0 {
            Vec::new()
        } else {
            vec![-c / b]
        });
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return Ok(Vec::new());
    }
    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
    let mut values = if q == 0.0 {
        vec![-b / (2.0 * a)]
    } else {
        vec![q / a, c / q]
    };
    values.retain(|value| value.is_finite());
    values.sort_by(f64::total_cmp);
    values.dedup();
    Ok(values)
}

macro_rules! surface_geometry {
    ($name:ident, $axes:expr) => {
        impl Surface for super::$name {
            fn physical_extent(&self) -> [f64; 2] {
                [f64::from(self.width), f64::from(self.height)]
            }

            fn layer_spacing(&self) -> f32 {
                self.layer_spacing
            }

            fn sample(&self, content: [f64; 2], offset: f64) -> Result<SurfaceSample, ErrorReason> {
                CurvedSurfaceParameters::new(self.width, self.height, self.curvature, $axes)
                    .sample(content, offset)
            }

            fn ray_intersections(
                &self,
                ray: &GeometryRay,
                offset: f64,
                domain: SurfaceDomain,
            ) -> Result<Vec<SurfaceIntersection>, ErrorReason> {
                CurvedSurfaceParameters::new(self.width, self.height, self.curvature, $axes)
                    .ray_intersections(ray, offset, domain)
            }

            fn validate_offsets(&self, offsets: [f64; 2]) -> Result<(), ErrorReason> {
                CurvedSurfaceParameters::new(self.width, self.height, self.curvature, $axes)
                    .validate_offsets(offsets)
            }

            fn bounds(&self, offsets: [f64; 2]) -> Result<GeometryShape, ErrorReason> {
                CurvedSurfaceParameters::new(self.width, self.height, self.curvature, $axes)
                    .bounds(offsets)
            }

            fn approximation_error(
                &self,
                patch: [f64; 4],
                offset: f64,
            ) -> Result<f64, ErrorReason> {
                CurvedSurfaceParameters::new(self.width, self.height, self.curvature, $axes)
                    .approximation_error(patch, offset)
            }

            fn exact_affine(&self, offset: f64) -> Option<[f64; 16]> {
                CurvedSurfaceParameters::new(self.width, self.height, self.curvature, $axes)
                    .exact_affine(offset)
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn equivalent(&self, other: &dyn Surface) -> bool {
                other.as_any().downcast_ref::<Self>() == Some(self)
            }
        }
    };
}

surface_geometry!(CylinderSurface, CurvatureAxes::Horizontal);
surface_geometry!(SphereSurface, CurvatureAxes::Radial);
